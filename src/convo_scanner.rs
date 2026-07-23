//! Scan conversation files (chat logs, dialogue transcripts, etc.) for
//! entities and memories.
//!
//! This is the Rust forward-port of the Python `convo_scanner` module. It uses
//! only regex and heuristic parsing — no LLM API calls. It supports both
//! generic transcript files (`.txt`, `.md`, `.json`, `.jsonl`) and Claude Code
//! project directories (`~/.claude/projects/<slug>/<id>.jsonl`).
//!
//! Public API:
//! - `ConvoScanner` — configurable scanner instance
//! - `scan_convo_dir` — convenience function to scan a directory
//! - `scan_convo` — convenience function to scan a single conversation string
//! - `is_conversation_file` — check whether a path is a recognized convo file

use crate::models::{DetectedEntity, EntityType, MemoryType};
use crate::shared::is_skip_dir;
use anyhow::{Context, Result};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// Maximum number of header lines to read from a JSONL file looking for `cwd`.
const MAX_HEADER_LINES: usize = 20;

/// Recognized conversation file extensions.
pub const CONVO_EXTENSIONS: &[&str] = &[".txt", ".md", ".json", ".jsonl"];

/// A memory snippet extracted from a conversation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConvoMemory {
    pub content: String,
    pub memory_type: MemoryType,
    pub confidence: f32,
    pub source: Option<String>,
}

/// A single conversation turn (speaker + text).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConvoTurn {
    pub speaker: Option<String>,
    pub text: String,
    pub timestamp: Option<String>,
}

/// Result of scanning a conversation.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ConvoScanResult {
    pub entities: Vec<DetectedEntity>,
    pub memories: Vec<ConvoMemory>,
    pub turns: Vec<ConvoTurn>,
    pub project_name: Option<String>,
    pub source_file: Option<String>,
}

/// Configuration for `ConvoScanner`.
#[derive(Debug, Clone)]
pub struct ConvoScanner {
    #[allow(dead_code)]
    project_keywords: HashSet<String>,
    #[allow(dead_code)]
    person_hints: HashSet<String>,
    memory_keywords: HashMap<MemoryType, Vec<String>>,
}

impl Default for ConvoScanner {
    fn default() -> Self {
        Self::new()
    }
}

