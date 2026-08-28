# Supported Runtimes

CCCC can run multiple agent runtimes in the same working group. Each actor chooses one runtime, while the daemon keeps messaging, delivery tracking, tasks, context, and Web/IM control in one shared CCCC group.

Use `cccc runtime list --all` to see the full supported list on your machine, and `cccc doctor` to check which CLI runtimes are installed.

## First-Class Runtimes

| Runtime | Runtime id | Entrypoint / surface | MCP setup |
|---------|------------|----------------------|-----------|
| Claude Code | `claude` | `claude` | Auto |
| Cline CLI | `cline` | `cline` | Auto |
| Codex CLI | `codex` | `codex` | Auto |
| DeepSeek Harness | `deepseek` | CCCC-managed `dsh-acp-demo` (headless ACP) | Automatic on first start; explicit setup remains available |
| GitHub Copilot CLI | `copilot` | `copilot` | Auto |
| Cursor CLI | `cursor` | `cursor-agent` | Prompt-assisted |
| Devin CLI | `devin` | `devin` | Auto |
| Kiro CLI | `kiro` | `kiro-cli` | Auto |
| Kilo Code CLI | `kilo` | `kilo` | Prompt-assisted |
| Antigravity CLI | `antigravity` | `agy` | Prompt-assisted |
| Droid CLI | `droid` | `droid` | Auto |
| Amp | `amp` | `amp` | Auto |
| Auggie (Augment) | `auggie` | `auggie` | Auto |
| Grok Build | `grok` | `grok` | Auto |
| Hermes Agent | `hermes` | `hermes` | Auto through the user's Hermes profile |
| Kimi CLI | `kimi` | `kimi` | Auto |
| OpenCode | `opencode` | `opencode` | Auto via launch config |
| ChatGPT Web Model | `web_model` | Bound ChatGPT Web conversation | Browser delivery + remote MCP connector |

`custom` is also supported as a manual fallback for any command-line agent that can be launched by CCCC.

## Autonomy Defaults

CCCC applies runtime-specific launch defaults for actors it starts. These defaults are intended to keep agent sessions moving without repeated approval prompts, while still leaving actor/profile commands editable in the Web settings.

| Runtime id | Default command | Permission / autonomy behavior |
|------------|-----------------|--------------------------------|
| `claude` | `claude --dangerously-skip-permissions` | Skips Claude Code permission prompts. |
| `cline` | `cline --tui --auto-approve true` | Opens Cline's interactive TUI and enables tool auto-approval. |
| `codex` | `codex -c shell_environment_policy.inherit=all --dangerously-bypass-approvals-and-sandbox --search` | Bypasses Codex approvals/sandbox and preserves actor environment inheritance for MCP subprocesses. |
| `deepseek` | CCCC-managed `dsh-acp-demo --config …/cordis.yml` | Official ACP app composition; provider permission requests are rejected rather than implicitly approved. |
| `copilot` | `copilot --allow-all` | Allows Copilot CLI tool execution without per-action approval. |
| `cursor` | `cursor-agent --yolo --approve-mcps` | Uses Cursor YOLO mode and approves MCP usage. |
| `devin` | `devin --permission-mode dangerous` | Uses Devin's dangerous permission mode. |
| `kiro` | `kiro-cli chat --trust-all-tools` | Trusts Kiro tools for the session. |
| `antigravity` | `agy --dangerously-skip-permissions` | Skips Antigravity tool permission prompts. |
| `droid` | `droid --auto high` | Starts Droid in high-autonomy mode. |
| `grok` | `grok --always-approve` | Starts Grok Build with approval prompts bypassed. |
| `hermes` | `hermes --tui --yolo` | Starts Hermes in TUI YOLO mode. |
| `kimi` | `kimi --yolo` | Starts Kimi in YOLO mode. |
| `opencode` | `opencode --auto` | Auto-approves OpenCode permission requests that are not explicitly denied. |
| `amp` | `amp` | No extra CCCC launch flag; Amp's current CLI default is already direct tool execution. |
| `auggie` | `auggie` | Use Auggie permissions or settings for per-tool approval policy; CCCC does not inject a broad wildcard permission rule. |
| `kilo` | `kilo` | Use Kilo's `kilo.jsonc` permission settings or Auto Approve UI for broad approval policy. |
| `web_model` | N/A | Browser-delivered runtime; local CLI launch flags do not apply. |
| `custom` | User command | CCCC preserves the user-provided command exactly. |

