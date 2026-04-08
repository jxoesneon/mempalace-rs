use crate::models::{DetectedEntity, EntityType};
use lazy_static::lazy_static;
use regex::Regex;
use std::collections::{HashMap, HashSet};

lazy_static! {
    static ref STOPWORDS: HashSet<&'static str> = {
        let mut s = HashSet::new();
        s.extend(vec![
            "the", "a", "an", "and", "or", "but", "in", "on", "at", "to", "for", "of", "with", "by", "from",
            "as", "is", "was", "are", "were", "be", "been", "being", "have", "has", "had", "do", "does", "did",
            "will", "would", "could", "should", "may", "might", "must", "shall", "can", "this", "that", "these",
            "those", "it", "its", "they", "them", "their", "we", "our", "you", "your", "i", "my", "me", "he",
            "she", "his", "her", "who", "what", "when", "where", "why", "how", "which", "if", "then", "so", "not",
            "no", "yes", "ok", "okay", "just", "very", "really", "also", "already", "still", "even", "only", "here",
            "there", "now", "too", "up", "out", "about", "like", "use", "get", "got", "make", "made", "take", "put",
            "come", "go", "see", "know", "think", "true", "false", "none", "null", "new", "old", "all", "any", "some",
            "return", "print", "def", "class", "import", "step", "usage", "run", "check", "find", "add", "set", "list",
            "args", "dict", "str", "int", "bool", "path", "file", "type", "name", "note", "example", "option", "result",
            "error", "warning", "info", "every", "each", "more", "less", "next", "last", "first", "second", "stack",
            "layer", "mode", "test", "stop", "start", "copy", "move", "source", "target", "output", "input", "data",
            "item", "key", "value", "returns", "raises", "yields", "self", "cls", "kwargs", "world", "well", "want",
            "topic", "choose", "social", "cars", "phones", "healthcare", "ex", "machina", "deus", "human", "humans",
            "people", "things", "something", "nothing", "everything", "anything", "someone", "everyone", "anyone",
            "way", "time", "day", "life", "place", "thing", "part", "kind", "sort", "case", "point", "idea", "fact",
            "sense", "question", "answer", "reason", "number", "version", "system", "hey", "hi", "hello", "thanks",
            "thank", "right", "let", "click", "hit", "press", "tap", "drag", "drop", "open", "close", "save", "load",
            "launch", "install", "download", "upload", "scroll", "select", "enter", "submit", "cancel", "confirm",
            "delete", "paste", "write", "read", "search", "show", "hide", "desktop", "documents", "downloads", "users",
            "home", "library", "applications", "preferences", "settings", "terminal", "actor", "vector", "remote",
            "control", "duration", "fetch", "agents", "tools", "others", "guards", "ethics", "regulation", "learning",
            "thinking", "memory", "language", "intelligence", "technology", "society", "culture", "future", "history",
            "science", "model", "models", "network", "networks", "training", "inference",
        ]);
        s
    };

    static ref PROPER_NOUN: Regex = Regex::new(r"\b([A-Z][a-zA-Z0-9]{1,19})\b").unwrap();
    static ref PROPER_PHRASE: Regex = Regex::new(r"\b([A-Z][a-z]+(?:\s+[A-Z][a-z]+)+)\b").unwrap();
    static ref CODE_FILE: Regex = Regex::new(r"\b([a-z0-9_-]+\.(?:rs|py|js|ts|sh))\b").unwrap();

    // Person signal patterns
    static ref DIALOGUE_PATTERNS: Vec<String> = vec![
        r"^>\s*{name}[:\s]".to_string(),
        r"^{name}:\s".to_string(),
        r"^\[{name}\]".to_string(),
        r#""{name}\s+said"#.to_string(),
    ];
    static ref PERSON_VERB_PATTERNS: Vec<String> = vec![
        r"\b{name}\s+said\b".to_string(),
        r"\b{name}\s+asked\b".to_string(),
        r"\b{name}\s+told\b".to_string(),
        r"\b{name}\s+replied\b".to_string(),
        r"\b{name}\s+laughed\b".to_string(),
        r"\b{name}\s+smiled\b".to_string(),
        r"\b{name}\s+cried\b".to_string(),
        r"\b{name}\s+felt\b".to_string(),
        r"\b{name}\s+thinks?\b".to_string(),
        r"\b{name}\s+wants?\b".to_string(),
        r"\b{name}\s+loves?\b".to_string(),
        r"\b{name}\s+hates?\b".to_string(),
        r"\b{name}\s+knows?\b".to_string(),
        r"\b{name}\s+decided\b".to_string(),
        r"\b{name}\s+pushed\b".to_string(),
        r"\b{name}\s+wrote\b".to_string(),
        r"\bhey\s+{name}\b".to_string(),
        r"\bthanks?\s+{name}\b".to_string(),
        r"\bhi\s+{name}\b".to_string(),
        r"\bdear\s+{name}\b".to_string(),
    ];
    static ref PRONOUN_PATTERNS: Vec<Regex> = vec![
        Regex::new(r"\bshe\b").unwrap(),
        Regex::new(r"\bher\b").unwrap(),
        Regex::new(r"\bhers\b").unwrap(),
        Regex::new(r"\bhe\b").unwrap(),
        Regex::new(r"\bhim\b").unwrap(),
        Regex::new(r"\bhis\b").unwrap(),
        Regex::new(r"\bthey\b").unwrap(),
        Regex::new(r"\bthem\b").unwrap(),
        Regex::new(r"\btheir\b").unwrap(),
    ];

    // Project signal patterns
    static ref PROJECT_VERB_PATTERNS: Vec<String> = vec![
        r"\bbuilding\s+{name}\b".to_string(),
        r"\bbuilt\s+{name}\b".to_string(),
        r"\bship(?:ping|ped)?\s+{name}\b".to_string(),
        r"\blaunch(?:ing|ed)?\s+{name}\b".to_string(),
        r"\bdeploy(?:ing|ed)?\s+{name}\b".to_string(),
        r"\binstall(?:ing|ed)?\s+{name}\b".to_string(),
        r"\bthe\s+{name}\s+architecture\b".to_string(),
        r"\bthe\s+{name}\s+pipeline\b".to_string(),
        r"\bthe\s+{name}\s+system\b".to_string(),
        r"\bthe\s+{name}\s+repo\b".to_string(),
        r"\b{name}\s+v\d+\b".to_string(),
        r"\b{name}\.py\b".to_string(),
        r"\b{name}-core\b".to_string(),
        r"\b{name}-local\b".to_string(),
        r"\bimport\s+{name}\b".to_string(),
        r"\bpip\s+install\s+{name}\b".to_string(),
    ];

    static ref CODE_BLOCK: Regex = Regex::new(r"(?s)```.*?```").unwrap();
    static ref INLINE_CODE: Regex = Regex::new(r"`.*?`").unwrap();
    static ref TERMINAL_CMD: Regex = Regex::new(r"(?m)^\$\s+.*").unwrap();
}

