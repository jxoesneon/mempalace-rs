use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::Path;

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

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct Dialect {
    pub entity_codes: HashMap<String, String>,
    pub skip_names: Vec<String>,
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
        }
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
            Some(name[..3].to_uppercase())
        } else {
            Some(name.to_uppercase())
        }
    }

    pub fn encode_emotions(&self, emotions: &[String]) -> String {
        let mut codes = Vec::new();
        for e in emotions {
            let code = EMOTION_CODES
                .get(e.as_str())
                .map(|&s| s.to_string())
                .unwrap_or_else(|| {
                    if e.len() >= 4 {
                        e[..4].to_string()
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
                detected.push(code.to_string());
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
            format!("{}...", &best[..52])
        } else {
            best.to_string()
        }
    }

    fn _detect_entities_in_text(&self, text: &str) -> Vec<String> {
        let mut found = Vec::new();
        let text_lower = text.to_lowercase();

        for (name, code) in &self.entity_codes {
            if !name.chars().all(|c| c.is_lowercase())
                && text_lower.contains(&name.to_lowercase())
                && !found.contains(code)
            {
                found.push(code.clone());
            }
        }

        if !found.is_empty() {
            return found;
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
                let code = clean[..std::cmp::min(clean.len(), 3)].to_uppercase();
                if !found.contains(&code) {
                    found.push(code);
                }
                if found.len() >= 3 {
                    break;
                }
            }
        }
        found
    }

    pub fn compress(&self, text: &str, metadata: Option<HashMap<String, String>>) -> String {
        let metadata = metadata.unwrap_or_default();

        let entities = self._detect_entities_in_text(text);
        let entity_str = if entities.is_empty() {
            "???"
        } else {
            &entities[..std::cmp::min(entities.len(), 3)].join("+")
        };

        let topics = self._extract_topics(text, 3);
        let topic_str = if topics.is_empty() {
            "misc"
        } else {
            &topics.join("_")
        };

        let quote = self._extract_key_sentence(text);
        let quote_part = if quote.is_empty() {
            String::new()
        } else {
            format!("\"{}\"", quote)
        };

        let emotions = self._detect_emotions(text);
        let emotion_str = emotions.join("+");

        let flags = self._detect_flags(text);
        let flag_str = flags.join("+");

        let mut lines = Vec::new();

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

        let mut parts = vec![format!("0:{}", entity_str), topic_str.to_string()];
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
        lines.join("\n")
    }

    pub fn decode(&self, dialect_text: &str) -> serde_json::Value {
        let lines: Vec<&str> = dialect_text.trim().split('\n').collect();
        let mut result = serde_json::json!({
            "header": {},
            "arc": "",
            "zettels": [],
            "tunnels": []
        });

        for line in lines {
            if let Some(stripped) = line.strip_prefix("ARC:") {
                result["arc"] = serde_json::Value::String(stripped.to_string());
            } else if line.starts_with("T:") {
                result["tunnels"]
                    .as_array_mut()
                    .unwrap()
                    .push(serde_json::Value::String(line.to_string()));
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
    /// NOTE: AAAK is lossy summarization, not compression. The "ratio"
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
            "note": "Estimates only. AAAK is lossy summarization, not lossless compression."
        })
    }
}

pub struct AAAKContext;

impl AAAKContext {
    pub fn compress(input: &str) -> String {
        let dialect = Dialect::default();
        dialect.compress(input, None)
    }
}
