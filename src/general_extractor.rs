//! # General Extractor
//!
//! Extract structured facts and entities from arbitrary text using regex heuristics.
//! No LLM calls are used.  This is a Rust forward port of the upstream Python
//! `general_extractor.py` module with additional fact/entity extraction.
//!
//! Public API:
//! - [`ExtractedFacts`]: container for all extracted structured facts.
//! - [`extract_facts`](fn@extract_facts): extract URLs, emails, dates, key-value pairs,
//!   named entities, memory snippets, etc.
//! - [`extract_entities`](fn@extract_entities): extract named entities with a kind label.
//! - [`extract_memories`](fn@extract_memories): extract heuristic memory types
//!   (decision, preference, milestone, problem, emotional).

use crate::models::MemoryType;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

lazy_static::lazy_static! {
    // -------------------------------------------------------------------------
    // Structured fact regexes
    // -------------------------------------------------------------------------
    static ref URL_RE: Regex = Regex::new(
        r#"https?://[^\s<>"{}|\\^`\[\]]+[^\s<>"{}|\\^`\[\].,;!?]"#
    ).unwrap();

    static ref EMAIL_RE: Regex = Regex::new(
        r"[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}"
    ).unwrap();

    // ISO 2024-06-23, US 06/23/2024, EU 23/06/2024, textual June 23, 2024
    static ref DATE_RE: Regex = Regex::new(
        r"(?i)\b(\d{4}-\d{2}-\d{2}|\d{1,2}[/-]\d{1,2}[/-]\d{2,4}|(?:Jan|Feb|Mar|Apr|May|Jun|Jul|Aug|Sep|Oct|Nov|Dec)[a-z]*\.?\s+\d{1,2}(?:,?\s+\d{4})?)\b"
    ).unwrap();

    static ref PHONE_RE: Regex = Regex::new(
        r"(?:\+?\d{1,3}[-.\s]?)?\(?\d{3}\)?[-.\s]?\d{3}[-.\s]?\d{4}\b"
    ).unwrap();

    static ref IP_RE: Regex = Regex::new(
        r"\b(?:\d{1,3}\.){3}\d{1,3}\b"
    ).unwrap();

    static ref HASHTAG_RE: Regex = Regex::new(r"#\w+").unwrap();
    static ref MENTION_RE: Regex = Regex::new(r"@\w+").unwrap();

    // Inline code `...` and simple dotted identifiers.
    static ref CODE_REF_RE: Regex = Regex::new(
        r"`[^`]+`|\b[A-Za-z_][A-Za-z0-9_]*(?:\.[A-Za-z_][A-Za-z0-9_]*)+\b"
    ).unwrap();

    // Key-value pairs like "Key: value" or "Key = value" on a single line.
    static ref KV_RE: Regex = Regex::new(
        r"(?im)^\s*([A-Za-z][A-Za-z0-9_ -]{0,40})\s*[:=][ \t]*([^\n]+?)\s*$"
    ).unwrap();

    // -------------------------------------------------------------------------
    // Named entity regexes
    // -------------------------------------------------------------------------
    static ref PEOPLE_RE: Regex = Regex::new(
        r"\b[A-Z][a-z]+(?:\s+[A-Z][a-z]+)+\b"
    ).unwrap();

    static ref ORG_RE: Regex = Regex::new(
        r"\b[A-Z][a-zA-Z]+(?:\s+[A-Z][a-zA-Z]+){1,3}\b"
    ).unwrap();

    static ref PROJECT_RE: Regex = Regex::new(
        r"(?i)\b(?:project|repo|repository|library|crate|package|module|app|application|service|tool|system|platform|framework|plugin|extension|bot|agent|pipeline|workspace)\s+([A-Za-z][A-Za-z0-9_-]*)\b"
    ).unwrap();

    // -------------------------------------------------------------------------
    // Memory marker regexes (ported from upstream general_extractor.py)
    // -------------------------------------------------------------------------
    static ref DECISION_MARKERS: Vec<Regex> = vec![
        Regex::new(r"(?i)\blet'?s (use|go with|try|pick|choose|switch to)\b").unwrap(),
        Regex::new(r"(?i)\bwe (should|decided|chose|went with|picked|settled on)\b").unwrap(),
        Regex::new(r"(?i)\bi'?m going (to|with)\b").unwrap(),
        Regex::new(r"(?i)\bbetter (to|than|approach|option|choice)\b").unwrap(),
        Regex::new(r"(?i)\binstead of\b").unwrap(),
        Regex::new(r"(?i)\brather than\b").unwrap(),
        Regex::new(r"(?i)\bthe reason (is|was|being)\b").unwrap(),
        Regex::new(r"(?i)\bbecause\b").unwrap(),
        Regex::new(r"(?i)\btrade-?off\b").unwrap(),
        Regex::new(r"(?i)\bpros and cons\b").unwrap(),
        Regex::new(r"(?i)\bover\b.*\bbecause\b").unwrap(),
        Regex::new(r"(?i)\barchitecture\b").unwrap(),
        Regex::new(r"(?i)\bapproach\b").unwrap(),
        Regex::new(r"(?i)\bstrategy\b").unwrap(),
        Regex::new(r"(?i)\bpattern\b").unwrap(),
        Regex::new(r"(?i)\bstack\b").unwrap(),
        Regex::new(r"(?i)\bframework\b").unwrap(),
        Regex::new(r"(?i)\binfrastructure\b").unwrap(),
        Regex::new(r"(?i)\bset (it |this )?to\b").unwrap(),
        Regex::new(r"(?i)\bconfigure\b").unwrap(),
        Regex::new(r"(?i)\bdefault\b").unwrap(),
    ];

    static ref PREFERENCE_MARKERS: Vec<Regex> = vec![
        Regex::new(r"(?i)\bi prefer\b").unwrap(),
        Regex::new(r"(?i)\balways use\b").unwrap(),
        Regex::new(r"(?i)\bnever use\b").unwrap(),
        Regex::new(r"(?i)\bdon'?t (ever |like to )?(use|do|mock|stub|import)\b").unwrap(),
        Regex::new(r"(?i)\bi like (to|when|how)\b").unwrap(),
        Regex::new(r"(?i)\bi hate (when|how|it when)\b").unwrap(),
        Regex::new(r"(?i)\bplease (always|never|don'?t)\b").unwrap(),
        Regex::new(r"(?i)\bmy (rule|preference|style|convention) is\b").unwrap(),
        Regex::new(r"(?i)\bwe (always|never)\b").unwrap(),
        Regex::new(r"(?i)\bfunctional\b.*\bstyle\b").unwrap(),
        Regex::new(r"(?i)\bimperative\b").unwrap(),
        Regex::new(r"(?i)\bsnake_?case\b").unwrap(),
        Regex::new(r"(?i)\bcamel_?case\b").unwrap(),
        Regex::new(r"(?i)\btabs\b.*\bspaces\b").unwrap(),
        Regex::new(r"(?i)\bspaces\b.*\btabs\b").unwrap(),
        Regex::new(r"(?i)\buse\b.*\binstead of\b").unwrap(),
    ];

    static ref MILESTONE_MARKERS: Vec<Regex> = vec![
        Regex::new(r"(?i)\bit works\b").unwrap(),
        Regex::new(r"(?i)\bit worked\b").unwrap(),
        Regex::new(r"(?i)\bgot it working\b").unwrap(),
        Regex::new(r"(?i)\bfixed\b").unwrap(),
        Regex::new(r"(?i)\bsolved\b").unwrap(),
        Regex::new(r"(?i)\bbreakthrough\b").unwrap(),
        Regex::new(r"(?i)\bfigured (it )?out\b").unwrap(),
        Regex::new(r"(?i)\bnailed it\b").unwrap(),
        Regex::new(r"(?i)\bcracked (it|the)\b").unwrap(),
        Regex::new(r"(?i)\bfinally\b").unwrap(),
        Regex::new(r"(?i)\bfirst time\b").unwrap(),
        Regex::new(r"(?i)\bfirst ever\b").unwrap(),
        Regex::new(r"(?i)\bnever (done|been|had) before\b").unwrap(),
        Regex::new(r"(?i)\bdiscovered\b").unwrap(),
        Regex::new(r"(?i)\brealized\b").unwrap(),
        Regex::new(r"(?i)\bfound (out|that)\b").unwrap(),
        Regex::new(r"(?i)\bturns out\b").unwrap(),
        Regex::new(r"(?i)\bthe key (is|was|insight)\b").unwrap(),
        Regex::new(r"(?i)\bthe trick (is|was)\b").unwrap(),
        Regex::new(r"(?i)\bnow i (understand|see|get it)\b").unwrap(),
        Regex::new(r"(?i)\bbuilt\b").unwrap(),
        Regex::new(r"(?i)\bcreated\b").unwrap(),
        Regex::new(r"(?i)\bimplemented\b").unwrap(),
        Regex::new(r"(?i)\bshipped\b").unwrap(),
        Regex::new(r"(?i)\blaunched\b").unwrap(),
        Regex::new(r"(?i)\bdeployed\b").unwrap(),
        Regex::new(r"(?i)\breleased\b").unwrap(),
        Regex::new(r"(?i)\bprototype\b").unwrap(),
        Regex::new(r"(?i)\bproof of concept\b").unwrap(),
        Regex::new(r"(?i)\bdemo\b").unwrap(),
        Regex::new(r"(?i)\bversion \d").unwrap(),
        Regex::new(r"(?i)\bv\d+\.\d+").unwrap(),
        Regex::new(r"(?i)\d+x (compression|faster|slower|better|improvement|reduction)").unwrap(),
        Regex::new(r"(?i)\d+% (reduction|improvement|faster|better|smaller)").unwrap(),
    ];

    static ref PROBLEM_MARKERS: Vec<Regex> = vec![
        Regex::new(r"(?i)\b(bug|error|crash|fail|broke|broken|issue|problem)\b").unwrap(),
        Regex::new(r"(?i)\bdoesn'?t work\b").unwrap(),
        Regex::new(r"(?i)\bnot working\b").unwrap(),
        Regex::new(r"(?i)\bwon'?t\b.*\bwork\b").unwrap(),
        Regex::new(r"(?i)\bkeeps? (failing|crashing|breaking|erroring)\b").unwrap(),
        Regex::new(r"(?i)\broot cause\b").unwrap(),
        Regex::new(r"(?i)\bthe (problem|issue|bug) (is|was)\b").unwrap(),
        Regex::new(r"(?i)\bturns out\b.*\b(was|because|due to)\b").unwrap(),
        Regex::new(r"(?i)\bthe fix (is|was)\b").unwrap(),
        Regex::new(r"(?i)\bworkaround\b").unwrap(),
        Regex::new(r"(?i)\bthat'?s why\b").unwrap(),
        Regex::new(r"(?i)\bthe reason it\b").unwrap(),
        Regex::new(r"(?i)\bfixed (it |the |by )\b").unwrap(),
        Regex::new(r"(?i)\bsolution (is|was)\b").unwrap(),
        Regex::new(r"(?i)\bresolved\b").unwrap(),
        Regex::new(r"(?i)\bpatched\b").unwrap(),
        Regex::new(r"(?i)\bthe answer (is|was)\b").unwrap(),
        Regex::new(r"(?i)\b(had|need) to\b.*\binstead\b").unwrap(),
    ];

    static ref EMOTION_MARKERS: Vec<Regex> = vec![
        Regex::new(r"(?i)\blove\b").unwrap(),
        Regex::new(r"(?i)\bscared\b").unwrap(),
        Regex::new(r"(?i)\bafraid\b").unwrap(),
        Regex::new(r"(?i)\bproud\b").unwrap(),
        Regex::new(r"(?i)\bhurt\b").unwrap(),
        Regex::new(r"(?i)\bhappy\b").unwrap(),
        Regex::new(r"(?i)\bsad\b").unwrap(),
        Regex::new(r"(?i)\bcry\b").unwrap(),
        Regex::new(r"(?i)\bcrying\b").unwrap(),
        Regex::new(r"(?i)\bmiss\b").unwrap(),
        Regex::new(r"(?i)\bsorry\b").unwrap(),
        Regex::new(r"(?i)\bgrateful\b").unwrap(),
        Regex::new(r"(?i)\bangry\b").unwrap(),
        Regex::new(r"(?i)\bworried\b").unwrap(),
        Regex::new(r"(?i)\blonely\b").unwrap(),
        Regex::new(r"(?i)\bbeautiful\b").unwrap(),
        Regex::new(r"(?i)\bamazing\b").unwrap(),
        Regex::new(r"(?i)\bwonderful\b").unwrap(),
        Regex::new(r"(?i)i feel").unwrap(),
        Regex::new(r"(?i)i'm scared").unwrap(),
        Regex::new(r"(?i)i love you").unwrap(),
        Regex::new(r"(?i)i'm sorry").unwrap(),
        Regex::new(r"(?i)i can't").unwrap(),
        Regex::new(r"(?i)i wish").unwrap(),
        Regex::new(r"(?i)i miss").unwrap(),
        Regex::new(r"(?i)i need").unwrap(),
        Regex::new(r"(?i)never told anyone").unwrap(),
        Regex::new(r"(?i)nobody knows").unwrap(),
        Regex::new(r"\*[^*]+\*").unwrap(),
    ];

    static ref CODE_LINE_PATTERNS: Vec<Regex> = vec![
        Regex::new(r"^\s*[\$#]\s").unwrap(),
        Regex::new(r"^\s*(cd|source|echo|export|pip|npm|git|python|bash|curl|wget|mkdir|rm|cp|mv|ls|cat|grep|find|chmod|sudo|brew|docker)\s").unwrap(),
        Regex::new(r"^\s*```").unwrap(),
        Regex::new(r"^\s*(import|from|def|class|function|const|let|var|return)\s").unwrap(),
        Regex::new(r"^\s*[A-Z_]{2,}=").unwrap(),
        Regex::new(r"^\s*\|").unwrap(),
        Regex::new(r"^\s*[-]{2,}").unwrap(),
        Regex::new(r"^\s*[{\}\[\]]\s*$").unwrap(),
        Regex::new(r"(?i)^\s*(if|for|while|try|except|elif|else:)\b").unwrap(),
        Regex::new(r"^\s*\w+\.\w+\(").unwrap(),
        Regex::new(r"^\s*\w+ = \w+\.\w+").unwrap(),
    ];

    static ref POSITIVE_WORDS: HashSet<&'static str> = {
        let mut s = HashSet::new();
        s.insert("pride"); s.insert("proud"); s.insert("joy"); s.insert("happy");
        s.insert("love"); s.insert("loving"); s.insert("beautiful"); s.insert("amazing");
        s.insert("wonderful"); s.insert("incredible"); s.insert("fantastic"); s.insert("brilliant");
        s.insert("perfect"); s.insert("excited"); s.insert("thrilled"); s.insert("grateful");
        s.insert("warm"); s.insert("breakthrough"); s.insert("success"); s.insert("works");
        s.insert("working"); s.insert("solved"); s.insert("fixed"); s.insert("nailed");
        s.insert("heart"); s.insert("hug"); s.insert("precious"); s.insert("adore");
        s
    };

    static ref NEGATIVE_WORDS: HashSet<&'static str> = {
        let mut s = HashSet::new();
        s.insert("bug"); s.insert("error"); s.insert("crash"); s.insert("crashing");
        s.insert("crashed"); s.insert("fail"); s.insert("failed"); s.insert("failing");
        s.insert("failure"); s.insert("broken"); s.insert("broke"); s.insert("breaking");
        s.insert("breaks"); s.insert("issue"); s.insert("problem"); s.insert("wrong");
        s.insert("stuck"); s.insert("blocked"); s.insert("unable"); s.insert("impossible");
        s.insert("missing"); s.insert("terrible"); s.insert("horrible"); s.insert("awful");
        s.insert("worse"); s.insert("worst"); s.insert("panic"); s.insert("disaster");
        s.insert("mess");
        s
    };

    // Common technology tokens.  We use these to boost the `technologies` list.
    static ref TECH_TOKENS: HashSet<&'static str> = {
        let mut s = HashSet::new();
        #[rustfmt::skip]
        let tokens: &[&str] = &[
            "rust", "python", "javascript", "typescript", "java", "go", "golang", "ruby",
            "c++", "csharp", "c#", "haskell", "scala", "kotlin", "swift", "react", "vue",
            "angular", "nodejs", "node.js", "docker", "kubernetes", "postgres", "postgresql",
            "mongodb", "redis", "aws", "azure", "gcp", "linux", "windows", "macos", "tensorflow",
            "pytorch", "github", "gitlab", "llvm", "wasm", "webassembly", "terraform", "elasticsearch",
            "fastapi", "django", "flask", "rocket", "actix", "tokio", "nginx", "apache", "rabbitmq",
            "kafka", "sqlite", "graphql", "rest", "openapi", "grpc", "protobuf", "json", "yaml",
            "toml", "xml", "html", "css", "sass", "webpack", "vite", "jest", "mocha", "cypress",
            "pytest", "cargo", "npm", "yarn", "pnpm", "gradle", "maven", "cmake", "make", "bazel",
            "ninja", "jenkins", "circleci", "github actions", "gitlab ci", "travis", "coveralls",
            "codecov", "sentry", "datadog", "prometheus", "grafana", "opentelemetry", "jaeger", "zipkin",
        ];
        for t in tokens {
            s.insert(*t);
        }
        s
    };

    // Common words that should not be counted as people/organizations.
    static ref STOP_WORDS: HashSet<&'static str> = {
        let mut s = HashSet::new();
        #[rustfmt::skip]
        let words: &[&str] = &[
            "the", "and", "for", "are", "but", "not", "you", "all", "can", "had", "her", "was",
            "one", "our", "out", "day", "get", "has", "him", "his", "how", "its", "may", "new",
            "now", "old", "see", "two", "who", "boy", "did", "she", "use", "her", "way", "many",
            "oil", "sit", "set", "run", "eat", "far", "sea", "eye", "ask", "own", "say", "too",
            "any", "try", "three", "also", "back", "after", "first", "well", "water", "been",
            "call", "who", "now", "find", "long", "down", "day", "did", "get", "come", "made",
            "may", "part", "over", "such", "take", "than", "them", "well", "were",
        ];
        for w in words {
            s.insert(*w);
        }
        s
    };
}

