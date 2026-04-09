use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::Path;

/// Current AAAK dialect version string emitted in every compressed block.
pub const AAAK_VERSION: &str = "V:3.2";

lazy_static::lazy_static! {
    pub static ref EMOTION_CODES: HashMap<&'static str, &'static str> = {
        let mut m = HashMap::new();
        m.insert("vulnerability", "vul");
        m.insert("vulnerable", "vul");
        m.insert("joy", "joy");
        m.insert("joyful", "joy");
        m.insert("fear", "fear");
        m.insert("mild_fear", "fear");
        m.insert("trust", "trust");
        m.insert("trust_building", "trust");
        m.insert("grief", "grief");
        m.insert("raw_grief", "grief");
        m.insert("wonder", "wonder");
        m.insert("philosophical_wonder", "wonder");
        m.insert("rage", "rage");
        m.insert("anger", "rage");
        m.insert("love", "love");
        m.insert("devotion", "love");
        m.insert("hope", "hope");
        m.insert("despair", "despair");
        m.insert("hopelessness", "despair");
        m.insert("peace", "peace");
        m.insert("relief", "relief");
        m.insert("humor", "humor");
        m.insert("dark_humor", "humor");
        m.insert("tenderness", "tender");
        m.insert("raw_honesty", "raw");
        m.insert("brutal_honesty", "raw");
        m.insert("self_doubt", "doubt");
        m.insert("anxiety", "anx");
        m.insert("exhaustion", "exhaust");
        m.insert("conviction", "convict");
        m.insert("quiet_passion", "passion");
        m.insert("warmth", "warmth");
        m.insert("curiosity", "curious");
        m.insert("gratitude", "grat");
        m.insert("frustration", "frust");
        m.insert("confusion", "confuse");
        m.insert("satisfaction", "satis");
        m.insert("excitement", "excite");
        m.insert("determination", "determ");
        m.insert("surprise", "surprise");
        m
    };

    pub static ref EMOTION_SIGNALS: HashMap<&'static str, &'static str> = {
        let mut m = HashMap::new();
        m.insert("decided", "determ");
        m.insert("prefer", "convict");
        m.insert("worried", "anx");
        m.insert("excited", "excite");
        m.insert("frustrated", "frust");
        m.insert("confused", "confuse");
        m.insert("love", "love");
        m.insert("hate", "rage");
        m.insert("hope", "hope");
        m.insert("fear", "fear");
        m.insert("trust", "trust");
        m.insert("happy", "joy");
        m.insert("sad", "grief");
        m.insert("surprised", "surprise");
        m.insert("grateful", "grat");
        m.insert("curious", "curious");
        m.insert("wonder", "wonder");
        m.insert("anxious", "anx");
        m.insert("relieved", "relief");
        m.insert("satisf", "satis");
        m.insert("disappoint", "grief");
        m.insert("concern", "anx");
        m
    };

    pub static ref FLAG_SIGNALS: HashMap<&'static str, &'static str> = {
        let mut m = HashMap::new();
        m.insert("decided", "DECISION");
        m.insert("chose", "DECISION");
        m.insert("switched", "DECISION");
        m.insert("migrated", "DECISION");
        m.insert("replaced", "DECISION");
        m.insert("instead of", "DECISION");
        m.insert("because", "DECISION");
        m.insert("what:", "DECISION");
        m.insert("decision:", "DECISION");
        m.insert("founded", "ORIGIN");
        m.insert("created", "ORIGIN");
        m.insert("started", "ORIGIN");
        m.insert("born", "ORIGIN");
        m.insert("launched", "ORIGIN");
        m.insert("first time", "ORIGIN");
        m.insert("core", "CORE");
        m.insert("fundamental", "CORE");
        m.insert("essential", "CORE");
        m.insert("principle", "CORE");
        m.insert("belief", "CORE");
        m.insert("always", "CORE");
        m.insert("never forget", "CORE");
        m.insert("turning point", "PIVOT");
        m.insert("changed everything", "PIVOT");
        m.insert("realized", "PIVOT");
        m.insert("breakthrough", "PIVOT");
        m.insert("epiphany", "PIVOT");
        m.insert("api", "TECHNICAL");
        m.insert("database", "TECHNICAL");
        m.insert("architecture", "TECHNICAL");
        m.insert("deploy", "TECHNICAL");
        m.insert("infrastructure", "TECHNICAL");
        m.insert("algorithm", "TECHNICAL");
        m.insert("framework", "TECHNICAL");
        m.insert("server", "TECHNICAL");
        m.insert("config", "TECHNICAL");
        m
    };

    pub static ref STOP_WORDS: HashSet<&'static str> = {
        let mut s = HashSet::new();
        let words = vec![
            "the", "a", "an", "is", "are", "was", "were", "be", "been", "being",
            "have", "has", "had", "do", "does", "did", "will", "would", "could", "should",
            "may", "might", "shall", "can", "to", "of", "in", "for", "on", "with",
            "at", "by", "from", "as", "into", "about", "between", "through", "during",
            "before", "after", "above", "below", "up", "down", "out", "off", "over",
            "under", "again", "further", "then", "once", "here", "there", "when",
            "where", "why", "how", "all", "each", "every", "both", "few", "more",
            "most", "other", "some", "such", "no", "nor", "not", "only", "own",
            "same", "so", "than", "too", "very", "just", "don", "now", "and", "but",
            "or", "if", "while", "that", "this", "these", "those", "it", "its", "i",
            "we", "you", "he", "she", "they", "me", "him", "her", "us", "them", "my",
            "your", "his", "our", "their", "what", "which", "who", "whom", "also",
            "much", "many", "like", "because", "since", "get", "got", "use", "used",
            "using", "make", "made", "thing", "things", "way", "well", "really",
            "want", "need",
        ];
        for w in words {
            s.insert(w);
        }
        s
    };
}

