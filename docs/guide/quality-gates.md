# Contributor Quality Gates

CCCC keeps local feedback fast while preserving required source-correctness coverage on every pull request. Compatibility and native-distribution checks that do not need to block each change run nightly and before release. Long-lived gates check current code and deliverables; one-time migrations are reviewed once instead of becoming permanent historical machinery.

## Local Commands

Run the impacted fast gate while developing:

```bash
scripts/quality_gate.sh fast
```

It runs Ruff error-level rules and whitespace checks, then selects checks from the changed files:

- Rust source changes run workspace formatting plus lib/bin Clippy and unit tests for the directly changed crates.
- Changed Rust integration-test files run only their owning test binary. Modules under `tests/suite/` map to the crate's `integration` target.
- Root Cargo configuration, the lockfile, toolchain configuration, or bundled third-party crate changes use workspace lib/bin checks.
- Web and Python checks run only when their respective files changed.

Local Cargo checks default to two build jobs to avoid memory pressure from the workspace's large integration-test binaries. Override that bound when the machine has enough memory:

```bash
CCCC_CARGO_JOBS=4 scripts/quality_gate.sh fast
```

Inspect the selection without running checks:

```bash
scripts/pre_commit_checks.sh --dry-run
```

The impacted gate reports its wall-clock time against a 120-second local feedback budget. Override the reporting threshold with `CCCC_PRECOMMIT_BUDGET_SECONDS`; exceeding it warns but does not hide a successful check. A cold Rust dependency build may exceed the target, while normal warm-cache runs should stay within it.

The fast gate does not replace complete pull-request coverage. CI and `scripts/pre_commit_checks.sh --full` still run every Rust integration target.

Before handing off a broad change, run the full local gate:

```bash
scripts/quality_gate.sh full
```

Full mode adds all Web tests and the complete Python suite in one serial process. The serial path is intentionally equivalent to nightly coverage so parallel pull-request jobs do not hide order-dependent failures.

Individual commands remain available:

```bash
npm -C web run check
npm -C web test
npm -C web run typecheck
npm -C web run build
uvx ruff check src scripts tests
```

## Web Toolchain

Vite+ is a project-local development dependency. Install the locked Web dependencies, then run commands through npm so they resolve `web/node_modules/.bin/vp` automatically:

```bash
npm ci --prefix web
npm -C web run check
```

In a source checkout, both implementations use `web/dist`, so
`npm -C web run build` is sufficient before restarting the local Web process.
Use `scripts/build_web.sh` when preparing a Python package: it builds the same
frontend and also refreshes the packaged copy under
`src/cccc/ports/web/dist` that is embedded in wheels. `CCCC_WEB_DIST` remains
the explicit override for testing a different bundle.

CI pins Node 24.19.0 for reproducible formatting, linting, testing, and bundling, while `engines.node` defines the supported non-EOL local runtime range. The project deliberately does not use `devEngines`, because exact package-manager checks can prevent every `npm` and `npx` command from starting when a developer has a different compatible npm version.

Workflow JavaScript actions use their Node 24-compatible major versions.
Dependabot groups GitHub Actions updates into one weekly maintenance change so
runner-runtime deprecations do not accumulate as warnings across every job.

`npm run check` runs Vite+ Oxfmt and Oxlint, followed by the independent TypeScript 5.9 `tsc --noEmit` compatibility check. `npm run typecheck` remains available separately for focused diagnosis.

Vite+ 0.2.4 / tsgolint 0.24 does not yet replace this project's `tsc` gate. Enabling both `lint.options.typeAware` and `typeCheck` produced 105 errors and 454 warnings across 439 files, while `tsc --noEmit` passed. Type-aware Vite+ checks remain disabled until their scope and diagnostics match the project; CI keeps the evidence-backed `vp check && npm run typecheck` combination.

## Design Boundary

- Formatting, linting, type checks, tests, and builds validate the current tree on every pull request.
- A formatter migration may use a temporary verifier during review, but that verifier and its historical manifests do not become permanent product dependencies.
- File length is a review signal, not a hard CI proxy for architecture quality. Refactor when cohesion, ownership, testing, or change risk provides concrete evidence.

## Pull-Request Jobs

| Job | Responsibility |
| --- | --- |
| `quality` | Ruff and quality-tool/workflow contract tests |
| `web` | Vite+ Oxfmt/Oxlint check, independent TypeScript check, all Web tests, and the production bundle |
| `python-tests` | Source-level Python tests distributed across two deterministic matrix shards on Python 3.14 |
| `python-compat` | Import, CLI, and MCP handshake coverage on the oldest supported Python, 3.11 |
| `package` | Compile, build, Twine check, install, wheel resource smoke, and packaged Web bundle contract after quality/Web/Python pass |
| `rust-linux` | Rust formatting, workspace Clippy, Python-free tests, installer/release source contracts, and serial combined-process lifecycle coverage in one reused workspace |
| `interop` | Focused Python/Rust persisted-state and lock compatibility tests |
| `windows-smoke` | Windows PTY compatibility, combined Web startup-failure cleanup, and forced daemon Job Object process-tree cleanup |
| `ci-required` | Stable aggregate result for branch protection; fails when any required job fails or is skipped |

