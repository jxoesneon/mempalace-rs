// dedup.rs — Semantic deduplication of memory drawers
//
// Groups memories by source_file, compares embeddings using the existing
// VectorStorage HNSW index, and reports (or deletes) near-duplicate drawers.
// Keeps the longest representative of each duplicate group.

use crate::vector_storage::{MemoryRecord, VectorStorage};
use anyhow::{anyhow, Result};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use tracing::warn;

/// Cosine distance threshold (lower = stricter).
/// 0.15 corresponds to ~85% cosine similarity.
pub const DEFAULT_THRESHOLD: f32 = 0.15;

/// Minimum number of drawers from a single source before it is worth checking.
pub const MIN_DRAWERS_TO_CHECK: usize = 5;

/// Minimum content length for a drawer to be considered a keeper.
const MIN_CONTENT_LENGTH: usize = 20;

/// Per-source deduplication result.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct SourceDedupReport {
    pub source: String,
    pub checked: usize,
    pub kept: usize,
    pub deleted: usize,
    pub kept_ids: Vec<i64>,
    pub deleted_ids: Vec<i64>,
}

/// Aggregated deduplication report across all checked sources.
#[derive(Debug, Clone, Default, Serialize)]
pub struct DedupReport {
    pub sources_checked: usize,
    pub total_drawers: usize,
    pub kept: usize,
    pub deleted: usize,
    pub per_source: Vec<SourceDedupReport>,
}

/// Group memory IDs by `source_file`, filtering by optional source pattern and wing.
/// Only sources with at least `min_count` drawers are returned.
fn group_by_source(
    vs: &VectorStorage,
    min_count: usize,
    source_pattern: Option<&str>,
    wing: Option<&str>,
) -> Result<HashMap<String, Vec<i64>>> {
    let mut groups: HashMap<String, Vec<i64>> = HashMap::new();

    let (sql, params): (String, Vec<Box<dyn rusqlite::ToSql>>) = match wing {
        Some(w) => (
            "SELECT id, source_file FROM memories WHERE wing = ?1".to_string(),
            vec![Box::new(w.to_string())],
        ),
        None => ("SELECT id, source_file FROM memories".to_string(), vec![]),
    };

    let mut stmt = vs
        .db
        .prepare(&sql)
        .map_err(|e| anyhow!("Failed to prepare source grouping query: {e}"))?;
    let params_ref: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();

    let rows = stmt
        .query_map(params_ref.as_slice(), |row| {
            let id: i64 = row.get(0)?;
            let source: Option<String> = row.get(1)?;
            Ok((id, source.unwrap_or_else(|| "unknown".to_string())))
        })
        .map_err(|e| anyhow!("Failed to iterate memories for grouping: {e}"))?;

    for row in rows {
        let (id, source) = row?;
        if let Some(pattern) = source_pattern {
            if !source.to_lowercase().contains(&pattern.to_lowercase()) {
                continue;
            }
        }
        groups.entry(source).or_default().push(id);
    }

    groups.retain(|_, ids| ids.len() >= min_count);
    Ok(groups)
}

