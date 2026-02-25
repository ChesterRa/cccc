# Vendored Source Provenance

- Upstream repository: `https://github.com/teng-lin/notebooklm-py`
- Upstream commit: `9eb13cea51af9dad53a66c3c467b3bbb78f72b0e`
- Vendor date: `2026-02-22`
- License: MIT (see `LICENSE`)

## Scope

The upstream runtime modules from `src/notebooklm/` are vendored under:

- `src/cccc/providers/notebooklm/_vendor/notebooklm/`

CLI-only assets were intentionally excluded to keep the vendored surface small.
The CCCC adapter should use a narrow boundary API from
`src/cccc/providers/notebooklm/adapter.py` and must not expose vendor internals
into daemon contracts.
