# CCCC (Rust implementation)

CCCC coordinates coding agents as a persistent group with shared messages,
delivery tracking, runtime state, a Web UI, and MCP access.

## End-user installation

```bash
curl -fsSL https://chesterra.github.io/cccc/install.sh | sh
```

Windows PowerShell uses `irm https://chesterra.github.io/cccc/install.ps1 | iex`.
These installers download a checksum-verified GitHub Release binary and require
neither Rust nor Python. Upgrade it through the same channel with:

```bash
cccc update
```

Use `cccc update --check` to inspect the detected installation and update source.
The maintained PyPI platform wheel still owns its public Python launcher and
bundles this binary privately when supported; that private payload remains updated
as part of the complete pip product. Do not install this crate with Cargo for
normal product use.

## Workspace development

CCCC requires Rust 1.88 or newer. From the repository, run and test this
implementation directly with:

```bash
cargo run -p cccc -- --version
cargo test -p cccc --locked
```

Web Model and NotebookLM browser projection require a locally installed Chrome,
Chromium, or Microsoft Edge browser. The core CLI, daemon, MCP server, and Web UI
do not require a browser.

Internal implementation crates use the `cccc-pair-*` namespace and are not
intended to be installed directly. Manual standalone workflow runs verify release
candidates; release tags also attach the verified native assets to GitHub Releases.

Project documentation and source: <https://github.com/chesterra/cccc>

License: Apache-2.0
