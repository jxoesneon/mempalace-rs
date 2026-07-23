//! Palace backup and restore utilities.
//!
//! MemPalace stores its state in a palace directory: SQLite databases
//! (`palace.db`, `knowledge.db`, `vectors.db`), the usearch vector index
//! (`vectors.usearch`), the write-ahead log (`wal.log`), plus any SQLite WAL
//! sidecar files (`*.db-wal`, `*.db-shm`). This module creates timestamped zip
//! archives of that directory and can restore a palace from an archive.

use anyhow::{anyhow, Context, Result};
use chrono::{DateTime, Local, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime};
use walkdir::WalkDir;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

/// Default prefix for backup archive names.
pub const DEFAULT_BACKUP_PREFIX: &str = "palace-backup";

/// Default timestamp format used in backup names.
pub const BACKUP_TIMESTAMP_FORMAT: &str = "%Y%m%d-%H%M%S-%9f";

/// Report returned by a successful backup or restore operation.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct BackupReport {
    pub backup_path: PathBuf,
    pub source_dir: PathBuf,
    pub files: usize,
    pub bytes: u64,
    pub timestamp: String,
    pub duration_ms: u128,
    pub pruned: Vec<PathBuf>,
}

/// Metadata for a single backup archive found on disk.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BackupInfo {
    pub path: PathBuf,
    pub created_at: String,
    pub bytes: u64,
    #[serde(skip, default = "unix_epoch")]
    pub modified: SystemTime,
}

fn unix_epoch() -> SystemTime {
    SystemTime::UNIX_EPOCH
}

impl Default for BackupInfo {
    fn default() -> Self {
        Self {
            path: PathBuf::new(),
            created_at: String::new(),
            bytes: 0,
            modified: SystemTime::UNIX_EPOCH,
        }
    }
}

/// Build a backup archive name from a prefix and a timestamp.
pub fn backup_name(prefix: &str, timestamp: &str) -> String {
    format!("{}-{}.zip", prefix, timestamp)
}

/// Check whether a file name looks like a backup produced by this module.
pub fn is_backup_file(name: &str, prefix: &str) -> bool {
    name.starts_with(prefix) && name.ends_with(".zip")
}

/// Create a timestamped zip backup of `palace_dir` inside `backup_dir`.
///
/// If `max_backups` is `Some(n)` and `n > 0`, the oldest matching backups in
/// `backup_dir` are deleted so that at most `n` remain.
pub fn create_backup(
    palace_dir: impl AsRef<Path>,
    backup_dir: impl AsRef<Path>,
    max_backups: Option<usize>,
) -> Result<BackupReport> {
    let palace_dir = palace_dir.as_ref();
    let backup_dir = backup_dir.as_ref();
    fs::create_dir_all(backup_dir)
        .with_context(|| format!("Failed to create backup directory {}", backup_dir.display()))?;

    let timestamp = Utc::now().format(BACKUP_TIMESTAMP_FORMAT).to_string();
    let name = backup_name(DEFAULT_BACKUP_PREFIX, &timestamp);
    let backup_path = backup_dir.join(&name);

    let start = Instant::now();
    let (files, bytes) = write_zip(palace_dir, &backup_path)?;
    let duration_ms = start.elapsed().as_millis();

    let pruned = match max_backups {
        Some(n) if n > 0 => prune_backups(backup_dir, n, DEFAULT_BACKUP_PREFIX)?,
        _ => Vec::new(),
    };

    Ok(BackupReport {
        backup_path,
        source_dir: palace_dir.to_path_buf(),
        files,
        bytes,
        timestamp: Utc::now().to_rfc3339(),
        duration_ms,
        pruned,
    })
}

