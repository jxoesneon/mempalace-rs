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
        };

        config.load_from_file();
        config.apply_env_overrides();
        config
    }
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
                    if let Some(path) = file_config["palace_path"].as_str() {
                        self.palace_path = path.to_string();
                    }
                    if let Some(name) = file_config["collection_name"].as_str() {
                        self.collection_name = name.to_string();
                    }
                    if let Some(wings) = file_config["topic_wings"].as_array() {
                        self.topic_wings = wings
                            .iter()
                            .filter_map(|v| v.as_str())
                            .map(|s| s.to_string())
                            .collect();
                    }
                    if let Some(keywords) = file_config["hall_keywords"].as_object() {
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
                }
            }
        }
    }

    fn load_people_map(&mut self) {
        let people_map_file = self.config_dir.join("people_map.json");
        if people_map_file.exists() {
            if let Ok(content) = fs::read_to_string(people_map_file) {
                if let Ok(map) = serde_json::from_str::<HashMap<String, String>>(&content) {
                    self.people_map = map;
                }
            }
        }
    }

    fn apply_env_overrides(&mut self) {
        if let Ok(val) =
            std::env::var("MEMPALACE_PALACE_PATH").or_else(|_| std::env::var("MEMPAL_PALACE_PATH"))
        {
            self.palace_path = val;
        }
    }

    pub fn init(&self) -> Result<PathBuf, std::io::Error> {
        fs::create_dir_all(&self.config_dir)?;
        let config_file = self.config_dir.join("config.json");
        if !config_file.exists() {
            let default_config = serde_json::json!({
                "palace_path": self.palace_path,
                "collection_name": self.collection_name,
                "topic_wings": self.topic_wings,
                "hall_keywords": self.hall_keywords,
            });
            fs::write(
                &config_file,
                serde_json::to_string_pretty(&default_config).unwrap(),
            )?;
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
