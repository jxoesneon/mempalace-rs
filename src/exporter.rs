// exporter.rs — Export palace memories from VectorStorage to JSON, CSV, or Markdown.
//
// Mirrors the upstream Python `exporter.py` Markdown tree export while adding
// JSON and CSV serialisations. All reads come from `VectorStorage`; optional
// wing/room filters are applied at the SQL level so large palaces can be
// streamed in batches without loading everything into memory.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;
use std::path::Path;
use std::str::FromStr;

use anyhow::{anyhow, Context, Result};
use chrono::{TimeZone, Utc};
use csv::WriterBuilder;
use regex::Regex;

use serde::{Deserialize, Serialize};

use crate::vector_storage::VectorStorage;

/// Supported export formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExportFormat {
    /// Pretty-printed JSON array of exported memories.
    Json,
    /// CSV with a header row and one record per memory.
    Csv,
    /// Markdown, either as a single document or as a browsable directory tree.
    Markdown,
}

impl FromStr for ExportFormat {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        match s.to_lowercase().as_str() {
            "json" => Ok(ExportFormat::Json),
            "csv" => Ok(ExportFormat::Csv),
            "markdown" | "md" => Ok(ExportFormat::Markdown),
            _ => Err(anyhow!("Unknown export format: {s}")),
        }
    }
}

impl std::fmt::Display for ExportFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExportFormat::Json => write!(f, "json"),
            ExportFormat::Csv => write!(f, "csv"),
            ExportFormat::Markdown => write!(f, "markdown"),
        }
    }
}

/// A single memory drawer prepared for export.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportedMemory {
    pub id: i64,
    pub content: String,
    pub wing: String,
    pub room: String,
    pub source: Option<String>,
    pub filed_at: String,
    pub last_accessed: String,
    pub access_count: i64,
    pub importance: f32,
}

impl ExportedMemory {
    fn from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Self> {
        let last_accessed_i64: i64 = row.get(7)?;
        let importance_score: f32 = row.get(9)?;
        Ok(Self {
            id: row.get(0)?,
            content: row.get(1)?,
            wing: row.get(2)?,
            room: row.get(3)?,
            source: row.get(4)?,
            filed_at: format_timestamp(row.get(5)?),
            last_accessed: format_timestamp(last_accessed_i64),
            access_count: row.get(8)?,
            importance: importance_score,
        })
    }
}

/// Export statistics returned by `export_to_file`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ExportStats {
    pub wings: usize,
    pub rooms: usize,
    pub drawers: usize,
}

/// Number of memories read from the database in each batch.
const DEFAULT_BATCH_SIZE: usize = 1000;

/// Export matching memories into a single in-memory string.
///
/// * `Json` — pretty-printed JSON array.
/// * `Csv` — CSV with a header row.
/// * `Markdown` — a single markdown document grouped by wing/room.
pub fn export_memories(
    storage: &VectorStorage,
    format: ExportFormat,
    wing: Option<&str>,
    room: Option<&str>,
) -> Result<String> {
    match format {
        ExportFormat::Json => export_json(storage, wing, room),
        ExportFormat::Csv => export_csv(storage, wing, room),
        ExportFormat::Markdown => export_markdown_string(storage, wing, room),
    }
}

/// Export matching memories to a file or directory.
///
/// * `Json` / `Csv` — written to a single file at `output_path`.
/// * `Markdown` — `output_path` is treated as a directory; an `index.md` and
///   one `wing/room.md` file per room are created inside it.
///
/// Returns counts of unique wings, unique rooms, and total drawers exported.
pub fn export_to_file(
    storage: &VectorStorage,
    output_path: impl AsRef<Path>,
    format: ExportFormat,
    wing: Option<&str>,
    room: Option<&str>,
) -> Result<ExportStats> {
    match format {
        ExportFormat::Json | ExportFormat::Csv => {
            let data = export_memories(storage, format, wing, room)?;
            let path = output_path.as_ref();
            let parent = path.parent().unwrap_or_else(|| Path::new("."));
            fs::create_dir_all(parent)?;
            fs::write(path, data).with_context(|| format!("Failed to write {path:?}"))?;
            let records = fetch_records(storage, wing, room, DEFAULT_BATCH_SIZE)?;
            Ok(stats_from_records(&records))
        }
        ExportFormat::Markdown => export_markdown_tree(storage, output_path.as_ref(), wing, room),
    }
}

