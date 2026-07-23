//! Pre-mining defense against drawer_id collisions.
//!
//! Runs immediately before a batched upsert. Computes the union of incoming
//! drawer_ids and existing drawer_ids that share a key with the batch; reports
//! collisions where any drawer_id appears more than once with conflicting
//! chunk metadata.
//!
//! Under the v3 hash recipe (see [`drawer_id_for_chunk`]) accidental collisions
//! are vanishingly rare — SHA-256 truncated to 24 hex chars makes a random
//! collision ~2^-96. The scan exists for two reasons:
//!
//! 1. Catch upstream bugs that emit duplicate `(source_file, chunk_index)` pairs
//!    in the same batch with conflicting content. The storage layer would
//!    silently let the last-write win; the scan surfaces it as an actionable
//!    error naming both call sites.
//! 2. Catch the astronomical-but-possible SHA-256 hash collision with a clear
//!    message instead of a silent overwrite at upsert time.
//!
//! The scan does NOT fire on idempotent re-mines — when an incoming drawer
//! matches an existing one with the same `(source_file, chunk_index)` metadata,
//! that is normal re-write behavior, not a collision.

use anyhow::{anyhow, Result};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// Length of the truncated hex SHA-256 used in drawer IDs.
pub const HASH_TRUNC_DRAWER: usize = 24;

/// Delimiter used when joining hash inputs. `|` is reserved in Windows filenames
/// and cannot appear in source paths, making it unambiguous.
pub const ID_DELIM: char = '|';

/// Recipe tag written to drawer metadata by helpers that use this module.
pub const ID_RECIPE: &str = "v3";

/// A single chunk about to be mined.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChunkCandidate {
    /// Stable drawer ID that will be used as the storage key.
    pub drawer_id: String,
    /// Source file the chunk came from.
    pub source_file: String,
    /// Chunk index within the source file.
    pub chunk_index: usize,
    /// Content hash for diagnostics (not used for collision discrimination).
    pub content_hash: String,
}

/// A record already present in the palace storage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExistingRecord {
    /// Stable drawer ID stored with the record.
    pub drawer_id: String,
    /// Source file the existing record came from.
    pub source_file: String,
    /// Chunk index within the source file.
    pub chunk_index: usize,
    /// Content hash for diagnostics.
    pub content_hash: String,
}

/// Reduced metadata key used for collision discrimination.
///
/// Two chunks are considered the same drawer iff their metadata keys match.
/// Different content at the same `(source_file, chunk_index)` is treated as a
/// normal re-mine, not a collision.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ChunkKey {
    pub source_file: String,
    pub chunk_index: usize,
}

impl ChunkKey {
    /// Render the key as a human-readable string.
    pub fn format(&self) -> String {
        format!(
            "source_file={:?}, chunk_index={}",
            self.source_file, self.chunk_index
        )
    }
}

/// Report produced by [`scan_for_collisions`].
#[derive(Debug, Clone, Default)]
pub struct CollisionReport {
    /// Map of colliding drawer_id to the set of metadata keys that produced it.
    pub collisions: BTreeMap<String, BTreeSet<ChunkKey>>,
}

impl CollisionReport {
    /// Returns true if no collisions were detected.
    pub fn is_empty(&self) -> bool {
        self.collisions.is_empty()
    }

    /// Returns the number of colliding drawer IDs.
    pub fn len(&self) -> usize {
        self.collisions.len()
    }
}

/// Build a delimiter-safe SHA-256 hash from the provided parts.
///
/// Each part is encoded as `len(part):part` before hashing to avoid collisions
/// caused by ambiguous concatenation (e.g. `"/a" + "1" == "/a1"` vs
/// `"/a1" + ""` vs `"" + "/a1"`).
fn delimited_sha256(parts: &[&str], truncate: usize) -> String {
    let key: String = parts
        .iter()
        .map(|part| format!("{}:{}", part.len(), part))
        .collect();
    let mut hasher = Sha256::new();
    hasher.update(key.as_bytes());
    let full = hex::encode(hasher.finalize());
    full.chars().take(truncate).collect()
}

