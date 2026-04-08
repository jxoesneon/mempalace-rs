use crate::config::MempalaceConfig;
use crate::storage::Storage;
use crate::vector_storage::VectorStorage;
use anyhow::Result;
use chrono;
use serde_json::json;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use walkdir::WalkDir;

pub const CONVO_EXTENSIONS: &[&str] = &[".txt", ".md", ".json", ".jsonl"];
pub const MIN_CHUNK_SIZE: usize = 30;

pub type ConvoDocuments = (
    Vec<String>,
    Vec<String>,
    Vec<serde_json::Map<String, serde_json::Value>>,
);

pub fn chunk_exchanges(content: &str) -> Vec<String> {
    let lines: Vec<&str> = content.lines().collect();
    let quote_lines = lines.iter().filter(|l| l.trim().starts_with('>')).count();

    if quote_lines >= 3 {
        chunk_by_exchange(&lines)
    } else {
        chunk_by_paragraph(content)
    }
}

fn chunk_by_exchange(lines: &[&str]) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut i = 0;

    while i < lines.len() {
        let line = lines[i].trim();
        if line.starts_with('>') {
            let user_turn = line;
            i += 1;

            let mut ai_lines = Vec::new();
            while i < lines.len() {
                let next_line = lines[i].trim();
                if next_line.starts_with('>') || next_line.starts_with("---") {
                    break;
                }
                if !next_line.is_empty() {
                    ai_lines.push(next_line);
                }
                i += 1;
            }

            let ai_response = ai_lines
                .iter()
                .take(8)
                .cloned()
                .collect::<Vec<_>>()
                .join(" ");
            let content = if !ai_response.is_empty() {
                format!("{}\n{}", user_turn, ai_response)
            } else {
                user_turn.to_string()
            };

            if content.trim().len() > MIN_CHUNK_SIZE {
                chunks.push(content);
            }
        } else {
            i += 1;
        }
    }

    chunks
}

fn chunk_by_paragraph(content: &str) -> Vec<String> {
    let mut chunks = Vec::new();
    let paragraphs: Vec<&str> = content
        .split("\n\n")
        .map(|p| p.trim())
        .filter(|p| !p.is_empty())
        .collect();

    if paragraphs.len() <= 1 && content.chars().filter(|&c| c == '\n').count() > 20 {
        let lines: Vec<&str> = content.lines().collect();
        for chunk in lines.chunks(25) {
            let group = chunk.join("\n").trim().to_string();
            if group.len() > MIN_CHUNK_SIZE {
                chunks.push(group);
            }
        }
        return chunks;
    }

    for para in paragraphs {
        if para.len() > MIN_CHUNK_SIZE {
            chunks.push(para.to_string());
        }
    }

    chunks
}

pub fn detect_convo_room(content: &str) -> String {
    let content_lower = content
        .chars()
        .take(3000)
        .collect::<String>()
        .to_lowercase();
    let mut scores = HashMap::new();

    let topic_keywords: HashMap<&str, &[&str]> = [
        (
            "technical",
            &[
                "code", "python", "function", "bug", "error", "api", "database", "server",
                "deploy", "git", "test", "debug", "refactor",
            ][..],
        ),
        (
            "architecture",
            &[
                "architecture",
                "design",
                "pattern",
                "structure",
                "schema",
                "interface",
                "module",
                "component",
                "service",
                "layer",
            ][..],
        ),
        (
            "planning",
            &[
                "plan",
                "roadmap",
                "milestone",
                "deadline",
                "priority",
                "sprint",
                "backlog",
                "scope",
                "requirement",
                "spec",
            ][..],
        ),
        (
            "decisions",
            &[
                "decided",
                "chose",
                "picked",
                "switched",
                "migrated",
                "replaced",
                "trade-off",
                "alternative",
                "option",
                "approach",
            ][..],
        ),
        (
            "problems",
            &[
                "problem",
                "issue",
                "broken",
                "failed",
                "crash",
                "stuck",
                "workaround",
                "fix",
                "solved",
                "resolved",
            ][..],
        ),
    ]
    .iter()
    .cloned()
    .collect();

    for (room, keywords) in topic_keywords {
        let score = keywords
            .iter()
            .filter(|&&kw| content_lower.contains(kw))
            .count();
        if score > 0 {
            scores.insert(room.to_string(), score);
        }
    }

    if let Some((best, _)) = scores.into_iter().max_by_key(|&(_, count)| count) {
        return best;
    }
    "general".to_string()
}

pub fn get_mineable_convo_files(convo_path: &Path) -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();
    for entry in WalkDir::new(convo_path)
        .into_iter()
        .filter_entry(|e| {
            let name = e.file_name().to_string_lossy();
            !crate::miner::SKIP_DIRS.contains(&name.as_ref())
        })
        .flatten()
    {
        let path = entry.path();
        if path.is_file() {
            let extension = path.extension().and_then(|s| s.to_str()).unwrap_or("");
            let ext_with_dot = format!(".{}", extension);
            if CONVO_EXTENSIONS.contains(&ext_with_dot.as_str()) {
                files.push(path.to_path_buf());
            }
        }
    }
    files
}

