//! Background service lifecycle manager for MemPalace.
//!
//! This module is a Rust port of the service-management surface from upstream
//! Python `mempalace` (notably `daemon.py` and `service.py`). It owns the
//! transport-neutral execution lifecycle: starting a background service (HTTP
//! health server or any user-provided async task), stopping it cleanly, checking
//! whether it is still running, and persisting a PID/state file under the
//! MemPalace config directory.
//!
//! The MCP transport itself remains in `crate::mcp_server`; this module only
//! manages the process/task lifecycle that keeps the service alive.

use std::fs;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::process;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch;
use tokio::task::JoinHandle;
use tracing::{info, warn};

/// Current status of a managed service.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ServiceStatus {
    Running,
    Stopped,
}

/// Serialized service state persisted to disk.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ServiceState {
    pub pid: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bind_address: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
    pub started_at: String,
    pub status: ServiceStatus,
}

/// Runtime status snapshot returned by [`Service::status`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ServiceStatusInfo {
    pub status: ServiceStatus,
    pub pid: Option<u32>,
    pub bind_address: Option<String>,
}

/// Context passed to a user-provided background service.
pub struct ServiceContext {
    pub config_dir: PathBuf,
    pub shutdown: watch::Receiver<bool>,
    pub token: String,
}

/// Manages the lifecycle of a MemPalace background service.
#[derive(Debug)]
pub struct Service {
    config_dir: PathBuf,
    state_file: PathBuf,
    token: String,
    shutdown_tx: Option<watch::Sender<bool>>,
    task: Option<JoinHandle<Result<()>>>,
    pid: Option<u32>,
    bound_addr: Option<String>,
}

impl Service {
    /// Create a new service manager that persists state under `config_dir`.
    pub fn new(config_dir: impl Into<PathBuf>) -> Result<Self> {
        let config_dir: PathBuf = config_dir.into();
        let service_dir = config_dir.join("service");
        fs::create_dir_all(&service_dir)?;
        let state_file = service_dir.join("state.json");

        Ok(Self {
            config_dir,
            state_file,
            token: String::new(),
            shutdown_tx: None,
            task: None,
            pid: None,
            bound_addr: None,
        })
    }

    /// Path to the persisted state file.
    pub fn state_file(&self) -> &Path {
        &self.state_file
    }

    /// MemPalace config directory.
    pub fn config_dir(&self) -> &Path {
        &self.config_dir
    }

    /// Current service token (empty before `start`).
    pub fn token(&self) -> &str {
        &self.token
    }

    /// PID of the running background service, if known.
    pub fn pid(&self) -> Option<u32> {
        self.pid
    }

    /// Bound address of the HTTP service, if known.
    pub fn bind_address(&self) -> Option<&str> {
        self.bound_addr.as_deref()
    }

    /// Returns `true` if the in-process background task is still running.
    pub fn is_running(&self) -> bool {
        if self.shutdown_tx.is_none() {
            return false;
        }
        self.task.as_ref().is_some_and(|t| !t.is_finished())
    }

    /// Read the persisted service state from disk.
    pub fn load_state(&self) -> Result<ServiceState> {
        let content = fs::read_to_string(&self.state_file).context("read service state file")?;
        let state: ServiceState =
            serde_json::from_str(&content).context("parse service state file")?;
        Ok(state)
    }

    /// Start a generic background service.
    ///
    /// The supplied closure receives a [`ServiceContext`] with a shutdown
    /// receiver and a stable token. It can run indefinitely and should exit
    /// when the shutdown receiver signals `true`.
    pub async fn start<F, Fut>(&mut self, service: F) -> Result<()>
    where
        F: FnOnce(ServiceContext) -> Fut + Send + 'static,
        Fut: Future<Output = Result<()>> + Send + 'static,
    {
        let (rx, token) = self.init_start(None)?;
        let ctx = ServiceContext {
            config_dir: self.config_dir.clone(),
            shutdown: rx,
            token: token.clone(),
        };
        let task = tokio::spawn(service(ctx));
        self.task = Some(task);
        info!("started generic background service (pid {})", process::id());
        Ok(())
    }

