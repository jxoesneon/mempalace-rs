//! Instructions CLI — manage per-agent and system instructions for the memory palace.
//!
//! This is a minimal Rust forward-port of the instruction/template management
//! surface found in upstream Python tooling. It stores text templates under the
//! configured MemPalace directory and lets users list, read, write, and delete
//! named instructions.

use crate::config::MempalaceConfig;
use crate::shared::{load_json_file, save_json_file};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

/// A single named instruction template.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Instruction {
    pub name: String,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
    pub updated_at: i64,
}

impl Instruction {
    pub fn new(name: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            content: content.into(),
            description: None,
            tags: None,
            updated_at: chrono::Utc::now().timestamp(),
        }
    }

    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn with_tags(mut self, tags: Vec<String>) -> Self {
        self.tags = Some(tags);
        self
    }
}

/// Instructions manager.
#[derive(Debug, Clone)]
pub struct InstructionsCli {
    dir: PathBuf,
}

impl InstructionsCli {
    pub fn new(config: &MempalaceConfig) -> Self {
        Self {
            dir: config.config_dir.join("instructions"),
        }
    }

    pub fn from_dir(dir: PathBuf) -> Self {
        Self { dir }
    }

    fn ensure_dir(&self) -> Result<()> {
        fs::create_dir_all(&self.dir).with_context(|| format!("creating {:?}", self.dir))?;
        Ok(())
    }

    fn path(&self, name: &str) -> PathBuf {
        self.dir.join(format!("{name}.txt"))
    }

    fn meta_path(&self, name: &str) -> PathBuf {
        self.dir.join(format!("{name}.json"))
    }

    /// Sanitize a name so it is safe for a filename.
    pub fn sanitize_name(name: &str) -> String {
        name.chars()
            .map(|c| {
                if c.is_alphanumeric() || c == '-' || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect::<String>()
            .trim_start_matches(|c: char| !c.is_alphanumeric())
            .to_string()
    }

    /// Save or update an instruction.
    pub fn save(&self, instruction: &Instruction) -> Result<()> {
        self.ensure_dir()?;
        let name = Self::sanitize_name(&instruction.name);
        let content_path = self.path(&name);
        let meta_path = self.meta_path(&name);
        fs::write(&content_path, &instruction.content)
            .with_context(|| format!("writing instruction content to {content_path:?}"))?;
        save_json_file(&meta_path, instruction)
            .with_context(|| format!("writing instruction meta to {meta_path:?}"))?;
        Ok(())
    }

    /// Load a named instruction.
    pub fn load(&self, name: &str) -> Result<Instruction> {
        let name = Self::sanitize_name(name);
        let meta_path = self.meta_path(&name);
        let content = fs::read_to_string(self.path(&name))
            .with_context(|| format!("reading instruction {name}"))?;
        let mut instruction: Instruction = if meta_path.exists() {
            load_json_file(&meta_path).with_context(|| format!("parsing meta for {name}"))?
        } else {
            Instruction::new(name.clone(), content.clone())
        };
        instruction.content = content;
        Ok(instruction)
    }

    /// List all stored instruction names.
    pub fn list(&self) -> Result<Vec<String>> {
        if !self.dir.exists() {
            return Ok(Vec::new());
        }
        let mut names = Vec::new();
        for entry in fs::read_dir(&self.dir)? {
            let entry = entry?;
            let path = entry.path();
            if let Some(ext) = path.extension() {
                if ext == "txt" {
                    if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                        names.push(stem.to_string());
                    }
                }
            }
        }
        names.sort();
        Ok(names)
    }

    /// Return all instructions as a map.
    pub fn load_all(&self) -> Result<HashMap<String, Instruction>> {
        let mut map = HashMap::new();
        for name in self.list()? {
            let inst = self.load(&name)?;
            map.insert(name, inst);
        }
        Ok(map)
    }

    /// Delete a named instruction.
    pub fn delete(&self, name: &str) -> Result<bool> {
        let name = Self::sanitize_name(name);
        let content_path = self.path(&name);
        let meta_path = self.meta_path(&name);
        let mut removed = false;
        if content_path.exists() {
            fs::remove_file(&content_path)?;
            removed = true;
        }
        if meta_path.exists() {
            fs::remove_file(&meta_path)?;
        }
        Ok(removed)
    }

    /// Get a rendered system prompt from an instruction by name, if it exists.
    pub fn render(&self, name: &str) -> Result<Option<String>> {
        if self.path(&Self::sanitize_name(name)).exists() {
            let inst = self.load(name)?;
            Ok(Some(inst.content))
        } else {
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn test_cli() -> InstructionsCli {
        InstructionsCli::from_dir(tempdir().unwrap().keep())
    }

    #[test]
    fn test_sanitize_name() {
        assert_eq!(InstructionsCli::sanitize_name("hello world"), "hello_world");
        assert_eq!(InstructionsCli::sanitize_name("!!!foo"), "foo");
    }

    #[test]
    fn test_save_and_load() {
        let cli = test_cli();
        let inst = Instruction::new("greeting", "Say hello.").with_description("greet");
        cli.save(&inst).unwrap();
        let loaded = cli.load("greeting").unwrap();
        assert_eq!(loaded.name, "greeting");
        assert_eq!(loaded.content, "Say hello.");
        assert_eq!(loaded.description, Some("greet".to_string()));
    }

    #[test]
    fn test_list() {
        let cli = test_cli();
        cli.save(&Instruction::new("a", "A")).unwrap();
        cli.save(&Instruction::new("b", "B")).unwrap();
        let names = cli.list().unwrap();
        assert_eq!(names, vec!["a", "b"]);
    }

    #[test]
    fn test_load_all() {
        let cli = test_cli();
        cli.save(&Instruction::new("x", "X")).unwrap();
        let all = cli.load_all().unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all["x"].content, "X");
    }

    #[test]
    fn test_delete() {
        let cli = test_cli();
        cli.save(&Instruction::new("d", "D")).unwrap();
        assert!(cli.delete("d").unwrap());
        assert!(!cli.delete("d").unwrap());
    }

    #[test]
    fn test_render_missing() {
        let cli = test_cli();
        assert!(cli.render("missing").unwrap().is_none());
    }

    #[test]
    fn test_render_existing() {
        let cli = test_cli();
        cli.save(&Instruction::new("sys", "You are helpful."))
            .unwrap();
        assert_eq!(cli.render("sys").unwrap().unwrap(), "You are helpful.");
    }

    #[test]
    fn test_load_without_meta() {
        let cli = test_cli();
        cli.ensure_dir().unwrap();
        fs::write(cli.path("raw"), "raw content").unwrap();
        let loaded = cli.load("raw").unwrap();
        assert_eq!(loaded.content, "raw content");
        assert_eq!(loaded.name, "raw");
    }
}
