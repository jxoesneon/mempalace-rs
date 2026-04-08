use mempalace_rs::dialect::{AAAKContext, Dialect};
use std::collections::HashMap;

#[test]
fn test_dialect_compress() {
    let dialect = Dialect::default();
    let text = "Alice decided to switch to Rust because of performance.";
    let compressed = dialect.compress(text, None);

    assert!(compressed.contains("DECISION"));

    let text2 = "Yesterday, Alice decided to switch to Rust.";
    let compressed2 = dialect.compress(text2, None);
    assert!(compressed2.contains("ALI")); // ALI from Alice (first 3 chars)
}

#[test]
fn test_emotion_detection() {
    let dialect = Dialect::default();
    let text = "I am so worried about the future, but also excited for the new release.";
    let compressed = dialect.compress(text, None);

    assert!(compressed.contains("anx"));
    assert!(compressed.contains("excite"));
}

#[test]
fn test_topic_extraction() {
    let dialect = Dialect::default();
    let text = "The technical implementation of the database architecture was critical.";
    let compressed = dialect.compress(text, None);

    assert!(compressed.contains("technical"));
    assert!(compressed.contains("implementation"));
}

#[test]
fn test_entity_mapping() {
    let mut entities = HashMap::new();
    entities.insert("Alice".to_string(), "ALC".to_string());
    entities.insert("Bob".to_string(), "BOB".to_string());
    let dialect = Dialect::new(Some(entities), Some(vec!["ignoreme".to_string()]));

    let text = "Alice said she prefers Rust. bob was there too.";
    let compressed = dialect.compress(text, None);
    assert!(compressed.contains("ALC"));
    assert!(compressed.contains("BOB"));

    // Check skipped entity
    assert_eq!(dialect.encode_entity("ignoreme"), None);

    // Check fallback logic for entity coding
    assert_eq!(dialect.encode_entity("Charlie").unwrap(), "CHA");
    assert_eq!(dialect.encode_entity("Al").unwrap(), "ALC");
}

#[test]
fn test_compress_with_metadata() {
    let dialect = Dialect::default();
    let mut metadata = HashMap::new();
    metadata.insert("source_file".to_string(), "test.md".to_string());
    metadata.insert("wing".to_string(), "technical".to_string());
    metadata.insert("room".to_string(), "rust".to_string());
    metadata.insert("date".to_string(), "2024-01-01".to_string());

    let compressed = dialect.compress("This is a test of metadata inclusion.", Some(metadata));
    assert!(compressed.contains("technical|rust|2024-01-01|test"));
}

#[test]
fn test_compress_empty() {
    let dialect = Dialect::default();
    let compressed = dialect.compress("", None);
    assert!(compressed.contains("0:???")); // No entities
    assert!(compressed.contains("misc")); // No topics
}

#[test]
fn test_encode_emotions() {
    let dialect = Dialect::default();
    let code = dialect.encode_emotions(&[
        "vulnerability".to_string(),
        "happy".to_string(),
        "unknown_long".to_string(),
        "shrt".to_string(),
    ]);
    assert!(code.contains("vul"));
    assert!(code.contains("unkn"));
    // "shrt" is the 4th emotion, so it's truncated by take(3)
    assert!(!code.contains("shrt"));
}

#[test]
fn test_decode() {
    let dialect = Dialect::default();
    let compressed = "ARC: The grand journey\ntechnical|rust|2024-01-01|test\n0:ALI|rust|\"We decided to rewrite it in Rust.\"\nT:Link to another note\nExtra line for zettels";
    let decoded = dialect.decode(compressed);

    assert_eq!(decoded["arc"].as_str().unwrap(), " The grand journey");
    assert_eq!(
        decoded["tunnels"][0].as_str().unwrap(),
        "T:Link to another note"
    );
    assert_eq!(decoded["header"]["wing"].as_str().unwrap(), "technical");
    assert_eq!(decoded["header"]["room"].as_str().unwrap(), "rust");
    assert_eq!(decoded["header"]["date"].as_str().unwrap(), "2024-01-01");
    assert_eq!(decoded["header"]["title"].as_str().unwrap(), "test");
    assert_eq!(
        decoded["zettels"][0].as_str().unwrap(),
        "0:ALI|rust|\"We decided to rewrite it in Rust.\""
    );
}

#[test]
fn test_decode_empty() {
    let dialect = Dialect::default();
    let decoded = dialect.decode("");
    assert_eq!(decoded["arc"], "");
    assert_eq!(decoded["zettels"].as_array().unwrap().len(), 0);
}

#[test]
fn test_aaak_context() {
    let compressed = AAAKContext::compress("A very simple sentence about things.");
    assert!(!compressed.is_empty());
}
