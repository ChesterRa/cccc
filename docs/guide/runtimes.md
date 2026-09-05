# Supported Runtimes

CCCC can run multiple agent runtimes in the same working group. Each actor chooses one runtime, while the daemon keeps messaging, delivery tracking, tasks, context, and Web/IM control in one shared CCCC group.

Use `cccc runtime list --all` to see the full supported list on your machine, and `cccc doctor` to check which CLI runtimes are installed.

## First-Class Runtimes

| Runtime | Runtime id | Entrypoint / surface | MCP setup |
|---------|------------|----------------------|-----------|
| Claude Code | `claude` | CCCC-managed Agent View session + native TUI | Injected into each managed session |
| Cline CLI | `cline` | `cline` | Auto |
| Codex CLI | `codex` | `codex` | Auto |
| DeepSeek Harness | `deepseek` | CCCC-managed `dsh-acp-demo` (structured ACP, no native terminal) | Automatic on first start; explicit setup remains available |
| GitHub Copilot CLI | `copilot` | `copilot` | Auto |
| Cursor CLI | `cursor` | `cursor-agent` | Prompt-assisted |
| Devin CLI | `devin` | `devin` | Auto |
| Kiro CLI | `kiro` | `kiro-cli` | Auto |
| Kilo Code CLI | `kilo` | CCCC-managed ACP + authenticated native TUI attach | Injected into each managed session |
| Antigravity CLI | `antigravity` | `agy` | Prompt-assisted |
| Droid CLI | `droid` | `droid` | Auto |
| Amp | `amp` | `amp` | Auto |
| Auggie (Augment) | `auggie` | `auggie` | Auto |
| Grok Build | `grok` | CCCC-managed Grok leader + ACP + native TUI | Injected into each managed session |
| Hermes Agent | `hermes` | `hermes` | Auto through the user's Hermes profile |
| Kimi Code | `kimi` | Native TUI | Auto through Kimi Code's MCP config |
| OpenCode | `opencode` | CCCC-managed ACP + authenticated native TUI attach | Injected into each managed session |
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
| `kimi` | `kimi --yolo` | Ask When Needed: routine actions are automatic; risky actions and questions can still ask. Use an explicit `kimi --auto` command for Never Ask mode. |
| `opencode` | `opencode --auto` | CCCC owns the ACP permission boundary and selects only request-scoped one-time approval; it never writes a persistent provider approval. |
| `amp` | `amp` | No extra CCCC launch flag; Amp's current CLI default is already direct tool execution. |
| `auggie` | `auggie` | Use Auggie permissions or settings for per-tool approval policy; CCCC does not inject a broad wildcard permission rule. |
| `kilo` | `kilo` | Same request-scoped ACP approval policy as OpenCode; no persistent provider approval is written. |
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
cccc setup --runtime kilo
```

DeepSeek Harness is an upstream developer preview, so CCCC owns and isolates the tested ACP composition. On first use, it installs only the four required packages (`dsh-acp`, `dsh-mcp-client`, `dsh-acp-demo`, and `dsh-llm-deepseek`) under `CCCC_HOME/runtimes/deepseek/<release>`. Exact direct versions plus an npm release cutoff keep every transitive `@deepseek-ai/dsh*` package on the same validated preview release. The managed LLM adapter caps output at 65,536 tokens so prompt and MCP tool context retain headroom inside the model window. Setup also prunes the obsolete direct `dsh` bundle and its managed profile patch from earlier preview installs. CCCC does not modify `~/.dsh` or a project `package.json`; the legacy one-shot `dsh --profile cccc-acp` path and its unused bundle profile are not used. Concurrent starts share one setup lock, and a failed installation remains retryable. Running `cccc setup --runtime deepseek` performs the same idempotent setup eagerly. Provider credentials such as `DEEPSEEK_API_KEY` remain deployment inputs and are never generated or persisted by setup.

Prompt-assisted runtimes print an idempotent setup prompt or contract that you run inside that runtime:

```bash
cccc setup --runtime cursor
cccc setup --runtime antigravity
```

For a custom runtime, provide the command when creating or editing the actor:

```bash
cccc actor add worker --runtime custom --command "my-agent --with-flags"
```

## Runtime interaction

Users choose a Runtime, not a runner mode. CLI runtimes expose their native terminal so the user can inspect and operate the Actor. Codex, Claude Code, Grok Build, OpenCode, and Kilo additionally run a structured background protocol against the same provider session; CCCC uses that protocol for identity, lifecycle, progress, completion, and cancellation.

Actor messages still enter those managed Runtimes through their native terminal. CCCC does not decide whether a message steers an active turn or waits behind it; the receiving Runtime applies its own configuration. DeepSeek Harness has only a structured ACP surface, while ChatGPT Web Model uses browser delivery plus a remote MCP connector. These are Runtime capabilities, not user-selectable modes.

Cline currently opens a fresh native terminal on each start. CCCC does not persist or reuse Cline's `--id` session identifier.

#### Admission evidence (2026-09-05)

Kilo 7.5.14 was checked with the installed native CLI, isolated provider homes,
and a loopback test model. The shared Actor/Analyst probe covers idle startup,
empty and populated session resume, consecutive results, native TUI cancellation,
and a busy-state follow-up exceeding 16,000 characters. A second probe checks
that a submitted native model/variant selection reaches ACP. These probes do not
use real provider credentials or paid inference. They do not establish native
Windows or macOS behavior; those remain platform validation boundaries.

Windows npm installs (`npm install -g @kilocode/cli`, or a project-local install)
expose `kilo.cmd`. CCCC resolves that official entrypoint to Node plus the
installed Kilo launcher for both ACP and TUI; no manual `kilo.exe` path is needed.
Node must be available beside the npm shim or on the configured `PATH`.
Windows CI separately exercises npm installation layouts, literal arguments and
environment, owned stdio launch, and native terminal launch with an offline fixture.
That fixture does not substitute for full real-Kilo platform validation.

Cline 3.0.61 is **not** admitted as a managed Analyst. Its ACP `session/new`
returns an ID before the core/session history exists; immediately loading that
empty ID reproducibly returns `Resource not found`. The
[upstream ACP implementation](https://github.com/cline/cline/blob/dac3b35/apps/cli/src/acp/acpAgent.ts)
starts its core lazily on a prompt. A shared native TUI/controller would need
additional Hub/session lifecycle work, not simply the existing ACP adapter.
CCCC keeps its current terminal Actor support instead of creating a synthetic
startup prompt or a second Analyst-only session path.

The Linux CI runs the real Codex/Claude empty-session probes and both Kilo probes
against pinned CLI versions. To repeat the Kilo checks locally after building
the current `cccc` binary:

```bash
CCCC_LAUNCHER_PATH="$PWD/target/debug/cccc" \
CCCC_KILO_MANAGED_LIVE=1 CCCC_KILO_MODEL_SYNC_LIVE=1 \
  cargo test -p cccc-pair-daemon --lib --locked live_kilo -- --test-threads=1
