use crate::config::MempalaceConfig;
use anyhow::{Context, Result};
use regex::Regex;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

lazy_static::lazy_static! {
    static ref TS_PATTERN: Regex = Regex::new(r"⏺\s+(\d{1,2}:\d{2}\s+[AP]M)\s+\w+,\s+(\w+)\s+(\d{1,2}),\s+(\d{4})").unwrap();
    static ref SKIP_PATTERNS: Regex = Regex::new(r"(?i)^(\./|cd |ls |python|bash|git |cat |source |export |claude|\./activate)").unwrap();
    static ref NON_WORD_REGEX: Regex = Regex::new(r"[^\w\.\-]").unwrap();
    static ref UNDERSCORE_REGEX: Regex = Regex::new(r"_+").unwrap();
}

const FALLBACK_KNOWN_PEOPLE: &[&str] = &["Alice", "Ben", "Riley", "Max", "Sam", "Devon", "Jordan"];

/// True session start: 'Claude Code v' header NOT followed by 'Ctrl+E'/'previous messages'
/// within the next 6 lines (those are context restores, not new sessions).
pub fn is_true_session_start(lines: &[String], idx: usize) -> bool {
    let end = (idx + 6).min(lines.len());
    let nearby = lines[idx..end].join("");
    !nearby.contains("Ctrl+E") && !nearby.contains("previous messages")
}

pub fn find_session_boundaries(lines: &[String]) -> Vec<usize> {
    let mut boundaries = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        if line.contains("Claude Code v") && is_true_session_start(lines, i) {
            boundaries.push(i);
        }
    }
    boundaries
}

pub fn extract_timestamp(lines: &[String]) -> Option<(String, String)> {
    let months: HashMap<&str, &str> = [
        ("January", "01"),
        ("February", "02"),
        ("March", "03"),
        ("April", "04"),
        ("May", "05"),
        ("June", "06"),
        ("July", "07"),
        ("August", "08"),
        ("September", "09"),
        ("October", "10"),
        ("November", "11"),
        ("December", "12"),
    ]
    .iter()
    .cloned()
    .collect();

    for line in lines.iter().take(50) {
        if let Some(caps) = TS_PATTERN.captures(line) {
            let time_str = &caps[1];
            let month = &caps[2];
            let day = &caps[3];
            let year = &caps[4];

            let mon = months.get(month).unwrap_or(&"00");
            let day_z = format!("{:0>2}", day);
            let time_safe = time_str.replace([':', ' '], "");
            let iso = format!("{}-{}-{}", year, mon, day_z);
            let human = format!("{}-{}-{}_{}", year, mon, day_z, time_safe);
            return Some((human, iso));
        }
    }
    None
}

pub fn extract_people(lines: &[String], config: &MempalaceConfig) -> Vec<String> {
    let mut found = HashSet::new();
    let text = lines
        .iter()
        .take(100)
        .cloned()
        .collect::<Vec<_>>()
        .join("\n");

    // People we know about
    for person in FALLBACK_KNOWN_PEOPLE {
        let re = Regex::new(&format!(r"(?i)\b{}\b", person)).unwrap();
        if re.is_match(&text) {
            found.insert(person.to_string());
        }
    }

    // Config names
    for name in config.people_map.values() {
        let re = Regex::new(&format!(r"(?i)\b{}\b", name)).unwrap();
        if re.is_match(&text) {
            found.insert(name.clone());
        }
    }

    // Working directory username hint
    let dir_re = Regex::new(r"/Users/(\w+)/").unwrap();
    if let Some(caps) = dir_re.captures(&text) {
        let username = &caps[1];
        if let Some(name) = config.people_map.get(username) {
            found.insert(name.clone());
        }
    }

    let mut result: Vec<String> = found.into_iter().collect();
    result.sort();
    result
}

pub fn extract_subject(lines: &[String]) -> String {
    for line in lines {
        if let Some(stripped) = line.strip_prefix("> ") {
            let prompt = stripped.trim();
            if !prompt.is_empty() && !SKIP_PATTERNS.is_match(prompt) && prompt.len() > 5 {
                let subject = prompt
                    .chars()
                    .map(|c| {
                        if c.is_alphanumeric() || c == '-' {
                            c
                        } else {
                            ' '
                        }
                    })
                    .collect::<String>();
                let subject = subject.split_whitespace().collect::<Vec<_>>().join("-");
                return subject.chars().take(60).collect();
            }
        }
    }
    "session".to_string()
}