pub fn prepare_convo_documents(
    chunks: Vec<String>,
    wing: &str,
    room: &str,
    source_file: &str,
) -> ConvoDocuments {
    let mut ids = Vec::new();
    let mut documents = Vec::new();
    let mut metadatas = Vec::new();

    for (i, chunk) in chunks.into_iter().enumerate() {
        let drawer_id = format!(
            "drawer_{}_{}_{}_{}",
            wing,
            room,
            hash_string(source_file),
            i
        );
        ids.push(drawer_id.clone());
        documents.push(chunk.clone());
        metadatas.push(
            json!({
                "wing": wing,
                "room": room,
                "source_file": source_file,
                "chunk_index": i,
                "filed_at": chrono::Utc::now().to_rfc3339(),
                "ingest_mode": "convos",
            })
            .as_object()
            .unwrap()
            .clone(),
        );
    }

    (ids, documents, metadatas)
}

pub fn process_convo_file(content: &str, wing: &str, source_file: &str) -> Option<ConvoDocuments> {
    let chunks = chunk_exchanges(content);
    if chunks.is_empty() {
        return None;
    }
    let room = detect_convo_room(content);
    Some(prepare_convo_documents(chunks, wing, &room, source_file))
}

pub async fn mine_convos(
    dir: &str,
    _storage: &Storage,
    config: &MempalaceConfig,
    wing_override: Option<&str>,
) -> Result<()> {
    let convo_path_raw = Path::new(dir);
    if !convo_path_raw.exists() {
        return Ok(());
    }
    let convo_path = convo_path_raw.canonicalize()?;

    let files = get_mineable_convo_files(&convo_path);
    if files.is_empty() {
        return Ok(());
    }

    let wing = wing_override
        .unwrap_or_else(|| {
            convo_path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("convos")
        })
        .to_string();

    println!(
        "Mining conversations in: {:?} into wing: {}",
        convo_path, wing
    );

    // Initialize VectorStorage
    let mut vs = VectorStorage::new(
        config.config_dir.join("vectors.db"),
        config.config_dir.join("vectors.usearch"),
    )?;

    for path in files {
        let source_file = path.to_string_lossy().to_string();

        // Skip if already filed
        if vs.has_source_file(&source_file).unwrap_or(false) {
            continue;
        }

        if let Ok(content) = fs::read_to_string(&path) {
            if let Some((_ids, documents, _metadatas)) =
                process_convo_file(&content, &wing, &source_file)
            {
                let count = documents.len();
                for doc in &documents {
                    vs.add_memory(doc, &wing, "convos", Some(&source_file), None)?;
                }
                println!(
                    "  ✓ Filed {} drawers from {}",
                    count,
                    path.file_name().unwrap().to_string_lossy()
                );
            }
        }
    }
    vs.save_index(config.config_dir.join("vectors.usearch"))?;

    Ok(())
}

