# RC19 Port Parity Findings (R4)

Status: completed baseline pass (2026-02-11), no unresolved parity finding in current register.

## Scope

- CLI (`src/cccc/cli.py`)
- MCP (`src/cccc/ports/mcp/server.py`)
- Web (`web/src/**`, `src/cccc/ports/web/**`)
- IM bridge (`src/cccc/ports/im/**`)
- user-facing docs/help parity

## Baseline Findings

| ID | Severity | Surface | Finding | Evidence | Owner | Status |
|----|----------|---------|---------|----------|-------|--------|
| RC19-001 | P1 | MCP docs | Tool capability count drift between docs and implementation. | `README.md:17`, `docs/index.md:34`, `docs/reference/architecture.md:170`, `docs/vnext/README.md:73` | `docs` | fixed |
| RC19-002 | P1 | CLI docs | Command examples list unsupported forms/options. | `docs/reference/cli.md:49`, `src/cccc/cli.py:2662` | `docs` | fixed |
| RC19-003 | P1 | Architecture docs | Blob storage path in docs mismatches runtime truth. | `docs/reference/architecture.md:140`, `src/cccc/resources/cccc-help.md:112` | `docs` | fixed |
| RC19-004 | P1 | Standards docs | IPC examples are pinned to stale release version token. | `docs/standards/CCCC_DAEMON_IPC_V1.md:65`, `docs/standards/CCCC_DAEMON_IPC_V1.md:1137` | `docs` | fixed |
| RC19-007 | P1 | Release docs | Release runbook examples still target old rc cycle. | `docs/vnext/RELEASE.md:28`, `docs/vnext/RELEASE.md:43` | `docs` | fixed |
| RC19-010 | P1 | Multilingual docs | ZH/JA README parity drift from current implementation/version narrative. | `README.zh-CN.md:17`, `README.ja.md:17` | `docs` | fixed |
| RC19-011 | P2 | MCP + automation parity | `group_state=stopped` is valid in automation action flow, but MCP group-state schema does not expose it. | `src/cccc/contracts/v1/automation.py:59`, `src/cccc/ports/mcp/server.py:784`, `src/cccc/ports/mcp/server.py:1823`, `tests/test_mcp_group_set_state_stopped.py:1` | `mcp-surface` | fixed |
| RC19-012 | P1 | MCP tool args parity | `cccc_automation_manage` schema accepts `actor_id`, but resolver accepted only `by`, causing schema-valid calls to fail. | `src/cccc/ports/mcp/server.py:130`, `tests/test_mcp_automation_manage_actor_id_alias.py:1` | `mcp-surface` | fixed |
| RC19-013 | P2 | IM bridge UX parity | IM runtime already supports implicit send for routed text, but subscribe/help/platform docs still say plain chat is ignored. | `src/cccc/ports/im/bridge.py:422`, `src/cccc/ports/im/commands.py:188`, `docs/guide/im-bridge/slack.md:161`, `docs/guide/im-bridge/discord.md:148` | `im-bridge` | fixed |

## R4 Exit Tracking

- Critical cross-surface mismatch unresolved: 0 (`P1`)
- Minor parity mismatch unresolved: 0 (`P2`)
- Next action: keep parity rows green while advancing R5/R7 assets