/// Restore a palace directory from a backup zip archive.
///
/// If the target directory already exists and is not empty, the operation
/// fails unless `force` is `true`.
pub fn restore_backup(
    backup_path: impl AsRef<Path>,
    palace_dir: impl AsRef<Path>,
    force: bool,
) -> Result<BackupReport> {
    let backup_path = backup_path.as_ref();
    let palace_dir = palace_dir.as_ref();

    if palace_dir.exists() && !force {
        let mut entries = fs::read_dir(palace_dir)
            .with_context(|| format!("Failed to read target directory {}", palace_dir.display()))?;
        if entries.next().is_some() {
            return Err(anyhow!(
                "Target directory {} is not empty. Pass --force to overwrite.",
                palace_dir.display()
            ));
        }
    }

    fs::create_dir_all(palace_dir)
        .with_context(|| format!("Failed to create target directory {}", palace_dir.display()))?;

    let start = Instant::now();
    let (files, bytes) = extract_zip(backup_path, palace_dir)?;
    let duration_ms = start.elapsed().as_millis();

    Ok(BackupReport {
        backup_path: backup_path.to_path_buf(),
        source_dir: palace_dir.to_path_buf(),
        files,
        bytes,
        timestamp: Utc::now().to_rfc3339(),
        duration_ms,
        pruned: Vec::new(),
    })
}

/// List all backup archives matching `prefix` in `backup_dir`.
///
/// Results are sorted newest-first by filesystem modification time.
pub fn list_backups(backup_dir: impl AsRef<Path>, prefix: &str) -> Result<Vec<BackupInfo>> {
    let backup_dir = backup_dir.as_ref();
    if !backup_dir.exists() {
        return Ok(Vec::new());
    }

    let mut infos = Vec::new();
    for entry in fs::read_dir(backup_dir)
        .with_context(|| format!("Failed to read backup directory {}", backup_dir.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if !is_backup_file(&name, prefix) {
            continue;
        }

        let meta = entry
            .metadata()
            .with_context(|| format!("Failed to read metadata for {}", path.display()))?;
        let bytes = meta.len();
        let modified = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
        let created_at = DateTime::<Local>::from(modified)
            .format("%Y-%m-%d %H:%M:%S%.3f")
            .to_string();

        infos.push(BackupInfo {
            path,
            created_at,
            bytes,
            modified,
        });
    }

    infos.sort_by_key(|b| std::cmp::Reverse(b.modified));
    Ok(infos)
}

/// Delete the oldest matching backups so that at most `max_backups` remain.
///
/// Returns the list of paths that were removed. If `max_backups` is 0 no
/// backups are deleted.
pub fn prune_backups(
    backup_dir: impl AsRef<Path>,
    max_backups: usize,
    prefix: &str,
) -> Result<Vec<PathBuf>> {
    if max_backups == 0 {
        return Ok(Vec::new());
    }

    let mut infos = list_backups(backup_dir, prefix)?;
    if infos.len() <= max_backups {
        return Ok(Vec::new());
    }

    infos.sort_by_key(|b| std::cmp::Reverse(b.modified));

    let mut removed = Vec::new();
    for info in infos.into_iter().skip(max_backups) {
        match fs::remove_file(&info.path) {
            Ok(()) => {
                removed.push(info.path);
            }
            Err(e) => {
                eprintln!(
                    "Backup prune: could not remove {}: {}",
                    info.path.display(),
                    e
                );
            }
        }
    }
    Ok(removed)
}

fn write_zip(source_dir: &Path, zip_path: &Path) -> Result<(usize, u64)> {
    let file = fs::File::create(zip_path)
        .with_context(|| format!("Failed to create zip file {}", zip_path.display()))?;
    let mut writer = BufWriter::new(file);
    let mut zip = ZipWriter::new(&mut writer);

    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);

    let mut files = 0;
    let mut bytes = 0;

    for entry in WalkDir::new(source_dir).into_iter().filter_map(|e| e.ok()) {
        let path = entry.path();
        let relative = path.strip_prefix(source_dir).unwrap_or(path);
        if relative.as_os_str().is_empty() {
            continue;
        }

        let name = relative.to_string_lossy().replace('\\', "/");
        if path.is_file() {
            zip.start_file(&name, options)
                .with_context(|| format!("Failed to start zip entry {}", name))?;
            let mut f = fs::File::open(path)
                .with_context(|| format!("Failed to open source file {}", path.display()))?;
            std::io::copy(&mut f, &mut zip)
                .with_context(|| format!("Failed to copy {} into zip", path.display()))?;
            let len = f.metadata().map(|m| m.len()).unwrap_or(0);
            bytes += len;
            files += 1;
        } else if path.is_dir() {
            zip.add_directory(&name, options)
                .with_context(|| format!("Failed to add zip directory {}", name))?;
        }
    }

    zip.finish()
        .with_context(|| format!("Failed to finalize zip {}", zip_path.display()))?;
    writer
        .flush()
        .with_context(|| "Failed to flush zip writer")?;

    Ok((files, bytes))
}

