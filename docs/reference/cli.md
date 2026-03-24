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

Bind a project path to the current or selected group as its authoritative workspace.

```bash
cccc attach .           # Attach current directory as authoritative workspace
cccc attach /path/to/project
```

Notes:
- `attach` defines the group's `authoritative_workspace`.
- It does not by itself create per-actor isolated workspaces.
- Current product direction keeps the default execution path lightweight: actors
  still default to `workspace_mode = shared` unless a future explicit isolated
  policy says otherwise.

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
cccc actor add <actor_id> --runtime custom --command "my-agent"
cccc actor add <actor_id> --profile-id shared-profile
```

Options:
- `--runtime`: Agent runtime (claude, codex, droid, etc.)
- `--command`: Custom command (for custom runtime)
- `--runner`: Runner type (pty or headless)
- `--title`: Display title
- `--profile-id`: Link the new live actor to a reusable `profile`
- `--profile-scope` / `--profile-owner-id`: Address explicit user-scoped
  profiles when `--profile-id` is used

### `cccc actor`

Manage actors.

```bash
cccc actor list                    # List actors
cccc actor start <actor_id>        # Start actor
cccc actor stop <actor_id>         # Stop actor
cccc actor restart <actor_id>      # Restart actor
cccc actor remove <actor_id>       # Remove actor
cccc actor update <actor_id> ...   # Update actor settings
cccc actor secrets <actor_id> ...  # Manage runtime-only secrets
cccc actor update <actor_id> --profile-id shared-profile
cccc actor update <actor_id> --profile-action convert_to_custom
```

### `cccc actor profile`

Manage reusable actor runtime profiles.

```bash
cccc actor profile list
cccc actor profile list --view my
cccc actor profile get <profile_id>
cccc actor profile upsert --name "Shared Codex" --runtime codex
cccc actor profile upsert --id shared --command "codex --resume"
cccc actor profile delete <profile_id>
cccc actor profile secrets <profile_id> --keys
```

Notes:
- Reusable `profile` records can now be managed directly from the CLI instead
  of only being mentioned as actor linkage metadata.
- `list` supports `--view global|my|all`.
- `get` and `delete` support `--scope global|user` and `--owner-id ...` so the
  CLI can address explicit user-scoped profile refs.
- `upsert` stores reusable launch intent and capability defaults; it does not
  by itself prove any live runtime continuity.
- `secrets` manages runtime-only secret keys for a reusable profile without
  mixing those values into the profile document itself.

Notes:
- An actor's runtime defaults may come from direct actor config or a linked
  reusable `profile`; that linkage is configuration intent, not live runtime
  proof.

## Message Commands

### `cccc send`

Send a message.

```bash
cccc send "Hello"                  # No --to: default recipient policy applies (default: foreman)
cccc send "Hello" --to @all        # Explicit broadcast
cccc send "Hello" --to @foreman    # Send to foreman
cccc send "Hello" --to peer-1      # Send to specific actor
```

### `cccc reply`

Reply to a message.

```bash
cccc reply <event_id> "Reply text"
```

### `cccc inbox`

View inbox.

```bash
cccc inbox --actor-id <id>         # View actor unread messages
cccc inbox --actor-id <id> --mark-read
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
- When a group is bound, curated `context_sync` exports are also auto-enqueued from `context_sync` updates.
- `cccc space sync` performs two-way reconcile for Group Space:
  - local `repo/space/` files -> provider sources,
  - provider source/artifact projection -> local `repo/space/` (`.sync/remote-sources` and `artifacts/`).

## Related Glossary

- [attach](/reference/glossary/attach)
- [authoritative_workspace](/reference/glossary/authoritative_workspace)
- [workspace_mode](/reference/glossary/workspace_mode)
- [profile](/reference/glossary/profile)
- [resume](/reference/glossary/resume)
- [status](/reference/glossary/status)

## Change Log

- `2026-03-23`: Added `cccc actor profile ...` reference coverage so reusable
  profile management is documented as a first-class CLI surface.
- `2026-03-23`: Added `profile` glossary alignment so CLI actor lifecycle notes distinguish reusable runtime config from live runtime evidence.

- `2026-03-21`: Aligned CLI wording with the new local glossary so `attach`, `resume`, and `status` stop drifting between shorthand and canonical repo-local meaning.

## Setup Commands

### `cccc setup`

Configure MCP for an agent runtime.

```bash
cccc setup --runtime claude        # Auto-configure for Claude Code
cccc setup --runtime codex         # Auto-configure for Codex
cccc setup --runtime kimi          # Auto-configure for Kimi CLI
```

## Web Commands

### `cccc web`

Start only the Web UI (daemon must be running).

```bash
cccc web                           # Start Web UI
cccc web --port 9000               # Custom port
```

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
| `CCCC_WEB_HOST` | `127.0.0.1` | Web UI bind address |
| `CCCC_WEB_PORT` | `8848` | Web UI port |
| `CCCC_LOG_LEVEL` | `INFO` | Log level |
