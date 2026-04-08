#!/bin/bash
# MemPalace Save Hook
# Periodic saves during long sessions

set -e

# Configuration
MEMPALACE_HOME="${MEMPALACE_HOME:-$HOME/.mempalace}"
SESSION_FILE="$MEMPALACE_HOME/.session_start"
HOOK_NAME="save"
SAVE_INTERVAL=3600  # 1 hour in seconds

# Initialize session if needed
if [ ! -f "$SESSION_FILE" ]; then
    date +%s > "$SESSION_FILE"
    echo "Session started, timer initialized"
    exit 0
fi

# Calculate session duration
SESSION_START=$(cat "$SESSION_FILE")
NOW=$(date +%s)
DURATION=$((NOW - SESSION_START))

# Check if it's time to save
if [ $DURATION -lt $SAVE_INTERVAL ]; then
    # Not time yet
    exit 0
fi

# Time to save - capture state
LAST_CMD=$(history 1 2>/dev/null | sed 's/^[ ]*[0-9]*[ ]*//' || echo "unknown")
CWD=$(pwd)
GIT_STATUS=$(git status --short 2>/dev/null | wc -l)

CONTEXT="Periodic save: duration=${DURATION}s, last_cmd=${LAST_CMD:0:100}, cwd=$CWD, git_changes=$GIT_STATUS"

# Save to diary
if command -v mempalace &> /dev/null; then
    mempalace diary-write --agent "$HOOK_NAME" --content "$CONTEXT"
    echo "✓ Periodic save completed (${DURATION}s into session)"
elif [ -f "./target/release/mempalace-rs" ]; then
    echo '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"tool_diary_write","arguments":{"agent":"'"$HOOK_NAME"'","content":"'"$CONTEXT"'"}}}'
    echo "✓ Periodic save completed (${DURATION}s into session)"
else
    echo "MemPalace not found, skipping diary write"
fi

# Reset timer
date +%s > "$SESSION_FILE"
