# CLI Reference

Complete command reference for the CCCC CLI.

## Global Commands

### `cccc`

Start the daemon and Web UI together.

```bash
cccc                    # Start daemon + Web UI
cccc --help             # Show help
```

### `cccc doctor`

Check your environment and diagnose issues.

```bash
cccc doctor             # Full environment check
```

Browser readiness uses the same executable discovery as the Web runtime, including the standard
Chrome, Edge, and Chromium installation directories on macOS and Windows. On Linux, the report
includes projected-browser readiness: system Chrome/Edge, required `Xvfb`
isolation, and the optional `x11vnc` VNC viewer. A missing `x11vnc` does not prevent browser
isolation; CCCC falls back to its CDP screencast viewer.

The installation section reports the executable handling the current invocation,
the first `cccc` selected by PATH, and all other `cccc` commands. A `CONFLICT`
status means an older installation is ahead of the current launcher; move the
current launcher's directory to the front of PATH and open a new terminal.

### `cccc runtime list`

List available agent runtimes.

```bash
cccc runtime list       # List detected runtimes
cccc runtime list --all # List all supported runtimes
```

## Daemon Commands

### `cccc daemon`

Manage the CCCC daemon.

```bash
cccc daemon status      # Check daemon status
cccc daemon start       # Start daemon
cccc daemon stop        # Stop daemon
```

Notes:
- `cccc daemon start` refuses to spawn a duplicate daemon if the pid-file process is still alive but IPC is not responding.
- In that case, run `cccc daemon stop` (or clean stale runtime state) before retrying start.

## Group Commands

### `cccc attach`

Create or attach to a working group.

```bash
cccc attach .           # Attach current directory as scope
cccc attach /path/to/project
```

### `cccc groups`

List all working groups.

```bash
cccc groups             # List groups
```

### `cccc use`

Switch to a different working group.

```bash
cccc use <group_id>     # Switch to group
```

### `cccc group`

Manage the current working group.

```bash
cccc group create --title "my-group"         # Create group
cccc group show <group_id>                   # Show group metadata
cccc group update --group <id> --title "..." # Update title/topic
cccc group use <group_id> .                  # Set active scope
cccc group start --group <id>                # Start group actors
cccc group stop --group <id>                 # Stop group actors
cccc group set-state idle --group <id>       # Set state: active/idle/paused/stopped
cccc group detach-scope <scope_key> --group <id>
cccc group delete --group <id> --confirm <id>
```

## Actor Commands

### `cccc actor add`

Add a new actor to the group.

```bash
cccc actor add <actor_id> --runtime claude
cccc actor add <actor_id> --runtime codex
cccc actor add <actor_id> --runtime web_model
cccc actor add <actor_id> --runtime custom --command "my-agent"
```

Options:
- `--runtime`: Agent runtime (claude, codex, web_model, droid, etc.)
- `--command`: Custom command (for custom runtime)
- `--runner`: Runner type (pty or headless; web_model is headless-only)
- `--title`: Display title

For the ChatGPT Web Model actor, create and start the actor from the target CCCC Web group, then finish ChatGPT sign-in, MCP URL, and chat binding in `Settings > Global > ChatGPT Web Model`.

### `cccc actor`

Manage actors.

```bash
cccc actor list                    # List actors
cccc actor add <actor_id> --scope /path/to/project
cccc actor start <actor_id>        # Start actor
cccc actor stop <actor_id>         # Stop actor
cccc actor restart <actor_id>      # Restart actor
cccc actor remove <actor_id>       # Remove actor
cccc actor update <actor_id> --scope /path/to/project
cccc actor secrets <actor_id> ...  # Manage runtime-only secrets
```

Actor scope arguments are project paths at the CLI boundary. Rust resolves them to the attached
scope key before persistence, keeping the stored group document compatible with the Python backend.

## Message Commands

### `cccc send`

Send a message.

```bash
cccc send "Hello"                  # No --to: default recipient policy applies (default: foreman)
cccc send "Hello" --to @foreman    # Send to foreman
cccc send "Hello" --to peer-1      # Send to specific actor
cccc send "Announcement" --to @all # Explicit broadcast
cccc send "Review this scope" --path src/api
```

### `cccc tracked-send`

Create a task and send one linked delegation message.

```bash
cccc tracked-send "Please implement this and reply with validation evidence." \
  --to peer-1 \
  --title "Implement feature" \
  --outcome "Feature is implemented and validation evidence is reported"
```

