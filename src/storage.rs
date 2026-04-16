use crate::config::MempalaceConfig;
use crate::models::Wing;
use crate::vector_storage::VectorStorage;
use anyhow::{anyhow, Result};
use rusqlite::{params, Connection, Result as SqlResult};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

fn open_vector_storage(config: &MempalaceConfig) -> anyhow::Result<VectorStorage> {
    VectorStorage::new(
        config.config_dir.join("vectors.db"),
        config.config_dir.join("vectors.usearch"),
    )
}

/// Primary storage engine for managing structured Palace data and wings.
pub struct Storage {
    pub conn: Connection,
}

#[derive(Debug, Clone, Default)]
pub struct Layer0 {
    pub path: PathBuf,
    pub text: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct PruneReport {
    pub clusters_found: usize,
    pub merged: usize,
    pub tokens_saved_est: usize,
}

impl Layer0 {
    pub fn new(path: Option<PathBuf>) -> Self {
        let path = path.unwrap_or_else(|| {
            let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
            PathBuf::from(&home).join(".mempalace").join("identity.txt")
        });
        Self { path, text: None }
    }

    pub fn format_render(path_exists: bool, content: Option<String>) -> String {
        if path_exists {
            if let Some(content_str) = content {
                return content_str.trim().to_string();
            }
        }
        "## L0 — IDENTITY\nNo identity configured. Create ~/.mempalace/identity.txt".to_string()
    }

    pub fn render(&mut self) -> String {
        if let Some(text) = &self.text {
            return text.clone();
        }

        let content = if self.path.exists() {
            fs::read_to_string(&self.path).ok()
        } else {
            None
        };

        let rendered = Self::format_render(self.path.exists(), content);
        self.text = Some(rendered.clone());
        rendered
    }
}

pub struct Layer1 {
    pub config: MempalaceConfig,
    pub wing: Option<String>,
}

impl Layer1 {
    pub fn new(config: MempalaceConfig, wing: Option<String>) -> Self {
        Self { config, wing }
    }

    pub fn build_where_clause(
        wing: Option<&String>,
        room: Option<&String>,
    ) -> Option<serde_json::Value> {
        let mut where_clause = HashMap::new();
        if let (Some(w), Some(r)) = (wing, room) {
            let mut and_vec = Vec::new();
            let mut w_map = HashMap::new();
            w_map.insert("wing".to_string(), serde_json::Value::String(w.clone()));
            and_vec.push(serde_json::Value::Object(w_map.into_iter().collect()));

            let mut r_map = HashMap::new();
            r_map.insert("room".to_string(), serde_json::Value::String(r.clone()));
            and_vec.push(serde_json::Value::Object(r_map.into_iter().collect()));

            where_clause.insert("$and".to_string(), serde_json::Value::Array(and_vec));
        } else if let Some(w) = wing {
            where_clause.insert("wing".to_string(), serde_json::Value::String(w.clone()));
        } else if let Some(r) = room {
            where_clause.insert("room".to_string(), serde_json::Value::String(r.clone()));
        }

        if where_clause.is_empty() {
            None
        } else {
            serde_json::to_value(where_clause).ok()
        }
    }

    pub async fn generate(&self) -> String {
        let vs = match open_vector_storage(&self.config) {
            Ok(vs) => vs,
            Err(_) => return "## L1 — Vector storage unavailable.".to_string(),
        };
        let records = match vs.get_memories(self.wing.as_deref(), None, 100) {
            Ok(r) => r,
            Err(_) => return "## L1 — Error fetching memories.".to_string(),
        };
        if records.is_empty() {
            return "## L1 — No memories yet.".to_string();
        }

        // Call touch_memory for each retrieved record to update access patterns
        for r in &records {
            let _ = vs.touch_memory(r.id);
        }

        let docs: Vec<String> = records.iter().map(|r| r.text_content.clone()).collect();
        let metas: Vec<Option<serde_json::Map<String, serde_json::Value>>> = records
            .iter()
            .map(|r| {
                let mut m = serde_json::Map::new();
                m.insert(
                    "importance".to_string(),
                    serde_json::Value::Number(
                        serde_json::Number::from_f64(r.importance as f64)
                            .unwrap_or_else(|| serde_json::Number::from(0)),
                    ),
                );
                m.insert(
                    "wing".to_string(),
                    serde_json::Value::String(r.wing.clone()),
                );
                m.insert(
                    "room".to_string(),
                    serde_json::Value::String(r.room.clone()),
                );
                if let Some(sf) = &r.source_file {
                    m.insert(
                        "source_file".to_string(),
                        serde_json::Value::String(sf.clone()),
                    );
                }
                Some(m)
            })
            .collect();
        let dialect = crate::dialect::Dialect::default();
        dialect.generate_layer1(&docs, &metas)
    }
}

pub struct Layer2 {
    pub config: MempalaceConfig,
}

impl Layer2 {
    pub fn new(config: MempalaceConfig) -> Self {
        Self { config }
    }