## Setup Commands

Most CLI runtimes can be prepared with `cccc setup --runtime <id>`:

```bash
cccc setup --runtime claude
cccc setup --runtime cline
cccc setup --runtime codex
cccc setup --runtime deepseek
cccc setup --runtime copilot
cccc setup --runtime devin
cccc setup --runtime kiro
cccc setup --runtime droid
cccc setup --runtime amp
cccc setup --runtime auggie
cccc setup --runtime grok
cccc setup --runtime hermes
cccc setup --runtime kimi
cccc setup --runtime opencode
```

DeepSeek Harness is an upstream developer preview, so CCCC owns and isolates the tested ACP composition. On first use, it installs only the four required packages (`dsh-acp`, `dsh-mcp-client`, `dsh-acp-demo`, and `dsh-llm-deepseek`) under `CCCC_HOME/runtimes/deepseek/<release>`. Exact direct versions plus an npm release cutoff keep every transitive `@deepseek-ai/dsh*` package on the same validated preview release. The managed LLM adapter caps output at 65,536 tokens so prompt and MCP tool context retain headroom inside the model window. Setup also prunes the obsolete direct `dsh` bundle and its managed profile patch from earlier preview installs. CCCC does not modify `~/.dsh` or a project `package.json`; the legacy one-shot `dsh --profile cccc-acp` path and its unused bundle profile are not used. Concurrent starts share one setup lock, and a failed installation remains retryable. Running `cccc setup --runtime deepseek` performs the same idempotent setup eagerly. Provider credentials such as `DEEPSEEK_API_KEY` remain deployment inputs and are never generated or persisted by setup.

Prompt-assisted runtimes print an idempotent setup prompt or contract that you run inside that runtime:

```bash
cccc setup --runtime cursor
cccc setup --runtime kilo
cccc setup --runtime antigravity
```

For a custom runtime, provide the command when creating or editing the actor:

```bash
cccc actor add worker --runtime custom --command "my-agent --with-flags"
```

## Runner Modes

Actors normally run in one of two modes:

- **PTY**: the runtime runs in an embedded terminal. This is the broadest compatibility mode.
- **Headless**: CCCC manages structured runtime I/O without a terminal. This gives tighter delivery and streaming control where supported.

Claude Code and Codex CLI support both PTY and headless operation. DeepSeek Harness is fixed to headless ACP operation. Most other CLI runtimes, including Cline, use PTY. ChatGPT Web Model is fixed to browser delivery plus a remote MCP connector.

Cline is currently integrated as a fresh-start PTY runtime. CCCC does not persist or reuse Cline's `--id` session identifier, so stopping and starting a Cline actor opens a new Cline TUI session.

### PTY delivery and recovery

A successful Send means that CCCC durably appended the message and attempted a
runtime handoff; it does not prove that the provider application understood or
acted on the text. For each concrete recipient, `runtime.delivery` records
`claimed` before external I/O and then `accepted`, `failed`, or `ambiguous`.
Concurrent claimants treat `claimed` as in progress. On daemon restart, a claim
without an outcome is settled to `ambiguous` and is not retried automatically.

Current-generation Send work with no accepted/ambiguous evidence can be
recovered in ledger order after actor/group activation. Mail is never promoted
by recovery: it remains in the Inbox until `cccc_inbox_read`, apart from the
single bounded content-free Mail notice. Within the current actor generation,
legacy `chat.read.event_id` remains an inclusive ledger watermark rather than a
per-event receipt. Recovery excludes `system.notify` records at or before the
furthest valid watermark, plus later notices that reference an event in that
read prefix, so an upgrade cannot replay old unread nudges into a new provider
session. Runtime handoff never advances the Inbox cursor.
Restarting a provider process does not transfer its input mode, preamble memory,
or hot terminal ring; durable ledger, Mail cursor, reply-obligation, and
runtime-delivery facts remain the recovery authority.

### Codex and Claude PTY Hook State

