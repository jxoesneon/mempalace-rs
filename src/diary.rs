use anyhow::Result;
use chrono::Utc;
use rusqlite::{params, Connection, Result as SqlResult};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct DiaryEntry {
    pub id: i64,
    pub agent: String,
    pub content: String,
    pub timestamp: String,
}

pub struct Diary {
    conn: Connection,
}

impl Diary {
    pub fn new<P: AsRef<Path>>(db_path: P) -> Result<Self> {
        let conn = Connection::open(db_path)?;
        let diary = Diary { conn };
        diary.init_db()?;
        Ok(diary)
    }

    pub fn new_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        let diary = Diary { conn };
        diary.init_db()?;
        Ok(diary)
    }

    fn init_db(&self) -> SqlResult<()> {
        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS diary_entries (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                agent TEXT NOT NULL,
                content TEXT NOT NULL,
                timestamp TEXT NOT NULL,
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP
            )",
            [],
        )?;

        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_diary_agent ON diary_entries(agent)",
            [],
        )?;

        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_diary_timestamp ON diary_entries(timestamp DESC)",
            [],
        )?;

        Ok(())
    }

    pub fn write_entry(&self, agent: &str, content: &str) -> Result<i64> {
        let timestamp = Utc::now().to_rfc3339();

        self.conn.execute(
            "INSERT INTO diary_entries (agent, content, timestamp) VALUES (?1, ?2, ?3)",
            params![agent, content, timestamp],
        )?;

        let id = self.conn.last_insert_rowid();
        Ok(id)
    }

    pub fn read_entries(&self, agent: &str, last_n: usize) -> Result<Vec<DiaryEntry>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, agent, content, timestamp FROM diary_entries 
             WHERE agent = ?1 
             ORDER BY timestamp DESC 
             LIMIT ?2",
        )?;

        let rows = stmt.query_map(params![agent, last_n as i64], |row| {
            Ok(DiaryEntry {
                id: row.get(0)?,
                agent: row.get(1)?,
                content: row.get(2)?,
                timestamp: row.get(3)?,
            })
        })?;

        let mut entries = Vec::new();
        for row in rows {
            entries.push(row?);
        }

        entries.reverse();
        Ok(entries)
    }

    pub fn read_all_entries(&self, agent: &str) -> Result<Vec<DiaryEntry>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, agent, content, timestamp FROM diary_entries 
             WHERE agent = ?1 
             ORDER BY timestamp DESC",
        )?;

        let rows = stmt.query_map(params![agent], |row| {
            Ok(DiaryEntry {
                id: row.get(0)?,
                agent: row.get(1)?,
                content: row.get(2)?,
                timestamp: row.get(3)?,
            })
        })?;

        let mut entries = Vec::new();
        for row in rows {
            entries.push(row?);
        }

        entries.reverse();
        Ok(entries)
    }

    pub fn delete_entry(&self, id: i64) -> Result<()> {
        self.conn
            .execute("DELETE FROM diary_entries WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn get_stats(&self) -> Result<(i64, i64)> {
        let total: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM diary_entries", [], |row| row.get(0))?;

        let agents: i64 = self.conn.query_row(
            "SELECT COUNT(DISTINCT agent) FROM diary_entries",
            [],
            |row| row.get(0),
        )?;

        Ok((total, agents))
    }
}

pub fn get_diary_path() -> String {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    let path = std::path::PathBuf::from(&home)
        .join(".mempalace")
        .join("diary.db");

    // Ensure the parent directory exists
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    path.to_string_lossy().to_string()
}

pub fn write_diary(agent: &str, content: &str) -> Result<()> {
    let path = get_diary_path();
    let diary = Diary::new(&path)?;
    let id = diary.write_entry(agent, content)?;
    tracing::info!("✓ Diary entry {} written for agent {}", id, agent);
    Ok(())
}

pub fn read_diary(agent: &str, last_n: usize) -> Result<Vec<DiaryEntry>> {
    let path = get_diary_path();
    let diary = Diary::new(&path)?;
    diary.read_entries(agent, last_n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_diary_create_and_read() {
        let diary = Diary::new_in_memory().unwrap();

        let id1 = diary.write_entry("test-agent", "First entry").unwrap();
        let id2 = diary.write_entry("test-agent", "Second entry").unwrap();
        let id3 = diary
            .write_entry("other-agent", "Different agent entry")
            .unwrap();

        assert_eq!(id1, 1);
        assert_eq!(id2, 2);
        assert_eq!(id3, 3);

        let entries = diary.read_entries("test-agent", 10).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].agent, "test-agent");
        assert_eq!(entries[0].content, "First entry");
        assert_eq!(entries[1].content, "Second entry");

        let other_entries = diary.read_entries("other-agent", 10).unwrap();
        assert_eq!(other_entries.len(), 1);
        assert_eq!(other_entries[0].content, "Different agent entry");
    }

    #[test]
    fn test_diary_last_n_limit() {
        let diary = Diary::new_in_memory().unwrap();

        for i in 1..=5 {
            diary
                .write_entry("limited-agent", &format!("Entry {}", i))
                .unwrap();
        }

        let entries = diary.read_entries("limited-agent", 3).unwrap();
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].content, "Entry 3");
        assert_eq!(entries[1].content, "Entry 4");
        assert_eq!(entries[2].content, "Entry 5");
    }

    #[test]
    fn test_diary_delete() {
        let diary = Diary::new_in_memory().unwrap();

        let id = diary.write_entry("delete-agent", "To be deleted").unwrap();
        let entries_before = diary.read_entries("delete-agent", 10).unwrap();
        assert_eq!(entries_before.len(), 1);

        diary.delete_entry(id).unwrap();

        let entries_after = diary.read_entries("delete-agent", 10).unwrap();
        assert!(entries_after.is_empty());
    }

    #[test]
    fn test_diary_stats() {
        let diary = Diary::new_in_memory().unwrap();

        let (total, agents) = diary.get_stats().unwrap();
        assert_eq!(total, 0);
        assert_eq!(agents, 0);

        diary.write_entry("agent-a", "Entry 1").unwrap();
        diary.write_entry("agent-a", "Entry 2").unwrap();
        diary.write_entry("agent-b", "Entry 3").unwrap();

        let (total, agents) = diary.get_stats().unwrap();
        assert_eq!(total, 3);
        assert_eq!(agents, 2);
    }

    #[test]
    fn test_diary_timestamp() {
        let diary = Diary::new_in_memory().unwrap();

        let before = Utc::now();
        diary.write_entry("time-agent", "Timed entry").unwrap();
        let after = Utc::now();

        let entries = diary.read_entries("time-agent", 1).unwrap();
        assert_eq!(entries.len(), 1);

        let entry_time = chrono::DateTime::parse_from_rfc3339(&entries[0].timestamp)
            .unwrap()
            .with_timezone(&chrono::Utc);

        assert!(entry_time >= before);
        assert!(entry_time <= after);
    }

    #[test]
    fn test_diary_public_functions() {
        let temp_dir = std::env::temp_dir().join("mempalace_diary_test");
        let _ = std::fs::create_dir_all(&temp_dir);

        std::env::set_var("HOME", &temp_dir);

        // Clean up any existing test diary database
        let diary_db = temp_dir.join(".mempalace").join("diary.db");
        let _ = std::fs::remove_file(&diary_db);

        write_diary("public-agent", "Public entry").unwrap();

        let entries = read_diary("public-agent", 5).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].content, "Public entry");

        // Cleanup
        let _ = std::fs::remove_file(&diary_db);
        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
