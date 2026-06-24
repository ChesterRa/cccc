# Vendored Source Provenance

- Upstream repository: `https://github.com/teng-lin/notebooklm-py`
- Upstream tag: `v0.7.2`
- Upstream commit: `915b5321e1c1f411e23bd8265517be8740749e56`
- Vendor date: `2026-06-24`
- License: MIT (see `LICENSE`)

## Scope

The upstream runtime modules from `src/notebooklm/` are vendored under:

- `src/cccc/providers/notebooklm/_vendor/notebooklm/`

CLI-only assets, package entrypoints, and agent/skill installer surfaces are
intentionally excluded to keep the vendored product surface small. The vendored
package `__init__.py` is also kept as a CCCC-local minimal wrapper so importing
the adapter does not perform broad upstream imports or logging setup.

The CCCC adapter should use a narrow boundary API from
`src/cccc/providers/notebooklm/adapter.py` and must not expose vendor internals
into daemon contracts.
