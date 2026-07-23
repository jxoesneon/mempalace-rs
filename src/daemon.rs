//! Daemon / background worker for Mempalace.
//!
//! A minimal, in-process background worker that persists a PID file under the
//! Mempalace config directory and runs a periodic maintenance loop. This is a
//! Rust port of the long-running daemon concept from upstream `mempalace.daemon`,
//! stripped down to the essentials: start, stop, status, restart, and a periodic
//! heartbeat / maintenance task.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::fs;
use tokio::io::AsyncWriteExt;
use tokio::sync::Notify;
use tokio::task::JoinHandle;
use tokio::time::{interval, MissedTickBehavior};
use tracing;

const DEFAULT_INTERVAL_SECONDS: u64 = 60;
const PID_FILE_NAME: &str = "pid";

/// Synchronous maintenance task that is invoked on every tick of the daemon loop.
pub type MaintenanceTask = Arc<dyn Fn() -> Result<()> + Send + Sync>;

/// Configuration for a [`Daemon`].
pub struct DaemonConfig {
    /// Path to the palace that this daemon serves.
    pub palace_path: PathBuf,
    /// Mempalace config directory (PID/state files are written underneath).
    pub config_dir: PathBuf,
    /// Interval between maintenance/heartbeat ticks.
    pub interval: Duration,
    /// Optional maintenance task to run each tick.
    pub maintenance: Option<MaintenanceTask>,
}

impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            palace_path: PathBuf::from("~/.mempalace/palace"),
            config_dir: default_config_dir(),
            interval: Duration::from_secs(DEFAULT_INTERVAL_SECONDS),
            maintenance: None,
        }
    }
}

impl DaemonConfig {
    /// Create a new daemon config for the given palace and config directories.
    pub fn new(palace_path: impl AsRef<Path>, config_dir: impl AsRef<Path>) -> Self {
        Self {
            palace_path: palace_path.as_ref().to_path_buf(),
            config_dir: config_dir.as_ref().to_path_buf(),
            interval: Duration::from_secs(DEFAULT_INTERVAL_SECONDS),
            maintenance: None,
        }
    }

    /// Set the tick interval.
    pub fn with_interval(mut self, interval: Duration) -> Self {
        self.interval = interval;
        self
    }

    /// Set the periodic maintenance task.
    pub fn with_maintenance(mut self, task: MaintenanceTask) -> Self {
        self.maintenance = Some(task);
        self
    }
}

/// Snapshot of daemon state returned by [`Daemon::status`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DaemonStatus {
    /// Whether the daemon background task is currently running.
    pub running: bool,
    /// PID read from the PID file, if present.
    pub pid: Option<u32>,
    /// Canonical palace path served by the daemon.
    pub palace_path: String,
    /// Configured tick interval in seconds.
    pub interval_seconds: u64,
}

/// A single pending daemon job.
///
/// This is a placeholder for the upstream job queue. It is intentionally
/// minimal so the CLI can expose `daemon jobs` without blocking the build.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DaemonJob {
    pub id: String,
    pub kind: String,
    pub status: String,
}

/// In-process background worker for Mempalace.
///
/// `Daemon` manages a PID file under `config_dir/daemon/<palace_key>/pid` and
/// spawns a tokio background task that ticks at a configured interval. Each tick
/// increments a heartbeat counter and optionally runs a caller-supplied
/// maintenance task on the blocking thread pool so that slow sync/prune work
/// does not starve the async runtime.
pub struct Daemon {
    palace_path: PathBuf,
    state_dir: PathBuf,
    pid_path: PathBuf,
    interval: Duration,
    maintenance: Option<MaintenanceTask>,
    shutdown: Arc<Notify>,
    handle: Option<JoinHandle<()>>,
    heartbeat: Arc<AtomicU64>,
}