/// A structured fact extracted from arbitrary text.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ExtractedFacts {
    pub urls: Vec<String>,
    pub emails: Vec<String>,
    pub dates: Vec<String>,
    pub phones: Vec<String>,
    pub ips: Vec<String>,
    pub key_values: Vec<(String, String)>,
    pub people: Vec<String>,
    pub organizations: Vec<String>,
    pub technologies: Vec<String>,
    pub projects: Vec<String>,
    pub memories: Vec<MemoryFact>,
    pub hashtags: Vec<String>,
    pub mentions: Vec<String>,
    pub code_refs: Vec<String>,
}

/// A heuristic memory snippet.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryFact {
    pub content: String,
    pub memory_type: MemoryType,
    pub confidence: f32,
    pub keywords: Vec<String>,
}

/// A named entity discovered in text.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entity {
    pub name: String,
    pub kind: String,
    pub context: String,
}

/// Extract all supported structured facts from `text`.
///
/// The returned [`ExtractedFacts`] contains unique entries for URLs, emails,
/// dates, key-value pairs, named entities, and heuristic memory snippets.
///
/// No external API calls are made.
pub fn extract_facts(text: &str) -> ExtractedFacts {
    let mut facts = ExtractedFacts::default();

    if text.trim().is_empty() {
        return facts;
    }

    facts.urls = unique_matches(&URL_RE, text);
    facts.emails = unique_matches(&EMAIL_RE, text);
    facts.dates = unique_matches(&DATE_RE, text);
    facts.phones = unique_matches(&PHONE_RE, text);
    facts.ips = unique_matches(&IP_RE, text);
    facts.hashtags = unique_matches(&HASHTAG_RE, text);
    facts.mentions = unique_matches(&MENTION_RE, text);
    facts.code_refs = extract_code_refs(text);

    facts.key_values = extract_key_values(text);
    facts.people = extract_people(text);
    facts.organizations = extract_organizations(text);
    facts.technologies = extract_technologies(text);
    facts.projects = extract_projects(text);
    facts.memories = extract_memories(text);

    facts
}