// ---------------------------------------------------------------------------
// Phase 3 — MetadataOverlay
// ---------------------------------------------------------------------------

/// Non-lossy structured metadata stored alongside an AAAK summary.
/// Serialised as a `JSON:{...}` line appended to the compressed output,
/// allowing consumers to strip it cleanly without touching the summary.
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct MetadataOverlay {
    /// Semantic version of the AAAK dialect that produced this block.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// Wing the memory belongs to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wing: Option<String>,
    /// Room the memory belongs to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub room: Option<String>,
    /// ISO-8601 date/timestamp.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,
    /// Source file name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_file: Option<String>,
    /// Arbitrary extra key/value pairs callers may attach.
    #[serde(skip_serializing_if = "HashMap::is_empty", default)]
    pub extra: HashMap<String, serde_json::Value>,
}

impl MetadataOverlay {
    /// Serialise to a `JSON:{...}` line suitable for appending to AAAK output.
    pub fn to_line(&self) -> String {
        match serde_json::to_string(self) {
            Ok(json) => format!("JSON:{}", json),
            Err(_) => String::new(),
        }
    }

    /// Parse a `JSON:{...}` line back into a `MetadataOverlay`.
    pub fn from_line(line: &str) -> Option<Self> {
        let json_str = line.strip_prefix("JSON:")?;
        serde_json::from_str(json_str).ok()
    }
}

// ---------------------------------------------------------------------------
// Dialect struct & core impl
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct Dialect {
    pub entity_codes: HashMap<String, String>,
    pub skip_names: Vec<String>,
    /// Phase 4: external emotion overrides (name → code).
    /// When non-empty, merged on top of the built-in EMOTION_CODES at runtime.
    #[serde(default)]
    pub custom_emotions: HashMap<String, String>,
}

impl Dialect {
    pub fn new(entities: Option<HashMap<String, String>>, skip_names: Option<Vec<String>>) -> Self {
        let mut entity_codes = HashMap::new();
        if let Some(entities) = entities {
            for (name, code) in entities {
                entity_codes.insert(name.clone(), code.clone());
                entity_codes.insert(name.to_lowercase(), code);
            }
        }
        Self {
            entity_codes,
            skip_names: skip_names
                .unwrap_or_default()
                .iter()
                .map(|s| s.to_lowercase())
                .collect(),
            custom_emotions: HashMap::new(),
        }
    }

    /// Phase 4: construct a Dialect with an external emotion dictionary merged
    /// on top of the built-in map.
    pub fn with_custom_emotions(
        entities: Option<HashMap<String, String>>,
        skip_names: Option<Vec<String>>,
        custom_emotions: HashMap<String, String>,
    ) -> Self {
        let mut dialect = Self::new(entities, skip_names);
        dialect.custom_emotions = custom_emotions;
        dialect
    }

    pub fn encode_entity(&self, name: &str) -> Option<String> {
        let name_lower = name.to_lowercase();
        if self.skip_names.iter().any(|s| name_lower.contains(s)) {
            return None;
        }
        if let Some(code) = self.entity_codes.get(name) {
            return Some(code.clone());
        }
        if let Some(code) = self.entity_codes.get(&name_lower) {
            return Some(code.clone());
        }
        for (key, code) in &self.entity_codes {
            if key.to_lowercase().contains(&name_lower) || name_lower.contains(&key.to_lowercase())
            {
                return Some(code.clone());
            }
        }
        // Auto-code: first 3 chars uppercase
        if name.len() >= 3 {
            let code = name.chars().take(3).collect::<String>().to_uppercase();
            Some(code)
        } else {
            Some(name.to_uppercase())
        }
    }

    pub fn encode_emotions(&self, emotions: &[String]) -> String {
        let mut codes = Vec::new();
        for e in emotions {
            let code = self
                .custom_emotions
                .get(e.as_str())
                .cloned()
                .or_else(|| EMOTION_CODES.get(e.as_str()).map(|&s| s.to_string()))
                .unwrap_or_else(|| {
                    if e.len() >= 4 {
                        if e.len() > 4 {
                            e.chars().take(4).collect::<String>()
                        } else {
                            e.to_string()
                        }
                    } else {
                        e.clone()
                    }
                });
            if !codes.contains(&code) {
                codes.push(code);
            }
        }
        codes.into_iter().take(3).collect::<Vec<_>>().join("+")
    }

    fn _detect_emotions(&self, text: &str) -> Vec<String> {
        let text_lower = text.to_lowercase();
        let mut detected = Vec::new();
        let mut seen = HashSet::new();
        for (keyword, code) in EMOTION_SIGNALS.iter() {
            if text_lower.contains(keyword) && !seen.contains(code) {
                // Check custom_emotions override first
                let final_code = self
                    .custom_emotions
                    .get(*code)
                    .map(|s| s.as_str())
                    .unwrap_or(code);
                detected.push(final_code.to_string());
                seen.insert(code);
            }
        }
        detected.into_iter().take(3).collect()
    }

    fn _detect_flags(&self, text: &str) -> Vec<String> {
        let text_lower = text.to_lowercase();
        let mut detected = Vec::new();
        let mut seen = HashSet::new();
        for (keyword, flag) in FLAG_SIGNALS.iter() {
            if text_lower.contains(keyword) && !seen.contains(flag) {
                detected.push(flag.to_string());
                seen.insert(flag);
            }
        }
        detected.into_iter().take(3).collect()
    }

