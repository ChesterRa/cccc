# CCCC native product

CCCC coordinates coding agents as a persistent group with shared messages,
delivery tracking, runtime state, a Web UI, and MCP access.

## Recommended end-user installation

Use the website installer for the normal end-user path:

```bash
curl -fsSL https://chesterra.github.io/cccc/install.sh | sh
```

Or install the same native executable through pip:

```bash
python -m pip install -U cccc-pair
```

Windows CMD or PowerShell uses `powershell.exe -NoProfile -ExecutionPolicy Bypass -Command "[Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12; Invoke-RestMethod 'https://chesterra.github.io/cccc/install.ps1' | Invoke-Expression"`.
These installers download a checksum-verified GitHub Release binary and require
neither Rust nor Python. The native pip wheel contains the same executable and
no CCCC Python runtime. Do not install this crate with Cargo for normal product
use. Upgrade either supported distribution with:

```bash
cccc update
```

Use `cccc update --check` to inspect the detected installation and update source.
The installer refuses to overwrite an existing public `cccc` command without its
standalone ownership marker. Pip-owned installations remain updated through pip.
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
