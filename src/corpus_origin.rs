//! corpus_origin.rs — Detect whether a corpus is AI dialogue, human prose, code, or mixed.
//!
//! This is a heuristic-only implementation: keyword density, turn-taking patterns,
//! timestamps, speaker labels, and code-syntax fingerprints. No LLM APIs are used.
//!
//! The module mirrors the intent of the upstream Python `corpus_origin.py` Tier 1
//! detector, extended with the additional categories requested by the Rust port
//! (human prose, code, mixed).

use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Classified origin of a corpus.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CorpusOrigin {
    /// Dialogue transcripts with an AI assistant (Claude, ChatGPT, Gemini, etc.).
    AiDialogue,
    /// Plain human writing: journal, narrative, essay, blog, etc.
    HumanProse,
    /// Source code, configuration files, or technical markup.
    Code,
    /// Strong signals for more than one origin (e.g. AI conversation about code).
    Mixed,
}

impl std::fmt::Display for CorpusOrigin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CorpusOrigin::AiDialogue => write!(f, "ai_dialogue"),
            CorpusOrigin::HumanProse => write!(f, "human_prose"),
            CorpusOrigin::Code => write!(f, "code"),
            CorpusOrigin::Mixed => write!(f, "mixed"),
        }
    }
}

/// Per-origin scores returned by the low-level [`score_samples`] function.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OriginScores {
    pub ai_dialogue: f64,
    pub human_prose: f64,
    pub code: f64,
    pub mixed: f64,
}

/// Result of heuristic origin detection.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OriginResult {
    pub origin: CorpusOrigin,
    pub confidence: f64,
    pub primary_platform: Option<String>,
    pub user_name: Option<String>,
    pub agent_persona_names: Vec<String>,
    pub evidence: Vec<String>,
}

impl OriginResult {
    /// Serialize to a JSON value.
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "origin": self.origin.to_string(),
            "confidence": self.confidence,
            "primary_platform": self.primary_platform,
            "user_name": self.user_name,
            "agent_persona_names": self.agent_persona_names,
            "evidence": self.evidence,
        })
    }
}

// ── Well-known AI brand terms (unambiguous) ─────────────────────────────────

const AI_UNAMBIGUOUS_TERMS: &[&str] = &[
    // Anthropic-specific
    "Anthropic",
    "Claude Code",
    "Claude 3",
    "Claude 4",
    "claude mcp",
    "CLAUDE.md",
    ".claude/",
    // OpenAI-specific
    "ChatGPT",
    "GPT-4",
    "GPT-3",
    "GPT-5",
    "OpenAI",
    "gpt-4o",
    "gpt-4-turbo",
    "o1-preview",
    "o3",
    // Google-specific
    "gemini-pro",
    "gemini-1.5",
    "Google AI",
    // Meta / others
    "Mixtral",
    "Cohere",
    // AI-infrastructure terms
    "MCP",
    "LLM",
    "RAG",
    "fine-tune",
    "context window",
    "embedding",
];

// Terms that collide with common English / names / zodiac / animals.
// Only counted when the corpus also contains an unambiguous AI signal.
const AI_AMBIGUOUS_TERMS: &[&str] = &[
    "Claude", "Opus", "Sonnet", "Haiku", "Gemini", "Bard", "Llama", "Mistral",
];

// Turn-marker patterns commonly seen in AI-dialogue transcripts.
const TURN_MARKERS: &[&str] = &[
    r"(?i)\buser\s*:\s*",
    r"(?i)\bassistant\s*:\s*",
    r"(?i)\bhuman\s*:\s*",
    r"(?i)\bai\s*:\s*",
    r"(?i)\b>>>\s*User\b",
    r"(?i)\b>>>\s*Assistant\b",
];

