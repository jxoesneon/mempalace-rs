#!/bin/bash
# MemPalace Save Hook
# Prevent context bloat by blocking Claude Code saves if the session gets too long

set -e

# Configuration
SAVE_INTERVAL=50  # Max human messages before blocking

# Phase 4: Environment Variable Ingestion
if [ -n "$MEMPAL_DIR" ]; then
    # Trigger asynchronous mine
    if command -v mempalace &> /dev/null; then
        mempalace mine "$MEMPAL_DIR" > /dev/null 2>&1 &
    elif [ -f "./target/release/mempalace-rs" ]; then
        ./target/release/mempalace-rs mine "$MEMPAL_DIR" > /dev/null 2>&1 &
    fi
fi

# Read JSON from stdin
PAYLOAD=$(cat)
if [ -z "$PAYLOAD" ]; then
    exit 0
fi

# Extract transcript_path and session_id using python3 (resilient fallback for jq)
TRANSCRIPT_PATH=$(echo "$PAYLOAD" | python3 -c "import sys, json; print(json.load(sys.stdin).get('transcript_path', ''))")
SESSION_ID=$(echo "$PAYLOAD" | python3 -c "import sys, json; print(json.load(sys.stdin).get('session_id', ''))")

if [ -z "$TRANSCRIPT_PATH" ] || [ ! -f "$TRANSCRIPT_PATH" ]; then
    exit 0
fi

# Parse the JSONL transcript to count human messages
HUMAN_MSG_COUNT=$(grep -c '"role":"user"' "$TRANSCRIPT_PATH" || echo 0)

if [ "$HUMAN_MSG_COUNT" -gt "$SAVE_INTERVAL" ]; then
    echo '{"decision": "block", "reason": "Context bloat detected. Please summarize."}'
    exit 0
fi

exit 0
