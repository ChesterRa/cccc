# Contributing to CCCC

Contributions are welcome — bug reports, feature requests, code, and documentation.
This page consolidates the conventions that are otherwise spread across the
README, [SUPPORT.md](SUPPORT.md), [SECURITY.md](SECURITY.md), and
[docs/guide/quality-gates.md](docs/guide/quality-gates.md).

## Ways to Contribute

- **Bug reports**: search [existing issues](https://github.com/ChesterRa/cccc/issues)
  first, then open a new one. Include `cccc --version`, your OS, the exact
  commands, and minimal reproduction steps. See [SUPPORT.md](SUPPORT.md) for the
  full checklist and daemon-log guidance.
- **Feature requests**: describe the problem, the proposed behavior, and the
  operational impact on your multi-agent setup.
- **Pull requests**: see the process below.
- **Security issues**: report privately per [SECURITY.md](SECURITY.md). Never
  open a public issue for a vulnerability.

Runtime state lives under `CCCC_HOME` (default `~/.cccc/`) — never commit it,
and do not paste tokens, credentials, or provider transcripts into issues.

## Development Environment

Source builds require:

- Rust 1.88 (pinned in `rust-toolchain.toml`; `rustup` installs it automatically)
- Node.js 24 with npm (CI pins 24.19.0)
- Python 3.11+ — for release packaging and repository-contract checks only;
  the product itself contains no Python implementation

Build and run from source:

```bash
npm ci --prefix web
npm -C web run build
cargo run --locked --features standalone -p cccc --bin cccc -- --port 0
```

The frontend bundle is embedded into the Rust executable. After web-only
changes, rebuild the executable (or rerun `npm -C web run build` before a Cargo
build) to see them in the native binary.

## Project Layout

| Path | Contents |
| --- | --- |
| `crates/` | Rust workspace: contracts, core, runtime, client, daemon, web, MCP, CLI (see [docs/reference/architecture.md](docs/reference/architecture.md)) |
| `web/` | React + TypeScript frontend (embedded into the binary) |
| `docs/` | VitePress documentation site |
| `scripts/`, `tests/` | Release packaging and repository-contract checks (Python) |

## Quality Gates

Run the checks selected by your changed files while developing:

```bash
scripts/quality_gate.sh fast
```

Inspect the selection without running it:

```bash
scripts/pre_commit_checks.sh --dry-run
```

Before handing off a broad change, run:

```bash
scripts/quality_gate.sh full
```

Useful individual commands:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
npm -C web run check
npm -C web test
npm -C web run build
uvx ruff check scripts tests
uv run --no-project --with pytest --with pyyaml python -m pytest -q
```

Notes:

- Combined daemon/Web process-lifecycle tests run with a single test thread to
  avoid races; CI already does this, and `cargo test --workspace` locally
  follows the same expectation for those binaries.
- Local Cargo checks default to two build jobs to limit memory pressure.
  Override with `CCCC_CARGO_JOBS=4` on larger machines.
- Changes to account linkage, embedded workbenches, or Group connections must
  also pass `python3 scripts/check_connect_browser.py`. See
  [docs/guide/quality-gates.md](docs/guide/quality-gates.md) for its setup.
- Repository-contract tests in `tests/` validate docs against code (for
  example, the MCP architecture surface). Editing the MCP tool list, IPC
  standards, or workflow contracts usually requires updating the matching
  contract test in the same PR.

## Code Constraints

- `unsafe_code` is forbidden workspace-wide.
- Clippy denies `unwrap_used`, `todo!`, and `dbg_macro` in product code.
- File length is a review signal, not a goal in itself. Refactor when cohesion,
  ownership, testing, or change risk provides concrete evidence.

## Commit Messages

Use [Conventional Commits](https://www.conventionalcommits.org/):
`fix:`, `feat:`, `docs:`, `test:`, `perf:`, `refactor:`, `chore:`, optionally
with a scope such as `fix(web):` or `fix(im):`. Commit messages in English or
Chinese are both accepted — keep the subject line short and specific.

## Pull Request Process

1. Fork the repository and create a topic branch.
2. Make the change, including tests and documentation updates where relevant.
3. Run the impacted quality gates (at minimum
   `scripts/quality_gate.sh fast`).
4. Open the pull request with a clear description: what changed, why, and how
   you verified it.
5. Keep PRs focused — one logical change per PR makes review faster.

CI runs these jobs on every PR:

| Job | Responsibility |
| --- | --- |
| `quality` | Ruff plus release-tool, workflow, documentation, and packaging contract tests |
| `web` | Frontend checks, TypeScript, all web tests, and the production bundle |
| `package` | Native wheel/archive tooling and wheel layout checks |
| `rust-linux` | Rust formatting, Clippy, workspace tests, Unix installer contracts |
| `windows-smoke` | Native Windows process-lifecycle checks |
| `ci-required` | Aggregate gate; fails when any required job fails or is skipped |

Slower native-distribution checks run nightly and again on release artifacts;
PRs only need to cover source correctness.

## Documentation

The docs site lives in `docs/` (VitePress). Preview locally:

```bash
npm ci --prefix docs
npm run dev --prefix docs
```

New pages need an entry in `docs/.vitepress/config.ts` to appear in navigation.

## Community

- Telegram: [t.me/ccccpair](https://t.me/ccccpair)
- Questions and troubleshooting: see [SUPPORT.md](SUPPORT.md)

## License

By contributing, you agree that your contributions are licensed under the
[Apache-2.0 License](LICENSE).