// Speaker-label / timestamp patterns (e.g. `[Speaker 1]`, `12:34 PM`, `2024-01-01`).
const SPEAKER_LABEL_PATTERNS: &[&str] = &[
    r"(?i)\[\s*[^\]]+\s*\]\s*:",
    r"(?i)\b\d{1,2}:\d{2}\s*(?:AM|PM)\b",
    r"(?i)\b\d{4}-\d{2}-\d{2}\b",
    r"(?i)\buser\s*\(|\bassistant\s*\(",
];

// Code fingerprints: tokens and constructs that are rare in normal prose.
const CODE_KEYWORDS: &[&str] = &[
    "fn ",
    "def ",
    "class ",
    "import ",
    "from ",
    "const ",
    "let ",
    "var ",
    "function",
    "=>",
    "::",
    "#[",
    "std::",
    "pub fn",
    "impl ",
    "match ",
    "console.log",
    "print(",
    "println!",
    "printf(",
    "using ",
    "namespace",
    "package",
    "module",
    "return ",
    "if (",
    "else if",
    "for (",
    "while (",
    "struct ",
    "enum ",
    "trait ",
    "type ",
    "async ",
    "await ",
    "require(",
    "include <",
    "#include",
    "#define",
    "public static",
    "private ",
    "protected ",
    "int main",
    "void ",
    "String[]",
    "const char",
    "lambda",
];

const CODE_PUNCTUATION: &[&str] = &[";", "{", "}", "==", "!=", "&&", "||", "=>", "->"];

const CODE_LINE_PREFIXES: &[&str] = &["//", "/*", "*/", "#", "<!", "<?"];

/// Build a regex with word boundaries only where the literal term permits them.
fn brand_regex(term: &str) -> Regex {
    let escaped = regex::escape(term);
    let prefix = if term.starts_with(|c: char| c.is_alphanumeric() || c == '_') {
        r"\b"
    } else {
        ""
    };
    let suffix = if term.ends_with(|c: char| c.is_alphanumeric() || c == '_') {
        r"\b"
    } else {
        ""
    };
    let pattern = format!("{}{}{}", prefix, escaped, suffix);
    Regex::new(&pattern).expect("brand pattern must compile")
}

/// Count non-overlapping matches of a case-insensitive regex in `text`.
fn count_matches(re: &Regex, text: &str) -> usize {
    re.find_iter(text).count()
}

/// Count raw substring occurrences (case-insensitive for keywords).
fn count_substr(text: &str, needle: &str) -> usize {
    let lower_text = text.to_lowercase();
    let lower_needle = needle.to_lowercase();
    let mut count = 0;
    let mut start = 0;
    while let Some(pos) = lower_text[start..].find(&lower_needle) {
        count += 1;
        start += pos + lower_needle.len();
    }
    count
}

