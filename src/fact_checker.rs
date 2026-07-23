//! Heuristic fact-checking against stored memories and the knowledge graph.
//!
//! This module performs purely offline verification of a claim against the
//! MemPalace palace: vector memories, the temporal knowledge graph, and the
//! entity registry. It returns a bounded score and a list of concrete issues.
//!
//! No LLM API calls are made; only the local embedding model, vector index,
//! SQLite knowledge graph, and JSON entity registry are used.

use std::collections::HashSet;

use anyhow::{anyhow, Result};
use chrono::Local;
use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::config::MempalaceConfig;
use crate::entity_registry::EntityRegistry;
use crate::knowledge_graph::KnowledgeGraph;
use crate::vector_storage::VectorStorage;

/// Direction of the fact-check outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FactCheckStatus {
    /// The stored evidence strongly supports the claim.
    Supported,
    /// The stored evidence contradicts the claim.
    Contradicted,
    /// No strong evidence either way.
    Neutral,
}

/// A single issue detected by the fact checker.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FactCheckIssue {
    /// Class of issue, e.g. `similar_name`, `relationship_mismatch`, `stale_fact`.
    pub issue_type: String,
    /// Human-readable explanation.
    pub detail: String,
    /// Severity in the range 0.0–1.0; higher means more serious.
    pub severity: f64,
}

/// A piece of evidence used by the fact checker.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FactCheckEvidence {
    /// Source of the evidence: `memory`, `knowledge_graph`, or `entity_registry`.
    pub source: String,
    /// Text or fact that was matched.
    pub matched: String,
    /// Similarity or confidence score (0.0–1.0) when applicable.
    pub score: f64,
}

/// Result of checking a claim against the palace.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FactCheckResult {
    /// Overall score in [-1.0, 1.0]. Positive means supported, negative
    /// means contradicted, near zero means neutral.
    pub score: f64,
    /// Qualitative classification derived from `score`.
    pub status: FactCheckStatus,
    /// Concrete issues found, if any.
    pub issues: Vec<FactCheckIssue>,
    /// Supporting or contradicting evidence.
    pub evidence: Vec<FactCheckEvidence>,
}

impl FactCheckResult {
    fn new(score: f64) -> Self {
        let status = classify_score(score);
        Self {
            score,
            status,
            issues: Vec::new(),
            evidence: Vec::new(),
        }
    }

    fn add_issue(&mut self, issue: FactCheckIssue) {
        self.score = (self.score - issue.severity).clamp(-1.0, 1.0);
        self.issues.push(issue);
        self.status = classify_score(self.score);
    }

    fn add_evidence(&mut self, evidence: FactCheckEvidence) {
        self.evidence.push(evidence);
    }
}

fn classify_score(score: f64) -> FactCheckStatus {
    if score > 0.2 {
        FactCheckStatus::Supported
    } else if score < -0.2 {
        FactCheckStatus::Contradicted
    } else {
        FactCheckStatus::Neutral
    }
}

/// A relationship claim extracted from natural-language text.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Claim {
    pub subject: String,
    pub predicate: String,
    pub object: String,
    /// Original text span that produced the claim.
    pub span: String,
}

/// Heuristic fact-check entry point.
///
/// Checks the claim against:
///   * vector memories (semantic support)
///   * knowledge graph (relationship mismatches, stale facts)
///   * entity registry (similar-name confusion)
///
/// The vector storage and knowledge graph are optional; if they are `None`,
/// those checks are skipped gracefully.
pub fn check_fact(
    claim: &str,
    vector_storage: Option<&VectorStorage>,
    knowledge_graph: Option<&KnowledgeGraph>,
    config: &MempalaceConfig,
) -> FactCheckResult {
    if claim.trim().is_empty() {
        return FactCheckResult::new(0.0);
    }

    let mut result = FactCheckResult::new(0.0);

    // 1. Memory-based semantic support.
    if let Some(vs) = vector_storage {
        if let Ok(mem_result) = check_fact_against_memories(claim, vs, 5) {
            result.score = (result.score + mem_result.score).clamp(-1.0, 1.0);
            result.issues.extend(mem_result.issues);
            result.evidence.extend(mem_result.evidence);
        }
    }

    // 2. Entity-name confusion.
    if let Ok(issues) = check_entity_confusion(claim, config) {
        for issue in issues {
            result.add_issue(issue);
        }
    }

    // 3. Knowledge-graph contradictions.
    if let Some(kg) = knowledge_graph {
        if let Ok(kg_result) = check_kg_contradictions(claim, kg) {
            result.score = (result.score + kg_result.score).clamp(-1.0, 1.0);
            result.issues.extend(kg_result.issues);
            result.evidence.extend(kg_result.evidence);
        }
    }

    result.status = classify_score(result.score);
    result
}