impl ConvoScanner {
    /// Create a scanner with the default heuristic keyword sets.
    pub fn new() -> Self {
        let project_keywords: HashSet<String> = [
            "project",
            "repo",
            "repository",
            "app",
            "application",
            "service",
            "library",
            "module",
            "package",
            "tool",
            "framework",
            "platform",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();

        let person_hints: HashSet<String> = [
            "said",
            "says",
            "asked",
            "told",
            "called",
            "emailed",
            "met with",
            "spoke to",
            "talked to",
            "with me",
            "my friend",
            "my colleague",
            "my manager",
            "my teammate",
            "he said",
            "she said",
            "they said",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();

        let mut memory_keywords: HashMap<MemoryType, Vec<String>> = HashMap::new();
        memory_keywords.insert(
            MemoryType::Decision,
            vec![
                "decided".to_string(),
                "decision".to_string(),
                "chose".to_string(),
                "picked".to_string(),
                "agreed".to_string(),
                "settled on".to_string(),
                "going with".to_string(),
                "we will".to_string(),
                "let's go with".to_string(),
            ],
        );
        memory_keywords.insert(
            MemoryType::Preference,
            vec![
                "prefer".to_string(),
                "like".to_string(),
                "dislike".to_string(),
                "favorite".to_string(),
                "want".to_string(),
                "don't want".to_string(),
                "hate".to_string(),
                "love".to_string(),
                "would rather".to_string(),
            ],
        );
        memory_keywords.insert(
            MemoryType::Milestone,
            vec![
                "launched".to_string(),
                "released".to_string(),
                "shipped".to_string(),
                "published".to_string(),
                "deployed".to_string(),
                "merged".to_string(),
                "completed".to_string(),
                "finished".to_string(),
                "milestone".to_string(),
            ],
        );
        memory_keywords.insert(
            MemoryType::Problem,
            vec![
                "problem".to_string(),
                "issue".to_string(),
                "bug".to_string(),
                "error".to_string(),
                "failed".to_string(),
                "broken".to_string(),
                "crash".to_string(),
                "stuck".to_string(),
                "workaround".to_string(),
                "resolved".to_string(),
                "fixed".to_string(),
            ],
        );
        memory_keywords.insert(
            MemoryType::Emotional,
            vec![
                "excited".to_string(),
                "worried".to_string(),
                "frustrated".to_string(),
                "happy".to_string(),
                "sad".to_string(),
                "angry".to_string(),
                "disappointed".to_string(),
                "proud".to_string(),
                "grateful".to_string(),
                "anxious".to_string(),
                "stressed".to_string(),
            ],
        );

        Self {
            project_keywords,
            person_hints,
            memory_keywords,
        }
    }

    /// Scan a directory recursively for conversation files.
    ///
    /// Returns one `ConvoScanResult` per file. Empty results are omitted unless
    /// `keep_empty` is true.
    pub fn scan_dir(&self, path: &Path, keep_empty: bool) -> Result<Vec<ConvoScanResult>> {
        let root = path
            .canonicalize()
            .with_context(|| format!("Failed to canonicalize {}", path.display()))?;

        // Fast path for Claude Code projects root.
        if is_claude_projects_root(&root) {
            return self.scan_claude_projects(&root);
        }

        let mut results = Vec::new();
        for entry in walkdir::WalkDir::new(&root)
            .follow_links(false)
            .into_iter()
            .filter_entry(|e| {
                let name = e.file_name().to_string_lossy();
                !is_skip_dir(&name)
            })
            .flatten()
        {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            if !is_conversation_file(path) {
                continue;
            }
            let result = self.scan_file(path)?;
            if keep_empty || !result.is_empty() {
                results.push(result);
            }
        }
        Ok(results)
    }

    /// Scan a single file path.
    pub fn scan_file(&self, path: &Path) -> Result<ConvoScanResult> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        let mut result = self.scan_text(&content);
        result.source_file = Some(path.to_string_lossy().to_string());
        Ok(result)
    }

    /// Scan a raw conversation string.
    pub fn scan_text(&self, content: &str) -> ConvoScanResult {
        let mut result = ConvoScanResult::default();
        let cleaned = normalize_content(content);
        result.turns = self.parse_turns(&cleaned);
        result.entities = self.extract_entities(&cleaned, &result.turns);
        result.memories = self.extract_memories(&cleaned, &result.turns);
        result
    }

    /// Parse individual conversation turns from a transcript.
    fn parse_turns(&self, content: &str) -> Vec<ConvoTurn> {
        let mut turns = Vec::new();
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }

            // Markdown quote style: "> Speaker: text"
            if let Some(turn) = self.parse_markdown_quote(line) {
                turns.push(turn);
                continue;
            }

            // Common chat patterns: "Speaker: text", "[HH:MM] Speaker: text", etc.
            if let Some(turn) = self.parse_chat_line(line) {
                turns.push(turn);
                continue;
            }

            // JSON line: {"role": "user", "content": "..."}
            if let Some(turn) = self.parse_json_turn(line) {
                turns.push(turn);
                continue;
            }
        }

        // If we couldn't parse any structured turns, treat the whole text as a
        // single anonymous turn so downstream extraction still works.
        if turns.is_empty() && !content.trim().is_empty() {
            turns.push(ConvoTurn {
                speaker: None,
                text: content.trim().to_string(),
                timestamp: None,
            });
        }

