# Contributor Quality Gates

CCCC keeps local feedback fast while preserving full pull-request coverage. Long-lived gates check current code and deliverables; one-time migrations are reviewed once instead of becoming permanent historical machinery.

## Local Commands

Run the impacted fast gate while developing:

```bash
scripts/quality_gate.sh fast
```

It runs Ruff error-level rules, whitespace checks, Web checks when Web files changed, Python syntax checks, and impacted Python tests. It does not run the complete test suites.

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

CI pins Node 20.19.5 for reproducible formatting, linting, testing, and bundling, while `engines.node` defines the supported local runtime range. The project deliberately does not use `devEngines`, because exact package-manager checks can prevent every `npm` and `npx` command from starting when a developer has a different compatible npm version.

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
| `python-tests` | Source-level Python tests distributed across four deterministic matrix shards |
| `package` | Compile, build, Twine check, install, wheel resource smoke, and packaged Web bundle contract after quality/Web/Python pass |
| `windows-smoke` | Windows PTY compatibility tests |

The Web job uploads its bundle and the package job consumes that artifact, so packaging tests the same bundle without rebuilding it. The `packaged_web_dist` pytest marker is reserved for assertions that require this artifact; source-only Python runs exclude it, while the package job executes it after downloading the bundle.

## Stable Python Shards

`scripts/quality/pytest_shards.py` discovers every `tests/**/test_*.py` and `tests/**/*_test.py` file. It sorts files by descending line-count weight and assigns each file to the currently lightest shard, with deterministic path and shard-index tie breakers.

This largest-processing-time strategy gives stable assignments for the same checkout, covers every file exactly once, and avoids the large imbalance of a simple hash bucket. It does not use `pytest-xdist`.

Inspect a shard with:

```bash
uv run python scripts/quality/pytest_shards.py --total 4 --index 0
```

## Nightly Serial Coverage

The scheduled `nightly-serial` job runs the complete source-level `tests/` suite in one pytest process, excluding only the artifact-dependent `packaged_web_dist` contract owned by the package job. Pull requests use the four file shards for lower wall-clock time; nightly preserves a simple reference run that can expose shared-state or order sensitivity across files.
