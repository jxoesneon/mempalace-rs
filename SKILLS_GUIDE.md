# MemPalace-rs AI Agent Integration Guide

Welcome to the definitive guide for connecting your AI agents to the MemPalace-rs memory stack. This guide covers the top 15 platforms, IDEs, and frameworks in the agentic ecosystem.

## 🚀 The Big 3 (Insta-Setup)

### 1. Claude (Desktop & Code)

**Type**: MCP (Model Context Protocol)

- **Claude Code**: Run locally in your terminal:
  ```bash
  claude add mcp "mempalace-rs" --command "cargo" --args "run,--,mcp-server" --cwd "$(pwd)"
  ```
- **Claude Desktop**: Edit `~/Library/Application Support/Claude/claude_desktop_config.json`:
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

### 2. Cursor

**Type**: MCP

1. Open **Settings > Features > MCP**.
2. Click **Add New MCP Server**.
3. Setup:
   - **Name**: `mempalace`
   - **Type**: `command`
   - **Command**: `cargo run -- mcp-server`
   - **CWD**: Full path to the repository.

### 3. Windsurf

**Type**: Skill File Bundle

1. Navigate to the project root in your file explorer.
2. Drag and drop `mempalace-rs.skill` into the Windsurf chat window or project sidebar.
3. Windsurf will automatically decompose the bundle into tools and architectural instructions.

---

## 🛠️ IDEs & Productivity Clients

### 4. Zed

**Type**: Custom Context Server
Edit `~/.config/zed/settings.json`:

```json
{
  "context_servers": {
    "mempalace": {
      "command": "cargo",
      "args": ["run", "--", "mcp-server"],
      "cwd": "/path/to/mempalace-rs"
    }
  }
}
```

### 5. VS Code (via MCP Extension)

**Type**: MCP

1. Install an MCP Client extension (e.g., "MCP Client for VS Code" or "Agent Builder").
2. Open the Command Palette (`Cmd+Shift+P`) and select **MCP: Add Server**.
3. Select `stdio` and provide the command `cargo run -- mcp-server` with the project CWD.

### 6. Cline (VS Code Extension)

**Type**: MCP

1. Go to Cline Settings (Gear icon in sidebar).
2. Add a new tool-source.
3. Use the `cargo run -- mcp-server` command. Cline will automatically discover all 19 tools.

### 7. PearAI

**Type**: MCP (Cursor Fork)
Follow the same instructions as **Cursor** (#2). PearAI uses the identical MCP engine.

---

## 💻 CLI & Developer Agents

### 8. Goose CLI

**Type**: CLI Extension

1. Run `goose configure`.
2. Select **Add Extension** > **Command-line Extension**.
3. Name: `mempalace`.
4. Run command: `cargo run -- mcp-server`.

### 9. mcp-agent (Python Framework)

**Type**: Native MCP Client
Integrate MemPalace into your Python agents:

```python
from mcp_agent import MultiServerMCPClient

async def main():
    client = MultiServerMCPClient()
    await client.connect_stdio("cargo", ["run", "--", "mcp-server"], cwd="/path/to/repo")
    # Tools are now available in client.tools
```

---

## 🧠 Frameworks & Orchestrators

### 10. LangGraph (LangChain)

**Type**: Tool Adapter
Use the `langchain-mcp-adapters`:

```bash
pip install langchain-mcp-adapters
```

```python
from langchain_mcp_adapters import get_mcp_tools
tools = get_mcp_tools("cargo", ["run", "--", "mcp-server"], cwd="/path/to/repo")
# Bind 'tools' to your LangGraph agent
```

### 11. CrewAI

**Type**: Tool Delegation
Define a MemPalace tool wrapper using the MCP stdio interface and assign it to your "Memory Researcher" agent.

### 12. OpenAI Agents SDK

**Type**: Native SDK Integration
OpenAI agents can consume MCP tools directly. Configure the MCP toolbox in your agent definition to point to the `mempalace-rs` stdio endpoint.

### 13. Google ADK (Agent Development Kit)

**Type**: Toolbox
Add MemPalace to your Google Cloud / Vertex AI agent by registering the `mempalace-rs` binary as a local tool in the ADK toolbox config.

---

## 🌐 Workflows & Marketplaces

### 14. n8n (Agent Mode)

**Type**: MCP Client Node

1. Use the **MCP Client Tool** node in your workflow.
2. Select **Self-Hosted** or provide the local command path.
3. n8n will expose all memory search and diary tools to your workflow.

### 15. LobeHub / Smithery

**Type**: Skill Import
Visit [Smithery.ai](https://smithery.ai) and search for `mempalace-rs`. You can "One-Click Install" the memory palace into supported LobeHub agents or other web-based clients that support Smithery-managed MCP servers.

---

## 💎 Hardened AAAK Protocol (v3.2)

Your agent can now leverage the high-stakes features of the v3.2 protocol. When interacting with the memory palace, be aware of:

### 1. High-Stakes Write Discipline

For critical memories (e.g., product decisions, architectural shifts), use the **Grammar Matrix** triggers:

- **Triggers**: `WHO:`, `WHAT:`, `WHY:`, `CONFIDENCE:`
- **Failsafe**: If validation fails, the palace automatically buffers the raw text (`RAW|FBF|`) to ensure zero context loss.

### 2. Semantic Shadowing

Entities are now uniquely identified via deterministic hashes. Instead of generic `KAI`, you will see `KAI[#8f92a]`. This prevents cross-project entity pollution and ensures specific context is never mixed.

### 3. Faithfulness Auditing

Optionally inspect the `JSON:` metadata line in compressed blocks to check the `faithfulness_score` (0.0 to 1.0). If you detect low faithfulness, use the `raw` search mode to retrieve verbatim context.

---

## 🧪 Advanced: Custom SDK Integration

Want to build your own? See the [Programmatic Usage](examples/README.md#programmatic-usage-rust-api) guide to use our Rust crate directly in your specialized agents.
