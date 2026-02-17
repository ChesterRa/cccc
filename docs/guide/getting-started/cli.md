# CLI Quick Start

Get started with CCCC using the command line.

## Step 1: Navigate to Your Project

```bash
cd /path/to/your/project
```

## Step 2: Create a Working Group

```bash
cccc attach .
```

This binds the current directory as a "scope" and creates a working group.

## Step 3: Configure MCP for Your Runtime

```bash
cccc setup --runtime claude   # or codex, droid, opencode, copilot
```

This configures the MCP (Model Context Protocol) so agents can interact with CCCC.

## Step 4: Add Your First Agent

```bash
cccc actor add assistant --runtime claude
```

The first enabled actor automatically becomes the "foreman" (coordinator).

## Step 5: Start the Agent

```bash
cccc group start
```

Or start a specific agent:

```bash
cccc actor start assistant
```

## Step 6: Send a Message

```bash
cccc send "Hello! Please introduce yourself."
```

## Step 7: View Responses

Watch the ledger in real-time:

```bash
cccc tail -f
```

Or check inbox:

```bash
cccc inbox --actor-id assistant
```

## Adding More Agents

Add a second agent:

```bash
cccc actor add reviewer --runtime codex
cccc actor start reviewer
```

Send to specific agents:

```bash
cccc send "Please implement the feature" --to assistant
cccc send "Please review the code" --to reviewer
cccc send "Status update please" --to @all
```

## Reply to Messages

```bash
# Find the event ID from cccc tail
cccc reply evt_abc123 "Thanks, that looks good!"
```

## Common Commands

### Group Management

```bash
cccc groups              # List all groups
cccc use <group_id>      # Switch group
cccc active              # Show active group
cccc group show <group_id> # Show group metadata
cccc group start         # Start all agents
cccc group stop          # Stop all agents
```

### Actor Management

```bash
cccc actor list                    # List actors
cccc actor add <id> --runtime <r>  # Add actor
cccc actor start <id>              # Start actor
cccc actor stop <id>               # Stop actor
cccc actor restart <id>            # Restart actor
cccc actor remove <id>             # Remove actor
```

### Messaging

```bash
cccc send "message"                # No --to: default recipient policy applies (default: foreman)
cccc send "msg" --to @all          # Explicit broadcast
cccc send "msg" --to assistant     # To specific actor
cccc reply <event_id> "response"   # Reply to message
cccc inbox --actor-id assistant    # View unread for one actor
cccc tail -n 50                    # Recent events
cccc tail -f                       # Follow events
```

### Daemon Control

```bash
cccc daemon status    # Check status
cccc daemon start     # Start daemon
cccc daemon stop      # Stop daemon
```

## Start Web UI (Optional)

While using CLI, you can also open the Web UI:

```bash
cccc   # Starts daemon + Web UI
```

Or just the Web UI (if daemon is already running):

```bash
cccc web
```

Access at http://127.0.0.1:8848/

## Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `CCCC_HOME` | `~/.cccc` | Runtime directory |
| `CCCC_WEB_PORT` | `8848` | Web UI port |
| `CCCC_LOG_LEVEL` | `INFO` | Log verbosity |

## Example Workflow

```bash
# Setup
cd ~/projects/my-app
cccc attach .
cccc setup --runtime claude
cccc actor add dev --runtime claude

# Work
cccc group start
cccc send "Please implement user authentication"

# Monitor
cccc tail -f

# Interact
cccc reply evt_123 "Use JWT tokens please"
cccc send "What's the progress?" --to dev

# Cleanup
cccc group stop
```

## Troubleshooting

### Daemon not starting?

```bash
cccc daemon status
cccc daemon stop      # Stop any stuck instance
cccc daemon start
```

### Agent not responding?

```bash
# Check agent status
cccc actor list

# Restart the agent
cccc actor restart <actor_id>

# Check MCP setup
cccc setup --runtime <name>
```

### Can't find my group?

```bash
# List all groups
cccc groups

# Re-attach if needed
cd /path/to/project
cccc attach .
```

## Next Steps

- [Workflows](/guide/workflows) - Learn collaboration patterns
- [CLI Reference](/reference/cli) - Complete command reference
- [IM Bridge](/guide/im-bridge/) - Set up mobile access
