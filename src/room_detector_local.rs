//! Local room detection without external API calls.
//!
//! Two ways to define rooms:
//!   1. Auto-detect from folder structure (zero config).
//!   2. Define manually in `mempalace.yaml` or `mempalace.json`.
//!
//! No internet. No API key. Your files stay on your machine.

use crate::shared::is_skip_dir;
use anyhow::{Context, Result};
use lazy_static::lazy_static;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// A detected or configured room suggestion.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoomSuggestion {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub keywords: Vec<String>,
    /// The original folder or source that produced this room, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

impl RoomSuggestion {
    /// Create a room suggestion with a name, description, and optional source.
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        source: Option<String>,
    ) -> Self {
        let name = name.into();
        let description = description.into();
        let keywords = vec![name.clone()];
        Self {
            name,
            description,
            keywords,
            source,
        }
    }

    /// Add a keyword to the room if not already present.
    pub fn with_keyword(mut self, keyword: impl Into<String>) -> Self {
        let kw = keyword.into();
        if !self.keywords.contains(&kw) {
            self.keywords.push(kw);
        }
        self
    }
}

/// Explicit room configuration loaded from `mempalace.yaml` / `mempalace.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoomConfig {
    /// Optional wing name for the project.
    #[serde(default)]
    pub wing: Option<String>,
    /// Explicitly configured rooms.
    pub rooms: Vec<RoomSuggestion>,
}

lazy_static! {
    /// Mapping from folder / filename keywords to canonical room names.
    static ref FOLDER_ROOM_MAP: HashMap<&'static str, &'static str> = {
        let mut m = HashMap::new();
        m.insert("frontend", "frontend");
        m.insert("front-end", "frontend");
        m.insert("front_end", "frontend");
        m.insert("client", "frontend");
        m.insert("ui", "frontend");
        m.insert("views", "frontend");
        m.insert("components", "frontend");
        m.insert("pages", "frontend");
        m.insert("backend", "backend");
        m.insert("back-end", "backend");
        m.insert("back_end", "backend");
        m.insert("server", "backend");
        m.insert("api", "backend");
        m.insert("routes", "backend");
        m.insert("services", "backend");
        m.insert("controllers", "backend");
        m.insert("models", "backend");
        m.insert("database", "backend");
        m.insert("db", "backend");
        m.insert("docs", "documentation");
        m.insert("doc", "documentation");
        m.insert("documentation", "documentation");
        m.insert("wiki", "documentation");
        m.insert("readme", "documentation");
        m.insert("notes", "documentation");
        m.insert("design", "design");
        m.insert("designs", "design");
        m.insert("mockups", "design");
        m.insert("wireframes", "design");
        m.insert("assets", "design");
        m.insert("storyboard", "design");
        m.insert("costs", "costs");
        m.insert("cost", "costs");
        m.insert("budget", "costs");
        m.insert("finance", "costs");
        m.insert("financial", "costs");
        m.insert("pricing", "costs");
        m.insert("invoices", "costs");
        m.insert("accounting", "costs");
        m.insert("meetings", "meetings");
        m.insert("meeting", "meetings");
        m.insert("calls", "meetings");
        m.insert("meeting_notes", "meetings");
        m.insert("standup", "meetings");
        m.insert("minutes", "meetings");
        m.insert("team", "team");
        m.insert("staff", "team");
        m.insert("hr", "team");
        m.insert("hiring", "team");
        m.insert("employees", "team");
        m.insert("people", "team");
        m.insert("research", "research");
        m.insert("references", "research");
        m.insert("reading", "research");
        m.insert("papers", "research");
        m.insert("planning", "planning");
        m.insert("roadmap", "planning");
        m.insert("strategy", "planning");
        m.insert("specs", "planning");
        m.insert("requirements", "planning");
        m.insert("tests", "testing");
        m.insert("test", "testing");
        m.insert("testing", "testing");
        m.insert("qa", "testing");
        m.insert("scripts", "scripts");
        m.insert("tools", "scripts");
        m.insert("utils", "scripts");
        m.insert("config", "configuration");
        m.insert("configs", "configuration");
        m.insert("settings", "configuration");
        m.insert("infrastructure", "configuration");
        m.insert("infra", "configuration");
        m.insert("deploy", "configuration");
        m.insert("src", "source");
        m.insert("source", "source");
        m.insert("sources", "source");
        m
    };

}

