<div align="center">

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="web/public/logo-dark.svg">
  <img src="web/public/logo.svg" width="160" alt="CCCC logo" />
</picture>

# CCCC

Coordinate coding agents through one durable Rust control plane.

[中文](README.zh-CN.md) | **English** | [日本語](README.ja.md)

</div>

CCCC combines a Rust daemon, CLI, MCP server, Web API, terminal runtime, and the existing React/TypeScript UI. Groups share an append-only ledger, structured context, read cursors, attachments, memory, capabilities, and actor lifecycle state.

## Install

Download the archive for your platform from GitHub Releases and place these binaries on `PATH`:

- `cccc`
- `ccccd`
- `cccc-mcp`
- `cccc-web`

Build from source when developing CCCC:

```bash
npm ci --prefix web
npm -C web run build
cargo build --workspace --release --locked
```

Run the compiled Rust CLI through Cargo. It uses `CCCC_HOME`, which defaults to `~/.cccc`:

```bash
cargo run --release -p cccc-cli --bin cccc
```

After the server binds successfully, the terminal prints the Web address and port.

Arguments after `--` are passed to CCCC. For example:

```bash
cargo run --release -p cccc-cli --bin cccc -- doctor
cargo run --release -p cccc-cli --bin cccc -- groups
```

Requirements: a supported Rust build uses Rust 1.88+; building the Web UI requires Node.js 20+. Running a release archive does not require Python or Node.js.

## Quick Start

```bash
cd /path/to/project
cccc daemon start
cccc group create --title "My team"
cccc groups
cccc group use <group_id> .
cccc actor add foreman --runtime claude
cccc actor add implementer --runtime codex
cccc group start
cccc send "Inspect the repository and report the first concrete task." --to foreman
cccc
```

Open <http://127.0.0.1:8848>. `cccc setup` prints the MCP server configuration for the current Rust installation.

## Data Compatibility

Python and Rust use the same `CCCC_HOME`:

```text
CCCC_HOME=${HOME}/.cccc
```

The default is `~/.cccc`. Rust reads the existing registry, groups, plain or gzip-compressed ledgers, state, and both Python access-token document layouts in place. Rust compaction writes Python-compatible segment names and manifests. On first startup it adds a `.cccc-rust-v1` compatibility marker but does not move or delete existing data. Back up `CCCC_HOME` before first use with a new implementation.

The repository keeps implementation selection explicit:

```bash
git switch python  # legacy Python implementation and ~/.cccc
git switch rust    # Rust implementation and the same ~/.cccc
```

Stop the active daemon before switching branches. Never run the Python and Rust daemons against the shared home at the same time.

## Main Commands

```bash
cccc --help
cccc daemon start|stop|status|run
cccc group create|show|update|start|stop|use
cccc actor list|add|update|start|stop|restart|secrets
cccc prompt <actor_id>
cccc send|tracked-send|reply|inbox|read|tail|ledger
cccc im set|config|start|stop|status|bind|pending|authorized|reject|revoke
cccc space status|bind|unbind|sync|ingest|query|sources|jobs|auth
cccc runtime list
cccc doctor
cccc mcp
cccc web
```

Use `cccc <command> --help` for exact arguments.

## Architecture

```text
React/TypeScript UI     CLI     MCP     remote connectors
          \              |       |             /
                   Rust Web/API
                         |
                  versioned daemon IPC
                         |
                    Rust daemon
          group state / ledger / runtime / memory
                         |
                  CCCC_HOME only
```

The daemon is the state writer. The CLI, Web API, and MCP port call the same operations rather than maintaining parallel state.

Workspace crates:

```text
cccc-contracts  shared IPC and event contracts
cccc-core       groups, ledger, scopes, memory, policy, Rust Home
cccc-runtime    PTY and headless process sessions
cccc-client     daemon IPC client
cccc-daemon     state operations and runtime lifecycle
cccc-mcp        MCP catalog, local tools, and daemon mapping
cccc-web        HTTP, WebSocket, browser, and embedded Web UI
cccc-cli        user-facing cccc command
```

## Integrations

- Web Model connectors bind remote MCP access to one group and actor. Chromium profiles under Rust Home preserve browser login state.
- Group Bridge pairs two instances, issues scoped credentials, supports idempotent messages and attachments, delivery receipts, WebSocket sessions, and access-filtered remote MCP.
- Group Space provides work/memory lanes, idempotent ingest, sources, jobs, local fallback search, and an optional NotebookLM browser login surface.
- Voice Secretary supports documents, leases, sessions, Browser ASR transcripts, and explicit `asr_unavailable` responses when no local transcription backend is configured.
- IM configuration and authorization state covers Telegram, Slack, Discord, Feishu, DingTalk, WeCom, and Weixin. The Rust package does not report an external network adapter as running unless that adapter is actually available.

## Docker

```bash
docker volume create cccc-data
docker compose -f docker/docker-compose.yml up --build
```

The container stores Rust state in `/data` through `CCCC_HOME=/data` and publishes the Web UI to localhost by default.

## Verification

```bash
scripts/pre_commit_checks.sh
scripts/build_package.sh
docker build -f docker/Dockerfile .
```

The standard gate runs Web lint/typecheck/build plus Rust format, Clippy, and workspace tests.

## Documentation

- [Getting started](docs/guide/getting-started/index.md)
- [CLI reference](docs/reference/cli.md)
- [Architecture](docs/reference/architecture.md)
- [Operations](docs/guide/operations.md)
- [Rust migration and compatibility](docs/rust-migration.md)

## License

Apache-2.0. See [LICENSE](LICENSE).