/// Deduplicate a single source group.
///
/// Sorts by content length (longest first), then walks the drawers in that
/// order. Each drawer is compared against the already-kept representatives
/// using the HNSW index; if any kept neighbor is within `threshold` cosine
/// distance, the drawer is marked as a duplicate.
fn dedup_source_group(
    vs: &VectorStorage,
    source: &str,
    ids: &[i64],
    threshold: f32,
    dry_run: bool,
) -> Result<SourceDedupReport> {
    let mut records: Vec<MemoryRecord> = ids
        .iter()
        .map(|id| vs.get_memory_by_id(*id))
        .collect::<Result<Vec<_>>>()
        .map_err(|e| anyhow!("Failed to fetch source group records for {source}: {e}"))?;

    // Longest first; tie-break on id for determinism.
    records.sort_by(|a, b| {
        b.text_content
            .len()
            .cmp(&a.text_content.len())
            .then(a.id.cmp(&b.id))
    });

    let mut kept_set: HashSet<i64> = HashSet::new();
    let mut kept: Vec<i64> = Vec::new();
    let mut deleted: Vec<i64> = Vec::new();

    for record in records {
        if record.text_content.len() < MIN_CONTENT_LENGTH {
            deleted.push(record.id);
            continue;
        }

        if kept_set.is_empty() {
            kept_set.insert(record.id);
            kept.push(record.id);
            continue;
        }

        let vector = match vs.embed_single(&record.text_content) {
            Ok(v) => v,
            Err(e) => {
                warn!(
                    "Failed to embed drawer {} for source {}: {e}; keeping it",
                    record.id, source
                );
                kept_set.insert(record.id);
                kept.push(record.id);
                continue;
            }
        };

        let limit = kept_set.len();
        let results = vs
            .index
            .filtered_search(&vector, limit, |key: u64| kept_set.contains(&(key as i64)))
            .map_err(|e| anyhow!("usearch filtered_search failed for source {source}: {e}"))?;

        let is_duplicate = results
            .keys
            .iter()
            .zip(results.distances.iter())
            .any(|(key, dist)| kept_set.contains(&(*key as i64)) && *dist < threshold);

        if is_duplicate {
            deleted.push(record.id);
        } else {
            kept_set.insert(record.id);
            kept.push(record.id);
        }
    }

    if !dry_run && !deleted.is_empty() {
        for id in &deleted {
            vs.delete_memory(*id)
                .map_err(|e| anyhow!("Failed to delete drawer {id}: {e}"))?;
        }
    }

    Ok(SourceDedupReport {
        source: source.to_string(),
        checked: ids.len(),
        kept: kept.len(),
        deleted: deleted.len(),
        kept_ids: kept,
        deleted_ids: deleted,
    })
}

/// Find duplicate drawers without modifying storage.
///
/// Returns a `DedupReport` describing which drawers would be deleted.
pub fn find_duplicates(
    vs: &VectorStorage,
    threshold: f32,
    source_pattern: Option<&str>,
    min_count: Option<usize>,
    wing: Option<&str>,
) -> Result<DedupReport> {
    let min_count = min_count.unwrap_or(MIN_DRAWERS_TO_CHECK);
    let groups = group_by_source(vs, min_count, source_pattern, wing)?;
    let mut report = DedupReport::default();

    for (source, ids) in groups {
        let source_report = dedup_source_group(vs, &source, &ids, threshold, true)?;
        report.total_drawers += source_report.checked;
        report.kept += source_report.kept;
        report.deleted += source_report.deleted;
        report.per_source.push(source_report);
    }

    report.sources_checked = report.per_source.len();
    report
        .per_source
        .sort_by(|a, b| b.checked.cmp(&a.checked).then(a.source.cmp(&b.source)));
    Ok(report)
}