/// Extract named entities from `text`.
///
/// Returns people, organizations, technologies, and projects with surrounding
/// context for downstream indexing.
pub fn extract_entities(text: &str) -> Vec<Entity> {
    let mut entities = Vec::new();
    if text.trim().is_empty() {
        return entities;
    }

    let lines: Vec<&str> = text.lines().collect();

    for (idx, line) in lines.iter().enumerate() {
        let context = surrounding_context(&lines, idx);

        for m in PEOPLE_RE.find_iter(line) {
            let name = m.as_str().to_string();
            if !is_stop_name(&name) {
                entities.push(Entity {
                    name: name.clone(),
                    kind: "person".to_string(),
                    context: context.clone(),
                });
            }
        }

        for m in ORG_RE.find_iter(line) {
            let name = m.as_str().to_string();
            if !is_stop_name(&name) {
                entities.push(Entity {
                    name: name.clone(),
                    kind: "organization".to_string(),
                    context: context.clone(),
                });
            }
        }

        for m in PROJECT_RE.captures_iter(line) {
            if let Some(g) = m.get(1) {
                entities.push(Entity {
                    name: g.as_str().to_string(),
                    kind: "project".to_string(),
                    context: context.clone(),
                });
            }
        }

        for token in line.split(|c: char| !c.is_alphanumeric()) {
            let lower = token.to_lowercase();
            if TECH_TOKENS.contains(lower.as_str()) && token.len() > 1 {
                entities.push(Entity {
                    name: token.to_string(),
                    kind: "technology".to_string(),
                    context: context.clone(),
                });
            }
        }
    }

    // Deduplicate by (name, kind).
    let mut seen = HashSet::new();
    entities.retain(|e| seen.insert((e.name.clone(), e.kind.clone())));
    entities
}

