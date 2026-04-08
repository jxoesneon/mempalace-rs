use mempalace_rs::dialect::Dialect;
use std::collections::HashMap;

#[test]
fn test_dialect_encode_entity() {
    let mut entities = HashMap::new();
    entities.insert("Alice".to_string(), "ALC".to_string());
    let dialect = Dialect::new(Some(entities), Some(vec!["SkipMe".to_string()]));

    assert_eq!(dialect.encode_entity("Alice"), Some("ALC".to_string()));
    assert_eq!(dialect.encode_entity("alice"), Some("ALC".to_string()));
    assert_eq!(
        dialect.encode_entity("Alice Smith"),
        Some("ALC".to_string())
    );
    assert_eq!(dialect.encode_entity("SkipMeNow"), None);
    assert_eq!(dialect.encode_entity("Bob"), Some("BOB".to_string())); // Auto-code
    assert_eq!(dialect.encode_entity("Xi"), Some("XI".to_string())); // Short auto-code
    assert_eq!(dialect.encode_entity("A"), Some("ALC".to_string())); // Substring match with Alice
    assert_eq!(dialect.encode_entity("!!!"), Some("!!!".to_string())); // Non-alphanumeric auto-code
    assert_eq!(dialect.encode_entity("ABC"), Some("ABC".to_string())); // 3-char auto-code
    assert_eq!(dialect.encode_entity("Test"), Some("TES".to_string())); // 3-char auto-code limit
}

#[test]
fn test_dialect_encode_emotions() {
    let dialect = Dialect::default();
    let emotions = vec![
        "vulnerability".to_string(),
        "joy".to_string(),
        "unknown".to_string(),
        "vulnerable".to_string(),
    ];
    let encoded = dialect.encode_emotions(&emotions);
    // encoded should be "vul+joy+unkn" (duplicates removed, limited to 3)
    assert!(encoded.contains("vul"));
    assert!(encoded.contains("joy"));
    assert!(encoded.contains("unkn"));
    assert_eq!(encoded.split('+').count(), 3);

    // Exactly 3
    let e3 = vec!["joy".to_string(), "anger".to_string(), "fear".to_string()];
    assert_eq!(dialect.encode_emotions(&e3), "joy+rage+fear");
}

#[test]
fn test_dialect_compress_with_metadata() {
    let dialect = Dialect::default();
    let mut metadata = HashMap::new();
    metadata.insert("wing".to_string(), "ProjectA".to_string());
    metadata.insert("room".to_string(), "General".to_string());
    metadata.insert("date".to_string(), "2026-04-07".to_string());
    metadata.insert("source_file".to_string(), "/path/to/file.txt".to_string());

    let text = "I decided to use Rust because it is fast and safe.";
    let compressed = dialect.compress(text, Some(metadata));
    println!("DIALECT COMPRESSED: {}", compressed);

    // Header check
    assert!(compressed.contains("ProjectA|General|2026-04-07|file"));
    // Content check
    assert!(compressed.contains("0:RUS"));
    assert!(compressed.contains("DECISION"));
}

#[test]
fn test_dialect_decode() {
    let dialect = Dialect::default();
    let dialect_text = "ProjectA|General|2026-04-07|file\nARC:Intro\nT:Tunnel1\n0:ALC|topic1|\"Quote\"|joy|DECISION";
    let decoded = dialect.decode(dialect_text);

    assert_eq!(decoded["header"]["wing"], "ProjectA");
    assert_eq!(decoded["arc"], "Intro");
    assert_eq!(decoded["tunnels"][0], "T:Tunnel1");
    assert_eq!(decoded["zettels"][0], "0:ALC|topic1|\"Quote\"|joy|DECISION");

    // Invalid lines
    let decoded2 = dialect.decode("ProjectA\nINVALID:LINE\n0:BAD");
    assert!(decoded2["header"]["wing"].is_null());
}

#[test]
fn test_dialect_extract_topics_edge_cases() {
    let dialect = Dialect::default();
    // Empty string: must contain entity placeholder and misc topic, and version header
    let empty_out = dialect.compress("", None);
    assert!(
        empty_out.contains("0:???"),
        "empty compress must have 0:???: {}",
        empty_out
    );
    assert!(
        empty_out.contains("misc"),
        "empty compress must have misc: {}",
        empty_out
    );

    // Proper noun and technical term boost
    let text = "MyDatabase and Custom-Function are Important.";
    let compressed = dialect.compress(text, None);
    assert!(compressed.contains("mydatabase") || compressed.contains("custom-function"));
}

#[test]
fn test_dialect_key_sentence_edge_cases() {
    let dialect = Dialect::default();
    // No sentences long enough
    let out = dialect.compress("Hi. No.", None);
    let summary_part = out.lines().filter(|l| !l.starts_with("JSON:")).collect::<Vec<_>>().join("\n");
    assert!(!summary_part.contains("\""));

    // Long sentence truncation
    let text = "I decided to switch because ".to_string() + &"a".repeat(100) + ".";
    let compressed = dialect.compress(&text, None);
    assert!(compressed.contains("...\""));
}
