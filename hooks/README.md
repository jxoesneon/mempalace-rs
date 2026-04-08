# MemPalace-rs Hooks

Auto-save hooks for preventing memory loss during long sessions.

## Overview

Hooks allow MemPalace to automatically capture context at key moments:

- **Pre-compact hook**: Runs before compression to save state
- **Save hook**: Runs periodically during long sessions
- **Exit hook**: Captures final context on shutdown

## Available Hooks

### Shell Integration

Add to your `.bashrc` or `.zshrc`:

```bash
# MemPalace auto-save hook
export MEMPALACE_HOME="$HOME/.mempalace"

# Auto-save before compression
mempal_precompact() {
    if command -v mempalace-rs &> /dev/null; then
        echo "Auto-saving to MemPalace..."
        mempalace-rs diary-write --agent shell --content "Pre-compress save: $(date)"
    fi
}

# Periodic save during long sessions
mempal_periodic_save() {
    local session_duration=$(($(date +%s) - ${MEMPALACE_SESSION_START:-$(date +%s)}))
    if [ $session_duration -gt 3600 ]; then  # > 1 hour
        mempalace-rs diary-write --agent shell --content "Periodic save after ${session_duration}s"
    fi
}

# Set session start time
export MEMPALACE_SESSION_START=$(date +%s)
```

### Pre-Compact Hook

Saves critical context before running compression:

```bash
#!/bin/bash
# ~/.mempalace/hooks/precompact.sh

# Save current working context
CWD=$(pwd)
GIT_BRANCH=$(git branch --show-current 2>/dev/null || echo "no-git")
RECENT_FILES=$(find . -maxdepth 2 -type f -mtime -1 | head -20)

# Write to diary
cargo run --bin mempalace -- diary-write \
    --agent precompact-hook \
    --content "Pre-compress state: cwd=$CWD, branch=$GIT_BRANCH, recent_files=$RECENT_FILES"

echo "Pre-compact state saved to MemPalace"
```

### Save Hook

Periodic saves during long development sessions:

```bash
#!/bin/bash
# ~/.mempalace/hooks/save.sh

SESSION_FILE="$HOME/.mempalace/.session_start"

if [ -f "$SESSION_FILE" ]; then
    SESSION_START=$(cat "$SESSION_FILE")
    NOW=$(date +%s)
    DURATION=$((NOW - SESSION_START))
    
    # Save every hour
    if [ $DURATION -gt 3600 ]; then
        # Capture recent activity
        LAST_CMD=$(history 1 | sed 's/^[ ]*[0-9]*[ ]*//')
        
        cargo run --bin mempalace -- diary-write \
            --agent save-hook \
            --content "Periodic save: duration=${DURATION}s, last_cmd=$LAST_CMD"
        
        # Reset session timer
        echo $NOW > "$SESSION_FILE"
    fi
fi
```

## Installation

### Automatic Setup

```bash
# Create hooks directory
mkdir -p ~/.mempalace/hooks

# Copy example hooks
cp examples/hooks/* ~/.mempalace/hooks/
chmod +x ~/.mempalace/hooks/*.sh

# Add to shell
if ! grep -q "mempalace" ~/.bashrc; then
    echo 'source ~/.mempalace/hooks/shell_integration.sh' >> ~/.bashrc
fi
```

### Manual Setup

1. Copy hook scripts to `~/.mempalace/hooks/`
2. Make them executable: `chmod +x ~/.mempalace/hooks/*.sh`
3. Add to your shell's rc file (`.bashrc`, `.zshrc`, etc.)

## Hook Triggers

### IDE Integration

**VS Code** (in `.vscode/settings.json`):

```json
{
  "tasks": {
    "version": "2.0.0",
    "tasks": [
      {
        "label": "MemPalace Save",
        "type": "shell",
        "command": "~/.mempalace/hooks/save.sh",
        "runOptions": {
          "runOn": "folderOpen"
        }
      }
    ]
  }
}
```

**IntelliJ/RustRover** (in `.idea/externalTools.xml`):

```xml
<tool name="MemPalace Save">
  <exec>
    <option name="COMMAND" value="$USER_HOME$/.mempalace/hooks/save.sh" />
  </exec>
</tool>
```

### Git Integration

Pre-commit hook (in `.git/hooks/pre-commit`):

```bash
#!/bin/bash
# Auto-save before commits
if [ -x ~/.mempalace/hooks/save.sh ]; then
    ~/.mempalace/hooks/save.sh
fi
```

### tmux Integration

In `.tmux.conf`:

```bash
# Save every 30 minutes in active tmux session
set -g @plugin 'tmux-plugins/tmux-resurrect'

# Periodic save hook
set-hook -g client-active 'run-shell ~/.mempalace/hooks/save.sh'
```

## Custom Hooks

Create your own hooks by adding scripts to `~/.mempalace/hooks/`:

```bash
#!/bin/bash
# ~/.mempalace/hooks/my-custom-hook.sh

# Your custom logic here
echo "Running custom MemPalace hook..."

# Example: Save on specific events
case "$1" in
    pre-build)
        cargo run --bin mempalace -- diary-write \
            --agent custom-hook \
            --content "Pre-build state saved"
        ;;
    post-test)
        TEST_RESULTS=$(cargo test 2>&1 | tail -5)
        cargo run --bin mempalace -- diary-write \
            --agent custom-hook \
            --content "Test results: $TEST_RESULTS"
        ;;
esac
```

## Hook Environment Variables

| Variable                  | Description                        |
|---------------------------|------------------------------------|
| `MEMPALACE_HOME`          | Path to MemPalace data directory   |
| `MEMPALACE_SESSION_START` | Unix timestamp of session start    |
| `MEMPALACE_HOOK_TYPE`     | Type of hook being run             |
| `MEMPALACE_QUIET`         | Suppress output if set             |

## Troubleshooting

### Hooks Not Running

1. Check permissions: `chmod +x ~/.mempalace/hooks/*.sh`
2. Verify path in shell rc file
3. Source rc file: `source ~/.bashrc` or restart shell

### Diary Entries Not Created

1. Check MemPalace binary is in PATH
2. Verify diary database: `ls ~/.mempalace/diary.db`
3. Test manually: `cargo run -- diary-write --agent test --content "test"`

### Session Timer Issues

Reset session timer:

```bash
date +%s > ~/.mempalace/.session_start
```

## Security Notes

- Hooks run with your user permissions
- Be careful about what data you capture in hooks
- Avoid logging sensitive information (passwords, keys)
- Review hook scripts before running

## Further Reading

- [HOOKS_TUTORIAL.md](HOOKS_TUTORIAL.md) — Step-by-step hook creation guide
- [MCP Setup](mcp_setup.md) — Using hooks with MCP server
- [Examples](../examples/) — More integration examples
