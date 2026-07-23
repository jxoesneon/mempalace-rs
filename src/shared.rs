//! Shared utility helpers used across the MemPalace codebase.
//!
//! This module centralises small pieces of duplicated logic: SQL/JSON
//! where-clause construction, directory ignore lists, and JSON file
//! persistence (atomic save, graceful missing-file fallback).

use anyhow::{Context, Result};
use serde::{de::DeserializeOwned, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io::{ErrorKind, Write};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

/// Build a usearch-style JSON filter clause for wing/room filtering.
///
/// Returns `None` when no filters are supplied.  When both wing and room are
/// supplied the result is an `{ "$and": [...] }` object; otherwise a single
/// key/object is returned.
///
/// This replaces the duplicate implementations previously living in
/// `src/searcher.rs` and `src/storage.rs`.
pub fn build_where_clause(wing: Option<&str>, room: Option<&str>) -> Option<serde_json::Value> {
    let mut where_clause = HashMap::<String, serde_json::Value>::new();
    if let (Some(w), Some(r)) = (wing, room) {
        let mut and_vec = Vec::new();
        let mut w_map = HashMap::<String, serde_json::Value>::new();
        w_map.insert("wing".to_string(), serde_json::Value::String(w.to_string()));
        and_vec.push(serde_json::Value::Object(w_map.into_iter().collect()));

        let mut r_map = HashMap::<String, serde_json::Value>::new();
        r_map.insert("room".to_string(), serde_json::Value::String(r.to_string()));
        and_vec.push(serde_json::Value::Object(r_map.into_iter().collect()));

        where_clause.insert("$and".to_string(), serde_json::Value::Array(and_vec));
    } else if let Some(w) = wing {
        where_clause.insert("wing".to_string(), serde_json::Value::String(w.to_string()));
    } else if let Some(r) = room {
        where_clause.insert("room".to_string(), serde_json::Value::String(r.to_string()));
    }

    if where_clause.is_empty() {
        None
    } else {
        serde_json::to_value(where_clause).ok()
    }
}

/// Directory names that should be ignored while scanning project / conversation
/// trees.  This is the union of the previously duplicated lists in `miner.rs`,
/// `convo_scanner.rs`, `project_scanner.rs`, and `room_detector_local.rs`.
pub const SKIP_DIRS: &[&str] = &[
    ".DS_Store",
    ".git",
    ".github",
    ".gitignore",
    ".idea",
    ".mempalace",
    ".mypy_cache",
    ".next",
    ".pytest_cache",
    ".ruff_cache",
    ".terraform",
    ".venv",
    ".vscode",
    "__pycache__",
    ".cache",
    ".claude",
    "build",
    "coverage",
    "dist",
    "env",
    "node_modules",
    "target",
    "venv",
    "vendor",
];

/// Check whether a directory name should be skipped.
pub fn is_skip_dir(name: &str) -> bool {
    SKIP_DIRS.contains(&name)
}

/// Load a JSON file from disk.
///
/// Returns `Err` if the file is missing or cannot be parsed.
pub fn load_json_file<T: DeserializeOwned>(path: &Path) -> Result<T> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("failed to read JSON file: {}", path.display()))?;
    serde_json::from_str(&content)
        .with_context(|| format!("failed to parse JSON file: {}", path.display()))
}

/// Load a JSON file from disk, returning `T::default()` when the file is missing.
///
/// Other errors (e.g. malformed JSON) are still propagated.
pub fn load_json_file_or_default<T: DeserializeOwned + Default>(path: &Path) -> Result<T> {
    match load_json_file(path) {
        Ok(value) => Ok(value),
        Err(e) => {
            let is_not_found = e
                .chain()
                .filter_map(|c| c.downcast_ref::<std::io::Error>())
                .any(|io| io.kind() == ErrorKind::NotFound);
            if is_not_found {
                Ok(T::default())
            } else {
                Err(e)
            }
        }
    }
}

