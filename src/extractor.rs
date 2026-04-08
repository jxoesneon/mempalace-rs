use crate::models::MemoryType;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

lazy_static::lazy_static! {
    pub static ref DECISION_MARKERS: Vec<Regex> = vec![
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
        Regex::new(r"(?i)\bwho:").unwrap(),
        Regex::new(r"(?i)\bwhat:").unwrap(),
        Regex::new(r"(?i)\bwhy:").unwrap(),
        Regex::new(r"(?i)\bconfidence:").unwrap(),
    ];

    pub static ref PREFERENCE_MARKERS: Vec<Regex> = vec![
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

    pub static ref MILESTONE_MARKERS: Vec<Regex> = vec![
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

    pub static ref PROBLEM_MARKERS: Vec<Regex> = vec![
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

    pub static ref EMOTION_MARKERS: Vec<Regex> = vec![
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

    pub static ref CODE_LINE_PATTERNS: Vec<Regex> = vec![
        Regex::new(r"^\s*[\$#]\s").unwrap(),
        Regex::new(r"^\s*(cd|source|echo|export|pip|npm|git|python|bash|curl|wget|mkdir|rm|cp|mv|ls|cat|grep|find|chmod|sudo|brew|docker)\s").unwrap(),
        Regex::new(r"^\s*```").unwrap(),
        Regex::new(r"^\s*(import|from|def|class|function|const|let|var|return)\s").unwrap(),
        Regex::new(r"^\s*[A-Z_]{2,}=").unwrap(),
        Regex::new(r"^\s*\|").unwrap(),
        Regex::new(r"^\s*[-]{2,}").unwrap(),
        Regex::new(r"^\s*[{}\[\]]\s*$").unwrap(),
        Regex::new(r"(?i)^\s*(if|for|while|try|except|elif|else:)\b").unwrap(),
        Regex::new(r"^\s*\w+\.\w+\(").unwrap(),
        Regex::new(r"^\s*\w+ = \w+\.\w+").unwrap(),
    ];
    
    // DECISION Matrix Extraction Patterns
    pub static ref DECISION_WHO: Regex = Regex::new(r"(?i)\b(who|by):\s*([A-Z][a-zA-Z]+)\b").unwrap();
    pub static ref DECISION_WHAT: Regex = Regex::new(r"(?i)\b(what|decision):\s*(.+?)(?:\.|$|;)").unwrap();
    pub static ref DECISION_WHY: Regex = Regex::new(r"(?i)\b(why|rationale|because):\s*(.+?)(?:\.|$|;)").unwrap();
    pub static ref DECISION_CONFIDENCE: Regex = Regex::new(r"(?i)\b(confidence|certainty):\s*(high|med|low|moderate)\b").unwrap();

    pub static ref POSITIVE_WORDS: HashSet<&'static str> = {
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

    pub static ref NEGATIVE_WORDS: HashSet<&'static str> = {
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StructuredMemory {
    pub content: String,
    pub memory_type: MemoryType,
    pub topic: Option<String>,
    pub matrix: HashMap<String, String>,
    pub sentiment: f32,
    pub confidence: f32,
    pub chunk_index: usize,
}

pub fn extract_structured_memories(text: &str) -> Vec<StructuredMemory> {
    let segments = split_into_segments(text);
    let mut memories = Vec::new();

    for segment in segments {
        if segment.trim().len() < 20 {
            continue;
        }

        let prose = extract_prose(&segment);
        let mut scores = HashMap::new();

        let (decision_score, _) = score_markers(&prose, &DECISION_MARKERS);
        if decision_score > 0.0 {
            scores.insert(MemoryType::Decision, decision_score);
        }

        let (preference_score, _) = score_markers(&prose, &PREFERENCE_MARKERS);
        if preference_score > 0.0 {
            scores.insert(MemoryType::Preference, preference_score);
        }

        let (milestone_score, _) = score_markers(&prose, &MILESTONE_MARKERS);
        if milestone_score > 0.0 {
            scores.insert(MemoryType::Milestone, milestone_score);
        }

        let (problem_score, _) = score_markers(&prose, &PROBLEM_MARKERS);
        if problem_score > 0.0 {
            scores.insert(MemoryType::Problem, problem_score);
        }

        let (emotion_score, _) = score_markers(&prose, &EMOTION_MARKERS);
        if emotion_score > 0.0 {
            scores.insert(MemoryType::Emotional, emotion_score);
        }

        if scores.is_empty() {
            continue;
        }

        let mut length_bonus = 0.0;
        if segment.len() > 500 {
            length_bonus = 2.0;
        } else if segment.len() > 200 {
            length_bonus = 1.0;
        }

        let (mut max_type, max_raw_score) = scores
            .iter()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
            .map(|(t, s)| (t.clone(), *s))
            .unwrap();

        let max_score = max_raw_score + length_bonus;

        // Disambiguate
        max_type = disambiguate(max_type, &prose, &scores);

        let confidence = (max_score / 5.0).min(1.0);
        if confidence < 0.3 {
            continue;
        }

        let sentiment_val = get_sentiment_score(&prose);
        let topic = extract_topic(&prose);
        
        let mut matrix = HashMap::new();
        if max_type == MemoryType::Decision {
            if let Some(caps) = DECISION_WHO.captures(&prose) {
                matrix.insert("WHO".to_string(), caps.get(2).unwrap().as_str().trim().to_string());
            }
            if let Some(caps) = DECISION_WHAT.captures(&prose) {
                matrix.insert("WHAT".to_string(), caps.get(2).unwrap().as_str().trim().to_string());
            }
            if let Some(caps) = DECISION_WHY.captures(&prose) {
                matrix.insert("WHY".to_string(), caps.get(2).unwrap().as_str().trim().to_string());
            }
            if let Some(caps) = DECISION_CONFIDENCE.captures(&prose) {
                matrix.insert("CONFIDENCE".to_string(), caps.get(2).unwrap().as_str().trim().to_string().to_uppercase());
            }
        }

        memories.push(StructuredMemory {
            content: segment.trim().to_string(),
            memory_type: max_type,
            topic,
            matrix,
            sentiment: sentiment_val,
            confidence,
            chunk_index: memories.len(),
        });
    }

    memories
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
    if alpha_ratio < 0.4 && stripped.len() > 10 {
        return true;
    }
    false
}

fn score_markers(text: &str, markers: &[Regex]) -> (f32, Vec<String>) {
    let text_lower = text.to_lowercase();
    let mut score = 0.0;
    let mut matched = Vec::new();

    for marker in markers {
        let matches: Vec<_> = marker.find_iter(&text_lower).collect();
        if !matches.is_empty() {
            score += matches.len() as f32;
            for m in matches {
                matched.push(m.as_str().to_string());
            }
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

fn extract_topic(text: &str) -> Option<String> {
    let re = Regex::new(r"[a-zA-Z][a-zA-Z_-]{2,}").unwrap();
    let mut freq = HashMap::new();

    for mat in re.find_iter(text) {
        let w = mat.as_str();
        let w_lower = w.to_lowercase();
        if crate::dialect::STOP_WORDS.contains(w_lower.as_str()) || w_lower.len() < 3 {
            continue;
        }
        let count = freq.entry(w_lower.clone()).or_insert(0);
        *count += 1;

        if w.chars().next().unwrap().is_uppercase() {
            *count += 2;
        }
        if w.contains('_') || w.contains('-') || w.chars().skip(1).any(|c| c.is_uppercase()) {
            *count += 2;
        }
    }

    let mut ranked: Vec<_> = freq.into_iter().collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1));
    ranked.into_iter().next().map(|(w, _)| w)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_decision() {
        let text = "We decided to go with Rust because of its memory safety.";
        let memories = extract_structured_memories(text);
        assert!(!memories.is_empty());
        assert_eq!(memories[0].memory_type, MemoryType::Decision);
    }

    #[test]
    fn test_extract_preference() {
        let text = "I prefer functional style over imperative.";
        let memories = extract_structured_memories(text);
        assert!(!memories.is_empty());
        assert_eq!(memories[0].memory_type, MemoryType::Preference);
    }

    #[test]
    fn test_extract_milestone() {
        let text = "It finally works! Breakthrough in compression.";
        let memories = extract_structured_memories(text);
        assert!(!memories.is_empty());
        assert_eq!(memories[0].memory_type, MemoryType::Milestone);
    }

    #[test]
    fn test_extract_problem() {
        let text = "The bug is causing a crash in production.";
        let memories = extract_structured_memories(text);
        assert!(!memories.is_empty());
        assert_eq!(memories[0].memory_type, MemoryType::Problem);
    }

    #[test]
    fn test_extract_emotional() {
        let text = "I feel so grateful for this amazing opportunity.";
        let memories = extract_structured_memories(text);
        assert!(!memories.is_empty());
        assert_eq!(memories[0].memory_type, MemoryType::Emotional);
    }

    #[test]
    fn test_split_by_turns() {
        let text = "Human: hello\nAI: hi there\nUser: how are you?\nClaude: I am fine.";
        let segments = split_into_segments(text);
        assert_eq!(segments.len(), 4);
    }

    #[test]
    fn test_split_by_groups() {
        let lines: Vec<String> = (0..30).map(|i| format!("Line {}\n", i)).collect();
        let text = lines.join("");
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
    fn test_disambiguate_problem_to_milestone() {
        let text = "We had a bug but we fixed it by patching the code.";
        let memories = extract_structured_memories(text);
        assert!(!memories.is_empty());
        // Should be Milestone because it has resolution
        assert_eq!(memories[0].memory_type, MemoryType::Milestone);
    }

    #[test]
    fn test_disambiguate_problem_to_emotional() {
        let text = "I was so worried about the crash but I'm so happy we resolved it! Nailed it!";
        let memories = extract_structured_memories(text);
        assert!(!memories.is_empty());
        // Resolved problem with high sentiment and emotional markers -> Emotional
        assert_eq!(memories[0].memory_type, MemoryType::Emotional);
    }

    #[test]
    fn test_extract_topic_with_boosts() {
        let text = "Talking about the Mempalace_System and how it works.";
        let topic = extract_topic(text);
        assert_eq!(topic, Some("mempalace_system".to_string()));
    }

    #[test]
    fn test_confidence_threshold_and_bonus() {
        let short_text = "I prefer Rust."; // Too short (< 20 chars)
        assert!(extract_structured_memories(short_text).is_empty());

        let long_text = "I prefer functional style over imperative because it makes the code more readable and maintainable in the long run. ".repeat(10);
        let memories = extract_structured_memories(&long_text);
        assert!(!memories.is_empty());
        assert!(memories[0].confidence > 0.6); // Should get length bonus
    }
}
