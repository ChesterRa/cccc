# CLI Quick Start

Get started with CCCC using the command line.

English | [简体中文](./cli-zh) | [日本語](./cli-ja)

## Before You Start

Install CCCC using the [installation guide](./#installation). This walkthrough
uses Claude Code: install its CLI and complete its login and any required
workspace trust prompts in your project first. You can use another
[supported runtime](../runtimes) that you have already configured.

Check that CCCC is available in your terminal:

```bash
cccc --version
cccc doctor
```

`doctor` reports the installation, detected runtimes and daemon status. Detecting
a runtime does not verify its provider login. The daemon can be stopped at this
point; start it next.

## Step 1: Start the Daemon

```bash
cccc daemon start
cccc daemon status
```

`daemon start` reuses an already-running compatible daemon. Commands such as
`attach`, `actor` and `send` need the daemon to be running.

## Step 2: Create a Working Group

```bash
cd /path/to/your/project
cccc attach .
```

This creates a new Group, binds the current directory as its project workspace
(a "scope"), and selects it as the active Group. The output includes its
`group_id`. To return to an existing Group, use `cccc groups` and
`cccc use <group_id>`; running `cccc attach .` again creates another Group.

## Step 3: Add Your First Agent

```bash
cccc actor add assistant --runtime claude
```

The first enabled actor automatically becomes the "foreman" (coordinator).

CCCC injects the CCCC MCP integration when it starts this Claude Code session;
no separate `cccc setup` step is needed for this example. Other runtimes may
require setup. See [Supported Runtimes](../runtimes) and
[`cccc setup`](/reference/cli#cccc-setup) for their integration requirements.

## Step 4: Start the Agent

```bash
cccc group start
```

`group start` starts the Group's enabled actors. To start just one actor, use:

```bash
cccc actor start assistant
```

Check `cccc actor list` for actor state. If startup requires an interactive
prompt, use the actor's terminal in the [Web UI](#start-web-ui-optional).

## Step 5: Send a Message

```bash
cccc send "Hello! Please introduce yourself." --to assistant
```

## Step 6: View Responses

Follow the Group's recorded messages and other events as JSON:

```bash
cccc tail -f
```

Look for the agent's `chat.message` events. Its native terminal output is
available in the Web UI. Press **Ctrl+C** to stop following; this leaves the
agents running.

`cccc inbox` serves a different purpose: it reads and **consumes** an actor's
unread Mail, marking that batch as read. Use `tail` or Web chat to view responses.

## Adding More Agents

Once Codex is installed and signed in, add a second agent:

```bash
cccc actor add reviewer --runtime codex
cccc actor start reviewer
```

Send to specific agents:

```bash
cccc send "Please implement the feature" --to assistant
cccc send "Please review the code" --to reviewer
cccc send "Please coordinate the next step" --to "@foreman"
cccc send "Team-wide constraint: pause deploys until CI is green" --to "@all"
```

Use task-backed delegation when the work should survive chat context switches and needs an owner, outcome, or completion evidence:

```bash
cccc tracked-send "Please implement the feature and reply with validation evidence." \
  --to assistant \
  --title "Implement feature" \
  --outcome "Feature is implemented and validation evidence is reported"
```

The multiline example uses Bash line continuations. In PowerShell, enter it on
one line.

## Reply to Messages

Replace `<event_id>` with the `id` of the message you want to answer from `tail`:

```bash
cccc reply <event_id> "Thanks, that looks good!"
cccc reply <event_id> "Non-urgent follow-up" --to assistant --mode mail
```

Mail is for agent recipients and does not prompt them immediately.

## Selecting a Group and Sender

Commands that accept `--group` resolve it in this order: the explicit option,
the `CCCC_GROUP_ID` environment variable, then the active Group selected by
`attach` or `cccc use`. Changing directories alone does not switch Groups.
Messaging uses `--by`, then `CCCC_ACTOR_ID`, then `user`. An Actor therefore
keeps its own Group when another session changes the active Group.

## Common Commands

For other instances, use `cccc connect` to discover accessible Groups, then
`cccc send --dst-instance ... --dst-group ...`. Reply with the incoming local
Event ID. See [Agent collaboration](../connect.md#agent-collaboration) for the
complete discovery, send and reply flow, including Direct connections.

### Group Management

```bash
cccc groups              # List all groups
cccc use <group_id>      # Switch group
cccc active              # Show active group
cccc group show <group_id> # Show group metadata
cccc group start         # Start this Group's enabled agents
cccc group stop          # Stop this Group's agents; keep history
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
cccc send "msg" --to assistant     # To specific actor
cccc send "msg" --to "@foreman"      # Ask the coordinator
cccc send "msg" --to "@all"          # Explicit broadcast, not default task dispatch
cccc tracked-send "work" --to assistant --title "Task title" --outcome "Done criterion"
cccc reply <event_id> "response"   # Reply to message
cccc inbox --actor-id assistant    # Read and consume an actor's unread Mail
cccc tail -n 50                    # Recent events
cccc tail -f                       # Follow events
```

### Daemon Control

```bash
cccc daemon status    # Check status
cccc daemon start     # Start daemon
cccc daemon stop      # Stop daemon and its managed processes across Groups
```

## Start Web UI (Optional)

In another terminal, start the Web UI:

```bash
cccc web
```

Running `cccc` with no subcommand does the same. Both reuse the daemon if it is
already running, or start it if needed. Open http://127.0.0.1:8848/ with the
default settings; if you configured a different binding, use the address shown
at startup. Use `cccc web --port 9000` to choose another port.

## Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `CCCC_HOME` | `~/.cccc` | Runtime directory |
| `CCCC_WEB_PORT` | `8848` | Web UI port when no saved binding or `--port` takes precedence |

Use the same `CCCC_HOME` for commands that should share Groups and runtime
state. See the [CLI reference](/reference/cli#environment-variables) for all Web
binding options and their precedence.

## Stop Working

After exiting `tail -f` with **Ctrl+C**, stop the current Group's agents:

```bash
cccc group stop
```

The Group and its recorded history remain available. `cccc group start` starts
its actors again. Use `cccc daemon stop` when you want to stop the whole instance,
including managed processes in other Groups.

## Troubleshooting

### Daemon not starting?

```bash
cccc daemon status
cccc doctor
```

If the daemon is stopped, run `cccc daemon start`. If it is unresponsive, inspect
the reported error before deliberately stopping and restarting it; a daemon
restart interrupts managed processes across Groups.

### Agent not responding?

```bash
# Check agent status
cccc actor list

# Inspect recorded errors and messages
cccc tail -n 50
```

Inspect the actor's terminal in the Web UI for login, trust or approval prompts.
Check its [runtime integration](../runtimes). After fixing the cause, retry with
`cccc actor start <actor_id>` or deliberately restart it with
`cccc actor restart <actor_id>`.

### Can't find my group?

```bash
# List all groups
cccc groups

# Select an existing Group
cccc use <group_id>
cccc active
```

Confirm that this terminal uses the expected `CCCC_HOME`. Use `attach` when you
intend to create a new Group or explicitly attach a scope to an existing one.

## Next Steps

- [Workflows](/guide/workflows) - Learn collaboration patterns
- [CLI Reference](/reference/cli) - Complete command reference
- [IM Bridge](/guide/im-bridge/) - Set up mobile access