    /// Start a minimal HTTP health service in the background.
    ///
    /// The `bind` argument is passed to [`TcpListener::bind`]; pass
    /// `127.0.0.1:0` to let the OS pick an ephemeral port. The resolved address
    /// is stored in the service state and returned to the caller.
    pub async fn start_http(&mut self, bind: &str) -> Result<String> {
        let listener = TcpListener::bind(bind)
            .await
            .with_context(|| format!("bind HTTP service to {bind}"))?;
        let addr = listener.local_addr().context("get local address")?;
        let addr_str = addr.to_string();

        let (rx, token) = self.init_start(Some(addr_str.clone()))?;
        let _ = token;
        let task = tokio::spawn(run_http_server(listener, rx));
        self.task = Some(task);
        self.bound_addr = Some(addr_str.clone());

        info!(
            "started HTTP health service on {addr_str} (pid {})",
            process::id()
        );
        Ok(addr_str)
    }

    /// Stop the background service and write a `Stopped` state file.
    pub async fn stop(&mut self) -> Result<()> {
        if self.shutdown_tx.is_none() && self.task.is_none() {
            return Ok(());
        }

        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(true);
        }
        if let Some(handle) = self.task.take() {
            let _ = tokio::time::timeout(Duration::from_secs(5), handle).await;
        }

        let stopped = ServiceState {
            pid: self.pid.unwrap_or(process::id()),
            bind_address: self.bound_addr.clone(),
            token: Some(self.token.clone()),
            started_at: Utc::now().to_rfc3339(),
            status: ServiceStatus::Stopped,
        };
        self.write_state(&stopped)?;

        self.pid = None;
        self.bound_addr = None;
        self.shutdown_tx = None;
        self.task = None;
        info!("stopped background service");
        Ok(())
    }

    /// Check whether the service is currently running.
    ///
    /// If the service is not managed in-process, the state file is consulted
    /// and the recorded PID is probed for liveness.
    pub fn status(&self) -> Result<ServiceStatusInfo> {
        if self.is_running() {
            return Ok(ServiceStatusInfo {
                status: ServiceStatus::Running,
                pid: self.pid,
                bind_address: self.bound_addr.clone(),
            });
        }

        if !self.state_file.exists() {
            return Ok(ServiceStatusInfo {
                status: ServiceStatus::Stopped,
                pid: None,
                bind_address: None,
            });
        }

        let state = self.load_state()?;
        let running = state.status == ServiceStatus::Running && is_pid_alive(state.pid);

        Ok(ServiceStatusInfo {
            status: if running {
                ServiceStatus::Running
            } else {
                ServiceStatus::Stopped
            },
            pid: Some(state.pid),
            bind_address: state.bind_address,
        })
    }

    // ------------------------------------------------------------------
    // Internal helpers
    // ------------------------------------------------------------------

    fn init_start(
        &mut self,
        bind_address: Option<String>,
    ) -> Result<(watch::Receiver<bool>, String)> {
        if self.is_running() {
            return Err(anyhow!("service is already running"));
        }

        let token = generate_token();
        let (tx, rx) = watch::channel(false);
        let pid = process::id();

        let state = ServiceState {
            pid,
            bind_address,
            token: Some(token.clone()),
            started_at: Utc::now().to_rfc3339(),
            status: ServiceStatus::Running,
        };
        self.write_state(&state)?;

        self.pid = Some(pid);
        self.token = token.clone();
        self.shutdown_tx = Some(tx);
        Ok((rx, token))
    }

    fn write_state(&self, state: &ServiceState) -> Result<()> {
        if let Some(parent) = self.state_file.parent() {
            fs::create_dir_all(parent)?;
        }
        let content = serde_json::to_string_pretty(state).context("serialize service state")?;
        fs::write(&self.state_file, content).context("write service state file")?;
        Ok(())
    }
}

/// Generate a random 256-bit token, hex-encoded.
fn generate_token() -> String {
    let bytes: [u8; 32] = rand::random();
    hex::encode(bytes)
}

/// Check whether a process with the given PID is still alive.
///
/// This is a best-effort probe that does not introduce new platform
/// dependencies. On Unix it uses `kill -0`; on Windows it uses `tasklist`.
#[cfg(unix)]
fn is_pid_alive(pid: u32) -> bool {
    use std::process::Command;
    match Command::new("kill").arg("-0").arg(pid.to_string()).output() {
        Ok(output) => output.status.success(),
        Err(_) => false,
    }
}

