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

Build and run from the repository root (Bash):

```bash
npm ci --prefix web
npm -C web run build
CCCC_HOME="$HOME/.cccc-dev" cargo run --locked --features standalone -p cccc --bin cccc -- --port 0
```

Use a dedicated `CCCC_HOME` outside the checkout for development; `--port 0`
selects a free port but does not isolate runtime data from your daily instance.
In PowerShell, set `$env:CCCC_HOME = Join-Path $HOME '.cccc-dev'` before running
these commands, and omit the Bash assignment before `cargo run`.

Release executables embed the frontend bundle, so rebuild the executable after
Web changes. Debug/source-run builds serve `web/dist` from disk; rebuild the Web
bundle and reload the page for frontend-only changes. See the
[Web toolchain guide](docs/guide/quality-gates.md#web-toolchain) for details.

## Project Layout

| Path | Contents |
| --- | --- |
| `crates/` | Rust workspace: contracts, core, runtime, client, daemon, web, MCP, CLI (see [docs/reference/architecture.md](docs/reference/architecture.md)) |
| `web/` | React + TypeScript frontend (embedded into the binary) |
| `docs/` | VitePress documentation site |
| `scripts/`, `tests/` | Release packaging and repository-contract checks (Python) |

## Quality Gates

The shell gates require Bash and [uv](https://docs.astral.sh/uv/getting-started/installation/),
which provides the `uv` and `uvx` commands used for Python tooling.

Run the checks selected by your changed files while developing:

```bash
scripts/quality_gate.sh fast
```

Inspect the selection without running it:

```bash
scripts/pre_commit_checks.sh --dry-run
```

Before handing off a broad change, run the full gate:

```bash
scripts/quality_gate.sh full
```

Useful individual commands:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked -- --test-threads=1
npm -C web run check
npm -C web test
npm -C web run build
uvx ruff check scripts tests
uv run --no-project --with pytest --with pyyaml python -m pytest -q
```

Notes:

- Process-lifecycle tests need explicit serialization. The command above runs
  all Rust tests serially for simplicity; CI separates the affected tests.
  Plain `cargo test --workspace` does not inherit CI's serial settings.
- Cargo checks invoked by the quality-gate scripts default to two build jobs.
  Override with `CCCC_CARGO_JOBS=4 scripts/quality_gate.sh fast` on larger
  machines. Direct Cargo commands use Cargo's own `--jobs` setting.
- Repository-contract tests in `tests/` validate docs against code (for
  example, the MCP architecture surface). Editing the MCP tool list, IPC
  standards, or workflow contracts usually requires updating the matching
  contract test in the same PR.

## Code Constraints

- `unsafe_code` is forbidden workspace-wide.
- Clippy denies `unwrap_used`, `todo!`, and `dbg_macro` in product code.
- File length is a review signal, not a goal in itself. Refactor when cohesion,
  ownership, testing, or change risk provides concrete evidence.

## Current Integration Contracts

When working on the areas changed in v0.4.44, preserve these boundaries:

- **Repository instructions:** use `AGENTS.md` through the runtime's native
  loading rules. CCCC does not inject it into prompts or reload sessions on save.
  The Web editor uses `/api/v1/groups/{group_id}/agents_md` and sends the displayed
  `expected_path` when saving. The old `/project_md` endpoint,
  `cccc_project_info` tool and PROJECT.md bootstrap metadata have been removed;
  do not reintroduce them in integrations. Existing files are left untouched.
  See [Architecture](docs/reference/architecture.md#mcp-server).
- **Async Web operations:** bind reads and UI updates to their originating
  Group, editor and request. Accepted writes may finish after the view changes;
  stale responses must not overwrite another scope or an unsaved draft.
  Invalidate affected read caches around mutations. Cover delayed reads,
  read-after-write ordering, navigation and failed saves in regression tests.
- **Voice:** the speaking service (Codex or Grok) and the Voice Analyst runtime
  are independent choices. Keep long-lived credentials on the host, preserve
  request context and language at handoff, and test interrupted or superseded audio
  without replaying Analyst work. See the
  [Voice guide](docs/guide/voice-secretary.md#live-voice-provider-connections).
- **Direct connections:** closed records leave the panel without a separate
  Remove UI. Cleanup uses the existing guarded daemon operation; offline and
  uncertain connections remain visible. Preserve queued messages' original
  routing and stop starting cleanup requests after the panel unmounts. See
  [Connect](docs/guide/connect.md) and `web/src/features/connect/` tests.

These notes supplement the API and architecture references; the
[v0.4.44 release notes](docs/release/v0.4.44_release_notes.md) describe the full
upgrade impact, including changes to repository instruction integrations.

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

Required CI jobs and their responsibilities are maintained in
[Contributor Quality Gates](docs/guide/quality-gates.md#pull-request-jobs).
New contributors may need a maintainer to approve the first workflow run.
Slower native-distribution checks run nightly and again on release artifacts.

## Documentation

The docs site lives in `docs/` (VitePress). Preview locally:

```bash
npm ci --prefix docs
npm run dev --prefix docs
```

New pages need an entry in `docs/.vitepress/config.ts` to appear in navigation.
When changing CLI quick-start commands or behavior notes, update the
[English](docs/guide/getting-started/cli.md),
[Chinese](docs/guide/getting-started/cli-zh.md), and
[Japanese](docs/guide/getting-started/cli-ja.md) guides together.

## Community

- Telegram: [t.me/ccccpair](https://t.me/ccccpair)
- Questions and troubleshooting: see [SUPPORT.md](SUPPORT.md)

## License

By contributing, you agree that your contributions are licensed under the
[Apache-2.0 License](LICENSE).