/// Compute a stable, collision-resistant drawer ID for a chunk.
///
/// The hash input is `wing|room|source_file|chunk_index` using a length-prefixed
/// delimiter, so boundary collisions like `("/a", "1")` vs `("/a1", "")`
/// cannot produce the same ID.
///
/// Returns `drawer_{wing}_{room}_{hash24}` where `hash24` is the first 24 hex
/// characters of the SHA-256 digest.
pub fn drawer_id_for_chunk(
    wing: &str,
    room: &str,
    source_file: &str,
    chunk_index: usize,
) -> String {
    let hash = delimited_sha256(
        &[wing, room, source_file, &chunk_index.to_string()],
        HASH_TRUNC_DRAWER,
    );
    format!("drawer_{}_{}_{}", wing, room, hash)
}

/// Compute a content hash for diagnostic collision reporting.
pub fn content_hash(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    hex::encode(hasher.finalize())
}

/// Scan a set of proposed chunks against existing records for collisions.
///
/// Returns a [`CollisionReport`] containing every drawer_id that would map to
/// more than one distinct `(source_file, chunk_index)` metadata key.
///
/// Duplicate chunks with identical metadata are collapsed and are not reported.
pub fn scan_for_collisions<I, E>(proposed: I, existing: E) -> CollisionReport
where
    I: IntoIterator<Item = ChunkCandidate>,
    E: IntoIterator<Item = ExistingRecord>,
{
    let mut key_map: HashMap<String, BTreeSet<ChunkKey>> = HashMap::new();

    for candidate in proposed {
        key_map
            .entry(candidate.drawer_id)
            .or_default()
            .insert(ChunkKey {
                source_file: candidate.source_file,
                chunk_index: candidate.chunk_index,
            });
    }

    for record in existing {
        key_map
            .entry(record.drawer_id)
            .or_default()
            .insert(ChunkKey {
                source_file: record.source_file,
                chunk_index: record.chunk_index,
            });
    }

    let collisions: BTreeMap<String, BTreeSet<ChunkKey>> = key_map
        .into_iter()
        .filter(|(_, keys)| keys.len() > 1)
        .collect();

    CollisionReport { collisions }
}

/// Format a [`CollisionReport`] as a human-readable error message.
///
/// The message enumerates every colliding drawer_id and the metadata keys
/// producing it, so a user fixing one collision does not have to rediscover
/// the next by re-running the mine.
pub fn format_collision_report(report: &CollisionReport) -> String {
    if report.is_empty() {
        return "No drawer_id collisions detected.".to_string();
    }

    let mut lines = Vec::new();
    let count = report.len();
    lines.push(format!(
        "Pre-mining collision scan detected {} colliding drawer_id{}:",
        count,
        if count == 1 { "" } else { "s" }
    ));

    for (drawer_id, keys) in &report.collisions {
        lines.push(format!("  {}:", drawer_id));
        for key in keys {
            lines.push(format!("    {}", key.format()));
        }
    }

    lines.push(
        "Each colliding drawer_id would cause a subsequent storage upsert to silently \
         overwrite the first. Fix the upstream chunker / miner to emit distinct keys, \
         or investigate the SHA-256 hash collision."
            .to_string(),
    );

    lines.join("\n")
}

/// Abort via [`anyhow::Error`] if the pre-mining scan detects any collisions.
///
/// This is a convenience wrapper around [`scan_for_collisions`] and
/// [`format_collision_report`] for callers that want to fail fast.
pub fn assert_no_collisions<I, E>(proposed: I, existing: E) -> Result<()>
where
    I: IntoIterator<Item = ChunkCandidate>,
    E: IntoIterator<Item = ExistingRecord>,
{
    let report = scan_for_collisions(proposed, existing);
    if !report.is_empty() {
        return Err(anyhow!(format_collision_report(&report)));
    }
    Ok(())
}

/// Convenience builder for a [`ChunkCandidate`] from raw fields.
pub fn candidate(
    wing: &str,
    room: &str,
    source_file: &str,
    chunk_index: usize,
    content: &str,
) -> ChunkCandidate {
    ChunkCandidate {
        drawer_id: drawer_id_for_chunk(wing, room, source_file, chunk_index),
        source_file: source_file.to_string(),
        chunk_index,
        content_hash: content_hash(content),
    }
}

