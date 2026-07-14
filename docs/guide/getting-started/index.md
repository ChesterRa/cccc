# Getting Started

## Install A Release

Download the archive matching your platform from GitHub Releases. Put `cccc`, `ccccd`, `cccc-mcp`, and `cccc-web` on `PATH`.

```bash
cccc version
cccc home
cccc doctor
```

The release binaries do not require Python. `cccc home` should print `~/.cccc` unless `CCCC_HOME` is set.

## Build From Source

Requirements:

- Rust 1.88+
- Node.js 20+
- npm

```bash
git switch rust
npm ci --prefix web
npm -C web run build
cargo build --workspace --release --locked
export PATH="$PWD/target/release:$PATH"
```

## Create A Group

```bash
cd /path/to/project
cccc group create --title "Project team"
cccc groups
cccc group use <group_id> .
cccc actor add foreman --runtime claude
cccc actor add peer1 --runtime codex
cccc group start
cccc send "Inspect the project and report the first task." --to foreman
```

Run `cccc` and open <http://127.0.0.1:8848>.

## Configure MCP

```bash
cccc setup
```

The output is a JSON MCP server entry using the current `cccc` executable and `CCCC_HOME`. Apply it to the selected agent runtime according to that runtime's MCP configuration format.

## Data Safety

Rust and Python default to the same `~/.cccc` home and share the registry, group, ledger, and state contracts. Rust adds a compatibility marker on first startup without moving or deleting existing files. Stop the active daemon before switching branches.

## Next Guides

- [CLI quick start](./cli.md)
- [Docker](./docker.md)
- [Architecture](../../reference/architecture.md)
- [Operations](../operations.md)
