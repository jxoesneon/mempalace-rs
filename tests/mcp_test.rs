use anyhow::Result;

#[tokio::test]
async fn test_mcp_stub() -> Result<()> {
    // run_mcp_server blocks on stdin, so we can't easily test it in integration tests
    // without spawning a process. For now, we just verify the module compiles.
    Ok(())
}
