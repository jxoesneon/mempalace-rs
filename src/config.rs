use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

pub const DEFAULT_PALACE_PATH: &str = "~/.mempalace/palace";
pub const DEFAULT_COLLECTION_NAME: &str = "mempalace_drawers";

pub fn default_topic_wings() -> Vec<String> {
    vec![
        "emotions".into(),
        "consciousness".into(),
        "memory".into(),
        "technical".into(),
        "identity".into(),
        "family".into(),
        "creative".into(),
    ]
}

pub fn default_hall_keywords() -> HashMap<String, Vec<String>> {
    let mut m = HashMap::new();
    m.insert(
        "emotions".into(),
        vec![
            "scared".into(),
            "afraid".into(),
            "worried".into(),
            "happy".into(),
            "sad".into(),
            "love".into(),
            "hate".into(),
            "feel".into(),
            "cry".into(),
            "tears".into(),
        ],
    );
    m.insert(
        "consciousness".into(),
        vec![
            "consciousness".into(),
            "conscious".into(),
            "aware".into(),
            "real".into(),
            "genuine".into(),
            "soul".into(),
            "exist".into(),
            "alive".into(),
        ],
    );
    m.insert(
        "memory".into(),
        vec![
            "memory".into(),
            "remember".into(),
            "forget".into(),
            "recall".into(),
            "archive".into(),
            "palace".into(),
            "store".into(),
        ],
    );
    m.insert(
        "technical".into(),
        vec![
            "code".into(),
            "python".into(),
            "script".into(),
            "bug".into(),
            "error".into(),
            "function".into(),
            "api".into(),
            "database".into(),
            "server".into(),
        ],
    );
    m.insert(
        "identity".into(),
        vec![
            "identity".into(),
            "name".into(),
            "who am i".into(),
            "persona".into(),
            "self".into(),
        ],
    );
    m.insert(
        "family".into(),
        vec![
            "family".into(),
            "kids".into(),
            "children".into(),
            "daughter".into(),
            "son".into(),
            "parent".into(),
            "mother".into(),
            "father".into(),
        ],
    );
    m.insert(
        "creative".into(),
        vec![
            "game".into(),
            "gameplay".into(),
            "player".into(),
            "app".into(),
            "design".into(),
            "art".into(),
            "music".into(),
            "story".into(),
        ],
    );
    m
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct MempalaceConfig {
    #[serde(skip)]
    pub config_dir: PathBuf,
    pub palace_path: String,
    pub collection_name: String,
    pub topic_wings: Vec<String>,
    pub hall_keywords: HashMap<String, Vec<String>>,
    pub people_map: HashMap<String, String>,
    /// Phase 4: optional path to an external emotions.json file.
    /// Format: `{"joy": "joy", "custom_emotion": "cst", ...}`
    /// When present, entries are merged on top of the built-in emotion codes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub emotions_path: Option<PathBuf>,
}

impl Default for MempalaceConfig {
    fn default() -> Self {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
        let config_dir = PathBuf::from(&home).join(".mempalace");

        let mut config = Self {
            config_dir,
            palace_path: DEFAULT_PALACE_PATH.replace("~", &home),
            collection_name: DEFAULT_COLLECTION_NAME.to_string(),
            topic_wings: default_topic_wings(),
            hall_keywords: default_hall_keywords(),
            people_map: HashMap::new(),
            emotions_path: None,
        };

        config.load_from_file();
        config.apply_env_overrides();
        config
    }
}

/// Expand a leading `~` or `~/` in a path string using the `HOME` env var.
/// Paths that do not start with `~` are returned unchanged.
fn expand_tilde(path: &str) -> String {
    if path == "~" {
        return std::env::var("HOME").unwrap_or_else(|_| path.to_string());
    }
    if let Some(rest) = path.strip_prefix("~/") {
        if let Ok(home) = std::env::var("HOME") {
            return format!("{home}/{rest}");
        }
    }
    path.to_string()
}

impl MempalaceConfig {
    pub fn new(config_dir: Option<PathBuf>) -> Self {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
        let dir = config_dir.unwrap_or_else(|| PathBuf::from(&home).join(".mempalace"));

        let mut config = Self {
            config_dir: dir.clone(),
            palace_path: DEFAULT_PALACE_PATH.replace("~", &home),
            collection_name: DEFAULT_COLLECTION_NAME.to_string(),
            topic_wings: default_topic_wings(),
            hall_keywords: default_hall_keywords(),
            people_map: HashMap::new(),
            emotions_path: None,
        };

        config.load_from_file();
        config.load_people_map();
        config.apply_env_overrides();
        config
    }

    fn load_from_file(&mut self) {
        let config_file = self.config_dir.join("config.json");
        if config_file.exists() {
            if let Ok(content) = fs::read_to_string(config_file) {
                if let Ok(file_config) = serde_json::from_str::<serde_json::Value>(&content) {
                    if let Some(path) = file_config.get("palace_path").and_then(|v| v.as_str()) {
                        self.palace_path = path.to_string();
                    }
                    if let Some(name) = file_config.get("collection_name").and_then(|v| v.as_str())
                    {
                        self.collection_name = name.to_string();
                    }
                    if let Some(wings) = file_config.get("topic_wings").and_then(|v| v.as_array()) {
                        self.topic_wings = wings
                            .iter()
                            .filter_map(|v| v.as_str())
                            .map(|s| s.to_string())
                            .collect();
                    }
                    if let Some(keywords) =
                        file_config.get("hall_keywords").and_then(|v| v.as_object())
                    {
                        for (k, v) in keywords {
                            if let Some(words) = v.as_array() {
                                let words_vec: Vec<String> = words
                                    .iter()
                                    .filter_map(|w| w.as_str())
                                    .map(|s| s.to_string())
                                    .collect();
                                self.hall_keywords.insert(k.clone(), words_vec);
                            }
                        }
                    }
                    if let Some(p_map) = file_config.get("people_map").and_then(|v| v.as_object()) {
                        for (k, v) in p_map {
                            if let Some(name) = v.as_str() {
                                self.people_map.insert(k.clone(), name.to_string());
                            }
                        }
                    }
                    if let Some(e_path) = file_config.get("emotions_path").and_then(|v| v.as_str())
                    {
                        self.emotions_path = Some(PathBuf::from(e_path));
                    }
                }
            }
        }
    }

    fn load_people_map(&mut self) {
        let people_map_file = self.config_dir.join("people_map.json");
        if people_map_file.exists() {
            if let Ok(content) = fs::read_to_string(people_map_file) {
                if let Ok(map) = serde_json::from_str::<HashMap<String, String>>(&content) {
                    self.people_map.extend(map);
                }
            }
        }
    }

    fn apply_env_overrides(&mut self) {
        if let Ok(val) =
            std::env::var("MEMPALACE_PALACE_PATH").or_else(|_| std::env::var("MEMPAL_PALACE_PATH"))
        {
            let path = PathBuf::from(expand_tilde(&val));
            if let Ok(canonical) = path.canonicalize() {
                if canonical.is_dir() {
                    self.palace_path = canonical.to_string_lossy().into_owned();
                }
            }
        }
        // Phase 4: allow overriding emotions file path via env var
        if let Ok(val) = std::env::var("MEMPALACE_EMOTIONS_PATH") {
            let path = PathBuf::from(expand_tilde(&val));
            if let Ok(canonical) = path.canonicalize() {
                if canonical.is_file() {
                    self.emotions_path = Some(canonical);
                }
            }
        }
    }

    /// Phase 4: load external emotion name→code mappings from `emotions.json`.
    /// Returns an empty map if the file is absent or malformed (graceful degradation).
    pub fn load_emotions_map(&self) -> HashMap<String, String> {
        // Prefer explicitly set path, then default location
        let path = self
            .emotions_path
            .clone()
            .unwrap_or_else(|| self.config_dir.join("emotions.json"));

        if !path.exists() {
            return HashMap::new();
        }

        match fs::read_to_string(&path) {
            Ok(content) => {
                serde_json::from_str::<HashMap<String, String>>(&content).unwrap_or_default()
            }
            Err(_) => HashMap::new(),
        }
    }

    pub fn init(&self) -> Result<PathBuf, std::io::Error> {
        fs::create_dir_all(&self.config_dir)?;
        let config_file = self.config_dir.join("config.json");
        if !config_file.exists() {
            let config_json = serde_json::to_string_pretty(self).unwrap();
            fs::write(&config_file, config_json)?;
        }
        Ok(config_file)
    }

    pub fn save_people_map(
        &self,
        people_map: &HashMap<String, String>,
    ) -> Result<PathBuf, std::io::Error> {
        fs::create_dir_all(&self.config_dir)?;
        let people_map_file = self.config_dir.join("people_map.json");
        fs::write(
            &people_map_file,
            serde_json::to_string_pretty(people_map).unwrap(),
        )?;
        Ok(people_map_file)
    }
}
