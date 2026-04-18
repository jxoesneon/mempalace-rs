use crate::config::MempalaceConfig;
use crate::models::Wing;
use crate::storage::Storage;
use crate::vector_storage::VectorStorage;
use anyhow::{anyhow, Result};
use serde_json::json;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

pub const READABLE_EXTENSIONS: &[&str] = &[
    ".txt", ".md", ".py", ".js", ".ts", ".jsx", ".tsx", ".json", ".yaml", ".yml", ".html", ".css",
    ".java", ".go", ".rs", ".rb", ".sh", ".csv", ".sql", ".toml",
];

pub const SKIP_DIRS: &[&str] = &[
    ".git",
    "node_modules",
    "__pycache__",
    ".venv",
    "venv",
    "env",
    "dist",
    "build",
    ".next",
    "coverage",
    ".mempalace",
    "target",
];

pub const CHUNK_SIZE: usize = 800;
pub const CHUNK_OVERLAP: usize = 100;
pub const MIN_CHUNK_SIZE: usize = 50;

pub fn chunk_text(content: &str) -> Vec<String> {
    let content = content.trim();
    if content.is_empty() {
        return vec![];
    }

    let mut chunks = Vec::new();
    let mut start = 0;

    while start < content.len() {
        // Ensure start is at a char boundary
        while start < content.len() && !content.is_char_boundary(start) {
            start += 1;
        }

        if start >= content.len() {
            break;
        }

        let mut end = std::cmp::min(start + CHUNK_SIZE, content.len());

        // Find nearest char boundary for end BEFORE any slicing
        while end > start && !content.is_char_boundary(end) {
            end -= 1;
        }

        if end < content.len() {
            // Try to break at paragraph boundary
            // Now safe to slice because we verified end is a char boundary
            let slice = &content[start..end];
            if let Some(newline_pos) = slice.rfind("\n\n") {
                if newline_pos > CHUNK_SIZE / 2 {
                    end = start + newline_pos;
                }
            } else if let Some(newline_pos) = slice.rfind('\n') {
                if newline_pos > CHUNK_SIZE / 2 {
                    end = start + newline_pos;
                }
            }
        }

        // Re-ensure end is at a char boundary after searching for newlines
        while end > start && !content.is_char_boundary(end) {
            end -= 1;
        }

        let chunk = content[start..end].trim();
        if chunk.len() >= MIN_CHUNK_SIZE {
            chunks.push(chunk.to_string());
        }

        if end >= content.len() {
            break;
        }

        // Compute overlap and ensure start is at a char boundary
        let next_start = if end > CHUNK_OVERLAP {
            let mut s = end - CHUNK_OVERLAP;
            while s < end && !content.is_char_boundary(s) {
                s += 1;
            }
            if s >= end {
                end
            } else {
                s
            }
        } else {
            0
        };

        if next_start <= start {
            start = end;
        } else {
            start = next_start;
        }
    }

    chunks
}

pub fn detect_room(
    filepath: &Path,
    content: &str,
    config: &MempalaceConfig,
    project_path: &Path,
) -> String {
    let relative = filepath
        .strip_prefix(project_path)
        .unwrap_or(filepath)
        .to_string_lossy()
        .to_lowercase();
    let filename = filepath
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_lowercase();
    let content_lower = content
        .chars()
        .take(2000)
        .collect::<String>()
        .to_lowercase();

    // Priority 1: folder path contains room name
    let path_parts: Vec<&str> = relative.split(['/', '\\']).collect();
    if path_parts.len() > 1 {
        for part in &path_parts[..path_parts.len() - 1] {
            for wing in &config.topic_wings {
                if !part.is_empty()
                    && (part.contains(&wing.to_lowercase()) || wing.to_lowercase().contains(part))
                {
                    return wing.clone();
                }
            }
        }
    }

    // Priority 2: filename matches room name
    for wing in &config.topic_wings {
        if !filename.is_empty()
            && (filename.contains(&wing.to_lowercase()) || wing.to_lowercase().contains(&filename))
        {
            return wing.clone();
        }
    }

    // Priority 3: keyword scoring
    let mut scores = HashMap::new();
    for (wing, keywords) in &config.hall_keywords {
        let mut score = 0;
        for kw in keywords {
            score += content_lower.matches(&kw.to_lowercase()).count();
        }
        if score > 0 {
            scores.insert(wing.clone(), score);
        }
    }

    if let Some((best, _)) = scores.into_iter().max_by_key(|&(_, count)| count) {
        return best;
    }

    "general".to_string()
}

