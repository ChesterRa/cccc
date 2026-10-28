# CLI Reference

Complete command reference for the CCCC CLI.

## Global Commands

### `cccc`

Start the daemon and the Web UI, reusing an existing daemon or starting one if
needed.

```bash
cccc                    # In a terminal: daemon + Web UI, with the terminal interface
cccc --headless         # Daemon + Web UI with plain log output (same as `cccc web`)
cccc --help             # Show help
```

In an interactive terminal, `cccc` opens a terminal interface for this launch.
Its main screen is a status panel: the version, the actual Web address, the
`CCCC_HOME` in use, whether this launch started the daemon, the Groups, and the
native sessions this CCCC knows. Terminals that recognize URLs open the Web
address directly, for example with Ctrl+click in Windows Terminal. Lists refresh
every few seconds while they are on screen; a failed refresh keeps the last list
and says how old it is.

The Groups table has NAME, STATUS and RUNNING columns. STATUS uses the Web UI's
words: Running, Idle, Paused, Offline and Stopped. Stopped means the Group was
stopped. Offline means it was not stopped or paused, but no actor is running:
for example a new Group, or one whose actors exited or whose native sessions are
closed. RUNNING counts the managed actors that are running and the native
members ready to receive from that Group; it is neither the number of members
nor the number of busy models.

On the main screen, Up/Down selects a Group and Enter opens it in `/groups`.
Type `/` to list the commands: Up/Down moves through the list, Tab completes the
selected command without running it, Enter runs it, and Esc closes the list.
Left/Right, Home/End, Delete and Ctrl+A/E/U edit the command line. Pasting only
inserts text: in terminals with bracketed paste, pasted newlines do not run a
command. Commands:

- `/groups` lists the Groups. Pick one with Up/Down (or j/k) to see its recorded
  state, how many tasks it has in each state, its actors and a link that opens
  it in the Web UI. `s` starts it (resumes a paused one) and `x` stops it, each
  after a confirmation that names the Group and what will happen. Starting runs
  the enabled actors and lets enabled native members receive again; stopping
  stops the managed actors and stops delivery to native members, whose apps stay
  open. `t` lists the Group's tasks: the unfinished ones (Active, then Planned),
  or the Done ones after `d`; PgUp/PgDn pages through a long list. Enter shows
  one task read-only, with its status, assignee, checklist, notes and outcome;
  tasks are edited in the Web UI. `b` or Esc goes back one level.
- `/sessions` lists every native session this CCCC knows. Enter shows the
  selected session's details, and `b` or Esc goes back.
- `/setup` connects Codex, Claude Code, Pi or OpenCode sessions; see
  [Native sessions](../guide/local-sessions).
- `/help` lists the commands and keys. It scrolls with Up/Down when the terminal
  is short.
- `/quit` (or `/exit`, or Ctrl+C on any page) stops what this launch started:
  the Web server, and the daemon when this launch started it. A daemon that was
  already running keeps running. If stopping takes too long, a second Ctrl+C
  forces an exit (after `/quit`, press Ctrl+C twice). After the interface
  closes, the terminal shows what actually stopped.

The interface is English only and needs at least 60x16 characters; on a wider
terminal it stays within 100 columns. In a smaller terminal it shows only a
notice and accepts only Ctrl+C, so nothing can be confirmed unseen. It does not use the mouse; Ctrl+Z does not
suspend it (use `/quit`), and Ctrl+L redraws the screen. `NO_COLOR` turns its
colors off.

`cccc` runs headless, with plain log output and no terminal interface, when
given `--headless`, when its input or output is not a terminal, or when
`CCCC_WEB_MODE` / `CCCC_WEB_READONLY` select read-only Exhibit mode. Use
`--headless` on servers, in containers, under a service manager and in scripts.
It applies only to this launch, so `cccc --headless` with a subcommand is an
error. Other options keep the interface: `cccc --port 9000` in a terminal opens
it on port 9000.

### `cccc doctor`

Check your environment and diagnose issues.

```bash
cccc doctor             # Full environment check
```

Browser readiness uses the same Chrome/Edge discovery as the projected Web runtime on
Linux, macOS, and Windows, and may additionally recognize system Chromium.
On Linux, the report includes projected-browser readiness: system Chrome/Edge, required `Xvfb`
isolation, and the optional `x11vnc` VNC viewer. A missing `x11vnc` does not prevent browser
isolation; CCCC falls back to its CDP screencast viewer.

