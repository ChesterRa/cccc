# RC19 Release Rehearsal Report (R8)

Status: local rehearsal complete (fresh-runner rehearsal refreshed on 2026-02-12; final publish rehearsal still requires tag pipeline run).

## Checklist

- [x] Build web assets
- [x] Build wheel/sdist
- [x] Install wheel in clean env
- [x] `cccc version`
- [x] `cccc mcp` initialize + `tools/list`
- [x] Core CLI smoke
- [x] Go/No-Go decision

## Results

| Item | Result | Notes |
|------|--------|-------|
| `python3 -m compileall -q src/cccc` | pass | Syntax-level preflight passed for core runtime |
| `npm --prefix web run typecheck` | pass | Strict TS gate passes (`tsc --noEmit -p tsconfig.json`) |
| `npm --prefix web run lint` | pass | ESLint pass on `src` (`.ts/.tsx`) |
| `npm --prefix web run build` | pass | Web bundle generated successfully (`src/cccc/ports/web/dist`) |
| `rg ",A,.*?,pending," docs/release/rc19_file_matrix.csv` | pass | No remaining A-tier pending items after deep web pass |
| `rg ",B,.*?,pending," docs/release/rc19_file_matrix.csv` | pass | No remaining B-tier pending items after docs/ops sweep |
| `npm --prefix docs run build` | pass | Release docs build green with RC19 section wired |
| `.venv/bin/python -m pytest -q` | pass | `80 passed` (2026-02-12) |
| `.venv/bin/python -m pytest -q tests/test_group_automation_baseline.py tests/test_automation_rules_constraints.py` | pass | baseline + one-time schedule constraints regression guard (`5 passed`) |
| clean venv packaging rehearsal (`/tmp/cccc-rc19-rehearsal.*`) | pass | `python -m build` + `twine check` + wheel install all green |
| wheel install + `cccc version` | pass | reports pre-bump package version in clean venv (`0.4.0rc18` at rehearsal time) |
| `cccc mcp` initialize + `tools/list` | pass | handshake OK, tool count 49 |
| CLI smoke (`daemon/attach/actor/send/tail`) | pass | smoke group `g_f512f14bf57d` |

## Decision

- Go/No-Go: Go (local rehearsal)
- Date: 2026-02-11
- Owner: `releng` + `core-platform`
- Remaining before publish:
  - run tag-triggered release workflow once (`v0.4.0-rc19`) as final publish rehearsal