/// Score a corpus sample set and return per-origin scores in the range [0, 1].
///
/// The scoring intentionally avoids LLM calls. It is meant to be fast enough to
/// run over every drawer during ingestion.
pub fn score_samples(samples: &[String]) -> OriginScores {
    let combined: String = if samples.is_empty() {
        String::new()
    } else {
        let total_len: usize = samples.iter().map(|s| s.len()).sum();
        let mut buf = String::with_capacity(total_len + 2 * samples.len());
        for (i, s) in samples.iter().enumerate() {
            if i > 0 {
                buf.push('\n');
                buf.push('\n');
            }
            buf.push_str(s);
        }
        buf
    };

    let total_chars = combined.len().max(1);
    let thousand_chars = total_chars as f64 / 1000.0;

    // ── AI dialogue signals ───────────────────────────────────────────────
    let mut unambiguous_hits: HashMap<String, usize> = HashMap::new();
    let mut total_unambiguous = 0usize;
    for term in AI_UNAMBIGUOUS_TERMS {
        let re = brand_regex(term);
        let n = count_matches(&re, &combined);
        if n > 0 {
            unambiguous_hits.insert((*term).to_string(), n);
            total_unambiguous += n;
        }
    }

    let mut ambiguous_hits: HashMap<String, usize> = HashMap::new();
    let mut total_ambiguous = 0usize;
    for term in AI_AMBIGUOUS_TERMS {
        let re = brand_regex(term);
        let n = count_matches(&re, &combined);
        if n > 0 {
            ambiguous_hits.insert((*term).to_string(), n);
            total_ambiguous += n;
        }
    }

    let mut turn_hits = 0usize;
    let mut turn_types_found = std::collections::HashSet::new();
    for pattern in TURN_MARKERS {
        let re = Regex::new(pattern).expect("turn marker must compile");
        let n = count_matches(&re, &combined);
        if n > 0 {
            turn_hits += n;
            turn_types_found.insert(*pattern);
        }
    }

    let mut speaker_hits = 0usize;
    for pattern in SPEAKER_LABEL_PATTERNS {
        let re = Regex::new(pattern).expect("speaker label pattern must compile");
        speaker_hits += count_matches(&re, &combined);
    }

    let has_ai_context = total_unambiguous > 0 || turn_hits > 0;
    let counted_brand_hits = total_unambiguous + if has_ai_context { total_ambiguous } else { 0 };

    // Brand/turn density per 1000 characters.
    let brand_density = counted_brand_hits as f64 / thousand_chars;
    let turn_density = turn_hits as f64 / thousand_chars;
    let speaker_density = speaker_hits as f64 / thousand_chars;

    // AI score: sigmoid-ish capped combination.
    let ai_score = ((brand_density * 0.6 + turn_density * 0.35 + speaker_density * 0.05)
        / (1.0 + (brand_density * 0.6 + turn_density * 0.35 + speaker_density * 0.05)))
        .clamp(0.0, 1.0);

    // ── Code signals ──────────────────────────────────────────────────────
    let mut code_keyword_hits = 0usize;
    for keyword in CODE_KEYWORDS {
        code_keyword_hits += count_substr(&combined, keyword);
    }

    let mut code_punct_hits = 0usize;
    for punct in CODE_PUNCTUATION {
        code_punct_hits += count_substr(&combined, punct);
    }

    let mut code_line_prefixes = 0usize;
    for line in combined.lines() {
        let trimmed = line.trim();
        for prefix in CODE_LINE_PREFIXES {
            if trimmed.starts_with(prefix) {
                code_line_prefixes += 1;
                break;
            }
        }
    }

    let code_density =
        (code_keyword_hits + code_punct_hits + code_line_prefixes) as f64 / thousand_chars.max(1.0);
    let code_score = (code_density / 3.0).clamp(0.0, 1.0);

    // ── Human prose signals ───────────────────────────────────────────────
    // Prose is measured by long paragraphs, average word length, and low code density.
    let line_count = combined.lines().count().max(1);
    let paragraph_count = combined
        .split(['\n', '\r'])
        .filter(|line| line.split_whitespace().count() >= 6)
        .count()
        .max(1);
    let words: Vec<&str> = combined.split_whitespace().collect();
    let avg_words_per_line = if line_count > 0 {
        words.len() as f64 / line_count as f64
    } else {
        0.0
    };
    let prose_density = (paragraph_count as f64 * 0.4 + avg_words_per_line * 0.05).clamp(0.0, 1.0);
    let prose_score = (prose_density * (1.0 - code_score * 0.7)).clamp(0.0, 1.0);

    // ── Mixed score ───────────────────────────────────────────────────────
    let mixed_score = if ai_score > 0.3 && code_score > 0.3 {
        (ai_score + code_score).clamp(0.0, 1.0)
    } else {
        0.0
    };

    OriginScores {
        ai_dialogue: ai_score,
        human_prose: prose_score,
        code: code_score,
        mixed: mixed_score,
    }
}