/// Check a claim against vector memories only, returning a bounded score.
///
/// The score is computed from the highest semantic similarity among the top
/// `limit` returned memories. Memories with `valid_to` in the past lower the
/// score because they are stale.
pub fn check_fact_against_memories(
    claim: &str,
    vector_storage: &VectorStorage,
    limit: usize,
) -> Result<FactCheckResult> {
    let limit = limit.clamp(1, 20);
    let records = vector_storage.search(claim, limit)?;

    if records.is_empty() {
        return Ok(FactCheckResult::new(0.0));
    }

    let now = Local::now().format("%Y-%m-%d").to_string();
    let now_epoch = chrono::Utc::now().timestamp();

    let mut best_score = 0.0f64;
    let mut result = FactCheckResult::new(0.0);

    for rec in &records {
        let mut similarity = rec.score as f64;

        // Penalise memories that are explicitly expired.
        if let Some(valid_to) = rec.valid_to {
            if valid_to < now_epoch {
                similarity *= 0.5;
                result.add_issue(FactCheckIssue {
                    issue_type: "stale_memory".to_string(),
                    detail: format!(
                        "Memory '{}' may be stale (valid_to {} < current date {})",
                        rec.text_content,
                        format_epoch(valid_to),
                        now
                    ),
                    severity: 0.15,
                });
            }
        }

        if similarity > best_score {
            best_score = similarity;
        }

        result.add_evidence(FactCheckEvidence {
            source: "memory".to_string(),
            matched: rec.text_content.clone(),
            score: similarity,
        });
    }

    // Scale cosine similarity (already ~[0,1]) into a modest support signal.
    result.score = (best_score * 0.6).clamp(-1.0, 1.0);
    result.status = classify_score(result.score);
    Ok(result)
}

fn format_epoch(ts: i64) -> String {
    chrono::DateTime::from_timestamp(ts, 0)
        .map(|dt| dt.format("%Y-%m-%d").to_string())
        .unwrap_or_else(|| ts.to_string())
}

/// Check for entity names mentioned in the text that are a small edit distance
/// away from a *different* registered name. This catches typos and mix-ups.
pub fn check_entity_confusion(
    claim: &str,
    config: &MempalaceConfig,
) -> Result<Vec<FactCheckIssue>> {
    let registry_path = config.config_dir.join("entity_registry.json");
    let registry = EntityRegistry::new(Some(registry_path))?;
    let all_names = collect_registry_names(&registry);
    if all_names.is_empty() {
        return Ok(Vec::new());
    }

    let claim_lower = claim.to_lowercase();
    let mut mentioned: Vec<String> = Vec::new();
    for name in &all_names {
        if word_boundary_contains(&claim_lower, &name.to_lowercase()) {
            mentioned.push(name.clone());
        }
    }
    if mentioned.is_empty() {
        return Ok(Vec::new());
    }

    let mut issues = Vec::new();
    let mut seen_pairs: HashSet<(String, String)> = HashSet::new();

    for name_a in &mentioned {
        let a_lower = name_a.to_lowercase();
        for name_b in &all_names {
            if name_a == name_b {
                continue;
            }
            let b_lower = name_b.to_lowercase();
            let pair_key = ordered_pair(&a_lower, &b_lower);
            if seen_pairs.contains(&pair_key) {
                continue;
            }

            // Only flag when the similar name is a *different* registered entry
            // that was NOT also mentioned.
            if mentioned.iter().any(|m| m.to_lowercase() == b_lower) {
                seen_pairs.insert(pair_key);
                continue;
            }

            let distance = edit_distance(&a_lower, &b_lower);
            if (1..=2).contains(&distance) {
                issues.push(FactCheckIssue {
                    issue_type: "similar_name".to_string(),
                    detail: format!(
                        "'{}' mentioned — did you mean '{}'? (edit distance {})",
                        name_a, name_b, distance
                    ),
                    severity: 0.3 + (0.1 * (2 - distance) as f64),
                });
                seen_pairs.insert(pair_key);
            }
        }
    }

    Ok(issues)
}

