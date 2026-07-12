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
npm -C web test
npm -C web run lint
npm -C web run typecheck
uvx ruff check src scripts tests
uv run python scripts/quality/source_size.py
```

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

## Pull-Request Jobs

| Job | Responsibility |
| --- | --- |
| `quality` | Source-size ratchet, Ruff, and quality-tool/workflow contract tests |
| `web` | ESLint with zero warnings, TypeScript, all Vitest tests, and the production bundle |
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