#[cfg(windows)]
fn is_pid_alive(pid: u32) -> bool {
    use std::process::Command;
    let output = match Command::new("tasklist")
        .args(["/FI", &format!("PID eq {}", pid), "/FO", "CSV", "/NH"])
        .output()
    {
        Ok(output) => output,
        Err(_) => return false,
    };

    if !output.status.success() {
        return false;
    }

    let text = String::from_utf8_lossy(&output.stdout);
    text.contains(&pid.to_string())
}

/// Minimal HTTP health server used by [`Service::start_http`].
async fn run_http_server(listener: TcpListener, mut shutdown: watch::Receiver<bool>) -> Result<()> {
    loop {
        tokio::select! {
            biased;
            _ = shutdown.changed() => {
                if *shutdown.borrow() {
                    break;
                }
            }
            accept = listener.accept() => {
                match accept {
                    Ok((stream, _)) => {
                        tokio::spawn(handle_http_connection(stream));
                    }
                    Err(e) => {
                        warn!("HTTP accept error: {}", e);
                    }
                }
            }
        }
    }
    Ok(())
}

/// Handle a single HTTP connection.
async fn handle_http_connection(mut stream: TcpStream) -> Result<()> {
    let (reader, mut writer) = stream.split();
    let mut reader = BufReader::new(reader);
    let mut line = String::new();
    let mut method = String::new();
    let mut path = String::new();
    let mut content_length = 0usize;

    // Read the request line and headers.
    loop {
        line.clear();
        let n = reader.read_line(&mut line).await?;
        if n == 0 {
            break;
        }
        let trimmed = line.trim_end_matches('\n').trim_end_matches('\r');
        if trimmed.is_empty() {
            break;
        }
        if method.is_empty() {
            let parts: Vec<&str> = trimmed.split_whitespace().collect();
            if parts.len() >= 2 {
                method = parts[0].to_string();
                path = parts[1].to_string();
            }
        } else if trimmed.to_ascii_lowercase().starts_with("content-length:") {
            if let Some(v) = trimmed.split(':').nth(1) {
                content_length = v.trim().parse::<usize>().unwrap_or(0);
            }
        }
    }

    if content_length > 0 {
        let mut body = vec![0u8; content_length];
        reader.read_exact(&mut body).await?;
    }

    let response = match (method.as_str(), path.as_str()) {
        ("GET", "/health") => format_response(
            200,
            "OK",
            json!({"status": "ok", "pid": process::id()}).to_string(),
        ),
        _ => format_response(404, "Not Found", json!({"error": "not found"}).to_string()),
    };

    writer.write_all(response.as_bytes()).await?;
    writer.flush().await?;
    Ok(())
}

