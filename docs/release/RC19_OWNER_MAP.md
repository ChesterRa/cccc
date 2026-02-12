# RC19 Owner Map (Domain-Level)

This map assigns **default owner groups** by domain so `rc19_file_matrix.csv` has no unowned `Tier A` rows.

## Owner Groups

- `core-platform`: contracts, kernel, daemon, runners, core utilities
- `mcp-surface`: MCP server tools, schemas, help/tool semantics
- `web-ux`: Web UI behavior, copy, interaction consistency
- `im-bridge`: IM adapters and bridge behavior
- `releng`: CI/release pipelines, packaging, reproducibility
- `qa`: test assets and quality verification
- `docs`: docs parity and release communication
- `ops`: scripts, docker, operational assets

## Domain -> Owner

| Domain | Owner |
|--------|-------|
| `contracts` | `core-platform` |
| `kernel` | `core-platform` |
| `daemon` | `core-platform` |
| `runners` | `core-platform` |
| `core-other` | `core-platform` |
| `port-mcp` | `mcp-surface` |
| `port-web` | `web-ux` |
| `web-ui` | `web-ux` |
| `port-im` | `im-bridge` |
| `ci-release` | `releng` |
| `tests` | `qa` |
| `docs-standards` | `docs` |
| `docs-reference` | `docs` |
| `docs-guide` | `docs` |
| `docs-other` | `docs` |
| `ops-scripts` | `ops` |
| `docker` | `ops` |
| `misc` | `ops` |

## Notes

- This is a starting map for `R1`; owners can be refined later.
- Any `P0/P1` finding must also include an individual assignee.

