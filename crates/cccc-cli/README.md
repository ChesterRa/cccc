# CCCC (Rust)

CCCC coordinates coding agents as a persistent group with shared messages,
delivery tracking, runtime state, a Web UI, and MCP access.

## Install

```bash
cargo install cccc --locked
```

Upgrade an existing crates.io installation with:

```bash
cccc update
```

Use `cccc update --check` to print the current and latest package versions plus
the selected update source without changing the installation. Cargo installs
update through crates.io. Prebuilt installs use the tagged GitHub Rust installer
and verify release assets with `SHA256SUMS`. After replacement succeeds, CCCC
stops any older daemon so the next command starts the updated binary.

CCCC requires Rust 1.88 or newer to install from source. After installation,
start the local daemon and Web UI with:

```bash
cccc
```

Then open <http://127.0.0.1:8848>.

Web Model and NotebookLM browser projection require a locally installed Chrome,
Chromium, or Microsoft Edge browser. The core CLI, daemon, MCP server, and Web UI
do not require a browser.

The public command and package are named `cccc`. Internal implementation crates
use the `cccc-pair-*` namespace and are not intended to be installed directly.

Project documentation and source: <https://github.com/chesterra/cccc>

License: Apache-2.0
