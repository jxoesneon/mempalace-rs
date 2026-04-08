// vector_storage.rs — MemPalace Pure-Rust Storage Engine (replaces ChromaDB)
//
// Zero-network, embedded storage combining:
//   • fastembed-rs  → CPU/ONNX text embeddings (AllMiniLML6V2, 384-dim)
//   • rusqlite      → relational source of truth
//   • usearch       → SIMD-accelerated HNSW ANN index

use std::path::Path;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, Context, Result};
use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};
use rusqlite::{params, Connection};
use std::path::PathBuf;
use usearch::{Index, IndexOptions, MetricKind, ScalarKind};

const VECTOR_DIMS: usize = 384;
const HNSW_M: usize = 16;
const HNSW_EF_CONSTRUCTION: usize = 128;

/// A structured record of a single atomic memory filed in the Palace.
#[derive(Debug, Clone)]
pub struct MemoryRecord {
    pub id: i64,
    pub text_content: String,
    pub wing: String,
    pub room: String,
    pub source_file: Option<String>,
    pub valid_from: i64,
    pub valid_to: Option<i64>,
    pub score: f32,
}

/// Represents a chronological validity window for a memory.
#[derive(Debug, Clone, Default)]
pub struct TemporalRange {
    pub valid_from: Option<i64>,
    pub valid_to: Option<i64>,
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before Unix epoch")
        .as_secs() as i64
}

fn build_index() -> Result<Index> {
    let opts = IndexOptions {
        dimensions: VECTOR_DIMS,
        metric: MetricKind::Cos,
        quantization: ScalarKind::F32,
        connectivity: HNSW_M,
        expansion_add: HNSW_EF_CONSTRUCTION,
        expansion_search: 64,
        ..Default::default()
    };
    Index::new(&opts).map_err(|e| anyhow!("usearch index creation failed: {e}"))
}

/// The pure-Rust vector storage engine combining SQLite metadata and usearch HNSW index.
pub struct VectorStorage {
    embedder: Arc<TextEmbedding>,
    db: Connection,
    index: Index,
}

impl VectorStorage {
    pub fn new(db_path: impl AsRef<Path>, index_path: impl AsRef<Path>) -> Result<Self> {
        // 1. Embedding model - resolve cache dir in priority order:
        //    a) MEMPALACE_MODELS_DIR env var (explicit config)
        //    b) models/ next to the running executable (release bundle)
        //    c) None → fastembed downloads on first use
        let cache_dir = std::env::var("MEMPALACE_MODELS_DIR")
            .ok()
            .map(PathBuf::from)
            .filter(|p| p.exists())
            .or_else(|| {
                std::env::current_exe()
                    .ok()
                    .and_then(|exe| exe.parent().map(|p| p.join("models")))
                    .filter(|p| p.exists())
            });

        let mut init_opts =
            InitOptions::new(EmbeddingModel::AllMiniLML6V2).with_show_download_progress(false);

        if let Some(cache) = cache_dir {
            init_opts = init_opts.with_cache_dir(cache);
        }

        let embedder =
            TextEmbedding::try_new(init_opts).context("Failed to initialise fastembed")?;

        // 2. SQLite
        let db = Connection::open(db_path.as_ref())
            .with_context(|| format!("Cannot open SQLite at {:?}", db_path.as_ref()))?;

        db.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA foreign_keys = ON;
             PRAGMA synchronous = NORMAL;
             CREATE TABLE IF NOT EXISTS memories (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                text_content TEXT NOT NULL,
                wing TEXT NOT NULL,
                room TEXT NOT NULL,
                source_file TEXT,
                valid_from INTEGER NOT NULL,
                valid_to INTEGER
             );
             CREATE INDEX IF NOT EXISTS idx_source_file ON memories (source_file);
             CREATE INDEX IF NOT EXISTS idx_wing_room ON memories (wing, room);
             CREATE INDEX IF NOT EXISTS idx_valid ON memories (valid_from, valid_to);
             CREATE TABLE IF NOT EXISTS drawers (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                content TEXT NOT NULL,
                wing TEXT NOT NULL,
                room TEXT NOT NULL,
                source_file TEXT,
                filed_at TEXT NOT NULL,
                embedding_id INTEGER REFERENCES memories(id)
             );
             CREATE INDEX IF NOT EXISTS idx_drawers_wing_room ON drawers (wing, room);
            ",
        )?;

        // 3. usearch HNSW index
        let index_path = index_path.as_ref();
        let index = if index_path.exists() {
            let idx = build_index()?;
            idx.load(
                index_path
                    .to_str()
                    .ok_or_else(|| anyhow!("Non-UTF8 index path"))?,
            )
            .map_err(|e| anyhow!("Failed to load usearch index: {e}"))?;
            idx
        } else {
            build_index()?
        };