pub fn get_mineable_files(project_path: &Path, no_gitignore: bool) -> Vec<std::path::PathBuf> {
    use ignore::WalkBuilder;
    let mut files = Vec::new();

    let canonical_root = match project_path.canonicalize() {
        Ok(p) => p,
        Err(_) => return files,
    };

    let mut builder = WalkBuilder::new(project_path);
    builder.follow_links(false);
    if no_gitignore {
        builder.ignore(false).git_ignore(false).git_exclude(false);
    }

    for entry in builder.build().flatten() {
        let path = entry.path();
        // Boundary check: only process entries whose canonical path can be
        // resolved and proven to stay under the canonical project root.
        // If canonicalization fails (broken symlink, permissions, race), skip.
        let canonical = match path.canonicalize() {
            Ok(c) => c,
            Err(_) => continue,
        };
        if !canonical.starts_with(&canonical_root) {
            continue;
        }
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        if SKIP_DIRS.contains(&name.as_ref()) {
            continue;
        }
        if path.is_file() {
            let extension = path.extension().and_then(|s| s.to_str()).unwrap_or("");
            let ext_with_dot = format!(".{}", extension);
            if READABLE_EXTENSIONS.contains(&ext_with_dot.as_str()) {
                let filename = path.file_name().unwrap_or_default().to_string_lossy();
                if filename != "mempalace.yaml"
                    && filename != "mempalace.json"
                    && filename != "package-lock.json"
                {
                    files.push(path.to_path_buf());
                }
            }
        }
    }
    files
}

use sha2::{Digest, Sha256};

pub fn prepare_documents(
    chunks: Vec<String>,
    wing_name: &str,
    room: &str,
    source_file: &str,
) -> (
    Vec<String>,
    Vec<String>,
    Vec<serde_json::Map<String, serde_json::Value>>,
) {
    let mut ids = Vec::new();
    let mut documents = Vec::new();
    let mut metadatas = Vec::new();

    let mtime = fs::metadata(source_file)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);

    for (i, chunk) in chunks.into_iter().enumerate() {
        let drawer_id = format!(
            "drawer_{}_{}_{}",
            wing_name,
            room,
            hash_string(&format!("{}{}", source_file, i))
        );
        ids.push(drawer_id);
        documents.push(chunk);
        metadatas.push(
            json!({
                "wing": wing_name,
                "room": room,
                "source_file": source_file,
                "source_mtime": mtime,
                "chunk_index": i,
                "filed_at": chrono::Utc::now().to_rfc3339(),
            })
            .as_object()
            .cloned()
            .unwrap_or_default(),
        );
    }
    (ids, documents, metadatas)
}

pub type ProjectFileResult = (
    String,
    Vec<String>,
    Vec<String>,
    Vec<serde_json::Map<String, serde_json::Value>>,
);

pub fn process_project_file(
    content: &str,
    wing_name: &str,
    source_file: &str,
    path: &Path,
    config: &MempalaceConfig,
    project_path: &Path,
) -> Option<ProjectFileResult> {
    let chunks = chunk_text(content);
    if chunks.is_empty() {
        return None;
    }
    let room = detect_room(path, content, config, project_path);
    let (ids, documents, metadatas) = prepare_documents(chunks, wing_name, &room, source_file);
    Some((room, ids, documents, metadatas))
}

#[derive(Debug, Clone, Default)]
pub struct MineOptions {
    pub wing_override: Option<String>,
    pub no_gitignore: bool,
    pub agent: Option<String>,
    pub limit: Option<usize>,
    pub dry_run: bool,
}

