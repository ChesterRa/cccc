# Contributor Quality Gates

CCCC keeps the product implementation native while using small Python scripts
only for release packaging and repository-contract checks. Pull requests cover
source correctness; slower native-distribution checks run nightly and again on
the exact release artifacts.

## Local Commands

Run the impacted gate while developing:

```bash
scripts/quality_gate.sh fast
```

It selects checks from changed files:

- Rust changes run formatting plus focused Clippy/tests for the affected crates.
- Rust integration-test changes run their owning test binary; modules under
  `tests/suite/` map to the crate's `integration` target.
- Web changes run format, lint, and TypeScript checks.
- Release scripts, repository tests, workflows, and contract documentation run
  the small Python tooling suite. This suite does not import or launch a CCCC
  Python product implementation.

Local Cargo checks default to two build jobs to reduce memory pressure. Override
the bound on machines with more memory:

```bash
CCCC_CARGO_JOBS=4 scripts/quality_gate.sh fast
```

Inspect selection without running it:

```bash
scripts/pre_commit_checks.sh --dry-run
```

The impacted gate reports elapsed time against a 120-second feedback target.
Override the reporting threshold with `CCCC_PRECOMMIT_BUDGET_SECONDS`. A warning
does not hide a successful check.

Before handing off a broad change, run:

```bash
scripts/quality_gate.sh full
```

Full mode adds all Web tests, a production Web build, every release-tool and
repository-contract test, and the full Rust pre-commit scope.

Individual commands remain available:

```bash
npm -C web run check
npm -C web test
npm -C web run build
uvx ruff check scripts tests
uv run --no-project --with pytest --with pyyaml python -m pytest -q
```

## Web Toolchain

Install the locked Web dependencies and invoke tools through npm so they resolve
the project-local Vite+ binaries:

```bash
npm ci --prefix web
npm -C web run check
```

The native product embeds `web/dist`. `npm -C web run build` is sufficient before
rebuilding the Rust executable; `scripts/build_web.sh` and
`scripts/build_web.ps1` are convenience wrappers. `CCCC_WEB_DIST` remains the
explicit test override.

CI pins Node 24.19.0. `npm run check` runs Vite+ Oxfmt/Oxlint followed by the
independent TypeScript 5.9 `tsc --noEmit` check. Type-aware Vite+ checks remain
disabled until their diagnostics and supported scope match this project.

## Pull-Request Jobs

| Job | Responsibility |
| --- | --- |
| `quality` | Ruff plus release-tool, workflow, documentation, and packaging contract tests |
| `web` | Vite+ checks, TypeScript, all Web tests, and the production bundle |
| `package` | Deterministic native wheel/archive tooling and exact Rust-only wheel layout checks |
| `rust-linux` | Rust formatting, workspace Clippy/tests, Unix installer contracts, and serial combined-process lifecycle coverage |
| `windows-smoke` | Focused native Windows process-lifecycle checks |
| `ci-required` | Stable branch-protection aggregate; fails when any required job fails or is skipped |

The Rust job installs no Python product runtime. Its formatting, linting, tests,
and lifecycle checks reuse one checkout and Cargo target directory. Combined
daemon/Web process tests run last with one test thread to avoid lifecycle races.

Python in `quality` and `package` is a build/test interpreter only. There is no
Python daemon job, engine matrix, or cross-engine interoperability job.

## Nightly Native Verification

| Job | Responsibility |
| --- | --- |
| `web-bundle` | Build the exact frontend embedded by native artifacts |
| `rust-dist` | Release-build the Rust workspace and run Unix installation/replacement smoke |
| `windows-installer` | Build the native Windows CLI and verify installer ownership and PATH handling |

`rust-dist` runs `scripts/tests/smoke_rust_replacement.sh` against the built
executable in a fresh `CCCC_HOME`. It verifies daemon lifecycle, a scoped Web
Model actor, MCP initialization, a real `cccc_code_exec` cell, and clean stop.

Release verification repeats installation and same-version replacement against
the exact Linux, Intel macOS, Apple Silicon macOS, and Windows artifacts. Linux
artifacts must satisfy the manylinux 2.28 dependency boundary; macOS artifacts
declare macOS 11.0; Windows builds target Server 2022. Publication requires four
native wheels, four standalone archives, matching executable hashes, checksums,
version-bound installers, release notes, installed CLI/MCP/daemon/Web smoke, and
platform installer verification. Each wheel must also carry `pip-v1` in the
shared ownership marker path, replace any stale standalone marker on install,
refuse standalone self-update, and remove both its executable and marker on pip
uninstall.

The release workflow packages the same native executable bytes into standalone
archives and dependency-free platform wheels. It does not build an sdist,
universal wheel, importable CCCC Python package, fallback engine, or second Rust
registry distribution.

## Design Boundary

- Formatting, linting, type checks, tests, and builds validate the current tree.
- Historical migration verifiers do not become permanent product dependencies.
- File length is a review signal, not a proxy for architecture quality. Refactor
  when cohesion, ownership, testing, or change risk provides concrete evidence.
