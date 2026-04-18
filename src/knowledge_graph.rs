use anyhow::{anyhow, Result};
use rusqlite::{params, Connection};
use serde_json::{json, Value};
use std::path::Path;

pub struct KnowledgeGraph {
    conn: Connection,
}

impl KnowledgeGraph {
    pub fn new(path: &str) -> Result<Self> {
        if path != ":memory:" {
            if let Some(parent) = Path::new(path).parent() {
                if !parent.as_os_str().is_empty() {
                    std::fs::create_dir_all(parent)
                        .map_err(|e| anyhow!("Failed to create directory for KG: {}", e))?;
                }
            }
        }
        let conn = Connection::open(path).map_err(|e| anyhow!("Failed to open KG DB: {}", e))?;
        let kg = KnowledgeGraph { conn };
        kg._init_db()?;
        Ok(kg)
    }

    fn _init_db(&self) -> Result<()> {
        self.conn
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS entities (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                type TEXT DEFAULT 'unknown',
                properties TEXT DEFAULT '{}',
                created_at TEXT DEFAULT CURRENT_TIMESTAMP
            );

            CREATE TABLE IF NOT EXISTS triples (
                id TEXT PRIMARY KEY,
                subject TEXT NOT NULL,
                predicate TEXT NOT NULL,
                object TEXT NOT NULL,
                valid_from TEXT,
                valid_to TEXT,
                confidence REAL DEFAULT 1.0,
                source_closet TEXT,
                source_file TEXT,
                extracted_at TEXT DEFAULT CURRENT_TIMESTAMP,
                FOREIGN KEY (subject) REFERENCES entities(id),
                FOREIGN KEY (object) REFERENCES entities(id)
            );

            CREATE INDEX IF NOT EXISTS idx_triples_subject ON triples(subject);
            CREATE INDEX IF NOT EXISTS idx_triples_object ON triples(object);
            CREATE INDEX IF NOT EXISTS idx_triples_predicate ON triples(predicate);
            CREATE INDEX IF NOT EXISTS idx_triples_valid ON triples(valid_from, valid_to);",
            )
            .map_err(|e| anyhow!("Failed to initialize KG DB: {}", e))?;
        Ok(())
    }

    fn _entity_id(&self, name: &str) -> String {
        name.to_lowercase().replace(' ', "_").replace('\'', "")
    }

    pub fn add_entity(
        &self,
        name: &str,
        entity_type: &str,
        properties: Option<Value>,
    ) -> Result<String> {
        let eid = self._entity_id(name);
        let props = properties.unwrap_or_else(|| json!({})).to_string();
        self.conn.execute(
            "INSERT OR REPLACE INTO entities (id, name, type, properties) VALUES (?1, ?2, ?3, ?4)",
            params![eid, name, entity_type, props],
        ).map_err(|e| anyhow!("Failed to add entity: {}", e))?;
        Ok(eid)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn add_triple(
        &self,
        subject: &str,
        predicate: &str,
        obj: &str,
        valid_from: Option<&str>,
        valid_to: Option<&str>,
        confidence: f64,
        source_closet: Option<&str>,
        source_file: Option<&str>,
    ) -> Result<String> {
        let sub_id = self._entity_id(subject);
        let obj_id = self._entity_id(obj);
        let pred = predicate.to_lowercase().replace(' ', "_");

        // Auto-create entities if they don't exist
        self.conn
            .execute(
                "INSERT OR IGNORE INTO entities (id, name) VALUES (?1, ?2)",
                params![sub_id, subject],
            )
            .map_err(|e| anyhow!("Failed to ensure subject entity: {}", e))?;
        self.conn
            .execute(
                "INSERT OR IGNORE INTO entities (id, name) VALUES (?1, ?2)",
                params![obj_id, obj],
            )
            .map_err(|e| anyhow!("Failed to ensure object entity: {}", e))?;

        // Check for existing identical triple
        let mut stmt = self.conn.prepare(
            "SELECT id FROM triples WHERE subject=?1 AND predicate=?2 AND object=?3 AND valid_to IS NULL"
        ).map_err(|e| anyhow!("Failed to prepare triple existence check: {}", e))?;
        let mut rows = stmt
            .query(params![sub_id, pred, obj_id])
            .map_err(|e| anyhow!("Failed to query triple existence: {}", e))?;
        if let Some(row) = rows
            .next()
            .map_err(|e| anyhow!("Failed to read triple row: {}", e))?
        {
            return row
                .get(0)
                .map_err(|e| anyhow!("Failed to get triple ID: {}", e));
        }

        let triple_id = format!(
            "t_{}_{}_{}_{}",
            sub_id,
            pred,
            obj_id,
            &self.hash_now(valid_from)
        );

        self.conn.execute(
            "INSERT INTO triples (id, subject, predicate, object, valid_from, valid_to, confidence, source_closet, source_file)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                triple_id,
                sub_id,
                pred,
                obj_id,
                valid_from,
                valid_to,
                confidence,
                source_closet,
                source_file,
            ],
        ).map_err(|e| anyhow!("Failed to insert triple: {}", e))?;
        Ok(triple_id)
    }

    fn hash_now(&self, seed: Option<&str>) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        use std::time::SystemTime;

        let mut hasher = DefaultHasher::new();
        seed.unwrap_or("").hash(&mut hasher);
        SystemTime::now().hash(&mut hasher);
        format!("{:x}", hasher.finish())[..8].to_string()
    }

    pub fn invalidate(
        &self,
        subject: &str,
        predicate: &str,
        obj: &str,
        ended: Option<&str>,
    ) -> Result<()> {
        let sub_id = self._entity_id(subject);
        let obj_id = self._entity_id(obj);
        let pred = predicate.to_lowercase().replace(' ', "_");
        let end_date = ended.map(|s| s.to_string()).unwrap_or_else(|| {
            use chrono::Local;
            Local::now().format("%Y-%m-%d").to_string()
        });

        self.conn.execute(
            "UPDATE triples SET valid_to=?1 WHERE subject=?2 AND predicate=?3 AND object=?4 AND valid_to IS NULL",
            params![end_date, sub_id, pred, obj_id],
        ).map_err(|e| anyhow!("Failed to invalidate triple: {}", e))?;
        Ok(())
    }

    pub fn query_entity(
        &self,
        name: &str,
        as_of: Option<&str>,
        direction: &str,
    ) -> Result<Vec<Value>> {
        let eid = self._entity_id(name);
        let mut results = Vec::new();

        if direction == "outgoing" || direction == "both" {
            let mut query = "SELECT t.id, t.subject, t.predicate, t.object, t.valid_from, t.valid_to, t.confidence, t.source_closet, t.source_file, t.extracted_at, e.name as obj_name FROM triples t JOIN entities e ON t.object = e.id WHERE t.subject = ?1".to_string();
            let mut params_vec: Vec<String> = vec![eid.clone()];
            if let Some(date) = as_of {
                query += " AND (t.valid_from IS NULL OR t.valid_from <= ?2) AND (t.valid_to IS NULL OR t.valid_to >= ?3)";
                params_vec.push(date.to_string());
                params_vec.push(date.to_string());
            }

            let mut stmt = self
                .conn
                .prepare(&query)
                .map_err(|e| anyhow!("Failed to prepare outgoing query: {}", e))?;
            let rows = stmt
                .query_map(rusqlite::params_from_iter(params_vec.iter()), |row| {
                    Ok(json!({
                        "direction": "outgoing",
                        "subject": name,
                        "predicate": row.get::<_, String>(2)?,
                        "object": row.get::<_, String>(10)?,
                        "valid_from": row.get::<_, Option<String>>(4)?,
                        "valid_to": row.get::<_, Option<String>>(5)?,
                        "confidence": row.get::<_, f64>(6)?,
                        "source_closet": row.get::<_, Option<String>>(7)?,
                        "current": row.get::<_, Option<String>>(5)?.is_none(),
                    }))
                })
                .map_err(|e| anyhow!("Failed to execute outgoing query: {}", e))?;

            for row in rows {
                results.push(row.map_err(|e| anyhow!("Failed to read outgoing row: {}", e))?);
            }
        }

        if direction == "incoming" || direction == "both" {
            let mut query = "SELECT t.id, t.subject, t.predicate, t.object, t.valid_from, t.valid_to, t.confidence, t.source_closet, t.source_file, t.extracted_at, e.name as sub_name FROM triples t JOIN entities e ON t.subject = e.id WHERE t.object = ?1".to_string();
            let mut params_vec: Vec<String> = vec![eid.clone()];
            if let Some(date) = as_of {
                query += " AND (t.valid_from IS NULL OR t.valid_from <= ?2) AND (t.valid_to IS NULL OR t.valid_to >= ?3)";
                params_vec.push(date.to_string());
                params_vec.push(date.to_string());
            }

            let mut stmt = self
                .conn
                .prepare(&query)
                .map_err(|e| anyhow!("Failed to prepare incoming query: {}", e))?;
            let rows = stmt
                .query_map(rusqlite::params_from_iter(params_vec.iter()), |row| {
                    Ok(json!({
                        "direction": "incoming",
                        "subject": row.get::<_, String>(10)?,
                        "predicate": row.get::<_, String>(2)?,
                        "object": name,
                        "valid_from": row.get::<_, Option<String>>(4)?,
                        "valid_to": row.get::<_, Option<String>>(5)?,
                        "confidence": row.get::<_, f64>(6)?,
                        "source_closet": row.get::<_, Option<String>>(7)?,
                        "current": row.get::<_, Option<String>>(5)?.is_none(),
                    }))
                })
                .map_err(|e| anyhow!("Failed to execute incoming query: {}", e))?;

            for row in rows {
                results.push(row.map_err(|e| anyhow!("Failed to read incoming row: {}", e))?);
            }
        }

        Ok(results)
    }

    pub fn stats(&self) -> Result<Value> {
        let mut entity_count: i64 = 0;
        let mut triple_count: i64 = 0;

        self.conn
            .query_row("SELECT COUNT(*) FROM entities", [], |row| {
                entity_count = row.get(0)?;
                Ok(())
            })
            .map_err(|e| anyhow!("Failed to get entity count: {}", e))?;

        self.conn
            .query_row("SELECT COUNT(*) FROM triples", [], |row| {
                triple_count = row.get(0)?;
                Ok(())
            })
            .map_err(|e| anyhow!("Failed to get triple count: {}", e))?;

        Ok(json!({
            "entities": entity_count,
            "triples": triple_count,
            "status": "active"
        }))
    }
}