pub fn split_mega_file(path: &Path, output_dir: &Path) -> Result<()> {
    let content = fs::read_to_string(path).context("Failed to read mega-file")?;
    let lines: Vec<String> = content.lines().map(|s| s.to_string() + "\n").collect();

    let mut boundaries = find_session_boundaries(&lines);
    if boundaries.len() < 2 {
        return Ok(()); // Not a mega-file
    }

    boundaries.push(lines.len());

    let config = MempalaceConfig::default();
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("mega_file");
    let stem_clean = Regex::new(r"[^\w-]")
        .unwrap()
        .replace_all(stem, "_")
        .chars()
        .take(40)
        .collect::<String>();

    for i in 0..boundaries.len() - 1 {
        let start = boundaries[i];
        let end = boundaries[i + 1];
        let chunk = &lines[start..end];

        if chunk.len() < 10 {
            continue;
        }

        let (ts_human, _ts_iso) = extract_timestamp(chunk)
            .unwrap_or_else(|| (format!("part{:02}", i + 1), String::new()));
        let people = extract_people(chunk, &config);
        let subject = extract_subject(chunk);

        let people_part = if people.is_empty() {
            "unknown".to_string()
        } else {
            people.iter().take(3).cloned().collect::<Vec<_>>().join("-")
        };

        let mut name = format!(
            "{}__{}_{}_{}.txt",
            stem_clean, ts_human, people_part, subject
        );
        name = NON_WORD_REGEX.replace_all(&name, "_").into_owned();
        name = UNDERSCORE_REGEX.replace_all(&name, "_").into_owned();

        let out_path = output_dir.join(name);
        fs::write(&out_path, chunk.join("")).context("Failed to write split file")?;
    }

    // Rename original to .mega_backup
    let mut backup_path = path.to_path_buf();
    backup_path.set_extension("mega_backup");
    fs::rename(path, backup_path).context("Failed to rename original file to backup")?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_true_session_start() {
        let lines = vec![
            "Claude Code v1.0\n".to_string(),
            "Welcome!\n".to_string(),
            "> Hello\n".to_string(),
        ];
        assert!(is_true_session_start(&lines, 0));

        let lines_restore = vec![
            "Claude Code v1.0\n".to_string(),
            "Ctrl+E to show previous messages\n".to_string(),
        ];
        assert!(!is_true_session_start(&lines_restore, 0));
    }

    #[test]
    fn test_extract_timestamp() {
        let lines = vec![
            "Claude Code v1.0\n".to_string(),
            "⏺ 10:30 AM Monday, March 30, 2026\n".to_string(),
        ];
        let res = extract_timestamp(&lines);
        assert!(res.is_some());
        let (human, iso) = res.unwrap();
        assert_eq!(human, "2026-03-30_1030AM");
        assert_eq!(iso, "2026-03-30");
    }

    #[test]
    fn test_extract_people() {
        let lines = vec!["Hello Ben and Alice\n".to_string()];
        let mut config = MempalaceConfig::default();
        config
            .people_map
            .insert("jdoe".to_string(), "John".to_string());

        let people = extract_people(&lines, &config);
        assert!(people.contains(&"Ben".to_string()));
        assert!(people.contains(&"Alice".to_string()));

        let lines_user = vec!["/Users/jdoe/project\n".to_string()];
        let people_user = extract_people(&lines_user, &config);
        assert!(people_user.contains(&"John".to_string()));
    }

    #[test]
    fn test_extract_subject() {
        let lines = vec![
            "> ls -la\n".to_string(),
            "> Please fix the bug in split_mega_files.rs\n".to_string(),
        ];
        let subject = extract_subject(&lines);
        assert_eq!(subject, "Please-fix-the-bug-in-split-mega-files-rs");
    }

    #[test]
    fn test_extract_timestamp_unknown_month() {
        let lines = vec!["⏺ 10:30 AM Monday, UnknownMonth 30, 2026\n".to_string()];
        let res = extract_timestamp(&lines);
        assert!(res.is_some());
        let (_human, iso) = res.unwrap();
        assert_eq!(iso, "2026-00-30"); // Fallback to 00
    }

    #[test]
    fn test_extract_subject_edge_cases() {
        let lines_short = vec!["> abc\n".to_string()];
        assert_eq!(extract_subject(&lines_short), "session");

        let lines_skip = vec![
            "> cd /tmp\n".to_string(),
            "> valid subject text\n".to_string(),
        ];
        assert_eq!(extract_subject(&lines_skip), "valid-subject-text");

        let lines_special = vec!["> Hello! @World #2026\n".to_string()];
        assert_eq!(extract_subject(&lines_special), "Hello-World-2026");
    }

    #[test]
    fn test_extract_people_username_mapping() {
        let mut config = MempalaceConfig::default();
        config
            .people_map
            .insert("jdoe".to_string(), "John".to_string());

        let lines_known = vec!["/Users/jdoe/project\n".to_string()];
        let people = extract_people(&lines_known, &config);
        assert!(people.contains(&"John".to_string()));

        let lines_unknown = vec!["/Users/unknown_user/project\n".to_string()];
        let people_unknown = extract_people(&lines_unknown, &config);
        assert!(people_unknown.is_empty());
    }

    #[test]
    fn test_split_mega_file_not_mega() {
        use tempfile::tempdir;
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("small.txt");
        fs::write(&file_path, "Claude Code v1.0\nJust one session.").unwrap();

        let res = split_mega_file(&file_path, dir.path());
        assert!(res.is_ok());
        // Should not have split anything or renamed
        assert!(file_path.exists());
    }

    #[test]
    fn test_split_mega_file_small_chunk() {
        use tempfile::tempdir;
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("mega.txt");
        let content = "Claude Code v1.0\nSession 1\n\nClaude Code v1.0\nTiny\n";
        fs::write(&file_path, content).unwrap();

        let res = split_mega_file(&file_path, dir.path());
        assert!(res.is_ok());
        // Should only have one file (Session 1), Tiny is too small (< 10 lines)
        // Actually, my lines count includes \n, so "Session 1" is also small here.
        // Let's make Session 1 larger.
        let large_content =
            "Claude Code v1.0\n".to_string() + &"line\n".repeat(15) + "\nClaude Code v1.0\nTiny\n";
        fs::write(&file_path, large_content).unwrap();
        split_mega_file(&file_path, dir.path()).unwrap();

        let files: Vec<_> = fs::read_dir(dir.path()).unwrap().collect();
        // 1 backup + 1 split file = 2
        assert_eq!(files.len(), 2);
    }
}