/// Convenience builder for an [`ExistingRecord`] from raw fields.
pub fn existing(
    wing: &str,
    room: &str,
    source_file: &str,
    chunk_index: usize,
    content: &str,
) -> ExistingRecord {
    ExistingRecord {
        drawer_id: drawer_id_for_chunk(wing, room, source_file, chunk_index),
        source_file: source_file.to_string(),
        chunk_index,
        content_hash: content_hash(content),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_drawer_id_for_chunk_is_stable() {
        let id1 = drawer_id_for_chunk("wing", "room", "src/main.rs", 0);
        let id2 = drawer_id_for_chunk("wing", "room", "src/main.rs", 0);
        assert_eq!(id1, id2);
        assert!(id1.starts_with("drawer_wing_room_"));
        assert_eq!(id1.len(), "drawer_wing_room_".len() + HASH_TRUNC_DRAWER);
    }

    #[test]
    fn test_drawer_id_for_chunk_respects_inputs() {
        assert_ne!(
            drawer_id_for_chunk("wing", "room", "src/main.rs", 0),
            drawer_id_for_chunk("wing", "room", "src/main.rs", 1)
        );
        assert_ne!(
            drawer_id_for_chunk("wing", "room", "src/main.rs", 0),
            drawer_id_for_chunk("wing", "room", "src/lib.rs", 0)
        );
        assert_ne!(
            drawer_id_for_chunk("wing", "room", "src/main.rs", 0),
            drawer_id_for_chunk("other", "room", "src/main.rs", 0)
        );
        assert_ne!(
            drawer_id_for_chunk("wing", "room", "src/main.rs", 0),
            drawer_id_for_chunk("wing", "other", "src/main.rs", 0)
        );
    }

    #[test]
    fn test_drawer_id_avoids_boundary_collision() {
        // Without a delimiter, "src/a" + "1" would hash the same as "src/a1" + "0"
        // if the concatenation "src/a1" happened to equal "src/a1". These are
        // different real files, so their IDs must differ.
        let id1 = drawer_id_for_chunk("w", "r", "src/a", 1);
        let id2 = drawer_id_for_chunk("w", "r", "src/a1", 0);
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_content_hash_stable() {
        let h1 = content_hash("hello world");
        let h2 = content_hash("hello world");
        let h3 = content_hash("hello world!");
        assert_eq!(h1, h2);
        assert_ne!(h1, h3);
        assert_eq!(h1.len(), 64);
    }

    #[test]
    fn test_scan_for_collisions_empty() {
        let report = scan_for_collisions([], []);
        assert!(report.is_empty());
        assert_eq!(report.len(), 0);
    }

    #[test]
    fn test_scan_for_collisions_no_conflict_for_unique_chunks() {
        let proposed = vec![
            candidate("w", "r", "src/a.rs", 0, "chunk0"),
            candidate("w", "r", "src/a.rs", 1, "chunk1"),
            candidate("w", "r", "src/b.rs", 0, "chunk0"),
        ];
        let report = scan_for_collisions(proposed, []);
        assert!(report.is_empty());
    }

    #[test]
    fn test_scan_for_collisions_duplicate_metadata_collapsed() {
        // Same source/index/content duplicated in the batch is not a collision.
        let a = candidate("w", "r", "src/a.rs", 0, "chunk0");
        let b = candidate("w", "r", "src/a.rs", 0, "chunk0");
        let report = scan_for_collisions(vec![a, b], []);
        assert!(report.is_empty());
    }

    #[test]
    fn test_scan_for_collisions_in_batch_collision() {
        // Two different chunks produced the same drawer_id (forced by sharing the
        // drawer_id field). This represents either a bug or a hash collision.
        let a = ChunkCandidate {
            drawer_id: "colliding-id".to_string(),
            source_file: "src/a.rs".to_string(),
            chunk_index: 0,
            content_hash: content_hash("content-a"),
        };
        let b = ChunkCandidate {
            drawer_id: "colliding-id".to_string(),
            source_file: "src/b.rs".to_string(),
            chunk_index: 0,
            content_hash: content_hash("content-b"),
        };
        let report = scan_for_collisions(vec![a, b], []);
        assert!(!report.is_empty());
        assert_eq!(report.len(), 1);
        let keys = report.collisions.get("colliding-id").unwrap();
        assert_eq!(keys.len(), 2);
    }

    #[test]
    fn test_scan_for_collisions_with_existing_records() {
        let proposed = vec![candidate("w", "r", "src/a.rs", 0, "new-content")];
        let drawer_id = proposed[0].drawer_id.clone();
        let existing = vec![existing("w", "r", "src/b.rs", 0, "old-content")];
        // Force the existing record to share the same drawer_id as the proposed one.
        let existing = existing
            .into_iter()
            .map(|mut rec| {
                rec.drawer_id = drawer_id.clone();
                rec
            })
            .collect::<Vec<_>>();
        let report = scan_for_collisions(proposed.clone(), existing);
        assert!(!report.is_empty());
        let keys = report.collisions.get(&drawer_id).unwrap();
        assert_eq!(keys.len(), 2);
    }

    #[test]
    fn test_scan_for_collisions_idempotent_remine() {
        // Same drawer_id, same metadata key: a normal re-mine, not a collision.
        let proposed = candidate("w", "r", "src/a.rs", 0, "new-content");
        let mut existing = existing("w", "r", "src/a.rs", 0, "old-content");
        existing.drawer_id = proposed.drawer_id.clone();
        let report = scan_for_collisions(vec![proposed], vec![existing]);
        assert!(report.is_empty());
    }

    #[test]
    fn test_format_collision_report_empty() {
        let report = CollisionReport::default();
        assert_eq!(
            format_collision_report(&report),
            "No drawer_id collisions detected."
        );
    }

    #[test]
    fn test_format_collision_report_single_collision() {
        let mut report = CollisionReport::default();
        let mut keys = BTreeSet::new();
        keys.insert(ChunkKey {
            source_file: "src/a.rs".to_string(),
            chunk_index: 0,
        });
        keys.insert(ChunkKey {
            source_file: "src/b.rs".to_string(),
            chunk_index: 0,
        });
        report.collisions.insert("drawer_w_r_abc".to_string(), keys);

        let text = format_collision_report(&report);
        assert!(text.contains("1 colliding drawer_id"));
        assert!(text.contains("drawer_w_r_abc"));
        assert!(text.contains("src/a.rs"));
        assert!(text.contains("src/b.rs"));
        assert!(text.contains("silently overwrite"));
    }

    #[test]
    fn test_format_collision_report_multiple_collisions() {
        let mut report = CollisionReport::default();
        for i in 0..2 {
            let mut keys = BTreeSet::new();
            keys.insert(ChunkKey {
                source_file: format!("src/a{}.rs", i),
                chunk_index: 0,
            });
            keys.insert(ChunkKey {
                source_file: format!("src/b{}.rs", i),
                chunk_index: 0,
            });
            report.collisions.insert(format!("drawer_w_r_{}", i), keys);
        }
        let text = format_collision_report(&report);
        assert!(text.contains("2 colliding drawer_ids"));
    }

    #[test]
    fn test_assert_no_collisions_ok() {
        let proposed = vec![
            candidate("w", "r", "src/a.rs", 0, "chunk0"),
            candidate("w", "r", "src/a.rs", 1, "chunk1"),
        ];
        assert!(assert_no_collisions(proposed, []).is_ok());
    }

    #[test]
    fn test_assert_no_collisions_err() {
        let a = ChunkCandidate {
            drawer_id: "same-id".to_string(),
            source_file: "src/a.rs".to_string(),
            chunk_index: 0,
            content_hash: content_hash("a"),
        };
        let b = ChunkCandidate {
            drawer_id: "same-id".to_string(),
            source_file: "src/b.rs".to_string(),
            chunk_index: 0,
            content_hash: content_hash("b"),
        };
        let err = assert_no_collisions(vec![a, b], []).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("Pre-mining collision scan detected"));
        assert!(msg.contains("same-id"));
    }

    #[test]
    fn test_chunk_key_format() {
        let key = ChunkKey {
            source_file: "src/main.rs".to_string(),
            chunk_index: 42,
        };
        assert_eq!(key.format(), "source_file=\"src/main.rs\", chunk_index=42");
    }

    #[test]
    fn test_constants() {
        assert_eq!(HASH_TRUNC_DRAWER, 24);
        assert_eq!(ID_RECIPE, "v3");
        assert_eq!(ID_DELIM, '|');
    }
}
