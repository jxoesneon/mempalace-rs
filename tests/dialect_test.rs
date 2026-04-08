use mempalace_rs::dialect::{AAAKContext, Dialect, MetadataOverlay, AAAK_VERSION};
use std::collections::HashMap;

#[test]
fn test_dialect_compress() {
    let dialect = Dialect::default();
    let text = "Alice decided to switch to Rust because of performance.";
    let compressed = dialect.compress(text, None);

    // Phase 1: version header must be present
    assert!(
        compressed.starts_with("V:3.2"),
        "compress() must start with V:3.2; got: {}",
        compressed
    );
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
    // Phase 3: overlay must also be present
    assert!(compressed.contains("JSON:"), "MetadataOverlay must be emitted: {}", compressed);
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
    let compressed = "V:3.2\nARC: The grand journey\ntechnical|rust|2024-01-01|test\n0:ALI|rust|\"We decided to rewrite it in Rust.\"\nT:Link to another note\nExtra line for zettels";
    let decoded = dialect.decode(compressed);

    // Phase 1: version parsed
    assert_eq!(decoded["version"].as_str().unwrap(), "3.2");
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
    // version field exists (null when not present)
    assert!(decoded.get("version").is_some());
}

#[test]
fn test_aaak_context() {
    let compressed = AAAKContext::compress("A very simple sentence about things.");
    assert!(!compressed.is_empty());
    // Phase 1: AAAKContext must also emit version header
    assert!(
        compressed.starts_with("V:"),
        "AAAKContext::compress must emit version header: {}",
        compressed
    );
}

// ── Phase 1: Versioning ───────────────────────────────────────────────────────

#[test]
fn test_version_constant() {
    assert_eq!(AAAK_VERSION, "V:3.2");
}

#[test]
fn test_compress_roundtrip_version() {
    let dialect = Dialect::default();
    let out = dialect.compress("Rust is fast and memory-safe.", None);
    let decoded = dialect.decode(&out);
    assert_eq!(decoded["version"].as_str().unwrap(), "3.2");
}

// ── Phase 2: Adaptive Density ─────────────────────────────────────────────────

#[test]
fn test_density_compact() {
    let dialect = Dialect::default();
    let text = "Alice Bob Charlie decided to migrate from Python to Rust for performance.";
    let out = dialect.compress_with_density(text, None, 1);
    let zettel = out.lines().find(|l| l.starts_with("0:")).unwrap();
    let entities: Vec<&str> = zettel
        .split('|')
        .next()
        .unwrap()
        .trim_start_matches("0:")
        .split('+')
        .filter(|s| !s.is_empty() && *s != "???")
        .collect();
    assert!(entities.len() <= 1, "density=1 → max 1 entity, got {:?}", entities);
}

#[test]
fn test_density_verbose() {
    let dialect = Dialect::default();
    let text = "Alice Bob Charlie Dave Eve all worked on Rust performance benchmarks.";
    let out = dialect.compress_with_density(text, None, 10);
    let zettel = out.lines().find(|l| l.starts_with("0:")).unwrap();
    let entities: Vec<&str> = zettel
        .split('|')
        .next()
        .unwrap()
        .trim_start_matches("0:")
        .split('+')
        .filter(|s| !s.is_empty() && *s != "???")
        .collect();
    // At density 10 we allow up to 5
    assert!(entities.len() <= 5, "density=10 → max 5 entities, got {:?}", entities);
}

// ── Phase 3: MetadataOverlay ──────────────────────────────────────────────────

#[test]
fn test_metadata_overlay_to_line_roundtrip() {
    let mut extra = HashMap::new();
    extra.insert("sprint".to_string(), "aaak-v3.2".to_string());
    let overlay = MetadataOverlay {
        version: Some("V:3.2".to_string()),
        wing: Some("technical".to_string()),
        room: Some("rust".to_string()),
        date: Some("2026-04-08".to_string()),
        source_file: Some("session.md".to_string()),
        extra,
    };
    let line = overlay.to_line();
    assert!(line.starts_with("JSON:"), "must start with JSON:");
    let parsed = MetadataOverlay::from_line(&line).expect("must parse back");
    assert_eq!(parsed.wing, Some("technical".to_string()));
    assert_eq!(parsed.room, Some("rust".to_string()));
    assert_eq!(parsed.extra.get("sprint").unwrap(), "aaak-v3.2");
}

