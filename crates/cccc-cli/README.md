# CCCC (Rust implementation)

CCCC coordinates coding agents as a persistent group with shared messages,
delivery tracking, runtime state, a Web UI, and MCP access.

## End-user installation

```bash
pip install -U cccc-pair
cccc rust
```

The PyPI platform wheel owns the public `cccc` launcher and bundles this Rust
binary privately when supported. Upgrade the launcher, Python implementation,
Rust payload, Web assets, and contracts together with:

```bash
cccc update
```

Use `cccc update --check` to inspect the PyPI channel and command without changing
the installation. Do not install this crate separately for normal product use;
that would create a second `cccc` on `PATH` and bypass coordinated updates.

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
intended to be installed directly. The manual standalone-candidate workflow is
for engineering diagnostics; release tags publish the unified PyPI product.

Project documentation and source: <https://github.com/chesterra/cccc>

License: Apache-2.0