/// Extract heuristic memory snippets from `text`.
///
/// Segments `text` by paragraph or speaker turn, scores each segment against the
/// five marker sets, and returns classified memories with confidence scores.
///
/// This is the direct Rust equivalent of upstream `extract_memories()`.
pub fn extract_memories(text: &str) -> Vec<MemoryFact> {
    let mut memories = Vec::new();
    if text.trim().is_empty() {
        return memories;
    }

    let segments = split_into_segments(text);
    for segment in segments {
        if segment.trim().len() < 20 {
            continue;
        }

        let prose = extract_prose(&segment);
        let mut scores: HashMap<MemoryType, f32> = HashMap::new();
        let mut matched_keywords: Vec<String> = Vec::new();

        let (score, mut words) = score_markers(&prose, &DECISION_MARKERS);
        if score > 0.0 {
            scores.insert(MemoryType::Decision, score);
        }
        matched_keywords.append(&mut words);

        let (score, mut words) = score_markers(&prose, &PREFERENCE_MARKERS);
        if score > 0.0 {
            scores.insert(MemoryType::Preference, score);
        }
        matched_keywords.append(&mut words);

        let (score, mut words) = score_markers(&prose, &MILESTONE_MARKERS);
        if score > 0.0 {
            scores.insert(MemoryType::Milestone, score);
        }
        matched_keywords.append(&mut words);

        let (score, mut words) = score_markers(&prose, &PROBLEM_MARKERS);
        if score > 0.0 {
            scores.insert(MemoryType::Problem, score);
        }
        matched_keywords.append(&mut words);

        let (score, mut words) = score_markers(&prose, &EMOTION_MARKERS);
        if score > 0.0 {
            scores.insert(MemoryType::Emotional, score);
        }
        matched_keywords.append(&mut words);

        if scores.is_empty() {
            continue;
        }

        let mut length_bonus = 0.0;
        if segment.len() > 500 {
            length_bonus = 2.0;
        } else if segment.len() > 200 {
            length_bonus = 1.0;
        }

        let (mut max_type, max_raw) = scores
            .iter()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
            .map(|(t, s)| (t.clone(), *s))
            .unwrap();

        max_type = disambiguate(max_type, &prose, &scores);
        let max_score = max_raw + length_bonus;
        let confidence = (max_score / 5.0).min(1.0);
        if confidence < 0.3 {
            continue;
        }

        matched_keywords.sort_unstable();
        matched_keywords.dedup();

        memories.push(MemoryFact {
            content: segment.trim().to_string(),
            memory_type: max_type,
            confidence,
            keywords: matched_keywords,
        });
    }

    memories
}

