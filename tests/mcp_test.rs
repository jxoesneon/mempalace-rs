use anyhow::Result;
use mempalace_rs::config::MempalaceConfig;
use mempalace_rs::mcp_server::McpServer;
use serde_json::json;
use std::sync::Arc;

fn setup_test() -> (MempalaceConfig, tempfile::TempDir) {
    let temp_dir = tempfile::tempdir().unwrap();
    let config = MempalaceConfig::new(Some(temp_dir.path().to_path_buf()));
    (config, temp_dir)
}

#[tokio::test]
async fn test_all_mcp_tools_dispatch() -> Result<()> {
    let (config, _td) = setup_test();
    let server = Arc::new(McpServer::new(config).await?);

    let tools = vec![
        "mempalace_status",
        "mempalace_list_wings",
        "mempalace_get_taxonomy",
        "mempalace_find_tunnels",
        "mempalace_graph_stats",
    ];

    for tool in tools {
        let req_params = json!({ "name": tool, "arguments": {} });
        let result: Result<serde_json::Value> = server.handle_tools_call(Some(req_params)).await;
        assert!(result.is_ok(), "Tool {} failed: {:?}", tool, result.err());
    }
    Ok(())
}

#[tokio::test]
async fn test_tool_error_handling() -> Result<()> {
    let (config, _td) = setup_test();
    let server = Arc::new(McpServer::new(config).await?);

    // Missing arguments
    let req_params = json!({ "name": "mempalace_list_rooms" });
    let result: Result<serde_json::Value> = server.handle_tools_call(Some(req_params)).await;
    assert!(result.is_err());

    // Unknown tool
    let req_params = json!({ "name": "unknown_tool", "arguments": {} });
    let result: Result<serde_json::Value> = server.handle_tools_call(Some(req_params)).await;
    assert!(result.is_err());

    Ok(())
}

#[tokio::test]
async fn test_kg_tools() -> Result<()> {
    let (config, _td) = setup_test();
    let server = Arc::new(McpServer::new(config).await?);

    // KG Add
    let add_args = json!({ "name": "mempalace_kg_add", "arguments": { "subject": "a", "predicate": "is", "object": "b" } });
    let res: serde_json::Value = server.handle_tools_call(Some(add_args)).await?;
    assert!(res["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("success"));

    // KG Query
    let query_args = json!({ "name": "mempalace_kg_query", "arguments": { "entity": "a" } });
    let res: serde_json::Value = server.handle_tools_call(Some(query_args)).await?;
    assert!(res["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("results"));

    // KG Invalidate
    let inv_args = json!({ "name": "mempalace_kg_invalidate", "arguments": { "subject": "a", "predicate": "is", "object": "b" } });
    let res: serde_json::Value = server.handle_tools_call(Some(inv_args)).await?;
    assert!(res["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("success"));

    Ok(())
}
