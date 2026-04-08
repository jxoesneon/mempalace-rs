# MemPalace-rs Examples

Usage examples for the Rust-native, offline-first AI memory system.

## Basic CLI Examples

### 1. Initialize a New Palace
Detect and tag rooms in your folder structure to prepare for mining.

```bash
cargo run -- init ~/my-project
```

### 2. Mine a Codebase
Ingest files into the Palace. Use `project` mode for source code and `convos` for chat transcripts.

```bash
# Mine entire project into a specific wing
cargo run -- mine ~/my-project --mode project --wing MyProject

# Mine conversations
cargo run -- mine ~/conversations --mode convos --wing Personal
```

### 3. Search Your Palace
Perform high-speed word search across all wings or specific rooms.

```bash
# Basic search
cargo run -- search "async patterns"
```

### 4. Get Wakeup Context
Retrieve the "Layer 0 + Layer 1" context designed for AI agent system prompts (~600-900 tokens).

```bash
# Default wakeup
cargo run -- wakeup

# Specific wing wakeup
cargo run -- wakeup --wing MyProject
```

### 5. Compress Memory
Use the AAAK (Atomic-Atomic-Atomic-Knowledge) dialect to compress drawers for extremely efficient long-term storage.

```bash
# Compress all drawers
cargo run -- compress

# Compress specific wing
cargo run -- compress --wing MyProject
```

### 6. MCP Server Integration
Run the Model Context Protocol server to connect your palace directly to Claude Code or other AI agents.

```bash
cargo run -- mcp-server
```

---

## Programmatic Usage (Rust API)

### 1. Search and Retrieval
Use the `Searcher` and `Storage` crates to interact with the Palace in your own Rust applications.

```rust
use mempalace_rs::{
    config::MempalaceConfig,
    searcher::Searcher,
    storage::Storage,
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = MempalaceConfig::default();
    
    // Initialize storage
    let storage = Storage::new("palace.db")?;
    
    // High-level search
    let searcher = Searcher::new(config);
    let results = searcher.search("async patterns", None, None, 5).await?;
    
    println!("Search Results: \n{}", results);
    
    Ok(())
}
```

### 2. Manual AAAK Compression
Directly use the AAAK dialect for text compression.

```rust
use mempalace_rs::dialect::AAAKContext;

fn main() {
    let input = "Meeting notes from April 8th about Project X deployment...";
    let compressed = AAAKContext::compress(input);
    println!("Compressed: {}", compressed);
}
```

---

## Integration Examples

### Claude Code Integration
Add the following to your `.claude/settings.json` (or equivalent MCP config):

```json
{
  "mcpServers": {
    "mempalace": {
      "command": "cargo",
      "args": ["run", "--", "mcp-server"],
      "cwd": "/path/to/mempalace-rs"
    }
  }
}
```

### CI/CD Integration
Automatically mine your project into a shared palace on every push.

```yaml
name: Mine Project
on:
  push:
    branches: [main]

jobs:
  mine:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - name: Mine into MemPalace
        run: |
          cargo run -- mine . --mode project --wing ${{ github.repository }}
```

## Troubleshooting

### No Results Found
- Check if palace is initialized and has drawers: `cargo run -- status`
- Ensure you are searching for exact words (exact-match engine).

### Model Issues
If embeddings fail to initialize, ensure the models are downloaded:
```bash
cargo run --bin download-model
```
