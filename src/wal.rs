//! Write-ahead log (WAL) for audit-trailing MemPalace write operations.
//!
//! Each write-side operation is appended as a single JSON line to a `wal.log`
//! file inside the palace directory.  WAL failures are logged but never fatal --
//! the primary operation proceeds regardless, matching the behaviour of the
//! upstream Python implementation.

use anyhow::{Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

const WAL_FILE_NAME: &str = "wal.log";

/// Keys whose values are redacted from WAL entries to avoid storing sensitive
/// verbatim content in the audit log.
const REDACT_KEYS: &[&str] = &[
    "content",
    "content_preview",
    "document",
    "entry",
    "entry_preview",
    "query",
    "text",
    "agent_input",
    "agent_name",
    "drawer_content",
    "memory_content",
    "preview",
];

/// A single WAL audit record.
///
/// The core fields are always present and structured enough for downstream
/// replay/auditing.  `params` and `result` are optional extensions for extra
/// context; any string value under `params` whose key is in `REDACT_KEYS` is
/// replaced with a length marker before serialization.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct WalEntry {
    pub timestamp: String,
    pub operation: String,
    pub wing: Option<String>,
    pub room: Option<String>,
    pub drawer_id: Option<String>,
    pub source_file: Option<String>,
    pub content_hash: Option<String>,
    #[serde(skip_serializing_if = "Map::is_empty", default)]
    pub params: Map<String, Value>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub result: Option<Value>,
}

impl WalEntry {
    /// Create a new entry with the current UTC timestamp.
    pub fn new(operation: impl Into<String>) -> Self {
        Self {
            timestamp: Utc::now().to_rfc3339(),
            operation: operation.into(),
            ..Default::default()
        }
    }

    /// Set the wing field.
    pub fn wing(mut self, wing: impl Into<String>) -> Self {
        self.wing = Some(wing.into());
        self
    }

    /// Set the room field.
    pub fn room(mut self, room: impl Into<String>) -> Self {
        self.room = Some(room.into());
        self
    }

    /// Set the drawer / memory id field.
    pub fn drawer_id(mut self, drawer_id: impl Into<String>) -> Self {
        self.drawer_id = Some(drawer_id.into());
        self
    }

    /// Set the source file field.
    pub fn source_file(mut self, source_file: impl Into<String>) -> Self {
        self.source_file = Some(source_file.into());
        self
    }

    /// Set the content hash field.
    pub fn content_hash(mut self, content_hash: impl Into<String>) -> Self {
        self.content_hash = Some(content_hash.into());
        self
    }

    /// Add an opaque parameter.  String values under sensitive keys are
    /// redacted when the entry is appended to the WAL.
    pub fn param(mut self, key: impl Into<String>, value: Value) -> Self {
        self.params.insert(key.into(), value);
        self
    }

    /// Attach a result value.
    pub fn result(mut self, result: Value) -> Self {
        self.result = Some(result);
        self
    }
}

/// Compute a SHA-256 hex digest for a chunk of content.
pub fn hash_content(content: impl AsRef<[u8]>) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_ref());
    hex::encode(hasher.finalize())
}

/// Redact sensitive string values inside a params map in place.
fn redact_params(params: &mut Map<String, Value>) {
    for (k, v) in params.iter_mut() {
        if REDACT_KEYS.iter().any(|r| *r == k) {
            let redacted = match v {
                Value::String(s) => format!("[REDACTED {} chars]", s.len()),
                _ => "[REDACTED]".to_string(),
            };
            *v = Value::String(redacted);
        }
    }
}

/// Write-ahead log handle.
#[derive(Debug, Clone)]
pub struct Wal {
    path: PathBuf,
}

impl Wal {
    /// Open (or create) the WAL inside `palace_dir`.
    pub fn new(palace_dir: impl AsRef<Path>) -> Result<Self> {
        let palace_dir = palace_dir.as_ref();
        fs::create_dir_all(palace_dir).context("create WAL directory")?;
        let path = palace_dir.join(WAL_FILE_NAME);
        if !path.exists() {
            File::create(&path).context("create WAL file")?;
        }
        Ok(Self { path })
    }

