# Support

## Before You Ask

Most questions are answered by these resources:

| Resource | What it covers |
|----------|---------------|
| `cccc doctor` | Environment check for Rust Home, runtimes, and daemon status |
| `cccc --help` | Complete CLI command reference |
| [Online docs](https://chesterra.github.io/cccc/) | Getting started, use cases, operations runbook, architecture |
| [FAQ](https://chesterra.github.io/cccc/guide/faq) | Common questions and troubleshooting |

## Bug Reports

Open a [GitHub Issue](https://github.com/ChesterRa/cccc/issues) and include:

- **Version**: output of `cccc version`
- **OS and CCCC version**: e.g., macOS 14.2 and `cccc version` output
- **Runtime**: which agent runtime(s) you're using (claude, codex, etc.)
- **Exact command**: the full command you ran
- **Actual output**: copy-paste the error or unexpected behavior
- **Expected behavior**: what you expected to happen
- **Reproduction steps**: minimal steps to trigger the issue

If the issue involves the daemon, include `cccc doctor`, `cccc daemon status`, and the relevant Rust tracing output.

## Feature Requests

Open a [GitHub Issue](https://github.com/ChesterRa/cccc/issues) with:

- **Problem statement**: what workflow is difficult or impossible today
- **Proposed behavior**: how you'd like it to work
- **Operational impact**: how this affects your multi-agent setup

## Security Issues

Please report security vulnerabilities privately. See [SECURITY.md](SECURITY.md) for instructions.

## Operational Notes

- CCCC is **local-first**. Rust runtime state lives under `CCCC_RUST_HOME` (default `~/.cccc-rust`), not in your repository.
- The daemon is the single source of truth. If something looks wrong, check `cccc daemon status` first.
- For recovery procedures, see the [Operations Runbook](https://chesterra.github.io/cccc/guide/operations).