pub async fn mine_project(
    dir: &str,
    storage: &Storage,
    config: &MempalaceConfig,
    options: MineOptions,
) -> Result<()> {
    let project_path_raw = Path::new(dir);
    if !project_path_raw.exists() || !project_path_raw.is_dir() {
        return Err(anyhow!(
            "Directory does not exist or is not a directory: {}",
            dir
        ));
    }
    let project_path = project_path_raw.canonicalize()?;

    let files = get_mineable_files(&project_path, options.no_gitignore);
    if files.is_empty() {
        return Ok(());
    }

    let wing_name = options
        .wing_override
        .unwrap_or_else(|| "general".to_string());
    tracing::info!(
        "Mining project files in: {:?} into wing: {}",
        project_path,
        wing_name
    );

    let wing = Wing {
        name: wing_name.clone(),
        r#type: "project".to_string(),
        keywords: vec![],
    };

    match storage.add_wing(&wing) {
        Ok(_) => {}
        Err(e) => {
            if !e.to_string().contains("UNIQUE") {
                return Err(e.into());
            }
        }
    }

    // Initialize VectorStorage
    let mut vs = VectorStorage::new(
        config.config_dir.join("vectors.db"),
        config.config_dir.join("vectors.usearch"),
    )?;

    let mut processed_files = 0;
    const BATCH_SIZE: usize = 32;

    let mut batch_texts = Vec::new();
    let mut batch_wings = Vec::new();
    let mut batch_rooms = Vec::new();
    let mut batch_sources = Vec::new();
    let mut batch_mtimes = Vec::new();

    for path in files {
        if let Some(l) = options.limit {
            if processed_files >= l {
                break;
            }
        }
        let source_file = path.to_string_lossy().to_string();

        // Fast check: mtime
        let current_mtime = fs::metadata(&path)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0);

        if !options.dry_run {
            if let Some(stored_mtime) = vs.get_source_mtime(&source_file)? {
                if (stored_mtime - current_mtime).abs() < 0.001 {
                    processed_files += 1;
                    continue;
                }
            }
        }

        if let Ok(content) = fs::read_to_string(&path) {
            if let Some((room, _ids, documents, metadatas)) = process_project_file(
                &content,
                &wing_name,
                &source_file,
                &path,
                config,
                &project_path,
            ) {
                let mut count = 0usize;
                for (doc, mut meta) in documents.into_iter().zip(metadatas.into_iter()) {
                    if let Some(agent_name) = &options.agent {
                        meta.insert("author".to_string(), json!(agent_name));
                    }

                    if !options.dry_run {
                        batch_texts.push(doc);
                        batch_wings.push(wing_name.clone());
                        batch_rooms.push(room.clone());
                        batch_sources.push(Some(source_file.clone()));
                        batch_mtimes.push(Some(current_mtime));

                        if batch_texts.len() >= BATCH_SIZE {
                            vs.add_memories_batch(
                                std::mem::take(&mut batch_texts),
                                std::mem::take(&mut batch_wings),
                                std::mem::take(&mut batch_rooms),
                                std::mem::take(&mut batch_sources),
                                std::mem::take(&mut batch_mtimes),
                            )?;

                            // Periodic save to prevent data loss on crash
                            processed_files += 1; // Reuse for batch count
                            if processed_files % 10 == 0 {
                                vs.save_index(config.config_dir.join("vectors.usearch"))?;
                            }
                        }
                    }
                    count += 1;
                }
                let filename = path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| "unknown".to_string());
                println!(
                    "  ✓ {} {} drawers from {}",
                    if options.dry_run {
                        "[DRY RUN] Would file"
                    } else {
                        "Filed"
                    },
                    count,
                    filename
                );
            }
        }
        processed_files += 1;
    }

    if !options.dry_run {
        // Flush remaining batch
        if !batch_texts.is_empty() {
            vs.add_memories_batch(
                batch_texts,
                batch_wings,
                batch_rooms,
                batch_sources,
                batch_mtimes,
            )?;
        }
        vs.save_index(config.config_dir.join("vectors.usearch"))?;
    }

    Ok(())
}