fn extract_zip(backup_path: &Path, palace_dir: &Path) -> Result<(usize, u64)> {
    let file = fs::File::open(backup_path)
        .with_context(|| format!("Failed to open backup {}", backup_path.display()))?;
    let mut archive = ZipArchive::new(BufReader::new(file))
        .with_context(|| format!("Failed to read zip archive {}", backup_path.display()))?;

    let mut files = 0;
    let mut bytes = 0;

    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).with_context(|| {
            format!(
                "Failed to read zip entry {i} from {}",
                backup_path.display()
            )
        })?;
        let entry_name = entry
            .enclosed_name()
            .ok_or_else(|| anyhow!("Invalid zip entry path at index {i}"))?;
        let out_path = palace_dir.join(&entry_name);

        if entry.is_dir() {
            fs::create_dir_all(&out_path)
                .with_context(|| format!("Failed to create directory {}", out_path.display()))?;
        } else {
            if let Some(parent) = out_path.parent() {
                fs::create_dir_all(parent).with_context(|| {
                    format!("Failed to create parent directory {}", parent.display())
                })?;
            }
            let mut out = fs::File::create(&out_path)
                .with_context(|| format!("Failed to create output file {}", out_path.display()))?;
            std::io::copy(&mut entry, &mut out)
                .with_context(|| format!("Failed to extract {}", out_path.display()))?;
            let len = out.metadata().map(|m| m.len()).unwrap_or(0);
            bytes += len;
            files += 1;
        }
    }

    Ok((files, bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;
    use tempfile::tempdir;

    fn write_test_palace(dir: &Path) {
        fs::create_dir_all(dir.join("sub")).unwrap();
        fs::write(dir.join("palace.db"), b"palace data").unwrap();
        fs::write(dir.join("palace.db-wal"), b"wal data").unwrap();
        fs::write(dir.join("palace.db-shm"), b"shm data").unwrap();
        fs::write(dir.join("knowledge.db"), b"kg data").unwrap();
        fs::write(dir.join("vectors.db"), b"vector sqlite data").unwrap();
        fs::write(dir.join("vectors.usearch"), b"vector index data").unwrap();
        fs::write(dir.join("wal.log"), b"audit log").unwrap();
        fs::write(dir.join("sub").join("extra.txt"), b"extra file").unwrap();
    }

    #[test]
    fn test_backup_name_and_is_backup_file() {
        assert_eq!(
            backup_name(DEFAULT_BACKUP_PREFIX, "20240101-120000-000000000"),
            "palace-backup-20240101-120000-000000000.zip"
        );
        assert!(is_backup_file(
            "palace-backup-20240101-120000-000000000.zip",
            DEFAULT_BACKUP_PREFIX
        ));
        assert!(!is_backup_file(
            "other-backup-20240101-120000.zip",
            DEFAULT_BACKUP_PREFIX
        ));
        assert!(!is_backup_file(
            "palace-backup-20240101-120000-000000000.txt",
            DEFAULT_BACKUP_PREFIX
        ));
    }

    #[test]
    fn test_create_backup_writes_zip() {
        let palace = tempdir().unwrap();
        let backups = tempdir().unwrap();
        write_test_palace(palace.path());

        let report = create_backup(palace.path(), backups.path(), None).unwrap();
        assert!(report.backup_path.exists());
        assert_eq!(report.source_dir, palace.path());
        assert_eq!(report.files, 8);
        assert!(report.bytes > 0);
        assert!(report.pruned.is_empty());
        assert!(report
            .backup_path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("palace-backup-"));
    }

    #[test]
    fn test_create_backup_empty_palace() {
        let palace = tempdir().unwrap();
        let backups = tempdir().unwrap();
        fs::create_dir_all(palace.path()).unwrap();

        let report = create_backup(palace.path(), backups.path(), None).unwrap();
        assert!(report.backup_path.exists());
        assert_eq!(report.files, 0);
        assert_eq!(report.bytes, 0);
    }

    #[test]
    fn test_restore_backup_roundtrip() {
        let palace = tempdir().unwrap();
        let backups = tempdir().unwrap();
        let restore = tempdir().unwrap();
        write_test_palace(palace.path());

        let report = create_backup(palace.path(), backups.path(), None).unwrap();
        let restore_report = restore_backup(&report.backup_path, restore.path(), false).unwrap();
        assert_eq!(restore_report.files, report.files);
        assert_eq!(restore_report.bytes, report.bytes);
        assert!(restore.path().join("palace.db").exists());
        assert_eq!(
            fs::read(restore.path().join("palace.db")).unwrap(),
            b"palace data"
        );
        assert!(restore.path().join("sub").join("extra.txt").exists());
        assert_eq!(
            fs::read(restore.path().join("sub").join("extra.txt")).unwrap(),
            b"extra file"
        );
    }

    #[test]
    fn test_restore_backup_refuses_non_empty_directory() {
        let palace = tempdir().unwrap();
        let backups = tempdir().unwrap();
        let restore = tempdir().unwrap();
        write_test_palace(palace.path());
        fs::write(restore.path().join("existing.txt"), b"existing").unwrap();

        let report = create_backup(palace.path(), backups.path(), None).unwrap();
        assert!(restore_backup(&report.backup_path, restore.path(), false).is_err());
        assert!(restore_backup(&report.backup_path, restore.path(), true).is_ok());
        assert!(restore.path().join("palace.db").exists());
    }

    #[test]
    fn test_restore_backup_allows_empty_directory() {
        let palace = tempdir().unwrap();
        let backups = tempdir().unwrap();
        let restore = tempdir().unwrap();
        write_test_palace(palace.path());
        fs::create_dir_all(restore.path()).unwrap();

        let report = create_backup(palace.path(), backups.path(), None).unwrap();
        assert!(restore_backup(&report.backup_path, restore.path(), false).is_ok());
    }

    fn make_backup_with_data(backups: &Path, data: &str) -> PathBuf {
        let palace = tempdir().unwrap();
        fs::write(palace.path().join("data.txt"), data).unwrap();
        let report = create_backup(palace.path(), backups, None).unwrap();
        report.backup_path
    }

    #[test]
    fn test_list_backups_sorted() {
        let backups = tempdir().unwrap();
        for i in 0..3 {
            make_backup_with_data(backups.path(), &format!("data {}", i));
            thread::sleep(Duration::from_millis(10));
        }
        let list = list_backups(backups.path(), DEFAULT_BACKUP_PREFIX).unwrap();
        assert_eq!(list.len(), 3);
        for i in 1..list.len() {
            assert!(list[i - 1].modified >= list[i].modified);
        }
    }

    #[test]
    fn test_list_backups_empty_directory() {
        let backups = tempdir().unwrap();
        let list = list_backups(backups.path(), DEFAULT_BACKUP_PREFIX).unwrap();
        assert!(list.is_empty());
    }

    #[test]
    fn test_list_backups_missing_directory() {
        let backups = PathBuf::from("/tmp/nonexistent-palace-backups-dir-12345");
        let list = list_backups(&backups, DEFAULT_BACKUP_PREFIX).unwrap();
        assert!(list.is_empty());
    }

    #[test]
    fn test_prune_backups_keeps_newest() {
        let backups = tempdir().unwrap();
        for i in 0..5 {
            make_backup_with_data(backups.path(), &format!("data {}", i));
            thread::sleep(Duration::from_millis(10));
        }
        let pruned = prune_backups(backups.path(), 2, DEFAULT_BACKUP_PREFIX).unwrap();
        assert_eq!(pruned.len(), 3);
        let remaining = list_backups(backups.path(), DEFAULT_BACKUP_PREFIX).unwrap();
        assert_eq!(remaining.len(), 2);
        // The two newest should remain.
        for p in &remaining {
            assert!(!pruned.contains(&p.path));
        }
    }

    #[test]
    fn test_prune_backups_zero_max() {
        let backups = tempdir().unwrap();
        for i in 0..3 {
            make_backup_with_data(backups.path(), &format!("data {}", i));
            thread::sleep(Duration::from_millis(10));
        }
        let pruned = prune_backups(backups.path(), 0, DEFAULT_BACKUP_PREFIX).unwrap();
        assert!(pruned.is_empty());
        assert_eq!(
            list_backups(backups.path(), DEFAULT_BACKUP_PREFIX)
                .unwrap()
                .len(),
            3
        );
    }

    #[test]
    fn test_create_backup_prunes_in_one_call() {
        let backups = tempdir().unwrap();
        for i in 0..5 {
            make_backup_with_data(backups.path(), &format!("data {}", i));
            thread::sleep(Duration::from_millis(10));
        }
        let palace = tempdir().unwrap();
        fs::write(palace.path().join("data.txt"), b"newest").unwrap();
        let report = create_backup(palace.path(), backups.path(), Some(2)).unwrap();
        assert_eq!(report.pruned.len(), 4);
        assert_eq!(
            list_backups(backups.path(), DEFAULT_BACKUP_PREFIX)
                .unwrap()
                .len(),
            2
        );
        assert!(list_backups(backups.path(), DEFAULT_BACKUP_PREFIX)
            .unwrap()
            .iter()
            .any(|b| b.path == report.backup_path));
    }

    #[test]
    fn test_prune_backups_no_op_when_under_limit() {
        let backups = tempdir().unwrap();
        for i in 0..2 {
            make_backup_with_data(backups.path(), &format!("data {}", i));
            thread::sleep(Duration::from_millis(10));
        }
        let pruned = prune_backups(backups.path(), 5, DEFAULT_BACKUP_PREFIX).unwrap();
        assert!(pruned.is_empty());
        assert_eq!(
            list_backups(backups.path(), DEFAULT_BACKUP_PREFIX)
                .unwrap()
                .len(),
            2
        );
    }

    #[test]
    fn test_backup_info_default() {
        let info = BackupInfo::default();
        assert_eq!(info.path, PathBuf::new());
        assert_eq!(info.created_at, "");
        assert_eq!(info.bytes, 0);
        assert_eq!(info.modified, SystemTime::UNIX_EPOCH);
    }

    #[test]
    fn test_backup_info_deserialization_uses_unix_epoch() {
        let json = r#"{"path":"p","created_at":"c","bytes":0}"#;
        let info: BackupInfo = serde_json::from_str(json).unwrap();
        assert_eq!(info.path, PathBuf::from("p"));
        assert_eq!(info.created_at, "c");
        assert_eq!(info.bytes, 0);
        assert_eq!(info.modified, SystemTime::UNIX_EPOCH);
    }

    #[test]
    fn test_list_backups_ignores_non_matching_files() {
        let backups = tempdir().unwrap();
        let palace = tempdir().unwrap();
        fs::write(palace.path().join("data.txt"), b"data").unwrap();
        let report = create_backup(palace.path(), backups.path(), None).unwrap();
        fs::write(backups.path().join("notes.txt"), b"not a backup").unwrap();
        fs::write(backups.path().join("junk.zip"), b"not a zip").unwrap();

        let list = list_backups(backups.path(), DEFAULT_BACKUP_PREFIX).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].path, report.backup_path);
    }

    #[cfg(windows)]
    #[test]
    fn test_prune_backups_logs_error_when_file_locked() {
        use std::fs::OpenOptions;
        use std::os::windows::fs::OpenOptionsExt;

        let backups = tempdir().unwrap();
        let path1 = make_backup_with_data(backups.path(), "first");
        thread::sleep(Duration::from_millis(10));
        let path2 = make_backup_with_data(backups.path(), "second");

        let _lock = OpenOptions::new()
            .read(true)
            .write(true)
            .share_mode(0)
            .open(&path1)
            .unwrap();

        let pruned = prune_backups(backups.path(), 1, DEFAULT_BACKUP_PREFIX).unwrap();
        assert!(pruned.is_empty());
        assert!(path1.exists());
        assert!(path2.exists());
    }

    #[cfg(not(windows))]
    #[test]
    fn test_prune_backups_logs_error_when_file_locked() {
        let backups = tempdir().unwrap();
        let path1 = make_backup_with_data(backups.path(), "first");
        thread::sleep(Duration::from_millis(10));
        let path2 = make_backup_with_data(backups.path(), "second");

        let mut permissions = fs::metadata(backups.path()).unwrap().permissions();
        permissions.set_readonly(true);
        fs::set_permissions(backups.path(), permissions).unwrap();

        let pruned = prune_backups(backups.path(), 1, DEFAULT_BACKUP_PREFIX).unwrap();
        assert!(pruned.is_empty());
        assert!(path1.exists());
        assert!(path2.exists());

        let mut permissions = fs::metadata(backups.path()).unwrap().permissions();
        permissions.set_readonly(false);
        fs::set_permissions(backups.path(), permissions).unwrap();
    }

    #[test]
    fn test_write_zip_fails_when_output_is_directory() {
        let palace = tempdir().unwrap();
        fs::write(palace.path().join("file.txt"), b"data").unwrap();
        let backups = tempdir().unwrap();
        let zip_path = backups.path().join("output.zip");
        fs::create_dir(&zip_path).unwrap();

        let result = write_zip(palace.path(), &zip_path);
        assert!(result.is_err());
    }

    #[test]
    fn test_create_backup_fails_when_backup_dir_is_file() {
        let palace = tempdir().unwrap();
        fs::write(palace.path().join("file.txt"), b"data").unwrap();
        let root = tempdir().unwrap();
        let backup_dir = root.path().join("backup-dir");
        fs::write(&backup_dir, b"not a directory").unwrap();

        let result = create_backup(palace.path(), &backup_dir, None);
        assert!(result.is_err());
    }

    #[test]
    fn test_restore_backup_fails_when_backup_missing() {
        let palace = tempdir().unwrap();
        let missing_dir = tempdir().unwrap();
        let missing = missing_dir.path().join("missing.zip");
        let result = restore_backup(&missing, palace.path(), false);
        assert!(result.is_err());
    }

    #[test]
    fn test_restore_backup_fails_when_target_is_file() {
        let backups = tempdir().unwrap();
        let backup_path = make_backup_with_data(backups.path(), "data");
        let target_dir = tempdir().unwrap();
        let target = target_dir.path().join("target.txt");
        fs::write(&target, b"existing").unwrap();

        let result = restore_backup(&backup_path, &target, false);
        assert!(result.is_err());
    }

    #[test]
    fn test_extract_zip_fails_on_corrupted_archive() {
        let palace = tempdir().unwrap();
        let bad_dir = tempdir().unwrap();
        let bad_zip = bad_dir.path().join("bad.zip");
        fs::write(&bad_zip, b"not a valid zip").unwrap();

        let result = extract_zip(&bad_zip, palace.path());
        assert!(result.is_err());
    }

    #[test]
    fn test_extract_zip_fails_on_truncated_local_header() {
        let palace = tempdir().unwrap();
        write_test_palace(palace.path());
        let backups = tempdir().unwrap();
        let backup_path = create_backup(palace.path(), backups.path(), None)
            .unwrap()
            .backup_path;

        let mut bytes = fs::read(&backup_path).unwrap();
        // Corrupt the local file header magic number so the archive opens but
        // reading the first entry fails.
        for byte in &mut bytes[..4] {
            *byte = 0;
        }
        fs::write(&backup_path, &bytes).unwrap();

        let restore = tempdir().unwrap();
        let result = extract_zip(&backup_path, restore.path());
        assert!(result.is_err());
    }

    #[test]
    fn test_extract_zip_fails_when_target_is_file() {
        let palace = tempdir().unwrap();
        write_test_palace(palace.path());
        let backups = tempdir().unwrap();
        let backup_path = create_backup(palace.path(), backups.path(), None)
            .unwrap()
            .backup_path;

        let restore_dir = tempdir().unwrap();
        let restore = restore_dir.path().join("restore.txt");
        fs::write(&restore, b"existing").unwrap();

        let result = extract_zip(&backup_path, &restore);
        assert!(result.is_err());
    }
}
