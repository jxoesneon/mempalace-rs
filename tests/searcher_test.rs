use mempalace_rs::config::MempalaceConfig;
use mempalace_rs::searcher::Searcher;

#[tokio::test]
async fn test_searcher_initialization() {
    let config = MempalaceConfig::default();
    let searcher = Searcher::new(config);
    assert_eq!(searcher.config.collection_name, "mempalace_drawers");
}

#[tokio::test]
#[ignore] // This needs a running ChromaDB server
async fn test_searcher_search_empty() {
    let config = MempalaceConfig::default();
    let searcher = Searcher::new(config);
    let result = searcher.search("nothing", None, None, 5).await;
    // Should return "No results found" or similar
    assert!(result.is_ok());
}

#[tokio::test]
#[ignore] // This needs a running ChromaDB server
async fn test_search_memories_programmatic() {
    let config = MempalaceConfig::default();
    let searcher = Searcher::new(config);
    let result = searcher.search_memories("nothing", None, None, 5).await;
    assert!(result.is_ok());
}
