# Releasing CCCC 0.4.x

This repo publishes one PyPI product, **`cccc-pair`** (CLI command: **`cccc`**),
with Python and Rust implementations on one version line.

## What the release pipeline produces

The GitHub Actions workflow builds and uploads:

- Python source distribution and universal fallback wheel
- Native wheels for manylinux 2.28 x86-64, Intel/Apple Silicon macOS, and Windows x86-64
- Bundled Web UI assets (built from `web/` and packaged under `cccc/ports/web/dist/`)
- Embedded MCP server (`cccc mcp`) + protocol reference (`cccc_help`, sourced from `cccc/resources/cccc-help.md`)

Platform jobs build, dependency-repair, install, and smoke the private Rust
payload. A dedicated interop job verifies shared persisted contracts. The
workflow collects the complete artifact set before one PyPI upload. The manual
standalone Rust workflow does not publish packages.

## Tag ↔ Version conventions

The release workflow is tag-driven (`v*`) and enforces one normalized identity
across the tag, PEP 440 in `pyproject.toml`, SemVer in `Cargo.toml`, Cargo.lock,
and each built Rust binary.

| Git tag | Upload target | Expected `pyproject.toml` version |
|--------|----------------|-----------------------------------|
| `v0.4.0` | PyPI | `0.4.0` |
| `v0.4.0-rcN` | TestPyPI | `0.4.0rcN` |
| `v0.4.0-alpha1` | TestPyPI | `0.4.0a1` |
| `v0.4.0-beta1` | TestPyPI | `0.4.0b1` |

## Maintainer checklist (local)

1. Bump `pyproject.toml`, `Cargo.toml`, internal dependency pins, and Cargo.lock together.
2. Build + verify:
   - `python -m compileall -q src/cccc`
   - `python -m build`
   - `python -m twine check dist/*`
3. Smoke-test the universal wheel locally; platform wheels are release-workflow gates:
   - `python -m pip install --force-reinstall dist/*.whl`
   - `cccc version`
4. Tag and push:
   - `git tag -a v0.4.0-rcN -m "v0.4.0-rcN"`
   - `git push --tags`

## Installing an RC from TestPyPI

```bash
python -m pip install --pre \
  --index-url https://test.pypi.org/simple \
  --extra-index-url https://pypi.org/simple \
  cccc-pair==0.4.0rcN
```