/// Detect the most likely origin of a corpus using cheap heuristics.
///
/// Returns a structured [`OriginResult`] with confidence, evidence, and the best
/// guess at a primary platform when the corpus is AI dialogue.
///
/// The function never panics and never calls an external API.
pub fn detect_origin_heuristic(samples: &[String]) -> OriginResult {
    if samples.is_empty() {
        return OriginResult {
            origin: CorpusOrigin::HumanProse,
            confidence: 0.4,
            primary_platform: None,
            user_name: None,
            agent_persona_names: vec![],
            evidence: vec![
                "No samples provided — defaulting to human_prose with low confidence.".to_string(),
            ],
        };
    }

    let combined: String = {
        let total_len: usize = samples.iter().map(|s| s.len()).sum();
        let mut buf = String::with_capacity(total_len + 2 * samples.len());
        for (i, s) in samples.iter().enumerate() {
            if i > 0 {
                buf.push('\n');
                buf.push('\n');
            }
            buf.push_str(s);
        }
        buf
    };
    let total_chars = combined.len();

    // Recompute per-signal bookkeeping for evidence generation.
    let mut unambiguous_hits: HashMap<String, usize> = HashMap::new();
    let mut total_unambiguous = 0usize;
    for term in AI_UNAMBIGUOUS_TERMS {
        let re = brand_regex(term);
        let n = count_matches(&re, &combined);
        if n > 0 {
            unambiguous_hits.insert((*term).to_string(), n);
            total_unambiguous += n;
        }
    }

    let mut ambiguous_hits: HashMap<String, usize> = HashMap::new();
    let mut total_ambiguous = 0usize;
    for term in AI_AMBIGUOUS_TERMS {
        let re = brand_regex(term);
        let n = count_matches(&re, &combined);
        if n > 0 {
            ambiguous_hits.insert((*term).to_string(), n);
            total_ambiguous += n;
        }
    }

    let mut turn_hits = 0usize;
    let mut turn_types_found = std::collections::HashSet::new();
    for pattern in TURN_MARKERS {
        let re = Regex::new(pattern).expect("turn marker must compile");
        let n = count_matches(&re, &combined);
        if n > 0 {
            turn_hits += n;
            turn_types_found.insert(*pattern);
        }
    }

    let has_ai_context = total_unambiguous > 0 || turn_hits > 0;
    let counted_brand_hits = total_unambiguous + if has_ai_context { total_ambiguous } else { 0 };

    let scores = score_samples(samples);

    // Build evidence list.
    let mut evidence: Vec<String> = Vec::new();
    let mut shown_hits: HashMap<String, usize> = unambiguous_hits.clone();
    if has_ai_context {
        shown_hits.extend(ambiguous_hits.clone());
    }
    if !shown_hits.is_empty() {
        let mut terms: Vec<(String, usize)> = shown_hits.into_iter().collect();
        terms.sort_by_key(|b| std::cmp::Reverse(b.1));
        let top = terms.into_iter().take(5).collect::<Vec<_>>();
        let parts: Vec<String> = top
            .into_iter()
            .map(|(k, v)| format!("'{}' ({}x)", k, v))
            .collect();
        evidence.push(format!("AI brand terms: {}", parts.join(", ")));
    } else if !ambiguous_hits.is_empty() && !has_ai_context {
        let mut terms: Vec<(String, usize)> = ambiguous_hits.into_iter().collect();
        terms.sort_by_key(|b| std::cmp::Reverse(b.1));
        let top = terms.into_iter().take(3).collect::<Vec<_>>();
        let parts: Vec<String> = top
            .into_iter()
            .map(|(k, v)| format!("'{}' ({}x)", k, v))
            .collect();
        evidence.push(format!(
            "Ambiguous terms present but suppressed (no co-occurring AI signal): {}",
            parts.join(", ")
        ));
    }
    if turn_hits > 0 {
        evidence.push(format!(
            "Turn markers detected: {} occurrences across {} pattern types",
            turn_hits,
            turn_types_found.len()
        ));
    }
    if scores.code > 0.0 {
        evidence.push(format!(
            "Code signals detected: code score {:.2}",
            scores.code
        ));
    }
    if scores.human_prose > 0.0 {
        evidence.push(format!(
            "Human prose signals detected: prose score {:.2}",
            scores.human_prose
        ));
    }

    // Determine primary platform when AI is likely.
    let primary_platform = if scores.ai_dialogue >= 0.3 {
        Some(guess_platform(&combined))
    } else {
        None
    };

    // Decision thresholds.
    const MEANINGFUL_TEXT_FLOOR: usize = 150;
    const AI_BRAND_THRESHOLD: f64 = 0.5;
    const AI_TURN_THRESHOLD: f64 = 2.0;

    let brand_density = if total_chars > 0 {
        counted_brand_hits as f64 / (total_chars as f64 / 1000.0)
    } else {
        0.0
    };
    let turn_density = if total_chars > 0 {
        turn_hits as f64 / (total_chars as f64 / 1000.0)
    } else {
        0.0
    };

    // Strong AI + strong code → mixed.
    if scores.ai_dialogue >= 0.6 && scores.code >= 0.6 {
        return OriginResult {
            origin: CorpusOrigin::Mixed,
            confidence: scores.mixed,
            primary_platform,
            user_name: None,
            agent_persona_names: vec![],
            evidence,
        };
    }

    // Strong AI dialogue.
    if brand_density >= AI_BRAND_THRESHOLD
        || turn_density >= AI_TURN_THRESHOLD
        || scores.ai_dialogue >= 0.6
    {
        let confidence = (0.6 + 0.1 * (brand_density + turn_density + scores.ai_dialogue * 5.0))
            .clamp(0.6, 0.95);
        return OriginResult {
            origin: CorpusOrigin::AiDialogue,
            confidence,
            primary_platform,
            user_name: None,
            agent_persona_names: vec![],
            evidence,
        };
    }

    // Strong code.
    if scores.code >= 0.6 {
        return OriginResult {
            origin: CorpusOrigin::Code,
            confidence: scores.code.clamp(0.6, 0.95),
            primary_platform: None,
            user_name: None,
            agent_persona_names: vec![],
            evidence,
        };
    }

    // Meaningful absence of AI/code → human prose.
    if counted_brand_hits == 0
        && turn_hits == 0
        && scores.code < 0.2
        && total_chars >= MEANINGFUL_TEXT_FLOOR
    {
        let mut narrative_evidence = evidence.clone();
        narrative_evidence.push(format!(
            "no unambiguous AI or code signal across {} chars of text — pure narrative",
            total_chars
        ));
        return OriginResult {
            origin: CorpusOrigin::HumanProse,
            confidence: 0.9,
            primary_platform: None,
            user_name: None,
            agent_persona_names: vec![],
            evidence: narrative_evidence,
        };
    }

    // Ambiguous or too-short-to-tell: default stance mirrors upstream — assume
    // AI dialogue with low confidence, because false negatives are expensive.
    let reason = if counted_brand_hits > 0 || turn_hits > 0 {
        "weak signal"
    } else {
        "insufficient text"
    };
    evidence.push(format!(
        "{} — applying default stance (ai_dialogue, low confidence)",
        reason
    ));
    OriginResult {
        origin: CorpusOrigin::AiDialogue,
        confidence: 0.4,
        primary_platform,
        user_name: None,
        agent_persona_names: vec![],
        evidence,
    }
}