    pub fn format_retrieval(
        wing: Option<&String>,
        room: Option<&String>,
        docs: &[Option<String>],
        metas: &[Option<serde_json::Map<String, serde_json::Value>>],
    ) -> String {
        if docs.is_empty() {
            let label = if let (Some(w), Some(r)) = (wing, room) {
                format!("wing={} room={}", w, r)
            } else if let Some(w) = wing {
                format!("wing={}", w)
            } else if let Some(r) = room {
                format!("room={}", r)
            } else {
                "general".to_string()
            };
            return format!("No drawers found for {}.", label);
        }

        let mut lines = vec![format!("## L2 — ON-DEMAND ({} drawers)", docs.len())];
        for (doc, meta) in docs.iter().zip(metas.iter()) {
            let room_name = meta
                .as_ref()
                .and_then(|m| m.get("room"))
                .and_then(|v| v.as_str())
                .unwrap_or("?");
            let source = meta
                .as_ref()
                .and_then(|m| m.get("source_file"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let source_name = PathBuf::from(source)
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string();

            let mut snippet = if let Some(d) = doc {
                d.trim().replace('\n', " ")
            } else {
                "".to_string()
            };
            if snippet.len() > 300 {
                snippet = format!("{}...", &snippet[..297]);
            }

            let importance = meta
                .as_ref()
                .and_then(|m| m.get("importance"))
                .and_then(|v| v.as_f64())
                .unwrap_or(3.0) as f32;
            let weight = (importance * 2.0).round().min(9.0) as u8;

            let mut entry = format!("  [{}] WT:{}| {}", room_name, weight, snippet);
            if !source_name.is_empty() {
                entry = format!("{}  ({})", entry, source_name);
            }
            lines.push(entry);
        }

        lines.join("\n")
    }

    pub async fn retrieve(
        &self,
        wing: Option<String>,
        room: Option<String>,
        n_results: usize,
    ) -> String {
        let vs = match open_vector_storage(&self.config) {
            Ok(vs) => vs,
            Err(_) => return "Vector storage unavailable.".to_string(),
        };
        let records = match vs.get_memories(wing.as_deref(), room.as_deref(), n_results) {
            Ok(r) => r,
            Err(e) => return format!("Retrieval error: {}", e),
        };
        if records.is_empty() {
            return Self::format_retrieval(wing.as_ref(), room.as_ref(), &[], &[]);
        }

        // Call touch_memory for retrieved records
        for r in &records {
            let _ = vs.touch_memory(r.id);
        }

        let docs: Vec<Option<String>> = records
            .iter()
            .map(|r| Some(r.text_content.clone()))
            .collect();
        let metas: Vec<Option<serde_json::Map<String, serde_json::Value>>> = records
            .iter()
            .map(|r| {
                let mut m = serde_json::Map::new();
                m.insert(
                    "importance".to_string(),
                    serde_json::Value::Number(
                        serde_json::Number::from_f64(r.importance as f64)
                            .unwrap_or_else(|| serde_json::Number::from(0)),
                    ),
                );
                m.insert(
                    "room".to_string(),
                    serde_json::Value::String(r.room.clone()),
                );
                if let Some(sf) = &r.source_file {
                    m.insert(
                        "source_file".to_string(),
                        serde_json::Value::String(sf.clone()),
                    );
                }
                Some(m)
            })
            .collect();
        Self::format_retrieval(wing.as_ref(), room.as_ref(), &docs, &metas)
    }
}

pub struct Layer3 {
    pub config: MempalaceConfig,
}

impl Layer3 {
    pub fn new(config: MempalaceConfig) -> Self {
        Self { config }
    }

    pub fn format_search(
        query: &str,
        docs: &[String],
        metas: &[Option<serde_json::Map<String, serde_json::Value>>],
        dists: &[f32],
    ) -> String {
        if docs.is_empty() || docs[0].is_empty() {
            return "No results found.".to_string();
        }

        let mut lines = vec![format!("## L3 — SEARCH RESULTS for \"{}\"", query)];
        for i in 0..docs.len() {
            let doc = &docs[i];
            let meta = &metas[i];
            let dist = dists[i];

            let similarity = 1.0 - dist;
            let wing_name = meta
                .as_ref()
                .and_then(|m| m.get("wing"))
                .and_then(|v| v.as_str())
                .unwrap_or("?");
            let room_name = meta
                .as_ref()
                .and_then(|m| m.get("room"))
                .and_then(|v| v.as_str())
                .unwrap_or("?");
            let source = meta
                .as_ref()
                .and_then(|m| m.get("source_file"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let source_name = PathBuf::from(source)
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string();

            let mut snippet = doc.trim().replace('\n', " ");
            if snippet.len() > 300 {
                snippet = format!("{}...", &snippet[..297]);
            }

            let importance = meta
                .as_ref()
                .and_then(|m| m.get("importance"))
                .and_then(|v| v.as_f64())
                .unwrap_or(3.0) as f32;
            let weight = (importance * 2.0).round().min(9.0) as u8;

            lines.push(format!(
                "  [{}] {}/{} (sim={:.3}, wt={})",
                i + 1,
                wing_name,
                room_name,
                similarity,
                weight
            ));
            lines.push(format!("      {}", snippet));
            if !source_name.is_empty() {
                lines.push(format!("      src: {}", source_name));
            }
        }

        lines.join("\n")
    }

    pub async fn search(
        &self,
        query: &str,
        wing: Option<String>,
        room: Option<String>,
        n_results: usize,
    ) -> String {
        let vs = match open_vector_storage(&self.config) {
            Ok(vs) => vs,
            Err(_) => return "Vector storage unavailable.".to_string(),
        };
        let records = match (&wing, &room) {
            (Some(w), Some(r)) => vs.search_room(query, w, r, n_results, None),
            _ => vs.search(query, n_results),
        };
        let records = match records {
            Ok(r) => r,
            Err(e) => return format!("Search error: {}", e),
        };
        if records.is_empty() {
            return Self::format_search(query, &[], &[], &[]);
        }

        // Call touch_memory for retrieved records
        for r in &records {
            let _ = vs.touch_memory(r.id);
        }

        let docs: Vec<String> = records.iter().map(|r| r.text_content.clone()).collect();
        let metas: Vec<Option<serde_json::Map<String, serde_json::Value>>> = records
            .iter()
            .map(|r| {
                let mut m = serde_json::Map::new();
                m.insert(
                    "importance".to_string(),
                    serde_json::Value::Number(
                        serde_json::Number::from_f64(r.importance as f64)
                            .unwrap_or_else(|| serde_json::Number::from(0)),
                    ),
                );
                m.insert(
                    "wing".to_string(),
                    serde_json::Value::String(r.wing.clone()),
                );
                m.insert(
                    "room".to_string(),
                    serde_json::Value::String(r.room.clone()),
                );
                if let Some(sf) = &r.source_file {
                    m.insert(
                        "source_file".to_string(),
                        serde_json::Value::String(sf.clone()),
                    );
                }
                Some(m)
            })
            .collect();
        let dists: Vec<f32> = records.iter().map(|r| 1.0 - r.score).collect();
        Self::format_search(query, &docs, &metas, &dists)
    }
}

pub struct MemoryStack {
    pub l0: Layer0,
    pub l1: Layer1,
    pub l2: Layer2,
    pub l3: Layer3,
    pub config: MempalaceConfig,
}

impl MemoryStack {
    pub fn new(config: MempalaceConfig) -> Self {
        let identity_path = config.config_dir.join("identity.txt");
        Self {
            l0: Layer0::new(Some(identity_path)),
            l1: Layer1::new(config.clone(), None),
            l2: Layer2::new(config.clone()),
            l3: Layer3::new(config.clone()),
            config,
        }
    }

    pub fn format_wake_up(l0: String, l1: String) -> String {
        [l0, "".to_string(), l1].join("\n")
    }

    pub async fn wake_up(&mut self, wing: Option<String>) -> String {
        let l0_render = self.l0.render();
        if wing.is_some() {
            self.l1.wing = wing;
        }
        let l1_render = self.l1.generate().await;

        Self::format_wake_up(l0_render, l1_render)
    }

    pub async fn recall(
        &self,
        wing: Option<String>,
        room: Option<String>,
        n_results: usize,
    ) -> String {
        self.l2.retrieve(wing, room, n_results).await
    }

    pub async fn search(
        &self,
        query: &str,
        wing: Option<String>,
        room: Option<String>,
        n_results: usize,
    ) -> String {
        self.l3.search(query, wing, room, n_results).await
    }

    pub async fn repair(&self, config: &MempalaceConfig) -> Result<()> {
        let mut vs = open_vector_storage(config)?;

        println!("  Rebuilding usearch index from SQLite metadata...");
        let mut stmt = vs.db.prepare("SELECT id, text_content FROM memories")?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })?;

        let opts = usearch::IndexOptions {
            dimensions: 384,
            metric: usearch::MetricKind::Cos,
            quantization: usearch::ScalarKind::F32,
            connectivity: 16,
            expansion_add: 128,
            expansion_search: 64,
            ..Default::default()
        };
        let new_index = usearch::Index::new(&opts)
            .map_err(|e| anyhow!("usearch index creation failed: {e}"))?;

        let mut count = 0;
        for row in rows {
            let (id, text) = row?;
            let vector = vs.embed_single(&text)?;

            let needed = new_index.size() + 1;
            if needed > new_index.capacity() {
                let new_cap = (needed * 2).max(64);
                new_index
                    .reserve(new_cap)
                    .map_err(|e| anyhow!("usearch reserve failed: {e}"))?;
            }
            new_index
                .add(id as u64, &vector)
                .map_err(|e| anyhow!("usearch add failed: {e}"))?;
            count += 1;
        }

        vs.index = new_index;
        vs.save_index(config.config_dir.join("vectors.usearch"))?;

        println!(
            "  ✓ Successfully repaired and re-indexed {} memories.",
            count
        );
        Ok(())
    }
}

impl Storage {
    pub async fn repair(&self, config: &MempalaceConfig) -> Result<()> {
        let mut vs = open_vector_storage(config)?;

        println!("  Rebuilding usearch index from SQLite metadata...");
        let mut stmt = vs.db.prepare("SELECT id, text_content FROM memories")?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })?;

        let opts = usearch::IndexOptions {
            dimensions: 384,
            metric: usearch::MetricKind::Cos,
            quantization: usearch::ScalarKind::F32,
            connectivity: 16,
            expansion_add: 128,
            expansion_search: 64,
            ..Default::default()
        };
        let new_index = usearch::Index::new(&opts)
            .map_err(|e| anyhow!("usearch index creation failed: {e}"))?;

        let mut count = 0;
        for row in rows {
            let (id, text) = row?;
            let vector = vs.embed_single(&text)?;

            let needed = new_index.size() + 1;
            if needed > new_index.capacity() {
                let new_cap = (needed * 2).max(64);
                new_index
                    .reserve(new_cap)
                    .map_err(|e| anyhow!("usearch reserve failed: {e}"))?;
            }
            new_index
                .add(id as u64, &vector)
                .map_err(|e| anyhow!("usearch add failed: {e}"))?;
            count += 1;
        }

        vs.index = new_index;
        vs.save_index(config.config_dir.join("vectors.usearch"))?;

        println!(
            "  ✓ Successfully repaired and re-indexed {} memories.",
            count
        );
        Ok(())
    }