The Rust CLI also forwards `--checklist`, `--assignee`, `--waiting-on`, `--handoff-to`,
`--notes`, `--priority`, `--no-reply-required`, and `--idempotency-key` to the daemon.

### `cccc reply`

Reply to a message.

```bash
cccc reply <event_id> "Reply text" --to peer-1 --priority attention --reply-required
```

### `cccc inbox`

View inbox.

```bash
cccc inbox --actor-id <id>         # View actor unread messages
cccc inbox --actor-id <id> --mark-read
cccc inbox --actor-id <id> --kind-filter notify
```

### `cccc tail`

Tail the ledger.

```bash
cccc tail                          # Show recent events
cccc tail -n 50                    # Show last 50 events
cccc tail -f                       # Follow new events
```

## IM Bridge Commands

### `cccc im`

Manage IM Bridge.

```bash
cccc im set telegram --token-env TELEGRAM_BOT_TOKEN
cccc im set slack --bot-token-env SLACK_BOT_TOKEN --app-token-env SLACK_APP_TOKEN
cccc im set discord --token-env DISCORD_BOT_TOKEN
cccc im set feishu --app-key-env FEISHU_APP_ID --app-secret-env FEISHU_APP_SECRET
cccc im set dingtalk --app-key-env DINGTALK_APP_KEY --app-secret-env DINGTALK_APP_SECRET --robot-code-env DINGTALK_ROBOT_CODE

cccc im start                      # Start IM bridge
cccc im stop                       # Stop IM bridge
cccc im status                     # Check IM bridge status
cccc im logs                       # View IM bridge logs
cccc im logs -f                    # Follow IM bridge logs
```

## Group Space Commands

### `cccc space`

Manage Group Space provider-backed shared memory.

```bash
cccc space status
cccc space credential status
cccc space credential set --auth-json '{"cookies":[{"name":"SID","value":"...","domain":".google.com"}]}'
cccc space credential set --auth-json-file ./notebooklm.storage_state.json
cccc space credential clear
cccc space health

cccc space bind [remote_space_id]    # omit to auto-create NotebookLM notebook
cccc space unbind
cccc space sync --force

cccc space ingest --kind context_sync --payload '{"vision":"v0.5 plan"}'
cccc space ingest --kind resource_ingest --payload '{"path":"docs/spec.md"}' --idempotency-key ingest-docs-1

cccc space query "What is the latest shared plan?"
cccc space query "Summarize risks from these sources" --options '{"source_ids":["src_1","src_2"]}'

cccc space jobs list
cccc space jobs list --state failed --limit 20
cccc space jobs retry <job_id>
cccc space jobs cancel <job_id>
```

Notes:
- `--group` is optional; defaults to the active group.
- Current provider is `notebooklm`.
- `--payload` and `--options` must be JSON objects.
- `cccc space query --options` only supports `source_ids` (array of source IDs).
- `language` / `lang` are not valid query options (put language requirement in query text).
- Provider credentials are write-only; CLI/Web only return masked metadata.
- `cccc space health` validates credential format and adapter compatibility.
- The Python implementation vendors `notebooklm-py` v0.8.0. The experimental
  Rust implementation uses a native protocol client and currently supports
  direct `resource_ingest` only for `pasted_text`; use `cccc space sync` for
  local `.md`/`.txt` files, or select Python for direct file/URL/YouTube/Drive
  ingestion.
- Artifact generation is asynchronous and does not save locally by default.
  Request wait/save explicitly when a local artifact is required. Native Rust
  download currently supports media, report/study-guide, infographic, and
  slide-deck outputs; interactive quiz/flashcard/mind-map and data-table
  downloads remain Python-only.
- When a group is bound, curated `context_sync` exports are also auto-enqueued from `context_sync` updates.
- `cccc space sync` performs two-way reconcile for Group Space:
  - local `repo/space/` files -> provider sources,
  - provider source/artifact projection -> local `repo/space/` (`.sync/remote-sources` and `artifacts/`).

## Implementation Selection

### `cccc python [command ...]` / `cccc rust [command ...]`

Select the product implementation persistently, then optionally execute a
command with that implementation. Python is the stable and recommended default;
Rust is an experimental opt-in for performance evaluation while feature and
integration parity remains in progress.

```bash
cccc status             # Show selected, running, and available implementations
cccc rust               # Select experimental Rust and launch daemon + Web
cccc rust doctor        # Select experimental Rust and run doctor
cccc python             # Select stable Python and launch daemon + Web
cccc python daemon start
```