/// Deduplicate drawers grouped by source.
///
/// When `dry_run` is `false`, duplicate drawers are deleted from the database.
/// The HNSW index is not updated; deleted records will be ignored on future
/// dedup passes because they are no longer returned by the database.
pub fn dedup_by_source(
    vs: &VectorStorage,
    threshold: f32,
    source_pattern: Option<&str>,
    min_count: Option<usize>,
    wing: Option<&str>,
    dry_run: bool,
) -> Result<DedupReport> {
    let min_count = min_count.unwrap_or(MIN_DRAWERS_TO_CHECK);
    let groups = group_by_source(vs, min_count, source_pattern, wing)?;
    let mut report = DedupReport::default();

    for (source, ids) in groups {
        let source_report = dedup_source_group(vs, &source, &ids, threshold, dry_run)?;
        report.total_drawers += source_report.checked;
        report.kept += source_report.kept;
        report.deleted += source_report.deleted;
        report.per_source.push(source_report);
    }

    report.sources_checked = report.per_source.len();
    report
        .per_source
        .sort_by(|a, b| b.checked.cmp(&a.checked).then(a.source.cmp(&b.source)));
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vector_storage::VectorStorage;
    use tempfile::tempdir;

    fn temp_storage() -> (tempfile::TempDir, VectorStorage) {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("vectors.db");
        let index_path = dir.path().join("vectors.usearch");
        let vs = VectorStorage::new(&db_path, &index_path).unwrap();
        (dir, vs)
    }

    #[test]
    fn test_empty_storage_find_duplicates() {
        let (_dir, vs) = temp_storage();
        let report = find_duplicates(&vs, DEFAULT_THRESHOLD, None, None, None).unwrap();
        assert_eq!(report.sources_checked, 0);
        assert_eq!(report.total_drawers, 0);
        assert_eq!(report.kept, 0);
        assert_eq!(report.deleted, 0);
        assert!(report.per_source.is_empty());
    }

    #[test]
    fn test_group_below_min_count_is_ignored() {
        let (_dir, mut vs) = temp_storage();
        for i in 0..3 {
            vs.add_memory(
                &format!("duplicate content number {}", i),
                "w",
                "r",
                Some("src.txt"),
                None,
            )
            .unwrap();
        }
        let report = find_duplicates(&vs, DEFAULT_THRESHOLD, None, None, None).unwrap();
        assert_eq!(report.sources_checked, 0);
    }

    #[test]
    fn test_find_duplicates_returns_expected_deletions() {
        let (_dir, mut vs) = temp_storage();
        let long_text =
            "the quick brown fox jumps over the lazy dog and then runs across the meadow";
        let distinct_text = "something completely unrelated about bicycles and gears";
        let short_text = "short";

        // Source group with 5 drawers: 3 duplicates, 1 distinct, 1 short.
        for _ in 0..3 {
            vs.add_memory(long_text, "w", "r", Some("src.txt"), None)
                .unwrap();
        }
        vs.add_memory(distinct_text, "w", "r", Some("src.txt"), None)
            .unwrap();
        vs.add_memory(short_text, "w", "r", Some("src.txt"), None)
            .unwrap();

        let report = find_duplicates(&vs, DEFAULT_THRESHOLD, None, None, None).unwrap();
        assert_eq!(report.sources_checked, 1);
        assert_eq!(report.total_drawers, 5);
        // Longest duplicate kept, distinct kept, short deleted, other two duplicates deleted.
        assert_eq!(report.kept, 2, "expected 2 kept, got {:?}", report);
        assert_eq!(report.deleted, 3, "expected 3 deleted, got {:?}", report);
    }

    #[test]
    fn test_dedup_by_source_dry_run_does_not_delete() {
        let (dir, mut vs) = temp_storage();
        let text = "this is a moderately long duplicate sentence for testing";
        for _ in 0..5 {
            vs.add_memory(text, "w", "r", Some("dry_run_src.txt"), None)
                .unwrap();
        }
        // Save index and drop so the file is flushed before reopening.
        let db_path = dir.path().join("vectors.db");
        let index_path = dir.path().join("vectors.usearch");
        vs.save_index(&index_path).unwrap();
        drop(vs);

        let mut vs2 = VectorStorage::new(&db_path, &index_path).unwrap();
        let before = vs2.memory_count().unwrap();
        let report = dedup_by_source(&mut vs2, DEFAULT_THRESHOLD, None, None, None, true).unwrap();
        assert_eq!(report.deleted, 4);
        assert_eq!(vs2.memory_count().unwrap(), before);
    }

    #[test]
    fn test_dedup_by_source_live_deletes() {
        let (dir, mut vs) = temp_storage();
        let text = "this is a moderately long duplicate sentence for testing live deletion";
        for _ in 0..5 {
            vs.add_memory(text, "w", "r", Some("live_src.txt"), None)
                .unwrap();
        }
        let db_path = dir.path().join("vectors.db");
        let index_path = dir.path().join("vectors.usearch");
        vs.save_index(&index_path).unwrap();
        drop(vs);

        let mut vs2 = VectorStorage::new(&db_path, &index_path).unwrap();
        let before = vs2.memory_count().unwrap();
        let report = dedup_by_source(&mut vs2, DEFAULT_THRESHOLD, None, None, None, false).unwrap();
        assert_eq!(report.sources_checked, 1);
        assert_eq!(report.deleted, 4);
        assert_eq!(vs2.memory_count().unwrap(), before - 4);
    }

    #[test]
    fn test_source_pattern_filter() {
        let (_dir, mut vs) = temp_storage();
        let text = "alpha project duplicate content for source filter";
        for _ in 0..5 {
            vs.add_memory(text, "w", "r", Some("alpha_project.txt"), None)
                .unwrap();
        }
        for _ in 0..5 {
            vs.add_memory(text, "w", "r", Some("beta_project.txt"), None)
                .unwrap();
        }

        let report = find_duplicates(&vs, DEFAULT_THRESHOLD, Some("alpha"), None, None).unwrap();
        assert_eq!(report.sources_checked, 1);
        assert_eq!(report.per_source[0].source, "alpha_project.txt");
    }

    #[test]
    fn test_wing_filter() {
        let (_dir, mut vs) = temp_storage();
        let text = "duplicate content for wing filter";
        for _ in 0..5 {
            vs.add_memory(text, "w1", "r", Some("src.txt"), None)
                .unwrap();
        }
        for _ in 0..5 {
            vs.add_memory(text, "w2", "r", Some("src.txt"), None)
                .unwrap();
        }

        let report = find_duplicates(&vs, DEFAULT_THRESHOLD, None, None, Some("w1")).unwrap();
        assert_eq!(report.sources_checked, 1);
        assert_eq!(report.per_source[0].source, "src.txt");
        assert_eq!(report.total_drawers, 5);
    }

    #[test]
    fn test_threshold_strict_vs_loose() {
        let (_dir, mut vs) = temp_storage();
        let text = "this is the exact same sentence used for strict threshold testing";
        for _ in 0..5 {
            vs.add_memory(text, "w", "r", Some("threshold_src.txt"), None)
                .unwrap();
        }

        let strict = find_duplicates(&vs, 0.0, None, None, None).unwrap();
        assert_eq!(
            strict.deleted, 0,
            "threshold 0.0 should keep all exact matches"
        );

        let loose = find_duplicates(&vs, 1.0, None, None, None).unwrap();
        assert_eq!(loose.deleted, 4, "threshold 1.0 should delete all but one");
    }

    #[test]
    fn test_min_count_override() {
        let (_dir, mut vs) = temp_storage();
        let text = "duplicate content for min count override";
        for _ in 0..4 {
            vs.add_memory(text, "w", "r", Some("small_src.txt"), None)
                .unwrap();
        }

        let report = find_duplicates(&vs, DEFAULT_THRESHOLD, None, Some(4), None).unwrap();
        assert_eq!(report.sources_checked, 1);
        assert_eq!(report.total_drawers, 4);
        assert_eq!(report.deleted, 3);
    }

    #[test]
    fn test_per_source_determinism() {
        let (_dir, mut vs) = temp_storage();
        let text = "deterministic duplicate content for ordering check";
        for _ in 0..6 {
            vs.add_memory(text, "w", "r", Some("order_src.txt"), None)
                .unwrap();
        }

        let r1 = find_duplicates(&vs, DEFAULT_THRESHOLD, None, None, None).unwrap();
        let r2 = find_duplicates(&vs, DEFAULT_THRESHOLD, None, None, None).unwrap();
        assert_eq!(r1.per_source, r2.per_source);
        assert_eq!(r1.per_source[0].kept_ids, r2.per_source[0].kept_ids);
        assert_eq!(r1.per_source[0].deleted_ids, r2.per_source[0].deleted_ids);
    }

    #[test]
    fn test_dedup_report_serializes() {
        let report = DedupReport {
            sources_checked: 1,
            total_drawers: 5,
            kept: 2,
            deleted: 3,
            per_source: vec![SourceDedupReport {
                source: "src.txt".to_string(),
                checked: 5,
                kept: 2,
                deleted: 3,
                kept_ids: vec![1, 2],
                deleted_ids: vec![3, 4, 5],
            }],
        };
        let json = serde_json::to_string(&report).unwrap();
        assert!(json.contains("src.txt"));
        assert!(json.contains("total_drawers"));
    }
}