fn extract_key_values(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for caps in KV_RE.captures_iter(text) {
        let key = caps
            .get(1)
            .map(|m| m.as_str().trim().to_string())
            .unwrap_or_default();
        let value = caps
            .get(2)
            .map(|m| m.as_str().trim().to_string())
            .unwrap_or_default();
        if !key.is_empty() && !value.is_empty() {
            out.push((key, value));
        }
    }
    out
}

fn extract_people(text: &str) -> Vec<String> {
    let mut people = Vec::new();
    for m in PEOPLE_RE.find_iter(text) {
        let name = m.as_str().to_string();
        if !is_stop_name(&name) {
            people.push(name);
        }
    }
    people.sort_unstable();
    people.dedup();
    people
}

fn extract_organizations(text: &str) -> Vec<String> {
    let mut orgs = Vec::new();
    for m in ORG_RE.find_iter(text) {
        let name = m.as_str().to_string();
        if !is_stop_name(&name) {
            orgs.push(name);
        }
    }
    orgs.sort_unstable();
    orgs.dedup();
    orgs
}

fn extract_technologies(text: &str) -> Vec<String> {
    let mut techs = Vec::new();
    for token in text.split(|c: char| !c.is_alphanumeric()) {
        if token.len() < 2 {
            continue;
        }
        let lower = token.to_lowercase();
        if TECH_TOKENS.contains(lower.as_str()) {
            techs.push(token.to_string());
        }
    }
    techs.sort_unstable();
    techs.dedup();
    techs
}

fn extract_projects(text: &str) -> Vec<String> {
    let mut projects = Vec::new();
    for caps in PROJECT_RE.captures_iter(text) {
        if let Some(g) = caps.get(1) {
            projects.push(g.as_str().to_string());
        }
    }
    projects.sort_unstable();
    projects.dedup();
    projects
}

fn extract_code_refs(text: &str) -> Vec<String> {
    let mut refs = Vec::new();
    for m in CODE_REF_RE.find_iter(text) {
        let s = m.as_str().trim_matches('`');
        if !s.is_empty() {
            refs.push(s.to_string());
        }
    }
    refs.sort_unstable();
    refs.dedup();
    refs
}

fn unique_matches(re: &Regex, text: &str) -> Vec<String> {
    let mut items = Vec::new();
    let mut seen = HashSet::new();
    for m in re.find_iter(text) {
        let s = m.as_str().to_string();
        if seen.insert(s.clone()) {
            items.push(s);
        }
    }
    items
}

fn surrounding_context(lines: &[&str], idx: usize) -> String {
    let start = idx.saturating_sub(1);
    let end = (idx + 2).min(lines.len());
    lines[start..end].join(" ").trim().to_string()
}

fn is_stop_name(name: &str) -> bool {
    let lower = name.to_lowercase();
    if lower.len() < 3 {
        return true;
    }
    for word in lower.split_whitespace() {
        if STOP_WORDS.contains(word) {
            return true;
        }
    }
    false
}

