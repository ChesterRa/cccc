# Releasing CCCC 0.4.x

This repo publishes the maintained Python compatibility package **`cccc-pair`**
(CLI command: **`cccc`**) and standalone Rust preview binaries on one version
line.

## What the release pipeline produces

The GitHub Actions workflow builds and uploads:

- Python source distribution, portable wheel, and four native platform wheels
- Native Rust archives for Linux x86-64, Intel/Apple Silicon macOS, and Windows x86-64
- Bundled Web UI assets (built from `web/` and packaged under `cccc/ports/web/dist/`)
- Embedded MCP server (`cccc mcp`) + protocol reference (`cccc_help`, sourced from `cccc/resources/cccc-help.md`)

Normal CI owns implementation and interoperability tests; release workflows do
not repeat those suites. Python publication performs one clean installation smoke
of the portable wheel, builds four Rust-backed platform wheels in parallel, and
publishes only after the source distribution and all five wheels form one verified
set. The standalone workflow builds the shared Web UI once, compiles and executes
each supported native binary once, then verifies the final Linux and Windows
installers in parallel before attaching the archives, checksums, and installers
to GitHub Releases.

## Tag ↔ Version conventions

The release workflows are tag-driven (`v*`) and enforce one normalized identity
across the tag, PEP 440 in `pyproject.toml`, SemVer in `Cargo.toml`, and
Cargo.lock. Automated native smokes confirm the built and installed versions.

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
4. Tag and push:
   - `git tag -a v0.4.0-rcN -m "v0.4.0-rcN"`
   - `git push --tags`
5. Confirm the bounded release gates pass before publication:
   - the Python source distribution, portable wheel, and four native wheels share one version,
   - all four native binaries execute `cccc --version` on their build hosts,
   - Linux and Windows install from the final release-candidate asset set,
   - the portable Python wheel installs without dependencies and runs `cccc version`,
   - no full source suite or cross-language interoperability suite is repeated.

## Installing an RC from TestPyPI

```bash
python -m pip install --pre \
  --index-url https://test.pypi.org/simple \
  --extra-index-url https://pypi.org/simple \
  cccc-pair==0.4.0rcN
```