        turns
    }

    fn parse_markdown_quote(&self, line: &str) -> Option<ConvoTurn> {
        if !line.starts_with('>') {
            return None;
        }
        let inner = line.trim_start_matches('>').trim();
        // "> User: ..."
        if let Some(pos) = inner.find(':') {
            let speaker = inner[..pos].trim().to_string();
            let text = inner[pos + 1..].trim().to_string();
            if !text.is_empty() {
                return Some(ConvoTurn {
                    speaker: Some(speaker),
                    text,
                    timestamp: None,
                });
            }
        }
        Some(ConvoTurn {
            speaker: Some("User".to_string()),
            text: inner.to_string(),
            timestamp: None,
        })
    }

    fn parse_chat_line(&self, line: &str) -> Option<ConvoTurn> {
        // Timestamp prefixes: "[10:30] Alice: hello", "2024-01-01 Alice: hello"
        let re = Regex::new(
            r"^(?:(?:\[\d{1,2}:\d{2}(?::\d{2})?\]|\d{4}-\d{2}-\d{2}[\sT]\d{2}:\d{2}:\d{2})\s+)?([^:]+?):\s*(.+)$"
        ).ok()?;
        let caps = re.captures(line)?;
        let speaker = caps.get(1)?.as_str().trim().to_string();
        let text = caps.get(2)?.as_str().trim().to_string();
        if speaker.is_empty() || text.is_empty() {
            return None;
        }
        // Avoid matching URLs or sentinel lines.
        if speaker.contains("http") || speaker.len() > 60 {
            return None;
        }
        Some(ConvoTurn {
            speaker: Some(speaker),
            text,
            timestamp: None,
        })
    }

    fn parse_json_turn(&self, line: &str) -> Option<ConvoTurn> {
        let trimmed = line.trim();
        if !(trimmed.starts_with('{') && trimmed.ends_with('}')) {
            return None;
        }
        let v: serde_json::Value = serde_json::from_str(trimmed).ok()?;
        let role = v.get("role").and_then(|r| r.as_str());
        let content = v.get("content").and_then(|c| c.as_str());
        let text = content.or_else(|| v.get("text").and_then(|t| t.as_str()))?;
        Some(ConvoTurn {
            speaker: role.map(|s| s.to_string()),
            text: text.to_string(),
            timestamp: v
                .get("timestamp")
                .and_then(|t| t.as_str())
                .map(String::from),
        })
    }

    /// Extract entities (people, projects, terms) using regex and heuristics.
    fn extract_entities(&self, content: &str, turns: &[ConvoTurn]) -> Vec<DetectedEntity> {
        let mut entities: HashMap<String, DetectedEntity> = HashMap::new();
        let text_lower = content.to_lowercase();

        // Projects from explicit "the X project" / "project X" patterns take
        // precedence over person extraction for the same token.
        let projects = self.extract_projects(content);
        for (name, signal) in projects {
            entities.insert(
                name.to_lowercase(),
                DetectedEntity {
                    name,
                    unique_id: None,
                    r#type: EntityType::Project,
                    confidence: 0.75,
                    signals: vec![signal],
                    aliases: vec![],
                    relationship: None,
                },
            );
        }

        // People from capitalized names and explicit mentions.
        let names = self.extract_person_names(content);
        for name in names {
            if entities.contains_key(&name.to_lowercase()) {
                continue;
            }
            let hints = self.person_hint_count(&name, &text_lower, turns);
            let confidence = if hints > 0 { 0.85 } else { 0.65 };
            let signal = format!(
                "mentioned {} time{} in conversation",
                hints,
                if hints == 1 { "" } else { "s" }
            );
            entities.insert(
                name.to_lowercase(),
                DetectedEntity {
                    name: name.clone(),
                    unique_id: None,
                    r#type: EntityType::Person,
                    confidence,
                    signals: vec![signal],
                    aliases: vec![],
                    relationship: None,
                },
            );
        }

        // Terms from backticks and quoted phrases.
        let terms = self.extract_terms(content);
        for (name, signal) in terms {
            if name.len() > 2 && !entities.contains_key(&name.to_lowercase()) {
                entities.insert(
                    name.to_lowercase(),
                    DetectedEntity {
                        name,
                        unique_id: None,
                        r#type: EntityType::Term,
                        confidence: 0.6,
                        signals: vec![signal],
                        aliases: vec![],
                        relationship: None,
                    },
                );
            }
        }

        entities.into_values().collect()
    }

    fn extract_person_names(&self, content: &str) -> Vec<String> {
        let mut names = HashSet::new();
        // Capitalized names: single ("Alice") or multi-word ("Alice Smith").
        let re = Regex::new(r"\b([A-Z][a-z]+(?:\s+[A-Z][a-z]+)*)\b").unwrap();
        for cap in re.captures_iter(content) {
            let name = cap[1].to_string();
            if !is_common_word(&name) {
                names.insert(name);
            }
        }
        // Explicit "Alice said" pattern.
        let re_said =
            Regex::new(r"\b([A-Z][a-zA-Z]+)\s+(?:said|says|asked|told|replied)\b").unwrap();
        for cap in re_said.captures_iter(content) {
            names.insert(cap[1].to_string());
        }
        names.into_iter().collect()
    }

    fn person_hint_count(&self, name: &str, text_lower: &str, turns: &[ConvoTurn]) -> usize {
        let name_lower = name.to_lowercase();
        let count = text_lower.matches(&name_lower).count();
        let mut turn_hints = 0;
        for turn in turns {
            if let Some(speaker) = &turn.speaker {
                if speaker.to_lowercase().contains(&name_lower) {
                    turn_hints += 1;
                }
            }
        }
        count + turn_hints
    }

    fn extract_projects(&self, content: &str) -> Vec<(String, String)> {
        let mut projects = Vec::new();
        let re = Regex::new(r"(?i)\b(?:the\s+)?([A-Z][A-Za-z0-9_-]+)\s+(?:project|repo|repository|app|application|service|library)\b").unwrap();
        for cap in re.captures_iter(content) {
            let name = cap[1].trim().to_string();
            if !name.is_empty() && !is_common_word(&name) {
                projects.push((name, "matched project name pattern".to_string()));
            }
        }
        let re2 =
            Regex::new(r"(?i)\b(?:project|repo|app|service)\s+([A-Z][A-Za-z0-9_-]+)\b").unwrap();
        for cap in re2.captures_iter(content) {
            let name = cap[1].trim().to_string();
            if !name.is_empty() && !is_common_word(&name) {
                projects.push((name, "matched project name pattern".to_string()));
            }
        }
        projects
    }

    fn extract_terms(&self, content: &str) -> Vec<(String, String)> {
        let mut terms = Vec::new();
        let re = Regex::new(r"`([^`\n]{2,40})`").unwrap();
        for cap in re.captures_iter(content) {
            terms.push((cap[1].to_string(), "code term".to_string()));
        }
        let re2 = Regex::new(r#""([^"\n]{3,40})""#).unwrap();
        for cap in re2.captures_iter(content) {
            terms.push((cap[1].to_string(), "quoted term".to_string()));
        }
        terms
    }

    /// Extract memory snippets (decisions, preferences, milestones, problems,
    /// emotional moments) from the conversation.
    fn extract_memories(&self, content: &str, turns: &[ConvoTurn]) -> Vec<ConvoMemory> {
        let mut memories = Vec::new();
        let text_lower = content.to_lowercase();

        for (ty, keywords) in &self.memory_keywords {
            for kw in keywords {
                for _m in text_lower.matches(kw) {
                    // Find the sentence containing the keyword.
                    let idx = text_lower.find(kw).unwrap_or(0);
                    let start = text_lower[..idx]
                        .rfind(['.', '\n'])
                        .map(|i| i + 1)
                        .unwrap_or(0);
                    let end = text_lower[idx + kw.len()..]
                        .find(['.', '\n'])
                        .map(|i| idx + kw.len() + i + 1)
                        .unwrap_or(text_lower.len());
                    let snippet = content[start..end].trim().to_string();
                    if !snippet.is_empty() {
                        let conf = memory_confidence(ty, &snippet, turns);
                        memories.push(ConvoMemory {
                            content: snippet,
                            memory_type: ty.clone(),
                            confidence: conf,
                            source: None,
                        });
                    }
                }
            }
        }

        // Deduplicate by content while preserving order.
        let mut seen = HashSet::new();
        memories.retain(|m| {
            let key = m.content.to_lowercase();
            if seen.contains(&key) {
                return false;
            }
            seen.insert(key);
            true
        });
        memories
    }

    // ------------------------------------------------------------------
    // Claude Code projects support
    // ------------------------------------------------------------------

    /// Scan a `.claude/projects/` root directory.
    fn scan_claude_projects(&self, root: &Path) -> Result<Vec<ConvoScanResult>> {
        let mut results = Vec::new();
        for entry in std::fs::read_dir(root)
            .with_context(|| format!("Failed to read dir {}", root.display()))?
        {
            let entry = entry?;
            let sub = entry.path();
            if !sub.is_dir()
                || !sub
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("")
                    .starts_with('-')
            {
                continue;
            }
            let sessions: Vec<PathBuf> = std::fs::read_dir(&sub)?
                .filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.is_file() && p.extension().and_then(|s| s.to_str()) == Some("jsonl"))
                .collect();
            if sessions.is_empty() {
                continue;
            }
            let name = resolve_project_name(&sub, &sessions);
            let mut result = ConvoScanResult {
                project_name: Some(name),
                source_file: Some(sub.to_string_lossy().to_string()),
                ..ConvoScanResult::default()
            };
            // Extract entities from the most recent session header.
            let newest = sessions.iter().max_by_key(|p| safe_mtime(p));
            if let Some(session) = newest {
                if let Some(cwd) = extract_cwd_from_session(session) {
                    let proj_name = Path::new(&cwd)
                        .file_name()
                        .and_then(|s| s.to_str())
                        .unwrap_or(&cwd)
                        .to_string();
                    result.entities.push(DetectedEntity {
                        name: proj_name,
                        unique_id: None,
                        r#type: EntityType::Project,
                        confidence: 0.85,
                        signals: vec![format!("Claude Code project from {}", cwd)],
                        aliases: vec![],
                        relationship: None,
                    });
                }
                if let Ok(content) = std::fs::read_to_string(session) {
                    let partial = self.scan_text(&content);
                    result.entities.extend(partial.entities);
                    result.memories.extend(partial.memories);
                    result.turns.extend(partial.turns);
                }
            }
            result.deduplicate_entities();
            results.push(result);
        }
        Ok(results)
    }
}

