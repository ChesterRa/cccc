# Releasing CCCC 0.4.x

This repo publishes the maintained Python compatibility package **`cccc-pair`**
(CLI command: **`cccc`**) and standalone Rust preview binaries on one version
line.

## What the release pipeline produces

The GitHub Actions workflow builds and uploads:

- Python source distribution and portable Python wheel
- Native Rust archives for Linux x86-64, Intel/Apple Silicon macOS, and Windows x86-64
- Bundled Web UI assets (built from `web/` and packaged under `cccc/ports/web/dist/`)
- Embedded MCP server (`cccc mcp`) + protocol reference (`cccc_help`, sourced from `cccc/resources/cccc-help.md`)

Normal CI owns implementation and interoperability tests. Python publication does
not compile Rust. The standalone workflow builds the shared Web UI once, compiles
each supported native binary once, and attaches the archives, checksums, and
installers to GitHub Releases. Final native installation smoke checks are manual.

## Tag ↔ Version conventions

The release workflows are tag-driven (`v*`) and enforce one normalized identity
across the tag, PEP 440 in `pyproject.toml`, SemVer in `Cargo.toml`, and
Cargo.lock. The manual native-install check confirms the built binary version.

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
3. Smoke-test the portable Python wheel locally:
   - `python -m pip install --force-reinstall dist/*.whl`
   - `cccc version`
4. After GitHub assets are published, manually test a native installation:
   - macOS/Linux: install into a temporary `CCCC_INSTALL_DIR` with
     `CCCC_NO_MODIFY_PATH=1`, then run `cccc --version` and
     `cccc update --check`.
   - Windows: install into a temporary directory with `install.ps1`, then run
     `cccc.exe --version` and `cccc.exe update --check`.
   - Confirm the release contains four archives, `SHA256SUMS`, `install.sh`, and
     `install.ps1`.
5. Tag and push:
   - `git tag -a v0.4.0-rcN -m "v0.4.0-rcN"`
   - `git push --tags`

## Installing an RC from TestPyPI

```bash
python -m pip install --pre \
  --index-url https://test.pypi.org/simple \
  --extra-index-url https://pypi.org/simple \
  cccc-pair==0.4.0rcN
```
