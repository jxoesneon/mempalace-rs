//! Hallways — within-wing entity-to-entity connectors.
//!
//! A **hallway** is a connection between two entities (people, projects,
//! concepts, interests) inside one wing, materialized from their
//! co-occurrence across that wing's drawers. This is the Rust forward-port
//! of the Python `mempalace.hallways` module.
//!
//! Persistence mirrors the upstream layout: a JSON file under the configured
//! MemPalace config directory so records survive across mines and are
//! inspectable / editable by hand if needed.

use crate::config::MempalaceConfig;
use crate::entity_detector::extract_entities;
use crate::shared::{load_json_file, save_json_file};
use crate::vector_storage::VectorStorage;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
#[cfg(test)]
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{debug, warn};

const SCHEMA_VERSION: i32 = 1;

type EntityPair = (Arc<str>, Arc<str>);

/// A single hallway record linking two entities within one wing.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Hallway {
    pub id: String,
    pub wing: String,
    pub entity_a: String,
    pub entity_b: String,
    pub co_occurrence_count: usize,
    pub rooms: Vec<String>,
    pub label: String,
    pub created_at: String,
    pub created_by: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub strength: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stability: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_activated: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub access_count: Option<u64>,
}

/// Persisted hallway file payload.
#[derive(Debug, Serialize, Deserialize)]
struct HallwayFile {
    schema_version: i32,
    hallways: Vec<Hallway>,
}

fn hallway_file(config: &MempalaceConfig) -> PathBuf {
    config.config_dir.join("hallways.json")
}

fn legacy_hallway_file() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join(".mempalace").join("hallways.json")
}

/// Load all hallway records from disk. Returns an empty vector if the file is
/// missing or corrupt.
pub fn load_hallways(config: &MempalaceConfig) -> Vec<Hallway> {
    let path = hallway_file(config);
    if path.exists() {
        match load_json_file::<serde_json::Value>(&path) {
            Ok(value) => {
                if let Ok(payload) = serde_json::from_value::<HallwayFile>(value.clone()) {
                    return payload.hallways;
                }
                if let Ok(list) = serde_json::from_value::<Vec<Hallway>>(value) {
                    return list;
                }
            }
            Err(e) => {
                debug!("hallways: load failed, treating as empty: {e}");
            }
        }
        return vec![];
    }

    let legacy = legacy_hallway_file();
    if legacy != path && legacy.exists() {
        warn!(
            "Legacy hallways file at '{}' is being ignored; configured location is '{}'. \
             Move or copy the legacy file to the configured path to recover its hallways.",
            legacy.display(),
            path.display()
        );
    }
    vec![]
}

/// Persist all hallway records atomically to the configured hallway file.
fn save_hallways(config: &MempalaceConfig, hallways: &[Hallway]) -> Result<()> {
    let path = hallway_file(config);
    let payload = HallwayFile {
        schema_version: SCHEMA_VERSION,
        hallways: hallways.to_vec(),
    };
    save_json_file(&path, &payload)
}

/// Deterministic short hallway id derived from wing + sorted entity pair.
fn hallway_id(wing: &str, entity_a: &str, entity_b: &str) -> String {
    let mut pair = [entity_a, entity_b];
    pair.sort();
    let key = format!("{}::{}::{}", wing, pair[0], pair[1]);
    let hash = Sha256::digest(key.as_bytes());
    let suffix = hex::encode(&hash[..4]);
    format!(
        "hallway_{}_{}_{}_{}",
        sanitize_id(wing),
        sanitize_id(pair[0]),
        sanitize_id(pair[1]),
        suffix
    )
}

fn sanitize_id(s: &str) -> String {
    s.to_lowercase()
        .replace([' ', '-'], "_")
        .replace(|c: char| !c.is_alphanumeric() && c != '_', "")
}