fn split_into_segments(text: &str) -> Vec<String> {
    let lines: Vec<&str> = text.lines().collect();
    let turn_patterns = vec![
        Regex::new(r"^>\s").unwrap(),
        Regex::new(r"(?i)^(Human|User|Q)\s*:").unwrap(),
        Regex::new(r"(?i)^(Assistant|AI|A|Claude|ChatGPT)\s*:").unwrap(),
    ];

    let mut turn_count = 0;
    for line in &lines {
        let stripped = line.trim();
        for pat in &turn_patterns {
            if pat.is_match(stripped) {
                turn_count += 1;
                break;
            }
        }
    }

    if turn_count >= 3 {
        return split_by_turns(&lines, &turn_patterns);
    }

    let paragraphs: Vec<String> = text
        .split("\n\n")
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty())
        .collect();

    if paragraphs.len() <= 1 && lines.len() > 20 {
        let mut segments = Vec::new();
        for i in (0..lines.len()).step_by(25) {
            let end = (i + 25).min(lines.len());
            let group = lines[i..end].join("\n").trim().to_string();
            if !group.is_empty() {
                segments.push(group);
            }
        }
        return segments;
    }

    paragraphs
}

fn split_by_turns(lines: &[&str], turn_patterns: &[Regex]) -> Vec<String> {
    let mut segments = Vec::new();
    let mut current = Vec::new();

    for line in lines {
        let stripped = line.trim();
        let is_turn = turn_patterns.iter().any(|pat| pat.is_match(stripped));
        if is_turn && !current.is_empty() {
            segments.push(current.join("\n"));
            current = vec![line.to_string()];
        } else {
            current.push(line.to_string());
        }
    }

    if !current.is_empty() {
        segments.push(current.join("\n"));
    }

    segments
}

fn extract_prose(text: &str) -> String {
    let mut prose = Vec::new();
    let mut in_code = false;
    for line in text.lines() {
        if line.trim().starts_with("```") {
            in_code = !in_code;
            continue;
        }
        if in_code {
            continue;
        }
        if !is_code_line(line) {
            prose.push(line);
        }
    }
    if prose.is_empty() {
        text.to_string()
    } else {
        prose.join("\n").trim().to_string()
    }
}

fn is_code_line(line: &str) -> bool {
    let stripped = line.trim();
    if stripped.is_empty() {
        return false;
    }
    for pat in &*CODE_LINE_PATTERNS {
        if pat.is_match(stripped) {
            return true;
        }
    }
    let alpha_count = stripped.chars().filter(|c| c.is_alphabetic()).count();
    let alpha_ratio = alpha_count as f32 / stripped.len().max(1) as f32;
    alpha_ratio < 0.4 && stripped.len() > 10
}

fn score_markers(text: &str, markers: &[Regex]) -> (f32, Vec<String>) {
    let text_lower = text.to_lowercase();
    let mut score = 0.0;
    let mut matched = Vec::new();
    for marker in markers {
        let mut hits = Vec::new();
        for m in marker.find_iter(&text_lower) {
            hits.push(m.as_str().to_string());
        }
        if !hits.is_empty() {
            score += hits.len() as f32;
            matched.extend(hits);
        }
    }
    (score, matched)
}

fn get_sentiment_score(text: &str) -> f32 {
    let words: Vec<String> = text
        .split(|c: char| !c.is_alphanumeric())
        .map(|s| s.to_lowercase())
        .filter(|s| !s.is_empty())
        .collect();
    let mut pos = 0;
    let mut neg = 0;
    for word in &words {
        if POSITIVE_WORDS.contains(word.as_str()) {
            pos += 1;
        } else if NEGATIVE_WORDS.contains(word.as_str()) {
            neg += 1;
        }
    }
    if pos + neg == 0 {
        return 0.0;
    }
    (pos as f32 - neg as f32) / (pos + neg) as f32
}

fn has_resolution(text: &str) -> bool {
    let text_lower = text.to_lowercase();
    let patterns = vec![
        r"(?i)\bfixed\b",
        r"(?i)\bsolved\b",
        r"(?i)\bresolved\b",
        r"(?i)\bpatched\b",
        r"(?i)\bgot it working\b",
        r"(?i)\bit works\b",
        r"(?i)\bnailed it\b",
        r"(?i)\bfigured (it )?out\b",
        r"(?i)\bthe (fix|answer|solution)\b",
    ];
    for p in patterns {
        if Regex::new(p).unwrap().is_match(&text_lower) {
            return true;
        }
    }
    false
}

fn disambiguate(
    memory_type: MemoryType,
    text: &str,
    scores: &HashMap<MemoryType, f32>,
) -> MemoryType {
    let sentiment = get_sentiment_score(text);

    if memory_type == MemoryType::Problem && has_resolution(text) {
        if scores.get(&MemoryType::Emotional).unwrap_or(&0.0) > &0.0 && sentiment > 0.0 {
            return MemoryType::Emotional;
        }
        return MemoryType::Milestone;
    }

    if memory_type == MemoryType::Problem && sentiment > 0.0 {
        if scores.get(&MemoryType::Milestone).unwrap_or(&0.0) > &0.0 {
            return MemoryType::Milestone;
        }
        if scores.get(&MemoryType::Emotional).unwrap_or(&0.0) > &0.0 {
            return MemoryType::Emotional;
        }
    }

    memory_type
}

#[cfg(test)]
mod tests {
    use super::*;