impl ConvoScanResult {
    /// Return true if the result contains no entities, memories, or turns.
    pub fn is_empty(&self) -> bool {
        self.entities.is_empty() && self.memories.is_empty() && self.turns.is_empty()
    }

    /// Merge another scan result into this one, deduplicating entities.
    pub fn merge(&mut self, other: ConvoScanResult) {
        self.entities.extend(other.entities);
        self.memories.extend(other.memories);
        self.turns.extend(other.turns);
        self.deduplicate_entities();
    }

    /// Deduplicate entities by normalized name, keeping the highest confidence.
    fn deduplicate_entities(&mut self) {
        let mut by_name: HashMap<String, DetectedEntity> = HashMap::new();
        for entity in self.entities.drain(..) {
            let key = entity.name.to_lowercase();
            let existing = by_name.entry(key).or_insert(entity.clone());
            if entity.confidence > existing.confidence {
                *existing = entity;
            }
        }
        self.entities = by_name.into_values().collect();
    }
}

/// Convenience function to scan a directory for conversation files.
pub fn scan_convo_dir(path: &Path) -> Result<Vec<ConvoScanResult>> {
    ConvoScanner::new().scan_dir(path, false)
}

/// Convenience function to scan a single conversation string.
pub fn scan_convo(content: &str) -> ConvoScanResult {
    ConvoScanner::new().scan_text(content)
}