The installation section reports the executable handling the current invocation,
the first `cccc` selected by PATH, and all other `cccc` commands. A `CONFLICT`
status means an older installation is ahead of the current launcher; move the
current launcher's directory to the front of PATH and open a new terminal.

### `cccc runtime list`

List available agent runtimes.

```bash
cccc runtime list       # List supported runtimes and whether each is available
cccc runtime list --all # Also include the custom runtime
```

## Local native sessions

`cccc sessions enable|disable --runtime codex|claude|pi|opencode` installs or removes the same-user native integration; `/setup` in the terminal interface runs the same enable step for the apps you pick. Use `cccc sessions list [--include-offline]`, `send --to <source_id> --text <text>`, and `create --sources <ids...>` to collaborate without pairwise invitations.

Use `cccc sessions join --group <group_id> --sources <ids...>` to add sources to an existing Group, or `leave --group <group_id> --from <source_id>` to pause one source's access without stopping its native process. `recovery --group <group_id> --actor <actor_id>` reads local recovery instructions; adding `--refresh` explicitly rechecks Codex's existing receive channel. Neither form starts or resumes the native application or replays messages. See [Native sessions](../guide/local-sessions) for requirements, reload steps and lifecycle semantics.

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
- Check `cccc daemon status` and `cccc doctor` before restarting an unresponsive daemon. `cccc daemon stop` stops its managed processes across Groups; after it completes, `cccc daemon start` starts the daemon again.

## Membership and Reach Commands

Membership is optional. Local CCCC remains usable without an account. Reach is
a Linux/macOS preview and requires an Admin Access Token in **Settings > Web
Access** before publishing the local Web surface.

```bash
cccc login             # Bind this machine through device authorization
cccc logout            # Retire this account device and clear its local identity
cccc reach install     # Install or upgrade the pinned cloudflared helper
cccc reach on          # Publish the authenticated Web surface
cccc reach status      # Show account, hostname, Web, and publication state
cccc reach off         # Stop publication without removing local identity
```

The token-bearing Web URL is shown here; keep it private. ChatGPT/Web Model
connector credentials are actor-specific and are managed from that actor's Web
Model settings. Windows helper installation is not included in this release
candidate, so Reach is unavailable there.

## Group Commands

### `cccc attach`

Create a working group with a project scope, or attach a scope to an explicitly
selected group. The selected group becomes active for subsequent commands.

```bash
cccc attach .           # Attach current directory as scope
cccc attach /path/to/project
cccc attach . --group <group_id> # Attach a scope to an existing Group
```

Without `--group`, each invocation creates a new Group. To select an existing
Group without attaching a scope, use `cccc groups` and `cccc use <group_id>`.
Relative paths in `cccc attach` and `cccc group use` resolve from the invoking
terminal's directory, independently of where the daemon was started.

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

`set-state stopped` is the same as `group stop`: it stops the Group's actors and
delivery to its native members, never only its recorded state.

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

Actor scope arguments are project paths at the CLI boundary. CCCC resolves them
to the attached scope key before persistence.

## Message Commands

### `cccc send`

Send a message.

```bash
cccc send "Hello"                  # No --to: default recipient policy applies (default: foreman)
cccc send "Hello" --to @foreman    # Send to foreman
cccc send "Hello" --to peer-1      # Send to specific actor
cccc send "Announcement" --to @all # Explicit broadcast
cccc send "Please answer" --to peer-1 --mode request-reply
cccc send "For later" --to peer-1 --mode mail
cccc send "Review this scope" --path src/api
```

`--mode` accepts `send`, `request-reply`, or `mail` and defaults to `send`.
`request-reply` requires concrete recipients; `mail` remains in Inbox without an
immediate runtime prompt.

### `cccc tracked-send`

Create a task and send one linked delegation message.

```bash
cccc tracked-send "Please implement this and reply with validation evidence." \
  --to peer-1 \
  --title "Implement feature" \
  --outcome "Feature is implemented and validation evidence is reported"
```

The CLI also forwards `--checklist`, `--assignee`, `--waiting-on`, `--handoff-to`,
`--notes`, `--task-priority`, and `--idempotency-key` to the daemon. The linked
message always uses Send.

### `cccc reply`

Reply to a message.

```bash
cccc reply <event_id> "Reply text" --to peer-1
cccc reply <event_id> "Reply for later" --to peer-1 --mode mail
```

