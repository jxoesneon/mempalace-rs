//! LLM refine — local, offline heuristic refinement of memory entries.
//!
//! This module provides helpers to "refine" a draft memory entry before it is
//! stored: spell-checking, entity extraction, capitalization normalization, and
//! a small amount of optional LLM-client polishing. It is designed to work with
//! the local LLM client and never requires external APIs by default.

use crate::entity_detector::extract_entities;
use crate::spellcheck::SpellChecker;
use anyhow::Result;
use serde::{Deserialize, Serialize};

/// Refinement options.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct RefineOptions {
    pub spellcheck: bool,
    pub extract_entities: bool,
    pub normalize_whitespace: bool,
    pub title_case_names: bool,
}

impl RefineOptions {
    pub fn all() -> Self {
        Self {
            spellcheck: true,
            extract_entities: true,
            normalize_whitespace: true,
            title_case_names: true,
        }
    }
}

/// Result of refining a text.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RefineResult {
    pub text: String,
    pub entities: Vec<String>,
    pub corrections: Vec<String>,
    pub changed: bool,
}

/// Refine a text using heuristic, local-only processing.
pub fn refine_text(text: &str, options: &RefineOptions) -> Result<RefineResult> {
    let mut result = RefineResult {
        text: text.to_string(),
        entities: Vec::new(),
        corrections: Vec::new(),
        changed: false,
    };

    if options.normalize_whitespace {
        let normalized = normalize_whitespace(&result.text);
        if normalized != result.text {
            result.changed = true;
            result.text = normalized;
        }
    }

    if options.spellcheck {
        let checker = SpellChecker::default();
        let known_names: std::collections::HashSet<String> = std::collections::HashSet::new();
        let corrected = checker.spellcheck_user_text(&result.text, &known_names);
        if corrected != result.text {
            result.changed = true;
            result.text = corrected;
        }
    }

    if options.extract_entities {
        result.entities = extract_entities(&result.text)
            .into_iter()
            .map(|e| e.name)
            .collect();
    }

    if options.title_case_names {
        let updated = title_case_names(&result.text, &result.entities);
        if updated != result.text {
            result.changed = true;
            result.text = updated;
        }
    }

    Ok(result)
}

/// Normalize whitespace: collapse multiple spaces, trim, and normalize newlines.
pub fn normalize_whitespace(text: &str) -> String {
    text.lines()
        .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

/// Title-case known entity names in a text while leaving other words untouched.
pub fn title_case_names(text: &str, entities: &[String]) -> String {
    let mut out = text.to_string();
    for entity in entities {
        let target = entity.to_lowercase();
        let replacement = title_case(entity);
        let mut result = String::new();
        let mut last = 0;
        for (idx, _) in out.to_lowercase().match_indices(&target) {
            result.push_str(&out[last..idx]);
            result.push_str(&replacement);
            last = idx + target.len();
        }
        result.push_str(&out[last..]);
        out = result;
    }
    out
}

fn title_case(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => c.to_uppercase().collect::<String>() + &chars.as_str().to_lowercase(),
    }
}

/// Ask a local LLM client to polish text, returning the original on failure.
/// This is a defensive wrapper: if the client is not available or returns an
/// error, the original text is preserved unchanged.
pub fn polish_with_llm(text: &str, _client: &dyn crate::llm_client::LlmClient) -> String {
    // Local-only refinement is intentionally conservative. A future iteration
    // can wire the LLM client here; for now we keep the original text so the
    // system never accidentally degrades user content.
    text.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_whitespace() {
        assert_eq!(
            normalize_whitespace("  hello   world  \n\n  again  "),
            "hello world\nagain"
        );
    }

    #[test]
    fn test_title_case() {
        assert_eq!(title_case("alice"), "Alice");
        assert_eq!(title_case("ALICE"), "Alice");
    }

    #[test]
    fn test_title_case_names() {
        let text = "alice and bob went to the store.";
        let entities = vec!["alice".to_string(), "bob".to_string()];
        let out = title_case_names(text, &entities);
        assert!(out.contains("Alice"));
        assert!(out.contains("Bob"));
    }

    #[test]
    fn test_refine_text_basic() {
        let result = refine_text("  alice   visited   paris  ", &RefineOptions::all()).unwrap();
        assert_eq!(result.text, "alice visited paris");
        assert!(result.changed);
        // The entity detector may or may not find these simple words; just ensure
        // the function returned successfully and normalized whitespace.
        assert!(result.text.split_whitespace().count() <= 3);
    }

    #[test]
    fn test_refine_text_disabled() {
        let result = refine_text("  alice   visited   paris  ", &RefineOptions::default()).unwrap();
        assert_eq!(result.text, "  alice   visited   paris  ");
        assert!(!result.changed);
    }

    #[test]
    fn test_polish_with_llm_passthrough() {
        let text = "keep me";
        let client = crate::llm_client::MockClient::new("mock response");
        assert_eq!(polish_with_llm(text, &client), text);
    }
}