    pub fn new(path: &str) -> SqlResult<Self> {
        // Create parent directory if it doesn't exist (for non-memory databases)
        if path != ":memory:" {
            if let Some(parent) = std::path::Path::new(path).parent() {
                let _ = std::fs::create_dir_all(parent);
            }
        }
        let conn = Connection::open(path)?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS wings (
                id INTEGER PRIMARY KEY,
                name TEXT NOT NULL UNIQUE,
                type TEXT NOT NULL
            )",
            [],
        )?;
        Ok(Storage { conn })
    }

    pub fn add_wing(&self, wing: &Wing) -> SqlResult<()> {
        self.conn.execute(
            "INSERT INTO wings (name, type) VALUES (?1, ?2)",
            params![wing.name, wing.r#type],
        )?;
        Ok(())
    }

    pub async fn status(&self, config: &MempalaceConfig) -> Result<()> {
        let mut stmt = self.conn.prepare("SELECT name, type FROM wings")?;
        let wing_rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;

        let mut wings = Vec::new();
        for wing in wing_rows {
            wings.push(wing?);
        }

        let (count, rooms) = match open_vector_storage(config) {
            Ok(vs) => {
                let count = vs.memory_count().unwrap_or(0);
                let rooms: std::collections::HashSet<String> = vs
                    .get_wings_rooms()
                    .unwrap_or_default()
                    .into_iter()
                    .map(|(_, r)| r)
                    .collect();
                (count, rooms)
            }
            Err(_) => (0, std::collections::HashSet::new()),
        };

        println!("\n  🏠 Palace Status");
        println!("  {}", "─".repeat(20));
        println!("  Total Drawers: {}", count);
        println!(
            "  Wings Filed:   {:?}",
            wings.iter().map(|(n, _)| n).collect::<Vec<_>>()
        );
        println!(
            "  Rooms Found:   {:?}",
            rooms.into_iter().collect::<Vec<_>>()
        );
        println!();

        Ok(())
    }

    pub async fn compress_drawers(
        &self,
        config: &MempalaceConfig,
        wing: Option<String>,
    ) -> Result<()> {
        if let Ok(vs) = open_vector_storage(config) {
            if let Ok(records) = vs.get_memories(wing.as_deref(), None, usize::MAX) {
                println!("\n  🗜  Compressing Drawers");
                println!("  {}", "─".repeat(24));
                for record in records {
                    let compressed = crate::dialect::AAAKContext::compress(&record.text_content);
                    println!("  {}", compressed);
                }
                println!();
            }
        }

        Ok(())
    }

    pub async fn prune_memories(
        &self,
        config: &MempalaceConfig,
        threshold: f32,
        dry_run: bool,
        wing: Option<String>,
    ) -> Result<PruneReport> {
        let vs = open_vector_storage(config)?;
        let dialect = crate::dialect::Dialect::default();

        let ids = vs.get_all_ids(wing.as_deref())?;
        let mut processed = std::collections::HashSet::new();
        let mut report = PruneReport {
            clusters_found: 0,
            merged: 0,
            tokens_saved_est: 0,
        };

        for id in ids {
            if processed.contains(&id) {
                continue;
            }

            let record = match vs.get_memory_by_id(id) {
                Ok(r) => r,
                Err(_) => continue,
            };

            let vec = vs.embed_single(&record.text_content)?;
            let neighbors = vs
                .index
                .search(&vec, 10)
                .map_err(|e| anyhow!("Search failed: {e}"))?;

            let mut cluster = Vec::new();
            cluster.push(record.clone());
            processed.insert(id);

            for i in 0..neighbors.keys.len() {
                let neighbor_id = neighbors.keys[i] as i64;
                let distance = neighbors.distances[i];

                if distance < (1.0 - threshold) && !processed.contains(&neighbor_id) {
                    if let Ok(neighbor_rec) = vs.get_memory_by_id(neighbor_id) {
                        // Check if it belongs to the same wing (if wing filter is active)
                        if wing
                            .as_ref()
                            .map(|w| *w == neighbor_rec.wing)
                            .unwrap_or(true)
                        {
                            cluster.push(neighbor_rec);
                            processed.insert(neighbor_id);
                        }
                    }
                }
            }

            if cluster.len() > 1 {
                report.clusters_found += 1;
                report.merged += cluster.len() - 1;

                // Pick winner (highest decayed importance)
                cluster.sort_by(|a, b| {
                    b.importance
                        .partial_cmp(&a.importance)
                        .unwrap_or(std::cmp::Ordering::Equal)
                });
                let winner = &cluster[0];
                let losers = &cluster[1..];

                let aaaks: Vec<String> = cluster.iter().map(|c| c.text_content.clone()).collect();
                let merged_aaak = dialect.merge_aaaks(&aaaks);

                if !dry_run {
                    vs.update_memory_summary(winner.id, &merged_aaak)?;
                    for loser in losers {
                        let _ = vs.delete_memory(loser.id);
                    }
                }

                // Estimate tokens saved (rough estimate: characters / 4)
                let total_chars: usize = losers.iter().map(|l| l.text_content.len()).sum();
                report.tokens_saved_est += total_chars / 4;
            }
        }

        Ok(report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_layer0_format_render() {
        let res = Layer0::format_render(false, None);
        assert!(res.contains("No identity configured"));

        let res2 = Layer0::format_render(true, Some("  test identity  \n".to_string()));
        assert_eq!(res2, "test identity");

        let res3 = Layer0::format_render(true, None);
        assert!(res3.contains("No identity configured"));
    }

    #[test]
    fn test_layer1_build_where_clause() {
        let wing = "eng".to_string();
        let room = "rust".to_string();

        assert_eq!(Layer1::build_where_clause(None, None), None);

        let wc_wing = Layer1::build_where_clause(Some(&wing), None).unwrap();
        assert_eq!(wc_wing["wing"], "eng");

        let wc_room = Layer1::build_where_clause(None, Some(&room)).unwrap();
        assert_eq!(wc_room["room"], "rust");

        let wc_both = Layer1::build_where_clause(Some(&wing), Some(&room)).unwrap();
        assert_eq!(wc_both["$and"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn test_layer1_format_generation_empty() {
        let dialect = crate::dialect::Dialect::default();
        assert_eq!(
            dialect.generate_layer1(&[], &[]),
            "## L1 — No memories yet."
        );
    }

    #[test]
    fn test_layer1_format_generation_with_data() {
        let docs = vec![
            "Important architectural decision...".to_string(),
            "A very long document...".repeat(10), // > 200 chars to test truncating
        ];

        let mut m1 = serde_json::Map::new();
        m1.insert(
            "room".to_string(),
            serde_json::Value::String("arch".to_string()),
        );
        m1.insert(
            "source_file".to_string(),
            serde_json::Value::String("/path/to/arch.md".to_string()),
        );
        m1.insert(
            "importance".to_string(),
            serde_json::Value::Number(serde_json::Number::from_f64(5.0).unwrap()),
        );

        let mut m2 = serde_json::Map::new();
        m2.insert(
            "room".to_string(),
            serde_json::Value::String("general".to_string()),
        );
        m2.insert(
            "weight".to_string(),
            serde_json::Value::Number(serde_json::Number::from_f64(1.0).unwrap()),
        );

        let metas = vec![Some(m1), Some(m2)];

        let dialect = crate::dialect::Dialect::default();
        let res = dialect.generate_layer1(&docs, &metas);
        assert!(res.contains("## L1 — ESSENTIAL STORY"));
        assert!(res.contains("### ARCH"));
        assert!(res.contains("### GENERAL"));
        assert!(res.contains("Important architectural decision..."));
        assert!(res.contains("arch.md"));
        assert!(res.contains("A very long document...")); // Should contain truncated version
        assert!(res.contains("..."));
    }

    #[test]
    fn test_layer1_format_generation_too_long() {
        let docs = vec!["A".repeat(250); 20];
        let mut m1 = serde_json::Map::new();
        m1.insert(
            "source_file".to_string(),
            serde_json::Value::String("B".repeat(200)),
        );
        let metas = vec![Some(m1); 20];

        let dialect = crate::dialect::Dialect::default();
        let res = dialect.generate_layer1(&docs, &metas);
        assert!(res.contains("... (more in L3 search)"));
    }

    #[test]
    fn test_layer2_format_retrieval() {
        assert_eq!(
            Layer2::format_retrieval(None, None, &[], &[]),
            "No drawers found for general."
        );

        let wing = "eng".to_string();
        let room = "rust".to_string();

        assert!(Layer2::format_retrieval(Some(&wing), None, &[], &[]).contains("wing=eng"));
        assert!(Layer2::format_retrieval(None, Some(&room), &[], &[]).contains("room=rust"));
        assert!(Layer2::format_retrieval(Some(&wing), Some(&room), &[], &[])
            .contains("wing=eng room=rust"));

        let docs = vec![Some("Snippet 1".to_string()), None, Some("A".repeat(400))];
        let mut m1 = serde_json::Map::new();
        m1.insert(
            "room".to_string(),
            serde_json::Value::String("hall".to_string()),
        );
        m1.insert(
            "source_file".to_string(),
            serde_json::Value::String("file1.txt".to_string()),
        );
        let metas = vec![Some(m1), None, None];

        let res = Layer2::format_retrieval(None, None, &docs, &metas);
        assert!(res.contains("## L2 — ON-DEMAND (3 drawers)"));
        assert!(res.contains("[hall] WT:6| Snippet 1"));
        assert!(res.contains("file1.txt"));
        assert!(res.contains("[?]")); // None doc
        assert!(res.contains(&"A".repeat(297))); // Truncated long doc
        assert!(res.contains("..."));
    }

    #[test]
    fn test_layer3_format_search() {
        assert_eq!(
            Layer3::format_search("query", &[], &[], &[]),
            "No results found."
        );
        assert_eq!(
            Layer3::format_search("query", &[String::new()], &[None], &[0.0]),
            "No results found."
        );

        let docs = vec!["Found result 1".to_string(), "B".repeat(400)];
        let mut m1 = serde_json::Map::new();
        m1.insert(
            "wing".to_string(),
            serde_json::Value::String("w1".to_string()),
        );
        m1.insert(
            "room".to_string(),
            serde_json::Value::String("r1".to_string()),
        );
        m1.insert(
            "source_file".to_string(),
            serde_json::Value::String("path/f1.txt".to_string()),
        );
        let metas = vec![Some(m1), None];
        let dists = vec![0.2_f32, 0.9_f32];

        let res = Layer3::format_search("test query", &docs, &metas, &dists);
        assert!(res.contains("## L3 — SEARCH RESULTS for \"test query\""));
        assert!(res.contains("[1] w1/r1"));
        assert!(res.contains("sim=0.800, wt=6")); // Default importance 3.0 -> wt 6
        assert!(res.contains("Found result 1"));
        assert!(res.contains("src: f1.txt"));
        assert!(res.contains("[2] ?/? (sim=0.100, wt=6)"));
        assert!(res.contains(&"B".repeat(297)));
    }

    #[test]
    fn test_memory_stack_format_wake_up() {
        let l0 = "Identity block".to_string();
        let l1 = "Generation block".to_string();
        let wake = MemoryStack::format_wake_up(l0, l1);
        assert_eq!(wake, "Identity block\n\nGeneration block");
    }

    #[test]
    fn test_storage_new_and_add_wing() {
        let storage = Storage::new(":memory:").unwrap();
        let wing = Wing {
            name: "test_wing".to_string(),
            r#type: "test".to_string(),
            keywords: vec![],
        };
        assert!(storage.add_wing(&wing).is_ok());
    }

    #[test]
    fn test_layer0_new_and_render() {
        // Test new with no path provided
        let l0_default = Layer0::new(None);
        let path_str = l0_default.path.to_string_lossy();
        assert!(path_str.contains("identity.txt"));
        assert!(path_str.contains(".mempalace"));

        // Test with a specific path that doesn't exist
        let temp_dir = std::env::temp_dir();
        let temp_path = temp_dir.join("nonexistent_identity.txt");
        let mut l0 = Layer0::new(Some(temp_path));

        let rendered = l0.render();
        assert!(rendered.contains("No identity configured"));

        // Render again to cover the `if let Some(text) = &self.text` branch
        let rendered2 = l0.render();
        assert_eq!(rendered, rendered2);

        // Test with a file that exists
        let existing_path = temp_dir.join("existing_identity.txt");
        fs::write(&existing_path, "My test identity").unwrap();
        let mut l0_existing = Layer0::new(Some(existing_path.clone()));
        let rendered_existing = l0_existing.render();
        assert_eq!(rendered_existing, "My test identity");
        fs::remove_file(existing_path).unwrap();
    }

    #[test]
    fn test_layer_new_methods() {
        let config = MempalaceConfig::default();
        let l1 = Layer1::new(config.clone(), Some("wing1".to_string()));
        assert_eq!(l1.wing, Some("wing1".to_string()));

        let l2 = Layer2::new(config.clone());
        assert_eq!(l2.config.collection_name, config.collection_name);

        let l3 = Layer3::new(config.clone());
        assert_eq!(l3.config.collection_name, config.collection_name);
    }

    #[test]
    fn test_memory_stack_new() {
        let temp_dir = tempfile::tempdir().unwrap();
        let config = MempalaceConfig::new(Some(temp_dir.path().to_path_buf()));
        let stack = MemoryStack::new(config);
        assert!(stack.l0.path.to_string_lossy().contains("identity.txt"));
        assert!(stack.l1.wing.is_none());
    }

    #[tokio::test]
    async fn test_layer_async_failures() {
        let temp_dir = tempfile::tempdir().unwrap();
        let config = MempalaceConfig::new(Some(temp_dir.path().to_path_buf()));
        // Set an invalid collection name to force error after connecting
        let invalid_config = MempalaceConfig {
            collection_name: "non_existent_collection_12345".to_string(),
            ..config
        };

        let l1 = Layer1::new(invalid_config.clone(), None);
        let res = l1.generate().await;
        assert!(
            res.contains("Could not connect")
                || res.contains("No palace found")
                || res.contains("Error fetching")
                || res.contains("No memories yet")
        );

        let l2 = Layer2::new(invalid_config.clone());
        let res2 = l2.retrieve(None, None, 10).await;
        assert!(
            res2.contains("Could not connect")
                || res2.contains("No palace found")
                || res2.contains("Retrieval error")
                || res2.contains("No drawers found")
        );

        let l3 = Layer3::new(invalid_config.clone());
        let res3 = l3.search("test", None, None, 10).await;
        assert!(
            res3.contains("Could not connect")
                || res3.contains("No palace found")
                || res3.contains("Search error")
                || res3.contains("No results found")
        );
    }

    #[tokio::test]
    async fn test_memory_stack_async_methods() {
        let invalid_config = MempalaceConfig {
            collection_name: "non_existent_collection_12345".to_string(),
            ..Default::default()
        };
        let mut stack = MemoryStack::new(invalid_config);

        let wake = stack.wake_up(Some("test_wing".to_string())).await;
        assert!(wake.contains("## L0")); // Identity part
        assert!(wake.contains("L1")); // Generation part
        assert_eq!(stack.l1.wing, Some("test_wing".to_string()));

        let recall = stack.recall(None, None, 5).await;
        assert!(!recall.is_empty());

        let search = stack.search("query", None, None, 5).await;
        assert!(!search.is_empty());
    }

    #[test]
    fn test_layer2_format_retrieval_empty() {
        assert!(Layer2::format_retrieval(None, None, &[], &[]).contains("No drawers found"));
    }

    #[test]
    fn test_storage_new_memory() {
        let storage = Storage::new(":memory:").unwrap();
        assert!(storage
            .add_wing(&crate::models::Wing {
                name: "mem".to_string(),
                r#type: "mem".to_string(),
                keywords: vec![],
            })
            .is_ok());

        assert!(storage
            .add_wing(&crate::models::Wing {
                name: "convo".to_string(),
                r#type: "convos".to_string(),
                keywords: vec!["chat".to_string()],
            })
            .is_ok());
    }

    #[test]
    fn test_layer1_build_where_clause_both() {
        let w = "w".to_string();
        let r = "r".to_string();
        let clause = Layer1::build_where_clause(Some(&w), Some(&r));
        let val = serde_json::to_value(clause).unwrap();
        assert_eq!(val["$and"][0]["wing"], "w");
        assert_eq!(val["$and"][1]["room"], "r");
    }

    #[test]
    fn test_storage_add_wing_convo() {
        let storage = Storage::new(":memory:").unwrap();
        let wing = crate::models::Wing {
            name: "c".to_string(),
            r#type: "convos".to_string(),
            keywords: vec!["k".to_string()],
        };
        storage.add_wing(&wing).unwrap();
    }

    #[test]
    fn test_storage_add_wing_duplicate() {
        let db_name = format!("test_duplicate_{}.db", rand::random::<u32>());
        let storage = Storage::new(&db_name).unwrap();
        let wing = crate::models::Wing {
            name: "test".to_string(),
            r#type: "test".to_string(),
            keywords: vec![],
        };
        storage.add_wing(&wing).unwrap();
        let result = storage.add_wing(&wing);
        assert!(result.is_err());
        let _ = std::fs::remove_file(db_name);
    }

    #[test]
    fn test_layer1_format_generation_single() {
        let docs = vec!["single".to_string(), "  ".to_string()];
        let metas = vec![None, None];
        let dialect = crate::dialect::Dialect::default();
        let output = dialect.generate_layer1(&docs, &metas);
        assert!(output.contains("single"));
    }

    #[test]
    fn test_layer1_format_generation_weights() {
        let docs = vec![
            "weight_test".to_string(),
            "emotional_test".to_string(),
            "long_test ".to_string() + &"A".repeat(210),
        ];
        let metas = vec![
            Some(
                serde_json::json!({"weight": 0.9})
                    .as_object()
                    .unwrap()
                    .clone(),
            ),
            Some(
                serde_json::json!({"emotional_weight": 0.8})
                    .as_object()
                    .unwrap()
                    .clone(),
            ),
            None,
        ];
        let dialect = crate::dialect::Dialect::default();
        let output = dialect.generate_layer1(&docs, &metas);
        assert!(output.contains("weight_test"));
        assert!(output.contains("emotional_test"));
        assert!(output.contains("long_test"));
        assert!(output.contains("..."));
    }
}