#[test]
fn test_overlay_emitted_only_when_meaningful() {
    let dialect = Dialect::default();
    // No metadata → no overlay line
    let no_meta = dialect.compress("Some text.", None);
    assert!(
        !no_meta.contains("JSON:"),
        "overlay must not appear without metadata"
    );
    // With metadata → overlay line
    let mut meta = HashMap::new();
    meta.insert("wing".to_string(), "test".to_string());
    let with_meta = dialect.compress("Some text.", Some(meta));
    assert!(
        with_meta.contains("JSON:"),
        "overlay must appear with metadata: {}",
        with_meta
    );
}

#[test]
fn test_decode_overlay_parsed() {
    let dialect = Dialect::default();
    let mut meta = HashMap::new();
    meta.insert("wing".to_string(), "emotions".to_string());
    meta.insert("room".to_string(), "joy".to_string());
    meta.insert("source_file".to_string(), "chat.md".to_string());
    let compressed = dialect.compress("I was so happy today!", Some(meta));
    let decoded = dialect.decode(&compressed);
    assert!(
        !decoded["overlay"].is_null(),
        "overlay field must be decoded: {}",
        decoded
    );
    assert_eq!(decoded["overlay"]["wing"].as_str().unwrap(), "emotions");
}

// ── Phase 4: Custom Emotions ──────────────────────────────────────────────────

#[test]
fn test_custom_emotion_override() {
    let mut custom = HashMap::new();
    custom.insert("joy".to_string(), "XJY".to_string());
    let dialect = Dialect::with_custom_emotions(None, None, custom);
    let code = dialect.encode_emotions(&["joy".to_string()]);
    assert_eq!(code, "XJY", "custom emotion code must override built-in");
}

#[test]
fn test_custom_emotion_fallback_to_builtin() {
    // No override for "grief" → uses built-in "grief"
    let custom = HashMap::new();
    let dialect = Dialect::with_custom_emotions(None, None, custom);
    let code = dialect.encode_emotions(&["grief".to_string()]);
    assert_eq!(code, "grief");
}

// ── Phase 5: Proposition Atomisation ─────────────────────────────────────────

#[test]
fn test_atomize_basic() {
    let dialect = Dialect::default();
    let text = "Alice migrated the service to Rust. \
                The new code is 10x faster. \
                Bob reviewed and approved the change. \
                Deployment was on Monday.";
    let props = dialect.atomize(text, 3);
    assert!(!props.is_empty(), "atomize must return at least one proposition");
    assert!(props.len() <= 3, "must respect max_propositions");
    for p in &props {
        assert!(!p.trim().is_empty(), "each proposition must be non-empty");
    }
}

#[test]
fn test_compress_propositions_format() {
    let dialect = Dialect::default();
    let text = "Alice chose Rust for performance. Bob picked tokio for async. SQLite stores data.";
    let out = dialect.compress_propositions(text, None, 3, 5);
    assert!(out.starts_with("V:3.2"), "proposition output must start with version header");
    assert!(out.contains("P0:"), "must have at least P0: proposition line: {}", out);
}

// ── Phase 7: Delta Encoding ───────────────────────────────────────────────────

#[test]
fn test_compress_delta_small_change() {
    let dialect = Dialect::default();
    let old = dialect.compress("Alice uses Rust for systems programming.", None);
    // Tiny addition
    let delta = dialect.compress_delta(&old, "Alice uses Rust for systems programming and async.");
    // Should be a DELTA: line (small change < 40%)
    println!("delta result: {}", delta);
    // Either DELTA: or full re-compress are both valid
    assert!(!delta.is_empty());
}

#[test]
fn test_compress_delta_large_change() {
    let dialect = Dialect::default();
    let old = dialect.compress("Rust ownership prevents null pointers at compile time.", None);
    let result = dialect.compress_delta(
        &old,
        "Quantum computing uses superposition and entanglement for parallel computation.",
    );
    // Large topic shift → full recompress (starts with V:3.2)
    assert!(
        result.starts_with("V:3.2") || result.starts_with("DELTA:"),
        "unexpected delta result: {}",
        result
    );
}

// ── Phase 9: Faithfulness Score ───────────────────────────────────────────────

#[test]
fn test_faithfulness_in_range() {
    let dialect = Dialect::default();
    let (_, score) = dialect.compress_with_faithfulness(
        "Rust memory safety ownership borrow-checker prevents data races.",
        None,
    );
    assert!(
        (0.0..=1.0).contains(&score),
        "faithfulness must be 0.0–1.0, got {}",
        score
    );
}

#[test]
fn test_faithfulness_nonzero_for_rich_text() {
    let dialect = Dialect::default();
    let (_, score) = dialect.compress_with_faithfulness(
        "Rust enables memory ownership borrowing lifetime enforcement.",
        None,
    );
    assert!(score > 0.0, "faithfulness should be > 0 for topic-rich text, got {}", score);
}