`--mode` accepts `send` or `mail` and defaults to `send`. Both modes close a
matching Send + Reply obligation; Mail stores the reply without immediately
prompting the recipient. Mail is agent-only. A send or reply may address
`user` alone or one/more agents, but never both in the same message.

### `cccc deliver`

Promote an existing Mail to Send, or retry a blocked/failed delivery, without
creating another chat message.

```bash
cccc deliver <event_id> --to peer-1
cccc deliver <event_id> --to peer-1 --force-ambiguous
```

`--force-ambiguous` is required when prior delivery may already have reached the
runtime and therefore may produce a duplicate prompt.

### `cccc cancel-reply`

Cancel every still-open reply obligation for an existing Send + Reply message.

```bash
cccc cancel-reply <event_id>
```

### `cccc inbox`

Read and consume an actor's unread Mail, advancing its read cursor. This changes
the actor's unread state; it is not a read-only view of the actor's responses.
Use `cccc tail` or Web chat to view recorded messages without consuming Mail.

```bash
cccc inbox --actor-id <id>         # Read and consume the next unread Mail batch
cccc inbox --actor-id <id> --limit 10
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
cccc im set mattermost --mattermost-url https://mattermost.example.com --bot-token-env MATTERMOST_BOT_TOKEN
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

cccc space ingest --kind context_sync --payload '{"vision":"v0.5 plan"}'
cccc space ingest --kind resource_ingest --payload '{"source_type":"file","file_path":"space/spec.md","title":"Spec"}' --idempotency-key ingest-file-1
cccc space ingest --kind resource_ingest --payload '{"source_type":"web_page","url":"https://example.com/spec"}' --idempotency-key ingest-url-1

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
- The native client follows the `notebooklm-py` v0.8.1 protocol baseline and
  supports attached-scope local files, pasted text, Web URL, YouTube, and
  Google Drive Docs/Slides/Sheets ingestion.
- Explicit ingest persists its job before one provider attempt. Failed jobs are
  retried only with `cccc space jobs retry`; there is no hidden background
  ingest retry loop.
- Automatic two-way repo and daily-memory mirroring is retired for 0.4.36.
  Use explicit ingest/source operations; Rust still reads legacy 0.4.35 sync
  metadata in status views without resuming remote mutations.
- Artifact generation is asynchronous and does not save locally by default.
  Request wait/save explicitly when a local artifact is required. Native Rust
  download currently supports media, report/study-guide, infographic, and
  slide-deck outputs; interactive quiz/flashcard/mind-map and data-table
  downloads are intentionally unavailable; generate/list still work and the
  capability matrix reports the boundary.

## Product Implementation

### Rust-only 0.4.36

`cccc` directly runs the native product executable. Product-engine selectors
are retired: `cccc python` and `cccc rust` are no longer commands, and an old
`CCCC_HOME/implementation.json` file is ignored rather than migrated or used as
a runtime choice. Use ordinary commands directly:

```bash
cccc                 # launch daemon + Web
cccc status          # show product, daemon, groups, actors, and agent runtimes
cccc doctor          # inspect the native installation and environment
cccc daemon start    # explicit daemon lifecycle
```

The former `ccccd` executable is also retired. Scripts should use
`cccc daemon start|stop|status|run`. Compatible daemon state filenames such as
`ccccd.addr.json` remain unchanged so a 0.4.35 home can be adopted safely.

## Setup Commands

### `cccc setup`

Prepare or report the MCP integration for an agent runtime.

```bash
cccc setup                         # Configure every supported runtime; unavailable CLIs are reported
cccc setup --runtime claude        # Report CCCC-owned per-session MCP for Claude Code
cccc setup --runtime codex         # Auto-configure for Codex
cccc setup --runtime copilot       # Auto-configure for GitHub Copilot CLI
cccc setup --runtime cursor        # Show prompt-assisted setup contract for Cursor CLI
cccc setup --runtime devin         # Auto-configure for Devin CLI
cccc setup --runtime kiro          # Auto-configure for Kiro CLI
cccc setup --runtime kimi          # Configure native Kimi Code MCP
cccc setup --runtime kimi --runtime-mode acp  # Report session-owned MCP
cccc setup --runtime auggie --runtime-mode acp # Report session-owned MCP
cccc setup --runtime kiro --runtime-mode acp   # Official V3; session-owned MCP
cccc setup --runtime hermes --runtime-mode acp # Official ACP; session-owned MCP
cccc setup --runtime droid --runtime-mode acp  # Official ACP; session-owned MCP
cccc setup --runtime pi            # Report CCCC-owned RPC session MCP
cccc setup --runtime kilo          # Show prompt-assisted setup contract for Kilo Code CLI
cccc setup --runtime antigravity   # Show prompt-assisted setup contract for Antigravity CLI
```

Without `--runtime`, setup performs one batch pass: installed CLI runtimes are configured,
prompt-assisted/manual runtimes return their configuration contract, and missing runtimes are
reported without aborting the remaining setup work.

### `cccc update`

Upgrade a website-installer-owned CCCC executable.

```bash
cccc update                        # Upgrade from stable GitHub Releases
cccc update --check                # Query latest release, ownership, and platform requirements
cccc update --check --offline      # Inspect local details without a network request
cccc update --channel stable       # Force the stable GitHub Release channel
cccc update --channel rc           # Force the prerelease GitHub Release channel
```

Notes:
- A stable build defaults to `stable`; a prerelease build defaults to `rc`.
- `--check` is read-only for standalone, pip-owned, and unmanaged installations.
  It reports the exact executable, ownership, build platform requirements,
  current version, latest channel version, and the appropriate next step. It
  does not replace files, run pip, stop services, or grant standalone ownership.
- A failed online check returns a nonzero exit code and retains local details
  with an explicit unknown update status. `--offline` requires `--check`, makes
  no network request, and labels the latest version as not checked. It does not
  imply that the installed version is current.
- Website-installer installations reuse the GitHub Pages installer and preserve
  their current install directory.
- Update discovery reads `https://chesterra.github.io/cccc/releases.json`, a small
  static file published with the installers, rather than querying the GitHub
  Releases API on each user's machine. No GitHub token is required. Archives
  still come from GitHub Releases and retain the installer's checksum checks.