    /// Open the WAL at an explicit file path.  The parent directory and the
    /// file itself are created if they do not exist.
    pub fn from_path(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).context("create WAL directory")?;
        }
        if !path.exists() {
            File::create(&path).context("create WAL file")?;
        }
        Ok(Self { path })
    }

    /// Return the path to the WAL file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Return the directory containing the WAL file.
    pub fn dir(&self) -> &Path {
        self.path.parent().unwrap_or_else(|| Path::new("."))
    }

    /// Append a single entry to the WAL as one JSON line.
    pub fn append(&self, mut entry: WalEntry) -> Result<()> {
        redact_params(&mut entry.params);
        let line = serde_json::to_string(&entry).context("serialize WAL entry")?;
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .context("open WAL file")?;
        writeln!(file, "{}", line).context("write WAL entry")?;
        file.flush().context("flush WAL file")?;
        Ok(())
    }

    /// Read all entries currently stored in the WAL.
    pub fn read_entries(&self) -> Result<Vec<WalEntry>> {
        let file = File::open(&self.path).context("open WAL file for reading")?;
        let reader = BufReader::new(file);
        let mut entries = Vec::new();
        for (i, line) in reader.lines().enumerate() {
            let line = line.with_context(|| format!("read WAL line {}", i + 1))?;
            if line.trim().is_empty() {
                continue;
            }
            let entry: WalEntry =
                serde_json::from_str(&line).with_context(|| format!("parse WAL line {}", i + 1))?;
            entries.push(entry);
        }
        Ok(entries)
    }

    /// Rotate the current WAL file to `wal.log.<timestamp>` and start a fresh one.
    ///
    /// Returns the path of the rotated file.  If the WAL file does not exist yet,
    /// this creates a fresh empty WAL and returns a path that would be used for
    /// the next rotation.
    pub fn rotate(&self) -> Result<PathBuf> {
        let timestamp = Utc::now().format("%Y%m%d_%H%M%S").to_string();
        let rotated = self.path.with_extension(format!("log.{}", timestamp));
        if self.path.exists() {
            fs::rename(&self.path, &rotated).context("rotate WAL file")?;
        }
        File::create(&self.path).context("create fresh WAL file")?;
        Ok(rotated)
    }

    /// Truncate the WAL file to zero bytes.
    pub fn truncate(&self) -> Result<()> {
        fs::write(&self.path, "").context("truncate WAL file")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::tempdir;

    #[test]
    fn test_wal_new_creates_directory() {
        let dir = tempdir().unwrap();
        let wal_dir = dir.path().join("nested").join("wal");
        let wal = Wal::new(&wal_dir).unwrap();
        assert!(wal_dir.exists());
        assert_eq!(wal.path(), wal_dir.join(WAL_FILE_NAME));
    }

    #[test]
    fn test_wal_from_path_creates_parent() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("logs").join("audit.log");
        let wal = Wal::from_path(&path).unwrap();
        assert!(wal.path().exists());
        assert_eq!(wal.path(), path);
    }

    #[test]
    fn test_append_and_read_entries() {
        let dir = tempdir().unwrap();
        let wal = Wal::new(dir.path()).unwrap();

        let entry = WalEntry::new("add_memory")
            .wing("people")
            .room("2024-01-01")
            .drawer_id("42")
            .source_file("note.txt")
            .content_hash(hash_content("hello world"))
            .param("importance", json!(5.0));

        wal.append(entry.clone()).unwrap();
        let entries = wal.read_entries().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].operation, "add_memory");
        assert_eq!(entries[0].wing, Some("people".to_string()));
        assert_eq!(entries[0].room, Some("2024-01-01".to_string()));
        assert_eq!(entries[0].drawer_id, Some("42".to_string()));
        assert_eq!(entries[0].source_file, Some("note.txt".to_string()));
        assert_eq!(entries[0].content_hash, Some(hash_content("hello world")));
        assert_eq!(entries[0].params.get("importance").unwrap(), &json!(5.0));
    }

    #[test]
    fn test_append_multiple_lines() {
        let dir = tempdir().unwrap();
        let wal = Wal::new(dir.path()).unwrap();
        wal.append(WalEntry::new("add_wing").wing("projects"))
            .unwrap();
        wal.append(WalEntry::new("delete_memory").drawer_id("7"))
            .unwrap();
        wal.append(WalEntry::new("update_memory").drawer_id("7"))
            .unwrap();

        let entries = wal.read_entries().unwrap();
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].operation, "add_wing");
        assert_eq!(entries[1].operation, "delete_memory");
        assert_eq!(entries[2].operation, "update_memory");
    }

    #[test]
    fn test_redaction_in_params() {
        let dir = tempdir().unwrap();
        let wal = Wal::new(dir.path()).unwrap();
        let entry = WalEntry::new("add_memory")
            .param("content", json!("secret text"))
            .param("query", json!("what is the meaning?"))
            .param("safe_key", json!("keep me"));
        wal.append(entry).unwrap();

        let entries = wal.read_entries().unwrap();
        assert_eq!(entries.len(), 1);
        let params = &entries[0].params;
        assert_eq!(
            params.get("content").unwrap(),
            &json!("[REDACTED 11 chars]")
        );
        assert_eq!(params.get("query").unwrap(), &json!("[REDACTED 20 chars]"));
        assert_eq!(params.get("safe_key").unwrap(), &json!("keep me"));
    }

    #[test]
    fn test_redaction_non_string() {
        let mut params = Map::new();
        params.insert("content".to_string(), json!({"nested": true}));
        params.insert("text".to_string(), json!(123));
        redact_params(&mut params);
        assert_eq!(params.get("content").unwrap(), &json!("[REDACTED]"));
        assert_eq!(params.get("text").unwrap(), &json!("[REDACTED]"));
    }

    #[test]
    fn test_hash_content_deterministic() {
        let h1 = hash_content("same");
        let h2 = hash_content("same");
        let h3 = hash_content("different");
        assert_eq!(h1, h2);
        assert_ne!(h1, h3);
        assert_eq!(h1.len(), 64);
    }

    #[test]
    fn test_read_entries_ignores_empty_lines() {
        let dir = tempdir().unwrap();
        let wal = Wal::new(dir.path()).unwrap();
        fs::write(&wal.path(), "\n\n").unwrap();
        let entries = wal.read_entries().unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn test_read_entries_corrupt_line_is_error() {
        let dir = tempdir().unwrap();
        let wal = Wal::new(dir.path()).unwrap();
        fs::write(&wal.path(), "not json\n").unwrap();
        assert!(wal.read_entries().is_err());
    }

    #[test]
    fn test_rotate_renames_file() {
        let dir = tempdir().unwrap();
        let wal = Wal::new(dir.path()).unwrap();
        wal.append(WalEntry::new("add_wing").wing("x")).unwrap();
        let rotated = wal.rotate().unwrap();
        assert!(rotated.exists());
        assert!(wal.path().exists());
        assert_eq!(rotated.parent().unwrap(), dir.path());
        assert!(rotated
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("wal.log."));

        // Fresh WAL should be empty.
        wal.append(WalEntry::new("add_wing").wing("y")).unwrap();
        let entries = wal.read_entries().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].wing, Some("y".to_string()));
    }

    #[test]
    fn test_rotate_without_existing_file() {
        let dir = tempdir().unwrap();
        let wal = Wal::new(dir.path()).unwrap();
        let rotated = wal.rotate().unwrap();
        assert!(rotated
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("wal.log."));
        assert!(wal.path().exists());
    }

    #[test]
    fn test_truncate_clears_entries() {
        let dir = tempdir().unwrap();
        let wal = Wal::new(dir.path()).unwrap();
        wal.append(WalEntry::new("add_wing").wing("z")).unwrap();
        wal.truncate().unwrap();
        let entries = wal.read_entries().unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn test_wal_entry_serialization_roundtrip() {
        let entry = WalEntry::new("update_memory")
            .wing("projects")
            .room("rust")
            .drawer_id("99")
            .source_file("src/lib.rs")
            .content_hash(hash_content("abc"))
            .param("old_hash", json!(hash_content("old")))
            .result(json!({"id": 99}));
        let json_str = serde_json::to_string(&entry).unwrap();
        let parsed: WalEntry = serde_json::from_str(&json_str).unwrap();
        assert_eq!(entry, parsed);
    }

    #[test]
    fn test_wal_entry_default_timestamp() {
        let entry = WalEntry::new("noop");
        assert!(!entry.timestamp.is_empty());
        assert!(entry.timestamp.contains('T'));
    }

    #[test]
    fn test_wal_dir() {
        let dir = tempdir().unwrap();
        let wal = Wal::new(dir.path()).unwrap();
        assert_eq!(wal.dir(), dir.path());
        assert_eq!(wal.path(), dir.path().join(WAL_FILE_NAME));
    }

    #[test]
    fn test_wal_entry_with_result() {
        let dir = tempdir().unwrap();
        let wal = Wal::new(dir.path()).unwrap();
        let entry = WalEntry::new("add_wing")
            .wing("projects")
            .result(json!({"success": true, "id": 42 }));
        wal.append(entry).unwrap();
        let entries = wal.read_entries().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].result, Some(json!({"success": true, "id": 42 })));
    }
}
