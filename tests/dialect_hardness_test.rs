use mempalace_rs::dialect::Dialect;
use std::collections::HashMap;

#[test]
fn test_semantic_shadowing_format() {
    let dialect = Dialect::new(None, None);
    let text = "Kai is working on the project with Alice.";
    let compressed = dialect.compress(text, None);
    
    // Format should contain Name[#hash]
    // The previous implementation used 3-letter uppercase codes, now suffixed with [#hash]
    assert!(compressed.contains("[#"));
    // e.g. KAI[#8f92a]
}

#[test]
fn test_write_discipline_decision_v1() {
    let dialect = Dialect::new(None, None);
    
    // Case 1: Compliant Decision (Grammar Matrix satisfied)
    let compliant_text = "WHO: Alice. WHAT: Use Rust. WHY: Memory safety. CONFIDENCE: High.";
    let compressed_compliant = dialect.compress(compliant_text, None);
    println!("DEBUG: compressed_compliant: {}", compressed_compliant);
    
    // Debug extraction
    let memories = mempalace_rs::extractor::extract_structured_memories(compliant_text);
    for (i, m) in memories.iter().enumerate() {
        println!("DEBUG: mem[{}] type={:?} matrix={:?}", i, m.memory_type, m.matrix);
    }

    assert!(compressed_compliant.contains("DECISION[v1]"));

    // Case 2: Malformed Decision (Missing WHO)
    let malformed_text = "We decided to use Rust because it is safe. Confidence is high."; 
    // This text triggers the DECISION marker but lacks the strict WHO: Alice format
    let compressed_malformed = dialect.compress_with_density(malformed_text, None, 5);
    
    // Should fallback to raw text if density >= 5
    assert!(compressed_malformed.starts_with("RAW|FBF|"));
    assert!(compressed_malformed.contains("We decided to use Rust"));
}

#[test]
fn test_faithfulness_score_inclusion() {
    let dialect = Dialect::new(None, None);
    let text = "Mempalace is an offline-first memory system for AI agents.";
    // Use low density to ensure we get a summary
    let compressed = dialect.compress_with_density(text, None, 5);
    
    assert!(compressed.contains("\"faithfulness\":"));
}