impl Daemon {
    /// Build a new daemon from the supplied configuration.
    pub fn new(config: DaemonConfig) -> Self {
        let state_dir = config
            .config_dir
            .join("daemon")
            .join(palace_key(&config.palace_path));
        let pid_path = state_dir.join(PID_FILE_NAME);
        Self {
            palace_path: config.palace_path,
            state_dir,
            pid_path,
            interval: config.interval,
            maintenance: config.maintenance,
            shutdown: Arc::new(Notify::new()),
            handle: None,
            heartbeat: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Number of heartbeat ticks that have elapsed since the daemon started.
    pub fn heartbeat_count(&self) -> u64 {
        self.heartbeat.load(Ordering::SeqCst)
    }

    /// Start the daemon background loop.
    ///
    /// Idempotent: if the daemon is already running, this returns `Ok(())`
    /// without spawning a second task.
    pub async fn start(&mut self) -> Result<()> {
        if self.handle.as_ref().is_some_and(|h| !h.is_finished()) {
            tracing::warn!("daemon already running for {}", self.palace_path.display());
            return Ok(());
        }

        let pid = std::process::id();
        write_pid_file(&self.pid_path, pid).await?;
        self.heartbeat.store(0, Ordering::SeqCst);

        let shutdown = Arc::clone(&self.shutdown);
        let interval = self.interval;
        let maintenance = self.maintenance.clone();
        let heartbeat = Arc::clone(&self.heartbeat);
        let handle = tokio::spawn(run_loop(shutdown, interval, maintenance, heartbeat));
        self.handle = Some(handle);

        tracing::info!("daemon started (pid {}) for {}", pid, self.palace_path.display());
        Ok(())
    }

    /// Stop the daemon background loop and remove the PID file.
    ///
    /// Returns `Ok(())` if the daemon is already stopped.
    pub async fn stop(&mut self) -> Result<()> {
        if let Some(handle) = self.handle.take() {
            self.shutdown.notify_one();
            let timeout = Duration::from_secs(5);
            match tokio::time::timeout(timeout, handle).await {
                Ok(Ok(())) => {}
                Ok(Err(e)) => tracing::error!("daemon background task failed: {}", e),
                Err(_) => tracing::warn!(
                    "daemon background task did not stop within {:?}, detaching",
                    timeout
                ),
            }
        }
        remove_pid_file(&self.pid_path).await?;
        tracing::info!("daemon stopped for {}", self.palace_path.display());
        Ok(())
    }

    /// Restart the daemon: stop then start.
    pub async fn restart(&mut self) -> Result<()> {
        self.stop().await?;
        self.start().await
    }

    /// Return the current daemon status.
    pub async fn status(&self) -> Result<DaemonStatus> {
        let pid = read_pid_file(&self.pid_path).await?;
        let running = self.handle.as_ref().is_some_and(|h| !h.is_finished());
        Ok(DaemonStatus {
            running,
            pid,
            palace_path: self.palace_path.to_string_lossy().to_string(),
            interval_seconds: self.interval.as_secs(),
        })
    }

    /// Return the list of pending daemon jobs.
    ///
    /// The full job queue is not yet implemented; this returns an empty list
    /// so the CLI can expose the upstream `daemon jobs` command.
    pub async fn list_jobs(&self) -> Result<Vec<DaemonJob>> {
        Ok(Vec::new())
    }

    /// Wait until the daemon is running, up to the supplied timeout.
    ///
    /// This is a thin wrapper around [`Daemon::status`] that polls until the
    /// background task is active or the timeout elapses.
    pub async fn wait(&self, timeout: Duration) -> Result<DaemonStatus> {
        let start = std::time::Instant::now();
        let poll = Duration::from_millis(50);
        loop {
            let status = self.status().await?;
            if status.running {
                return Ok(status);
            }
            if start.elapsed() >= timeout {
                anyhow::bail!("daemon did not become running within {:?}", timeout);
            }
            tokio::time::sleep(poll).await;
        }
    }

    /// Path to the palace served by this daemon.
    pub fn palace_path(&self) -> &Path {
        &self.palace_path
    }

    /// Path to the PID file.
    pub fn pid_path(&self) -> &Path {
        &self.pid_path
    }

    /// Directory used for daemon state files.
    pub fn state_dir(&self) -> &Path {
        &self.state_dir
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        if self.handle.is_some() {
            self.shutdown.notify_one();
            let _ = std::fs::remove_file(&self.pid_path);
        }
    }
}

async fn run_loop(
    shutdown: Arc<Notify>,
    interval_duration: Duration,
    maintenance: Option<MaintenanceTask>,
    heartbeat: Arc<AtomicU64>,
) {
    let mut ticker = interval(interval_duration);
    ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
    // Skip the immediate first tick so the first maintenance run is after `interval_duration`.
    ticker.tick().await;

    loop {
        tokio::select! {
            _ = shutdown.notified() => break,
            _ = ticker.tick() => {
                heartbeat.fetch_add(1, Ordering::SeqCst);
                if let Some(task) = maintenance.as_ref() {
                    let task = Arc::clone(task);
                    match tokio::task::spawn_blocking(move || task()).await {
                        Ok(Ok(())) => {}
                        Ok(Err(e)) => tracing::error!("daemon maintenance task failed: {}", e),
                        Err(e) => tracing::error!("daemon maintenance task panicked: {}", e),
                    }
                }
            }
        }
    }
}

async fn write_pid_file(path: &Path, pid: u32) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .await
            .with_context(|| format!("failed to create daemon state dir: {}", parent.display()))?;
    }
    let temp_path = path.with_extension("tmp");
    let mut file = fs::File::create(&temp_path)
        .await
        .with_context(|| format!("failed to create pid temp file: {}", temp_path.display()))?;
    file.write_all(pid.to_string().as_bytes())
        .await
        .with_context(|| "failed to write pid")?;
    file.flush()
        .await
        .with_context(|| "failed to flush pid file")?;
    drop(file);
    fs::rename(&temp_path, path)
        .await
        .with_context(|| format!("failed to rename pid file to {}", path.display()))?;
    Ok(())
}

