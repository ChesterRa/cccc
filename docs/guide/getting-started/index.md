# Getting Started

## Install A Release

The release installer installs or upgrades the single Rust `cccc` executable. Daemon, Web, and MCP modes are included in that executable.

macOS / Linux:

```bash
bash -o pipefail -c 'curl -fsSL https://github.com/ChesterRa/cccc/releases/latest/download/install.sh | bash'
```

Windows PowerShell:

```powershell
& { $ErrorActionPreference = "Stop"; irm https://github.com/ChesterRa/cccc/releases/latest/download/install.ps1 | iex }
```

Open a new terminal after the installer updates your user `PATH`, then run:

```bash
cccc version
cccc home
cccc doctor
```

The release binaries do not require Python, Node.js, or Rust. `cccc home` should print `~/.cccc` unless `CCCC_HOME` is set. The installer never modifies that directory.

The ChatGPT Web Model also needs a system Google Chrome or Microsoft Edge browser. On native Linux,
install `Xvfb` to keep projected browser windows off the host desktop. `x11vnc` is optional because
the embedded viewer can fall back to CDP screencasting. Run `cccc doctor` to verify these dependencies.
Rust release binaries use a headless Chromium surface instead, so their doctor report marks Xvfb as
not required while still checking that a supported browser is discoverable.

### Inspect Or Pin The Installer

To inspect the Unix installer before running it:

```bash
installer="$(mktemp)" &&
  curl -fsSL https://github.com/ChesterRa/cccc/releases/latest/download/install.sh -o "$installer" &&
  less "$installer" &&
  bash "$installer" &&
  rm -f "$installer"
```

To install a fixed version, download the installer from that exact release. A requested version never falls back to another release:

```bash
installer="$(mktemp)" &&
  curl -fsSL https://github.com/ChesterRa/cccc/releases/download/v0.4.32/install.sh -o "$installer" &&
  CCCC_VERSION=0.4.32 bash "$installer" &&
  rm -f "$installer"
```

PowerShell can use the same exact release asset:

```powershell
& {
  $ErrorActionPreference = "Stop"
  $env:CCCC_VERSION = "0.4.32"
  irm https://github.com/ChesterRa/cccc/releases/download/v0.4.32/install.ps1 | iex
}
```

The default install directory is `~/.local/bin` on macOS/Linux and `%LOCALAPPDATA%\CCCC\bin` on Windows. Override it with `CCCC_INSTALL_DIR`. The installer does not use `sudo`, does not modify the machine-level Windows `PATH`, and restores the previous executable if verification or replacement fails. If the CCCC daemon is running, it is stopped for the version switch and restarted afterward.

Supported release targets are Linux x86_64 with glibc, macOS x86_64, macOS arm64, and Windows x86_64. Linux musl/Alpine, Linux arm64, Windows arm64, and 32-bit systems are rejected before download because no matching release archive exists.

For manual installation, download the matching archive and `SHA256SUMS` from GitHub Releases, verify the archive hash, and place its `cccc` executable in a directory on `PATH`. Checksums detect corruption but do not replace release signing or platform code signing.

### Upgrade Or Remove

Run the same one-line installer to upgrade. It stages and verifies the new executable before switching versions. To remove a default Unix installation, stop the daemon and delete the installed file:

```bash
cccc daemon stop
rm ~/.local/bin/cccc
```

You may also remove the managed `# CCCC` PATH entry from `~/.zprofile` or `~/.bashrc`. On Windows, stop the daemon, remove `%LOCALAPPDATA%\CCCC\bin`, and remove that exact directory from your user-level `PATH`. CCCC data remains in `CCCC_HOME` unless you deliberately remove it separately.

## Build From Source

Requirements:

- Rust 1.88+
- Node.js 20+
- npm

```bash
git switch rust
cargo build --workspace --release --locked
export PATH="$PWD/target/release:$PATH"
```

The Cargo build automatically runs `npm ci` when Web dependencies are missing
and rebuilds the embedded Web UI when its sources change.

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
