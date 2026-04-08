use mempalace_rs::config::MempalaceConfig;
use std::collections::HashMap;
use std::fs;
use tempfile::tempdir;

#[test]
fn test_config_new_with_dir() {
    let dir = tempdir().unwrap();
    let config_dir = dir.path().to_path_buf();

    let config = MempalaceConfig::new(Some(config_dir.clone()));
    assert_eq!(config.config_dir, config_dir);
}

#[test]
fn test_config_load_from_file() {
    let dir = tempdir().unwrap();
    let config_dir = dir.path().to_path_buf();

    let config_json = serde_json::json!({
        "palace_path": "/tmp/custom_palace",
        "collection_name": "custom_collection",
        "topic_wings": ["custom_wing"],
        "hall_keywords": {
            "custom_wing": ["word1", "word2"]
        }
    });

    fs::write(
        config_dir.join("config.json"),
        serde_json::to_string(&config_json).unwrap(),
    )
    .unwrap();

    let config = MempalaceConfig::new(Some(config_dir));
    assert_eq!(config.palace_path, "/tmp/custom_palace");
    assert_eq!(config.collection_name, "custom_collection");
    assert_eq!(config.topic_wings, vec!["custom_wing".to_string()]);
    assert_eq!(
        config.hall_keywords.get("custom_wing").unwrap(),
        &vec!["word1".to_string(), "word2".to_string()]
    );
}

#[test]
fn test_config_partial_json() {
    std::env::remove_var("MEMPALACE_PALACE_PATH");
    std::env::remove_var("MEMPAL_PALACE_PATH");
    let dir = tempdir().unwrap();
    let config_dir = dir.path().to_path_buf();

    let config_json = serde_json::json!({
        "palace_path": "/tmp/partial_palace"
    });

    fs::write(
        config_dir.join("config.json"),
        serde_json::to_string(&config_json).unwrap(),
    )
    .unwrap();

    let config = MempalaceConfig::new(Some(config_dir));
    assert_eq!(config.palace_path, "/tmp/partial_palace");
    assert_eq!(config.collection_name, "mempalace_drawers");
}

#[test]
fn test_config_malformed_json() {
    let dir = tempdir().unwrap();
    let config_dir = dir.path().to_path_buf();
    fs::write(config_dir.join("config.json"), "{ invalid }").unwrap();
    let config = MempalaceConfig::new(Some(config_dir));
    assert!(config.palace_path.contains(".mempalace/palace"));
}

#[test]
fn test_config_null_json() {
    let dir = tempdir().unwrap();
    let config_dir = dir.path().to_path_buf();
    fs::write(config_dir.join("config.json"), "null").unwrap();
    let config = MempalaceConfig::new(Some(config_dir));
    assert!(config.palace_path.contains(".mempalace/palace"));
}

#[test]
fn test_config_unreadable_file() {
    let dir = tempdir().unwrap();
    let config_dir = dir.path().to_path_buf();
    let file_path = config_dir.join("config.json");
    fs::write(&file_path, "{}").unwrap();

    use std::os::unix::fs::PermissionsExt;
    let mut perms = fs::metadata(&file_path).unwrap().permissions();
    perms.set_mode(0o000);
    fs::set_permissions(&file_path, perms).unwrap();

    let config = MempalaceConfig::new(Some(config_dir));
    assert!(config.palace_path.contains(".mempalace/palace"));

    let mut perms = fs::metadata(&file_path).unwrap().permissions();
    perms.set_mode(0o644);
    fs::set_permissions(&file_path, perms).unwrap();
}

#[test]
fn test_config_load_people_map() {
    let dir = tempdir().unwrap();
    let config_dir = dir.path().to_path_buf();

    let mut people_map = HashMap::new();
    people_map.insert("Alice".to_string(), "ALC".to_string());

    fs::write(
        config_dir.join("people_map.json"),
        serde_json::to_string(&people_map).unwrap(),
    )
    .unwrap();

    let config = MempalaceConfig::new(Some(config_dir));
    assert_eq!(config.people_map.get("Alice").unwrap(), "ALC");
}

#[test]
fn test_config_init_and_save() {
    let dir = tempdir().unwrap();
    let config_dir = dir.path().to_path_buf();
    let config = MempalaceConfig::new(Some(config_dir.clone()));

    let config_file = config.init().unwrap();
    assert!(config_file.exists());

    let mut people_map = HashMap::new();
    people_map.insert("Bob".to_string(), "BOB".to_string());
    people_map.insert("Charlie".to_string(), "CHA".to_string());
    let people_file = config.save_people_map(&people_map).unwrap();
    assert!(people_file.exists());
    let content = fs::read_to_string(&people_file).unwrap();
    assert!(content.contains("Bob"));
    assert!(content.contains("Charlie"));

    let empty_map = HashMap::new();
    let _ = config.save_people_map(&empty_map).unwrap();
    let content2 = fs::read_to_string(&people_file).unwrap();
    assert!(!content2.contains("Bob"));
}