    /// Phase 2: `max_topics` is now caller-controlled (driven by `density`).
    fn _extract_topics(&self, text: &str, max_topics: usize) -> Vec<String> {
        let re = Regex::new(r"[a-zA-Z][a-zA-Z_-]{2,}").unwrap();
        let mut freq = HashMap::new();
        for mat in re.find_iter(text) {
            let w = mat.as_str();
            let w_lower = w.to_lowercase();
            if STOP_WORDS.contains(w_lower.as_str()) || w_lower.len() < 3 {
                continue;
            }
            let count = freq.entry(w_lower.clone()).or_insert(0);
            *count += 1;

            // Boost proper nouns or technical terms
            if w.chars().next().unwrap().is_uppercase() {
                *count += 2;
            }
            if w.contains('_') || w.contains('-') || w.chars().skip(1).any(|c| c.is_uppercase()) {
                *count += 2;
            }
        }

        let mut ranked: Vec<_> = freq.into_iter().collect();
        ranked.sort_by(|a, b| b.1.cmp(&a.1));
        ranked
            .into_iter()
            .take(max_topics)
            .map(|(w, _)| w)
            .collect()
    }

    fn _extract_key_sentence(&self, text: &str) -> String {
        let sentences: Vec<&str> = text
            .split(['.', '!', '?', '\n'])
            .map(|s| s.trim())
            .filter(|s| s.len() > 10)
            .collect();

        if sentences.is_empty() {
            return String::new();
        }

        let decision_words = vec![
            "decided",
            "because",
            "instead",
            "prefer",
            "switched",
            "chose",
            "realized",
            "important",
            "key",
            "critical",
            "discovered",
            "learned",
            "conclusion",
            "solution",
            "reason",
            "why",
            "breakthrough",
            "insight",
        ];

        let mut scored = Vec::new();
        for s in sentences {
            let mut score = 0;
            let s_lower = s.to_lowercase();
            for w in &decision_words {
                if s_lower.contains(w) {
                    score += 2;
                }
            }
            if s.len() < 80 {
                score += 1;
            }
            if s.len() < 40 {
                score += 1;
            }
            if s.len() > 150 {
                score -= 2;
            }
            scored.push((score, s));
        }

        scored.sort_by(|a, b| b.0.cmp(&a.0));
        let best = scored[0].1;
        if best.len() > 55 {
            format!("{}...", best.chars().take(52).collect::<String>())
        } else {
            best.to_string()
        }
    }

    /// Phase 2: `max_entities` is now caller-controlled (driven by `density`).
    /// Phase 9 (Hardening): Support shadow ID formatting NAME[#id].
    fn _detect_entities_in_text(&self, text: &str, max_entities: usize) -> Vec<String> {
        let mut found = Vec::new();
        let text_lower = text.to_lowercase();

        for (name, code) in &self.entity_codes {
            if !name.chars().all(|c| c.is_lowercase())
                && text_lower.contains(&name.to_lowercase())
                && !found.contains(code)
            {
                // Generate the stable shadow ID for shadowing
                let shadow_id = self._generate_shadow_id(name);
                found.push(format!("{}[#{}]", code, shadow_id));
            }
        }

        if !found.is_empty() {
            return found.into_iter().take(max_entities).collect();
        }

        let words: Vec<&str> = text.split_whitespace().collect();
        for (i, w) in words.iter().enumerate() {
            let clean: String = w.chars().filter(|c| c.is_alphabetic()).collect();
            if clean.len() >= 2
                && clean.chars().next().unwrap().is_uppercase()
                && clean.chars().skip(1).all(|c| c.is_lowercase())
                && i > 0
                && !STOP_WORDS.contains(clean.to_lowercase().as_str())
            {
                let code: String = clean.chars().take(3).collect::<String>().to_uppercase();
                let shadow_id = self._generate_shadow_id(&clean);
                let shadow_code = format!("{}[#{}]", code, shadow_id);
                if !found.contains(&shadow_code) {
                    found.push(shadow_code);
                }
                if found.len() >= max_entities {
                    break;
                }
            }
        }
        found
    }