- The Pages workflow selects only published releases with the complete installer
  asset set. Its versioned document has the shape
  `{"schema_version":1,"repository":"ChesterRa/cccc","channels":{"stable":"0.4.37","rc":null}}`.
  `rc` contains the latest complete prerelease, or `null` when none exists. The
  installer default and `stable` are generated from the same release snapshot;
  a discovery/build failure must not replace the live Pages deployment.
- Version selection follows release precedence, with CCCC's `alphaN`, `betaN`,
  and `rcN` suffixes compared numerically (for example, `rc10` follows `rc2`).
  A same-version update is a no-op. Ordinary updates never install an older
  version, including when Pages is briefly stale. An explicit switch to the
  other channel, such as `--channel stable` from an RC build, may install an
  older release of that channel. Missing channels and invalid metadata fail
  before invoking the installer; there is no fallback to the rate-limited API.
- Fork distributors using `CCCC_GITHUB_REPOSITORY` must also set
  `CCCC_RELEASE_INDEX_URL` to their HTTPS static index. The index's `repository`
  must match the configured repository. The standard installation needs neither
  setting; these values never carry credentials.
- Older installed versions still use their old discovery code. Once a fixed
  release is published, rerun the original installation command to upgrade an
  installation whose old `cccc update` is blocked by API rate limits.
- Pip-owned, source-tree, and other markerless executables are not updated by
  this command; they can still use `--check`. Pip users run
  `python -m pip install -U "cccc-pair>=0.4.36"`; the wheel contains the same
  native executable and no Python runtime or fallback. Run `cccc daemon stop`
  and close foreground CCCC processes before asking pip to replace it.
- A standalone update restarts the daemon when it was running, while the old
  combined Web process remains stopped; the next bare `cccc` starts the updated
  Web process.
- On Windows, standalone self-update continues in a separate PowerShell process
  after the original executable exits so the binary can be replaced safely.

## Web Commands

### `cccc web`

Start the Web UI, reusing an existing daemon or starting one if needed, just
like `cccc --headless`: never with the terminal interface, always with plain log
output. Unlike `cccc --headless`, it accepts `--exhibit` (or `--mode exhibit`)
for a read-only Web UI.

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

For `cccc` and `cccc web`, an existing live Web binding is reused when neither
`--host` nor `--port` is supplied. Otherwise, each value resolves from its command
option, saved **Web Access** setting, environment variable, then built-in default.
