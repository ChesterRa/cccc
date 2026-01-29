# CCCC — Multi-Agent Collaboration Kernel

**English** | [中文](README.zh-CN.md) | [日本語](README.ja.md)

> **Status**: 0.4.0rc17 (Release Candidate)

[![Documentation](https://img.shields.io/badge/docs-online-blue)](https://dweb-channel.github.io/cccc/)
[![License](https://img.shields.io/badge/license-Apache--2.0-green)](LICENSE)

CCCC is a **local-first multi-agent collaboration kernel** that coordinates AI agents like a modern IM.

**Key features**:
- 🤖 **Multi-runtime support** — Claude Code, Codex CLI, Droid, OpenCode, Copilot, and more
- 📝 **Append-only ledger** — Durable history as single source of truth
- 🌐 **Web-first console** — Mobile-friendly control plane
- 💬 **IM-grade messaging** — @mentions, reply/quote, read receipts
- 🔧 **MCP tool surface** — 38+ tools for reliable agent operations
- 🔌 **IM Bridge** — Telegram, Slack, Discord, Feishu, DingTalk

![CCCC Chat UI](screenshots/chat.png)

---

## Quick Start

```bash
# Install
pip install --index-url https://pypi.org/simple \
  --extra-index-url https://test.pypi.org/simple \
  cccc-pair==0.4.0rc17

# Start
cccc
```

Open `http://127.0.0.1:8848/` to access the Web UI.

---

## Documentation

📚 **[Read the Docs](https://dweb-channel.github.io/cccc/)** — Full guides, reference, and API documentation.

---

## Installation

### Install with AI Assistant

Copy this prompt to your AI assistant (Claude, ChatGPT, etc.):

> Please help me install and start CCCC (Claude Code Collaboration Context) multi-agent collaboration system.
>
> Steps:
>
> 1. Install cccc-pair:
>    ```
>    pip install --index-url https://pypi.org/simple \
>      --extra-index-url https://test.pypi.org/simple \
>      cccc-pair==0.4.0rc17
>    ```
>
> 2. After installation, start CCCC:
>    ```
>    cccc
>    ```
>
> 3. Tell me the access URL (usually http://localhost:8848/ui/)
>
> If you encounter any errors, please help me diagnose and resolve them.

### Upgrading from older versions

If you have an older version of cccc-pair installed (e.g., 0.3.x), you must uninstall it first:

```bash
# For pipx users
pipx uninstall cccc-pair

# For pip users
pip uninstall cccc-pair

# Remove any leftover binaries if needed
rm -f ~/.local/bin/cccc ~/.local/bin/ccccd
```

> **Note**: Version 0.4.x has a completely different command structure from 0.3.x. The old `init`, `run`, `bridge` commands are replaced with `attach`, `daemon`, `mcp`, etc.

### From TestPyPI (recommended)

```bash
pip install --index-url https://pypi.org/simple \
  --extra-index-url https://test.pypi.org/simple \
  cccc-pair==0.4.0rc17
```

### From source

```bash
git clone https://github.com/dweb-channel/cccc
cd cccc
pip install -e .
```

### Using uv (recommended for Windows)

```bash
uv venv -p 3.11 .venv
uv pip install -e .
uv run cccc --help
```

**Requirements**: Python 3.9+, macOS / Linux / Windows

---

## Core Concepts

| Concept | Description |
|---------|-------------|
| **Working Group** | Collaboration unit with durable history (like a group chat) |
| **Actor** | An agent session (PTY or headless) |
| **Scope** | A directory attached to a group |
| **Ledger** | Append-only event stream |
| **CCCC_HOME** | Runtime home, default `~/.cccc/` |

---

## Runtimes & MCP

CCCC supports multiple agent runtimes:

```bash
cccc runtime list --all    # List available runtimes
cccc setup --runtime <name> # Configure MCP
```

**Auto MCP setup**: `claude`, `codex`, `droid`, `amp`, `auggie`, `neovate`, `gemini`
**Manual setup**: `cursor`, `kilocode`, `opencode`, `copilot`, `custom`

---

## Multi-Agent Setup

To set up multi-agent collaboration on a project:

```bash
# Attach to your project directory
cd /path/to/repo
cccc attach .

# Setup MCP for your runtime
cccc setup --runtime claude

# Add actors (first enabled actor becomes foreman)
cccc actor add foreman --runtime claude
cccc actor add peer-1  --runtime codex

# Start the group
cccc group start
```

---

## Web UI

The bundled Web UI provides:

- Multi-group navigation
- Actor management (add/start/stop/restart)
- Chat with @mentions and reply
- Embedded terminal per actor
- Context & automation settings
- IM Bridge configuration

---

## IM Bridge

Bridge your working group to IM platforms:

```bash
cccc im set telegram --token-env TELEGRAM_BOT_TOKEN
cccc im start
```

Supported: **Telegram** | **Slack** | **Discord** | **Feishu/Lark** | **DingTalk**

---

## CLI Cheat Sheet

```bash
cccc doctor              # Check environment
cccc groups              # List groups
cccc use <group_id>      # Switch group
cccc send "msg" --to @all
cccc inbox --mark-read
cccc tail -n 50 -f
cccc daemon status|start|stop
```

---

## PROJECT.md

Place `PROJECT.md` at your repo root as the project constitution. Agents read it via `cccc_project_info` MCP tool.

---

## Security Notes

The Web UI has high privilege. For remote access:
- Set `CCCC_WEB_TOKEN` environment variable
- Use an access gateway (Cloudflare Access, Tailscale, WireGuard)

---

## Why a Rewrite?

<details>
<summary>History: v0.3.x → v0.4.x</summary>

v0.3.x (tmux-first) proved the concept but hit limits:

1. **No unified ledger** — Messages in multiple files caused latency
2. **Actor count limit** — tmux layout limited to 1–2 actors
3. **Weak agent control surface** — Limited autonomy
4. **No first-class remote access** — Web control plane needed

v0.4.x introduces:
- Unified append-only ledger
- N-actor model
- MCP control plane with 38+ tools
- Web-first console
- IM-grade messaging

Legacy version: [cccc-tmux](https://github.com/ChesterRa/cccc-tmux)

</details>

---

## License

Apache-2.0