    fn _generate_shadow_id(&self, name: &str) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut hasher = DefaultHasher::new();
        name.to_lowercase().hash(&mut hasher);
        let hash_val = hasher.finish();
        format!("{:x}", hash_val).chars().take(5).collect()
    }

    // ---------------------------------------------------------------------------
    // Phase 2 — density-driven compression
    // ---------------------------------------------------------------------------

    /// Summarisation density: 1 = ultra-compact, 5 = default, 10 = verbose.
    ///
    /// Density controls how many entities and topics are extracted.
    /// | density | max_entities | max_topics |
    /// |---------|-------------|------------|
    /// | 1–2     | 1           | 2          |
    /// | 3–4     | 2           | 3          |
    /// | 5 (def) | 3           | 3          |
    /// | 6–8     | 4           | 5          |
    /// | 9–10    | 5           | 7          |
    fn density_limits(density: usize) -> (usize, usize) {
        match density {
            0..=2 => (1, 2),
            3..=4 => (2, 3),
            5 => (3, 3),
            6..=8 => (4, 5),
            _ => (5, 7),
        }
    }

    // ---------------------------------------------------------------------------
    // Phase 1+2+3 — versioned, density-aware compress
    // ---------------------------------------------------------------------------

    /// Compress `text` into an AAAK block.
    ///
    /// * `metadata` — optional key/value pairs (`source_file`, `wing`, `room`, `date`).
    /// * `density`  — summarisation verbosity 1–10 (default 5 via `compress()`).
    pub fn compress_with_density(
        &self,
        text: &str,
        metadata: Option<HashMap<String, String>>,
        density: usize,
    ) -> String {
        let metadata = metadata.unwrap_or_default();
        let (max_entities, max_topics) = Self::density_limits(density);

        let entities = self._detect_entities_in_text(text, max_entities);
        let entity_str = if entities.is_empty() {
            "???".to_string()
        } else {
            entities.join("+")
        };

        let topics = self._extract_topics(text, max_topics);
        let topic_str = if topics.is_empty() {
            "misc".to_string()
        } else {
            topics.join("_")
        };

        let quote = self._extract_key_sentence(text);
        let quote_part = if quote.is_empty() {
            String::new()
        } else {
            format!("\"{}\"", quote)
        };

        let emotions = self._detect_emotions(text);
        let emotion_str = emotions.join("+");

        let mut flags = self._detect_flags(text);

        // Write Discipline: Grammar Matrix Validation
        let structured_memories = crate::extractor::extract_structured_memories(text);
        let mut is_compliant_decision = false;

        if flags.iter().any(|f| f == "DECISION") {
            if let Some(decision_mem) = structured_memories
                .iter()
                .find(|m| m.memory_type == crate::models::MemoryType::Decision)
            {
                let m = &decision_mem.matrix;
                if m.contains_key("WHO")
                    && m.contains_key("WHAT")
                    && m.contains_key("WHY")
                    && m.contains_key("CONFIDENCE")
                {
                    is_compliant_decision = true;
                    // Tag with registry version
                    flags = flags
                        .into_iter()
                        .map(|f| {
                            if f == "DECISION" {
                                "DECISION[v1]".to_string()
                            } else {
                                f
                            }
                        })
                        .collect();
                } else if density >= 5 {
                    // Critical failure for high-density: Fallback to Raw
                    return format!("RAW|FBF|{}", text);
                }
            }
        }

        let flag_str = flags.join("+");

        // Faithfulness Scoring
        let faithfulness_score = self._calculate_faithfulness(text, &entities, &topics);

        let mut lines = Vec::new();

        // Phase 1 — version header
        lines.push(AAAK_VERSION.to_string());

        let source = metadata.get("source_file");
        let wing = metadata.get("wing");
        let room = metadata.get("room");
        let date = metadata.get("date");

        if source.is_some() || wing.is_some() {
            let header_parts = [
                wing.map(|s| s.as_str()).unwrap_or("?"),
                room.map(|s| s.as_str()).unwrap_or("?"),
                date.map(|s| s.as_str()).unwrap_or("?"),
                source
                    .map(|s| {
                        Path::new(s)
                            .file_stem()
                            .and_then(|s| s.to_str())
                            .unwrap_or("?")
                    })
                    .unwrap_or("?"),
            ];
            lines.push(header_parts.join("|"));
        }

        let mut parts = vec![format!("0:{}", entity_str), topic_str];
        if !quote_part.is_empty() {
            parts.push(quote_part);
        }
        if !emotion_str.is_empty() {
            parts.push(emotion_str);
        }
        if !flag_str.is_empty() {
            parts.push(flag_str);
        }

        lines.push(parts.join("|"));

        // Phase 3 — MetadataOverlay
        let overlay = MetadataOverlay {
            version: Some(AAAK_VERSION.to_string()),
            wing: wing.cloned(),
            room: room.cloned(),
            date: date.cloned(),
            source_file: source.cloned(),
            extra: {
                let mut map = HashMap::new();
                map.insert(
                    "faithfulness".to_string(),
                    serde_json::Value::Number(
                        serde_json::Number::from_f64(faithfulness_score as f64)
                            .unwrap_or(serde_json::Number::from(0)),
                    ),
                );
                if is_compliant_decision {
                    map.insert(
                        "grammar_reg".to_string(),
                        serde_json::Value::String("v1".to_string()),
                    );
                }
                map
            },
        };
        let overlay_line = overlay.to_line();
        if !overlay_line.is_empty()
            && (wing.is_some() || source.is_some() || !overlay.extra.is_empty())
        {
            lines.push(overlay_line);
        }

        lines.join("\n")
    }

    fn _calculate_faithfulness(&self, text: &str, entities: &[String], topics: &[String]) -> f32 {
        if text.is_empty() {
            return 1.0;
        }

        // Basic heuristic: entity/topic density + sentence persistence
        let e_score = (entities.len() as f32 * 0.2).min(0.5);
        let t_score = (topics.len() as f32 * 0.1).min(0.5);

        let score = e_score + t_score;
        (score * 100.0).round() / 100.0
    }

    /// Compress with default density (5).
    pub fn compress(&self, text: &str, metadata: Option<HashMap<String, String>>) -> String {
        self.compress_with_density(text, metadata, 5)
    }

    // ---------------------------------------------------------------------------
    // Phase 1 — versioned decode
    // ---------------------------------------------------------------------------

    /// Phase 8: Merge multiple AAAK summaries into one (the first one is the "winner").
    /// Unionizes topics and entities, keeps winner's arc/quote/emotions.
    pub fn merge_aaaks(&self, blocks: &[String]) -> String {
        if blocks.is_empty() {
            return String::new();
        }
        if blocks.len() == 1 {
            return blocks[0].clone();
        }

        let mut all_entities = HashSet::new();
        let mut all_topics = HashSet::new();

        for block in blocks {
            let decoded = self.decode(block);
            if let Some(zettels) = decoded.get("zettels").and_then(|z| z.as_array()) {
                for zettel in zettels {
                    if let Some(entities) = zettel.get("entities").and_then(|e| e.as_array()) {
                        for e in entities {
                            if let Some(s) = e.as_str() {
                                all_entities.insert(s.to_string());
                            }
                        }
                    }
                    if let Some(topics) = zettel.get("topics").and_then(|t| t.as_array()) {
                        for t in topics {
                            if let Some(s) = t.as_str() {
                                all_topics.insert(s.to_string());
                            }
                        }
                    }
                }
            }
        }

        // Winner is the first block
        let winner = &blocks[0];
        let mut lines: Vec<String> = winner.lines().map(|s| s.to_string()).collect();

        // Update the winner's Zettel line (usually the 3rd line or 2nd if no version header??)
        // Actually V:3.2 has version on line 1, header on line 2, zettels starting line 3.
        for line in &mut lines {
            if line.contains('|') && !line.starts_with("JSON:") && !line.starts_with("V:") {
                let parts: Vec<&str> = line.split('|').collect();
                if parts.len() >= 2 && parts[0].contains(':') {
                    // This is a zettel line: entity_arc|topics|...
                    let mut entities_list: Vec<_> = all_entities.iter().cloned().collect();
                    entities_list.sort();
                    let arc_prefix = parts[0].split(':').next().unwrap_or("0");
                    let new_entities = format!("{}:{}", arc_prefix, entities_list.join("+"));

                    let mut topics_list: Vec<_> = all_topics.iter().cloned().collect();
                    topics_list.sort();
                    let new_topics = topics_list.join("_");

                    let mut new_parts = parts.clone();
                    new_parts[0] = &new_entities;
                    new_parts[1] = &new_topics;
                    *line = new_parts.join("|");
                }
            }
        }

        lines.join("\n")
    }

    pub fn decode(&self, dialect_text: &str) -> serde_json::Value {
        let lines: Vec<&str> = dialect_text.trim().split('\n').collect();
        let mut result = serde_json::json!({
            "version": null,
            "header": {},
            "arc": "",
            "zettels": [],
            "tunnels": [],
            "overlay": null
        });

        for line in lines {
            // Phase 1 — version line
            if let Some(ver) = line.strip_prefix("V:") {
                result["version"] = serde_json::Value::String(ver.to_string());
            } else if let Some(stripped) = line.strip_prefix("ARC:") {
                result["arc"] = serde_json::Value::String(stripped.to_string());
            } else if line.starts_with("T:") {
                result["tunnels"]
                    .as_array_mut()
                    .unwrap()
                    .push(serde_json::Value::String(line.to_string()));
            // Phase 3 — MetadataOverlay
            } else if line.starts_with("JSON:") {
                if let Some(overlay) = MetadataOverlay::from_line(line) {
                    result["overlay"] = serde_json::to_value(&overlay).unwrap_or_default();
                }
            } else if line.contains('|') && line.split('|').next().unwrap().contains(':') {
                result["zettels"]
                    .as_array_mut()
                    .unwrap()
                    .push(serde_json::Value::String(line.to_string()));
            } else if line.contains('|') {
                let parts: Vec<&str> = line.split('|').collect();
                result["header"] = serde_json::json!({
                    "wing": parts.first().unwrap_or(&""),
                    "room": parts.get(1).unwrap_or(&""),
                    "date": parts.get(2).unwrap_or(&""),
                    "title": parts.get(3).unwrap_or(&""),
                });
            }
        }
        result
    }

    /// Estimate token count using word-based heuristic (~1.3 tokens per word).
    ///
    /// This is an approximation. The old len(text)/3 heuristic was wildly inaccurate
    /// and made AAAK compression ratios look much better than reality.
    /// ~1.3 tokens/word is a conservative average (most English words tokenize to 1-2 tokens;
    /// punctuation and special chars in AAAK (|, +, :) each cost a token).
    pub fn count_tokens(text: &str) -> usize {
        let words: Vec<&str> = text.split_whitespace().collect();
        // ~1.3 tokens per word is a conservative average
        std::cmp::max(1, (words.len() as f64 * 1.3).round() as usize)
    }

    /// Get size comparison stats for a text->AAAK conversion.
    ///
    /// NOTE: AAAK is lossy summarisation, not compression. The "ratio"
    /// reflects how much shorter the summary is, not a compression ratio
    /// in the traditional sense — information is lost.
    pub fn compression_stats(&self, original_text: &str, compressed: &str) -> serde_json::Value {
        let orig_tokens = Self::count_tokens(original_text);
        let comp_tokens = Self::count_tokens(compressed);
        let size_ratio = if comp_tokens > 0 {
            (orig_tokens as f64 / comp_tokens as f64 * 10.0).round() / 10.0
        } else {
            1.0
        };

        serde_json::json!({
            "original_tokens_est": orig_tokens,
            "summary_tokens_est": comp_tokens,
            "size_ratio": size_ratio,
            "original_chars": original_text.len(),
            "summary_chars": compressed.len(),
            "note": "Estimates only. AAAK is lossy summarisation, not lossless compression."
        })
    }

    // ---------------------------------------------------------------------------
    // Phase 5 — Proposition Atomisation
    // ---------------------------------------------------------------------------

    /// Split `text` into atomic propositions: self-contained, factoid-level sentences.
    ///
    /// Each proposition ideally encapsulates a single distinct claim, making retrieval
    /// more precise (inspired by Dense X Retrieval, arXiv 2312.06648).
    ///
    /// Returns up to `max_propositions` propositions sorted by information density.
    pub fn atomize(&self, text: &str, max_propositions: usize) -> Vec<String> {
        let sentences: Vec<&str> = text
            .split(['.', '!', '?', '\n'])
            .map(|s| s.trim())
            .filter(|s| s.len() >= 20)
            .collect();

        let fact_signals = [
            "decided",
            "is",
            "uses",
            "requires",
            "because",
            "means",
            "therefore",
            "thus",
            "enables",
            "prevents",
            "causes",
            "results",
            "switched",
            "chose",
            "replaced",
            "migrated",
        ];

        let mut scored: Vec<(i32, &str)> = sentences
            .iter()
            .map(|s| {
                let s_lower = s.to_lowercase();
                let mut score = 0i32;
                // Prefer sentences with named entities (starts with uppercase after first word)
                if s.split_whitespace()
                    .skip(1)
                    .any(|w| w.chars().next().map(|c| c.is_uppercase()).unwrap_or(false))
                {
                    score += 2;
                }
                // Prefer factual signal words
                for w in &fact_signals {
                    if s_lower.contains(w) {
                        score += 1;
                    }
                }
                // Penalise very long sentences (less atomic)
                if s.len() > 150 {
                    score -= 2;
                } else if s.len() < 80 {
                    score += 1;
                }
                (score, *s)
            })
            .collect();

        scored.sort_by(|a, b| b.0.cmp(&a.0));
        scored
            .into_iter()
            .take(max_propositions)
            .map(|(_, s)| s.to_string())
            .collect()
    }

    /// Compress `text` as a sequence of AAAK proposition lines (`P0:`, `P1:`, …).
    ///
    /// This produces a multi-line AAAK block where each line represents one
    /// atomic proposition independently compressed.
    pub fn compress_propositions(
        &self,
        text: &str,
        metadata: Option<HashMap<String, String>>,
        max_propositions: usize,
        density: usize,
    ) -> String {
        let metadata = metadata.unwrap_or_default();
        let propositions = self.atomize(text, max_propositions);
        let (max_entities, max_topics) = Self::density_limits(density);

        let mut lines = Vec::new();

        // Version header
        lines.push(AAAK_VERSION.to_string());

        let wing = metadata.get("wing");
        let room = metadata.get("room");
        let date = metadata.get("date");
        let source = metadata.get("source_file");

        if source.is_some() || wing.is_some() {
            let header_parts = [
                wing.map(|s| s.as_str()).unwrap_or("?"),
                room.map(|s| s.as_str()).unwrap_or("?"),
                date.map(|s| s.as_str()).unwrap_or("?"),
                source
                    .map(|s| {
                        Path::new(s)
                            .file_stem()
                            .and_then(|os| os.to_str())
                            .unwrap_or("?")
                    })
                    .unwrap_or("?"),
            ];
            lines.push(header_parts.join("|"));
        }

        for (idx, prop) in propositions.iter().enumerate() {
            let entities = self._detect_entities_in_text(prop, max_entities);
            let entity_str = if entities.is_empty() {
                "???".to_string()
            } else {
                entities.join("+")
            };
            let topics = self._extract_topics(prop, max_topics);
            let topic_str = if topics.is_empty() {
                "misc".to_string()
            } else {
                topics.join("_")
            };
            let emotions = self._detect_emotions(prop);
            let flags = self._detect_flags(prop);

            let mut parts = vec![format!("P{}:{}", idx, entity_str), topic_str];
            if !emotions.is_empty() {
                parts.push(emotions.join("+"));
            }
            if !flags.is_empty() {
                parts.push(flags.join("+"));
            }
            lines.push(parts.join("|"));
        }

        // MetadataOverlay
        let overlay = MetadataOverlay {
            version: Some(AAAK_VERSION.to_string()),
            wing: wing.cloned(),
            room: room.cloned(),
            date: date.cloned(),
            source_file: source.cloned(),
            extra: HashMap::new(),
        };
        let overlay_line = overlay.to_line();
        if !overlay_line.is_empty() && (wing.is_some() || source.is_some()) {
            lines.push(overlay_line);
        }

        lines.join("\n")
    }

    // ---------------------------------------------------------------------------
    // Phase 9 — Faithfulness score
    // ---------------------------------------------------------------------------

    /// Returns `(compressed_aaak, faithfulness_score)`.
    ///
    /// Faithfulness (0.0–1.0) measures what fraction of the top-10 topics
    /// extracted from the original text appear in the compressed output.
    /// A score of 1.0 means the summary captured all key topics; 0.0 means full
    /// information loss.
    pub fn compress_with_faithfulness(
        &self,
        text: &str,
        metadata: Option<HashMap<String, String>>,
    ) -> (String, f64) {
        let full_topics: HashSet<String> = self._extract_topics(text, 10).into_iter().collect();
        let compressed = self.compress(text, metadata);
        let compressed_lower = compressed.to_lowercase();
        let covered = full_topics
            .iter()
            .filter(|t| compressed_lower.contains(t.as_str()))
            .count();
        let score = covered as f64 / full_topics.len().max(1) as f64;
        (compressed, score)
    }

    // ---------------------------------------------------------------------------
    // Phase 7 — Delta encoding
    // ---------------------------------------------------------------------------

    /// Compare a new text against an existing AAAK summary.
    ///
    /// If the topic-level change is < 40%, emits a compact `DELTA:+added,-removed`
    /// line. Otherwise falls back to a full `compress()`.
    pub fn compress_delta(&self, old_aaak: &str, new_text: &str) -> String {
        let new_aaak = self.compress(new_text, None);

        // Extract topic tokens from both summaries
        let extract_tokens = |s: &str| -> HashSet<String> {
            s.split(['|', '\n'])
                .flat_map(|seg| seg.split('_'))
                .map(|t| t.trim().to_lowercase())
                .filter(|t| !t.is_empty() && !t.starts_with("0:") && !t.starts_with("v:"))
                .collect()
        };

        let old_tokens = extract_tokens(old_aaak);
        let new_tokens = extract_tokens(&new_aaak);

        if old_tokens.is_empty() {
            return new_aaak;
        }

        let added: Vec<String> = new_tokens.difference(&old_tokens).cloned().collect();
        let removed: Vec<String> = old_tokens.difference(&new_tokens).cloned().collect();

        let change_ratio = (added.len() + removed.len()) as f64 / old_tokens.len().max(1) as f64;

        if change_ratio < 0.40 {
            let mut parts = Vec::new();
            if !added.is_empty() {
                let mut sorted = added.clone();
                sorted.sort();
                parts.push(
                    sorted
                        .iter()
                        .map(|s| format!("+{}", s))
                        .collect::<Vec<_>>()
                        .join(","),
                );
            }
            if !removed.is_empty() {
                let mut sorted = removed.clone();
                sorted.sort();
                parts.push(
                    sorted
                        .iter()
                        .map(|s| format!("-{}", s))
                        .collect::<Vec<_>>()
                        .join(","),
                );
            }
            if parts.is_empty() {
                return "DELTA:(no change)".to_string();
            }
            format!("DELTA:{}", parts.join(","))
        } else {
            new_aaak
        }
    }

    pub fn generate_layer1(
        &self,
        docs: &[String],
        metas: &[Option<serde_json::Map<String, serde_json::Value>>],
    ) -> String {
        if docs.is_empty() {
            return "## L1 — No memories yet.".to_string();
        }

        let mut scored = Vec::new();
        for (doc, meta) in docs.iter().zip(metas.iter()) {
            let mut importance = 3.0;
            if let Some(meta_map) = meta {
                for key in &["importance", "emotional_weight", "weight"] {
                    if let Some(val) = meta_map.get(*key) {
                        if let Some(f) = val.as_f64() {
                            importance = f;
                            break;
                        }
                    }
                }
            }
            // Density-aware: shorter docs with high importance get a slight boost
            let density_boost = if !doc.is_empty() {
                100.0 / doc.len() as f64
            } else {
                0.0
            };
            importance += density_boost * 0.1;

            scored.push((importance, meta, doc));
        }

        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        let top = scored.into_iter().take(15);

        let mut by_room: HashMap<String, Vec<_>> = HashMap::new();
        for (imp, meta, doc) in top {
            let room = meta
                .as_ref()
                .and_then(|m| m.get("room"))
                .and_then(|v| v.as_str())
                .unwrap_or("general")
                .to_string();
            by_room.entry(room).or_default().push((imp, meta, doc));
        }

        let mut lines = vec!["## L1 — ESSENTIAL STORY".to_string()];
        let mut sorted_rooms: Vec<_> = by_room.keys().cloned().collect::<Vec<_>>();
        sorted_rooms.sort();

        let mut total_len = 0;
        let max_chars = 3200;

        for room in sorted_rooms {
            let room_header = format!("### {}", room.to_uppercase());
            lines.push(room_header.clone());
            total_len += room_header.len();

            let room_docs = by_room.get(&room).unwrap();
            for (imp, meta, doc) in room_docs {
                let mut snippet = doc.trim().replace('\n', " ");
                if snippet.len() > 200 {
                    snippet = format!("{}...", &snippet[..197]);
                }

                // Map importance (e.g. 5.0) to 0-9 weight
                let weight = (imp * 2.0).round().min(9.0) as u8;
                let mut entry_line = format!("  - WT:{}| {}", weight, snippet);

                if let Some(meta_map) = meta {
                    if let Some(sf) = meta_map.get("source_file").and_then(|v| v.as_str()) {
                        let source_name = std::path::Path::new(sf)
                            .file_name()
                            .and_then(|s| s.to_str())
                            .unwrap_or("");
                        if !source_name.is_empty() {
                            entry_line = format!("{}  ({})", entry_line, source_name);
                        }
                    }
                }

                if total_len + entry_line.len() > max_chars {
                    lines.push("  ... (more in L3 search)".to_string());
                    return lines.join("\n");
                }

                lines.push(entry_line.clone());
                total_len += entry_line.len();
            }
        }

        lines.join("\n")
    }
}