    // -------------------------------------------------------------------------
    // Structured fact tests
    // -------------------------------------------------------------------------
    #[test]
    fn test_extract_empty() {
        let facts = extract_facts("");
        assert!(facts.urls.is_empty());
        assert!(facts.emails.is_empty());
        assert!(facts.memories.is_empty());
    }

    #[test]
    fn test_extract_url() {
        let text = "Check out https://example.com/path and http://test.org?q=1.";
        let facts = extract_facts(text);
        assert_eq!(
            facts.urls,
            vec!["https://example.com/path", "http://test.org?q=1"]
        );
    }

    #[test]
    fn test_extract_email() {
        let text = "Email me at alice@example.com or bob@company.co.uk.";
        let facts = extract_facts(text);
        assert_eq!(facts.emails, vec!["alice@example.com", "bob@company.co.uk"]);
    }

    #[test]
    fn test_extract_dates() {
        let text = "Meetings on 2024-06-23, 06/23/2024, and June 23, 2024.";
        let facts = extract_facts(text);
        assert_eq!(
            facts.dates,
            vec!["2024-06-23", "06/23/2024", "June 23, 2024"]
        );
    }

    #[test]
    fn test_extract_phones_and_ips() {
        let text = "Call +1 (555) 123-4567 or visit 192.168.1.1.";
        let facts = extract_facts(text);
        assert_eq!(facts.phones, vec!["+1 (555) 123-4567"]);
        assert_eq!(facts.ips, vec!["192.168.1.1"]);
    }

    #[test]
    fn test_extract_hashtags_mentions_code() {
        let text = "Thanks @alice for the #rust tips. Try `cargo build`.";
        let facts = extract_facts(text);
        assert_eq!(facts.mentions, vec!["@alice"]);
        assert_eq!(facts.hashtags, vec!["#rust"]);
        assert_eq!(facts.code_refs, vec!["cargo build"]);
    }

    #[test]
    fn test_extract_key_values() {
        let text = "Name: MemPalace\nVersion = 0.5.0\nEmpty value:\nKey: value";
        let facts = extract_facts(text);
        assert_eq!(
            facts.key_values,
            vec![
                ("Name".to_string(), "MemPalace".to_string()),
                ("Version".to_string(), "0.5.0".to_string()),
                ("Key".to_string(), "value".to_string()),
            ]
        );
    }

    #[test]
    fn test_extract_people() {
        let text = "Alice Smith and Bob Johnson are working on the project.";
        let facts = extract_facts(text);
        assert!(facts.people.contains(&"Alice Smith".to_string()));
        assert!(facts.people.contains(&"Bob Johnson".to_string()));
    }

    #[test]
    fn test_extract_organizations() {
        let text = "OpenAI Inc and the Rust Foundation are partners.";
        let facts = extract_facts(text);
        assert!(facts.organizations.contains(&"OpenAI Inc".to_string()));
        assert!(facts.organizations.contains(&"Rust Foundation".to_string()));
    }

    #[test]
    fn test_extract_technologies() {
        let text = "We use Rust, Tokio, Postgres, and Redis for the backend.";
        let facts = extract_facts(text);
        assert!(facts.technologies.contains(&"Rust".to_string()));
        assert!(facts.technologies.contains(&"Tokio".to_string()));
        assert!(facts.technologies.contains(&"Postgres".to_string()));
        assert!(facts.technologies.contains(&"Redis".to_string()));
    }

    #[test]
    fn test_extract_projects() {
        let text = "Project MemPalace and repo mempalace-rs are related.";
        let facts = extract_facts(text);
        assert!(facts.projects.contains(&"MemPalace".to_string()));
        assert!(facts.projects.contains(&"mempalace-rs".to_string()));
    }

    #[test]
    fn test_extract_entities_deduplicates() {
        let text = "Rust is great. Rust is fast. Project MemPalace uses Rust.";
        let entities = extract_entities(text);
        let techs: Vec<_> = entities.iter().filter(|e| e.kind == "technology").collect();
        assert_eq!(techs.len(), 1);
        assert_eq!(techs[0].name, "Rust");
    }

    #[test]
    fn test_extract_entities_has_context() {
        let text = "Alice Smith\nworks on Rust\nat the Foundation.";
        let entities = extract_entities(text);
        let person = entities.iter().find(|e| e.kind == "person").unwrap();
        assert!(person.context.contains("Alice Smith"));
        assert!(person.context.contains("Rust"));
    }

    // -------------------------------------------------------------------------
    // Memory extraction tests
    // -------------------------------------------------------------------------
    #[test]
    fn test_extract_decision() {
        let text = "We decided to go with Rust because of its memory safety.";
        let facts = extract_facts(text);
        assert_eq!(facts.memories.len(), 1);
        assert_eq!(facts.memories[0].memory_type, MemoryType::Decision);
        assert!(facts.memories[0].confidence >= 0.3);
    }

    #[test]
    fn test_extract_preference() {
        let text = "I prefer functional style over imperative.";
        let facts = extract_facts(text);
        assert_eq!(facts.memories[0].memory_type, MemoryType::Preference);
    }

    #[test]
    fn test_extract_milestone() {
        let text = "It finally works! This is a real breakthrough in compression.";
        let facts = extract_facts(text);
        assert_eq!(facts.memories[0].memory_type, MemoryType::Milestone);
    }

    #[test]
    fn test_extract_problem() {
        let text = "The bug is causing a crash in production.";
        let facts = extract_facts(text);
        assert_eq!(facts.memories[0].memory_type, MemoryType::Problem);
    }

    #[test]
    fn test_extract_emotional() {
        let text = "I feel so grateful for this amazing opportunity.";
        let facts = extract_facts(text);
        assert_eq!(facts.memories[0].memory_type, MemoryType::Emotional);
    }

