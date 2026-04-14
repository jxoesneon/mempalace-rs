use crate::models::{DetectedEntity, EntityType};
use anyhow::{anyhow, Result};
use lazy_static::lazy_static;
use regex::Regex;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::PathBuf;

lazy_static! {
    static ref COMMON_ENGLISH_WORDS: HashSet<&'static str> = {
        let mut s = HashSet::new();
        s.extend(vec![
            "ever",
            "grace",
            "will",
            "bill",
            "mark",
            "april",
            "may",
            "june",
            "joy",
            "hope",
            "faith",
            "chance",
            "chase",
            "hunter",
            "dash",
            "flash",
            "star",
            "sky",
            "river",
            "brook",
            "lane",
            "art",
            "clay",
            "gil",
            "nat",
            "max",
            "rex",
            "ray",
            "jay",
            "rose",
            "violet",
            "lily",
            "ivy",
            "ash",
            "reed",
            "sage",
            "monday",
            "tuesday",
            "wednesday",
            "thursday",
            "friday",
            "saturday",
            "sunday",
            "january",
            "february",
            "march",
            "april",
            "june",
            "july",
            "august",
            "september",
            "october",
            "november",
            "december",
        ]);
        s
    };
    static ref PERSON_CONTEXT_PATTERNS: Vec<&'static str> = vec![
        r"\b{name}\s+said\b",
        r"\b{name}\s+told\b",
        r"\b{name}\s+asked\b",
        r"\b{name}\s+laughed\b",
        r"\b{name}\s+smiled\b",
        r"\b{name}\s+was\b",
        r"\b{name}\s+is\b",
        r"\b{name}\s+called\b",
        r"\b{name}\s+texted\b",
        r"\bwith\s+{name}\b",
        r"\bsaw\s+{name}\b",
        r"\bcalled\s+{name}\b",
        r"\btook\s+{name}\b",
        r"\bpicked\s+up\s+{name}\b",
        r"\bdrop(?:ped)?\s+(?:off\s+)?{name}\b",
        r"\b{name}(?:'s|s')\b",
        r"\bhey\s+{name}\b",
        r"\bthanks?\s+{name}\b",
        r"^{name}[:\s]",
        r"\bmy\s+(?:son|daughter|kid|child|brother|sister|friend|partner|colleague|coworker)\s+{name}\b",
    ];
    static ref CONCEPT_CONTEXT_PATTERNS: Vec<&'static str> = vec![
        r"\bhave\s+you\s+{name}\b",
        r"\bif\s+you\s+{name}\b",
        r"\b{name}\s+since\b",
        r"\b{name}\s+again\b",
        r"\bnot\s+{name}\b",
        r"\b{name}\s+more\b",
        r"\bwould\s+{name}\b",
        r"\bcould\s+{name}\b",
        r"\bwill\s+{name}\b",
        r"(?:the\s+)?{name}\s+(?:of|in|at|for|to)\b",
    ];
    static ref NAME_INDICATOR_PHRASES: Vec<&'static str> = vec![
        "given name",
        "personal name",
        "first name",
        "forename",
        "masculine name",
        "feminine name",
        "boy's name",
        "girl's name",
        "male name",
        "female name",
        "irish name",
        "welsh name",
        "scottish name",
        "gaelic name",
        "hebrew name",
        "arabic name",
        "norse name",
        "old english name",
        "is a name",
        "as a name",
        "name meaning",
        "name derived from",
        "legendary irish",
        "legendary welsh",
        "legendary scottish",
    ];
    static ref PLACE_INDICATOR_PHRASES: Vec<&'static str> = vec![
        "city in",
        "town in",
        "village in",
        "municipality",
        "capital of",
        "district of",
        "county",
        "province",
        "region of",
        "island of",
        "mountain in",
        "river in",
    ];
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct PersonInfo {
    pub source: String,
    pub contexts: Vec<String>,
    pub aliases: Vec<String>,
    pub relationship: String,
    pub confidence: f32,
    pub canonical: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct WikiResult {
    pub inferred_type: String, // "person", "place", "concept", "unknown", "ambiguous"
    pub confidence: f32,
    pub wiki_summary: Option<String>,
    pub wiki_title: Option<String>,
    pub confirmed: bool,
    pub word: String,
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RegistryData {
    pub version: u32,
    pub mode: String,
    pub people: HashMap<String, PersonInfo>,
    pub projects: Vec<String>,
    pub ambiguous_flags: Vec<String>,
    pub wiki_cache: HashMap<String, WikiResult>,
}

impl RegistryData {
    pub fn empty() -> Self {
        Self {
            version: 1,
            mode: "personal".to_string(),
            people: HashMap::new(),
            projects: Vec::new(),
            ambiguous_flags: Vec::new(),
            wiki_cache: HashMap::new(),
        }
    }
}

pub struct EntityRegistry {
    pub path: PathBuf,
    pub data: RegistryData,
    client: Client,
}

impl EntityRegistry {
    pub fn new(path: Option<PathBuf>) -> Self {
        let path = path.unwrap_or_else(|| {
            let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
            PathBuf::from(home)
                .join(".mempalace")
                .join("entity_registry.json")
        });

        let mut registry = Self {
            path,
            data: RegistryData::empty(),
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .unwrap_or_else(|_| Client::new()),
        };

        let _ = registry.load();
        registry
    }

    pub fn load(&mut self) -> Result<()> {
        if self.path.exists() {
            let content = fs::read_to_string(&self.path)?;
            self.data = serde_json::from_str(&content)?;
        }
        Ok(())
    }

    pub fn save(&self) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let content = serde_json::to_string_pretty(&self.data)?;
        fs::write(&self.path, content)?;
        Ok(())
    }

    pub fn get_canonical_name(&self, name: &str) -> Option<String> {
        let name_lower = name.to_lowercase();

        // 1. Direct match in people
        for (canonical, info) in &self.data.people {
            if canonical.to_lowercase() == name_lower {
                return Some(canonical.clone());
            }
            if info.aliases.iter().any(|a| a.to_lowercase() == name_lower) {
                return Some(canonical.clone());
            }
        }

        // 2. Match in projects
        for project in &self.data.projects {
            if project.to_lowercase() == name_lower {
                return Some(project.clone());
            }
        }

        None
    }

    pub fn register_entity(&mut self, entity: &DetectedEntity) {
        match entity.r#type {
            EntityType::Person => {
                let name = &entity.name;
                let entry = self
                    .data
                    .people
                    .entry(name.clone())
                    .or_insert_with(|| PersonInfo {
                        source: "learned".to_string(),
                        contexts: vec![self.data.mode.clone()],
                        aliases: entity.aliases.clone(),
                        relationship: entity.relationship.clone().unwrap_or_default(),
                        confidence: entity.confidence,
                        canonical: None,
                    });

                if entity.confidence > entry.confidence {
                    entry.confidence = entity.confidence;
                }

                for alias in &entity.aliases {
                    if !entry.aliases.contains(alias) {
                        entry.aliases.push(alias.clone());
                    }
                }

                if COMMON_ENGLISH_WORDS.contains(name.to_lowercase().as_str()) {
                    let name_lower = name.to_lowercase();
                    if !self.data.ambiguous_flags.contains(&name_lower) {
                        self.data.ambiguous_flags.push(name_lower);
                    }
                }
            }
            EntityType::Project => {
                if !self.data.projects.contains(&entity.name) {
                    self.data.projects.push(entity.name.clone());
                }
            }
            _ => {}
        }
        let _ = self.save();
    }

    pub fn lookup(&self, name: &str, context: Option<&str>) -> Option<EntityType> {
        let name_lower = name.to_lowercase();

        // Check if ambiguous
        if self.data.ambiguous_flags.contains(&name_lower) {
            if let Some(ctx) = context {
                return self.disambiguate(name, ctx);
            }
        }

        if self
            .data
            .people
            .keys()
            .any(|k| k.to_lowercase() == name_lower)
        {
            return Some(EntityType::Person);
        }

        // Check aliases
        for info in self.data.people.values() {
            if info.aliases.iter().any(|a| a.to_lowercase() == name_lower) {
                return Some(EntityType::Person);
            }
        }

        if self
            .data
            .projects
            .iter()
            .any(|p| p.to_lowercase() == name_lower)
        {
            return Some(EntityType::Project);
        }

        // Check wiki cache
        if let Some(res) = self.data.wiki_cache.get(name) {
            if res.confirmed {
                return match res.inferred_type.as_str() {
                    "person" => Some(EntityType::Person),
                    "project" => Some(EntityType::Project),
                    _ => Some(EntityType::Term),
                };
            }
        }

        None
    }

    fn disambiguate(&self, name: &str, context: &str) -> Option<EntityType> {
        let name_escaped = regex::escape(name);
        let ctx_lower = context.to_lowercase();

        let mut person_score = 0;
        for pat in PERSON_CONTEXT_PATTERNS.iter() {
            let re = Regex::new(&format!("(?i){}", pat.replace("{name}", &name_escaped))).unwrap();
            if re.is_match(&ctx_lower) {
                person_score += 1;
            }
        }

        let mut concept_score = 0;
        for pat in CONCEPT_CONTEXT_PATTERNS.iter() {
            let re = Regex::new(&format!("(?i){}", pat.replace("{name}", &name_escaped))).unwrap();
            if re.is_match(&ctx_lower) {
                concept_score += 1;
            }
        }

        if person_score > concept_score {
            Some(EntityType::Person)
        } else if concept_score > person_score {
            Some(EntityType::Term)
        } else {
            None
        }
    }

    pub async fn research_wikipedia(
        &mut self,
        word: &str,
        auto_confirm: bool,
    ) -> Result<WikiResult> {
        if let Some(res) = self.data.wiki_cache.get(word) {
            return Ok(res.clone());
        }

        let mut url = reqwest::Url::parse("https://en.wikipedia.org/api/rest_v1/page/summary/")?;
        url.path_segments_mut()
            .map_err(|_| anyhow!("Invalid URL path"))?
            .push(word);

        let resp = self
            .client
            .get(url)
            .header("User-Agent", "MemPalace/1.0")
            .send()
            .await?;

        if resp.status().as_u16() == 404 {
            let result = WikiResult {
                inferred_type: "person".to_string(),
                confidence: 0.70,
                wiki_summary: None,
                wiki_title: None,
                confirmed: auto_confirm,
                word: word.to_string(),
                note: Some("not found in Wikipedia — likely a proper noun".to_string()),
            };
            self.data
                .wiki_cache
                .insert(word.to_string(), result.clone());
            self.save()?;
            return Ok(result);
        }

        if !resp.status().is_success() {
            return Err(anyhow!("Wikipedia API error: {}", resp.status()));
        }

        let data: serde_json::Value = resp.json().await?;
        let page_type = data["type"].as_str().unwrap_or("");
        let extract = data["extract"].as_str().unwrap_or("").to_lowercase();
        let title = data["title"].as_str().map(|s| s.to_string());

        let mut inferred_type = "concept".to_string();
        let mut confidence = 0.60;
        let mut note = None;

        if page_type == "disambiguation" {
            let desc = data["description"].as_str().unwrap_or("").to_lowercase();
            if desc.contains("name") || desc.contains("given name") {
                inferred_type = "person".to_string();
                confidence = 0.65;
                note = Some("disambiguation page with name entries".to_string());
            } else {
                inferred_type = "ambiguous".to_string();
                confidence = 0.4;
            }
        } else if NAME_INDICATOR_PHRASES.iter().any(|p| extract.contains(p)) {
            inferred_type = "person".to_string();
            confidence = if extract.contains(&format!("{} is a", word.to_lowercase()))
                || extract.contains(&format!("{} (name", word.to_lowercase()))
            {
                0.90
            } else {
                0.80
            };
        } else if PLACE_INDICATOR_PHRASES.iter().any(|p| extract.contains(p)) {
            inferred_type = "place".to_string();
            confidence = 0.80;
        }

        let result = WikiResult {
            inferred_type,
            confidence,
            wiki_summary: Some(data["extract"].as_str().unwrap_or("").to_string()),
            wiki_title: title,
            confirmed: auto_confirm,
            word: word.to_string(),
            note,
        };

        self.data
            .wiki_cache
            .insert(word.to_string(), result.clone());
        self.save()?;
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_registration_and_persistence() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("entity_registry.json");
        let mut registry = EntityRegistry::new(Some(path.clone()));

        let entity = DetectedEntity {
            name: "Riley".to_string(),
            unique_id: None,
            r#type: EntityType::Person,
            confidence: 1.0,
            signals: vec!["manual".to_string()],
            aliases: vec!["Ry".to_string()],
            relationship: Some("daughter".to_string()),
        };

        registry.register_entity(&entity);
        assert!(registry.data.people.contains_key("Riley"));
        assert_eq!(registry.data.people["Riley"].relationship, "daughter");
        assert!(registry.data.people["Riley"]
            .aliases
            .contains(&"Ry".to_string()));

        // Test persistence
        let registry2 = EntityRegistry::new(Some(path));
        assert!(registry2.data.people.contains_key("Riley"));
        assert_eq!(registry2.data.people["Riley"].relationship, "daughter");
    }

    #[test]
    fn test_alias_resolution() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("entity_registry.json");
        let mut registry = EntityRegistry::new(Some(path));

        let entity = DetectedEntity {
            name: "Maxwell".to_string(),
            unique_id: None,
            r#type: EntityType::Person,
            confidence: 1.0,
            signals: vec![],
            aliases: vec!["Max".to_string()],
            relationship: None,
        };

        registry.register_entity(&entity);
        assert_eq!(
            registry.get_canonical_name("Max"),
            Some("Maxwell".to_string())
        );
        assert_eq!(
            registry.get_canonical_name("Maxwell"),
            Some("Maxwell".to_string())
        );
        assert_eq!(registry.get_canonical_name("Unknown"), None);
    }

    #[test]
    fn test_disambiguation() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("entity_registry.json");
        let mut registry = EntityRegistry::new(Some(path));

        // "Grace" is a common word
        let entity = DetectedEntity {
            name: "Grace".to_string(),
            unique_id: None,
            r#type: EntityType::Person,
            confidence: 1.0,
            signals: vec![],
            aliases: vec![],
            relationship: None,
        };
        registry.register_entity(&entity);

        assert!(registry.data.ambiguous_flags.contains(&"grace".to_string()));

        // Person context
        let person_ctx = "I went to the park with Grace today.";
        assert_eq!(
            registry.lookup("Grace", Some(person_ctx)),
            Some(EntityType::Person)
        );

        // Concept context
        let concept_ctx = "The grace of the dancer was amazing.";
        assert_eq!(
            registry.lookup("Grace", Some(concept_ctx)),
            Some(EntityType::Term)
        );
    }
}