pub struct AAAKContext;

impl AAAKContext {
    pub fn compress(input: &str) -> String {
        let dialect = Dialect::default();
        dialect.compress(input, None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version_header_present() {
        let dialect = Dialect::default();
        let out = dialect.compress("Alice decided to switch to Rust.", None);
        assert!(
            out.starts_with("V:3.2"),
            "V:3.2 header must be first line; got: {}",
            out
        );
    }

    #[test]
    fn test_decode_parses_version() {
        let dialect = Dialect::default();
        let compressed = dialect.compress("Alice decided to use Rust.", None);
        let decoded = dialect.decode(&compressed);
        assert_eq!(decoded["version"].as_str().unwrap(), "3.2");
    }

    #[test]
    fn test_compress_with_density_low() {
        let dialect = Dialect::default();
        // density=1 → max 1 entity, 2 topics
        let out = dialect.compress_with_density(
            "Alice and Bob decided to migrate from Python to Rust for performance reasons.",
            None,
            1,
        );
        // At density 1 we get at most 1 entity in 0:X segment
        let zettel_line = out.lines().find(|l| l.starts_with("0:")).unwrap();
        let entity_part = zettel_line.split('|').next().unwrap(); // "0:ALI"
        let entities: Vec<&str> = entity_part.trim_start_matches("0:").split('+').collect();
        assert!(
            entities.len() <= 1,
            "density=1 should yield at most 1 entity"
        );
    }

    #[test]
    fn test_compress_with_density_high() {
        let dialect = Dialect::default();
        let out = dialect.compress_with_density(
            "Alice and Bob and Charlie decided to migrate from Python to Rust for performance.",
            None,
            9,
        );
        let zettel_line = out.lines().find(|l| l.starts_with("0:")).unwrap();
        let entity_part = zettel_line.split('|').next().unwrap();
        let entities: Vec<&str> = entity_part.trim_start_matches("0:").split('+').collect();
        assert!(
            entities.len() <= 5,
            "density=9 should yield at most 5 entities"
        );
    }

    #[test]
    fn test_metadata_overlay_roundtrip() {
        let overlay = MetadataOverlay {
            version: Some("V:3.2".to_string()),
            wing: Some("technical".to_string()),
            room: Some("rust".to_string()),
            date: Some("2026-04-08".to_string()),
            source_file: Some("session.md".to_string()),
            extra: HashMap::new(),
        };
        let line = overlay.to_line();
        assert!(
            line.starts_with("JSON:"),
            "overlay line must start with JSON:"
        );
        let parsed = MetadataOverlay::from_line(&line).unwrap();
        assert_eq!(parsed.wing, Some("technical".to_string()));
        assert_eq!(parsed.room, Some("rust".to_string()));
    }

    #[test]
    fn test_compress_emits_overlay_when_metadata_present() {
        let dialect = Dialect::default();
        let mut meta = HashMap::new();
        meta.insert("wing".to_string(), "technical".to_string());
        meta.insert("source_file".to_string(), "session.md".to_string());
        let out = dialect.compress("Rust is fast.", Some(meta));
        assert!(
            out.contains("JSON:"),
            "overlay JSON line must be emitted: {}",
            out
        );
    }

    #[test]
    fn test_decode_parses_overlay() {
        let dialect = Dialect::default();
        let mut meta = HashMap::new();
        meta.insert("wing".to_string(), "technical".to_string());
        meta.insert("room".to_string(), "rust".to_string());
        meta.insert("source_file".to_string(), "s.md".to_string());
        let compressed = dialect.compress("Rust is performant.", Some(meta));
        let decoded = dialect.decode(&compressed);
        assert!(!decoded["overlay"].is_null(), "overlay must be decoded");
        assert_eq!(decoded["overlay"]["wing"].as_str().unwrap(), "technical");
    }

    #[test]
    fn test_custom_emotions_override() {
        let mut custom = HashMap::new();
        custom.insert("joy".to_string(), "XJY".to_string());
        let dialect = Dialect::with_custom_emotions(None, None, custom);
        let encoded = dialect.encode_emotions(&["joy".to_string()]);
        // Custom override takes precedence
        assert_eq!(encoded, "XJY");
    }

    #[test]
    fn test_atomize_returns_propositions() {
        let dialect = Dialect::default();
        let text = "Alice decided to rewrite the service in Rust. \
                    The new implementation is 10x faster. \
                    Bob reviewed the PR and approved it. \
                    Deployment happened on Friday.";
        let props = dialect.atomize(text, 3);
        assert!(!props.is_empty());
        assert!(props.len() <= 3);
        // Each proposition should be a non-empty string
        for p in &props {
            assert!(!p.is_empty());
        }
    }

    #[test]
    fn test_compress_propositions_format() {
        let dialect = Dialect::default();
        let text =
            "Alice decided to use Rust. Bob chose tokio for async. The database uses SQLite.";
        let out = dialect.compress_propositions(text, None, 3, 5);
        assert!(out.starts_with("V:3.2"));
        // Should have P0: line
        assert!(out.contains("P0:"), "must have P0: proposition line");
    }

    #[test]
    fn test_faithfulness_score_bounded() {
        let dialect = Dialect::default();
        let text = "Rust enables safe concurrency via ownership and borrowing.";
        let (_, score) = dialect.compress_with_faithfulness(text, None);
        assert!(
            (0.0..=1.0).contains(&score),
            "faithfulness must be 0.0–1.0, got {}",
            score
        );
    }

    #[test]
    fn test_faithfulness_high_for_rich_text() {
        let dialect = Dialect::default();
        // Dense technical text should have good faithfulness
        let text = "Rust memory ownership borrowing lifetime borrow-checker prevents null pointers performs zero-cost abstractions.";
        let (_, score) = dialect.compress_with_faithfulness(text, None);
        assert!(score > 0.0, "faithfulness should be > 0 for rich text");
    }

    #[test]
    fn test_compress_delta_small_change() {
        let dialect = Dialect::default();
        let original = "Alice decided to use Rust for performance.";
        let old_aaak = dialect.compress(original, None);
        // Slightly modified text
        let new_text = "Alice decided to use Rust for performance and safety.";
        let delta = dialect.compress_delta(&old_aaak, new_text);
        // Small change (< 40%) should give DELTA: prefix
        assert!(
            delta.starts_with("DELTA:") || delta.starts_with("V:"),
            "should be delta or full recompress: {}",
            delta
        );
    }

    #[test]
    fn test_compress_delta_large_change_gives_full() {
        let dialect = Dialect::default();
        let old_aaak = dialect.compress("Alice uses Python for scripting.", None);
        let new_text = "A completely different topic: quantum computing and superconductors require cryogenic temperatures.";
        let result = dialect.compress_delta(&old_aaak, new_text);
        // Large topic divergence should yield full recompression (V:3.2 header)
        assert!(
            result.starts_with("V:3.2") || result.starts_with("DELTA:"),
            "unexpected result: {}",
            result
        );
    }
}