/// Return true if the path points to a recognized conversation file.
pub fn is_conversation_file(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_lowercase();
    let ext_with_dot = format!(".{}", ext);
    CONVO_EXTENSIONS.contains(&ext_with_dot.as_str())
}

/// Return true if the path looks like a Claude Code `.claude/projects/` root.
pub fn is_claude_projects_root(path: &Path) -> bool {
    if !path.is_dir() {
        return false;
    }
    let children = match std::fs::read_dir(path) {
        Ok(c) => c,
        Err(_) => return false,
    };
    for entry in children.flatten() {
        let child = entry.path();
        if !child.is_dir() {
            continue;
        }
        let name = child.file_name().and_then(|s| s.to_str()).unwrap_or("");
        if !name.starts_with('-') {
            continue;
        }
        let has_jsonl = match std::fs::read_dir(&child) {
            Ok(entries) => entries.flatten().any(|e| {
                let p = e.path();
                p.is_file() && p.extension().and_then(|s| s.to_str()) == Some("jsonl")
            }),
            Err(_) => false,
        };
        if has_jsonl {
            return true;
        }
    }
    false
}

fn normalize_content(content: &str) -> String {
    content.replace("\r\n", "\n").replace("\u{feff}", "")
}

fn is_common_word(name: &str) -> bool {
    let common: HashSet<&str> = [
        "The", "A", "An", "Is", "Are", "Was", "Were", "Be", "Been", "Being", "Have", "Has", "Had",
        "Do", "Does", "Did", "Will", "Would", "Could", "Should", "May", "Might", "Must", "Shall",
        "Can", "Need", "Dare", "Ought", "Used", "To", "Of", "In", "For", "On", "With", "At", "By",
        "From", "As", "Into", "Through", "During", "Before", "After", "Above", "Below", "Between",
        "Among", "Under", "Again", "Further", "Then", "Once", "Here", "There", "When", "Where",
        "Why", "How", "All", "Each", "Every", "Both", "Few", "More", "Most", "Other", "Some",
        "Such", "No", "Not", "Only", "Own", "Same", "So", "Than", "Too", "Very", "Just", "Now",
        "Also", "Back", "Any", "Because", "Give", "Most", "New", "Our", "Out", "Over", "Then",
        "This", "That", "These", "Those", "We", "You", "They", "He", "She", "It", "I", "Me", "Him",
        "Her", "Us", "Them", "My", "Your", "His", "Its", "Their",
    ]
    .iter()
    .copied()
    .collect();
    let first = name.split_whitespace().next().unwrap_or(name);
    common.contains(first) || common.contains(name)
}