/// Parse a semicolon-separated entity list (mirrors the upstream behaviour).
///
/// If the value is missing, falls back to extracting entities from the drawer
/// content using the local entity detector.
pub fn parse_entities(value: Option<&str>, content: &str) -> Vec<String> {
    let items: Vec<String> = value
        .map(|v| {
            v.split(';')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default();

    if !items.is_empty() {
        return dedupe_preserve_order(items);
    }

    let detected = extract_entities(content);
    dedupe_preserve_order(detected.into_iter().map(|e| e.name).collect())
}

fn dedupe_preserve_order(items: Vec<String>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut result = Vec::new();
    for item in items {
        let normalized = item.to_lowercase();
        if seen.insert(normalized) {
            result.push(item);
        }
    }
    result
}

/// Internal entity parsing that returns `Arc<str>` so co-occurrence building can
/// share entity strings without repeated cloning.
fn parse_entities_arc(value: Option<&str>, content: &str) -> Vec<Arc<str>> {
    let items: Vec<Arc<str>> = value
        .map(|v| {
            v.split(';')
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .map(Arc::from)
                .collect()
        })
        .unwrap_or_default();

    if !items.is_empty() {
        return dedupe_preserve_order_arc(items);
    }

    let detected = extract_entities(content);
    dedupe_preserve_order_arc(detected.into_iter().map(|e| Arc::from(e.name)).collect())
}

fn dedupe_preserve_order_arc(items: Vec<Arc<str>>) -> Vec<Arc<str>> {
    let mut seen = std::collections::HashSet::new();
    let mut result = Vec::new();
    for item in items {
        let normalized = item.to_lowercase();
        if seen.insert(normalized) {
            result.push(item);
        }
    }
    result
}

/// Compute entity-pair hallways for one wing.
///
/// Scans all drawers in the wing, counts entity co-occurrences, and returns
/// hallway records for pairs that meet `min_count`. Other wings' existing
/// records on disk are preserved; this wing's records are replaced.
/// Default chunk size for `compute_hallways_for_wing`.  Keeps peak memory low
/// while still amortizing the per-chunk query overhead.
const HALLWAY_CHUNK_SIZE: usize = 1000;

pub fn compute_hallways_for_wing(
    config: &MempalaceConfig,
    wing: &str,
    vs: &VectorStorage,
    min_count: usize,
) -> Result<Vec<Hallway>> {
    let min_count = min_count.max(1);

    let mut pair_counts: std::collections::HashMap<EntityPair, usize> =
        std::collections::HashMap::new();
    let mut pair_rooms: std::collections::HashMap<EntityPair, BTreeSet<Arc<str>>> =
        std::collections::HashMap::new();

    for chunk in vs.get_memories_chunked(Some(wing), None, HALLWAY_CHUNK_SIZE) {
        let records = chunk?;
        for record in records {
            let entities = parse_entities_arc(None, &record.text_content);
            if entities.len() < 2 {
                continue;
            }
            let room: Arc<str> = Arc::from(record.room);
            for i in 0..entities.len() {
                for j in (i + 1)..entities.len() {
                    let a = &entities[i];
                    let b = &entities[j];
                    if a == b {
                        continue;
                    }
                    let (first, second) = if a.as_ref() <= b.as_ref() {
                        (a.clone(), b.clone())
                    } else {
                        (b.clone(), a.clone())
                    };
                    let key = (first, second);
                    *pair_counts.entry(key.clone()).or_insert(0) += 1;
                    pair_rooms.entry(key).or_default().insert(room.clone());
                }
            }
        }
    }

    let existing = load_hallways(config);
    let mut existing_dynamics: std::collections::HashMap<
        EntityPair,
        serde_json::Map<String, serde_json::Value>,
    > = std::collections::HashMap::new();
    for h in &existing {
        if h.wing != wing {
            continue;
        }
        let mut pair = [h.entity_a.clone(), h.entity_b.clone()];
        pair.sort();
        let key: (Arc<str>, Arc<str>) = (Arc::from(pair[0].clone()), Arc::from(pair[1].clone()));
        let mut preserved = serde_json::Map::new();
        for field in ["strength", "stability", "last_activated", "access_count"] {
            if let serde_json::Value::Object(map) = serde_json::to_value(h).unwrap() {
                if let Some(v) = map.get(field) {
                    preserved.insert(field.to_string(), v.clone());
                }
            }
        }
        existing_dynamics.insert(key, preserved);
    }

    let mut created: Vec<Hallway> = Vec::new();
    let created_at = chrono::Utc::now().to_rfc3339();

    let mut keys: Vec<EntityPair> = pair_counts.keys().cloned().collect();
    keys.sort_by(|a, b| (a.0.as_ref(), a.1.as_ref()).cmp(&(b.0.as_ref(), b.1.as_ref())));
    for key in keys {
        let count = pair_counts[&key];
        if count < min_count {
            continue;
        }
        let (entity_a_arc, entity_b_arc) = key.clone();
        let entity_a = entity_a_arc.to_string();
        let entity_b = entity_b_arc.to_string();
        let rooms: Vec<String> = pair_rooms
            .get(&key)
            .map(|s| s.iter().map(|r| r.to_string()).collect())
            .unwrap_or_default();
        let room_summary = if rooms.is_empty() {
            "(no room tags)".to_string()
        } else {
            let head: Vec<_> = rooms.iter().take(3).cloned().collect();
            let mut summary = head.join(", ");
            if rooms.len() > 3 {
                summary.push_str(&format!(", +{} more", rooms.len() - 3));
            }
            summary
        };
        let room_word = if rooms.len() == 1 { "room" } else { "rooms" };
        let label = format!(
            "{} ↔ {} (co-occur in {} drawers across {} {}: {})",
            entity_a,
            entity_b,
            count,
            if rooms.is_empty() {
                "no".to_string()
            } else {
                rooms.len().to_string()
            },
            room_word,
            room_summary
        );

        let id = hallway_id(wing, &entity_a, &entity_b);
        let mut record = Hallway {
            id,
            wing: wing.to_string(),
            entity_a,
            entity_b,
            co_occurrence_count: count,
            rooms,
            label,
            created_at: created_at.clone(),
            created_by: "auto".to_string(),
            strength: None,
            stability: None,
            last_activated: None,
            access_count: None,
        };

        if let Some(preserved) = existing_dynamics.get(&key) {
            for (k, v) in preserved {
                match k.as_str() {
                    "strength" => record.strength = v.as_f64(),
                    "stability" => record.stability = v.as_f64(),
                    "last_activated" => record.last_activated = v.as_i64(),
                    "access_count" => record.access_count = v.as_u64(),
                    _ => {}
                }
            }
        }
        let now = chrono::Utc::now().timestamp();
        if record.strength.is_none()
            || record.stability.is_none()
            || record.last_activated.is_none()
            || record.access_count.is_none()
        {
            let mut dyn_state = crate::dynamics::MemoryDynamics::new();
            dyn_state.initialize(now);
            record.strength = Some(record.strength.unwrap_or(dyn_state.strength));
            record.stability = Some(record.stability.unwrap_or(dyn_state.stability));
            record.last_activated = Some(record.last_activated.unwrap_or(dyn_state.last_activated));
            record.access_count = Some(record.access_count.unwrap_or(dyn_state.access_count));
        }
        created.push(record);
    }

    let preserved_other_wings: Vec<Hallway> =
        existing.into_iter().filter(|h| h.wing != wing).collect();
    save_hallways(config, &[preserved_other_wings, created.clone()].concat())?;

    Ok(created)
}

/// List hallway records, optionally filtered by wing.
pub fn list_hallways(config: &MempalaceConfig, wing: Option<&str>) -> Vec<Hallway> {
    let all = load_hallways(config);
    match wing {
        Some(w) => all.into_iter().filter(|h| h.wing == w).collect(),
        None => all,
    }
}

/// Remove one hallway record by id. Returns true if a record was removed.
pub fn delete_hallway(config: &MempalaceConfig, hallway_id: &str) -> Result<bool> {
    let hallways = load_hallways(config);
    let original_len = hallways.len();
    let filtered: Vec<Hallway> = hallways
        .into_iter()
        .filter(|h| h.id != hallway_id)
        .collect();
    if filtered.len() == original_len {
        return Ok(false);
    }
    save_hallways(config, &filtered)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::MempalaceConfig;
    use crate::vector_storage::VectorStorage;
    use serde_json::json;
    use tempfile::tempdir;

    fn test_config() -> (tempfile::TempDir, MempalaceConfig) {
        let dir = tempdir().unwrap();
        let config = MempalaceConfig::new(Some(dir.path().to_path_buf()));
        (dir, config)
    }

    fn add_test_memory(
        vs: &mut VectorStorage,
        dir: &std::path::Path,
        wing: &str,
        room: &str,
        content: &str,
    ) {
        vs.add_memory(content, wing, room, None, None).unwrap();
        vs.save_index(dir.join("vectors.usearch")).unwrap();
    }

    #[test]
    fn test_hallway_id_is_symmetric() {
        let a = hallway_id("diary", "Aya", "Lumi");
        let b = hallway_id("diary", "Lumi", "Aya");
        assert_eq!(a, b);
        assert!(a.starts_with("hallway_diary_aya_lumi_"));
    }

    #[test]
    fn test_parse_entities_from_string() {
        let got = parse_entities(Some("Aya; Lumi; consciousness"), "ignored");
        assert_eq!(got, vec!["Aya", "Lumi", "consciousness"]);
    }

    #[test]
    fn test_parse_entities_dedupes() {
        let got = parse_entities(Some("Aya;Aya;Lumi"), "");
        assert_eq!(got, vec!["Aya", "Lumi"]);
    }

    #[test]
    fn test_list_and_delete_hallway() {
        let (_dir, config) = test_config();
        let h1 = Hallway {
            id: "hallway_test_a_b_12345678".to_string(),
            wing: "test".to_string(),
            entity_a: "A".to_string(),
            entity_b: "B".to_string(),
            co_occurrence_count: 3,
            rooms: vec!["room1".to_string()],
            label: "A ↔ B".to_string(),
            created_at: "2024-01-01T00:00:00Z".to_string(),
            created_by: "test".to_string(),
            strength: None,
            stability: None,
            last_activated: None,
            access_count: None,
        };
        save_hallways(&config, &[h1.clone()]).unwrap();

        let listed = list_hallways(&config, None);
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, "hallway_test_a_b_12345678");

        let filtered = list_hallways(&config, Some("other"));
        assert!(filtered.is_empty());

        let deleted = delete_hallway(&config, "hallway_test_a_b_12345678").unwrap();
        assert!(deleted);
        assert!(list_hallways(&config, None).is_empty());

        let not_found = delete_hallway(&config, "missing").unwrap();
        assert!(!not_found);
    }

    #[test]
    fn test_compute_hallways_for_wing() {
        let (dir, config) = test_config();
        let mut vs = VectorStorage::new(
            dir.path().join("vectors.db"),
            dir.path().join("vectors.usearch"),
        )
        .unwrap();

        add_test_memory(
            &mut vs,
            dir.path(),
            "diary",
            "2024-01-01",
            "Aya and Lumi and Aya and Lumi and Aya and Lumi talked about the project. Aya was happy.",
        );
        add_test_memory(
            &mut vs,
            dir.path(),
            "diary",
            "2024-01-02",
            "Lumi and Aya and Lumi and Aya and Lumi and Aya reviewed the project together.",
        );

        let hallways = compute_hallways_for_wing(&config, "diary", &vs, 2).unwrap();
        assert!(!hallways.is_empty());
        let ids: Vec<_> = hallways.iter().map(|h| h.id.clone()).collect();
        let unique: std::collections::HashSet<_> = ids.iter().cloned().collect();
        assert_eq!(ids.len(), unique.len(), "hallway ids should be unique");

        let loaded = list_hallways(&config, Some("diary"));
        assert_eq!(loaded.len(), hallways.len());
    }

    #[test]
    fn test_compute_preserves_other_wings() {
        let (dir, config) = test_config();
        let other = Hallway {
            id: "hallway_other_x_y_00000000".to_string(),
            wing: "other".to_string(),
            entity_a: "X".to_string(),
            entity_b: "Y".to_string(),
            co_occurrence_count: 5,
            rooms: vec!["r".to_string()],
            label: "X ↔ Y".to_string(),
            created_at: "2024-01-01T00:00:00Z".to_string(),
            created_by: "test".to_string(),
            strength: None,
            stability: None,
            last_activated: None,
            access_count: None,
        };
        save_hallways(&config, &[other.clone()]).unwrap();

        let mut vs = VectorStorage::new(
            dir.path().join("vectors.db"),
            dir.path().join("vectors.usearch"),
        )
        .unwrap();
        add_test_memory(
            &mut vs,
            dir.path(),
            "diary",
            "d1",
            "Alice and Bob and Alice and Bob and Alice and Bob had coffee.",
        );
        compute_hallways_for_wing(&config, "diary", &vs, 1).unwrap();

        let all = list_hallways(&config, None);
        assert!(all.iter().any(|h| h.wing == "other"));
        assert!(all.iter().any(|h| h.wing == "diary"));
    }

    #[test]
    fn test_save_hallways_schema_version() {
        let (_dir, config) = test_config();
        save_hallways(&config, &[]).unwrap();
        let content = fs::read_to_string(hallway_file(&config)).unwrap();
        let value: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert_eq!(value["schema_version"], json!(SCHEMA_VERSION));
        assert_eq!(value["hallways"], json!([]));
    }
}
