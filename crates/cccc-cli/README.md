# CCCC (Rust implementation)

CCCC coordinates coding agents as a persistent group with shared messages,
delivery tracking, runtime state, a Web UI, and MCP access.

## Recommended end-user installation

Install the complete CCCC product from PyPI:

```bash
python -m pip install -U cccc-pair
```

Supported platform wheels bundle this Rust executable privately behind the
public Python launcher, so users can switch with `cccc rust` while retaining the
Python default and fallback. Do not install this crate with Cargo for normal
product use.

## Experimental standalone Rust preview

```bash
curl -fsSL https://chesterra.github.io/cccc/install.sh | sh
```

Windows CMD or PowerShell uses `powershell.exe -NoProfile -ExecutionPolicy Bypass -Command "[Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12; Invoke-RestMethod 'https://chesterra.github.io/cccc/install.ps1' | Invoke-Expression"`.
These installers download a checksum-verified GitHub Release binary and require
neither Rust nor Python. This Rust-only channel has no Python fallback or
implementation switching and is not the recommended replacement for the pip
product. Upgrade it through the same channel with:

```bash
cccc update
```

Use `cccc update --check` to inspect the detected installation and update source.
The installer refuses to overwrite an existing public `cccc` command without its
standalone ownership marker. The private wheel payload remains updated only as
part of the complete pip product.
Commands in other directories are preserved. The default installer moves its
directory to the front of the user PATH and reports remaining duplicates; verify
the selected command in a new terminal with `cccc doctor`.

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
candidates. When explicitly run on a matching release tag, that workflow can also
attach the verified experimental preview assets to GitHub Releases.

Project documentation and source: <https://github.com/chesterra/cccc>

License: Apache-2.0