The selector must be the first argument. It is intentionally not a one-shot
override: agent runtimes and later terminal invocations all follow the same
selection in `CCCC_HOME`. Switching validates the target first and then stops
the active Web/daemon pair. If Rust is absent or has a different product version,
the command fails without changing the selection or falling back to Python.
If the selection file is corrupt, ordinary commands fail visibly; an explicit
`cccc python` selector replaces it and restores the safe default.

Python is the stable initial default only while no implementation choice has
been stored. After `cccc rust` or `cccc python`, a bare `cccc` follows that
persisted choice; the Web startup banner prints the implementation that actually
started. Use `cccc python` to return to the stable implementation at any time.

`status`, `version`, and `update` are stable launcher commands. `status` shows
the selected implementation, the implementation reported by a live daemon, and
whether the bundled Rust payload is usable. `version` is the shared product
version. `update` follows the installer that owns the public executable: the
website installer for an experimental standalone Rust preview, or pip for the
recommended complete `cccc-pair` distribution.

The legacy `ccccd start|stop|status|run` command remains as a compatibility
alias, but now passes through the same implementation launcher. New automation
should prefer `cccc daemon ...`.

## Setup Commands

### `cccc setup`

Configure MCP for an agent runtime.

```bash
cccc setup                         # Configure every supported runtime; unavailable CLIs are reported
cccc setup --runtime claude        # Auto-configure for Claude Code
cccc setup --runtime codex         # Auto-configure for Codex
cccc setup --runtime copilot       # Auto-configure for GitHub Copilot CLI
cccc setup --runtime cursor        # Show prompt-assisted setup contract for Cursor CLI
cccc setup --runtime devin         # Auto-configure for Devin CLI
cccc setup --runtime kiro          # Auto-configure for Kiro CLI
cccc setup --runtime kimi          # Auto-configure for Kimi CLI
cccc setup --runtime kilo          # Show prompt-assisted setup contract for Kilo Code CLI
cccc setup --runtime antigravity   # Show prompt-assisted setup contract for Antigravity CLI
```

Without `--runtime`, setup performs one batch pass: installed CLI runtimes are configured,
prompt-assisted/manual runtimes return their configuration contract, and missing runtimes are
reported without aborting the remaining setup work.

### `cccc update`

Upgrade CCCC through the detected installation channel.

```bash
cccc update                        # Upgrade using the detected channel
cccc update --check                # Show install detection + planned command

# pip distribution only
cccc update --channel stable       # Force the stable PyPI channel
cccc update --channel rc           # Force the TestPyPI RC channel
```

Notes:
- The default channel follows the detected install metadata when possible, then falls back to `stable`.
- Experimental standalone Rust installations reuse the GitHub Pages installer,
  preserve their current install directory, and contain no Python fallback or
  implementation switching.
- Editable and local-path installs are reported but not updated automatically.
- The recommended platform wheel updates the public launcher, stable Python
  implementation, and experimental private Rust payload together.
- After a successful update, CCCC stops the older Web/daemon pair; the next
  command starts the selected implementation from the new product version.

## Web Commands

### `cccc web`

Start only the Web UI (daemon must be running).

```bash
cccc web                           # Start Web UI
cccc web --port 9000               # Custom port
cccc web --exhibit                 # Read-only exhibit mode
cccc web --mode exhibit            # Equivalent explicit mode
```

Only one Web process may run for a given `CCCC_HOME`. If another CCCC process
for that home is active, an interactive launch displays its PID and asks whether
to stop it before continuing. Non-interactive launches fail instead of stopping
an existing process implicitly.

## MCP Commands

### `cccc mcp`

Start the MCP server (for agent integration).

```bash
cccc mcp                           # Start MCP server (stdio mode)
```

## Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `CCCC_HOME` | `~/.cccc` | Runtime home directory |
| `CCCC_WEB_HOST` | saved setting, then `127.0.0.1` | Web UI bind address; `--host` overrides both |
| `CCCC_WEB_PORT` | saved setting, then `8848` | Web UI port; `--port` overrides both |
| `CCCC_WEB_MODE` | `normal` | Set to `exhibit` for a read-only Web UI |
| `CCCC_WEB_READONLY` | unset | Truthy value also enables read-only exhibit mode |
| `CCCC_WEB_READY_TIMEOUT_SECONDS` | `10` | Supervised Web child readiness timeout before CCCC treats startup as failed |
| `CCCC_LOG_LEVEL` | `INFO` | Log level |