fn hash_string(s: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(s.as_bytes());
    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::Storage;
    use std::fs;

    #[test]
    fn test_chunk_text() {
        let content = "A".repeat(1000);
        let chunks = chunk_text(&content);
        assert!(chunks.len() > 1);
    }

    #[test]
    fn test_chunk_text_empty() {
        assert!(chunk_text("").is_empty());
        assert!(chunk_text("   ").is_empty());
    }

    #[test]
    fn test_chunk_text_short() {
        let chunks = chunk_text("Hello world");
        assert!(chunks.is_empty()); // MIN_CHUNK_SIZE is 50
    }

    #[test]
    fn test_chunk_text_exact_min() {
        let content = "A".repeat(50);
        let chunks = chunk_text(&content);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0], content);
    }

    #[test]
    fn test_chunk_text_newline_break() {
        let part1 = "A".repeat(600);
        let part2 = "B".repeat(600);
        let content = format!("{}\n\n{}", part1, part2);
        let chunks = chunk_text(&content);
        // It should break at the double newline
        assert_eq!(chunks.len(), 2);
        assert!(chunks[0].contains('A'));
        assert!(!chunks[0].contains('B'));
    }

    #[test]
    fn test_chunk_text_long_with_breaks() {
        // Test paragraph break: \n\n after CHUNK_SIZE / 2 (which is 400)
        let part1 = "A".repeat(450);
        let part2 = "B".repeat(450);
        let content = format!("{}\n\n{}", part1, part2);
        let chunks = chunk_text(&content);
        assert!(chunks.len() >= 2);
        assert_eq!(chunks[0].len(), 450);

        // Test line break: \n after CHUNK_SIZE / 2
        let content_single = format!("{}\n{}", part1, part2);
        let chunks2 = chunk_text(&content_single);
        assert!(chunks2.len() >= 2);
        assert_eq!(chunks2[0].len(), 450);
    }

    #[test]
    fn test_detect_room() {
        let mut hall_keywords = std::collections::HashMap::new();
        hall_keywords.insert(
            "frontend".to_string(),
            vec!["react".to_string(), "css".to_string()],
        );
        hall_keywords.insert("room2".to_string(), vec!["banana".to_string()]);
        let config = MempalaceConfig {
            topic_wings: vec![
                "infra".to_string(),
                "backend".to_string(),
                "arch".to_string(),
            ],
            hall_keywords,
            ..Default::default()
        };

        let project_path = std::path::Path::new("/project");

        // Priority 1: Path contains room
        let path = std::path::Path::new("/project/infra/module.rs");
        assert_eq!(
            detect_room(path, "some code", &config, project_path),
            "infra"
        );

        let path_sub = std::path::Path::new("/project/infrastructure/module.rs");
        assert_eq!(detect_room(path_sub, "", &config, project_path), "infra");

        let path_super = std::path::Path::new("/project/inf/module.rs");
        assert_eq!(detect_room(path_super, "", &config, project_path), "infra");

        let path_exact = std::path::Path::new("/project/backend/module.rs");
        assert_eq!(
            detect_room(path_exact, "", &config, project_path),
            "backend"
        );

        let path2 = std::path::Path::new("/project/my_arch_folder/file.rs");
        assert_eq!(detect_room(path2, "", &config, project_path), "arch");

        // Priority 2: Filename contains room
        let path3 = std::path::Path::new("/project/src/infra.rs");
        assert_eq!(detect_room(path3, "", &config, project_path), "infra");

        let path4 = std::path::Path::new("/project/src/backend_utils.rs");
        assert_eq!(
            detect_room(path4, "some code", &config, project_path),
            "backend"
        );

        // Priority 3: Keywords
        let path5 = std::path::Path::new("/project/src/ui.rs");
        assert_eq!(
            detect_room(path5, "import react; write css;", &config, project_path),
            "frontend"
        );

        // Root path
        assert_eq!(
            detect_room(std::path::Path::new("/"), "", &config, project_path),
            "general"
        );
    }

    #[test]
    fn test_detect_room_keyword_scoring() {
        let mut hall_keywords = std::collections::HashMap::new();
        hall_keywords.insert("roomA".to_string(), vec!["apple".to_string()]);
        hall_keywords.insert("roomB".to_string(), vec!["banana".to_string()]);
        let config = MempalaceConfig {
            topic_wings: vec![],
            hall_keywords,
            ..Default::default()
        };

        let path = std::path::Path::new("/project/file.txt");
        let project_path = std::path::Path::new("/project");

        // Single match
        assert_eq!(
            detect_room(path, "apple apple", &config, project_path),
            "roomA"
        );

        // Multi match, roomB wins
        assert_eq!(
            detect_room(path, "apple banana banana", &config, project_path),
            "roomB"
        );
    }

    #[test]
    fn test_hash_string() {
        assert_eq!(hash_string("test"), hash_string("test"));
        assert_ne!(hash_string("test1"), hash_string("test2"));
        assert_ne!(hash_string("🦀"), hash_string("🦀🦀"));
        assert_ne!(
            hash_string(&"A".repeat(1000)),
            hash_string(&"A".repeat(1001))
        );
    }

    #[test]
    fn test_prepare_documents() {
        let chunks = vec!["chunk1".to_string(), "chunk2".to_string()];
        let (ids, docs, metadatas) =
            prepare_documents(chunks.clone(), "test_wing", "test_room", "test_file.rs");

        assert_eq!(ids.len(), 2);
        assert_eq!(docs.len(), 2);
        assert_eq!(metadatas.len(), 2);

        assert!(ids[0].starts_with("drawer_test_wing_test_room_"));
        assert_eq!(docs[0], "chunk1");
        assert_eq!(metadatas[0]["wing"].as_str().unwrap(), "test_wing");
        assert_eq!(metadatas[0]["chunk_index"].as_u64().unwrap(), 0);
    }

    #[test]
    fn test_get_mineable_files() {
        let temp_dir = tempfile::tempdir().unwrap();
        let path = temp_dir.path();

        // Create valid file
        fs::write(path.join("test.rs"), "fn main() {}").unwrap();
        // Create skip dir
        let git_dir = path.join(".git");
        fs::create_dir(&git_dir).unwrap();
        fs::write(git_dir.join("test2.rs"), "fn main() {}").unwrap();
        // Create unreadable file
        fs::write(path.join("test.bin"), "0101").unwrap();
        // Create ignored file
        fs::write(path.join("mempalace.yaml"), "").unwrap();

        // Create file with no extension
        fs::write(path.join("no_extension"), "test").unwrap();

        // Create valid file but with an extension that's not in the list
        fs::write(path.join("test.xyz"), "test").unwrap();

        let files = get_mineable_files(path, false);

        assert_eq!(files.len(), 1);
        assert!(files[0].to_string_lossy().ends_with("test.rs"));
    }

    #[test]
    fn test_process_project_file() {
        let content = "A".repeat(50);
        let temp_config_dir = tempfile::tempdir().unwrap();
        let config = MempalaceConfig::new(Some(temp_config_dir.path().to_path_buf()));
        let path = std::path::Path::new("/project/src/main.rs");
        let project_path = std::path::Path::new("/project");

        let result = process_project_file(
            &content,
            "test_wing",
            "test_file.rs",
            path,
            &config,
            project_path,
        );

        assert!(result.is_some());
        let (room, ids, docs, metadatas) = result.unwrap();
        assert_eq!(room, "general");
        assert_eq!(ids.len(), 1);
        assert_eq!(docs.len(), 1);
        assert_eq!(metadatas.len(), 1);

        let result_empty =
            process_project_file("", "test_wing", "test_file.rs", path, &config, project_path);
        assert!(result_empty.is_none());
    }

    #[tokio::test]
    async fn test_mine_project_invalid_dir() {
        let storage = Storage::new("test_mine.db").unwrap();
        let temp_config_dir = tempfile::tempdir().unwrap();
        let config = MempalaceConfig::new(Some(temp_config_dir.path().to_path_buf()));
        let result = mine_project(
            "/nonexistent/dir",
            &storage,
            &config,
            MineOptions::default(),
        )
        .await;
        assert!(result.is_err());
        let _ = fs::remove_file("test_mine.db");
    }

    #[tokio::test]
    async fn test_mine_project_storage_error() {
        let storage = Storage::new("test_mine_storage.db").unwrap();
        let temp_config_dir = tempfile::tempdir().unwrap();
        let config = MempalaceConfig::new(Some(temp_config_dir.path().to_path_buf()));
        let temp_dir = tempfile::tempdir().unwrap();
        // Add a file to trigger the DB connection step
        fs::write(temp_dir.path().join("test.rs"), "A".repeat(100)).unwrap();
        let result = mine_project(
            temp_dir.path().to_str().unwrap(),
            &storage,
            &config,
            MineOptions::default(),
        )
        .await;
        assert!(result.is_ok());
        let _ = fs::remove_file("test_mine_storage.db");
    }

    #[tokio::test]
    async fn test_mine_project_with_file() {
        let storage = Storage::new("test_mine_file.db").unwrap();
        let temp_config_dir = tempfile::tempdir().unwrap();
        let config = MempalaceConfig::new(Some(temp_config_dir.path().to_path_buf()));
        let temp_dir = tempfile::tempdir().unwrap();
        let file_path = temp_dir.path().join("main.rs");
        fs::write(&file_path, "A".repeat(100)).unwrap();

        let result = mine_project(
            temp_dir.path().to_str().unwrap(),
            &storage,
            &config,
            MineOptions::default(),
        )
        .await;
        assert!(result.is_ok());
        let _ = fs::remove_file("test_mine_file.db");
    }

    #[test]
    fn test_get_mineable_files_with_skips() {
        let temp_dir = tempfile::tempdir().unwrap();
        let path = temp_dir.path();

        // Valid file
        fs::write(path.join("main.rs"), "fn main() {}").unwrap();
        // Hidden folder
        fs::create_dir(path.join(".git")).unwrap();
        fs::write(path.join(".git").join("config"), "").unwrap();
        // Skip folder
        fs::create_dir(path.join("target")).unwrap();
        fs::write(path.join("target").join("debug"), "").unwrap();

        let files = get_mineable_files(path, false);
        assert_eq!(files.len(), 1);
        assert!(files[0].to_string_lossy().ends_with("main.rs"));
    }

    #[test]
    fn test_get_mineable_files_nested() {
        let temp_dir = tempfile::tempdir().unwrap();
        let path = temp_dir.path();
        fs::create_dir_all(path.join("a/b/c")).unwrap();
        fs::write(path.join("a/b/c/file.rs"), "").unwrap();
        let files = get_mineable_files(path, false);
        assert_eq!(files.len(), 1);

        // No extension
        fs::write(path.join("LICENSE"), "").unwrap();
        let files2 = get_mineable_files(path, false);
        assert_eq!(files2.len(), 1); // LICENSE should be skipped by extension check
    }

    #[test]
    #[cfg(unix)]
    fn test_get_mineable_files_excludes_symlinks_outside_root() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();

        // Create a legitimate file inside root
        fs::write(root.path().join("real.rs"), "fn main() {}").unwrap();

        // Create a file outside root that we will try to reach via symlink
        let outside_file = outside.path().join("secret.rs");
        fs::write(&outside_file, "secret content").unwrap();

        // Create a symlink inside root that points outside
        let link_path = root.path().join("escape.rs");
        std::os::unix::fs::symlink(&outside_file, &link_path).unwrap();

        let files = get_mineable_files(root.path(), false);

        // Only the real file inside root should be returned; the escaping symlink must be excluded.
        assert_eq!(
            files.len(),
            1,
            "symlink pointing outside root must be excluded"
        );
        assert!(files[0].ends_with("real.rs"));
    }

    #[test]
    fn test_process_project_file_empty() {
        let config = MempalaceConfig::default();
        let project_path = std::path::Path::new("/project");
        let result = process_project_file(
            "",
            "wing",
            "file.rs",
            &project_path.join("file.rs"),
            &config,
            project_path,
        );
        assert!(result.is_none());
    }
}
