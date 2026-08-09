# Vendored Source Provenance

- Upstream repository: `https://github.com/teng-lin/notebooklm-py`
- Upstream tag: `v0.8.0`
- Upstream commit: `8fb61cb125be9f59dfe163561e355922967c604a`
- Vendor date: `2026-08-08`
- License: MIT (see `LICENSE`)

## Scope

The upstream runtime modules from `src/notebooklm/` are vendored under:

- `src/cccc/providers/notebooklm/_vendor/notebooklm/`

Upstream application/transport surfaces (`_app/`, `cli/`, `mcp/`, `server/`,
`__main__.py`, `notebooklm_cli.py`, and `_serving.py`) are intentionally
excluded. CCCC owns those responsibilities through its daemon, IPC, Web, and
MCP contracts; vendoring them would duplicate product behavior and pull in
optional dependencies that the provider adapter does not use.

The vendored package `__init__.py` is kept as a CCCC-local minimal wrapper so
importing the adapter does not perform broad upstream imports or logging setup.
No upstream runtime source patches are applied for this revision.

The CCCC adapter should use a narrow boundary API from
`src/cccc/providers/notebooklm/adapter.py` and must not expose vendor internals
into daemon contracts.