/// Normalize a folder/file name into a lookup key.
fn normalize_name(name: &str) -> String {
    name.to_lowercase().replace(['-', ' '], "_")
}

/// Check if a name looks like a plausible room name (starts with a letter, length > 2).
fn is_valid_room_name(name: &str) -> bool {
    let trimmed = name.trim();
    trimmed.len() > 2
        && trimmed
            .chars()
            .next()
            .map(|c| c.is_alphabetic())
            .unwrap_or(false)
}

/// Load explicit room configuration from `mempalace.yaml`, `mempalace.yml`, or `mempalace.json`.
///
/// Returns `Ok(None)` if no config file is present.
pub fn load_room_config(project_dir: &Path) -> Result<Option<RoomConfig>> {
    let candidates = [
        project_dir.join("mempalace.yaml"),
        project_dir.join("mempalace.yml"),
        project_dir.join("mempalace.json"),
    ];

    for path in &candidates {
        if !path.exists() {
            continue;
        }
        let content = fs::read_to_string(path)
            .with_context(|| format!("Failed to read room config: {}", path.display()))?;

        let config: RoomConfig = if path.extension().and_then(|e| e.to_str()) == Some("json") {
            serde_json::from_str(&content)
                .with_context(|| format!("Invalid JSON room config: {}", path.display()))?
        } else {
            serde_yaml::from_str(&content)
                .with_context(|| format!("Invalid YAML room config: {}", path.display()))?
        };

        return Ok(Some(config));
    }

    Ok(None)
}

/// Detect rooms from a list of file system paths.
///
/// Each path is inspected as a potential top-level directory. Directory names
/// are mapped to canonical room names via the built-in keyword map. Any
/// directory name that is not in the map but looks like a valid room name is
/// also accepted as-is.
///
/// The result always includes a `general` fallback room.
pub fn detect_rooms_from_paths(paths: &[PathBuf]) -> Result<Vec<RoomSuggestion>> {
    let mut found: HashMap<String, String> = HashMap::new();

    for path in paths {
        let is_dir = fs::metadata(path).map(|m| m.is_dir()).unwrap_or(false);
        if !is_dir {
            continue;
        }

        let original = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if original.is_empty() || is_skip_dir(original) {
            continue;
        }

        let key = normalize_name(original);
        if let Some(&room_name) = FOLDER_ROOM_MAP.get(key.as_str()) {
            if !found.contains_key(room_name) {
                found.insert(room_name.to_string(), original.to_string());
            }
        } else if is_valid_room_name(original) && !found.contains_key(&key) {
            found.insert(key.clone(), original.to_string());
        }
    }

    Ok(build_rooms(found, "Files from {}/"))
}

/// Detect rooms from a project directory.
///
/// If a `mempalace.yaml` / `mempalace.json` config exists, the configured rooms
/// are returned. Otherwise, the folder structure is scanned and then filename
/// patterns are used as a fallback.
///
/// The result always includes a `general` fallback room.
pub fn detect_rooms_from_dir(project_dir: &Path) -> Result<Vec<RoomSuggestion>> {
    if let Some(config) = load_room_config(project_dir)? {
        return Ok(ensure_general(config.rooms));
    }

    detect_rooms_from_folders(project_dir)
}

/// Main entry point for local room detection.
///
/// Returns the detected rooms and a human-readable source label.
///
/// * If an explicit config exists, `source` is `"explicit config"`.
/// * If folder detection finds more than just `general`, `source` is
///   `"folder structure"`.
/// * If filename patterns are used, `source` is `"filename patterns"`.
/// * Otherwise, `source` is `"fallback (flat project)"`.
pub fn detect_rooms_local(project_dir: &Path) -> Result<(Vec<RoomSuggestion>, String)> {
    if load_room_config(project_dir)?.is_some() {
        return Ok((
            detect_rooms_from_dir(project_dir)?,
            "explicit config".into(),
        ));
    }

    let mut rooms = detect_rooms_from_folders(project_dir)?;
    let mut source = "folder structure";

    // If only "general" was found, fall back to filename patterns.
    if rooms.len() <= 1 {
        rooms = detect_rooms_from_files(project_dir)?;
        source = if rooms.len() <= 1 {
            "fallback (flat project)"
        } else {
            "filename patterns"
        };
    }

    Ok((rooms, source.into()))
}

