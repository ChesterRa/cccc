<div align="center">

<img src="screenshots/logo.png" width="160" />

# CCCC

### Local-first Multi-agent Collaboration Kernel

**A lightweight multi-agent framework with infrastructure-grade reliability.**

Chat-native, prompt-driven, and bi-directional by design.

Run multiple coding agents as a **durable, coordinated system** — not a pile of disconnected terminal sessions.

Three commands to go. Zero infrastructure, production-grade power.

[![PyPI](https://img.shields.io/pypi/v/cccc-pair?label=PyPI&color=blue)](https://pypi.org/project/cccc-pair/)
[![Python](https://img.shields.io/pypi/pyversions/cccc-pair)](https://pypi.org/project/cccc-pair/)
[![License](https://img.shields.io/badge/license-Apache--2.0-green)](LICENSE)
[![Docs](https://img.shields.io/badge/docs-online-blue)](https://chesterra.github.io/cccc/)

**English** | [中文](README.zh-CN.md) | [日本語](README.ja.md)

</div>

---

## Build With the Official SDK

For app integration, bots, IDE extensions, and background services, use the official SDK repo:

- [cccc-sdk](https://github.com/ChesterRa/cccc-sdk)
- Python package: `cccc-sdk` (import as `cccc_sdk`)
- TypeScript package: `cccc-sdk`

SDK clients connect to the same CCCC daemon and share the same `CCCC_HOME` runtime state.

## Why v0.4.0 Is a Generational Upgrade

- **Chat-native orchestration**: assign work in Web chat as naturally as talking to teammates, with full delivery/read/ack/reply visibility.
- **Workflow-by-design**: configure multi-agent behavior with guidance prompts and automation rules, instead of brittle ad-hoc scripts.
- **Bi-directional control**: CCCC orchestrates agents, while agents can also schedule and customize CCCC workflows through MCP tools.
- **Beyond the browser**: the same operating model extends to Telegram/Slack/Discord/Feishu/DingTalk via IM bridges.

## The Problem

Using multiple coding agents today usually means:

- **Lost context** — coordination lives in terminal scrollback and disappears on restart
- **No delivery guarantees** — did the agent actually *read* your message?
- **Fragmented ops** — start/stop/recover/escalate across separate tools
- **No remote access** — checking on a long-running group from your phone is not an option

These aren't minor inconveniences. They're the reason most multi-agent setups stay fragile demos instead of reliable workflows.

## What CCCC Does

CCCC is a single `pip install` with zero external dependencies — no database, no message broker, no Docker required. Yet it delivers the operational reliability you'd expect from a production messaging system:

| Capability | How |
|---|---|
| **Single source of truth** | Append-only ledger (`ledger.jsonl`) records every message and event — replayable, auditable, never lost |
| **Reliable messaging** | Read cursors, attention ACK, reply-required obligations — you know exactly who read what |
| **Unified control plane** | Web UI, CLI, MCP tools, and IM bridges all talk to one daemon — no state fragmentation |
| **Multi-runtime orchestration** | Claude Code, Codex CLI, Gemini CLI, Copilot, and 8 more runtimes in one group |
| **Role-based coordination** | Foreman + peer model with permission boundaries and recipient routing (`@all`, `@peers`, `@foreman`) |
| **Remote operations** | Bridge to Telegram, Slack, Discord, Feishu, or DingTalk — manage groups from your phone |


## How CCCC looks

<div align="center">

<video src="https://github.com/user-attachments/assets/460b6719-428b-4c1c-8879-0ebf8b8cee4f" controls="controls" muted="muted" autoplay="autoplay" loop="loop" style="max-width: 100%;">
</video>

</div>

## Quick Start

### Install

```bash
# Stable channel (PyPI)
pip install -U cccc-pair

# RC channel (TestPyPI)
pip install -U --pre \
  --index-url https://test.pypi.org/simple/ \
  --extra-index-url https://pypi.org/simple/ \
  cccc-pair
```

> **Requirements**: Python 3.9+, macOS / Linux / Windows

### Launch

```bash
cccc
```

Open **http://127.0.0.1:8848** — the Web UI is ready.

### Create a multi-agent group

```bash
cd /path/to/your/repo
cccc attach .                              # bind this directory as a scope
cccc setup --runtime claude                # configure MCP for your runtime
cccc actor add foreman --runtime claude    # first actor becomes foreman
cccc actor add reviewer --runtime codex    # add a peer
cccc group start                           # start all actors
cccc send "Split the task and begin." --to @all
```

You now have two agents collaborating in a persistent group with full message history, delivery tracking, and a web dashboard.

## Programmatic Access (SDK)

Use the official SDK when you need to integrate CCCC into external applications or services:

```bash
pip install -U cccc-sdk
npm install cccc-sdk
```

The SDK does not include a daemon. It connects to a running `cccc` core instance.

## Architecture

```mermaid
graph TB
    subgraph Agents["Agent Runtimes"]
        direction LR
        A1["Claude Code"]
        A2["Codex CLI"]
        A3["Gemini CLI"]
        A4["+ 9 more"]
    end

    subgraph Daemon["CCCC Daemon · single writer"]
        direction LR
        Ledger[("Ledger<br/>append-only JSONL")]
        ActorMgr["Actor<br/>Manager"]
        Auto["Automation<br/>Rules · Nudge · Cron"]
        Ledger ~~~ ActorMgr ~~~ Auto
    end

    subgraph Ports["Control Plane"]
        direction LR
        Web["Web UI<br/>:8848"]
        CLI["CLI"]
        MCP["MCP<br/>(stdio)"]
    end

    subgraph IM["IM Bridges"]
        direction LR
        TG["Telegram"]
        SL["Slack"]
        DC["Discord"]
        FS["Feishu"]
        DT["DingTalk"]
    end

    Agents <-->|MCP tools| Daemon
    Daemon <--> Ports
    Web <--> IM

```

**Key design decisions:**

- **Daemon is the single writer** — all state changes go through one process, eliminating race conditions
- **Ledger is append-only** — events are never mutated, making history reliable and debuggable
- **Ports are thin** — Web, CLI, MCP, and IM bridges are stateless frontends; the daemon owns all truth
- **Runtime home is `CCCC_HOME`** (default `~/.cccc/`) — runtime state stays out of your repo

## Supported Runtimes

CCCC orchestrates agents across 12 runtimes. Each actor in a group can use a different runtime.

| Runtime | Auto MCP Setup | Command |
|---------|:--------------:|---------|
| Claude Code | ✅ | `claude` |
| Codex CLI | ✅ | `codex` |
| Gemini CLI | ✅ | `gemini` |
| Droid | ✅ | `droid` |
| Amp | ✅ | `amp` |
| Auggie | ✅ | `auggie` |
| Neovate | ✅ | `neovate` |
| Copilot | — | `copilot` |
| Cursor | — | `cursor-agent` |
| Kilo Code | — | `kilocode` |
| OpenCode | — | `opencode` |
| Custom | — | Any command |

```bash
cccc setup --runtime claude    # auto-configures MCP for this runtime
cccc runtime list --all        # show all available runtimes
cccc doctor                    # verify environment and runtime availability
```

## Messaging & Coordination

CCCC implements IM-grade messaging semantics, not just "paste text into a terminal":

- **Recipient routing** — `@all`, `@peers`, `@foreman`, or specific actor IDs
- **Read cursors** — each agent explicitly marks messages as read via MCP
- **Reply & quote** — structured `reply_to` with quoted context
- **Attention ACK** — priority messages require explicit acknowledgment
- **Reply-required obligations** — tracked until the recipient responds
- **Auto-wake** — disabled agents are automatically started when they receive a message

Messages are delivered to PTY actors via terminal injection and to headless actors via system notifications. The daemon tracks delivery state for every message.

## Automation & Policies

A built-in rules engine handles operational concerns so you don't have to babysit:

| Policy | What it does |
|--------|-------------|
| **Nudge** | Reminds agents about unread messages after a configurable timeout |
| **Reply-required follow-up** | Escalates when required replies are overdue |
| **Actor idle detection** | Notifies foreman when an agent goes silent |
| **Keepalive** | Periodic check-in reminders for the foreman |
| **Silence detection** | Alerts when an entire group goes quiet |

Beyond built-in policies, you can create custom automation rules:

- **Interval triggers** — "every N minutes, send a standup reminder"
- **Cron schedules** — "every weekday at 9am, post a status check"
- **One-time triggers** — "at 5pm today, pause the group"
- **Operational actions** — set group state or control actor lifecycles (admin-only, one-time only)

## Web UI

The built-in Web UI at `http://127.0.0.1:8848` provides:

- **Chat view** with `@mention` autocomplete and reply threading
- **Per-actor embedded terminals** (xterm.js) — see exactly what each agent is doing
- **Group & actor management** — create, configure, start, stop, restart
- **Automation rule editor** — configure triggers, schedules, and actions visually
- **Context panel** — shared vision, sketch, milestones, and tasks
- **IM bridge configuration** — connect to Telegram/Slack/Discord/Feishu/DingTalk
- **Settings** — messaging policies, delivery tuning, terminal transcript controls
- **Light / Dark / System themes**

| Chat | Terminal |
|:----:|:-------:|
| ![Chat](screenshots/chat.png) | ![Terminal](screenshots/terminal.png) |

### Remote access

For accessing the Web UI from outside localhost:

- **Cloudflare Tunnel** (recommended) — `cloudflared tunnel --url http://127.0.0.1:8848`
- **Tailscale** — bind to your tailnet IP: `CCCC_WEB_HOST=$TAILSCALE_IP cccc`
- Always set `CCCC_WEB_TOKEN` for any non-local access

## IM Bridges

Bridge your working group to your team's IM platform:

```bash
cccc im set telegram --token-env TELEGRAM_BOT_TOKEN
cccc im start
```

| Platform | Status |
|----------|--------|
| Telegram | ✅ Supported |
| Slack | ✅ Supported |
| Discord | ✅ Supported |
| Feishu / Lark | ✅ Supported |
| DingTalk | ✅ Supported |

From any supported platform, use `/send @all <message>` to talk to your agents, `/status` to check group health, and `/pause` / `/resume` to control operations — all from your phone.

## CLI Reference

```bash
# Lifecycle
cccc                           # start daemon + web UI
cccc daemon start|status|stop  # daemon management

# Groups
cccc attach .                  # bind current directory
cccc groups                    # list all groups
cccc use <group_id>            # switch active group
cccc group start|stop          # start/stop all actors

# Actors
cccc actor add <id> --runtime <runtime>
cccc actor start|stop|restart <id>

# Messaging
cccc send "message" --to @all
cccc reply <event_id> "response"
cccc tail -n 50 -f             # follow the ledger

# Inbox
cccc inbox                     # show unread messages
cccc inbox --mark-read         # mark all as read

# Operations
cccc doctor                    # environment check
cccc setup --runtime <name>    # configure MCP
cccc runtime list --all        # available runtimes

# IM
cccc im set <platform> --token-env <ENV_VAR>
cccc im start|stop|status
```

## MCP Tools

Agents interact with CCCC through **49 MCP tools** across 7 namespaces:

| Namespace | Tools | Examples |
|-----------|:-----:|---------|
| **Session** | 2 | `cccc_bootstrap` (one-call init), `cccc_help` (operational playbook) |
| **Messaging** | 7 | `cccc_message_send`, `cccc_message_reply`, `cccc_file_send`, `cccc_inbox_list`, `cccc_inbox_mark_read` ... |
| **Group & Actor** | 10 | `cccc_group_info`, `cccc_group_list`, `cccc_actor_add/remove/start/stop/restart`, `cccc_runtime_list`, `cccc_group_set_state` |
| **Automation** | 2 | `cccc_automation_state`, `cccc_automation_manage` (create/update/enable/disable/delete rules) |
| **Context** | 19 | `cccc_context_get/sync`, `cccc_vision_update`, `cccc_sketch_update`, `cccc_milestone_*`, `cccc_task_*`, `cccc_note_*`, `cccc_reference_*`, `cccc_presence_*` |
| **Headless** | 3 | `cccc_headless_status`, `cccc_headless_set_status`, `cccc_headless_ack_message` |
| **System** | 6 | `cccc_notify_send/ack`, `cccc_terminal_tail`, `cccc_project_info`, `cccc_debug_snapshot`, `cccc_debug_tail_logs` |

Agents with MCP access can self-organize: read their inbox, reply, manage tasks and milestones, set automation rules, and coordinate with peers — all within permission boundaries.

## Where CCCC Fits

| Scenario | Fit |
|----------|-----|
| Multiple coding agents collaborating on one codebase | ✅ Core use case |
| Human + agent coordination with full audit trail | ✅ Core use case |
| Long-running groups managed remotely via phone/IM | ✅ Strong fit |
| Multi-runtime teams (e.g., Claude + Codex + Gemini) | ✅ Strong fit |
| Single-agent local coding helper | ⚠️ Works, but CCCC's value shines with multiple participants |
| Pure DAG workflow orchestration | ❌ Use a dedicated orchestrator; CCCC can complement it |

CCCC is a **collaboration kernel** — it owns the coordination layer and stays composable with external CI/CD, orchestrators, and deployment tools.

## Security

- **Web UI is high-privilege.** Always set `CCCC_WEB_TOKEN` for non-local access.
- **Daemon IPC has no authentication.** It binds to localhost by default.
- **IM bot tokens** are read from environment variables, never stored in config files.
- **Runtime state** lives in `CCCC_HOME` (`~/.cccc/`), not in your repository.

For detailed security guidance, see [SECURITY.md](SECURITY.md).

## Documentation

📚 **[Full documentation](https://chesterra.github.io/cccc/)**

| Section | Description |
|---------|-------------|
| [Getting Started](https://chesterra.github.io/cccc/guide/getting-started/) | Install, launch, create your first group |
| [Use Cases](https://chesterra.github.io/cccc/guide/use-cases) | Practical multi-agent scenarios |
| [Web UI Guide](https://chesterra.github.io/cccc/guide/web-ui) | Navigating the dashboard |
| [IM Bridge Setup](https://chesterra.github.io/cccc/guide/im-bridge/) | Connect Telegram, Slack, Discord, Feishu, DingTalk |
| [Operations Runbook](https://chesterra.github.io/cccc/guide/operations) | Recovery, troubleshooting, maintenance |
| [CLI Reference](https://chesterra.github.io/cccc/reference/cli) | Complete command reference |
| [SDK (Python/TypeScript)](https://github.com/ChesterRa/cccc-sdk) | Integrate apps/services with official daemon clients |
| [Architecture](https://chesterra.github.io/cccc/reference/architecture) | Design decisions and system model |
| [Features Deep Dive](https://chesterra.github.io/cccc/reference/features) | Messaging, automation, runtimes in detail |
| [CCCS Standard](docs/standards/CCCS_V1.md) | Collaboration protocol specification |
| [Daemon IPC Standard](docs/standards/CCCC_DAEMON_IPC_V1.md) | IPC protocol specification |

## Installation Options

### pip (stable, recommended)

```bash
pip install -U cccc-pair
```

### pip (RC from TestPyPI)

```bash
pip install -U --pre \
  --index-url https://test.pypi.org/simple/ \
  --extra-index-url https://pypi.org/simple/ \
  cccc-pair
```

### From source

```bash
git clone https://github.com/ChesterRa/cccc
cd cccc
pip install -e .
```

### uv (fast, recommended on Windows)

```bash
uv venv -p 3.11 .venv
uv pip install -e .
uv run cccc --help
```

### Docker

```bash
cd docker
CCCC_WEB_TOKEN=your-secret docker compose up -d
```

The Docker image bundles Claude Code, Codex CLI, Gemini CLI, and Factory CLI. See [`docker/`](docker/) for full configuration.

### Upgrading from 0.3.x

The 0.4.x line is a ground-up rewrite. Clean uninstall first:

```bash
pipx uninstall cccc-pair || true
pip uninstall cccc-pair || true
rm -f ~/.local/bin/cccc ~/.local/bin/ccccd
```

Then install fresh and run `cccc doctor` to verify your environment.

> The tmux-first 0.3.x line is archived at [cccc-tmux](https://github.com/ChesterRa/cccc-tmux).

## Community

📱 Join our Telegram group: [t.me/ccccpair](https://t.me/ccccpair)

Share workflows, troubleshoot issues, and connect with other CCCC users.

## Contributing

Contributions are welcome. Please:

1. Check existing [Issues](https://github.com/ChesterRa/cccc/issues) before opening a new one
2. For bugs: include `cccc version`, OS, exact commands, and reproduction steps
3. For features: describe the problem, proposed behavior, and operational impact
4. Keep runtime state in `CCCC_HOME` — never commit it to the repo

## License

[Apache-2.0](LICENSE)
