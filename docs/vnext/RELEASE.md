# Releasing CCCC 0.4.x

This repo publishes the Python package **`cccc-pair`** (CLI command: **`cccc`**).

## RC19 Program

For `v0.4.0rc19`, release execution is governed by:

- `docs/release/RC19_RELEASE_BOARD.md`
- `docs/release/RC19_AUDIT_METHOD.md`
- `docs/release/rc19_file_matrix.csv` (generated)

## What the release pipeline produces

The GitHub Actions workflow builds and uploads:

- Python `sdist` + `wheel`
- Bundled Web UI assets (built from `web/` and packaged under `cccc/ports/web/dist/`)
- Embedded MCP server (`cccc mcp`) + help playbook (`cccc_help`, sourced from `cccc/resources/cccc-help.md`)

## Tag ↔ Version conventions

The release workflow is tag-driven (`v*`) and enforces that the git tag matches `pyproject.toml`’s version (PEP 440).

| Git tag | Upload target | Expected `pyproject.toml` version |
|--------|----------------|-----------------------------------|
| `v0.4.0` | PyPI | `0.4.0` |
| `v0.4.0-rcN` | TestPyPI | `0.4.0rcN` |
| `v0.4.0-alpha1` | TestPyPI | `0.4.0a1` |
| `v0.4.0-beta1` | TestPyPI | `0.4.0b1` |

## Maintainer checklist (local)

1. Bump `pyproject.toml` version.
2. Build + verify:
   - `python -m compileall -q src/cccc`
   - `python -m build`
   - `python -m twine check dist/*`
3. Smoke-test the wheel:
   - `python -m pip install --force-reinstall dist/*.whl`
   - `cccc version`
4. Tag and push:
   - `git tag -a v0.4.0-rcN -m "v0.4.0-rcN"`
   - `git push --tags`

## Installing an RC from TestPyPI

```bash
python -m pip install --index-url https://pypi.org/simple \
  --extra-index-url https://test.pypi.org/simple \
  cccc-pair==0.4.0rcN
```
