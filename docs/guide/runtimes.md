# Supported Runtimes

CCCC can run multiple agent runtimes in the same working group. Each actor chooses one runtime, while the daemon keeps messaging, delivery tracking, tasks, context, and Web/IM control in one shared CCCC group.

Use `cccc runtime list --all` to see the full supported list on your machine, and `cccc doctor` to check which CLI runtimes are installed.

## First-Class Runtimes

| Runtime | Runtime id | Entrypoint / surface | MCP setup |
|---------|------------|----------------------|-----------|
| Claude Code | `claude` | `claude` | Auto |
| Cline CLI | `cline` | `cline` | Auto |
| Codex CLI | `codex` | `codex` | Auto |
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

Claude Code and Codex CLI support both PTY and headless operation. Most other CLI runtimes, including Cline, use PTY. ChatGPT Web Model is fixed to browser delivery plus a remote MCP connector.

Cline is currently integrated as a fresh-start PTY runtime. CCCC does not persist or reuse Cline's `--id` session identifier, so stopping and starting a Cline actor opens a new Cline TUI session.

### Codex and Claude PTY Hook State

The Rust and Python daemons do not parse PTY output to infer activity for eligible provider sessions. Codex PTY activity comes from lifecycle hooks injected only into processes that CCCC starts: prompt and tool events report `working`, permission requests report `waiting`, and verified stop/session events report `idle` or `stopped`. CCCC registers only events in the current [Codex Hooks contract](https://developers.openai.com/codex/hooks); non-zero tool commands still complete through `PostToolUse`, and Codex does not currently expose separate `PostToolUseFailure` or `StopFailure` events. Every injected hook process carries a per-launch fence, and Codex turn-scoped events must identify the active provider turn or its bound tool operation. Tool operations are observed serially: a second operation cannot start before the active one closes, and operation-specific events must carry the exact active operation ID. Late events from an older launch, session, turn, or operation cannot overwrite current state.

Turn and per-turn operation identity histories have a hard 4096-entry safety bound and never evict entries. Reaching a bound fails closed instead of making an old identity reusable: turn exhaustion revokes the active turn for the rest of that session, while operation exhaustion revokes operation writes for the current turn. The corresponding working-state reasons are `codex_hook_turn_fence_exhausted` and `codex_hook_operation_fence_exhausted`.

Claude PTY hooks do not provide one stable turn identifier across prompt, tool, permission, notification, and stop events. CCCC therefore treats them fail-closed: fenced `SessionStart` and `SessionEnd` establish only the session boundary, normal `terminal_write` opens a local `working` generation, and Esc or Ctrl-C closes it to `idle`. Claude PTY prompt/tool/permission/notification/stop hooks cannot change working state, so permission `waiting` and automatic stop `idle` are intentionally not claimed as precise. The API exposes this limitation through `effective_working_reason` values prefixed with `claude_pty_fail_closed_`. Claude headless sessions are unchanged and continue to use structured provider events for precise turn lifecycle.

Both integrations are session-only. CCCC generates a new launch fence for every actual provider process, including a direct `codex resume`, and passes it only through that process environment. An already-running actor keeps its existing fence. Before a new process is started, the launch transaction invalidates the prior capability and writes the new token's pending or unavailable baseline under the same state lock; a late event can therefore finish before that transaction and be overwritten, or arrive afterward and fail its token fence. Codex receives command-line hook overrides and their exact trust hashes. For a direct Claude Code command, CCCC reads Claude's effective final `--settings` value, preserves its fields and existing hooks, and replaces duplicate CLI settings arguments with one merged inline document. Relative settings paths resolve from the actor working directory; the source file is never modified. Wrapper and alternate commands are not mutated. CCCC does not write `~/.codex`, `~/.claude`, or project settings files, and sessions launched outside CCCC do not run the CCCC status hook. Version 2 hook-state files remain readable for diagnostics but are reported as `legacy_unfenced`; configuring a new launch replaces them with a fenced version 3 pending state, and tokenless legacy events cannot unlock it.

Claude PTY hook state requires Claude Code 2.1.141 or newer, confirmed by a successful `--version` probe. Wrapper commands, alternate commands, failed probes, and older Claude versions remain on the prior PTY state source and are not mutated; their newly written unavailable baseline prevents a stale hook file from being treated as current. For an otherwise eligible direct command, a settings merge, hook executable, or spawn failure records a specific `HookUnavailable…` launch reason and fails closed instead of silently falling back to terminal-text inference. Enterprise policy, `disableAllHooks`, and safe/bare modes can still prevent a valid injected hook from running; that remains visible as a pending hook reason.

The Rust and Python backends share the same version 3 hook-state and version 1 runtime-activity disk schemas, paths, advisory locks, and committed-write protocol: flush and sync the temporary file, atomically replace the destination, then sync the parent directory where the platform supports it. Independent-process tests exercise bidirectional state/activity reads and cross-language lock exclusion. The same contract test is enabled for Windows CI; this change was locally exercised on macOS, not on a Windows host. Switching backends is supported by stopping the old daemon, starting the new daemon, and restarting the actor; running two daemons against the same `CCCC_HOME` is not supported.

Verified PTY hook events also feed the Web runtime activity ticker. This is a separate, short-lived observability channel rather than chat history: it carries only structured lifecycle fields, replays briefly after reconnects, and detects long-running turn or tool activity. See [PTY Runtime Activity](/guide/runtime-activity) for the event contract, retention, and privacy boundaries.

In the Rust backend, `runtime=codex|claude` with `runner=headless` starts a daemon-managed provider process. Codex uses its app-server JSON-RPC transport and Claude uses bidirectional stream-json. Messages are delivered automatically, provider health determines the actor's `running` value, and stopping the actor or group terminates the provider process. Headless state comes from these structured provider protocols rather than the PTY hooks.

`web_model` and custom external headless actors keep the pull-consumer contract: an external executor calls `cccc_runtime_wait_next_turn` and `cccc_runtime_complete_turn`. These actors do not claim to have a local provider process.

CCCC also preserves current Grok Build PTY sessions with its native `--session-id` and `--resume` flags. A fresh actor launch receives a CCCC-owned UUID; later starts resume that exact actor session rather than using Grok's directory-wide `--continue` selection. Commands that already contain Grok session-control flags remain user-owned and are not rewritten. Set `CCCC_RUNTIME_RESUME=0` to disable provider-session reuse globally.

## ChatGPT Web Model

`web_model` does not use `cccc setup`. Create the ChatGPT Web Model actor from the CCCC Web group, then finish sign-in, MCP URL setup, and conversation binding in **Settings > ChatGPT Web Model**.

This runtime works with MCP-capable GPT-5.x ChatGPT Web sessions. GPT-5.x Pro sessions are advisory-only for this integration because they do not expose third-party MCP connectors.

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

Before the Rust daemon creates an automatically managed PTY runtime session, it checks that the runtime's `cccc` MCP entry points to the active public CCCC executable. Missing entries are installed, stale user/global entries are replaced when the runtime provides a safe removal command, and the result is verified before the actor process starts. Codex keeps its actor-scoped launch override. A stale entry from a more specific project or non-user scope fails with an actionable error instead of being silently overwritten. Prompt-assisted runtimes (`cursor`, `kilo`, and `antigravity`) retain their startup setup contract, while `custom` remains manual.

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
