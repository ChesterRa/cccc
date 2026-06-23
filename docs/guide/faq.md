# FAQ

Frequently asked questions about CCCC.

## Positioning

### How does CCCC compare to native agent teams and other tools?

**vs. native agent teams (Claude Code subagents/agent teams and similar single-vendor features).**
Native teams give you the smoothest experience inside one vendor and one session — if you only run Claude Code and your work fits in a session, they are a great default. CCCC adds what a single vendor structurally cannot:

- **Cross-vendor groups** — Claude Code, Codex CLI, Grok Build, Kimi CLI, ChatGPT Web, and more in one group, so you can route work to whichever model or subscription fits each role.
- **Durable state** — groups, messages, read/ack receipts, and tasks live in an append-only ledger owned by a daemon. Restarting a terminal (or your machine) does not dissolve the team.
- **Remote operations** — check, pause, resume, and redirect a running group from Telegram, Slack, Discord, Feishu, DingTalk, WeCom, or Weixin.
- **An audit trail** — every message and its delivery state is replayable for review and debugging.

**vs. parallel task runners (worktree/task-board tools).**
These tools excel at fanning out isolated tasks in parallel. CCCC's focus is the coordination layer they intentionally skip: agents that talk to each other, hand off work, acknowledge attention messages, and get nudged when they stall — plus daemon-owned lifecycle and IM-side operations. The two approaches compose well: keep a task runner for fan-out and use CCCC as the durable coordination plane.

**vs. IM assistant gateways (personal-assistant products that live in your chat app).**
Those products put a general assistant in your messenger. CCCC is built for delivery-grade collaboration on real work: tracked tasks with owners and outcomes, read/ack semantics, multi-agent groups bound to a repository scope, and a tiered token and capability-allowlist security model.

In short: CCCC does not replace your agents — it is the coordination layer that turns them into a durable, observable team. See also [Positioning](/reference/positioning) for what CCCC deliberately is and is not.

## Installation & Setup

### How do I install CCCC?

```bash
# From PyPI
pip install -U cccc-pair

# From TestPyPI (explicit RC testing)
pip install -U --pre \
  --index-url https://test.pypi.org/simple \
  --extra-index-url https://pypi.org/simple \
  cccc-pair

# From source
git clone https://github.com/ChesterRa/cccc
cd cccc
pip install -e .
```

### How do I upgrade from an older version (0.3.x)?

You must uninstall the old version first:

```bash
# For pipx users
pipx uninstall cccc-pair

# For pip users
pip uninstall cccc-pair

# Remove any leftover binaries
rm -f ~/.local/bin/cccc ~/.local/bin/ccccd
```

Then install the new version. Note that 0.4.x has a completely different command structure from 0.3.x.

### What are the system requirements?

- Python 3.9+
- macOS, Linux, or Windows
- At least one supported agent runtime CLI

### How do I check if CCCC is working?

```bash
cccc doctor
```

This checks Python version, available runtimes, and daemon status.

## Agents

### Which AI agents are supported?

- Claude Code (`claude`)
- Codex CLI (`codex`)
- GitHub Copilot CLI (`copilot`)
- Cursor CLI (`cursor-agent`)
- Devin CLI (`devin`)
- Kiro CLI (`kiro-cli`)
- Kilo Code CLI (`kilo`)
- Antigravity CLI (`agy`)
- Droid (`droid`)
- Grok Build (`grok`)
- Hermes Agent (`hermes`)
- Kimi CLI (`kimi`)
- OpenCode (`opencode`)
- Amp (`amp`)
- Auggie (`auggie`)
- Custom (manual fallback; provide your own command and MCP wiring)

### What's the difference between Foreman and Peer?

- **Foreman**: The first enabled actor. Coordinates work, receives system notifications, can manage other actors.
- **Peer**: Independent expert. Has their own judgment, can only manage themselves.

### How do I add a custom agent?

```bash
cccc actor add my-agent --runtime custom --command "my-custom-cli"
```

### Agent won't start?

1. Check the terminal tab for error messages
2. Verify MCP is configured: `cccc setup --runtime <name>`
3. Ensure the CLI is installed and in PATH
4. Try: `cccc actor restart <actor_id>`

## Messaging

### How do I send a message to a specific agent?

```bash
cccc send "Please do X" --to agent-name
```

Or in the Web UI, type `@agent-name` in your message.

### Agent isn't responding to my messages?

1. Check if the agent is running (green indicator in Web UI)
2. Check the inbox: `cccc inbox --actor-id <agent-id>`
3. Look at the terminal tab for errors
4. Try restarting the agent

### How do read receipts work?

Agents call `cccc_inbox_mark_read` to mark messages as read. This is cumulative - marking message X means all messages up to X are read.

## Remote Access

### How do I access CCCC from my phone?

**Option 1: Cloudflare Tunnel**
```bash
cloudflared tunnel --url http://127.0.0.1:8848
```

**Option 2: IM Bridge**
```bash
cccc im set telegram --token-env TELEGRAM_BOT_TOKEN
cccc im start
```

**Option 3: Tailscale**
```bash
CCCC_WEB_HOST=$(tailscale ip -4) cccc
```

### Is it safe to expose the Web UI?

Before exposing the Web UI, create an **Admin Access Token** in **Settings > Web Access** and then sign in with that token.

Use Cloudflare Access or Tailscale for additional security.

## Performance

### How much resources does CCCC use?

- Daemon: Minimal (Python async)
- Web UI: Standard React app
- Agents: Depends on the runtime

### The ledger file is getting large

CCCC supports snapshot/compaction. Large blobs are stored separately in the `blobs/` directory.

### How do I reduce message latency?

1. Ensure agents are already running
2. Use specific @mentions instead of broadcasts
3. Keep the daemon running (don't restart frequently)

## Troubleshooting

### Daemon won't start

```bash
cccc daemon status  # Check if already running
cccc daemon stop    # Stop existing instance
cccc daemon start   # Start fresh
```

### Port 8848 is unavailable

```bash
CCCC_WEB_PORT=9000 cccc
```

On Windows, Hyper-V / WSL / WinNAT / HNS can reserve a TCP port even when no
process is listening on it. If `8848` still fails to start and you do not see an
owning PID, check the excluded port ranges:

```powershell
netsh interface ipv4 show excludedportrange protocol=tcp
```

If `8848` falls inside one of those ranges, start CCCC on a different port:

```powershell
cccc web --port 9000
```

### MCP not working

```bash
cccc setup --runtime <name>  # Re-run setup
cccc doctor                  # Check configuration
```

### Web UI not loading

1. Check daemon is running: `cccc daemon status`
2. Check the port: http://127.0.0.1:8848/
3. Check browser console for errors
4. Try a different browser

## Concepts

### What is a Working Group?

A working group is like an IM group chat with execution capabilities. It includes:
- An append-only ledger (message history)
- One or more actors (agents)
- Optional scopes (project directories)

### What is the Ledger?

The ledger is an append-only event stream that stores all messages, state changes, and decisions. It's the single source of truth for a working group.

### What is MCP?

MCP (Model Context Protocol) is how agents interact with CCCC. It exposes a rich tool surface for messaging, context management, automation, and system control.

### What is a Scope?

A scope is a project directory attached to a working group. Agents work within scopes, and events are attributed to scopes.
