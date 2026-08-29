# Getting Started

Get CCCC running in 10 minutes.

## Choose Your Approach

CCCC offers two ways to get started:

<div class="vp-card-container">

### [Web UI Quick Start](./web)

**Recommended for most users**

- Visual interface for managing agents
- Point-and-click configuration
- Real-time terminal view
- Mobile-friendly

### [CLI Quick Start](./cli)

**For terminal enthusiasts**

- Full control via command line
- Scriptable and automatable
- Great for CI/CD integration
- Power user features

### [Docker Deployment](./docker)

**For servers and teams**

- One-command deployment
- Pre-installed AI agent CLIs
- Persistent data with volumes
- Docker Compose and K8s ready

</div>

## Prerequisites

The website installer does not require Python or a Rust toolchain. You need a
supported 64-bit platform and at least one AI agent CLI:

- At least one AI agent CLI:
  - [Claude Code](https://docs.anthropic.com/en/docs/claude-code) (recommended)
  - [Codex CLI](https://github.com/openai/codex)
  - [GitHub Copilot CLI](https://docs.github.com/en/copilot/reference/copilot-cli-reference)
  - [Cursor CLI](https://cursor.com/docs/cli/overview)
  - [Devin CLI](https://docs.devin.ai/ja/cli)
  - [Kiro CLI](https://kiro.dev/docs/cli/)
  - [Kilo Code CLI](https://kilo.ai/docs/code-with-ai/platforms/cli)
  - [Antigravity CLI](https://antigravity.google/docs/cli-overview)
  - [Kimi CLI](https://github.com/MoonshotAI/kimi-cli)
- Or a ChatGPT account with remote MCP connector support for the ChatGPT Web Model runtime
- Or a custom runtime command if you wire MCP manually

The MCP JavaScript code mode (`cccc_code_exec` / `cccc_code_wait`) additionally
requires Node.js on the CCCC host. The standalone Rust build does not invoke a
Python backend.

The ChatGPT Web Model also needs a system Google Chrome or Microsoft Edge browser. On native Linux,
install `Xvfb` so CCCC can keep projected browser windows off the host desktop; `x11vnc` is optional
and enables the VNC viewer instead of the built-in CDP screencast fallback:

```bash
# Debian / Ubuntu
sudo apt install xvfb x11vnc

# Fedora
sudo dnf install xorg-x11-server-Xvfb x11vnc

# Arch Linux
sudo pacman -S xorg-server-xvfb x11vnc
```

Run `cccc doctor` to verify these dependencies. CCCC does not install OS packages automatically.

## Installation

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

::: warning Version 0.4.x Breaking Changes
Version 0.4.x has a completely different command structure from 0.3.x. The old `init`, `run`, `bridge` commands are replaced with `attach`, `daemon`, `mcp`, etc.
:::

### Website installer (recommended)

macOS or Linux:

```bash
curl -fsSL https://chesterra.github.io/cccc/install.sh | sh
```

Windows CMD or PowerShell:

```powershell
powershell.exe -NoProfile -ExecutionPolicy Bypass -Command "[Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12; Invoke-RestMethod 'https://chesterra.github.io/cccc/install.ps1' | Invoke-Expression"
```

The installer downloads a checksum-verified native executable and refuses to
overwrite a command it does not own.

### From PyPI (v0.4.36+)

```bash
python -m pip install -U "cccc-pair>=0.4.36"
```

The native platform wheel installs the same Rust `cccc` executable through pip.
The lower bound prevents pip from silently selecting a historical Python-only
release when 0.4.36 has no wheel for the current platform. It contains no Python
daemon, product package, launcher, or fallback. Unsupported platforms therefore
fail resolution rather than receiving a portable Python wheel. Generic
`pip install .` and editable `pip install -e .` source builds are rejected
instead of installing an empty package; use the source workflow below.

The pip wheel and website installer record distinct ownership beside the
executable. To switch a command directory from pip to the website installer,
first run `python -m pip uninstall cccc-pair`; the installer will not overwrite
pip-owned files, even when `CCCC_ALLOW_REPLACE_EXISTING=1` is set.

### From Source

Source packaging requires Rust 1.88+, Node.js 24 with npm, and Python 3.11+
only for the archive helper. Python is not included in the built product.

```bash
git clone https://github.com/ChesterRa/cccc
cd cccc
./scripts/build_package.sh
./target/release/cccc --version
./target/release/cccc
```

Use `cargo run --locked --features standalone -p cccc --bin cccc -- --port 0`
for an iterative debug build. On Windows, run
`powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\scripts\build_package.ps1`
and then `.\target\release\cccc.exe`.

## Verify Installation

```bash
cccc status
cccc doctor
```

`status` shows the native product, daemon, groups, actors, and detected agent
runtimes. `doctor` checks the installation, agent runtimes, system
configuration, invoked CCCC executable, PATH resolution, and duplicate `cccc`
commands.

## Next Steps

- [Web UI Quick Start](./web) - Get started with the visual interface
- [CLI Quick Start](./cli) - Get started with the command line
- [Docker Deployment](./docker) - Deploy CCCC in a Docker container
- [SDK Overview](/sdk/) - Integrate CCCC into external apps/services
- [Use Cases](/guide/use-cases) - Learn high-ROI real-world patterns
- [Operations Runbook](/guide/operations) - Run CCCC with operator-grade reliability
- [Positioning](/reference/positioning) - Decide where CCCC should sit in your stack
