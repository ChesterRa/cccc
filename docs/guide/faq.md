# FAQ

Frequently asked questions about CCCC.

## Terminology Alignment

This document follows the local glossary:

- `group` is the concise repo-local term, while `working group` remains
  compatibility wording
- `attach` defines a group's `authoritative_workspace`
- `scope` is older wording that may still appear in historical answers
- `profile` means reusable actor runtime configuration and launch intent
- `status` is an evidence-bound surface, not proof that every deeper capability
  is available
- `resume` is layered and does not only mean native runtime session continuity

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
- Droid (`droid`)
- Gemini CLI (`gemini`)
- Kimi CLI (`kimi`)
- Amp (`amp`)
- Auggie (`auggie`)
- Neovate (`neovate`)
- Custom (manual fallback; provide your own command and MCP wiring)

### What's the difference between Foreman and Peer?

- **Foreman**: The first enabled actor. Coordinates work, receives system notifications, can manage other actors.
- **Peer**: Independent expert. Has their own judgment, can only manage themselves.

### How do I add a custom agent?

```bash
cccc actor add my-agent --runtime custom --command "my-custom-cli"
```

### What is the difference between an actor and a profile?

- An `actor` is the live scheduled participant inside a group.
- A `profile` is reusable runtime configuration that an actor may link to.
- Linking a profile does not by itself prove that the actor is currently
  running, resumed, or attached to any particular live session.

Typical profile-backed path:

```bash
cccc actor profile upsert --id shared-dev --name "Shared Dev" --runtime claude
cccc actor add dev --profile-id shared-dev
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

A `group` is the core collaboration unit in CCCC. Older FAQ wording may still
say `working group` for compatibility. It includes:
- An append-only ledger (message history)
- One or more actors (agents)
- An attached project path that acts as the group's authoritative workspace

### What is the Ledger?

The ledger is an append-only event stream that stores all messages, state
changes, and decisions. It's the single source of truth for a group.

### What is MCP?

MCP (Model Context Protocol) is how agents interact with CCCC. It exposes a rich tool surface for messaging, context management, automation, and system control.

### What is a Scope?

`Scope` is older compatibility wording for an attached project directory. In
current glossary terms, `attach` sets the group's
`authoritative_workspace`, while actor runtimes may later work in the same path
or a different `execution_workspace` depending on explicit policy.

## Related Glossary

- [group](/reference/glossary/group)
- [attach](/reference/glossary/attach)
- [authoritative_workspace](/reference/glossary/authoritative_workspace)
- [execution_workspace](/reference/glossary/execution_workspace)
- [profile](/reference/glossary/profile)
- [resume](/reference/glossary/resume)
- [status](/reference/glossary/status)

## Change Log

- `2026-03-21`: Added local glossary alignment so FAQ answers stop treating `working group`, `scope`, `status`, and `resume` as self-evident legacy shorthand.
- `2026-03-23`: Added a concrete profile-management command path so the actor-versus-profile FAQ points to the current CLI surface.
- `2026-03-23`: Added `profile` alignment and an explicit actor-versus-profile FAQ so reusable runtime configuration is not confused with live actor state.
