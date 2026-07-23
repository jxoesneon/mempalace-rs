//! Closet LLM — per-palace local LLM configuration and persona templates.
//!
//! A "closet" is a small, private store of LLM settings and system prompts that
//! travel with the palace. This module lets users pick a default local model,
//! save system prompts, and switch between personas without leaving the local
//! machine.

use crate::config::MempalaceConfig;
use crate::shared::{load_json_file, save_json_file};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// A named persona / system prompt.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Persona {
    pub name: String,
    pub system_prompt: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

impl Persona {
    pub fn new(name: impl Into<String>, system_prompt: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            system_prompt: system_prompt.into(),
            description: None,
        }
    }

    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }
}

/// LLM configuration saved with the palace.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ClosetLlmConfig {
    pub default_provider: String,
    pub default_model: String,
    pub local_endpoint: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
}

impl Default for ClosetLlmConfig {
    fn default() -> Self {
        Self {
            default_provider: "ollama".to_string(),
            default_model: "llama3.2".to_string(),
            local_endpoint: "http://localhost:11434".to_string(),
            api_key: None,
        }
    }
}

/// Closet LLM manager.
#[derive(Debug, Clone)]
pub struct ClosetLlm {
    dir: PathBuf,
}

impl ClosetLlm {
    pub fn new(config: &MempalaceConfig) -> Self {
        Self {
            dir: config.config_dir.join("closet_llm"),
        }
    }

    pub fn from_dir(dir: PathBuf) -> Self {
        Self { dir }
    }

    fn config_path(&self) -> PathBuf {
        self.dir.join("config.json")
    }

    fn personas_path(&self) -> PathBuf {
        self.dir.join("personas.json")
    }

    /// Load the closet config or return the default.
    pub fn load_config(&self) -> Result<ClosetLlmConfig> {
        let path = self.config_path();
        if !path.exists() {
            return Ok(ClosetLlmConfig::default());
        }
        load_json_file(&path).with_context(|| format!("loading closet config from {path:?}"))
    }

    /// Save the closet config.
    pub fn save_config(&self, config: &ClosetLlmConfig) -> Result<()> {
        save_json_file(&self.config_path(), config)
            .with_context(|| "saving closet config")
    }

    fn load_personas_map(&self) -> Result<HashMap<String, Persona>> {
        let path = self.personas_path();
        if !path.exists() {
            return Ok(HashMap::new());
        }
        load_json_file(&path).with_context(|| format!("loading personas from {path:?}"))
    }

    fn save_personas_map(&self, personas: &HashMap<String, Persona>) -> Result<()> {
        save_json_file(&self.personas_path(), personas)
            .with_context(|| "saving personas")
    }

    /// List all saved personas.
    pub fn list_personas(&self) -> Result<Vec<Persona>> {
        let mut out: Vec<Persona> = self.load_personas_map()?.into_values().collect();
        out.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(out)
    }

    /// Get a persona by name.
    pub fn get_persona(&self, name: &str) -> Result<Option<Persona>> {
        Ok(self.load_personas_map()?.get(name).cloned())
    }

    /// Save or update a persona.
    pub fn save_persona(&self, persona: &Persona) -> Result<()> {
        let mut map = self.load_personas_map()?;
        map.insert(persona.name.clone(), persona.clone());
        self.save_personas_map(&map)
    }

    /// Delete a persona by name.
    pub fn delete_persona(&self, name: &str) -> Result<bool> {
        let mut map = self.load_personas_map()?;
        let removed = map.remove(name).is_some();
        if removed {
            self.save_personas_map(&map)?;
        }
        Ok(removed)
    }

    /// Get the default system prompt, if any.
    pub fn default_system_prompt(&self) -> Result<Option<String>> {
        let config = self.load_config()?;
        let personas = self.load_personas_map()?;
        Ok(personas
            .get(&config.default_model)
            .or_else(|| personas.get("default"))
            .map(|p| p.system_prompt.clone()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn test_closet() -> ClosetLlm {
        ClosetLlm::from_dir(tempdir().unwrap().keep())
    }

    #[test]
    fn test_default_config() {
        let closet = test_closet();
        let config = closet.load_config().unwrap();
        assert_eq!(config.default_provider, "ollama");
        assert_eq!(config.local_endpoint, "http://localhost:11434");
    }

    #[test]
    fn test_save_and_load_config() {
        let closet = test_closet();
        let mut config = ClosetLlmConfig::default();
        config.default_model = "qwen2.5".to_string();
        closet.save_config(&config).unwrap();
        let loaded = closet.load_config().unwrap();
        assert_eq!(loaded.default_model, "qwen2.5");
    }

    #[test]
    fn test_save_and_get_persona() {
        let closet = test_closet();
        let p = Persona::new("coder", "You are a Rust expert.").with_description("dev");
        closet.save_persona(&p).unwrap();
        let loaded = closet.get_persona("coder").unwrap().unwrap();
        assert_eq!(loaded.system_prompt, "You are a Rust expert.");
    }

    #[test]
    fn test_list_personas() {
        let closet = test_closet();
        closet.save_persona(&Persona::new("b", "B")).unwrap();
        closet.save_persona(&Persona::new("a", "A")).unwrap();
        let list = closet.list_personas().unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].name, "a");
    }

    #[test]
    fn test_delete_persona() {
        let closet = test_closet();
        closet.save_persona(&Persona::new("x", "X")).unwrap();
        assert!(closet.delete_persona("x").unwrap());
        assert!(!closet.delete_persona("x").unwrap());
    }

    #[test]
    fn test_default_system_prompt() {
        let closet = test_closet();
        let mut config = ClosetLlmConfig::default();
        config.default_model = "default".to_string();
        closet.save_config(&config).unwrap();
        closet
            .save_persona(&Persona::new("default", "You are helpful."))
            .unwrap();
        assert_eq!(
            closet.default_system_prompt().unwrap().unwrap(),
            "You are helpful."
        );
    }
}
