# FAQ

Frequently asked questions about CCCC.

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
- macOS, Linux, or Windows (WSL recommended for PTY on Windows)
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
- Droid (`droid`)
- OpenCode (`opencode`)
- Gemini CLI (`gemini`)
- Amp (`amp`)
- Auggie (`auggie`)
- Cursor (`cursor`)
- Kilocode (`kilocode`)
- Neovate (`neovate`)
- Custom (any command)

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
cccc send "@agent-name Please do X"
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

Always set an authentication token:
```bash
export CCCC_WEB_TOKEN="your-secret-token"
cccc
```

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

### Port 8848 is in use

```bash
CCCC_WEB_PORT=9000 cccc
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
