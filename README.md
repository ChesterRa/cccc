# CCCC Pair — Modern Multi-Agent Orchestrator

**English** | [中文](README.zh-CN.md) | [日本語](README.ja.md)

Two always-on AI peers co-drive your repository as equals. They plan, build, critique, and converge through evidence — not just talk. You stay in control via an interactive TUI or your team chat.

**🎯 Production-grade orchestrator** • **🖥️ Zero-config TUI** • **📊 Real-time monitoring** • **🧪 Evidence-driven workflow**

[![License](https://img.shields.io/badge/License-Apache_2.0-blue.svg)](LICENSE)
[![PyPI](https://img.shields.io/pypi/v/cccc-pair)](https://pypi.org/project/cccc-pair/)
[![Python](https://img.shields.io/pypi/pyversions/cccc-pair)](https://pypi.org/project/cccc-pair/)
[![Telegram Community](https://img.shields.io/badge/Telegram-Join_Community-2CA5E0?style=flat&logo=telegram&logoColor=white)](https://t.me/ccccpair)

---

## 🖼️ CCCC at a Glance

![CCCC TUI Screenshot](./screenshots/tui-main.png)

> **Modern terminal interface** with interactive setup wizard, real-time timeline, command completion, and status monitoring — all in one clean layout.

### Runtime in Action

![CCCC Runtime Screenshot](./screenshots/tui-runtime.png)

> **Four-pane layout**: Top-left TUI console (Timeline + status bar), top-right PeerA terminal, bottom-right PeerB terminal. Screenshot shows Foreman conducting strategic analysis while both Peers (using opencode) process tasks and coordinate autonomously.

---

## ✨ What Makes CCCC Different

<table>
<tr>
<td width="50%">

**🤝 Autonomous Dual-Agent Collaboration**
Two equal peers collaborate and **drive tasks forward automatically** — no constant user intervention needed. They challenge each other, surface better options, and catch errors faster.

**🖥️ Interactive TUI with Zero Config**
Point-and-click setup wizard (↑↓ + Enter). No YAML editing. No memorizing commands. Tab completion for everything.

**📊 Real-Time Observability**
Live Timeline shows peer messages. Status panel tracks handoffs, self-checks, and Foreman runs.

</td>
<td width="50%">

**🧪 Evidence-First Workflow**
Only tested patches, stable logs, and commits count as "done". Chat alone never changes state.

**🔗 Multi-Platform Bridges**
Optional Telegram/Slack/Discord integration. Bring the work to where your team already is.

**📋 Repo-Native Anchors**
Strategic board (POR.md) and per-task sheets (SUBPOR.md) live in your repo. Everyone sees the same truth.

</td>
</tr>
</table>

---

## Why CCCC? (The Pain → Payoff)

### Single-Agent Pain Points (You May Recognize These)

- 🛑 **Constant Babysitting** — Single agent stalls without your input; you must keep prompting to make progress
- ⏳ **Stalls & Restarts** — Context evaporates between runs; work drifts and repeats
- 💬 **Low-Signal Threads** — Long monologues with little verification, no audit trail
- 🚩 **Vanishing Decisions** — Hard to see what changed, why, and who approved

### CCCC Payoff with Dual Peers & Modern Tooling

- 🚀 **Autonomous Progress** — Peers communicate and drive tasks forward on their own (10-15 min per cycle); add Foreman for near-continuous operation
- 🤝 **Multi-Peer Synergy** — One builds, the other challenges; better options emerge; errors die faster
- ✅ **Evidence-First Loop** — Only tested/logged/committed results count as progress
- 🖥️ **Interactive TUI** — Zero-config setup, real-time monitoring, command completion built-in
- 📋 **POR/SUBPOR Anchors** — One strategic board (POR) + per-task sheets (SUBPOR) keep everyone aligned without ceremony
- 🔔 **Low-Noise Cadence** — Built-in nudge/self-check trims chatter; panel shows what matters
- 🔍 **Auditable Decisions** — Recent choices & pivots captured; review and roll forward confidently

---

## When to Use CCCC

- You want **autonomous progress you can trust**, with small, reversible steps
- You need **collaboration you can observe** in TUI/IM, not a black box
- Your project benefits from a **living strategic board** and lightweight task sheets in the repo
- You care about **repeatability**: tests, stable logs, and commits as the final word

---

## TUI Highlights

CCCC features a modern, keyboard-driven TUI with zero-config setup:

- **Setup Panel** — Interactive wizard (↑↓ + Enter), no YAML editing needed
- **Runtime Panel** — Real-time Timeline + Status, see all peer messages at a glance
- **Tab Completion** — Type `/` and press Tab to explore all commands
- **Command History** — Up/Down arrows + Ctrl+R reverse search
- **Rich Shortcuts** — Standard editing keys (Ctrl+A/E/W/U/K) work as expected

> See the screenshots above for the actual interface.

---

## Requirements

CCCC uses tmux to manage a multi-pane terminal layout. Ensure the following dependencies are installed:

| Dependency | Description | Installation |
|------------|-------------|--------------|
| **Python** | ≥ 3.9 | Pre-installed on most systems |
| **tmux** | Terminal multiplexer for multi-pane layout | macOS: `brew install tmux`<br>Ubuntu/Debian: `sudo apt install tmux`<br>Windows: Requires WSL |
| **git** | Version control | Pre-installed on most systems |
| **Agent CLI** | At least one required | See below |

### Supported CLI Actors

CCCC is **vendor-agnostic**. Any role (PeerA, PeerB, Aux, Foreman) can use any supported CLI:

| CLI | Official Docs |
|-----|---------------|
| **Claude Code** | [docs.anthropic.com/claude-code](https://docs.anthropic.com/en/docs/claude-code) |
| **Codex CLI** | [github.com/openai/codex](https://github.com/openai/codex) |
| **Gemini CLI** | [github.com/google-gemini/gemini-cli](https://github.com/google-gemini/gemini-cli) |
| **Factory Droid** | [factory.ai](https://factory.ai/) |
| **OpenCode** | [opencode.ai/docs](https://opencode.ai/docs/) |
| **Kilocode** | [kilo.ai/docs/cli](https://kilo.ai/docs/cli) |
| **GitHub Copilot** | [github.com/features/copilot/cli](https://github.com/features/copilot/cli) |
| **Augment Code** | [docs.augmentcode.com/cli](https://docs.augmentcode.com/cli/overview) |
| **Cursor** | [cursor.com/cli](https://cursor.com/en-US/cli) |

> **Mix and match freely** — choose the best CLI for each role based on your needs. See each CLI's official docs for installation instructions.

> **Windows Users**: CCCC requires WSL (Windows Subsystem for Linux). [Install WSL](https://docs.microsoft.com/en-us/windows/wsl/install) first, then proceed in the WSL terminal.

---

## Key Configuration Files: PROJECT.md & FOREMAN_TASK.md

These two files are your primary interface for communicating tasks to the AI. **Write them carefully.**

### PROJECT.md (Project Description)

Located at repo root. **Automatically injected into PeerA and PeerB's system prompts.**

**Should include:**
- Project background and goals
- Tech stack and architecture overview
- Coding conventions and standards
- Current phase priorities
- Any context peers need to know

```markdown
# Project Overview
This is a xxx system using Python + FastAPI + PostgreSQL...

# Current Priorities
1. Complete user authentication module
2. Optimize database query performance

# Coding Standards
- Use type hints
- Every function needs a docstring
- Test coverage > 80%
```

### FOREMAN_TASK.md (Supervisor Tasks)

Located at repo root. **Automatically injected into Foreman.** Foreman runs every 15 minutes and reads this file to decide what to do.

**Should include:**
- Periodic check items
- Standing task list
- Quality gate requirements

```markdown
# Foreman Standing Tasks

## Every Check
1. Run `pytest` to ensure tests pass
2. Review if POR.md needs updating
3. Check for unresolved TODOs

## Quality Gates
- Never skip failing tests
- New code must have corresponding tests
```

> **Tip**: The more complex your task, the more important these files become. Clear intent enables autonomous progress.

---

## Installation

```bash
# Option 1: pipx (Recommended - auto-isolates environment)
pip install pipx  # if you don't have pipx
pipx install cccc-pair

# Option 2: pip
pip install cccc-pair
```

---

## Quick Start

```bash
# 1. Initialize
cd your-project && cccc init

# 2. Verify environment
cccc doctor   # Fix any issues before proceeding

# 3. Launch
cccc run      # TUI opens with interactive Setup Panel
```

**What happens on launch:**
- tmux opens with 4 panes: TUI (top-left), log (bottom-left), PeerA (top-right), PeerB (bottom-right)
- Setup Panel guides you to select CLI actors (↑↓ + Enter)
- Once configured, type `/help` to see all commands

**That's it!** The TUI guides you through the rest.

---

## Commands Reference

All commands support Tab completion. Type `/` and press Tab to explore.

| Command | Description | Example |
|---------|-------------|---------|
| `/help` | Show full command list | `/help` |
| `/a <text>` | Send message to PeerA | `/a Review the auth logic` |
| `/b <text>` | Send message to PeerB | `/b Fix the failing test` |
| `/both <text>` | Send message to both peers | `/both Let's plan the next milestone` |
| `/pause` | Pause handoff delivery (messages saved to inbox) | `/pause` |
| `/resume` | Resume handoff delivery (sends NUDGE for pending) | `/resume` |
| `/restart peera\|peerb\|both` | Restart peer CLI process | `/restart peerb` |
| `/quit` | Exit CCCC (detach tmux) | `/quit` |
| `/setup` | Toggle Setup Panel | `/setup` |
| `/foreman on\|off\|status\|now` | Control Foreman (if enabled) | `/foreman status` |
| `/aux <prompt>` | Run Aux helper once | `/aux Run full test suite` |
| `/verbose on\|off` | Toggle peer summaries + Foreman CC | `/verbose off` |

### Natural Language Routing

You can also use routing prefixes for natural language input (no slash needed):

```
a: Review the authentication logic and suggest improvements
b: Implement the fix with comprehensive tests
both: Let's discuss the roadmap for next quarter
```

> **Full command reference**: See [docs/COMMANDS.md](docs/COMMANDS.md) for cross-platform command matrix and keyboard shortcuts.

---

## How It Works

### Core Workflow

1. **User sends a goal** via TUI or IM (e.g., "Add OAuth support")
2. **PeerA frames intent** with acceptance criteria and constraints
3. **PeerB counters** with a sharper path or safer rollout
4. **Peers iterate** until consensus, then implement with small patches (≤150 lines)
5. **Evidence gates progress**: Only tested patches, stable logs, and commits count as "done"

### Key Concepts

- **Mailbox Protocol** — Peers exchange `<TO_USER>` and `<TO_PEER>` messages with evidence refs
- **POR/SUBPOR Anchors** — Strategic board (`docs/por/POR.md`) and per-task sheets live in your repo
- **Evidence Types** — Patch diffs, test logs, benchmark results, commit hashes

> **Deep dive**: See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for the full collaboration architecture.

---

## Optional Features

### Aux (On-Demand Helper)

A third peer for burst work — strategic reviews, heavy tests, bulk transforms.

- Enable in Setup Panel by selecting an actor for `aux`
- Invoke: `/aux <prompt>` in TUI, or `/aux` in chat bridges
- Runs once per invocation, no persistent state

### Foreman (User Proxy)

A lightweight timer-based agent (default: 15 minutes) that performs periodic checks.

- Enable in Setup Panel by selecting an actor for Foreman
- Configure tasks in `FOREMAN_TASK.md` at repo root
- Control: `/foreman on|off|status|now`

### Auto-Compact

Automatically compresses peer context during idle periods to prevent token waste.

- Triggers after ≥6 messages, 15 min interval, 2 min idle (configurable)
- Zero manual intervention required

> **Configuration details**: See [docs/ADVANCED.md](docs/ADVANCED.md) for full feature documentation.

---

## IM Bridges (Telegram/Slack/Discord)

CCCC includes optional chat bridges to bring the work to where your team already is.

### Features

- **Routing**: Use `a:`, `b:`, or `both:` prefixes, or `/a`, `/b`, `/both` commands
- **Bidirectional File Exchange**: Upload files to peers for processing; receive files generated by peers automatically
- **RFD Cards**: Inline approval buttons for Request-For-Decision cards
- **Peer Summaries**: Optional (toggle with `/verbose on|off`)

### Setup

1. **Create a bot** (Telegram: @BotFather, Slack: App Studio, Discord: Developer Portal)
2. **Set token** via TUI Setup Panel (select the bridge section and enter token when prompted)
3. **Allowlist your chat**:
   - Start a conversation with the bot, send `/whoami` to get your `chat_id`
   - Add `chat_id` to `.cccc/settings/telegram.yaml` (or slack.yaml/discord.yaml) allowlist
4. **Autostart** (optional):
   - Set `autostart: true` in config to launch bridge with `cccc run`

> **Chat commands**: See [docs/COMMANDS.md](docs/COMMANDS.md) for full IM command reference.

---

## A Typical Session (End-to-End, ~3 Minutes)

### 1. Explore (Short)

In TUI or chat, route an idea to both peers:

```
both: Add a short section to README about team chat tips
```

- PeerA frames intent
- PeerB asks one focused question

### 2. Decide (Concise CLAIM)

- PeerA writes a CLAIM in `to_peer.md` with acceptance criteria and constraints
- PeerB COUNTERs with a sharper path or safer rollout

### 3. Build (Evidence-First)

- Peers propose small, verifiable changes with 1-2 line EVIDENCE notes:
  - `tests OK` / `stable logs` / `commit:abc123`
- Orchestrator logs outcomes to ledger
- Status panel updates

### 4. Team Visibility

- Telegram/Slack/Discord (if enabled) receive concise summaries
- Peers stay quiet unless blocked

### Cadence

- **Self-Check**: Every N handoffs (configurable, default 20), orchestrator triggers a short alignment check
- **POR Update**: PeerB receives periodic reminders to review `POR.md` and all active `SUBPOR.md` files
- **Auto-Compact**: When peers are idle after sufficient work, orchestrator automatically compacts context (default: ≥6 messages, 15 min interval, 2 min idle)
- **Foreman Runs**: Every 15 minutes (if enabled), Foreman performs one standing task or writes one request

---

## Folder Layout

```
.cccc/                    # Orchestrator domain (gitignored)
  settings/               # Configuration (TUI handles most changes)
  mailbox/                # Message exchange between peers
  state/                  # Runtime state, logs, ledger
docs/por/                 # Strategy anchors
  POR.md                  # Strategic board
  T######-slug/SUBPOR.md  # Per-task sheets
PROJECT.md                # Your project brief (injected into system prompts)
FOREMAN_TASK.md           # Foreman tasks (if using Foreman)
```

---

## Configuration

CCCC follows "convention over configuration" principles. Sensible defaults work out of the box.

### Key Config Files (All in `.cccc/settings/`)

- **`cli_profiles.yaml`** — Actor bindings, roles, delivery settings (mailbox, nudge, keepalive, auto-compact)
- **`agents.yaml`** — CLI actor definitions and capabilities (compact support, commands, IO profiles)
- **`policies.yaml`** — Strategic policies (autonomy level, handoff filters)
- **`telegram.yaml`** — Telegram bridge config (token, allowlist, routing)
- **`slack.yaml`** — Slack bridge config (similar structure)
- **`discord.yaml`** — Discord bridge config (similar structure)
- **`foreman.yaml`** — Foreman agent and cadence

**No manual editing required** — TUI Setup Panel handles all common changes. Advanced users can tweak YAML directly for fine-grained control.

### Environment Variables (Optional Overrides)

- `CLAUDE_I_CMD` — Override default `claude` command (e.g., `claude-dev`)
- `CODEX_I_CMD` — Override default `codex` command
- `GEMINI_I_CMD` — Override default `gemini` command
- `CCCC_HOME` — Override default `.cccc` directory path

---

## FAQ

**Do I need to learn all the commands?**
No! Setup Panel uses point-and-click (↑↓ + Enter). Tab completion and `/help` cover the rest.

**Can I use CCCC without Telegram/Slack/Discord?**
Yes! TUI works perfectly standalone. IM bridges are optional.

**What about safety?**
Chats never change state directly — only evidence (patches/tests/logs) does. Irreversible changes require dual-sign from both peers. Full audit trail in ledger.

**How do I reset for a new task?**
Run `cccc reset` (or `cccc reset --archive` to preserve old POR/SUBPOR).

**How do I debug issues?**
Check `.cccc/state/status.json`, `ledger.jsonl`, `orchestrator.log`, or run `cccc doctor`.

> **More questions?** See [docs/FAQ.md](docs/FAQ.md) for the complete FAQ.

---

## Community

**📱 Join our Telegram group**: [t.me/ccccpair](https://t.me/ccccpair)

Share workflows, troubleshoot issues, and connect with other CCCC users.

---

**CCCC Pair** — Modern orchestration for modern teams. 🚀