        Ok(Self {
            embedder: Arc::new(embedder),
            db,
            index,
        })
    }

    pub fn add_memory(
        &mut self,
        text: &str,
        wing: &str,
        room: &str,
        source_file: Option<&str>,
        temporal: Option<TemporalRange>,
    ) -> Result<i64> {
        let vector = self.embed_single(text)?;
        let (valid_from, valid_to) = match temporal {
            Some(t) => (t.valid_from.unwrap_or_else(now_unix), t.valid_to),
            None => (now_unix(), None),
        };

        self.db.execute(
            "INSERT INTO memories (text_content, wing, room, source_file, valid_from, valid_to)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![text, wing, room, source_file, valid_from, valid_to],
        )?;

        let row_id = self.db.last_insert_rowid();

        let needed = self.index.size() + 1;
        if needed > self.index.capacity() {
            let new_cap = (needed * 2).max(64);
            self.index
                .reserve(new_cap)
                .map_err(|e| anyhow!("usearch reserve failed: {e}"))?;
        }

        self.index
            .add(row_id as u64, &vector)
            .map_err(|e| anyhow!("usearch add failed: {e}"))?;

        Ok(row_id)
    }

    pub fn search_room(
        &self,
        query: &str,
        wing: &str,
        room: &str,
        limit: usize,
        at_time: Option<i64>,
    ) -> Result<Vec<MemoryRecord>> {
        if limit == 0 {
            return Ok(vec![]);
        }
        let at_time = at_time.unwrap_or_else(now_unix);
        let query_vector = self.embed_single(query)?;

        let mut stmt = self.db.prepare_cached(
            "SELECT id FROM memories
             WHERE wing = ?1 AND room = ?2
               AND valid_from <= ?3
               AND (valid_to IS NULL OR valid_to >= ?3)",
        )?;

        let candidate_ids: Vec<u64> = stmt
            .query_map(params![wing, room, at_time], |row| row.get::<_, i64>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?
            .into_iter()
            .map(|id| id as u64)
            .collect();

        if candidate_ids.is_empty() {
            return Ok(vec![]);
        }

        let candidate_set: std::collections::HashSet<u64> = candidate_ids.iter().cloned().collect();
        let results = self
            .index
            .filtered_search(&query_vector, limit, |key: u64| {
                candidate_set.contains(&key)
            })
            .map_err(|e| anyhow!("usearch filtered_search failed: {e}"))?;

        if results.keys.is_empty() {
            return Ok(vec![]);
        }

        let id_placeholders: String = results
            .keys
            .iter()
            .enumerate()
            .map(|(i, _)| format!("?{}", i + 1))
            .collect::<Vec<_>>()
            .join(", ");

        let sql = format!(
            "SELECT id, text_content, wing, room, source_file, valid_from, valid_to
             FROM memories WHERE id IN ({id_placeholders})"
        );

        let mut stmt = self.db.prepare(&sql)?;
        let params_vec: Vec<&dyn rusqlite::ToSql> = results
            .keys
            .iter()
            .map(|k| k as &dyn rusqlite::ToSql)
            .collect();

        let mut record_map: std::collections::HashMap<i64, MemoryRecord> = stmt
            .query_map(params_vec.as_slice(), |row| {
                Ok(MemoryRecord {
                    id: row.get(0)?,
                    text_content: row.get(1)?,
                    wing: row.get(2)?,
                    room: row.get(3)?,
                    source_file: row.get(4)?,
                    valid_from: row.get(5)?,
                    valid_to: row.get(6)?,
                    score: 0.0,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?
            .into_iter()
            .map(|r| (r.id, r))
            .collect();

        let mut ordered: Vec<MemoryRecord> = results
            .keys
            .iter()
            .zip(results.distances.iter())
            .filter_map(|(&key, &dist)| {
                let id = key as i64;
                record_map.remove(&id).map(|mut rec| {
                    rec.score = 1.0 - dist;
                    rec
                })
            })
            .collect();

        ordered.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        Ok(ordered)
    }

    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<MemoryRecord>> {
        if limit == 0 {
            return Ok(vec![]);
        }
        let query_vector = self.embed_single(query)?;

        let results = self
            .index
            .search(&query_vector, limit)
            .map_err(|e| anyhow!("usearch search failed: {e}"))?;

        if results.keys.is_empty() {
            return Ok(vec![]);
        }

        let id_placeholders: String = results
            .keys
            .iter()
            .enumerate()
            .map(|(i, _)| format!("?{}", i + 1))
            .collect::<Vec<_>>()
            .join(", ");

        let sql = format!(
            "SELECT id, text_content, wing, room, source_file, valid_from, valid_to
             FROM memories WHERE id IN ({id_placeholders})"
        );

        let mut stmt = self.db.prepare(&sql)?;
        let params_vec: Vec<&dyn rusqlite::ToSql> = results
            .keys
            .iter()
            .map(|k| k as &dyn rusqlite::ToSql)
            .collect();

        let mut record_map: std::collections::HashMap<i64, MemoryRecord> = stmt
            .query_map(params_vec.as_slice(), |row| {
                Ok(MemoryRecord {
                    id: row.get(0)?,
                    text_content: row.get(1)?,
                    wing: row.get(2)?,
                    room: row.get(3)?,
                    source_file: row.get(4)?,
                    valid_from: row.get(5)?,
                    valid_to: row.get(6)?,
                    score: 0.0,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?
            .into_iter()
            .map(|r| (r.id, r))
            .collect();

        let mut ordered: Vec<MemoryRecord> = results
            .keys
            .iter()
            .zip(results.distances.iter())
            .filter_map(|(&key, &dist)| {
                let id = key as i64;
                record_map.remove(&id).map(|mut rec| {
                    rec.score = 1.0 - dist;
                    rec
                })
            })
            .collect();

        ordered.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        Ok(ordered)
    }

    pub fn get_memories(
        &self,
        wing: Option<&str>,
        room: Option<&str>,
        limit: usize,
    ) -> Result<Vec<MemoryRecord>> {
        let (sql, params_dyn): (String, Vec<Box<dyn rusqlite::ToSql>>) = match (wing, room) {
            (Some(w), Some(r)) => (
                format!("SELECT id, text_content, wing, room, source_file, valid_from, valid_to FROM memories WHERE wing = ?1 AND room = ?2 ORDER BY valid_from DESC LIMIT {limit}"),
                vec![Box::new(w.to_string()), Box::new(r.to_string())],
            ),
            (Some(w), None) => (
                format!("SELECT id, text_content, wing, room, source_file, valid_from, valid_to FROM memories WHERE wing = ?1 ORDER BY valid_from DESC LIMIT {limit}"),
                vec![Box::new(w.to_string())],
            ),
            (None, Some(r)) => (
                format!("SELECT id, text_content, wing, room, source_file, valid_from, valid_to FROM memories WHERE room = ?1 ORDER BY valid_from DESC LIMIT {limit}"),
                vec![Box::new(r.to_string())],
            ),
            (None, None) => (
                format!("SELECT id, text_content, wing, room, source_file, valid_from, valid_to FROM memories ORDER BY valid_from DESC LIMIT {limit}"),
                vec![],
            ),
        };
        let mut stmt = self.db.prepare(&sql)?;
        let params_ref: Vec<&dyn rusqlite::ToSql> = params_dyn.iter().map(|p| p.as_ref()).collect();
        let records = stmt
            .query_map(params_ref.as_slice(), |row| {
                Ok(MemoryRecord {
                    id: row.get(0)?,
                    text_content: row.get(1)?,
                    wing: row.get(2)?,
                    room: row.get(3)?,
                    source_file: row.get(4)?,
                    valid_from: row.get(5)?,
                    valid_to: row.get(6)?,
                    score: 0.0,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(records)
    }

    pub fn delete_memory(&self, id: i64) -> Result<()> {
        self.db
            .execute("DELETE FROM memories WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn has_source_file(&self, source_file: &str) -> Result<bool> {
        let count: i64 = self.db.query_row(
            "SELECT COUNT(*) FROM memories WHERE source_file = ?1 LIMIT 1",
            params![source_file],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }

    pub fn get_wings_rooms(&self) -> Result<Vec<(String, String)>> {
        let mut stmt = self
            .db
            .prepare("SELECT DISTINCT wing, room FROM memories ORDER BY wing, room")?;
        let pairs = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(pairs)
    }

    pub fn save_index(&self, index_path: impl AsRef<Path>) -> Result<()> {
        let path = index_path
            .as_ref()
            .to_str()
            .ok_or_else(|| anyhow!("Non-UTF8 path"))?;
        self.index
            .save(path)
            .map_err(|e| anyhow!("Save failed: {e}"))
    }

    pub fn memory_count(&self) -> Result<u64> {
        self.db
            .query_row("SELECT COUNT(*) FROM memories", [], |row| {
                row.get::<_, i64>(0)
            })
            .map(|n| n as u64)
            .context("Count failed")
    }

    pub fn index_size(&self) -> usize {
        self.index.size()
    }

    fn embed_single(&self, text: &str) -> Result<Vec<f32>> {
        let mut batch = self
            .embedder
            .embed(vec![text.to_string()], None)
            .context("fastembed failed")?;
        let vec = batch.pop().ok_or_else(|| anyhow!("Empty batch"))?;
        if vec.len() != VECTOR_DIMS {
            return Err(anyhow!("Expected {VECTOR_DIMS}-dim, got {}", vec.len()));
        }
        Ok(vec)
    }
}

impl Drop for VectorStorage {
    fn drop(&mut self) {
        let _ = self.db.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);");
    }
}
