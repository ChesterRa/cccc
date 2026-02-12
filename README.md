# CCCC — Local-First Multi-Agent Collaboration Kernel

**English** | [中文](README.zh-CN.md) | [日本語](README.ja.md)

[![Documentation](https://img.shields.io/badge/docs-online-blue)](https://dweb-channel.github.io/cccc/)
[![License](https://img.shields.io/badge/license-Apache--2.0-green)](LICENSE)

CCCC helps you run real multi-agent collaboration as a durable system, not a collection of fragile terminal sessions.

You get:
- A single source of truth (`ledger.jsonl`) for all collaboration events
- One control plane across Web UI, CLI, MCP, and IM bridges
- Reliable message delivery semantics (read/ack/reply-required)
- Multi-runtime orchestration (Claude, Codex, Gemini, Copilot, and others)

![CCCC Chat UI](screenshots/chat.png)

## Why CCCC

Most teams hit the same bottlenecks when using multiple coding agents:
- Coordination is ad-hoc and disappears in terminal scrollback
- Message delivery is ambiguous across tools and sessions
- Operational control (start/stop/recover/escalate) is fragmented
- Remote operation from mobile or IM channels is brittle

CCCC addresses these with a daemon-centered architecture:
- **Append-only ledger** as the collaboration record
- **Explicit recipient routing** and obligation signals
- **Unified operations surface** via Web/CLI/MCP/IM
- **Local-first runtime home** (`CCCC_HOME`, default `~/.cccc`)

## 10-Minute Quick Start

### 1) Install

```bash
python -m pip install -U cccc-pair
```

For explicit RC verification:

```bash
python -m pip install --index-url https://pypi.org/simple \
  --extra-index-url https://test.pypi.org/simple \
  cccc-pair==0.4.0rc19
```

### 2) Start CCCC

```bash
cccc
```

Open `http://127.0.0.1:8848/`.

### 3) Create your first multi-agent group

```bash
cd /path/to/repo
cccc attach .
cccc setup --runtime claude
cccc actor add foreman --runtime claude
cccc actor add reviewer --runtime codex
cccc group start
cccc send "Please split the current task and start implementation." --to @all
```

## Product Capabilities

- **Multi-Agent Runtime Orchestration**
  - Add/start/stop/restart actors per group
  - Foreman + peers role model with permission boundaries
- **Durable Collaboration Ledger**
  - Every message/event is append-only
  - Replayable history for debugging and operations
- **IM-Grade Messaging Semantics**
  - `@all` / `@peers` / `@foreman` / actor-level routing
  - Structured reply, read cursors, attention/ack, reply-required obligations
- **Automation and System Policies**
  - Interval, recurring schedule, one-time triggers
  - Reminder actions plus controlled operational actions
- **Multi-Channel Operations**
  - Web UI control plane
  - CLI for scripted workflows
  - MCP for agent-side control
  - IM bridges (Telegram/Slack/Discord/Feishu/DingTalk)

## Where CCCC Fits

| If you need... | CCCC fit |
|---|---|
| A persistent collaboration substrate for multiple coding agents | Excellent fit |
| Human + agent coordination with durable audit history | Excellent fit |
| Mobile/IM-assisted operations for long-running groups | Strong fit |
| Deterministic workflow DAG orchestration with rich task scheduling UI | Use CCCC + external orchestrator |

CCCC is intentionally a **collaboration kernel**, not an all-in-one workflow studio.

## Architecture at a Glance

- **Core unit**: Working Group
- **Source of truth**: append-only group ledger
- **Single writer**: daemon
- **Ports are thin**: Web/CLI/MCP/IM call daemon IPC
- **Runtime home**: `CCCC_HOME` (default `~/.cccc`)

See:
- `docs/reference/architecture.md`
- `docs/standards/CCCS_V1.md`
- `docs/standards/CCCC_DAEMON_IPC_V1.md`

## Documentation Map

- Start here: `docs/guide/getting-started/index.md`
- Practical scenarios: `docs/guide/use-cases.md`
- Operations runbook: `docs/guide/operations.md`
- Product positioning: `docs/reference/positioning.md`
- CLI reference: `docs/reference/cli.md`
- Features deep dive: `docs/reference/features.md`

Online docs: https://dweb-channel.github.io/cccc/

## Security and Operations Notes

- Web UI is high privilege; set `CCCC_WEB_TOKEN` for any remote access.
- Prefer Cloudflare Access or Tailscale over direct public exposure.
- Keep runtime state in `CCCC_HOME`, not inside repo working trees.
- For recovery workflows and runbook commands, see `docs/guide/operations.md`.

## Upgrading from 0.3.x

`0.4.x` is a new architecture line. Legacy commands and behavior changed.

Before upgrading:

```bash
pipx uninstall cccc-pair || true
pip uninstall cccc-pair || true
rm -f ~/.local/bin/cccc ~/.local/bin/ccccd
```

Then install fresh and run `cccc doctor`.

## Legacy Line

The old tmux-first implementation remains at:
- https://github.com/ChesterRa/cccc-tmux

## License

Apache-2.0
