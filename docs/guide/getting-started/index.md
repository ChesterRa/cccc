# Getting Started

Get CCCC running in 10 minutes.

## Terminology Alignment

This page follows the local glossary:

- `group` is the main collaboration unit
- `actor` is the live scheduled participant
- `profile` is reusable actor runtime configuration and launch intent
- `attach` sets the group's `authoritative_workspace`
- older `scope` wording may still appear as compatibility shorthand

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
  - [Kimi CLI](https://github.com/MoonshotAI/kimi-cli)
  - Or a custom runtime command if you wire MCP manually

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
pip install -U cccc-pair
```

### From TestPyPI (for explicit RC testing)

```bash
pip install -U --pre \
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
cccc doctor
```

This checks Python version, available runtimes, and system configuration.

If you already know you want reusable runtime intent instead of one-off actor
creation, the current CLI supports a profile-backed start path:

```bash
cccc actor profile upsert --id starter-shared --name "Starter Shared" --runtime claude
cccc actor add starter --profile-id starter-shared
```

That is the profile-backed setup path. The live scheduled participant is still
the actor; the profile stores reusable runtime defaults.

## Next Steps

- [Web UI Quick Start](./web) - Get started with the visual interface
- [CLI Quick Start](./cli) - Get started with the command line
- [Docker Deployment](./docker) - Deploy CCCC in a Docker container
- [SDK Overview](/sdk/) - Integrate CCCC into external apps/services
- [Use Cases](/guide/use-cases) - Learn high-ROI real-world patterns
- [Operations Runbook](/guide/operations) - Run CCCC with operator-grade reliability
- [Positioning](/reference/positioning) - Decide where CCCC should sit in your stack

## Related Glossary

- [group](/reference/glossary/group)
- [actor](/reference/glossary/actor)
- [profile](/reference/glossary/profile)
- [attach](/reference/glossary/attach)
- [authoritative_workspace](/reference/glossary/authoritative_workspace)

## Change Log

- `2026-03-23`: Added a profile-backed setup pointer so the landing page reflects the current actor/profile product split.
- `2026-03-23`: Added glossary alignment so the main getting-started entry uses repo-local `group` / `actor` / `profile` / `attach` meanings instead of leaving old shorthand implicit.
