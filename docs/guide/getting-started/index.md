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

Both approaches require:

- **Python 3.11+** installed
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

### From PyPI (recommended)

```bash
python -m pip install -U cccc-pair
```

PyPI is the stable CCCC product distribution, with Python as its stable and
recommended implementation. Supported platform wheels also contain a private,
version-matched **experimental Rust implementation** for opt-in performance
evaluation with `cccc rust`. Feature and integration parity is still in progress;
use `cccc python` for reliability-critical workflows. Other platforms receive the
universal Python wheel.

### Experimental standalone Rust preview

::: warning Rust-only preview
Use this optional channel only to evaluate deployment without Python. It has no
Python fallback or implementation switching and is not yet the recommended
replacement for the complete pip product.
:::

macOS or Linux:

```bash
curl -fsSL https://chesterra.github.io/cccc/install.sh | sh
```

Windows CMD or PowerShell:

```powershell
powershell.exe -NoProfile -ExecutionPolicy Bypass -Command "[Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12; Invoke-RestMethod 'https://chesterra.github.io/cccc/install.ps1' | Invoke-Expression"
```

The installer downloads a checksum-verified GitHub Release binary. It does not
require a Rust or Python toolchain and refuses to overwrite an existing `cccc`
command it does not own. Uninstall that command deliberately or choose another
`CCCC_INSTALL_DIR` first. During the `v0.4.34-rc2` validation period, the hosted
installer is pinned to that release candidate. You can also select it explicitly
with `CCCC_VERSION`:

Commands in other directories are never removed. The default installer puts its
directory first in the user PATH and reports every remaining `cccc` command. Open
a new terminal after installation and run `cccc doctor` to verify that PATH
resolves to the executable you just installed.

```bash
curl -fsSL https://chesterra.github.io/cccc/install.sh | CCCC_VERSION=0.4.34-rc2 sh
```

```powershell
$env:CCCC_VERSION = "0.4.34-rc2"
powershell.exe -NoProfile -ExecutionPolicy Bypass -Command "[Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12; Invoke-RestMethod 'https://chesterra.github.io/cccc/install.ps1' | Invoke-Expression"
Remove-Item Env:CCCC_VERSION
```

### From TestPyPI (for explicit RC testing)

```bash
python -m pip install -U --pre \
  --index-url https://test.pypi.org/simple \
  --extra-index-url https://pypi.org/simple \
  cccc-pair
```

### From Source

```bash
git clone https://github.com/ChesterRa/cccc
cd cccc
pip install -e .
```

## Verify Installation

```bash
cccc status
cccc doctor
```

`status` shows the selected, running, and available product implementations.
Python is the stable initial default while no implementation choice has been
saved. On a supported platform wheel, use `cccc rust` to opt into the bundled
experimental Rust implementation or `cccc python` to return to stable Python;
later bare invocations follow that persisted choice. `doctor` checks Python,
agent runtimes, system configuration, the invoked CCCC executable, PATH
resolution, and duplicate `cccc` commands.

## Next Steps

- [Web UI Quick Start](./web) - Get started with the visual interface
- [CLI Quick Start](./cli) - Get started with the command line
- [Docker Deployment](./docker) - Deploy CCCC in a Docker container
- [SDK Overview](/sdk/) - Integrate CCCC into external apps/services
- [Use Cases](/guide/use-cases) - Learn high-ROI real-world patterns
- [Operations Runbook](/guide/operations) - Run CCCC with operator-grade reliability
- [Positioning](/reference/positioning) - Decide where CCCC should sit in your stack