async fn read_pid_file(path: &Path) -> Result<Option<u32>> {
    if !path.exists() {
        return Ok(None);
    }
    let content = fs::read_to_string(path)
        .await
        .with_context(|| format!("failed to read pid file: {}", path.display()))?;
    let pid = content
        .trim()
        .parse::<u32>()
        .with_context(|| format!("invalid pid file contents: {}", content))?;
    Ok(Some(pid))
}

async fn remove_pid_file(path: &Path) -> Result<()> {
    if path.exists() {
        fs::remove_file(path)
            .await
            .with_context(|| format!("failed to remove pid file: {}", path.display()))?;
    }
    Ok(())
}

/// Return the default Mempalace config directory (`~/.mempalace` on Unix,
/// `%USERPROFILE%\.mempalace` on Windows, or `.` if neither is set).
pub fn default_config_dir() -> PathBuf {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join(".mempalace")
}

/// Stable key derived from the palace path, used to partition daemon state.
fn palace_key(palace_path: &Path) -> String {
    let normalized = palace_path.to_string_lossy().to_lowercase();
    let mut hasher = Sha256::new();
    hasher.update(normalized.as_bytes());
    let result = hasher.finalize();
    hex::encode(&result[..12])
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicU64;
    use tempfile::tempdir;
    use tokio::time::{sleep, Duration};

    #[test]
    fn test_palace_key_is_stable_and_short() {
        let path = Path::new("/home/user/.mempalace/palace");
        let key1 = palace_key(path);
        let key2 = palace_key(path);
        assert_eq!(key1, key2);
        assert_eq!(key1.len(), 24);
        assert!(key1.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_palace_key_is_case_insensitive() {
        let lower = palace_key(Path::new("/home/user/.mempalace/palace"));
        let upper = palace_key(Path::new("/HOME/USER/.MEMPALACE/PALACE"));
        assert_eq!(lower, upper);
    }

    #[test]
    fn test_default_config_dir_contains_mempalace() {
        let dir = default_config_dir();
        let s = dir.to_string_lossy();
        assert!(s.contains(".mempalace"), "expected .mempalace in {}", s);
    }

    #[test]
    fn test_daemon_config_builder() {
        let config = DaemonConfig::new(PathBuf::from("/palace"), PathBuf::from("/config"))
            .with_interval(Duration::from_secs(30))
            .with_maintenance(Arc::new(|| Ok(())));
        assert_eq!(config.palace_path, PathBuf::from("/palace"));
        assert_eq!(config.config_dir, PathBuf::from("/config"));
        assert_eq!(config.interval, Duration::from_secs(30));
        assert!(config.maintenance.is_some());
    }

    #[tokio::test]
    async fn test_pid_file_read_write_and_remove() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("pid");

        write_pid_file(&path, 42).await.unwrap();
        assert!(path.exists());
        assert_eq!(read_pid_file(&path).await.unwrap(), Some(42));

        remove_pid_file(&path).await.unwrap();
        assert!(!path.exists());
        assert_eq!(read_pid_file(&path).await.unwrap(), None);
    }

    #[tokio::test]
    async fn test_daemon_start_stop_removes_pid_file() {
        let dir = tempdir().unwrap();
        let config = DaemonConfig::new(dir.path().join("palace"), dir.path().join("mempalace"))
            .with_interval(Duration::from_millis(10));
        let mut daemon = Daemon::new(config);

        daemon.start().await.unwrap();
        assert!(daemon.pid_path().exists());
        let status = daemon.status().await.unwrap();
        assert!(status.running);
        assert_eq!(status.pid, Some(std::process::id()));

        daemon.stop().await.unwrap();
        assert!(!daemon.pid_path().exists());
        let status = daemon.status().await.unwrap();
        assert!(!status.running);
        assert_eq!(status.pid, None);
    }

    #[tokio::test]
    async fn test_daemon_heartbeat_counts_ticks() {
        let dir = tempdir().unwrap();
        let config = DaemonConfig::new(dir.path().join("palace"), dir.path().join("mempalace"))
            .with_interval(Duration::from_millis(10));
        let mut daemon = Daemon::new(config);

        daemon.start().await.unwrap();
        sleep(Duration::from_millis(120)).await;
        daemon.stop().await.unwrap();

        let count = daemon.heartbeat_count();
        assert!(count >= 2, "expected at least 2 heartbeats, got {}", count);
    }

    #[tokio::test]
    async fn test_daemon_maintenance_task_runs() {
        let dir = tempdir().unwrap();
        let counter = Arc::new(AtomicU64::new(0));
        let counter_clone = Arc::clone(&counter);
        let config = DaemonConfig::new(dir.path().join("palace"), dir.path().join("mempalace"))
            .with_interval(Duration::from_millis(10))
            .with_maintenance(Arc::new(move || {
                counter_clone.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }));
        let mut daemon = Daemon::new(config);

        daemon.start().await.unwrap();
        sleep(Duration::from_millis(80)).await;
        daemon.stop().await.unwrap();

        let count = counter.load(Ordering::SeqCst);
        assert!(
            count >= 1,
            "expected maintenance to run at least once, got {}",
            count
        );
    }

    #[tokio::test]
    async fn test_daemon_maintenance_failure_is_non_fatal() {
        let dir = tempdir().unwrap();
        let counter = Arc::new(AtomicU64::new(0));
        let counter_clone = Arc::clone(&counter);
        let config = DaemonConfig::new(dir.path().join("palace"), dir.path().join("mempalace"))
            .with_interval(Duration::from_millis(10))
            .with_maintenance(Arc::new(move || {
                counter_clone.fetch_add(1, Ordering::SeqCst);
                Err(anyhow::anyhow!("maintenance failure"))
            }));
        let mut daemon = Daemon::new(config);

        daemon.start().await.unwrap();
        sleep(Duration::from_millis(80)).await;
        let status = daemon.status().await.unwrap();
        assert!(status.running);
        daemon.stop().await.unwrap();

        let count = counter.load(Ordering::SeqCst);
        assert!(count >= 1, "expected maintenance attempts, got {}", count);
    }

    #[tokio::test]
    async fn test_daemon_double_start_is_idempotent() {
        let dir = tempdir().unwrap();
        let config = DaemonConfig::new(dir.path().join("palace"), dir.path().join("mempalace"))
            .with_interval(Duration::from_millis(10));
        let mut daemon = Daemon::new(config);

        daemon.start().await.unwrap();
        daemon.start().await.unwrap(); // should not spawn a second task

        let status = daemon.status().await.unwrap();
        assert!(status.running);
        daemon.stop().await.unwrap();
    }

    #[tokio::test]
    async fn test_daemon_stop_when_not_running_is_ok() {
        let dir = tempdir().unwrap();
        let config = DaemonConfig::new(dir.path().join("palace"), dir.path().join("mempalace"));
        let mut daemon = Daemon::new(config);

        daemon.stop().await.unwrap();
        let status = daemon.status().await.unwrap();
        assert!(!status.running);
        assert_eq!(status.pid, None);
    }

    #[tokio::test]
    async fn test_daemon_restart() {
        let dir = tempdir().unwrap();
        let config = DaemonConfig::new(dir.path().join("palace"), dir.path().join("mempalace"))
            .with_interval(Duration::from_millis(10));
        let mut daemon = Daemon::new(config);

        daemon.start().await.unwrap();
        let before = daemon.status().await.unwrap();
        assert!(before.running);

        daemon.restart().await.unwrap();
        let after = daemon.status().await.unwrap();
        assert!(after.running);
        assert!(daemon.pid_path().exists());

        daemon.stop().await.unwrap();
    }

    #[tokio::test]
    async fn test_daemon_drop_notifies_shutdown_and_removes_pid() {
        let dir = tempdir().unwrap();
        let config = DaemonConfig::new(dir.path().join("palace"), dir.path().join("mempalace"))
            .with_interval(Duration::from_millis(10));
        let mut daemon = Daemon::new(config);

        daemon.start().await.unwrap();
        let pid_path = daemon.pid_path().to_path_buf();
        assert!(pid_path.exists());

        drop(daemon);
        assert!(
            !pid_path.exists(),
            "pid file should be removed when daemon is dropped"
        );
    }

    #[tokio::test]
    async fn test_daemon_status_json_roundtrip() {
        let status = DaemonStatus {
            running: true,
            pid: Some(1234),
            palace_path: "/palace".into(),
            interval_seconds: 30,
        };
        let json = serde_json::to_string(&status).unwrap();
        let back: DaemonStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(status, back);
    }

    #[tokio::test]
    async fn test_daemon_list_jobs_empty() {
        let dir = tempdir().unwrap();
        let config = DaemonConfig::new(dir.path().join("palace"), dir.path().join("mempalace"));
        let daemon = Daemon::new(config);
        let jobs = daemon.list_jobs().await.unwrap();
        assert!(jobs.is_empty());
    }

    #[tokio::test]
    async fn test_daemon_wait_becomes_running() {
        let dir = tempdir().unwrap();
        let config = DaemonConfig::new(dir.path().join("palace"), dir.path().join("mempalace"))
            .with_interval(Duration::from_millis(10));
        let mut daemon = Daemon::new(config);

        daemon.start().await.unwrap();
        let status = daemon.wait(Duration::from_secs(1)).await.unwrap();
        assert!(status.running);
        daemon.stop().await.unwrap();
    }

    #[tokio::test]
    async fn test_daemon_wait_times_out_when_not_started() {
        let dir = tempdir().unwrap();
        let config = DaemonConfig::new(dir.path().join("palace"), dir.path().join("mempalace"));
        let daemon = Daemon::new(config);
        assert!(daemon.wait(Duration::from_millis(50)).await.is_err());
    }
}
