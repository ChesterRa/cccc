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

- **Python 3.9+** installed
- At least one AI agent CLI:
  - [Claude Code](https://docs.anthropic.com/en/docs/claude-code) (recommended)
  - [Codex CLI](https://github.com/openai/codex)
  - [GitHub Copilot CLI](https://docs.github.com/en/copilot)
  - Or any other supported runtime

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

### From PyPI

```bash
python -m pip install -U cccc-pair
```

### From TestPyPI (for explicit RC testing)

```bash
python -m pip install --index-url https://pypi.org/simple \
  --extra-index-url https://test.pypi.org/simple \
  cccc-pair==<rc-version>
```

### From Source

```bash
git clone https://github.com/ChesterRa/cccc
cd cccc
pip install -e .
```

## Verify Installation

```bash
cccc doctor
```

This checks Python version, available runtimes, and system configuration.

## Next Steps

- [Web UI Quick Start](./web) - Get started with the visual interface
- [CLI Quick Start](./cli) - Get started with the command line
- [Docker Deployment](./docker) - Deploy CCCC in a Docker container
- [Use Cases](/guide/use-cases) - Learn high-ROI real-world patterns
- [Operations Runbook](/guide/operations) - Run CCCC with operator-grade reliability
- [Positioning](/reference/positioning) - Decide where CCCC should sit in your stack
