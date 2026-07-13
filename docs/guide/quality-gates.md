# Contributor Quality Gates

CCCC keeps local feedback fast while preserving full pull-request coverage. The same small tools enforce source-size and Python test-shard policy locally and in GitHub Actions.

## Local Commands

Run the default fast gate while developing:

```bash
scripts/quality_gate.sh fast
```

It checks the source-size ratchet, Ruff error-level rules, whitespace, Web lint/typecheck when Web files changed, Python syntax, and impacted Python tests. It does not run the full Python suite.

Before handing off a broad change, run the full local gate:

```bash
scripts/quality_gate.sh full
```

The full mode adds all Web tests and the complete Python suite in one serial process. The serial path is intentionally equivalent to the nightly coverage check; local or CI parallelism must not hide order-dependent failures.

Individual commands remain available:

```bash
npm -C web run check
npm -C web test
npm -C web run typecheck
npm -C web run build
uvx ruff check src scripts tests
uv run python scripts/quality/source_size.py
```

Vite+ is a project-local development dependency. Install the locked Web dependencies, then run all commands through npm so they resolve `web/node_modules/.bin/vp` automatically:

```bash
npm ci --prefix web
npm -C web run check
```

CI pins Node 20.19.5 for reproducible formatting, linting, testing, and bundling, while `engines.node` defines the supported local runtime range. The Web project deliberately does not use `devEngines`, because exact package-manager checks can prevent every `npm` and `npx` command from starting when a developer has a different compatible npm version.

`npm run check` is the stable composite gate. It runs Vite+ Oxfmt and Oxlint first, then the independent TypeScript 5.9 `tsc --noEmit` compatibility check. `npm run typecheck` remains available separately for focused diagnosis, but CI calls only the composite script.

Vite+ 0.2.4 / tsgolint 0.24 does not yet replace this project's `tsc` gate. Enabling both `lint.options.typeAware` and `typeCheck` produced 105 errors and 454 warnings across 439 files; limiting the command to `src` still produced 49 errors and 434 warnings, while `tsc --noEmit` passed. The first unbounded diagnostic print also triggered a Vite+ stdout panic. Enabling `typeAware` alone still produced 13 errors and 454 warnings, while `typeCheck` alone is rejected by the Vite+ schema. Until the tool follows this project's TypeScript scope and diagnostics equivalently, do not enable those options or make tsgolint a CI requirement; keep the evidence-backed `vp check && npm run typecheck` fallback.

## Source-Size Ratchet

The gate covers production files under `src/cccc/**/*.py` and `web/src/**/*.{ts,tsx}`. Tests, vendored code, generated code, and build output are excluded.

- New production files must contain at most 300 lines.
- Existing oversized files are recorded in `scripts/quality/source-size-baseline.json`.
- An oversized file may not grow beyond its recorded value.
- When an oversized file shrinks, its baseline must shrink in the same change.
- Once a file reaches 300 lines, remove its baseline entry.
- Pull requests compare the baseline with the merge base, so adding or raising an allowance fails even if the current file matches it.

Base resolution is mandatory. Pull requests use the target base SHA, pushes use the event's `before` SHA, and local runs automatically use the current branch upstream or `origin/main`/`origin/master`. Override local resolution with `--base-ref <commit>` or `SOURCE_SIZE_BASE_REF=<commit>`. If no trusted Git history exists, the gate fails instead of silently skipping the comparison.

For the first push to an empty repository, where GitHub supplies an all-zero `before` SHA, CI uses the explicit `--bootstrap-baseline` mode. That mode is a visible trust decision for creating the first baseline; normal PR, push, and local runs never use it.

After a legitimate reduction, regenerate the baseline and inspect the diff:

```bash
uv run python scripts/quality/source_size.py --write-baseline
git diff -- scripts/quality/source-size-baseline.json
uv run python scripts/quality/source_size.py
```

Never regenerate the baseline to permit growth.

### One-time Oxfmt migration

The Oxfmt 0.57.0 rollout has one versioned exception manifest at `scripts/quality/oxfmt-migration-v1.json`. It is not a reusable line allowance. Each entry records the trusted base blob OID, the SHA-256 and Python `splitlines()` count of the formatted bytes, and the corresponding base count. A baseline increase is accepted only when all recorded values and the current file match exactly.

Run the verifier after installing Web dependencies:

```bash
node scripts/quality/verify_oxfmt_migration.mjs
```

The verifier extracts the same trusted Git base used by the source-size gate, formats its `web/src` tree once with the lockfile's Oxfmt 0.57.0 binary, and rejects semantic edits, incomplete manifests, hash or line drift, and non-exact baseline values. Local quality gates and the CI Web job run it unconditionally. Once v1 exists in the trusted base, its bytes are immutable; a future formatter upgrade must use a separately reviewed v2 manifest.

Six pre-migration semantic changes are separately frozen in `scripts/quality/preexisting-reviewed-v1.json`: `AgentTab.tsx`, `ContextModal/index.tsx`, `ProjectedBrowserSurfacePanel.tsx`, `ActorConfigModal.tsx`, `GuidanceTab.tsx`, and `IMBridgeTab.tsx`. Their review trail includes T557 (AgentTab runtime-option snapshot), T560 (noVNC package-root import), T564/T565/T566 (the five React key fixes and independent reviews), plus the final-tree Context and actor-config UI fixes. The manifest requires the exact trusted-base blob and line count, current SHA-256 and line count, and old/new baseline for all six paths; it is disjoint from the formatter manifest and becomes byte-immutable after landing. `ChatComposer.tsx` is deliberately excluded because its user-requested focus-border change already grew beyond the old baseline before this closeout.

## Pull-Request Jobs

| Job | Responsibility |
| --- | --- |
| `quality` | Source-size ratchet, Ruff, and quality-tool/workflow contract tests |
| `web` | Vite+ Oxfmt/Oxlint composite check, TypeScript compatibility check, all Vite+ tests, and the production bundle |
| `python-tests` | All Python test files distributed across four deterministic matrix shards |
| `package` | Compile, build, Twine check, install, and wheel resource smoke test after quality/Web/Python pass |
| `windows-smoke` | Windows PTY compatibility tests |

The Web job uploads its bundle and the package job consumes that artifact, so packaging tests the same bundle without rebuilding it.

## Stable Python Shards

`scripts/quality/pytest_shards.py` discovers every `tests/**/test_*.py` and `tests/**/*_test.py` file. It sorts files by descending line-count weight and assigns each file to the currently lightest shard, with deterministic path and shard-index tie breakers.

This largest-processing-time strategy gives stable assignments for the same checkout, covers every file exactly once, and avoids the large imbalance of a simple hash bucket. It does not use `pytest-xdist`.

Inspect a shard with:

```bash
uv run python scripts/quality/pytest_shards.py --total 4 --index 0
```

## Nightly Serial Coverage

The scheduled `nightly-serial` job runs the complete `tests/` suite in one pytest process. Pull requests use the four file shards for lower wall-clock time; nightly preserves a simple reference run that can expose shared-state or order sensitivity across files.