/// Persist a JSON value atomically.
///
/// The value is written to a unique temporary file in the same directory and
/// then renamed into place.  Parent directories are created if necessary.
pub fn save_json_file<T: Serialize + ?Sized>(path: &Path, value: &T) -> Result<()> {
    let dir = path.parent().context("path has no parent directory")?;
    fs::create_dir_all(dir)
        .with_context(|| format!("failed to create directory: {}", dir.display()))?;

    let content = serde_json::to_string_pretty(value)
        .with_context(|| "failed to serialize JSON value")?;

    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let temp_path = dir.join(format!(".tmp-{}-{}.json", std::process::id(), nanos));

    let mut file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&temp_path)
        .with_context(|| format!("failed to create temporary file: {}", temp_path.display()))?;
    file.write_all(content.as_bytes())
        .with_context(|| format!("failed to write temporary file: {}", temp_path.display()))?;
    file.flush()
        .with_context(|| format!("failed to flush temporary file: {}", temp_path.display()))?;
    drop(file);

    fs::rename(&temp_path, path)
        .with_context(|| format!("failed to rename temporary file to: {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};
    use tempfile::tempdir;

    #[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq)]
    struct TestPayload {
        name: String,
        count: i32,
    }

    #[test]
    fn test_build_where_clause_empty() {
        assert_eq!(build_where_clause(None, None), None);
    }

    #[test]
    fn test_build_where_clause_wing_only() {
        let res = build_where_clause(Some("engineering"), None).unwrap();
        assert_eq!(res["wing"], "engineering");
    }

    #[test]
    fn test_build_where_clause_room_only() {
        let res = build_where_clause(None, Some("rust")).unwrap();
        assert_eq!(res["room"], "rust");
    }

    #[test]
    fn test_build_where_clause_wing_and_room() {
        let res = build_where_clause(Some("engineering"), Some("rust")).unwrap();
        let and_arr = res["$and"].as_array().unwrap();
        assert_eq!(and_arr.len(), 2);
        assert_eq!(and_arr[0]["wing"], "engineering");
        assert_eq!(and_arr[1]["room"], "rust");
    }

    #[test]
    fn test_skip_dirs_and_is_skip_dir() {
        assert!(is_skip_dir(".git"));
        assert!(is_skip_dir("node_modules"));
        assert!(is_skip_dir("target"));
        assert!(!is_skip_dir("src"));
        assert!(!is_skip_dir("docs"));
    }

    #[test]
    fn test_save_and_load_json_file() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.json");
        let payload = TestPayload {
            name: "hello".to_string(),
            count: 42,
        };

        save_json_file(&path, &payload).unwrap();
        let loaded: TestPayload = load_json_file(&path).unwrap();
        assert_eq!(loaded, payload);
    }

    #[test]
    fn test_load_json_file_missing() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("missing.json");
        assert!(load_json_file::<TestPayload>(&path).is_err());
    }

    #[test]
    fn test_load_json_file_or_default_missing() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("missing.json");
        let loaded: TestPayload = load_json_file_or_default(&path).unwrap();
        assert_eq!(loaded, TestPayload::default());
    }

    #[test]
    fn test_load_json_file_or_default_malformed() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("bad.json");
        fs::write(&path, "not-json").unwrap();
        assert!(load_json_file_or_default::<TestPayload>(&path).is_err());
    }

    #[test]
    fn test_save_json_file_creates_parent_dirs() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("a").join("b").join("c.json");
        let payload = TestPayload::default();
        save_json_file(&path, &payload).unwrap();
        assert!(path.exists());
    }

    #[test]
    fn test_save_json_file_atomic_overwrite() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("atomic.json");
        fs::write(&path, "old content").unwrap();
        let payload = TestPayload {
            name: "new".to_string(),
            count: 7,
        };
        save_json_file(&path, &payload).unwrap();
        let loaded: TestPayload = load_json_file(&path).unwrap();
        assert_eq!(loaded, payload);
    }
}
