use crate::entity_registry::EntityRegistry;
use crate::models::{DetectedEntity, EntityType};
use anyhow::Result;
use dialoguer::{theme::ColorfulTheme, Input, Select};
use std::fs;
use std::path::PathBuf;

pub fn run_onboarding() -> Result<()> {
    let theme = ColorfulTheme::default();

    println!("Welcome to MemPalace! Let's set up your memory palace.");

    let mode_options = vec!["Work", "Personal", "Combo"];
    let mode_idx = Select::with_theme(&theme)
        .with_prompt("Choose your primary Mode")
        .items(&mode_options)
        .default(1)
        .interact()?;
    let mode = mode_options[mode_idx].to_lowercase();

    let people_input: String = Input::with_theme(&theme)
        .with_prompt("Who are the key People in your life? (comma separated)")
        .interact_text()?;
    let people: Vec<String> = people_input
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    let projects_input: String = Input::with_theme(&theme)
        .with_prompt("What Projects are you currently working on? (comma separated)")
        .interact_text()?;
    let projects: Vec<String> = projects_input
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    let wings_input: String = Input::with_theme(&theme)
        .with_prompt("Any specific Wings (categories) you want to track? (comma separated)")
        .interact_text()?;
    let wings: Vec<String> = wings_input
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    println!("Great! Bootstrapping your memory...");

    let mut registry = EntityRegistry::new(None);
    registry.data.mode = mode;

    for person in &people {
        registry.register_entity(&DetectedEntity {
            name: person.clone(),
            r#type: EntityType::Person,
            confidence: 1.0,
            signals: vec!["onboarding".to_string()],
            aliases: vec![],
            relationship: None,
        });
    }

    for project in &projects {
        registry.register_entity(&DetectedEntity {
            name: project.clone(),
            r#type: EntityType::Project,
            confidence: 1.0,
            signals: vec!["onboarding".to_string()],
            aliases: vec![],
            relationship: None,
        });
    }

    bootstrap_files(&people, &projects, &wings, None)?;

    println!("Onboarding complete! Your palace is ready.");
    Ok(())
}

pub fn bootstrap_files(
    people: &[String],
    projects: &[String],
    wings: &[String],
    base_path: Option<PathBuf>,
) -> Result<()> {
    let mempalace_dir = match base_path {
        Some(p) => p,
        None => {
            let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
            PathBuf::from(home).join(".mempalace")
        }
    };

    fs::create_dir_all(&mempalace_dir)?;

    let mut aaak_entities = String::from("# AAAK Entities Registry\n\n");
    aaak_entities.push_str("## People\n");
    for person in people {
        let code = generate_aaak_code(person);
        aaak_entities.push_str(&format!(
            "- **{}** ({}) - Relationship: Unknown\n",
            person, code
        ));
    }

    aaak_entities.push_str("\n## Projects\n");
    for project in projects {
        let code = generate_aaak_code(project);
        aaak_entities.push_str(&format!("- **{}** ({}) - Status: Active\n", project, code));
    }

    let aaak_path = mempalace_dir.join("aaak_entities.md");
    fs::write(aaak_path, aaak_entities)?;

    let mut critical_facts = String::from("# Critical Facts\n\n");
    critical_facts.push_str("## Important Wings\n");
    for wing in wings {
        critical_facts.push_str(&format!("- {}\n", wing));
    }

    critical_facts.push_str("\n## Ground Truths\n");
    critical_facts
        .push_str("- The user is setting up MemPalace to provide context for AI agents.\n");

    let facts_path = mempalace_dir.join("critical_facts.md");
    fs::write(facts_path, critical_facts)?;

    Ok(())
}

fn generate_aaak_code(name: &str) -> String {
    let stripped: String = name.chars().filter(|c| c.is_alphabetic()).collect();
    if stripped.len() >= 3 {
        stripped[..3].to_uppercase()
    } else {
        let mut code = stripped.to_uppercase();
        while code.len() < 3 {
            code.push('X');
        }
        code
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_aaak_code_generation() {
        assert_eq!(generate_aaak_code("Alice"), "ALI");
        assert_eq!(generate_aaak_code("Jo"), "JOX");
        assert_eq!(generate_aaak_code("A"), "AXX");
        assert_eq!(generate_aaak_code("Project X"), "PRO");
    }

    #[test]
    fn test_bootstrap_files() {
        let dir = tempdir().unwrap();
        let path = dir.path().to_path_buf();

        let people = vec!["Alice".to_string(), "Bob".to_string()];
        let projects = vec!["MemPalace".to_string()];
        let wings = vec!["Work".to_string()];

        bootstrap_files(&people, &projects, &wings, Some(path.clone())).unwrap();

        let aaak_content = fs::read_to_string(path.join("aaak_entities.md")).unwrap();
        assert!(aaak_content.contains("Alice"));
        assert!(aaak_content.contains("ALI"));
        assert!(aaak_content.contains("Bob"));
        assert!(aaak_content.contains("BOB"));
        assert!(aaak_content.contains("MemPalace"));
        assert!(aaak_content.contains("MEM"));

        let facts_content = fs::read_to_string(path.join("critical_facts.md")).unwrap();
        assert!(facts_content.contains("Work"));
        assert!(facts_content.contains("Ground Truths"));
    }
}