/// Detect rooms by scanning the top-level and one-level-deep subdirectories.
fn detect_rooms_from_folders(project_dir: &Path) -> Result<Vec<RoomSuggestion>> {
    let mut found: HashMap<String, String> = HashMap::new();

    if !project_dir.exists() {
        return Err(anyhow::anyhow!(
            "Directory not found: {}",
            project_dir.display()
        ));
    }

    // Top-level directories.
    for entry in read_dir_skip_fail(project_dir) {
        let Some((name, _is_dir)) = entry else {
            continue;
        };
        if !is_dir(&project_dir.join(&name)) {
            continue;
        }
        if is_skip_dir(name.as_str()) {
            continue;
        }
        process_folder_name(&name, &mut found);
    }

    // Walk one level deeper for nested patterns.
    for entry in read_dir_skip_fail(project_dir) {
        let Some((name, _)) = entry else { continue };
        let subdir = project_dir.join(&name);
        if !is_dir(&subdir) || is_skip_dir(name.as_str()) {
            continue;
        }
        for subentry in read_dir_skip_fail(&subdir) {
            let Some((subname, _)) = subentry else {
                continue;
            };
            let subpath = subdir.join(&subname);
            if !is_dir(&subpath) || is_skip_dir(subname.as_str()) {
                continue;
            }
            process_folder_name(&subname, &mut found);
        }
    }

    Ok(build_rooms(found, "Files from {}/"))
}

/// Detect rooms from recurring filename patterns.
///
/// Returns the most common mapped rooms that appear at least twice, capped at
/// six rooms. If no patterns are found, a single `general` room is returned.
fn detect_rooms_from_files(project_dir: &Path) -> Result<Vec<RoomSuggestion>> {
    let mut counts: HashMap<String, usize> = HashMap::new();

    for entry in walk_dir_limited(project_dir, 2) {
        let filename = entry
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();
        let name_lower = normalize_name(&filename);
        for (keyword, room) in FOLDER_ROOM_MAP.iter() {
            if name_lower.contains(keyword) {
                *counts.entry(room.to_string()).or_insert(0) += 1;
            }
        }
    }

    let mut sorted: Vec<(String, usize)> = counts.into_iter().collect();
    sorted.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));

    let mut rooms = Vec::new();
    for (room, count) in sorted {
        if count >= 2 && rooms.len() < 6 {
            rooms.push(RoomSuggestion::new(
                &room,
                format!("Files related to {room}"),
                None,
            ));
        }
    }

    Ok(ensure_general(rooms))
}

/// Print a proposed room structure to stdout.
pub fn print_proposed_structure(
    project_name: &str,
    rooms: &[RoomSuggestion],
    total_files: usize,
    source: &str,
) {
    println!("\n{}", "=".repeat(55));
    println!("  MemPalace Init — Local setup");
    println!("{}\n", "=".repeat(55));
    println!("  WING: {project_name}");
    println!("  ({total_files} files found, rooms detected from {source})\n");
    for room in rooms {
        println!("    ROOM: {}", room.name);
        println!("          {}", room.description);
    }
    println!("\n{}", "─".repeat(55));
}

/// Count regular files under a directory, ignoring skipped directories.
///
/// This is a shallow-ish count: files directly under the directory and
/// files one level deep are counted, but deeply nested directories are
/// ignored to keep the operation fast for project summaries.
pub fn count_files(project_dir: &Path) -> usize {
    if !project_dir.exists() {
        return 0;
    }
    walk_dir_limited(project_dir, 2).len()
}