/// Guess the primary AI platform based on unambiguous brand-term frequency.
fn guess_platform(text: &str) -> String {
    let lower = text.to_lowercase();
    let mut scores: HashMap<&str, usize> = HashMap::new();
    for (name, terms) in [
        (
            "Claude (Anthropic)",
            &["claude", "anthropic", "claude code", "claude mcp"][..],
        ),
        (
            "ChatGPT (OpenAI)",
            &[
                "chatgpt",
                "openai",
                "gpt-4",
                "gpt-3",
                "gpt-5",
                "o1-preview",
                "o3",
            ][..],
        ),
        (
            "Gemini (Google)",
            &["gemini", "gemini-pro", "gemini-1.5", "google ai", "bard"][..],
        ),
    ] {
        let mut count = 0usize;
        for term in terms {
            count += count_substr(&lower, term);
        }
        scores.insert(name, count);
    }
    let mut best: Vec<(&str, usize)> = scores.into_iter().filter(|(_, c)| *c > 0).collect();
    best.sort_by_key(|b| std::cmp::Reverse(b.1));
    best.first()
        .map(|(name, _)| (*name).to_string())
        .unwrap_or_else(|| "Unknown AI platform".to_string())
}

/// Convenience helper for CLI / onboarding: take a single corpus string and run detection.
pub fn detect_origin(text: &str) -> OriginResult {
    detect_origin_heuristic(&[text.to_string()])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn samples(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn test_display_corpus_origin() {
        assert_eq!(CorpusOrigin::AiDialogue.to_string(), "ai_dialogue");
        assert_eq!(CorpusOrigin::HumanProse.to_string(), "human_prose");
        assert_eq!(CorpusOrigin::Code.to_string(), "code");
        assert_eq!(CorpusOrigin::Mixed.to_string(), "mixed");
    }

    #[test]
    fn test_empty_samples() {
        let result = detect_origin_heuristic(&[]);
        assert_eq!(result.origin, CorpusOrigin::HumanProse);
        assert_eq!(result.confidence, 0.4);
        assert!(result.evidence[0].contains("No samples"));
    }

    #[test]
    fn test_ai_dialogue_unambiguous_brand() {
        let result = detect_origin(
            "Claude Code just helped me refactor a crate. I love ChatGPT for quick questions.",
        );
        assert_eq!(result.origin, CorpusOrigin::AiDialogue);
        assert!(result.confidence >= 0.6);
        assert!(result
            .evidence
            .iter()
            .any(|e| e.to_lowercase().contains("ai brand terms")));
        assert!(result.primary_platform.is_some());
        let platform = result.primary_platform.unwrap();
        assert!(platform.contains("Claude") || platform.contains("ChatGPT"));
    }

    #[test]
    fn test_ai_dialogue_turn_markers() {
        let result = detect_origin_heuristic(&samples(&[
            "User: What is the capital of France?",
            "Assistant: Paris is the capital of France.",
            "User: Thanks!",
            "Assistant: You're welcome. ChatGPT is great for quick questions.",
        ]));
        assert_eq!(result.origin, CorpusOrigin::AiDialogue);
        assert!(result
            .evidence
            .iter()
            .any(|e| e.to_lowercase().contains("turn markers")
                || e.to_lowercase().contains("ai brand")));
    }

    #[test]
    fn test_ambiguous_terms_suppressed_without_context() {
        // "Claude" appears as a French name; "Gemini" appears as astrology.
        let result = detect_origin_heuristic(&samples(&[
            "Claude went to the market. Gemini was rising in the sky.",
            "Llama spit at Mistral. Sonnet wrote a haiku.",
        ]));
        // Ambiguous terms should not tip us into AI dialogue.
        assert!(result.confidence <= 0.5);
        assert!(result
            .evidence
            .iter()
            .any(|e| e.to_lowercase().contains("suppressed")));
    }

    #[test]
    fn test_ambiguous_terms_counted_with_context() {
        // "Claude" + "ChatGPT" gives an unambiguous co-signal, so "Claude" counts.
        let result =
            detect_origin("Claude and ChatGPT both answered my questions. Bard stayed quiet.");
        assert_eq!(result.origin, CorpusOrigin::AiDialogue);
        assert!(result.confidence >= 0.6);
        assert!(result
            .evidence
            .iter()
            .any(|e| e.to_lowercase().contains("claude")));
    }

    #[test]
    fn test_human_prose() {
        let result = detect_origin(
            "Dear diary, today I walked to the lake and thought about the future. The sky was grey and the water was still. I felt a calm that I had not felt in weeks.",
        );
        assert_eq!(result.origin, CorpusOrigin::HumanProse);
        assert!(result.confidence >= 0.8);
        assert!(result
            .evidence
            .iter()
            .any(|e| e.to_lowercase().contains("pure narrative")));
    }

    #[test]
    fn test_code_detection() {
        let code = r#"
fn main() {
    let x = 42;
    println!("{}", x);
}

fn helper(a: i32) -> i32 {
    a * 2
}
"#;
        let result = detect_origin(code);
        assert_eq!(result.origin, CorpusOrigin::Code);
        assert!(result.confidence >= 0.6);
        assert!(result
            .evidence
            .iter()
            .any(|e| e.to_lowercase().contains("code")));
    }

    #[test]
    fn test_mixed_code_and_ai_dialogue() {
        let result = detect_origin_heuristic(&samples(&[
            "User: Can you help me write a Rust function?",
            "Assistant: Sure. fn main() { println!(\"hello\"); }",
            "User: Great, now add error handling.",
            "Assistant: Use Result<T, E> and the ? operator.",
            "User: I also use ChatGPT and Claude Code for this.",
            "Assistant: Both are helpful AI assistants.",
        ]));
        assert!(matches!(
            result.origin,
            CorpusOrigin::Mixed | CorpusOrigin::AiDialogue
        ));
        assert!(result.confidence >= 0.6);
        assert!(result.primary_platform.is_some());
    }

    #[test]
    fn test_score_samples_normalized() {
        let scores = score_samples(&samples(&["User: hi", "Assistant: hello"]));
        assert!(scores.ai_dialogue >= 0.0 && scores.ai_dialogue <= 1.0);
        assert!(scores.human_prose >= 0.0 && scores.human_prose <= 1.0);
        assert!(scores.code >= 0.0 && scores.code <= 1.0);
        assert!(scores.mixed >= 0.0 && scores.mixed <= 1.0);
    }

    #[test]
    fn test_guess_platform_claude() {
        assert_eq!(
            guess_platform("I use Claude Code and Anthropic's API."),
            "Claude (Anthropic)"
        );
    }

    #[test]
    fn test_guess_platform_chatgpt() {
        assert_eq!(
            guess_platform("ChatGPT plus gpt-4o is really fast."),
            "ChatGPT (OpenAI)"
        );
    }

    #[test]
    fn test_guess_platform_gemini() {
        assert_eq!(
            guess_platform("Bard was replaced by Gemini and gemini-pro."),
            "Gemini (Google)"
        );
    }

    #[test]
    fn test_origin_result_to_json() {
        let result = OriginResult {
            origin: CorpusOrigin::AiDialogue,
            confidence: 0.85,
            primary_platform: Some("Claude (Anthropic)".to_string()),
            user_name: None,
            agent_persona_names: vec![],
            evidence: vec!["test".to_string()],
        };
        let json = result.to_json();
        assert_eq!(json["origin"], "ai_dialogue");
        assert_eq!(json["confidence"], 0.85);
        assert_eq!(json["primary_platform"], "Claude (Anthropic)");
    }

    #[test]
    fn test_insufficient_text_default_stance() {
        let result = detect_origin("ok");
        // Too short to tell; default stance is AI dialogue with low confidence.
        assert_eq!(result.origin, CorpusOrigin::AiDialogue);
        assert_eq!(result.confidence, 0.4);
        assert!(result
            .evidence
            .iter()
            .any(|e| e.to_lowercase().contains("insufficient text")));
    }

    #[test]
    fn test_speaker_labels_and_timestamps() {
        let result = detect_origin_heuristic(&samples(&[
            "[2024-01-15] User: hello",
            "[2024-01-15] Assistant: hi there",
            "[2024-01-15] User: Can you help me refactor this Claude Code project?",
            "[2024-01-15] Assistant: Sure, I can help with that.",
        ]));
        assert!(matches!(
            result.origin,
            CorpusOrigin::AiDialogue | CorpusOrigin::Mixed
        ));
        assert!(result.confidence >= 0.6);
    }

    #[test]
    fn test_python_code_detection() {
        let result = detect_origin(
            "def foo(x):\n    if x > 0:\n        return x * 2\n    return 0\n\nimport os",
        );
        assert_eq!(result.origin, CorpusOrigin::Code);
        assert!(result.confidence >= 0.6);
    }
}
