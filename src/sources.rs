//! Source provenance tracking for memories.
//!
//! This module provides helpers to inspect the `source_file` field of the
//! memory table in `VectorStorage`. It can list unique source files, count
//! memories per source, and validate whether the source paths still exist on
//! disk.

use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result};

use crate::vector_storage::VectorStorage;

/// Statistics for a single source file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceStats {
    /// The source path as stored in `memories.source_file`.
    pub source: String,
    /// Number of memories that reference this source.
    pub count: u64,
    /// Whether the source path still exists on disk.
    pub exists: bool,
}

/// Return a sorted list of unique source files stored in the palace.
///
/// `NULL` source entries are ignored. Empty strings are kept because they are a
/// valid (if unusual) stored value.
pub fn list_sources(storage: &VectorStorage) -> Result<Vec<String>> {
    let mut stmt = storage
        .db
        .prepare("SELECT DISTINCT source_file FROM memories WHERE source_file IS NOT NULL ORDER BY source_file")?;
    let sources = stmt
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()
        .context("Failed to list sources")?;
    Ok(sources)
}

/// Count how many memories reference each source file.
///
/// Returns a map from source path to count. `NULL` sources are ignored.
pub fn count_by_source(storage: &VectorStorage) -> Result<HashMap<String, u64>> {
    let mut stmt = storage
        .db
        .prepare("SELECT source_file, COUNT(*) FROM memories WHERE source_file IS NOT NULL GROUP BY source_file")?;
    let rows = stmt
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? as u64))
        })?
        .collect::<rusqlite::Result<HashMap<_, _>>>()
        .context("Failed to count memories per source")?;
    Ok(rows)
}

/// Check whether a single source path exists on disk.
///
/// A `NULL` or empty path is considered non-existent.
pub fn source_exists(source: &str) -> bool {
    if source.is_empty() {
        return false;
    }
    Path::new(source).exists()
}

/// Build a full report of source provenance.
///
/// The returned vector is sorted by source path. Each entry contains the source
/// path, the number of memories referencing it, and whether the path still
/// exists on disk.
pub fn source_report(storage: &VectorStorage) -> Result<Vec<SourceStats>> {
    let counts = count_by_source(storage)?;
    let mut report: Vec<SourceStats> = counts
        .into_iter()
        .map(|(source, count)| SourceStats {
            exists: source_exists(&source),
            source,
            count,
        })
        .collect();
    report.sort_by(|a, b| a.source.cmp(&b.source));
    Ok(report)
}

/// Count the total number of memories that have a non-null source file.
pub fn count_sourced_memories(storage: &VectorStorage) -> Result<u64> {
    let count: i64 = storage
        .db
        .query_row(
            "SELECT COUNT(*) FROM memories WHERE source_file IS NOT NULL",
            [],
            |row| row.get(0),
        )
        .context("Failed to count sourced memories")?;
    Ok(count as u64)
}

/// Count the total number of memories that have no source file.
pub fn count_unsourced_memories(storage: &VectorStorage) -> Result<u64> {
    let count: i64 = storage
        .db
        .query_row(
            "SELECT COUNT(*) FROM memories WHERE source_file IS NULL",
            [],
            |row| row.get(0),
        )
        .context("Failed to count unsourced memories")?;
    Ok(count as u64)
}

/// Print a human-readable source report to stdout.
///
/// This is used by the CLI `Sources` command.
pub fn print_source_report(storage: &VectorStorage) -> Result<()> {
    let report = source_report(storage)?;
    let sourced = count_sourced_memories(storage)?;
    let unsourced = count_unsourced_memories(storage)?;

    println!("Source provenance report");
    println!("==========================");
    println!("Sourced memories: {sourced}");
    println!("Unsourced memories: {unsourced}");
    println!("Unique sources: {}", report.len());
    if report.is_empty() {
        println!("No source files recorded.");
        return Ok(());
    }
    println!();
    println!("{:<50} {:>10} {:>8}", "Source", "Memories", "Exists");
    println!(
        "{:<50} {:>10} {:>8}",
        "-".repeat(50),
        "-".repeat(10),
        "-".repeat(8)
    );
    for stat in report {
        println!(
            "{:<50} {:>10} {:>8}",
            truncate(&stat.source, 50),
            stat.count,
            if stat.exists { "yes" } else { "no" }
        );
    }
    Ok(())
}