/// Save an explicit room configuration to `mempalace.yaml`.
///
/// The `wing` field is set from the project directory name if not provided.
pub fn save_config(
    project_dir: &Path,
    wing: Option<&str>,
    rooms: &[RoomSuggestion],
) -> Result<PathBuf> {
    let config = RoomConfig {
        wing: wing.map(|s| s.to_string()),
        rooms: rooms.to_vec(),
    };
    let path = project_dir.join("mempalace.yaml");
    let content = serde_yaml::to_string(&config)
        .with_context(|| "Failed to serialize room config to YAML")?;
    fs::write(&path, content)
        .with_context(|| format!("Failed to write room config: {}", path.display()))?;
    Ok(path)
}

// --- internal helpers --------------------------------------------------------

fn process_folder_name(name: &str, found: &mut HashMap<String, String>) {
    let key = normalize_name(name);
    if let Some(&room_name) = FOLDER_ROOM_MAP.get(key.as_str()) {
        if !found.contains_key(room_name) {
            found.insert(room_name.to_string(), name.to_string());
        }
    } else if is_valid_room_name(name) && !found.contains_key(&key) {
        found.insert(key, name.to_string());
    }
}

fn build_rooms(found: HashMap<String, String>, description_template: &str) -> Vec<RoomSuggestion> {
    let mut rooms: Vec<RoomSuggestion> = found
        .into_iter()
        .map(|(room_name, original)| {
            let mut suggestion = RoomSuggestion::new(
                room_name.clone(),
                description_template.replace("{}", &original),
                Some(original.clone()),
            );
            suggestion = suggestion.with_keyword(room_name.clone());
            suggestion
        })
        .collect();

    rooms.sort_by(|a, b| a.name.cmp(&b.name));
    ensure_general(rooms)
}

fn ensure_general(rooms: Vec<RoomSuggestion>) -> Vec<RoomSuggestion> {
    let mut rooms = rooms;
    if !rooms.iter().any(|r| r.name == "general") {
        rooms.push(RoomSuggestion::new(
            "general",
            "Files that don't fit other rooms",
            None,
        ));
    }
    rooms
}

fn is_dir(path: &Path) -> bool {
    fs::metadata(path).map(|m| m.is_dir()).unwrap_or(false)
}

/// Read a directory, skipping entries that cannot be queried (e.g., reparse points).
fn read_dir_skip_fail(path: &Path) -> Vec<Option<(String, bool)>> {
    let mut results = Vec::new();
    if let Ok(entries) = fs::read_dir(path) {
        for entry in entries {
            match entry {
                Ok(e) => {
                    let name = e.file_name().to_string_lossy().into_owned();
                    let is_dir = e.metadata().map(|m| m.is_dir()).unwrap_or(false);
                    results.push(Some((name, is_dir)));
                }
                Err(_) => results.push(None),
            }
        }
    }
    results
}

/// Walk a directory tree up to a given depth, skipping ignored directories.
fn walk_dir_limited(path: &Path, max_depth: usize) -> Vec<PathBuf> {
    let mut results = Vec::new();
    if max_depth == 0 {
        return results;
    }
    if let Ok(entries) = fs::read_dir(path) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if is_skip_dir(name.as_str()) {
                continue;
            }
            let p = entry.path();
            if let Ok(m) = entry.metadata() {
                if m.is_file() {
                    results.push(p.clone());
                } else if m.is_dir() {
                    results.extend(walk_dir_limited(&p, max_depth - 1));
                }
            }
        }
    }
    results
}