```

Codex 0.153.2 Esc was also checked through CCCC against a local test model. The
interrupted turn published `turn_aborted`; an already queued follow-up could then
start a new turn. This is not evidence that CCCC swallowed Esc, so the keyboard
and cancellation paths were not changed. Do not discard queued user messages
merely to make the terminal appear idle.

### Kimi Code

The `kimi` runtime targets [Kimi Code](https://github.com/MoonshotAI/kimi-code)
(`@moonshot-ai/kimi-code`), not the former Python `kimi-cli`. Install the current
client and launch CCCC from a new terminal if its installer changed your `PATH`.
Actor commands, Runtime Profiles, and private environment variables remain the
normal configuration surface; this adapter does not enable Kimi as a Voice Analyst.

Actor startup and `cccc setup --runtime kimi` use the same MCP setup implementation.
It updates only `mcpServers.cccc` in `$KIMI_CODE_HOME/mcp.json`, defaulting to
`~/.kimi-code/mcp.json`, without replacing other servers or malformed files.
It never guesses a data root from a leftover `.kimi` directory or calls the
removed `kimi mcp add` command. A project-level `.kimi-code/mcp.json` takes
precedence: a conflicting `cccc` entry must be corrected or removed by the
operator, and is not silently overwritten or reported as ready.

On first use, complete Kimi's workspace trust and login/model setup in its native
terminal **before sending CCCC messages**. Enabling bracketed paste does not prove
that Kimi has left those dialogs. CCCC does not pre-approve trust or detect
completion of these dialogs; input sent too early may be consumed by a dialog. An
initialized Kimi Code 0.41.0 TUI accepts the normal single Enter submit, including
multiline input. CCCC does not add a second Enter based on older client behavior.

To resume a specific Kimi session, use `kimi --session <id> --yolo` in the Actor's
custom command or Runtime Profile. CCCC preserves that command but does not yet
capture Kimi session IDs or automatically resume them. Avoid adding `--continue`
to a shared profile: it selects the most recent session in the working directory,
which may belong to another Actor. Use Kimi's own `/new` command or remove the
explicit resume argument to start a new provider conversation.

### Delivery and recovery

A successful Send means that CCCC durably appended the message. For each
concrete recipient, `runtime.delivery` records
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
Restarting a provider process does not transfer its transient input mode or hot
terminal ring; durable ledger, Mail cursor, reply-obligation, and
runtime-delivery facts remain the recovery authority.

Starting, restarting, creating a new session, or restoring after daemon startup
does not count as Actor work and never submits a synthetic model turn. CCCC
creates or reconnects the managed control session and opens its native terminal
while the model remains idle. The first successfully accepted real CCCC delivery
also carries the pending startup instructions; a rejected delivery leaves them
pending. Human terminal input is likewise real work, but lifecycle operations
alone are not.

### Managed Codex/Claude/Grok/OpenCode/Kilo sessions

For every Codex Actor, CCCC creates one
daemon-owned app-server thread and opens Codex's writable remote TUI against
that exact thread. The configured executable must implement the Codex
app-server command; unsupported subcommands, wrappers, or prompt tails fail
explicitly instead of selecting another transport. The
app-server and TUI receive the same executable, model, Codex Profile, supported
`-c` overrides, YOLO policy, and private environment; only the host receives
the actor-scoped CCCC MCP and listener arguments. Provider events are the
working-state and completion authority, while Actor deliveries go immediately
through the native TUI so Codex applies its own queue/steer policy. Stop/start
resumes the same validated thread, while `actor new-session` deliberately
creates a new one. Voice Analyst uses this same Codex host/remote-TUI substrate,
with a global user MCP identity and its own warm lifecycle instead of Actor
identity and Group lifecycle.

A new Codex thread is made durable through native metadata operations before
the TUI attaches; an ID or planned rollout path alone is not a resumable history.
Starting or restarting an idle Actor does not submit a bootstrap prompt or run
the model. An empty thread can be stopped and resumed without first sending a
message.

Direct Claude Code Actors require Claude Code 2.1.259 or newer. Before first use,
accept Claude's bypass-mode disclaimer interactively with
`claude --dangerously-skip-permissions` under the same Claude configuration.
If it has not been accepted, the managed launch returns Claude's actionable
error before opening an Actor terminal; CCCC does not write workspace trust or
disclaimer acceptance into the user's global configuration.

CCCC owns one
Agent View background session, launches `claude attach` against that exact
session, and derives turn ownership, tool results, completion, cancellation,
and provider errors from Claude's append-only transcript. Actor deliveries go
through the attached native terminal. Runtime Profile settings and private environment are merged
into a stable, owner-scoped, permission-protected CCCC settings file because
Agent View does not retain arbitrary launch environment in its job record and
keeps that settings path in the session's respawn metadata. An ordinary stop
retains the file so start can either re-adopt the matching live idle job or cold
resume the same provider session from a validated version-2 receipt. The
complete effective launch identity is
fenced because an exact cold resume cannot reapply changed model, settings, or
provider environment. CCCC-owned background, attach, session, MCP, autonomy,
and resume flags cannot be supplied by the user. Wrappers, renamed binaries,
prompt tails, print mode, and user-owned session topology fail explicitly;
there is no Hook, PTY-paste, or `claude -p` fallback.

A Claude session stopped before its first input can also resume the same ID
without a transcript. CCCC requires positive empty-job evidence; a zero output-token
counter is still empty. Existing input, output, nonzero usage, or a published
transcript keeps the strict history checks. Do not delete `.claude` or `.codex`
to troubleshoot a managed-session startup failure.

If a saved transcript path no longer exists after a worktree move, initial
recovery searches the configured Claude project store for the same session ID.
Only a unique, validated regular file is accepted. The recovered file is pinned
for the running session; later relocation, replacement, or ambiguous candidates
fail explicitly rather than replaying or switching history.

Direct Grok Actors use the same managed-session contract through Grok's native
topology: CCCC owns one private leader, connects an ACP observer, and attaches
the native writable Grok TUI to the exact same provider session. Structured ACP
events own progress, completion, cancellation, and working state; terminal text is never scraped as
protocol. CCCC injects the actor-scoped MCP server into the session rather than
changing Grok's global MCP registry. Stop/start validates and loads the version-2
managed receipt, while `actor new-session` deliberately replaces it. Grok
subcommands, wrappers, prompt tails, and user-owned leader/session flags fail
explicitly; there is no raw-PTY fallback beside the managed path.

Direct OpenCode Actors use one
`opencode acp` process as both the structured controller endpoint and an
authenticated loopback backend. CCCC injects the
actor-scoped MCP server when it creates or loads the ACP session and attaches
OpenCode's native writable TUI to that exact backend and session. CCCC observes
authenticated `session.status` events for lifecycle state, while new Actor
messages are handed to the native TUI without waiting for an active turn to settle.
Losing that non-replayable lifecycle stream invalidates the session rather than
guessing that it is idle. Stop/start validates and loads a version-2 OpenCode
receipt; `actor new-session` deliberately replaces it. CCCC owns ACP/server,
session, attach, cwd, MCP, and permission arguments. It accepts documented
model, agent, pure-mode, and logging options, but subcommands, wrappers, prompt
tails, and user-owned topology/session flags fail explicitly with no raw-PTY
fallback. OpenCode does not emit the accepted user prompt through ACP, so CCCC
correlates protocol-originated requests on OpenCode's authenticated backend
event stream before acknowledging admission. Pre-admission ACP updates remain
bounded and are released only after that match. For supported OpenCode releases,
the prompt response is the exact completion fence for those protocol requests;
Grok retains its explicit bounded post-response normalization. OpenCode keeps a
TUI model change local until that TUI submits its next message. CCCC observes the
submitted message's provider, model, and variant and mirrors them into the same
ACP session for later managed requests. Add `--model provider/model` to the
Runtime command when the model must be selected at launch instead.

The Rust daemon also owns the lifetime of every process-backed actor. On Windows, the daemon host and each terminal actor use non-breakaway Job Objects with `KILL_ON_JOB_CLOSE`; Codex and actor-launched MCP descendants inherit containment when they are created, so an abrupt daemon or combined Web-process exit cannot leave them orphaned. On POSIX, each terminal actor is a separate session and normal stop/reap terminates its entire process group. Process cleanup never removes `group.yaml`, `ledger.jsonl`, or retained terminal history.

Kilo shares the OpenCode ACP/HTTP adapter for both Actors and Voice Analyst.
CCCC owns a private `kilo acp` backend and attaches `kilo attach` to that exact
session; it does not reuse an unrelated global Kilo daemon. Kilo receives
session-scoped MCP injection, and no setup or bootstrap prompt is sent merely
because it starts. Its settings use `KILO_*` rather than `OPENCODE_*`, including
`KILO_DB` for the durable session store. The same model-selection rule applies:
submit one message after changing the native TUI model, or set
`--model provider/model` in the launch command. Existing terminal-only receipts
are not adopted; subsequent managed stop/start preserves the new session.

Codex, Claude, Grok, OpenCode, and Kilo always pair their native terminal with the
managed background session described above. DeepSeek uses ACP NDJSON through
CCCC's fixed composition and has no terminal surface. Provider health determines
the Actor's `running` value, and stopping the Actor or Group closes the owned
provider session. Internal `headless.*` event names remain a wire-format detail;
they are not a selectable Actor mode.

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
managed-headless logs receive a one-time streaming dedupe-index migration
when DeepSeek first writes to them, without loading the full log into memory.

For daemon-managed Codex protocol turns, a provider status of `failed`, `error`,
or `cancelled`, or an explicit provider error, is persisted as
`headless.turn.failed`; only a successful terminal notification is persisted as
`headless.turn.completed`. Acceptance has already advanced the actor's read
cursor, so a provider failure is not silently retried, but it does release the
session lane for later queued turns.

Daemon-managed Codex runs with non-interactive approval policy. If app-server
nevertheless sends a provider-initiated approval, user-input, elicitation, or
tool request, CCCC returns an explicit JSON-RPC unsupported-method error instead
of hanging the turn or approving it implicitly. Interactive approval or input
remains available in the Actor's native terminal.

Daemon-managed Codex Actors persist the app-server thread in
the runtime-session state. An ordinary actor stop/start resumes that exact thread
after validating the runtime, workspace, command, model, and saved-state
status. If the provider rejects the resume, CCCC records the failure and starts
a fresh thread. `actor_new_session` deliberately clears the saved thread first,
and `CCCC_RUNTIME_RESUME=0` disables this reuse globally.

Daemon-managed Claude Actors persist the Agent View session id
in the shared runtime-session state. An ordinary stop/start validates the
runtime, workspace, command, complete effective configuration identity, and
saved-state status. It re-adopts one matching live idle job or uses Claude's
exact cold-resume form; a copied, busy, ambiguous, or mismatched job fails
closed instead of being guessed. `actor_new_session` clears the receipt, and
`CCCC_RUNTIME_RESUME=0` disables reuse. A legacy Hook or print-mode receipt is
never resumed.

`web_model` keeps the pull-consumer contract: an external executor calls
`cccc_runtime_wait_next_turn` and `cccc_runtime_complete_turn`. It does not claim
to have a local provider process or native terminal.

For a running Antigravity actor, `actor_new_session` submits the runtime's
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

Each Actor can have its own Runtime, command override, and private environment. CCCC derives the interaction surface from the selected Runtime. Runtime state stays in `CCCC_HOME`, not in your repository.

Native terminal output always uses bounded memory and can optionally persist a bounded per-Actor transcript. See [Terminal history](terminal-history.md) for opt-in persistence, retention, cursor, restart, and security behavior.

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

Before the Rust daemon creates a Runtime session, it establishes the Runtime's CCCC MCP path. Codex, Claude Code, Grok, OpenCode, and Kilo receive an Actor-scoped server inside their managed session; none of their global MCP registries is changed. Other automatically configured runtimes are checked against the active public CCCC executable: missing entries are installed, safely replaceable stale user/global entries are replaced, and the result is verified before the Actor process starts. A failed check, repair, or verification prevents launch, including daemon restart recovery. A stale entry from a more specific project or non-user scope fails with an actionable error instead of being silently overwritten. Prompt-assisted runtimes (`cursor` and `antigravity`) retain their startup setup contract, while indirect custom provider commands remain responsible for their own MCP configuration. `cccc setup` for Claude, Grok, OpenCode, and Kilo therefore reports session ownership instead of mutating provider-global configuration.

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