The daemon does not parse PTY output to infer activity for eligible provider
sessions. Codex PTY activity comes from lifecycle hooks injected only into
processes that CCCC starts: prompt and tool events report `working`, permission
requests report `waiting`, and verified stop/session events report `idle` or
`stopped`. CCCC registers only events in the current
[Codex Hooks contract](https://developers.openai.com/codex/hooks); non-zero tool
commands still complete through `PostToolUse`, and Codex does not currently
expose separate `PostToolUseFailure` or `StopFailure` events. Every injected
hook process carries a per-launch fence, and Codex turn-scoped events must
identify the active provider turn or its bound tool operation. Tool operations
are observed serially: a second operation cannot start before the active one
closes, and operation-specific events must carry the exact active operation ID.
Late events from an older launch, session, turn, or operation cannot overwrite
current state.

Turn and per-turn operation identity histories have a hard 4096-entry safety bound and never evict entries. Reaching a bound fails closed instead of making an old identity reusable: turn exhaustion revokes the active turn for the rest of that session, while operation exhaustion revokes operation writes for the current turn. The corresponding working-state reasons are `codex_hook_turn_fence_exhausted` and `codex_hook_operation_fence_exhausted`.

Claude PTY hooks do not provide one stable turn identifier across prompt, tool, permission, notification, and stop events. CCCC therefore treats them fail-closed: fenced `SessionStart` and `SessionEnd` establish only the session boundary, normal `terminal_write` opens a local `working` generation, and Esc or Ctrl-C closes it to `idle`. Claude PTY prompt/tool/permission/notification/stop hooks cannot change working state, so permission `waiting` and automatic stop `idle` are intentionally not claimed as precise; `PostToolUseFailure` is still registered so the separate runtime-activity projection can close a failed tool instead of leaving it active. The API exposes this limitation through `effective_working_reason` values prefixed with `claude_pty_fail_closed_`. Claude headless sessions are unchanged and continue to use structured provider events for precise turn lifecycle.

Both integrations are session-only. CCCC generates a new launch fence for every actual provider process, including a direct `codex resume`, and passes it only through that process environment. An already-running actor keeps its existing fence. Before a new process is started, the launch transaction invalidates the prior capability and writes the new token's pending or unavailable baseline under the same state lock; a late event can therefore finish before that transaction and be overwritten, or arrive afterward and fail its token fence. Codex receives command-line hook overrides plus exact per-hook trust hashes for only the CCCC commands injected through session flags. Project, plugin, and user hooks keep their own Codex trust decisions, and CCCC does not use the global hook-trust bypass flag. CCCC first validates the session Hook configuration with a bounded local Codex config probe; an unsupported, failed, or timed-out probe records `HookUnavailableSettings` and launches the original Codex command without Hook injection. For a direct Claude Code command, CCCC reads Claude's effective final `--settings` value, preserves its fields and existing hooks, and replaces duplicate CLI settings arguments with one merged inline document. Relative settings paths resolve from the actor working directory; the source file is never modified. Wrapper and alternate commands are not mutated. CCCC does not write `~/.codex`, `~/.claude`, or project settings files, and sessions launched outside CCCC do not run the CCCC status hook. Version 2 hook-state files remain readable for diagnostics but are reported as `legacy_unfenced`; configuring a new launch replaces them with a fenced version 3 pending state, and tokenless legacy events cannot unlock it.

For a direct Codex PTY actor, the same fenced `SessionStart` event is also the
primary source of durable provider session identity. CCCC persists its UUID
only after the launch token and process PID still match the running actor, then
uses `codex resume <session_id>` on the next compatible start. The terminal
`/status` parser remains a delayed compatibility fallback for older or
hook-unavailable Codex versions; current Codex releases no longer need to expose
session identity in human-readable status output.

Claude PTY hook state requires Claude Code 2.1.141 or newer, confirmed by a successful `--version` probe. Wrapper commands, alternate commands, failed probes, and older Claude versions remain on the prior PTY state source and are not mutated; their newly written unavailable baseline prevents a stale hook file from being treated as current. For an otherwise eligible direct command, a settings merge, hook executable, or spawn failure records a specific `HookUnavailable…` launch reason and fails closed instead of silently falling back to terminal-text inference. Enterprise policy, `disableAllHooks`, and safe/bare modes can still prevent a valid injected hook from running; that remains visible as a pending hook reason.

Version 3 hook state and version 1 runtime activity use the canonical
0.4.35-compatible paths, advisory locks, and committed-write protocol: flush
and sync the temporary file, atomically replace the destination, then sync the
parent directory where the platform supports it. Frozen-home migration tests
cover the retained state boundary. Never run two daemons against the same
`CCCC_HOME`.

The Rust daemon also owns the lifetime of every process-backed actor. On Windows, the daemon host and each PTY actor use non-breakaway Job Objects with `KILL_ON_JOB_CLOSE`; Codex and actor-launched MCP descendants inherit containment when they are created, so an abrupt daemon or combined Web-process exit cannot leave them orphaned. On POSIX, each PTY actor is already a separate session and normal stop/reap terminates its entire process group. Process cleanup never removes `group.yaml`, `ledger.jsonl`, or retained `.pty` history.

Verified PTY hook events also feed the Web runtime activity ticker. This is a separate, short-lived observability channel rather than chat history: it carries only structured lifecycle fields, replays briefly after reconnects, and detects long-running turn or tool activity. See [PTY Runtime Activity](/guide/runtime-activity) for the event contract, retention, and privacy boundaries.

`runtime=codex|claude|deepseek` with `runner=headless` starts a daemon-managed
provider process. Codex uses its app-server JSON-RPC transport, Claude uses
bidirectional stream-json, and DeepSeek uses ACP NDJSON through CCCC's fixed
composition. Messages are delivered automatically, provider health determines
the actor's `running` value, and stopping the actor or group terminates the
provider process. Headless state comes from these structured provider protocols
rather than the PTY hooks.

DeepSeek ACP prompts are sent as `ContentBlock[]`. ACP agent-message chunks are
projected to `headless.message.delta` and `headless.message.completed`; turn
boundaries use `headless.turn.started` plus `headless.turn.completed` or
`headless.turn.failed`. This is the same durable event contract used by Web SSE
and reconnect snapshots. The daemon inherits its process environment, then
overlays actor/profile values, but forces the managed `DSH_HOME` into CCCC's
versioned runtime directory. ACP session data is isolated per actor at
`CCCC_HOME/groups/<group_id>/state/deepseek/<actor_id>/sessions`, never in the
attached project. Installation and provider turns each have a 300-second bound.
A timed-out turn is cancelled and recorded as failed only after its terminal
response; if confirmation cannot be obtained, the supervisor is stopped before
the source message remains eligible for retry. Missing credentials and
context-window overflow stop the current runtime and require a lifecycle
start/restart, preventing a permanently invalid request from entering a provider
retry loop. That gate is durable across daemon restarts; daemon restore and
message-triggered auto-wake leave it closed, while a successfully initialized
lifecycle start opens it for the replacement provider process. Existing large
Codex/Claude headless logs receive a one-time streaming dedupe-index migration
when DeepSeek first writes to them, without loading the full log into memory.

For daemon-managed Codex headless turns, a provider status of `failed`, `error`,
or `cancelled`, or an explicit provider error, is persisted as
`headless.turn.failed`; only a successful terminal notification is persisted as
`headless.turn.completed`. Acceptance has already advanced the actor's read
cursor, so a provider failure is not silently retried, but it does release the
session lane for later queued turns.

Daemon-managed Codex runs with non-interactive approval policy. If app-server
nevertheless sends a provider-initiated approval, user-input, elicitation, or
tool request, CCCC returns an explicit JSON-RPC unsupported-method error instead
of hanging the turn or approving it implicitly. Use the PTY runner when the
provider workflow requires interactive approval or input.

Daemon-managed Codex headless actors persist the app-server thread in the
runtime-session state. An ordinary actor stop/start resumes that exact thread
after validating the runtime, workspace, command, model, and saved-state
status. If the provider rejects the resume, CCCC records the failure and starts
a fresh thread. `actor_new_session` deliberately clears the saved thread first,
and `CCCC_RUNTIME_RESUME=0` disables this reuse globally.

Daemon-managed Claude headless actors use the same shared state boundary for
their explicit provider session. A fresh direct `claude` command receives a
CCCC-owned `--session-id`; a compatible ordinary stop/start uses `--resume`
with that same id after validating the runtime,
workspace, stable command, model, and saved-state status. Wrapper commands and
commands that already contain Claude session-control flags remain user-owned
and are not rewritten. `actor_new_session` clears the saved session, and
`CCCC_RUNTIME_RESUME=0` disables reuse. If the provider rejects a saved session
during startup, CCCC reports that start as failed and marks the id ineligible.
If a process survives startup but the first streamed result rejects that same
resume, CCCC records `headless.session.resume_failed`, marks the id ineligible,
and stops that provider session. The next start creates a fresh session rather
than retrying the dead id; CCCC does not hide either failure behind an automatic
retry.

`web_model` keeps the pull-consumer contract: an external executor calls
`cccc_runtime_wait_next_turn` and `cccc_runtime_complete_turn`. The daemon also
exposes this generic pull contract to programmatically configured
`custom+headless` actors; the standard Web actor editor does not currently
expose that combination. These actors do not claim to have a local provider
process.

CCCC also preserves current Grok Build PTY sessions with its native `--session-id` and `--resume` flags. A fresh actor launch receives a CCCC-owned UUID; later starts resume that exact actor session rather than using Grok's directory-wide `--continue` selection. Commands that already contain Grok session-control flags remain user-owned and are not rewritten. Set `CCCC_RUNTIME_RESUME=0` to disable provider-session reuse globally.

For a running Antigravity PTY actor, `actor_new_session` submits the runtime's
native `/clear` command. This creates a new provider conversation while keeping
the authenticated process, project, and terminal sandbox alive. A stopped
Antigravity actor starts normally. Ordinary stop/start behavior remains
process-based and does not claim provider-session resume semantics.

## ChatGPT Web Model

`web_model` does not use `cccc setup`. Create the ChatGPT Web Model actor from the CCCC Web group, then finish sign-in, MCP URL setup, and conversation binding in **Settings > ChatGPT Web Model**.

This runtime works with ChatGPT Web sessions that can use the CCCC MCP connector.
Text-only **Standard** delivery remains the default. The explicitly experimental
**GPT Pro** mode attaches one tiny blank PNG to each delivered batch for accounts
where that ChatGPT-side behavior exposes the connector. CCCC does not select the
ChatGPT model and cannot guarantee that this compatibility workaround will keep
working when ChatGPT changes.

For details, see [ChatGPT Web Model Runtime](/guide/web-model-runtime).

## Choosing a Runtime

Use a mixed group when different agents are good at different roles:

- Use a Claude Code or Codex actor as the foreman when you want strong local coding orchestration.
- Add a second runtime as reviewer to diversify feedback.
- Use ChatGPT Web Model when you want a browser-backed GPT-5.x actor with CCCC MCP access.
- Use `custom` only when the runtime is not first-class yet or needs a special command.

Each actor can have its own runtime, command override, private environment, and runner mode. Runtime state stays in `CCCC_HOME`, not in your repository.

PTY terminal output always uses bounded memory and can optionally persist a bounded per-actor transcript. See [Terminal history](terminal-history.md) for opt-in persistence, retention, cursor, restart, and security behavior.

## Verification and Troubleshooting

```bash
cccc runtime list --all
cccc doctor
```

Common checks:

| Symptom | Check |
|---------|-------|
| Runtime is listed but unavailable | Install the CLI and make sure the command is on `PATH`. |
| MCP tools are missing in the runtime | Run `cccc setup --runtime <id>` or follow the prompt-assisted setup instructions. |
| Custom actor will not start | Ensure `--command` is set; CCCC cannot infer a command for `custom`. |
| Existing actor does not pick up setup changes | Restart the actor after setup or profile changes. |
| ChatGPT Web Model cannot call CCCC | Confirm the public HTTPS MCP URL, ChatGPT connector setup, and bound conversation. |

Before the Rust daemon creates an automatically managed PTY session or launches Claude/Codex directly as a daemon-managed local headless provider, it checks that the runtime's `cccc` MCP entry points to the active public CCCC executable. Missing entries are installed, stale user/global entries are replaced when the runtime provides a safe removal command, and the result is verified before the actor process starts. A failed check, repair, or verification prevents the actor from launching, including during daemon restart recovery. Codex keeps its actor-scoped launch override. A stale entry from a more specific project or non-user scope fails with an actionable error instead of being silently overwritten. Prompt-assisted runtimes (`cursor`, `kilo`, and `antigravity`) retain their startup setup contract, while indirect custom provider commands remain responsible for their own MCP configuration.

This preflight runs before the provider discovers its tools. It therefore repairs Python-to-Rust executable path changes without requiring a second restart. Sessions that were already running when an external MCP configuration changed still need to be restarted because provider tool catalogs are session-scoped.

### Cline installation

Cline's npm package loads a platform-specific optional package. If `cline --version` reports that the platform package is missing, verify that npm is using the official registry, then reinstall with optional dependencies enabled:

```bash
npm config set registry https://registry.npmjs.org/
npm install -g cline --include=optional
cline --version
cccc setup --runtime cline
```

CCCC uses Cline's own noninteractive `mcp add` command and verifies the resulting `cline_mcp_settings.json`; it does not hand-edit Cline's configuration.

The Web UI also exposes runtime detection and actor configuration from the add/edit actor dialogs.
