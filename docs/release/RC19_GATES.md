# RC19 Quality Gates

## Pre-merge Gates (required)

- Build succeeds for `web/` and Python package artifacts.
- Tests execute and pass in CI (no build-only green).
- No unresolved `P0` findings.
- Any new `P1` has owner and due date.

## Pre-tag Gates (`v0.4.0-rc19`)

- `P0 = 0` in findings register.
- Critical docs/implementation parity checks complete.
- Rehearsal commands pass:
  - package build and wheel install
  - `cccc version`
  - `cccc mcp` initialize + `tools/list`
  - core CLI smoke (`attach`, `group`, `actor`, `send`, `tail`)

## Gate Command Set (local)

```bash
npm ci --prefix web
npm -C web run build
python3 -m compileall -q src/cccc
python3 -m build
python3 -m twine check dist/*
```

If local Python deps are missing, run the same command set in CI/release runner and keep artifacts in the rehearsal report.