The `rust-linux` pull-request job is self-contained: it does not install or
execute the Python backend, and its formatting, linting, tests, and lifecycle
steps share one checkout, toolchain, and Cargo target directory. Cross-language
tests that launch `src/cccc` stay excluded from that job so its boundary remains
honest, but run in the separate mandatory `interop` job instead. CLI tests that
spawn and stop a combined daemon/Web process run last with one test thread. This
preserves the process-exit contract without paying for three separate Rust job
setups or racing the rest of the workspace test binary.

## Nightly Compatibility and Native Verification

Slow compatibility and native-distribution checks run once per day and remain
manually dispatchable. They do not repeat after every `main` or `rust` push;
release workflows independently verify the exact artifacts they publish.

| Job | Responsibility |
| --- | --- |
| `nightly-serial` | Complete source-level Python suite in one process on Python 3.11 |
| `python-compat` | Import, CLI, and MCP handshake coverage on Python 3.12 and 3.13 |
| `web-bundle` | Build the exact frontend embedded by native artifacts |
| `rust-dist` | Release-build the Rust workspace and run Unix installation/replacement smoke |
| `windows-installer` | Build the native Windows CLI and verify installer ownership and PATH handling |

`rust-dist` executes `scripts/tests/smoke_rust_replacement.sh` against the
actual built executable. The smoke uses a fresh `CCCC_HOME`, verifies offline
`status`, starts the daemon, creates a scoped Web Model actor, performs an MCP
handshake and a real `cccc_code_exec` cell, then stops the daemon and verifies
offline status again. The release-candidate verifiers repeat this check for
each installed Linux and macOS artifact; Windows verifies installed offline
status, MCP startup, daemon lifecycle, and that the executable is released
after shutdown.

Each final standalone verifier also starts the installed combined Web/daemon
process and reruns the exact same-version installer in place. Publication is
blocked unless the old Web process exits, the process lock and executable are
released, the replacement daemon restarts, and the installed MCP/code-mode
smoke still succeeds. This is the public lifecycle used by `cccc update`, not a
source-tree substitute.

Before packaging, Linux standalone builds run in the manylinux 2.28 container
already used by the native wheel and must pass an ELF dependency/ABI check;
OpenSSL is statically carried rather than delegated to the target host. macOS
builds declare and verify a macOS 11.0 minimum plus system-only dylib boundary.
Windows native-wheel and standalone build/verification runners are pinned to
Server 2022. These checks make the advertised platform floor a property of the
artifact rather than the moving `*-latest` runner labels.

The full Windows Rust workspace job is intentionally retired because it did not complete reliably on hosted runners. Windows keeps focused PTY compatibility coverage in `windows-smoke`. Python releases install and smoke one portable wheel plus a source distribution, build and install-smoke four native Rust wheels in parallel, and publish only after the exact version-matched set passes metadata, payload, MCP, daemon-lifecycle, and engine-switch checks. Standalone releases build the shared Web bundle once, execute each of the four native binaries once, and run the final Linux, Intel macOS, Apple Silicon macOS, and Windows installer verifiers in parallel before publication. These bounded release gates do not repeat the full Rust/Python suites, Web tests, or cross-language interoperability tests owned by normal CI. The Web job uploads its bundle and the package job consumes that artifact, so packaging tests the same bundle without rebuilding it. The `packaged_web_dist` pytest marker is reserved for assertions that require this artifact; source-only Python runs exclude it, while the package job executes it after downloading the bundle.

## Stable Python Shards

`scripts/quality/pytest_shards.py` discovers every `tests/**/test_*.py` and `tests/**/*_test.py` file. It sorts files by descending line-count weight and assigns each file to the currently lightest shard, with deterministic path and shard-index tie breakers.

This largest-processing-time strategy gives stable assignments for the same checkout, covers every file exactly once, and avoids the large imbalance of a simple hash bucket. It does not use `pytest-xdist`.

Inspect a shard with:

```bash
uv run python scripts/quality/pytest_shards.py --total 2 --index 0
```

## Nightly Serial Coverage

The scheduled `nightly-serial` job runs the complete source-level `tests/` suite in one pytest process, excluding only the artifact-dependent `packaged_web_dist` contract owned by the package job. Pull requests use two file shards for lower wall-clock time; nightly preserves a simple reference run that can expose shared-state or order sensitivity across files. The lightweight compatibility matrix covers Python 3.12 and 3.13 without repeating the full suite on every push.