fn truncate(s: &str, max_len: usize) -> String {
    if s.chars().count() <= max_len {
        s.to_string()
    } else {
        let mut out = s
            .chars()
            .take(max_len.saturating_sub(3))
            .collect::<String>();
        out.push_str("...");
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vector_storage::VectorStorage;
    use std::fs::File;
    use std::io::Write;
    use tempfile::tempdir;

    fn temp_storage() -> (tempfile::TempDir, VectorStorage) {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("db.sqlite");
        let index_path = dir.path().join("index.usearch");
        let vs = VectorStorage::new(&db_path, &index_path).unwrap();
        (dir, vs)
    }

    fn insert_raw(
        storage: &mut VectorStorage,
        text: &str,
        wing: &str,
        room: &str,
        source: Option<&str>,
    ) {
        storage
            .add_memory(text, wing, room, source, None)
            .expect("failed to add memory");
    }

    #[test]
    fn test_list_sources_empty() {
        let (_dir, storage) = temp_storage();
        let sources = list_sources(&storage).unwrap();
        assert!(sources.is_empty());
    }

    #[test]
    fn test_list_sources_returns_unique_sorted() {
        let (_dir, mut storage) = temp_storage();
        insert_raw(&mut storage, "a", "w", "r", Some("/tmp/b.txt"));
        insert_raw(&mut storage, "a", "w", "r", Some("/tmp/a.txt"));
        insert_raw(&mut storage, "a", "w", "r", Some("/tmp/b.txt"));
        insert_raw(&mut storage, "a", "w", "r", None);

        let sources = list_sources(&storage).unwrap();
        assert_eq!(sources, vec!["/tmp/a.txt", "/tmp/b.txt"]);
    }

    #[test]
    fn test_count_by_source() {
        let (_dir, mut storage) = temp_storage();
        insert_raw(&mut storage, "a", "w", "r", Some("/tmp/a.txt"));
        insert_raw(&mut storage, "b", "w", "r", Some("/tmp/a.txt"));
        insert_raw(&mut storage, "c", "w", "r", Some("/tmp/b.txt"));
        insert_raw(&mut storage, "d", "w", "r", None);

        let counts = count_by_source(&storage).unwrap();
        let mut expected = HashMap::new();
        expected.insert("/tmp/a.txt".to_string(), 2u64);
        expected.insert("/tmp/b.txt".to_string(), 1u64);
        assert_eq!(counts, expected);
    }

    #[test]
    fn test_source_exists() {
        assert!(!source_exists(""));
        assert!(!source_exists("/nonexistent/path/file.txt"));
        let dir = tempdir().unwrap();
        let file = dir.path().join("real.txt");
        File::create(&file).unwrap();
        assert!(source_exists(file.to_str().unwrap()));
    }

    #[test]
    fn test_source_report() {
        let (dir, mut storage) = temp_storage();
        let real_file = dir.path().join("real.txt");
        File::create(&real_file).unwrap();
        let missing = dir.path().join("missing.txt");

        insert_raw(
            &mut storage,
            "a",
            "w",
            "r",
            Some(real_file.to_str().unwrap()),
        );
        insert_raw(&mut storage, "b", "w", "r", Some(missing.to_str().unwrap()));
        insert_raw(&mut storage, "c", "w", "r", None);

        let report = source_report(&storage).unwrap();
        assert_eq!(report.len(), 2);
        assert_eq!(report[0].source, missing.to_str().unwrap().to_string());
        assert_eq!(report[0].count, 1);
        assert!(!report[0].exists);
        assert_eq!(report[1].source, real_file.to_str().unwrap().to_string());
        assert_eq!(report[1].count, 1);
        assert!(report[1].exists);
    }

    #[test]
    fn test_count_sourced_and_unsourced() {
        let (_dir, mut storage) = temp_storage();
        insert_raw(&mut storage, "a", "w", "r", Some("/tmp/a.txt"));
        insert_raw(&mut storage, "b", "w", "r", Some("/tmp/a.txt"));
        insert_raw(&mut storage, "c", "w", "r", None);
        insert_raw(&mut storage, "d", "w", "r", None);
        insert_raw(&mut storage, "e", "w", "r", Some("/tmp/b.txt"));

        assert_eq!(count_sourced_memories(&storage).unwrap(), 3);
        assert_eq!(count_unsourced_memories(&storage).unwrap(), 2);
    }

    #[test]
    fn test_count_by_source_empty() {
        let (_dir, storage) = temp_storage();
        let counts = count_by_source(&storage).unwrap();
        assert!(counts.is_empty());
    }

    #[test]
    fn test_print_source_report() {
        let (dir, mut storage) = temp_storage();
        let real_file = dir.path().join("real.txt");
        File::create(&real_file).unwrap();
        insert_raw(
            &mut storage,
            "a",
            "w",
            "r",
            Some(real_file.to_str().unwrap()),
        );
        insert_raw(&mut storage, "b", "w", "r", None);

        // Just verify it does not panic / error.
        print_source_report(&storage).unwrap();
    }

    #[test]
    fn test_truncate_long_source() {
        let long = "a".repeat(100);
        let out = truncate(&long, 50);
        assert_eq!(out.len(), 50);
        assert!(out.ends_with("..."));
    }

    #[test]
    fn test_truncate_short_source() {
        let short = "hello";
        let out = truncate(short, 50);
        assert_eq!(out, "hello");
    }

    #[test]
    fn test_source_report_with_empty_source_string() {
        let (_dir, mut storage) = temp_storage();
        // Empty string is a valid stored value and should be reported.
        insert_raw(&mut storage, "a", "w", "r", Some(""));
        insert_raw(&mut storage, "b", "w", "r", Some(""));

        let report = source_report(&storage).unwrap();
        assert_eq!(report.len(), 1);
        assert_eq!(report[0].source, "");
        assert_eq!(report[0].count, 2);
        assert!(!report[0].exists);
    }

    #[test]
    fn test_source_report_preserves_total_counts() {
        let (dir, mut storage) = temp_storage();
        let file = dir.path().join("multi.txt");
        File::create(&file).unwrap();
        let path = file.to_str().unwrap().to_string();

        for i in 0..5 {
            insert_raw(&mut storage, &format!("text-{i}"), "w", "r", Some(&path));
        }

        let report = source_report(&storage).unwrap();
        assert_eq!(report.len(), 1);
        assert_eq!(report[0].count, 5);
        assert!(report[0].exists);
        assert_eq!(count_sourced_memories(&storage).unwrap(), 5);
        assert_eq!(count_unsourced_memories(&storage).unwrap(), 0);
    }

    #[test]
    fn test_print_source_report_empty() {
        let (_dir, storage) = temp_storage();
        print_source_report(&storage).unwrap();
    }

    #[test]
    fn test_source_exists_directory() {
        let dir = tempdir().unwrap();
        assert!(source_exists(dir.path().to_str().unwrap()));
    }

    #[test]
    fn test_list_sources_ignores_null_only() {
        let (_dir, mut storage) = temp_storage();
        insert_raw(&mut storage, "a", "w", "r", None);
        let sources = list_sources(&storage).unwrap();
        assert!(sources.is_empty());
    }

    #[test]
    fn test_count_by_source_does_not_include_null() {
        let (_dir, mut storage) = temp_storage();
        insert_raw(&mut storage, "a", "w", "r", None);
        insert_raw(&mut storage, "b", "w", "r", Some("/tmp/s.txt"));
        let counts = count_by_source(&storage).unwrap();
        assert_eq!(counts.len(), 1);
        assert_eq!(counts.get("/tmp/s.txt"), Some(&1));
    }

    #[test]
    fn test_source_report_with_many_sources() {
        let (dir, mut storage) = temp_storage();
        let mut created = Vec::new();
        for i in 0..10 {
            let path = dir.path().join(format!("source_{i}.txt"));
            let mut f = File::create(&path).unwrap();
            writeln!(f, "content {i}").unwrap();
            created.push(path);
        }
        for (i, path) in created.iter().enumerate() {
            let count = (i + 1) as u64;
            for j in 0..count {
                insert_raw(
                    &mut storage,
                    &format!("text-{i}-{j}"),
                    "w",
                    "r",
                    Some(path.to_str().unwrap()),
                );
            }
        }

        let report = source_report(&storage).unwrap();
        assert_eq!(report.len(), 10);
        for (i, stat) in report.iter().enumerate() {
            assert!(stat.exists);
            assert_eq!(stat.count, (i + 1) as u64);
        }
    }
}