fn collect_registry_names(registry: &EntityRegistry) -> Vec<String> {
    let mut names: HashSet<String> = HashSet::new();
    for (canonical, info) in &registry.data.people {
        names.insert(canonical.clone());
        for alias in &info.aliases {
            if !alias.is_empty() {
                names.insert(alias.clone());
            }
        }
    }
    for project in &registry.data.projects {
        if !project.is_empty() {
            names.insert(project.clone());
        }
    }
    names.into_iter().collect()
}

fn word_boundary_contains(text: &str, word: &str) -> bool {
    // Simple word-boundary check: the word must be surrounded by non-word chars
    // or string boundaries.
    let escaped = regex::escape(word);
    let pattern = format!(r"(?i)\b{}\b", escaped);
    Regex::new(&pattern)
        .map(|re| re.is_match(text))
        .unwrap_or(false)
}

fn ordered_pair(a: &str, b: &str) -> (String, String) {
    if a <= b {
        (a.to_string(), b.to_string())
    } else {
        (b.to_string(), a.to_string())
    }
}

/// Compute the Levenshtein edit distance between two strings.
#[allow(clippy::needless_range_loop)]
pub fn edit_distance(a: &str, b: &str) -> usize {
    let a_chars: Vec<char> = a.chars().collect();
    let b_chars: Vec<char> = b.chars().collect();
    let n = a_chars.len();
    let m = b_chars.len();

    if n == 0 {
        return m;
    }
    if m == 0 {
        return n;
    }

    let mut prev = vec![0usize; m + 1];
    let mut curr = vec![0usize; m + 1];

    for j in 0..=m {
        prev[j] = j;
    }

    for i in 1..=n {
        curr[0] = i;
        for j in 1..=m {
            let cost = if a_chars[i - 1] == b_chars[j - 1] {
                0
            } else {
                1
            };
            curr[j] = (prev[j] + 1).min(curr[j - 1] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }

    prev[m]
}

/// Extract relationship claims from text of the forms:
///   "X is Y's Z" and "X's Z is Y".
///
/// Both resolve to the triple `(X, Z, Y)` meaning "X has role Z wrt Y".
pub fn extract_claims(text: &str) -> Vec<Claim> {
    let mut claims = Vec::new();

    let pattern1 = Regex::new(r"\b([A-Z][\w-]+)\s+is\s+([A-Z][\w-]+)'s\s+([a-z]{3,20})\b").unwrap();
    let pattern2 = Regex::new(r"\b([A-Z][\w-]+)'s\s+([a-z]{3,20})\s+is\s+([A-Z][\w-]+)\b").unwrap();

    for m in pattern1.captures_iter(text) {
        let subject = m.get(1).map(|x| x.as_str()).unwrap_or("").to_string();
        let object = m.get(2).map(|x| x.as_str()).unwrap_or("").to_string();
        let predicate = m
            .get(3)
            .map(|x| x.as_str().to_lowercase())
            .unwrap_or_default();
        let span = m.get(0).map(|x| x.as_str()).unwrap_or("").to_string();
        if !subject.is_empty() && !object.is_empty() && !predicate.is_empty() {
            claims.push(Claim {
                subject,
                predicate,
                object,
                span,
            });
        }
    }

    for m in pattern2.captures_iter(text) {
        let object = m.get(1).map(|x| x.as_str()).unwrap_or("").to_string();
        let predicate = m
            .get(2)
            .map(|x| x.as_str().to_lowercase())
            .unwrap_or_default();
        let subject = m.get(3).map(|x| x.as_str()).unwrap_or("").to_string();
        let span = m.get(0).map(|x| x.as_str()).unwrap_or("").to_string();
        if !subject.is_empty() && !object.is_empty() && !predicate.is_empty() {
            claims.push(Claim {
                subject,
                predicate,
                object,
                span,
            });
        }
    }

    claims
}

/// Check text claims against the temporal knowledge graph.
///
/// Detects:
///   * `relationship_mismatch` — KG has the same subject/object but a different predicate.
///   * `stale_fact` — KG has the exact triple but its `valid_to` is in the past.
pub fn check_kg_contradictions(text: &str, kg: &KnowledgeGraph) -> Result<FactCheckResult> {
    let claims = extract_claims(text);
    if claims.is_empty() {
        return Ok(FactCheckResult::new(0.0));
    }

    let mut result = FactCheckResult::new(0.0);
    let today = Local::now().format("%Y-%m-%d").to_string();

    for claim in &claims {
        let kg_outgoing = kg.query_entity(&claim.subject, None, "outgoing")?;
        let kg_incoming = kg.query_entity(&claim.subject, None, "incoming")?;
        let mut facts = kg_outgoing;
        facts.extend(kg_incoming);

        let mut found_exact = false;
        let mut found_mismatch = false;
        let mut found_stale = false;

        for fact in facts {
            let fact_pred = fact["predicate"].as_str().unwrap_or("");
            let fact_obj = fact["object"].as_str().unwrap_or("");
            let fact_sub = fact["subject"].as_str().unwrap_or("");
            let valid_to = fact["valid_to"].as_str();
            let is_current = fact["current"].as_bool().unwrap_or(true);

            let object_match = fact_obj.eq_ignore_ascii_case(&claim.object)
                || fact_sub.eq_ignore_ascii_case(&claim.object);
            if !object_match {
                continue;
            }

            let predicate_match = fact_pred.eq_ignore_ascii_case(&claim.predicate);

            if predicate_match {
                found_exact = true;
                if !is_current || is_past(valid_to, &today) {
                    found_stale = true;
                    result.add_issue(FactCheckIssue {
                        issue_type: "stale_fact".to_string(),
                        detail: format!(
                            "'{}' is no longer current; KG records '{}' as ended {}.",
                            claim.span,
                            claim.predicate,
                            valid_to.unwrap_or("?")
                        ),
                        severity: 0.5,
                    });
                } else {
                    result.add_evidence(FactCheckEvidence {
                        source: "knowledge_graph".to_string(),
                        matched: format!("{} {} {}", fact_sub, fact_pred, fact_obj),
                        score: 0.8,
                    });
                }
            } else {
                found_mismatch = true;
                result.add_issue(FactCheckIssue {
                    issue_type: "relationship_mismatch".to_string(),
                    detail: format!(
                        "'{}' conflicts with KG: {} is recorded as '{}' of {}, not '{}'.",
                        claim.span, fact_sub, fact_pred, fact_obj, claim.predicate
                    ),
                    severity: 0.6,
                });
            }
        }

        if !found_exact && !found_mismatch && !found_stale {
            // No evidence for the specific claim in the KG is neutral.
        }

        // If we found a mismatch, subtract from the score; if exact and current, add.
        if found_mismatch {
            result.score -= 0.5;
        } else if found_exact && !found_stale {
            result.score += 0.4;
        }
    }

    result.score = result.score.clamp(-1.0, 1.0);
    result.status = classify_score(result.score);
    Ok(result)
}

fn is_past(valid_to: Option<&str>, today: &str) -> bool {
    match valid_to {
        Some(date) => date < today,
        None => false,
    }
}

/// Check a single claim against a knowledge graph. Convenience wrapper that
/// extracts claims and then calls `check_kg_contradictions`.
pub fn check_fact_against_kg(claim: &str, kg: &KnowledgeGraph) -> Result<FactCheckResult> {
    check_kg_contradictions(claim, kg)
}

/// Validate a single claim and return its status, throwing an error only when
/// inputs are malformed.
pub fn validate_claim(claim: &str, config: &MempalaceConfig) -> Result<FactCheckResult> {
    if claim.trim().is_empty() {
        return Err(anyhow!("Empty claim"));
    }
    Ok(check_fact(claim, None, None, config))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;

    use crate::config::MempalaceConfig;
    use crate::knowledge_graph::KnowledgeGraph;

    fn make_config() -> (MempalaceConfig, tempfile::TempDir) {
        let temp_dir = tempdir().unwrap();
        let config = MempalaceConfig::new(Some(temp_dir.path().to_path_buf()));
        (config, temp_dir)
    }

    #[test]
    fn test_empty_claim_is_neutral() {
        let (config, _td) = make_config();
        let result = check_fact("", None, None, &config);
        assert_eq!(result.status, FactCheckStatus::Neutral);
        assert_eq!(result.score, 0.0);
        assert!(result.issues.is_empty());
    }

    #[test]
    fn test_whitespace_claim_is_neutral() {
        let (config, _td) = make_config();
        let result = check_fact("   \n\t  ", None, None, &config);
        assert_eq!(result.status, FactCheckStatus::Neutral);
    }

    #[test]
    fn test_validate_claim_empty() {
        let (config, _td) = make_config();
        assert!(validate_claim("", &config).is_err());
    }

    #[test]
    fn test_validate_claim_non_empty() {
        let (config, _td) = make_config();
        let result = validate_claim("Alice is Bob's friend", &config).unwrap();
        assert_eq!(result.status, FactCheckStatus::Neutral);
    }

    #[test]
    fn test_edit_distance() {
        assert_eq!(edit_distance("", "abc"), 3);
        assert_eq!(edit_distance("abc", ""), 3);
        assert_eq!(edit_distance("abc", "abc"), 0);
        assert_eq!(edit_distance("kitten", "sitting"), 3);
        assert_eq!(edit_distance("alice", "alise"), 1);
        assert_eq!(edit_distance("bob", "rob"), 1);
        assert_eq!(edit_distance("alice", "alicia"), 2);
    }

    #[test]
    fn test_extract_claims_both_patterns() {
        let text = "Bob is Alice's brother. Alice's husband is Bob.";
        let claims = extract_claims(text);
        assert_eq!(claims.len(), 2);
        assert_eq!(claims[0].subject, "Bob");
        assert_eq!(claims[0].predicate, "brother");
        assert_eq!(claims[0].object, "Alice");
        assert_eq!(claims[1].subject, "Bob");
        assert_eq!(claims[1].predicate, "husband");
        assert_eq!(claims[1].object, "Alice");
    }

    #[test]
    fn test_extract_claims_no_match() {
        let claims = extract_claims("There is no relationship here.");
        assert!(claims.is_empty());
    }

    #[test]
    fn test_extract_claims_predicate_case() {
        let claims = extract_claims("Eve is Mallory's collaborator");
        assert_eq!(claims.len(), 1);
        assert_eq!(claims[0].predicate, "collaborator");
    }

    #[test]
    fn test_word_boundary_contains() {
        assert!(word_boundary_contains("Alice went to the market", "alice"));
        assert!(word_boundary_contains("Bob's brother", "bob"));
        assert!(!word_boundary_contains("malice", "alice"));
        assert!(!word_boundary_contains("Bobby", "bob"));
    }

    #[test]
    fn test_ordered_pair() {
        assert_eq!(
            ordered_pair("bob", "alice"),
            ("alice".to_string(), "bob".to_string())
        );
        assert_eq!(
            ordered_pair("alice", "alice"),
            ("alice".to_string(), "alice".to_string())
        );
    }

    #[test]
    fn test_entity_confusion_empty_registry() {
        let (config, _td) = make_config();
        let issues = check_entity_confusion("Alice is Bob's friend", &config).unwrap();
        assert!(issues.is_empty());
    }

    #[test]
    fn test_entity_confusion_typos_detected() {
        let (config, _td) = make_config();
        let registry_path = config.config_dir.join("entity_registry.json");
        std::fs::create_dir_all(&config.config_dir).unwrap();
        let data = serde_json::json!({
            "version": 1,
            "mode": "personal",
            "people": {
                "Alice": {
                    "source": "test",
                    "contexts": [],
                    "aliases": [],
                    "relationship": "",
                    "confidence": 1.0,
                    "canonical": null
                },
                "Alicia": {
                    "source": "test",
                    "contexts": [],
                    "aliases": [],
                    "relationship": "",
                    "confidence": 1.0,
                    "canonical": null
                }
            },
            "projects": [],
            "ambiguous_flags": [],
            "wiki_cache": {}
        });
        let mut file = std::fs::File::create(&registry_path).unwrap();
        file.write_all(data.to_string().as_bytes()).unwrap();

        let issues = check_entity_confusion("Alice went to Paris", &config).unwrap();
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].issue_type, "similar_name");
        assert!(issues[0].detail.contains("Alicia"));
    }

    #[test]
    fn test_entity_confusion_no_false_positive_when_both_mentioned() {
        let (config, _td) = make_config();
        let registry_path = config.config_dir.join("entity_registry.json");
        std::fs::create_dir_all(&config.config_dir).unwrap();
        let data = serde_json::json!({
            "version": 1,
            "mode": "personal",
            "people": {
                "Alice": {
                    "source": "test",
                    "contexts": [],
                    "aliases": [],
                    "relationship": "",
                    "confidence": 1.0,
                    "canonical": null
                },
                "Alicia": {
                    "source": "test",
                    "contexts": [],
                    "aliases": [],
                    "relationship": "",
                    "confidence": 1.0,
                    "canonical": null
                }
            },
            "projects": [],
            "ambiguous_flags": [],
            "wiki_cache": {}
        });
        let mut file = std::fs::File::create(&registry_path).unwrap();
        file.write_all(data.to_string().as_bytes()).unwrap();

        let issues =
            check_entity_confusion("Alice and Alicia are different people", &config).unwrap();
        assert!(issues.is_empty());
    }

    #[test]
    fn test_entity_confusion_aliases_included() {
        let (config, _td) = make_config();
        let registry_path = config.config_dir.join("entity_registry.json");
        std::fs::create_dir_all(&config.config_dir).unwrap();
        let data = serde_json::json!({
            "version": 1,
            "mode": "personal",
            "people": {
                "Alice": {
                    "source": "test",
                    "contexts": [],
                    "aliases": ["Allie"],
                    "relationship": "",
                    "confidence": 1.0,
                    "canonical": null
                }
            },
            "projects": ["MemPalace"],
            "ambiguous_flags": [],
            "wiki_cache": {}
        });
        let mut file = std::fs::File::create(&registry_path).unwrap();
        file.write_all(data.to_string().as_bytes()).unwrap();

        let issues = check_entity_confusion("Allie works on MemPalace", &config).unwrap();
        // Allie is close to the registered canonical name Alice, which is not mentioned.
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].issue_type, "similar_name");
        assert!(issues[0].detail.contains("Alice"));
    }

    #[test]
    fn test_kg_contradiction_relationship_mismatch() {
        let kg = KnowledgeGraph::new(":memory:").unwrap();
        kg.add_triple("Bob", "husband", "Alice", None, None, 1.0, None, None)
            .unwrap();

        let result = check_kg_contradictions("Bob is Alice's brother", &kg).unwrap();
        assert_eq!(result.status, FactCheckStatus::Contradicted);
        assert_eq!(result.issues.len(), 1);
        assert_eq!(result.issues[0].issue_type, "relationship_mismatch");
    }

    #[test]
    fn test_kg_contradiction_stale_fact() {
        let kg = KnowledgeGraph::new(":memory:").unwrap();
        kg.add_triple(
            "Bob",
            "brother",
            "Alice",
            Some("2020-01-01"),
            Some("2021-01-01"),
            1.0,
            None,
            None,
        )
        .unwrap();

        let result = check_kg_contradictions("Bob is Alice's brother", &kg).unwrap();
        assert_eq!(result.status, FactCheckStatus::Contradicted);
        assert_eq!(result.issues.len(), 1);
        assert_eq!(result.issues[0].issue_type, "stale_fact");
    }

    #[test]
    fn test_kg_contradiction_supported_fact() {
        let kg = KnowledgeGraph::new(":memory:").unwrap();
        kg.add_triple("Bob", "brother", "Alice", None, None, 1.0, None, None)
            .unwrap();

        let result = check_kg_contradictions("Bob is Alice's brother", &kg).unwrap();
        assert_eq!(result.status, FactCheckStatus::Supported);
        assert_eq!(result.issues.len(), 0);
        assert_eq!(result.evidence.len(), 1);
    }

    #[test]
    fn test_kg_contradiction_no_claims() {
        let kg = KnowledgeGraph::new(":memory:").unwrap();
        let result = check_kg_contradictions("The weather is nice today", &kg).unwrap();
        assert_eq!(result.status, FactCheckStatus::Neutral);
        assert!(result.issues.is_empty());
    }

    #[test]
    fn test_check_fact_against_kg_wrapper() {
        let kg = KnowledgeGraph::new(":memory:").unwrap();
        kg.add_triple("Bob", "brother", "Alice", None, None, 1.0, None, None)
            .unwrap();
        let result = check_fact_against_kg("Bob is Alice's brother", &kg).unwrap();
        assert_eq!(result.status, FactCheckStatus::Supported);
    }

    #[test]
    fn test_classify_score_boundaries() {
        assert_eq!(classify_score(0.25), FactCheckStatus::Supported);
        assert_eq!(classify_score(-0.25), FactCheckStatus::Contradicted);
        assert_eq!(classify_score(0.0), FactCheckStatus::Neutral);
        assert_eq!(classify_score(0.2), FactCheckStatus::Neutral);
        assert_eq!(classify_score(-0.2), FactCheckStatus::Neutral);
    }

    #[test]
    fn test_collect_registry_names() {
        let (config, _td) = make_config();
        let registry_path = config.config_dir.join("entity_registry.json");
        std::fs::create_dir_all(&config.config_dir).unwrap();
        let data = serde_json::json!({
            "version": 1,
            "mode": "personal",
            "people": {
                "Alice": {
                    "source": "test",
                    "contexts": [],
                    "aliases": ["Allie"],
                    "relationship": "",
                    "confidence": 1.0,
                    "canonical": null
                }
            },
            "projects": ["MemPalace"],
            "ambiguous_flags": [],
            "wiki_cache": {}
        });
        let mut file = std::fs::File::create(&registry_path).unwrap();
        file.write_all(data.to_string().as_bytes()).unwrap();

        let registry = EntityRegistry::new(Some(registry_path)).unwrap();
        let names = collect_registry_names(&registry);
        assert!(names.contains(&"Alice".to_string()));
        assert!(names.contains(&"Allie".to_string()));
        assert!(names.contains(&"MemPalace".to_string()));
    }

    #[test]
    fn test_is_past() {
        assert!(is_past(Some("2020-01-01"), "2024-01-01"));
        assert!(!is_past(Some("2099-01-01"), "2024-01-01"));
        assert!(!is_past(None, "2024-01-01"));
    }

    #[test]
    fn test_fact_check_result_add_issue_updates_score() {
        let mut result = FactCheckResult::new(0.5);
        result.add_issue(FactCheckIssue {
            issue_type: "test".to_string(),
            detail: "test".to_string(),
            severity: 0.4,
        });
        assert!((result.score - 0.1).abs() < 0.001);
        assert_eq!(result.status, FactCheckStatus::Neutral);
    }

    #[test]
    fn test_fact_check_result_add_issue_clamps() {
        let mut result = FactCheckResult::new(-0.9);
        result.add_issue(FactCheckIssue {
            issue_type: "test".to_string(),
            detail: "test".to_string(),
            severity: 0.5,
        });
        assert_eq!(result.score, -1.0);
        assert_eq!(result.status, FactCheckStatus::Contradicted);
    }

    #[test]
    fn test_fact_check_result_new_negative() {
        let result = FactCheckResult::new(-0.3);
        assert_eq!(result.status, FactCheckStatus::Contradicted);
    }

    // Allow unused `td` in tests — it holds the temp directory alive.
    #[allow(dead_code)]
    struct _TempDirGuard(tempfile::TempDir);
}