fn memory_confidence(ty: &MemoryType, snippet: &str, turns: &[ConvoTurn]) -> f32 {
    let base = match ty {
        MemoryType::Decision => 0.85,
        MemoryType::Preference => 0.75,
        MemoryType::Milestone => 0.80,
        MemoryType::Problem => 0.80,
        MemoryType::Emotional => 0.70,
    };
    let speaker_bonus = if turns.iter().any(|t| {
        t.speaker
            .as_ref()
            .map(|s| s.to_lowercase() == "user" || s.to_lowercase() == "assistant")
            .unwrap_or(false)
    }) {
        0.05
    } else {
        0.0
    };
    let length_bonus: f32 = if snippet.len() > 80 { 0.05 } else { 0.0 };
    (base + speaker_bonus + length_bonus).min(0.99_f32)
}

fn safe_mtime(path: &Path) -> u64 {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn extract_cwd_from_session(session_file: &Path) -> Option<String> {
    let file = std::fs::File::open(session_file).ok()?;
    use std::io::{BufRead, BufReader};
    let reader = BufReader::new(file);
    for (i, line) in reader.lines().enumerate() {
        if i >= MAX_HEADER_LINES {
            break;
        }
        let line = line.ok()?;
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let v: serde_json::Value = serde_json::from_str(line).ok()?;
        if let Some(cwd) = v.get("cwd").and_then(|c| c.as_str()) {
            if !cwd.is_empty() {
                return Some(cwd.to_string());
            }
        }
    }
    None
}

fn resolve_project_name(project_dir: &Path, sessions: &[PathBuf]) -> String {
    for session in sessions.iter().rev() {
        if let Some(cwd) = extract_cwd_from_session(session) {
            let name = Path::new(&cwd)
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or(&cwd)
                .to_string();
            if !name.is_empty() {
                return name;
            }
        }
    }
    decode_slug_fallback(
        project_dir
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or(""),
    )
}

fn decode_slug_fallback(slug: &str) -> String {
    let stripped = slug.trim_start_matches('-');
    let parts: Vec<&str> = stripped.split('-').filter(|p| !p.is_empty()).collect();
    parts.last().copied().unwrap_or(slug).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_scan_convo_empty() {
        let result = scan_convo("");
        assert!(result.is_empty());
    }

    #[test]
    fn test_scan_convo_entities_and_memories() {
        let content = r#"
User: Alice and I decided to use the Rust project for the backend.
Assistant: That's a great choice. Bob will help with the `api`.
User: I prefer PostgreSQL over MySQL.
User: The launch went well yesterday, I'm excited!
        "#;
        let result = scan_convo(content);
        assert!(!result.turns.is_empty());
        assert!(
            result
                .entities
                .iter()
                .any(|e| { e.name == "Alice" && e.r#type == EntityType::Person }),
            "expected Alice as person"
        );
        assert!(
            result
                .entities
                .iter()
                .any(|e| { e.name == "Rust" && e.r#type == EntityType::Project }),
            "expected Rust as project"
        );
        assert!(
            result.entities.iter().any(|e| e.r#type == EntityType::Term),
            "expected at least one term entity"
        );
        assert!(
            result
                .memories
                .iter()
                .any(|m| m.memory_type == MemoryType::Decision),
            "expected decision memory"
        );
        assert!(
            result
                .memories
                .iter()
                .any(|m| m.memory_type == MemoryType::Preference),
            "expected preference memory"
        );
        assert!(
            result
                .memories
                .iter()
                .any(|m| m.memory_type == MemoryType::Emotional),
            "expected emotional memory"
        );
    }

    #[test]
    fn test_parse_markdown_quote() {
        let scanner = ConvoScanner::new();
        let turn = scanner.parse_markdown_quote("> User: hello world").unwrap();
        assert_eq!(turn.speaker, Some("User".to_string()));
        assert_eq!(turn.text, "hello world");
    }

    #[test]
    fn test_parse_chat_line() {
        let scanner = ConvoScanner::new();
        let turn = scanner
            .parse_chat_line("[10:30] Alice: hello world")
            .unwrap();
        assert_eq!(turn.speaker, Some("Alice".to_string()));
        assert_eq!(turn.text, "hello world");
        let turn2 = scanner.parse_chat_line("Bob: hi there").unwrap();
        assert_eq!(turn2.speaker, Some("Bob".to_string()));
    }

    #[test]
    fn test_parse_json_turn() {
        let scanner = ConvoScanner::new();
        let line = r#"{"role":"user","content":"hello","timestamp":"2024-01-01T00:00:00Z"}"#;
        let turn = scanner.parse_json_turn(line).unwrap();
        assert_eq!(turn.speaker, Some("user".to_string()));
        assert_eq!(turn.text, "hello");
        assert_eq!(turn.timestamp, Some("2024-01-01T00:00:00Z".to_string()));
    }

    #[test]
    fn test_is_conversation_file() {
        let temp = tempfile::tempdir().unwrap();
        let md = temp.path().join("chat.md");
        std::fs::write(&md, "hello").unwrap();
        assert!(is_conversation_file(&md));
        let bin = temp.path().join("data.bin");
        std::fs::write(&bin, "hello").unwrap();
        assert!(!is_conversation_file(&bin));
    }

    #[test]
    fn test_is_claude_projects_root() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join(".claude").join("projects");
        let slug = root.join("-home-user-my-project");
        std::fs::create_dir_all(&slug).unwrap();
        let session = slug.join("session.jsonl");
        std::fs::write(&session, "{}\n").unwrap();
        assert!(is_claude_projects_root(&root));
    }

    #[test]
    fn test_decode_slug_fallback() {
        assert_eq!(decode_slug_fallback("-home-user-foo"), "foo");
        assert_eq!(decode_slug_fallback("foo"), "foo");
        assert_eq!(decode_slug_fallback("--"), "--");
    }

    #[test]
    fn test_extract_cwd_from_session() {
        let temp = tempfile::tempdir().unwrap();
        let session = temp.path().join("session.jsonl");
        let mut file = std::fs::File::create(&session).unwrap();
        writeln!(file, "{{\"cwd\":\"/home/user/my-project\"}}").unwrap();
        writeln!(file, "{{\"foo\":\"bar\"}}").unwrap();
        assert_eq!(
            extract_cwd_from_session(&session),
            Some("/home/user/my-project".to_string())
        );
    }

    #[test]
    fn test_resolve_project_name() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("-home-user-foo");
        std::fs::create_dir(&dir).unwrap();
        let session = dir.join("a.jsonl");
        std::fs::write(&session, "{\"cwd\":\"/home/user/bar\"}\n").unwrap();
        let name = resolve_project_name(&dir, &[session]);
        assert_eq!(name, "bar");
    }

    #[test]
    fn test_scan_dir_with_jsonl() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path();
        let file = dir.join("chat.md");
        std::fs::write(
            &file,
            "User: Alice said we should use the ApiService project.\nAssistant: Great idea.\n",
        )
        .unwrap();
        let results = scan_convo_dir(dir).unwrap();
        assert_eq!(results.len(), 1);
        let result = &results[0];
        assert!(
            result.entities.iter().any(|e| e.name == "Alice"),
            "expected Alice"
        );
        assert!(
            result.entities.iter().any(|e| e.name == "ApiService"),
            "expected ApiService"
        );
    }

    #[test]
    fn test_scan_dir_skips_non_convo_files() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path();
        std::fs::write(dir.join("code.rs"), "fn main() {}").unwrap();
        std::fs::write(dir.join("README.md"), "hello").unwrap();
        let results = scan_convo_dir(dir).unwrap();
        assert_eq!(results.len(), 1);
        assert!(results[0]
            .source_file
            .as_ref()
            .unwrap()
            .ends_with("README.md"));
    }

    #[test]
    fn test_scan_dir_skips_dirs() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path();
        let skip = dir.join(".git");
        std::fs::create_dir(&skip).unwrap();
        std::fs::write(skip.join("chat.md"), "User: hello").unwrap();
        let results = scan_convo_dir(dir).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_convo_scanner_merge() {
        let scanner = ConvoScanner::new();
        let mut a = scanner.scan_text("Alice said hi.");
        let b = scanner.scan_text("Bob said hello.");
        a.merge(b);
        assert!(a.entities.iter().any(|e| e.name == "Alice"));
        assert!(a.entities.iter().any(|e| e.name == "Bob"));
    }

    #[test]
    fn test_convo_scan_result_is_empty() {
        let mut r = ConvoScanResult::default();
        assert!(r.is_empty());
        r.turns.push(ConvoTurn {
            speaker: None,
            text: "x".to_string(),
            timestamp: None,
        });
        assert!(!r.is_empty());
    }

    #[test]
    fn test_scan_claude_projects() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join(".claude").join("projects");
        let slug = root.join("-home-user-awesome-project");
        std::fs::create_dir_all(&slug).unwrap();
        let session = slug.join("session.jsonl");
        std::fs::write(
            &session,
            "{\"cwd\":\"/home/user/awesome-project\"}\nUser: Alice said we should ship.\n",
        )
        .unwrap();
        let results = ConvoScanner::new().scan_dir(&root, false).unwrap();
        assert_eq!(results.len(), 1);
        let result = &results[0];
        assert_eq!(result.project_name.as_deref(), Some("awesome-project"));
        assert!(result.entities.iter().any(|e| e.name == "awesome-project"));
        assert!(result.entities.iter().any(|e| e.name == "Alice"));
    }

    #[test]
    fn test_scan_dir_nonexistent_is_error() {
        let path = Path::new("/this/should/not/exist/for/convo/scanner");
        let result = scan_convo_dir(path);
        assert!(result.is_err());
    }

    #[test]
    fn test_memory_confidence() {
        let turns = vec![ConvoTurn {
            speaker: Some("user".to_string()),
            text: "x".to_string(),
            timestamp: None,
        }];
        let c = memory_confidence(&MemoryType::Decision, "we decided to use rust", &turns);
        assert!(c > 0.85);
    }

    #[test]
    fn test_extract_terms() {
        let scanner = ConvoScanner::new();
        let terms = scanner.extract_terms("Use `rustc` and \"cargo build\".");
        let names: Vec<String> = terms.iter().map(|(n, _)| n.clone()).collect();
        assert!(names.contains(&"rustc".to_string()));
        assert!(names.contains(&"cargo build".to_string()));
    }

    #[test]
    fn test_common_words_filtered() {
        let scanner = ConvoScanner::new();
        let names = scanner.extract_person_names("The And Or Is Are Was");
        assert!(names.is_empty());
    }

    #[tokio::test]
    async fn test_convo_scanner_integration_in_miner() {
        // This test just verifies the module can be used alongside miner.
        use crate::miner;
        let _ = miner::chunk_text("hello world this is a long enough chunk for testing");
        let result = scan_convo("User: Alice said we prefer Rust.");
        assert!(result.entities.iter().any(|e| e.name == "Alice"));
    }
}