fn hash_string(s: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    s.hash(&mut hasher);
    format!("{:x}", hasher.finish())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chunk_exchanges_by_exchange() {
        let q1 = format!("> user query 1 {}", "A".repeat(50));
        let a1 = format!("AI response 1 {}", "B".repeat(50));
        let q2 = format!("> user query 2 {}", "C".repeat(50));
        let a2 = format!("AI response 2 {}", "D".repeat(50));
        let q3 = format!("> user query 3 {}", "E".repeat(50));
        let a3 = format!("AI response 3 {}", "F".repeat(50));

        let content = format!("{}\n{}\n{}\n{}\n{}\n{}\n", q1, a1, q2, a2, q3, a3);
        let chunks = chunk_exchanges(&content);
        assert_eq!(chunks.len(), 3);
        assert!(chunks[0].contains("> user query 1"));
        assert!(chunks[1].contains("> user query 2"));
        assert!(chunks[2].contains("> user query 3"));
    }

    #[test]
    fn test_chunk_exchanges_empty_ai() {
        // Trigger chunk_by_exchange by having 3+ quote lines
        let q1 = format!("> user query 1 {}", "A".repeat(30));
        let q2 = format!("> user query 2 {}", "B".repeat(30));
        let q3 = format!("> user query 3 {}", "C".repeat(30));
        let content = format!("{}\n{}\n{}\n", q1, q2, q3);
        let chunks = chunk_exchanges(&content);
        assert_eq!(chunks.len(), 3);
    }

    #[test]
    fn test_chunk_exchanges_by_paragraph() {
        let content = "Para 1\n\nPara 2\n\nPara 3\n\nPara 4\n";
        let chunks = chunk_exchanges(content);
        // MIN_CHUNK_SIZE is 30, so "Para X" is skipped.
        assert_eq!(chunks.len(), 0);

        let long_para1 = "A".repeat(50);
        let long_para2 = "B".repeat(50);
        let content2 = format!("{}\n\n{}", long_para1, long_para2);
        let chunks2 = chunk_exchanges(&content2);
        assert_eq!(chunks2.len(), 2);
    }

    #[test]
    fn test_chunk_by_paragraph_single_long_text() {
        let lines: Vec<String> = (0..30).map(|i| format!("Line {}", i)).collect();
        let content = lines.join("\n");
        let chunks = chunk_by_paragraph(&content);
        // It should split into chunks of 25 lines
        assert_eq!(chunks.len(), 2);
    }

    #[test]
    fn test_chunk_by_exchange_with_separator() {
        let q1 = format!("> User input {}", "A".repeat(50));
        let q2 = format!("> User 2 {}", "B".repeat(50));
        let q3 = format!("> User 3 {}", "C".repeat(50));

        let content = format!(
            "{}\nLine 1\nLine 2\n---\nIgnored line\n{}\nAI 2\n{}\nAI 3",
            q1, q2, q3
        );
        let chunks = chunk_exchanges(&content);
        assert_eq!(chunks.len(), 3);
        // Ensure "Ignored line" is not in the first chunk because of ---
        assert!(!chunks[0].contains("Ignored line"));
    }

    #[test]
    fn test_detect_convo_room() {
        assert_eq!(
            detect_convo_room("python code api database deploy git test"),
            "technical"
        );
        assert_eq!(
            detect_convo_room("architecture design pattern module"),
            "architecture"
        );
        assert_eq!(
            detect_convo_room("plan roadmap milestone deadline scope"),
            "planning"
        );
        assert_eq!(
            detect_convo_room("we decided and chose to migrate"),
            "decisions"
        );
        assert_eq!(
            detect_convo_room("issue crashed broken workaround"),
            "problems"
        );
        assert_eq!(detect_convo_room("hello world how are you"), "general");
    }

    #[test]
    fn test_prepare_convo_documents() {
        let chunks = vec!["chunk1".to_string(), "chunk2".to_string()];
        let (ids, docs, metadatas) =
            prepare_convo_documents(chunks.clone(), "test_wing", "test_room", "test_file.md");

        assert_eq!(ids.len(), 2);
        assert_eq!(docs.len(), 2);
        assert_eq!(metadatas.len(), 2);

        assert!(ids[0].starts_with("drawer_test_wing_test_room_"));
        assert_eq!(docs[0], "chunk1");
        assert_eq!(metadatas[0]["wing"].as_str().unwrap(), "test_wing");
        assert_eq!(metadatas[0]["ingest_mode"].as_str().unwrap(), "convos");
        assert_eq!(metadatas[0]["chunk_index"].as_u64().unwrap(), 0);
    }

    #[test]
    fn test_process_convo_file() {
        // empty content
        assert!(process_convo_file("", "wing", "file.md").is_none());

        let q1 = format!("> user query 1 {}", "A".repeat(50));
        let content = format!("{}\nAI response", q1);
        let result = process_convo_file(&content, "wing", "file.md");
        assert!(result.is_some());
        let (ids, docs, _metas) = result.unwrap();
        assert_eq!(ids.len(), 1);
        assert_eq!(docs.len(), 1);
    }

    #[tokio::test]
    async fn test_mine_convos_error() {
        let storage = Storage::new("test_convo_storage.db").unwrap();
        let config = MempalaceConfig::default();

        // Add a file to trigger connection error
        let temp_dir = tempfile::tempdir().unwrap();
        std::fs::write(temp_dir.path().join("test.txt"), "hello").unwrap();
        let result = mine_convos(temp_dir.path().to_str().unwrap(), &storage, &config, None).await;
        assert!(result.is_err());

        let _ = std::fs::remove_file("test_convo_storage.db");
    }

    #[test]
    fn test_get_mineable_convo_files() {
        let temp_dir = tempfile::tempdir().unwrap();
        let path = temp_dir.path();

        // Create valid files
        std::fs::write(path.join("test.md"), "test").unwrap();
        std::fs::write(path.join("test.jsonl"), "test").unwrap();

        // Create skip dir
        let skip_dir = path.join(".git");
        std::fs::create_dir(&skip_dir).unwrap();
        std::fs::write(skip_dir.join("test.md"), "test").unwrap();

        // Create unreadable file (not in CONVO_EXTENSIONS)
        std::fs::write(path.join("test.bin"), "0101").unwrap();

        let files = get_mineable_convo_files(path);

        assert_eq!(files.len(), 2);
        assert!(files
            .iter()
            .any(|f| f.to_string_lossy().ends_with("test.md")));
        assert!(files
            .iter()
            .any(|f| f.to_string_lossy().ends_with("test.jsonl")));
    }

    #[tokio::test]
    async fn test_mine_convos_main_logic() {
        let storage = Storage::new("test_convo_main.db").unwrap();
        let config = MempalaceConfig::default();
        let result = mine_convos("/non/existent/path", &storage, &config, None).await;
        assert!(result.is_ok());
    }

    #[test]
    fn test_process_convo_file_empty() {
        let result = process_convo_file("", "wing", "test.md");
        assert!(result.is_none());
    }

    #[test]
    fn test_detect_convo_room_tie() {
        // architecture should win easily
        assert_eq!(
            detect_convo_room("architecture architecture architecture architecture"),
            "architecture"
        );
    }
}
