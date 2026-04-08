use lazy_static::lazy_static;
use regex::Regex;
use std::collections::HashSet;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;
use symspell::{SymSpell, UnicodeStringStrategy, Verbosity};

lazy_static! {
    static ref RE_HAS_DIGIT: Regex = Regex::new(r"\d").unwrap();
    static ref RE_IS_CAMEL: Regex = Regex::new(r"[A-Z][a-z]+[A-Z]|[a-z]+[A-Z]").unwrap();
    static ref RE_IS_ALLCAPS: Regex = Regex::new(r"^[A-Z_@#$%^&*()+={\[\]}|<>?.:/\\]+$").unwrap();
    static ref RE_IS_TECHNICAL: Regex = Regex::new(r"[-_]").unwrap();
    static ref RE_IS_URL_OR_PATH: Regex =
        Regex::new(r"(?i)https?://|www\.|/Users/|~/|^\.{1,2}/|\.[a-z]{2,4}$").unwrap();
    static ref RE_IS_CODE_OR_EMOJI: Regex = Regex::new(r"[`*_#{}\[\]\\]").unwrap();
    static ref RE_TOKEN: Regex = Regex::new(r"(\S+)").unwrap();
}

/// Returns true if the token should be skipped for spellcheck.
pub fn should_skip(word: &str) -> bool {
    if word.len() < 4 {
        return true;
    }
    if RE_HAS_DIGIT.is_match(word)
        || RE_IS_CAMEL.is_match(word)
        || RE_IS_ALLCAPS.is_match(word)
        || RE_IS_TECHNICAL.is_match(word)
        || RE_IS_URL_OR_PATH.is_match(word)
        || RE_IS_CODE_OR_EMOJI.is_match(word)
    {
        return true;
    }
    false
}

pub struct SpellChecker {
    symspell: SymSpell<UnicodeStringStrategy>,
}

impl SpellChecker {
    pub fn new() -> Self {
        let mut symspell: SymSpell<UnicodeStringStrategy> = SymSpell::default();

        // Common words for testing and basic functionality.
        let common_words = vec![
            "already",
            "know",
            "question",
            "before",
            "different",
            "benchmarks",
            "testing",
            "please",
            "spell",
            "check",
            "really",
            "write",
            "coherently",
            "there",
            "many",
            "also",
            "your",
            "been",
            "from",
            "have",
            "they",
            "were",
            "what",
            "when",
            "which",
            "with",
            "decided",
            "switch",
            "performance",
        ];

        for word in common_words {
            let line = format!("{} 100", word);
            symspell.load_dictionary_line(&line, 0, 1, " ");
        }

        // Try loading system words if available.
        let dict_path = "/usr/share/dict/words";
        if Path::new(dict_path).exists() {
            if let Ok(file) = File::open(dict_path) {
                let reader = BufReader::new(file);
                for line in reader.lines().map_while(Result::ok) {
                    let word = line.trim();
                    if !word.is_empty() && word.len() >= 4 {
                        symspell.load_dictionary_line(
                            &format!("{} 1", word.to_lowercase()),
                            0,
                            1,
                            " ",
                        );
                    }
                }
            }
        }

        Self { symspell }
    }

    pub fn spellcheck_transcript(&self, content: &str) -> String {
        let known_names = HashSet::new();
        content
            .lines()
            .map(|line| {
                let trimmed = line.trim_start();
                if !trimmed.starts_with('>') {
                    return line.to_string();
                }

                let p_idx = line.find('>').unwrap();
                let prefix = &line[0..p_idx + 1];
                let rest = &line[p_idx + 1..];

                let ws_len = rest.len() - rest.trim_start().len();
                let mid_ws = &rest[0..ws_len];
                let message = rest.trim_start();

                let corrected = self.spellcheck_user_text(message, &known_names);
                format!("{}{}{}", prefix, mid_ws, corrected)
            })
            .collect::<Vec<String>>()
            .join("\n")
    }

    pub fn spellcheck_user_text(&self, text: &str, known_names: &HashSet<String>) -> String {
        let mut out = String::new();
        let mut last_idx = 0;

        for mat in RE_TOKEN.find_iter(text) {
            out.push_str(&text[last_idx..mat.start()]);

            let token = mat.as_str();

            // Separate trailing punctuation.
            let mut end = token.len();
            while end > 0 && ".,!?;:'\")".contains(token.chars().nth(end - 1).unwrap()) {
                end -= 1;
            }

            let stripped = &token[0..end];
            let punct = &token[end..];

            if stripped.is_empty() || should_skip(stripped) || known_names.contains(stripped) {
                out.push_str(token);
            } else if stripped.chars().next().is_some_and(|c| c.is_uppercase()) {
                // Keep capitalized words as is.
                out.push_str(token);
            } else {
                let max_edits = if stripped.len() <= 7 { 2 } else { 3 };
                let suggestions = self
                    .symspell
                    .lookup(stripped, Verbosity::Top, max_edits as i64);
                if let Some(suggestion) = suggestions.first() {
                    out.push_str(&suggestion.term);
                    out.push_str(punct);
                } else {
                    out.push_str(token);
                }
            }
            last_idx = mat.end();
        }
        out.push_str(&text[last_idx..]);
        out
    }
}

impl Default for SpellChecker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_should_skip() {
        assert!(should_skip("it")); // < 4
        assert!(should_skip("ChromaDB")); // CamelCase
        assert!(should_skip("bge-large")); // Technical (hyphen)
        assert!(should_skip("NDCG@10")); // ALLCAPS / Digit
        assert!(should_skip("3am")); // Digit / < 4
        assert!(should_skip("https://example.com")); // URL
        assert!(should_skip("./src/main.rs")); // File path
        assert!(should_skip("../docs/")); // File path
        assert!(!should_skip("already")); // Normal
    }

    #[test]
    fn test_spellcheck_simple_typo() {
        let sc = SpellChecker::new();
        let known_names = HashSet::new();
        assert_eq!(
            sc.spellcheck_user_text("alredy knoe", &known_names),
            "already know"
        );
    }

    #[test]
    fn test_spellcheck_transcript() {
        let sc = SpellChecker::new();
        let transcript = "> alredy knoe\nAssistant: alredy knoe";
        let corrected = sc.spellcheck_transcript(transcript);
        assert_eq!(corrected, "> already know\nAssistant: alredy knoe");
    }

    #[test]
    fn test_known_names_preservation() {
        let sc = SpellChecker::new();
        let mut known_names = HashSet::new();
        known_names.insert("alredy".to_string());
        assert_eq!(
            sc.spellcheck_user_text("alredy knoe", &known_names),
            "alredy know"
        );
    }
}