#[derive(Default, Debug)]
struct EntityScores {
    person_score: f32,
    project_score: f32,
    person_signals: Vec<String>,
    project_signals: Vec<String>,
}

pub fn extract_entities(text: &str) -> Vec<DetectedEntity> {
    // 1. Filter out code blocks and terminal commands
    let clean_text = filter_code_and_commands(text);
    let lines: Vec<&str> = clean_text.lines().collect();

    // 2. Extract candidates
    let candidates = extract_candidates(&clean_text);
    let mut results = Vec::new();

    // 3. Score and classify
    for (name, frequency) in candidates {
        let scores = score_entity(&name, &clean_text, &lines);
        let entity = classify_entity(&name, frequency, scores);
        results.push(entity);
    }

    // Sort by confidence descending
    results.sort_by(|a, b| {
        b.confidence
            .partial_cmp(&a.confidence)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    results
}

fn filter_code_and_commands(text: &str) -> String {
    let mut clean = CODE_BLOCK.replace_all(text, "").to_string();
    clean = INLINE_CODE.replace_all(&clean, "").to_string();
    clean = TERMINAL_CMD.replace_all(&clean, "").to_string();
    clean
}

fn extract_candidates(text: &str) -> HashMap<String, usize> {
    let mut counts = HashMap::new();

    // Single words
    for mat in PROPER_NOUN.captures_iter(text) {
        if let Some(m) = mat.get(1) {
            let word = m.as_str();
            if !STOPWORDS.contains(&word.to_lowercase().as_str()) && word.len() > 1 {
                *counts.entry(word.to_string()).or_insert(0) += 1;
            }
        }
    }

    // Multi-word phrases
    for mat in PROPER_PHRASE.captures_iter(text) {
        if let Some(m) = mat.get(1) {
            let full_phrase = m.as_str();
            let words: Vec<&str> = full_phrase.split_whitespace().collect();

            // Add all sub-phrases of length 2 to words.len()
            for len in 2..=words.len() {
                for start in 0..=(words.len() - len) {
                    let sub_phrase = words[start..start + len].join(" ");
                    let significant_words = words[start..start + len]
                        .iter()
                        .filter(|&&w| !STOPWORDS.contains(&w.to_lowercase().as_str()))
                        .count();
                    if significant_words >= 2 {
                        *counts.entry(sub_phrase).or_insert(0) += 1;
                    }
                }
            }
        }
    }

    // Code file references (can be lowercase)
    for mat in CODE_FILE.captures_iter(text) {
        if let Some(m) = mat.get(1) {
            let file_ref = m.as_str();
            // If it's something like "main.rs", we want to extract "main" or "main.rs"
            // The tests expect "main"
            if let Some(dot_pos) = file_ref.find('.') {
                let name = &file_ref[..dot_pos];
                if name.len() > 1 {
                    *counts.entry(name.to_string()).or_insert(0) += 1;
                }
            }
        }
    }

    // Filter by frequency >= 3
    counts
        .into_iter()
        .filter(|&(_, count)| count >= 3)
        .collect()
}

fn score_entity(name: &str, text: &str, lines: &[&str]) -> EntityScores {
    let mut scores = EntityScores::default();
    let n_escaped = regex::escape(name);

    // --- Person signals ---

    // Dialogue markers
    for p in DIALOGUE_PATTERNS.iter() {
        let rx = Regex::new(&p.replace("{name}", &n_escaped)).unwrap();
        let matches = rx.find_iter(text).count();
        if matches > 0 {
            scores.person_score += matches as f32 * 3.0;
            scores
                .person_signals
                .push(format!("dialogue marker ({}x)", matches));
        }
    }

    // Person verbs
    for p in PERSON_VERB_PATTERNS.iter() {
        let rx = Regex::new(&format!("(?i){}", p.replace("{name}", &n_escaped))).unwrap();
        let matches = rx.find_iter(text).count();
        if matches > 0 {
            scores.person_score += matches as f32 * 2.0;
            scores
                .person_signals
                .push(format!("person action ({}x)", matches));
        }
    }

    // Pronoun proximity
    let name_lower = name.to_lowercase();
    for (i, line) in lines.iter().enumerate() {
        if line.to_lowercase().contains(&name_lower) {
            let start = i.saturating_sub(2);
            let end = if i + 3 < lines.len() {
                i + 3
            } else {
                lines.len()
            };
            let window_text = lines[start..end].join(" ").to_lowercase();

            for rx in PRONOUN_PATTERNS.iter() {
                if rx.is_match(&window_text) {
                    scores.person_score += 2.0;
                    scores.person_signals.push("pronoun nearby".to_string());
                    break;
                }
            }
        }
    }

    // Direct address
    let direct_rx = Regex::new(&format!(
        r"(?i)\bhey\s+{n_escaped}\b|\bthanks?\s+{n_escaped}\b|\bhi\s+{n_escaped}\b"
    ))
    .unwrap();
    let direct_matches = direct_rx.find_iter(text).count();
    if direct_matches > 0 {
        scores.person_score += direct_matches as f32 * 4.0;
        scores
            .person_signals
            .push(format!("addressed directly ({}x)", direct_matches));
    }

    // --- Project signals ---

    for p in PROJECT_VERB_PATTERNS.iter() {
        let rx = Regex::new(&format!("(?i){}", p.replace("{name}", &n_escaped))).unwrap();
        let matches = rx.find_iter(text).count();
        if matches > 0 {
            scores.project_score += matches as f32 * 2.0;
            scores
                .project_signals
                .push(format!("project verb ({}x)", matches));
        }
    }

    let versioned_rx = Regex::new(&format!(r"(?i)\b{n_escaped}[-v]\w+")).unwrap();
    let v_matches = versioned_rx.find_iter(text).count();
    if v_matches > 0 {
        scores.project_score += v_matches as f32 * 3.0;
        scores
            .project_signals
            .push(format!("versioned/hyphenated ({}x)", v_matches));
    }

    let code_ref_rx = Regex::new(&format!(
        r"(?i)\b{n_escaped}\.(py|js|ts|yaml|yml|json|sh|rs)\b"
    ))
    .unwrap();
    let c_matches = code_ref_rx.find_iter(text).count();
    if c_matches > 0 {
        scores.project_score += c_matches as f32 * 3.0;
        scores
            .project_signals
            .push(format!("code file reference ({}x)", c_matches));
    }

    scores
}

fn classify_entity(name: &str, frequency: usize, scores: EntityScores) -> DetectedEntity {
    let ps = scores.person_score;
    let prs = scores.project_score;
    let total = ps + prs;

    if total == 0.0 {
        let confidence = (frequency as f32 / 50.0).min(0.4);
        return DetectedEntity {
            name: name.to_string(),
            r#type: EntityType::Term,
            confidence: (confidence * 100.0).round() / 100.0,
            signals: vec![format!("appears {}x, no strong type signals", frequency)],
            aliases: vec![],
            relationship: None,
        };
    }

    let person_ratio = ps / total;

    // Check for signal variety for person
    let mut signal_categories = HashSet::new();
    for s in &scores.person_signals {
        if s.contains("dialogue") {
            signal_categories.insert("dialogue");
        } else if s.contains("action") {
            signal_categories.insert("action");
        } else if s.contains("pronoun") {
            signal_categories.insert("pronoun");
        } else if s.contains("addressed") {
            signal_categories.insert("addressed");
        }
    }

    let has_two_signal_types = signal_categories.len() >= 2;

    if person_ratio >= 0.7 && has_two_signal_types && ps >= 5.0 {
        let confidence = 0.5 + person_ratio * 0.5;
        DetectedEntity {
            name: name.to_string(),
            r#type: EntityType::Person,
            confidence: (confidence.min(0.99) * 100.0).round() / 100.0,
            signals: scores.person_signals.into_iter().take(3).collect(),
            aliases: vec![],
            relationship: None,
        }
    } else if person_ratio >= 0.7 && (!has_two_signal_types || ps < 5.0) {
        DetectedEntity {
            name: name.to_string(),
            r#type: EntityType::Term,
            confidence: 0.4,
            signals: vec![format!("appears {}x — weak signals", frequency)],
            aliases: vec![],
            relationship: None,
        }
    } else if person_ratio <= 0.3 {
        let confidence = 0.5 + (1.0 - person_ratio) * 0.5;
        DetectedEntity {
            name: name.to_string(),
            r#type: EntityType::Project,
            confidence: (confidence.min(0.99) * 100.0).round() / 100.0,
            signals: scores.project_signals.into_iter().take(3).collect(),
            aliases: vec![],
            relationship: None,
        }
    } else {
        DetectedEntity {
            name: name.to_string(),
            r#type: EntityType::Term,
            confidence: 0.5,
            signals: vec!["mixed signals — needs review".to_string()],
            aliases: vec![],
            relationship: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_filter_code() {
        let text = "Hello.\n```\nfn main() {}\n```\n$ cargo run\nAnd `ls -la`.";
        let filtered = filter_code_and_commands(text);
        assert!(!filtered.contains("fn main"));
        assert!(!filtered.contains("cargo run"));
        assert!(!filtered.contains("ls -la"));
    }

    #[test]
    fn test_person_signals() {
        let text = "Alice said she was happy. Alice asked about Bob. Bob replied to Alice.\n[Alice] Hello world.\nAlice Alice Alice Alice Alice";
        let entities = extract_entities(text);
        let alice = entities
            .iter()
            .find(|e| e.name == "Alice")
            .expect("Alice not found");
        assert_eq!(alice.r#type, EntityType::Person);
        assert!(alice.confidence > 0.7);
    }

    #[test]
    fn test_project_signals() {
        let text = "We are building Mempalace. I just deployed Mempalace. Mempalace v2 is out. Mempalace.rs is great.\nMempalace Mempalace Mempalace Mempalace Mempalace";
        let entities = extract_entities(text);
        let mem = entities
            .iter()
            .find(|e| e.name == "Mempalace")
            .expect("Mempalace not found");
        assert_eq!(mem.r#type, EntityType::Project);
        assert!(mem.confidence > 0.7);
    }

    #[test]
    fn test_term_signals() {
        let text = "This is a simple Concept. Concept appears many times. Concept Concept Concept.";
        let entities = extract_entities(text);
        let concept = entities
            .iter()
            .find(|e| e.name == "Concept")
            .expect("Concept not found");
        assert_eq!(concept.r#type, EntityType::Term);
    }

    #[test]
    fn test_multi_word_phrase() {
        let text = "Building The Big Project. We built The Big Project. The Big Project The Big Project The Big Project.";
        let entities = extract_entities(text);
        let project = entities
            .iter()
            .find(|e| e.name == "The Big Project")
            .expect("Phrase not found");
        assert_eq!(project.r#type, EntityType::Project);
    }

    #[test]
    fn test_direct_address() {
        let text = "Hey Riley, how are you? Thanks Riley! Hi Riley. Riley Riley Riley Riley.";
        let entities = extract_entities(text);
        let riley = entities
            .iter()
            .find(|e| e.name == "Riley")
            .expect("Riley not found");
        assert_eq!(riley.r#type, EntityType::Person);
        assert!(riley.confidence > 0.8);
    }

    #[test]
    fn test_versioned_project() {
        let text = "Using MyLibrary-v1.0. MyLibrary-core is stable. MyLibrary-local MyLibrary MyLibrary MyLibrary.";
        let entities = extract_entities(text);
        let lib = entities
            .iter()
            .find(|e| e.name == "MyLibrary")
            .expect("Library not found");
        assert_eq!(lib.r#type, EntityType::Project);
    }

    #[test]
    fn test_code_file_reference() {
        let text = "Check main.rs for details. main.rs has the logic. main.rs main.rs main.rs.";
        let entities = extract_entities(text);
        let main = entities
            .iter()
            .find(|e| e.name == "main")
            .expect("main not found");
        assert_eq!(main.r#type, EntityType::Project);
    }

    #[test]
    fn test_weak_signals() {
        let text = "SomeWord exists. SomeWord is here. SomeWord SomeWord SomeWord.";
        let entities = extract_entities(text);
        let word = entities
            .iter()
            .find(|e| e.name == "SomeWord")
            .expect("SomeWord not found");
        assert_eq!(word.r#type, EntityType::Term);
        // Frequency is 5, confidence = (5/50).min(0.4) = 0.1
        assert_eq!(word.confidence, 0.1);
    }

    #[test]
    fn test_mixed_signals() {
        // High frequency but mixed signals
        let text = "ProjectX is great. Hey ProjectX, said Alice. ProjectX.py is here. ProjectX ProjectX ProjectX ProjectX ProjectX ProjectX ProjectX.";
        let entities = extract_entities(text);
        let px = entities
            .iter()
            .find(|e| e.name == "ProjectX")
            .expect("ProjectX not found");
        // Mixed signals often fall into Term with 0.5 confidence if ratio is middle
        assert_eq!(px.r#type, EntityType::Term);
        assert_eq!(px.confidence, 0.5);
    }

    #[test]
    fn test_frequency_filter() {
        let text = "OnlyTwice OnlyTwice.";
        let entities = extract_entities(text);
        assert!(!entities.iter().any(|e| e.name == "OnlyTwice"));

        let text3 = "ExactlyThree ExactlyThree ExactlyThree.";
        let entities3 = extract_entities(text3);
        assert!(entities3.iter().any(|e| e.name == "ExactlyThree"));
    }
}
