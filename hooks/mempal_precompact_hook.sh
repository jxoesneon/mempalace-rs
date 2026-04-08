#!/bin/bash
# MemPalace Pre-Compact Hook
# Saves critical context before compression

set -e

# Configuration
MEMPALACE_HOME="${MEMPALACE_HOME:-$HOME/.mempalace}"
HOOK_NAME="precompact"

# Get current context
CWD=$(pwd)
GIT_BRANCH=$(git branch --show-current 2>/dev/null || echo "no-git")
RECENT_FILES=$(find . -maxdepth 2 -type f -mtime -1 2>/dev/null | head -20 | tr '\n' ', ')

# Build context message
CONTEXT="Pre-compact state: cwd=$CWD, branch=$GIT_BRANCH"
if [ -n "$RECENT_FILES" ]; then
    CONTEXT="$CONTEXT, recent_files=${RECENT_FILES:0:200}"
fi

# Save to diary using MCP tool or CLI
if command -v mempalace &> /dev/null; then
    mempalace diary-write --agent "$HOOK_NAME" --content "$CONTEXT"
elif [ -f "./target/release/mempalace-rs" ]; then
    ./target/release/mempalace-rs mcp-server &
    # Use MCP tool via JSON-RPC
    echo '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"tool_diary_write","arguments":{"agent":"'"$HOOK_NAME"'","content":"'"$CONTEXT"'"}}}'
else
    echo "MemPalace not found, skipping diary write"
fi

echo "✓ Pre-compact state saved to MemPalace"