    #[test]
    fn test_extract_memories_short_text_skipped() {
        let text = "I prefer Rust.";
        let memories = extract_memories(text);
        assert!(memories.is_empty());
    }

    #[test]
    fn test_disambiguate_problem_to_milestone() {
        let text = "We had a bug but we fixed it by patching the code.";
        let facts = extract_facts(text);
        assert_eq!(facts.memories[0].memory_type, MemoryType::Milestone);
    }

    #[test]
    fn test_disambiguate_problem_to_emotional() {
        let text = "I was so worried about the crash but I'm so happy we resolved it! Nailed it!";
        let facts = extract_facts(text);
        assert_eq!(facts.memories[0].memory_type, MemoryType::Emotional);
    }

    #[test]
    fn test_split_by_turns() {
        let text = "Human: hello, this is a longer greeting message\nAI: hi there, how can I help you today?\nUser: how are you doing on this fine day?\nClaude: I am fine, thank you for asking.";
        let segments = split_into_segments(text);
        assert_eq!(segments.len(), 4);
    }

    #[test]
    fn test_split_by_groups() {
        let lines: Vec<String> = (0..30).map(|i| format!("Line {}\n", i)).collect();
        let text = lines.join("");
        let memories = extract_memories(&text);
        assert!(memories.is_empty());
        let segments = split_into_segments(&text);
        assert!(segments.len() > 1);
    }

    #[test]
    fn test_extract_prose_with_code() {
        let text = "Prose before.\n```\nCode block\n```\nProse after.";
        let prose = extract_prose(text);
        assert!(prose.contains("Prose before"));
        assert!(prose.contains("Prose after"));
        assert!(!prose.contains("Code block"));
    }

    #[test]
    fn test_sentiment_score() {
        assert!(get_sentiment_score("I am happy and grateful") > 0.0);
        assert!(get_sentiment_score("There is a bug and a crash") < 0.0);
        assert_eq!(get_sentiment_score("The table is brown"), 0.0);
    }

    #[test]
    fn test_code_line_detection() {
        assert!(is_code_line("  import os"));
        assert!(is_code_line("  $ ls -la"));
        assert!(is_code_line("  const x = 1;"));
        assert!(!is_code_line("This is a normal sentence."));
    }

    #[test]
    fn test_has_resolution() {
        assert!(has_resolution("We fixed the bug."));
        assert!(has_resolution("It works now."));
        assert!(!has_resolution("It is broken."));
    }

    #[test]
    fn test_extract_facts_includes_memory_keywords() {
        let text = "We decided to use Rust because it is safe.";
        let facts = extract_facts(text);
        assert!(!facts.memories[0].keywords.is_empty());
    }

    #[test]
    fn test_deduplication_and_unique_matches() {
        let text = "https://a.com https://a.com https://b.com";
        let facts = extract_facts(text);
        assert_eq!(facts.urls, vec!["https://a.com", "https://b.com"]);
    }

    #[test]
    fn test_extract_entities_empty() {
        let entities = extract_entities("   ");
        assert!(entities.is_empty());
    }

    #[test]
    fn test_extract_memories_empty() {
        let memories = extract_memories("");
        assert!(memories.is_empty());
    }

    #[test]
    fn test_extract_facts_no_match() {
        let text = "The quick brown fox jumps over the lazy dog.";
        let facts = extract_facts(text);
        assert!(facts.memories.is_empty());
        assert!(facts.urls.is_empty());
        assert!(facts.emails.is_empty());
    }

    #[test]
    fn test_extract_memories_length_bonus() {
        let text = "I prefer functional style over imperative because it makes the code more readable and maintainable in the long run. ".repeat(10);
        let facts = extract_facts(&text);
        assert!(!facts.memories.is_empty());
        assert!(facts.memories[0].confidence > 0.6);
    }

    #[test]
    fn test_score_markers_counts_hits() {
        let text = "I love it. I love it. I love it.";
        let (score, hits) = score_markers(text, &EMOTION_MARKERS);
        assert!(score > 0.0);
        assert_eq!(hits.len(), 3);
    }

    #[test]
    fn test_key_value_edge_cases() {
        let text = "A = 1\nB: 2\nC:    spaced    \nD =";
        let kvs = extract_key_values(text);
        assert!(kvs.iter().any(|(k, v)| k == "A" && v == "1"));
        assert!(kvs.iter().any(|(k, v)| k == "B" && v == "2"));
        assert!(kvs.iter().any(|(k, v)| k == "C" && v == "spaced"));
        assert!(!kvs.iter().any(|(k, _)| k == "D"));
    }

    #[test]
    fn test_people_excludes_common_words() {
        let text = "The And For are not people. Alice Smith is a person.";
        let people = extract_people(text);
        assert!(!people
            .iter()
            .any(|p| p.to_lowercase().contains("the and for")));
        assert!(people.contains(&"Alice Smith".to_string()));
    }

    #[test]
    fn test_extract_entities_finds_person_organization_and_technology() {
        let text = "Alice Smith from the Rust Foundation works with Rust on Project MemPalace.";
        let entities = extract_entities(text);
        assert!(entities
            .iter()
            .any(|e| e.kind == "person" && e.name == "Alice Smith"));
        assert!(entities
            .iter()
            .any(|e| e.kind == "organization" && e.name.contains("Rust Foundation")));
        assert!(entities
            .iter()
            .any(|e| e.kind == "technology" && e.name == "Rust"));
        assert!(entities
            .iter()
            .any(|e| e.kind == "project" && e.name == "MemPalace"));
    }
}
