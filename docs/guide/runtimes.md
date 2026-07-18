# Supported Runtimes

CCCC can run multiple agent runtimes in the same working group. Each actor chooses one runtime, while the daemon keeps messaging, delivery tracking, tasks, context, and Web/IM control in one shared CCCC group.

Use `cccc runtime list --all` to see the full supported list on your machine, and `cccc doctor` to check which CLI runtimes are installed.

## First-Class Runtimes

| Runtime | Runtime id | Entrypoint / surface | MCP setup |
|---------|------------|----------------------|-----------|
| Claude Code | `claude` | `claude` | Auto |
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

Claude Code and Codex CLI support both PTY and headless operation. Most other CLI runtimes use PTY. ChatGPT Web Model is fixed to browser delivery plus a remote MCP connector.

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

The Web UI also exposes runtime detection and actor configuration from the add/edit actor dialogs.