/// Sanitize a string for use as a directory or file name component.
fn safe_path_component(name: &str) -> String {
    let re = Regex::new(r#"[\\/:*?"<>|]"#).unwrap();
    let mut cleaned = re.replace_all(name, "_").to_string();
    cleaned = cleaned
        .trim_matches(|c: char| c == '.' || c.is_whitespace())
        .to_string();
    if cleaned.is_empty() {
        cleaned = "unknown".to_string();
    }
    cleaned
}

/// Format a Unix timestamp as an RFC 3339 string.
fn format_timestamp(ts: i64) -> String {
    Utc.timestamp_opt(ts, 0)
        .single()
        .unwrap_or_else(|| Utc.timestamp_opt(0, 0).single().unwrap())
        .to_rfc3339()
}

/// Format content for a markdown blockquote, handling multiline input.
fn quote_content(text: &str) -> String {
    let trimmed = text.trim_end();
    trimmed
        .lines()
        .map(|line| format!("> {line}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Build the SQL query and parameters for filtered memory enumeration.
fn build_filter_sql(
    wing: Option<&str>,
    room: Option<&str>,
) -> (String, Vec<Box<dyn rusqlite::ToSql>>) {
    let mut sql = String::from(
        "SELECT id, text_content, wing, room, source_file, valid_from, valid_to, \
         last_accessed, access_count, importance_score FROM memories",
    );
    let mut conditions: Vec<String> = Vec::new();
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    if let Some(w) = wing {
        conditions.push("wing = ?".to_string());
        params.push(Box::new(w.to_string()));
    }
    if let Some(r) = room {
        conditions.push("room = ?".to_string());
        params.push(Box::new(r.to_string()));
    }
    if !conditions.is_empty() {
        sql.push_str(" WHERE ");
        sql.push_str(&conditions.join(" AND "));
    }
    sql.push_str(" ORDER BY wing, room, id");
    (sql, params)
}

/// Fetch all matching records in batches.
fn fetch_records(
    storage: &VectorStorage,
    wing: Option<&str>,
    room: Option<&str>,
    batch_size: usize,
) -> Result<Vec<ExportedMemory>> {
    let (sql, params) = build_filter_sql(wing, room);
    let params_ref: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();

    let mut stmt = storage.db.prepare(&sql)?;
    let rows = stmt.query_map(params_ref.as_slice(), ExportedMemory::from_row)?;

    let mut records = Vec::new();
    let mut batch = Vec::with_capacity(batch_size);
    for row in rows {
        batch.push(row?);
        if batch.len() >= batch_size {
            records.append(&mut batch);
        }
    }
    records.append(&mut batch);
    Ok(records)
}

/// Compute export statistics from a list of records.
fn stats_from_records(records: &[ExportedMemory]) -> ExportStats {
    let mut wings: BTreeSet<String> = BTreeSet::new();
    let mut rooms: BTreeSet<String> = BTreeSet::new();
    for r in records {
        wings.insert(r.wing.clone());
        rooms.insert(format!("{}/{}", r.wing, r.room));
    }
    ExportStats {
        wings: wings.len(),
        rooms: rooms.len(),
        drawers: records.len(),
    }
}

fn export_json(storage: &VectorStorage, wing: Option<&str>, room: Option<&str>) -> Result<String> {
    let records = fetch_records(storage, wing, room, DEFAULT_BATCH_SIZE)?;
    serde_json::to_string_pretty(&records).context("Failed to serialize JSON")
}

fn export_csv(storage: &VectorStorage, wing: Option<&str>, room: Option<&str>) -> Result<String> {
    let records = fetch_records(storage, wing, room, DEFAULT_BATCH_SIZE)?;
    let mut writer = WriterBuilder::new().from_writer(Vec::new());
    for record in &records {
        writer
            .serialize(record)
            .with_context(|| "Failed to serialize CSV record")?;
    }
    writer
        .into_inner()
        .map_err(|e| anyhow!("CSV flush failed: {e}"))
        .and_then(|v| String::from_utf8(v).context("CSV output is not valid UTF-8"))
}

/// Render a single markdown document with all matching memories grouped by wing/room.
fn export_markdown_string(
    storage: &VectorStorage,
    wing: Option<&str>,
    room: Option<&str>,
) -> Result<String> {
    let records = fetch_records(storage, wing, room, DEFAULT_BATCH_SIZE)?;
    if records.is_empty() {
        return Ok("# Palace Export\n\nNo memories found.\n".to_string());
    }

    let mut grouped: BTreeMap<String, BTreeMap<String, Vec<&ExportedMemory>>> = BTreeMap::new();
    for r in &records {
        grouped
            .entry(r.wing.clone())
            .or_default()
            .entry(r.room.clone())
            .or_default()
            .push(r);
    }

    let today = Utc::now().format("%Y-%m-%d");
    let mut lines = vec![format!("# Palace Export — {today}\n")];
    lines.push("| Wing | Rooms | Drawers |".to_string());
    lines.push("|------|-------|---------|".to_string());

    let mut index_rows: Vec<(String, usize, usize)> = Vec::new();
    for (wing, rooms) in &grouped {
        let room_count = rooms.len();
        let drawer_count = rooms.values().map(|v| v.len()).sum();
        index_rows.push((wing.clone(), room_count, drawer_count));
    }
    for (wing, room_count, drawer_count) in &index_rows {
        lines.push(format!("| {wing} | {room_count} | {drawer_count} |"));
    }
    lines.push(String::new());

    for (wing, rooms) in grouped {
        lines.push(format!("## Wing: {wing}\n"));
        for (room, drawers) in rooms {
            lines.push(format!("### Room: {room}\n"));
            for drawer in drawers {
                lines.push(format!("#### Drawer {}\n", drawer.id));
                lines.push(quote_content(&drawer.content));
                lines.push(String::new());
                lines.push("| Field | Value |".to_string());
                lines.push("|-------|-------|".to_string());
                lines.push(format!(
                    "| Source | {} |",
                    drawer.source.as_deref().unwrap_or("unknown")
                ));
                lines.push(format!("| Filed | {} |", drawer.filed_at));
                lines.push(format!("| Last accessed | {} |", drawer.last_accessed));
                lines.push(format!("| Access count | {} |", drawer.access_count));
                lines.push(format!("| Importance | {:.2} |", drawer.importance));
                lines.push(String::new());
                lines.push("---".to_string());
                lines.push(String::new());
            }
        }
    }

    Ok(lines.join("\n"))
}

/// Write a browsable markdown tree like the upstream Python exporter.
fn export_markdown_tree(
    storage: &VectorStorage,
    output_dir: &Path,
    wing: Option<&str>,
    room: Option<&str>,
) -> Result<ExportStats> {
    let records = fetch_records(storage, wing, room, DEFAULT_BATCH_SIZE)?;
    if records.is_empty() {
        fs::create_dir_all(output_dir)?;
        let index_path = output_dir.join("index.md");
        fs::write(
            &index_path,
            "# Palace Export\n\nPalace is empty — nothing to export.\n",
        )
        .with_context(|| format!("Failed to write {index_path:?}"))?;
        return Ok(ExportStats::default());
    }

    // Group records by wing/room.
    let mut grouped: BTreeMap<String, BTreeMap<String, Vec<ExportedMemory>>> = BTreeMap::new();
    for r in records {
        grouped
            .entry(r.wing.clone())
            .or_default()
            .entry(r.room.clone())
            .or_default()
            .push(r);
    }

    fs::create_dir_all(output_dir)?;

    let today = Utc::now().format("%Y-%m-%d");
    let mut index_lines = vec![format!("# Palace Export — {today}\n")];
    index_lines.push("| Wing | Rooms | Drawers |".to_string());
    index_lines.push("|------|-------|---------|".to_string());

    let mut stats = ExportStats {
        wings: grouped.len(),
        ..ExportStats::default()
    };

    for (wing_name, rooms) in &grouped {
        let safe_wing = safe_path_component(wing_name);
        let wing_dir = output_dir.join(&safe_wing);
        fs::create_dir_all(&wing_dir)?;

        stats.rooms += rooms.len();
        let wing_drawer_count: usize = rooms.values().map(|v| v.len()).sum();
        stats.drawers += wing_drawer_count;
        index_lines.push(format!(
            "| [{wing_name}]({safe_wing}/) | {} | {wing_drawer_count} |",
            rooms.len()
        ));

        for (room_name, drawers) in rooms {
            let safe_room = safe_path_component(room_name);
            let room_path = wing_dir.join(format!("{safe_room}.md"));
            let mut file = fs::File::create(&room_path)
                .with_context(|| format!("Failed to create {room_path:?}"))?;
            writeln!(file, "# {wing_name} / {room_name}\n")?;

            for drawer in drawers {
                let source = drawer.source.as_deref().unwrap_or("unknown");
                writeln!(file, "## {}\n", drawer.id)?;
                writeln!(file, "{}\n", quote_content(&drawer.content))?;
                writeln!(file, "| Field | Value |")?;
                writeln!(file, "|-------|-------|")?;
                writeln!(file, "| Source | {source} |")?;
                writeln!(file, "| Filed | {} |", drawer.filed_at)?;
                writeln!(file, "| Last accessed | {} |", drawer.last_accessed)?;
                writeln!(file, "| Access count | {} |", drawer.access_count)?;
                writeln!(file, "| Importance | {:.2} |", drawer.importance)?;
                writeln!(file, "\n---\n")?;
            }
        }
    }

    index_lines.push(String::new());
    let index_path = output_dir.join("index.md");
    fs::write(&index_path, index_lines.join("\n"))
        .with_context(|| format!("Failed to write {index_path:?}"))?;

    Ok(stats)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use tempfile::tempdir;

    use crate::embedder_factory::EmbedderFactory;
    use crate::vector_storage::VectorStorage;

    fn make_test_storage() -> (VectorStorage, PathBuf) {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("vectors.db");
        let index_path = dir.path().join("vectors.usearch");
        let embedder = EmbedderFactory::get_embedder().unwrap();
        let vs = VectorStorage::new_with_embedder(&db_path, &index_path, embedder).unwrap();
        (vs, dir.path().to_path_buf())
    }

    fn seed_memories(vs: &mut VectorStorage) {
        vs.add_memory(
            "First project note",
            "projects",
            "2024-01-01",
            Some("notes.txt"),
            None,
        )
        .unwrap();
        vs.add_memory("Second project note", "projects", "2024-01-01", None, None)
            .unwrap();
        vs.add_memory(
            "A personal thought",
            "people",
            "alice",
            Some("journal.md"),
            None,
        )
        .unwrap();
        vs.add_memory("Another thought for Alice", "people", "alice", None, None)
            .unwrap();
    }

    #[test]
    fn test_export_format_from_str() {
        assert_eq!(ExportFormat::from_str("json").unwrap(), ExportFormat::Json);
        assert_eq!(ExportFormat::from_str("CSV").unwrap(), ExportFormat::Csv);
        assert_eq!(
            ExportFormat::from_str("Md").unwrap(),
            ExportFormat::Markdown
        );
        assert!(ExportFormat::from_str("yaml").is_err());
    }

    #[test]
    fn test_export_format_display() {
        assert_eq!(ExportFormat::Json.to_string(), "json");
        assert_eq!(ExportFormat::Csv.to_string(), "csv");
        assert_eq!(ExportFormat::Markdown.to_string(), "markdown");
    }

    #[test]
    fn test_export_json_all() {
        let (mut vs, _dir) = make_test_storage();
        seed_memories(&mut vs);

        let out = export_memories(&vs, ExportFormat::Json, None, None).unwrap();
        let parsed: Vec<ExportedMemory> = serde_json::from_str(&out).unwrap();
        assert_eq!(parsed.len(), 4);
        assert!(parsed
            .iter()
            .any(|r| r.wing == "projects" && r.room == "2024-01-01"));
        assert!(parsed
            .iter()
            .any(|r| r.wing == "people" && r.room == "alice"));
    }

    #[test]
    fn test_export_json_filter_wing() {
        let (mut vs, _dir) = make_test_storage();
        seed_memories(&mut vs);

        let out = export_memories(&vs, ExportFormat::Json, Some("projects"), None).unwrap();
        let parsed: Vec<ExportedMemory> = serde_json::from_str(&out).unwrap();
        assert_eq!(parsed.len(), 2);
        assert!(parsed.iter().all(|r| r.wing == "projects"));
    }

    #[test]
    fn test_export_json_filter_room() {
        let (mut vs, _dir) = make_test_storage();
        seed_memories(&mut vs);

        let out = export_memories(&vs, ExportFormat::Json, None, Some("alice")).unwrap();
        let parsed: Vec<ExportedMemory> = serde_json::from_str(&out).unwrap();
        assert_eq!(parsed.len(), 2);
        assert!(parsed.iter().all(|r| r.room == "alice"));
    }

    #[test]
    fn test_export_json_filter_both() {
        let (mut vs, _dir) = make_test_storage();
        seed_memories(&mut vs);

        let out = export_memories(
            &vs,
            ExportFormat::Json,
            Some("projects"),
            Some("2024-01-01"),
        )
        .unwrap();
        let parsed: Vec<ExportedMemory> = serde_json::from_str(&out).unwrap();
        assert_eq!(parsed.len(), 2);
    }

    #[test]
    fn test_export_json_no_match() {
        let (mut vs, _dir) = make_test_storage();
        seed_memories(&mut vs);

        let out = export_memories(&vs, ExportFormat::Json, Some("missing"), None).unwrap();
        let parsed: Vec<ExportedMemory> = serde_json::from_str(&out).unwrap();
        assert!(parsed.is_empty());
    }

    #[test]
    fn test_export_csv_all() {
        let (mut vs, _dir) = make_test_storage();
        seed_memories(&mut vs);

        let csv = export_memories(&vs, ExportFormat::Csv, None, None).unwrap();
        let mut reader = csv::ReaderBuilder::new().from_reader(csv.as_bytes());
        let headers = reader.headers().unwrap().clone();
        assert!(headers.iter().any(|h| h == "id"));
        assert!(headers.iter().any(|h| h == "content"));
        assert!(headers.iter().any(|h| h == "wing"));
        assert!(headers.iter().any(|h| h == "room"));

        let rows: Vec<ExportedMemory> = reader.deserialize().map(|r| r.unwrap()).collect();
        assert_eq!(rows.len(), 4);
        assert!(rows.iter().any(|r| r.wing == "projects"));
        assert!(rows.iter().any(|r| r.wing == "people"));
    }

    #[test]
    fn test_export_csv_filter_wing() {
        let (mut vs, _dir) = make_test_storage();
        seed_memories(&mut vs);

        let csv = export_memories(&vs, ExportFormat::Csv, Some("people"), None).unwrap();
        let mut reader = csv::ReaderBuilder::new().from_reader(csv.as_bytes());
        let rows: Vec<ExportedMemory> = reader.deserialize().map(|r| r.unwrap()).collect();
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().all(|r| r.wing == "people"));
    }

    #[test]
    fn test_export_csv_multiline_content_is_quoted() {
        let (mut vs, _dir) = make_test_storage();
        vs.add_memory(
            "Line one\nLine two\nLine three",
            "notes",
            "multiline",
            Some("file.md"),
            None,
        )
        .unwrap();

        let csv = export_memories(&vs, ExportFormat::Csv, None, None).unwrap();
        let mut reader = csv::ReaderBuilder::new().from_reader(csv.as_bytes());
        let rows: Vec<ExportedMemory> = reader.deserialize().map(|r| r.unwrap()).collect();
        let found = rows.into_iter().find(|r| r.wing == "notes").unwrap();
        assert!(found.content.contains('\n'));
    }

    #[test]
    fn test_export_markdown_string() {
        let (mut vs, _dir) = make_test_storage();
        seed_memories(&mut vs);

        let md = export_memories(&vs, ExportFormat::Markdown, None, None).unwrap();
        assert!(md.contains("# Palace Export"));
        assert!(md.contains("## Wing: projects"));
        assert!(md.contains("### Room: 2024-01-01"));
        assert!(md.contains("## Wing: people"));
        assert!(md.contains("### Room: alice"));
        assert!(md.contains("First project note"));
        assert!(md.contains("A personal thought"));
        assert!(md.contains("| Source | notes.txt |"));
        assert!(md.contains("| Access count |"));
    }

    #[test]
    fn test_export_markdown_string_empty() {
        let (vs, _dir) = make_test_storage();
        let md = export_memories(&vs, ExportFormat::Markdown, None, None).unwrap();
        assert!(md.contains("No memories found"));
    }

    #[test]
    fn test_export_markdown_string_filter() {
        let (mut vs, _dir) = make_test_storage();
        seed_memories(&mut vs);

        let md = export_memories(&vs, ExportFormat::Markdown, Some("people"), None).unwrap();
        assert!(md.contains("## Wing: people"));
        assert!(!md.contains("## Wing: projects"));
    }

    #[test]
    fn test_export_to_file_json() {
        let (mut vs, _dir) = make_test_storage();
        seed_memories(&mut vs);

        let out_dir = tempdir().unwrap();
        let file_path = out_dir.path().join("palace.json");
        let stats = export_to_file(&vs, &file_path, ExportFormat::Json, None, None).unwrap();
        assert_eq!(stats.drawers, 4);
        assert_eq!(stats.wings, 2);
        assert_eq!(stats.rooms, 2);

        let content = fs::read_to_string(&file_path).unwrap();
        let parsed: Vec<ExportedMemory> = serde_json::from_str(&content).unwrap();
        assert_eq!(parsed.len(), 4);
    }

    #[test]
    fn test_export_to_file_csv() {
        let (mut vs, _dir) = make_test_storage();
        seed_memories(&mut vs);

        let out_dir = tempdir().unwrap();
        let file_path = out_dir.path().join("palace.csv");
        let stats = export_to_file(&vs, &file_path, ExportFormat::Csv, None, None).unwrap();
        assert_eq!(stats.drawers, 4);

        let content = fs::read_to_string(&file_path).unwrap();
        let mut reader = csv::ReaderBuilder::new().from_reader(content.as_bytes());
        let rows: Vec<ExportedMemory> = reader.deserialize().map(|r| r.unwrap()).collect();
        assert_eq!(rows.len(), 4);
    }

    #[test]
    fn test_export_to_file_markdown_tree() {
        let (mut vs, _dir) = make_test_storage();
        seed_memories(&mut vs);

        let out_dir = tempdir().unwrap();
        let stats =
            export_to_file(&vs, out_dir.path(), ExportFormat::Markdown, None, None).unwrap();
        assert_eq!(stats.drawers, 4);
        assert_eq!(stats.wings, 2);
        assert_eq!(stats.rooms, 2);

        let index = out_dir.path().join("index.md");
        assert!(index.exists());
        let index_content = fs::read_to_string(&index).unwrap();
        assert!(index_content.contains("# Palace Export"));
        assert!(index_content.contains("projects"));
        assert!(index_content.contains("people"));

        let projects_dir = out_dir.path().join("projects");
        assert!(projects_dir.exists());
        let room_md = projects_dir.join("2024-01-01.md");
        assert!(room_md.exists());
        let room_content = fs::read_to_string(&room_md).unwrap();
        assert!(room_content.contains("# projects / 2024-01-01"));
        assert!(room_content.contains("First project note"));
        assert!(room_content.contains("Second project note"));

        let people_dir = out_dir.path().join("people");
        assert!(people_dir.exists());
        let alice_md = people_dir.join("alice.md");
        assert!(alice_md.exists());
    }

    #[test]
    fn test_export_to_file_markdown_tree_empty() {
        let (vs, _dir) = make_test_storage();
        let out_dir = tempdir().unwrap();
        let stats =
            export_to_file(&vs, out_dir.path(), ExportFormat::Markdown, None, None).unwrap();
        assert_eq!(stats.drawers, 0);
        assert_eq!(stats.wings, 0);
        assert_eq!(stats.rooms, 0);
        let index = out_dir.path().join("index.md");
        assert!(index.exists());
        let content = fs::read_to_string(&index).unwrap();
        assert!(content.contains("Palace is empty"));
    }

    #[test]
    fn test_export_to_file_markdown_tree_filter() {
        let (mut vs, _dir) = make_test_storage();
        seed_memories(&mut vs);

        let out_dir = tempdir().unwrap();
        let stats = export_to_file(
            &vs,
            out_dir.path(),
            ExportFormat::Markdown,
            Some("people"),
            None,
        )
        .unwrap();
        assert_eq!(stats.drawers, 2);
        assert_eq!(stats.wings, 1);
        assert_eq!(stats.rooms, 1);

        assert!(out_dir.path().join("people").exists());
        assert!(!out_dir.path().join("projects").exists());
    }

    #[test]
    fn test_safe_path_component() {
        assert_eq!(safe_path_component("foo/bar"), "foo_bar");
        assert_eq!(safe_path_component("a:b?c"), "a_b_c");
        assert_eq!(safe_path_component("   .hello.  "), "hello");
        assert_eq!(safe_path_component(""), "unknown");
        assert_eq!(safe_path_component("   "), "unknown");
        assert_eq!(safe_path_component("valid-name"), "valid-name");
    }

    #[test]
    fn test_quote_content() {
        let text = "one\ntwo\nthree";
        let quoted = quote_content(text);
        let lines: Vec<&str> = quoted.lines().collect();
        assert_eq!(lines, vec!["> one", "> two", "> three"]);
    }

    #[test]
    fn test_format_timestamp() {
        let s = format_timestamp(0);
        assert!(s.starts_with("1970-01-01"));
    }

    #[test]
    fn test_export_to_file_creates_parent_dirs() {
        let (mut vs, _dir) = make_test_storage();
        seed_memories(&mut vs);

        let out_dir = tempdir().unwrap();
        let nested = out_dir.path().join("a").join("b").join("palace.json");
        export_to_file(&vs, &nested, ExportFormat::Json, None, None).unwrap();
        assert!(nested.exists());
    }

    #[test]
    fn test_export_to_file_markdown_sanitised_names() {
        let (mut vs, _dir) = make_test_storage();
        vs.add_memory("unsafe name test", "wing:bad", "room:evil", None, None)
            .unwrap();

        let out_dir = tempdir().unwrap();
        export_to_file(&vs, out_dir.path(), ExportFormat::Markdown, None, None).unwrap();
        let wing_dir = out_dir.path().join("wing_bad");
        assert!(wing_dir.exists());
        assert!(wing_dir.join("room_evil.md").exists());
    }
}
