use mempalace_rs::knowledge_graph::KnowledgeGraph;
use tempfile::tempdir;

#[test]
fn test_kg_file_initialization() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("kg.db");
    let _kg = KnowledgeGraph::new(db_path.to_str().unwrap()).unwrap();
    assert!(db_path.exists());
}

#[test]
fn test_kg_add_entity_without_properties() {
    let kg = KnowledgeGraph::new(":memory:").unwrap();
    let id = kg.add_entity("Alice", "person", None).unwrap();
    assert_eq!(id, "alice");
}

#[test]
fn test_kg_complex_queries() {
    let kg = KnowledgeGraph::new(":memory:").unwrap();
    kg.add_triple(
        "Alice",
        "knows",
        "Bob",
        Some("2024-01-01"),
        None,
        1.0,
        None,
        None,
    )
    .unwrap();
    kg.add_triple("Bob", "works at", "TechCo", None, None, 0.9, None, None)
        .unwrap();

    // Outgoing
    let out = kg.query_entity("Alice", None, "outgoing").unwrap();
    assert_eq!(out.len(), 1);
    assert_eq!(out[0]["object"], "Bob");

    // Incoming
    let inc = kg.query_entity("Bob", None, "incoming").unwrap();
    assert_eq!(inc.len(), 1);
    assert_eq!(inc[0]["subject"], "Alice");

    // Both
    let both = kg.query_entity("Bob", None, "both").unwrap();
    assert_eq!(both.len(), 2); // 1 incoming, 1 outgoing
}

#[test]
fn test_kg_duplicate_triple() {
    let kg = KnowledgeGraph::new(":memory:").unwrap();
    let id1 = kg
        .add_triple("Alice", "knows", "Bob", None, None, 1.0, None, None)
        .unwrap();
    let id2 = kg
        .add_triple("Alice", "knows", "Bob", None, None, 1.0, None, None)
        .unwrap();
    assert_eq!(id1, id2);
}

#[test]
fn test_kg_as_of_query() {
    let kg = KnowledgeGraph::new(":memory:").unwrap();
    kg.add_triple(
        "Alice",
        "lives in",
        "London",
        Some("2020-01-01"),
        Some("2022-01-01"),
        1.0,
        None,
        None,
    )
    .unwrap();
    kg.add_triple(
        "Alice",
        "lives in",
        "Paris",
        Some("2022-01-02"),
        None,
        1.0,
        None,
        None,
    )
    .unwrap();

    // Query in 2021
    let q2021 = kg
        .query_entity("Alice", Some("2021-01-01"), "outgoing")
        .unwrap();
    assert_eq!(q2021.len(), 1);
    assert_eq!(q2021[0]["object"], "London");

    // Query in 2023
    let q2023 = kg
        .query_entity("Alice", Some("2023-01-01"), "outgoing")
        .unwrap();
    assert_eq!(q2023.len(), 1);
    assert_eq!(q2023[0]["object"], "Paris");
}

#[test]
fn test_kg_invalidate() {
    let kg = KnowledgeGraph::new(":memory:").unwrap();
    kg.add_triple("Alice", "status", "active", None, None, 1.0, None, None)
        .unwrap();
    kg.invalidate("Alice", "status", "active", Some("2024-04-07"))
        .unwrap();

    let results = kg.query_entity("Alice", None, "outgoing").unwrap();
    assert_eq!(results[0]["valid_to"], "2024-04-07");
    assert_eq!(results[0]["current"], false);
}

#[test]
fn test_kg_invalid_path() {
    let result = KnowledgeGraph::new("/invalid/path/that/cannot/exist/kg.db");
    assert!(result.is_err());
}