// --- tests -------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn make_dir(base: &Path, name: &str) -> PathBuf {
        let path = base.join(name);
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn make_file(path: &Path) -> PathBuf {
        fs::write(path, "test").unwrap();
        path.to_path_buf()
    }

    #[test]
    fn test_normalize_name() {
        assert_eq!(normalize_name("Front-End"), "front_end");
        assert_eq!(normalize_name("my docs"), "my_docs");
        assert_eq!(normalize_name("SRC"), "src");
    }

    #[test]
    fn test_is_valid_room_name() {
        assert!(is_valid_room_name("src"));
        assert!(is_valid_room_name("frontend"));
        assert!(!is_valid_room_name("ab"));
        assert!(!is_valid_room_name("123"));
        assert!(!is_valid_room_name(".git"));
    }

    #[test]
    fn test_room_suggestion_builder() {
        let room = RoomSuggestion::new("frontend", "UI files", Some("frontend".into()))
            .with_keyword("ui")
            .with_keyword("ui");
        assert_eq!(room.name, "frontend");
        assert_eq!(room.description, "UI files");
        assert_eq!(room.source, Some("frontend".into()));
        assert!(room.keywords.contains(&"ui".into()));
        assert_eq!(room.keywords.iter().filter(|k| *k == "ui").count(), 1);
    }

    #[test]
    fn test_detect_rooms_from_paths_empty() -> Result<()> {
        let rooms = detect_rooms_from_paths(&[])?;
        assert_eq!(rooms.len(), 1);
        assert_eq!(rooms[0].name, "general");
        Ok(())
    }

    #[test]
    fn test_detect_rooms_from_paths_maps_keywords() -> Result<()> {
        let dir = tempdir()?;
        let base = dir.path();
        let paths = vec![
            make_dir(base, "src"),
            make_dir(base, "tests"),
            make_dir(base, "docs"),
            make_dir(base, "frontend"),
            make_dir(base, "backend"),
            make_dir(base, "scripts"),
        ];
        let rooms = detect_rooms_from_paths(&paths)?;
        let names: Vec<String> = rooms.iter().map(|r| r.name.clone()).collect();
        assert!(names.contains(&"source".into()));
        assert!(names.contains(&"testing".into()));
        assert!(names.contains(&"documentation".into()));
        assert!(names.contains(&"frontend".into()));
        assert!(names.contains(&"backend".into()));
        assert!(names.contains(&"scripts".into()));
        assert!(names.contains(&"general".into()));
        Ok(())
    }

    #[test]
    fn test_detect_rooms_from_paths_skips_non_dirs() -> Result<()> {
        let dir = tempdir()?;
        let base = dir.path();
        let file_path = base.join("readme.txt");
        fs::write(&file_path, "hello")?;
        let rooms = detect_rooms_from_paths(&[file_path])?;
        assert_eq!(rooms.len(), 1);
        assert_eq!(rooms[0].name, "general");
        Ok(())
    }

    #[test]
    fn test_detect_rooms_from_paths_accepts_valid_unknown_names() -> Result<()> {
        let dir = tempdir()?;
        let base = dir.path();
        let paths = vec![make_dir(base, "app"), make_dir(base, "lib")];
        let rooms = detect_rooms_from_paths(&paths)?;
        let names: Vec<String> = rooms.iter().map(|r| r.name.clone()).collect();
        assert!(names.contains(&"app".into()));
        assert!(names.contains(&"lib".into()));
        assert!(names.contains(&"general".into()));
        Ok(())
    }

    #[test]
    fn test_detect_rooms_from_folders_top_level() -> Result<()> {
        let dir = tempdir()?;
        let base = dir.path();
        make_dir(base, "src");
        make_dir(base, "tests");
        make_dir(base, "docs");
        let rooms = detect_rooms_from_folders(base)?;
        let names: Vec<String> = rooms.iter().map(|r| r.name.clone()).collect();
        assert!(names.contains(&"source".into()));
        assert!(names.contains(&"testing".into()));
        assert!(names.contains(&"documentation".into()));
        assert!(names.contains(&"general".into()));
        Ok(())
    }

    #[test]
    fn test_detect_rooms_from_folders_nested() -> Result<()> {
        let dir = tempdir()?;
        let base = dir.path();
        let app = make_dir(base, "app");
        make_dir(&app, "components");
        make_dir(&app, "services");
        let rooms = detect_rooms_from_folders(base)?;
        let names: Vec<String> = rooms.iter().map(|r| r.name.clone()).collect();
        assert!(names.contains(&"app".into()));
        assert!(names.contains(&"frontend".into()));
        assert!(names.contains(&"backend".into()));
        assert!(names.contains(&"general".into()));
        Ok(())
    }

    #[test]
    fn test_detect_rooms_from_folders_skips_ignored_dirs() -> Result<()> {
        let dir = tempdir()?;
        let base = dir.path();
        make_dir(base, "src");
        make_dir(base, ".git");
        make_dir(base, "node_modules");
        let rooms = detect_rooms_from_folders(base)?;
        let names: Vec<String> = rooms.iter().map(|r| r.name.clone()).collect();
        assert!(names.contains(&"source".into()));
        assert!(!names.contains(&".git".into()));
        assert!(!names.contains(&"node_modules".into()));
        assert!(names.contains(&"general".into()));
        Ok(())
    }

    #[test]
    fn test_detect_rooms_from_folders_missing_dir() {
        let dir = tempdir().unwrap();
        let missing = dir.path().join("missing");
        assert!(detect_rooms_from_folders(&missing).is_err());
    }

    #[test]
    fn test_detect_rooms_from_files_fallback() -> Result<()> {
        let dir = tempdir()?;
        let base = dir.path();
        make_dir(base, "flat");
        let files = [
            "test_auth.py",
            "test_api.py",
            "test_db.py",
            "readme.md",
            "config.json",
        ];
        for f in files {
            make_file(&base.join(f));
        }
        let rooms = detect_rooms_from_files(base)?;
        let names: Vec<String> = rooms.iter().map(|r| r.name.clone()).collect();
        // "test" maps to "testing" and appears three times.
        assert!(names.contains(&"testing".into()));
        Ok(())
    }

    #[test]
    fn test_detect_rooms_from_files_general_when_no_signal() -> Result<()> {
        let dir = tempdir()?;
        let base = dir.path();
        make_file(&base.join("a.txt"));
        make_file(&base.join("b.txt"));
        let rooms = detect_rooms_from_files(base)?;
        assert_eq!(rooms.len(), 1);
        assert_eq!(rooms[0].name, "general");
        Ok(())
    }

    #[test]
    fn test_detect_rooms_from_dir_prefers_config_yaml() -> Result<()> {
        let dir = tempdir()?;
        let base = dir.path();
        fs::write(
            base.join("mempalace.yaml"),
            r#"
wing: my_project
rooms:
  - name: custom_room
    description: A custom room
    keywords: [custom]
"#,
        )?;
        // Also create a folder that would normally map to "source".
        make_dir(base, "src");
        let rooms = detect_rooms_from_dir(base)?;
        let names: Vec<String> = rooms.iter().map(|r| r.name.clone()).collect();
        assert!(names.contains(&"custom_room".into()));
        assert!(!names.contains(&"source".into()));
        assert!(names.contains(&"general".into()));
        Ok(())
    }

    #[test]
    fn test_detect_rooms_from_dir_prefers_config_json() -> Result<()> {
        let dir = tempdir()?;
        let base = dir.path();
        fs::write(
            base.join("mempalace.json"),
            r#"{"wing": "json_project", "rooms": [{"name": "json_room", "description": "from json", "keywords": ["json"]}]}"#,
        )?;
        let rooms = detect_rooms_from_dir(base)?;
        let names: Vec<String> = rooms.iter().map(|r| r.name.clone()).collect();
        assert!(names.contains(&"json_room".into()));
        assert!(names.contains(&"general".into()));
        Ok(())
    }

    #[test]
    fn test_detect_rooms_from_dir_auto_detect() -> Result<()> {
        let dir = tempdir()?;
        let base = dir.path();
        make_dir(base, "src");
        make_dir(base, "tests");
        let rooms = detect_rooms_from_dir(base)?;
        let names: Vec<String> = rooms.iter().map(|r| r.name.clone()).collect();
        assert!(names.contains(&"source".into()));
        assert!(names.contains(&"testing".into()));
        assert!(names.contains(&"general".into()));
        Ok(())
    }

    #[test]
    fn test_detect_rooms_local_uses_fallback() -> Result<()> {
        let dir = tempdir()?;
        let base = dir.path();
        // No signal folders, only generic files.
        make_file(&base.join("a.txt"));
        make_file(&base.join("b.txt"));
        let (rooms, source) = detect_rooms_local(base)?;
        assert_eq!(rooms.len(), 1);
        assert_eq!(rooms[0].name, "general");
        assert_eq!(source, "fallback (flat project)");
        Ok(())
    }

    #[test]
    fn test_detect_rooms_local_filename_fallback() -> Result<()> {
        let dir = tempdir()?;
        let base = dir.path();
        // No signal folders, so folder detection yields only general.
        make_file(&base.join("test_a.py"));
        make_file(&base.join("test_b.py"));
        make_file(&base.join("test_c.py"));
        let (rooms, source) = detect_rooms_local(base)?;
        let names: Vec<String> = rooms.iter().map(|r| r.name.clone()).collect();
        assert!(names.contains(&"testing".into()));
        assert!(names.contains(&"general".into()));
        assert_eq!(source, "filename patterns");
        Ok(())
    }

    #[test]
    fn test_detect_rooms_local_config_source() -> Result<()> {
        let dir = tempdir()?;
        let base = dir.path();
        fs::write(
            base.join("mempalace.yaml"),
            "rooms:\n  - name: configured\n    description: configured room\n    keywords: [configured]\n",
        )?;
        let (rooms, source) = detect_rooms_local(base)?;
        let names: Vec<String> = rooms.iter().map(|r| r.name.clone()).collect();
        assert!(names.contains(&"configured".into()));
        assert!(names.contains(&"general".into()));
        assert_eq!(source, "explicit config");
        Ok(())
    }

    #[test]
    fn test_load_room_config_returns_none() -> Result<()> {
        let dir = tempdir()?;
        let base = dir.path();
        assert!(load_room_config(base)?.is_none());
        Ok(())
    }

    #[test]
    fn test_load_room_config_invalid_yaml_is_err() {
        let dir = tempdir().unwrap();
        let base = dir.path();
        fs::write(base.join("mempalace.yaml"), "{not valid yaml").unwrap();
        assert!(load_room_config(base).is_err());
    }

    #[test]
    fn test_save_and_load_config() -> Result<()> {
        let dir = tempdir()?;
        let base = dir.path();
        let rooms = vec![
            RoomSuggestion::new("frontend", "UI files", Some("frontend".into())),
            RoomSuggestion::new("backend", "Server files", Some("backend".into())),
        ];
        let path = save_config(base, Some("my_project"), &rooms)?;
        assert!(path.exists());
        let loaded = load_room_config(base)?.expect("config should load");
        assert_eq!(loaded.wing.as_deref(), Some("my_project"));
        assert_eq!(loaded.rooms.len(), 2);
        assert_eq!(loaded.rooms[0].name, "frontend");
        Ok(())
    }

    #[test]
    fn test_ensure_general_already_present() {
        let rooms = vec![RoomSuggestion::new("general", "General", None)];
        let out = ensure_general(rooms);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].name, "general");
    }

    #[test]
    fn test_build_rooms_description() {
        let mut found = HashMap::new();
        found.insert("source".into(), "src".into());
        let rooms = build_rooms(found, "Files from {}/");
        assert_eq!(rooms.len(), 2);
        let source = rooms.iter().find(|r| r.name == "source").unwrap();
        assert_eq!(source.description, "Files from src/");
        assert_eq!(source.source, Some("src".into()));
    }

    #[test]
    fn test_print_proposed_structure_does_not_panic() {
        let rooms = vec![
            RoomSuggestion::new("frontend", "Frontend", None),
            RoomSuggestion::new("general", "General", None),
        ];
        print_proposed_structure("test_wing", &rooms, 42, "folder structure");
    }

    #[test]
    fn test_count_files() -> Result<()> {
        let dir = tempdir()?;
        let base = dir.path();
        make_dir(base, "src");
        make_file(&base.join("root.txt"));
        make_file(&base.join("src").join("a.rs"));
        make_file(&base.join("src").join("b.rs"));
        make_dir(base, ".git");
        make_file(&base.join(".git").join("ignore"));
        assert_eq!(count_files(base), 3);
        Ok(())
    }

    #[test]
    fn test_count_files_missing_dir() {
        assert_eq!(count_files(&PathBuf::from("/this/does/not/exist")), 0);
    }
}
