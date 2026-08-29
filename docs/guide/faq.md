# FAQ

Frequently asked questions about CCCC.

## Positioning

### How does CCCC compare to native agent teams and other tools?

**vs. native agent teams (Claude Code subagents/agent teams and similar single-vendor features).**
Native teams give you the smoothest experience inside one vendor and one session — if you only run Claude Code and your work fits in a session, they are a great default. CCCC adds what a single vendor structurally cannot:

- **Cross-vendor groups** — Claude Code, Codex CLI, Grok Build, Kimi CLI, ChatGPT Web, and more in one group, so you can route work to whichever model or subscription fits each role.
- **Durable state** — groups, messages, delivery/read/reply facts, and tasks live in an append-only ledger owned by a daemon. Restarting a terminal (or your machine) does not dissolve the team.
- **Remote operations** — check, pause, resume, and redirect a running group from Telegram, Slack, Discord, Feishu, DingTalk, WeCom, or Weixin.
- **An audit trail** — every message and its delivery state is replayable for review and debugging.

**vs. parallel task runners (worktree/task-board tools).**
These tools excel at fanning out isolated tasks in parallel. CCCC's focus is the coordination layer they intentionally skip: agents that talk to each other, choose whether a message should interrupt or wait in Mail, hand off tracked work, and expose delivery/read/reply state — plus daemon-owned lifecycle and IM-side operations. The two approaches compose well: keep a task runner for fan-out and use CCCC as the durable coordination plane.

**vs. IM assistant gateways (personal-assistant products that live in your chat app).**
Those products put a general assistant in your messenger. CCCC is built for delivery-grade collaboration on real work: tracked tasks with owners and outcomes, explicit delivery/read/reply semantics, multi-agent groups bound to a repository scope, and a tiered token and capability-allowlist security model.

In short: CCCC does not replace your agents — it is the coordination layer that turns them into a durable, observable team. See also [Positioning](/reference/positioning) for what CCCC deliberately is and is not.

## Installation & Setup

### How do I install CCCC?

```bash
# Recommended native installer (macOS / Linux)
curl -fsSL https://chesterra.github.io/cccc/install.sh | sh

# Package-manager-compatible native wheel
python -m pip install -U "cccc-pair>=0.4.36"

# From source
git clone https://github.com/ChesterRa/cccc
cd cccc
./scripts/build_package.sh
./target/release/cccc --version
```

The source package helper requires Rust 1.88+, Node.js 24 with npm, and Python
3.11+ for archive assembly only; the resulting CCCC executable has no Python
runtime dependency.

Keep the v0.4.36 lower bound so an unsupported platform fails clearly instead
of selecting the historical Python product.

Windows CMD or PowerShell uses the native installer:

```bash
powershell.exe -NoProfile -ExecutionPolicy Bypass -Command "[Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12; Invoke-RestMethod 'https://chesterra.github.io/cccc/install.ps1' | Invoke-Expression"
```

Both installation channels provide the same Rust executable. The pip wheel has
no importable CCCC Python package or fallback implementation.

### Why does `cccc` still start an older installation after an upgrade?

CCCC does not delete commands owned by another Python environment or installer.
Run `cccc doctor` and inspect the `Installation` section. It shows the current
launcher, the first command selected by PATH, and every duplicate. The standalone
installer never infers ownership from version output. Rust self-update identifies
its exact current executable only when the containing directory has the complete
`.cccc-standalone` ownership marker. Markerless commands and foreign ownership
markers remain untouched unless `CCCC_ALLOW_REPLACE_EXISTING=1` explicitly
authorizes migration through the installer. It puts its default directory first for new terminals; an
existing terminal must be reopened (or its PATH refreshed). Custom install directories and
`CCCC_NO_MODIFY_PATH=1` require you to move that directory to the front manually.
When PATH still selects the old command, run the newly installed executable by
the absolute path printed by the installer to get the current diagnostic report.
The native pip wheel uses the same marker path with value `pip-v1`, so a pip
install cannot inherit stale standalone self-update authority. To move that
command directory to the website installer, uninstall `cccc-pair` with pip
first; the explicit replacement flag does not override pip ownership.

### How do I uninstall CCCC without losing my groups?

Run `cccc home` first and stop the running product. Pip installations should be
removed with `python -m pip uninstall cccc-pair`. For a website-script install,
verify the complete `.cccc-standalone` ownership marker and remove only the
owned executable and marker; do not delete a shared command directory. CCCC
intentionally retains `CCCC_HOME`, so groups, ledgers, settings, credentials,
and browser profiles remain available after reinstall. Exact macOS/Linux and
Windows standalone steps are in the
[distribution and migration guide](../rust-migration.md#uninstall-without-removing-user-data).

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

- A supported 64-bit Linux, macOS, or Windows target
- At least one supported agent runtime CLI

Normal installation requires neither Python nor a Rust toolchain. The MCP
JavaScript code mode additionally requires Node.js on the CCCC host.

### Can I choose a different product implementation?

You do not. CCCC 0.4.36 has one Rust product implementation. The 0.4.35
`cccc python` / `cccc rust` selectors and persisted implementation preference
are retired.

### How do I check if CCCC is working?

```bash
cccc status
cccc doctor
```

These show native product and daemon status, agent runtimes, installation
ownership, and environment diagnostics.

### Does a leftover `cccc-web.lock` mean CCCC is still running?

No. The operating-system file lock is authoritative; the file may retain the
last owner's PID after a crash. `cccc` automatically reclaims an unlocked file,
replaces the PID, and starts its embedded daemon when the Web process starts.
Manual lock deletion or a separate `cccc daemon start` is not required. If CCCC
reports that another instance is running, that process still holds the real file
lock and should be stopped normally.

### Why does an embedded browser open a physical Chrome window on Linux?

Projected browsers require `Xvfb` to stay off the host desktop. Install `xvfb` (and optionally
`x11vnc` for the VNC viewer), run `cccc doctor`, then use **Restart ChatGPT browser**. Current CCCC
fails browser startup when Xvfb is missing instead of silently falling back to the host `DISPLAY`.

## Agents

### Which AI agents are supported?

- Claude Code (`claude`)
- Cline CLI (`cline`)
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

Agents call `cccc_inbox_read` to receive and consume the next ordered batch.
The returned batch boundary is committed cumulatively; bootstrap previews and
Web polling do not consume messages.

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

- Daemon: Minimal native background service
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