fn format_response(status: u16, reason: &str, body: String) -> String {
    format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tempfile::tempdir;
    use tokio::io::AsyncWriteExt;
    use tokio::net::TcpStream;
    use tokio::time::timeout;

    fn temp_service() -> (tempfile::TempDir, Service) {
        let dir = tempdir().unwrap();
        let service = Service::new(dir.path().to_path_buf()).unwrap();
        (dir, service)
    }

    async fn http_request(addr: &str, request: &str) -> Result<String> {
        let mut stream = TcpStream::connect(addr).await?;
        stream.write_all(request.as_bytes()).await?;
        stream.shutdown().await?;
        let mut buf = Vec::new();
        let read_fut = async {
            loop {
                let mut chunk = [0u8; 1024];
                match stream.try_read(&mut chunk) {
                    Ok(0) => break,
                    Ok(n) => buf.extend_from_slice(&chunk[..n]),
                    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        tokio::task::yield_now().await;
                    }
                    Err(ref e) if e.kind() == std::io::ErrorKind::ConnectionReset => break,
                    Err(e) => return Err(e),
                }
            }
            Ok::<_, std::io::Error>(())
        };
        timeout(Duration::from_secs(2), read_fut).await??;
        Ok(String::from_utf8_lossy(&buf).to_string())
    }

    #[tokio::test]
    async fn new_creates_service_directory() {
        let (dir, service) = temp_service();
        let state_dir = service.config_dir().join("service");
        assert!(state_dir.exists());
        assert!(dir.path().exists());
    }

    #[tokio::test]
    async fn start_http_stores_state() {
        let (_dir, mut service) = temp_service();
        let addr = service.start_http("127.0.0.1:0").await.unwrap();
        let info = service.status().unwrap();
        assert_eq!(info.status, ServiceStatus::Running);
        assert_eq!(info.pid, Some(process::id()));
        assert_eq!(info.bind_address, Some(addr));
        assert!(service.state_file().exists());
        let state = service.load_state().unwrap();
        assert_eq!(state.status, ServiceStatus::Running);
        assert_eq!(state.pid, process::id());
        assert!(state.token.is_some());
        service.stop().await.unwrap();
    }

    #[tokio::test]
    async fn health_endpoint_returns_ok() {
        let (_dir, mut service) = temp_service();
        let addr = service.start_http("127.0.0.1:0").await.unwrap();
        let response = http_request(&addr, "GET /health HTTP/1.1\r\nHost: localhost\r\n\r\n")
            .await
            .unwrap();
        assert!(response.contains("200 OK"));
        assert!(response.contains("\"status\":\"ok\""));
        service.stop().await.unwrap();
    }

    #[tokio::test]
    async fn stop_updates_status_to_stopped() {
        let (_dir, mut service) = temp_service();
        let _addr = service.start_http("127.0.0.1:0").await.unwrap();
        service.stop().await.unwrap();
        let info = service.status().unwrap();
        assert_eq!(info.status, ServiceStatus::Stopped);
        assert_eq!(service.pid(), None);
        let state = service.load_state().unwrap();
        assert_eq!(state.status, ServiceStatus::Stopped);
    }

    #[tokio::test]
    async fn double_start_returns_error() {
        let (_dir, mut service) = temp_service();
        service.start_http("127.0.0.1:0").await.unwrap();
        let result = service.start_http("127.0.0.1:0").await;
        assert!(result.is_err());
        service.stop().await.unwrap();
    }

    #[tokio::test]
    async fn status_stale_pid_is_stopped() {
        let (_dir, service) = temp_service();
        let state = ServiceState {
            pid: 999_999,
            bind_address: None,
            token: None,
            started_at: Utc::now().to_rfc3339(),
            status: ServiceStatus::Running,
        };
        service.write_state(&state).unwrap();
        let info = service.status().unwrap();
        assert_eq!(info.status, ServiceStatus::Stopped);
        assert_eq!(info.pid, Some(999_999));
    }

    #[tokio::test]
    async fn start_generic_service() {
        let (_dir, mut service) = temp_service();
        service
            .start(|mut ctx| async move {
                let _ = ctx.shutdown.changed().await;
                Ok(())
            })
            .await
            .unwrap();
        assert!(service.is_running());
        let info = service.status().unwrap();
        assert_eq!(info.status, ServiceStatus::Running);
        assert_eq!(info.bind_address, None);
        service.stop().await.unwrap();
        assert!(!service.is_running());
    }

    #[tokio::test]
    async fn http_unknown_path_returns_404() {
        let (_dir, mut service) = temp_service();
        let addr = service.start_http("127.0.0.1:0").await.unwrap();
        let response = http_request(&addr, "GET /foo HTTP/1.1\r\nHost: localhost\r\n\r\n")
            .await
            .unwrap();
        assert!(response.contains("404 Not Found"));
        assert!(response.contains("not found"));
        service.stop().await.unwrap();
    }

    #[tokio::test]
    async fn start_http_bad_bind_fails() {
        let (_dir, mut service) = temp_service();
        let result = service.start_http("256.0.0.1:0").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn load_state_from_file() {
        let (_dir, service) = temp_service();
        let state = ServiceState {
            pid: 1234,
            bind_address: Some("127.0.0.1:5678".to_string()),
            token: Some("abc123".to_string()),
            started_at: "2024-01-01T00:00:00Z".to_string(),
            status: ServiceStatus::Running,
        };
        service.write_state(&state).unwrap();
        let loaded = service.load_state().unwrap();
        assert_eq!(loaded, state);
        let info = service.status().unwrap();
        assert_eq!(info.pid, Some(1234));
        assert_eq!(info.bind_address, Some("127.0.0.1:5678".to_string()));
    }

    #[tokio::test]
    async fn status_no_state_file_is_stopped() {
        let (_dir, service) = temp_service();
        let info = service.status().unwrap();
        assert_eq!(info.status, ServiceStatus::Stopped);
        assert!(info.pid.is_none());
    }

    #[tokio::test]
    async fn token_is_unique() {
        let a = generate_token();
        let b = generate_token();
        assert_ne!(a, b);
        assert_eq!(a.len(), 64);
    }
}
