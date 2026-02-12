# RC19 Contract/Parity Gap Baseline

Generated for `R2` (Architecture & Contract Audit).

## Summary

- Total baseline gaps: 12
- `P0`: 0
- `P1`: 6
- `P2`: 6

## Baseline Gap List

| ID | Severity | Area | Gap | Evidence | Suggested Fix |
|----|----------|------|-----|----------|---------------|
| RC19-GAP-001 | P1 | Tooling docs parity | Public docs claim `38+` or `41` MCP tools while implementation exposes 49 tools | `README.md:17`, `docs/index.md:34`, `docs/reference/architecture.md:170`, `docs/vnext/README.md:73`, implementation in `src/cccc/ports/mcp/server.py:1` | Replace hardcoded counts with generated number or non-numeric wording; align all docs in one sweep |
| RC19-GAP-002 | P1 | CLI docs parity | CLI reference lists unsupported commands/options (`cccc group edit`, `cccc groups --json`) | `docs/reference/cli.md:65`, `docs/reference/cli.md:84`, parser in `src/cccc/cli.py:2662` | Rewrite CLI reference directly from parser source of truth |
| RC19-GAP-003 | P1 | Storage path parity | Docs mention outdated blob path `state/ledger/blobs`, runtime uses `state/blobs` | `docs/reference/architecture.md:140`, `docs/vnext/ARCHITECTURE.md:140`, runtime note in `src/cccc/resources/cccc-help.md:112` | Normalize all docs and standards/examples to `state/blobs` |
| RC19-GAP-004 | P1 | Version/status drift | Main README still marks project as `0.4.0rc18`; repo head is already beyond rc18 | `README.md:5`, package version `pyproject.toml:7`, head `v0.4.0-rc18-36-g...` | Refresh release status and install examples for rc19 cycle |
| RC19-GAP-005 | P1 | Standards example drift | Daemon IPC examples still hardcode `0.4.0rc14` | `docs/standards/CCCC_DAEMON_IPC_V1.md:65`, `docs/standards/CCCC_DAEMON_IPC_V1.md:1137` | Update examples to version-agnostic placeholders or current release token |
| RC19-GAP-006 | P1 | Release playbook drift | Release doc still uses rc16 examples and old tag snippets | `docs/vnext/RELEASE.md:20`, `docs/vnext/RELEASE.md:35` | Refresh release guide with rc19 board flow and tag examples |
| RC19-GAP-007 | P2 | Product status drift | vnext status board is stale and inconsistent with implemented surfaces | `docs/vnext/STATUS.md:3`, `docs/vnext/STATUS.md:11` | Rewrite status page from current reality and release goals |
| RC19-GAP-008 | P2 | CI quality gate | CI currently build/smoke oriented; no explicit test gate in workflow | `.github/workflows/ci.yml:38` | Add pytest gate and optional lint/type gate |
| RC19-GAP-009 | P2 | Release quality gate | Release workflow builds and publishes but lacks full regression test gate | `.github/workflows/release.yml:95` | Add test execution before upload step |
| RC19-GAP-010 | P2 | Governance assets | Missing baseline release/governance docs (`CHANGELOG`, `SECURITY`, support policy) | repository root inventory | Add minimal production-grade governance docs before rc19 publish |
| RC19-GAP-011 | P2 | SDK positioning clarity | SDK doc is still labeled draft and disconnected from current release narrative | `docs/sdk/CLIENT_SDK.md:1` | Clarify status (`planned` vs `shipped`) and align with release scope |
| RC19-GAP-012 | P2 | State-surface parity | Automation contract supports `group_state=stopped`, but MCP `cccc_group_set_state` schema does not expose `stopped` | `src/cccc/contracts/v1/automation.py:59`, `src/cccc/ports/mcp/server.py:1818` | Align MCP schema/description with runtime semantics, or explicitly document intentional restriction |

## Recommended Fix Order

1. `RC19-GAP-001` to `RC19-GAP-006` (contract/doc truth)
2. `RC19-GAP-008` and `RC19-GAP-009` (quality gates)
3. `RC19-GAP-007`, `RC19-GAP-010`, `RC19-GAP-011` (governance and communication)

## Exit Criteria for R2 Baseline

- Each gap has owner and target phase (`R6` or `R7`)
- All `P1` gaps have concrete patch plan
- Findings are mirrored in `RC19_FINDINGS_REGISTER.md`

## R2 Closure Update (2026-02-11)

- Added `tests/test_cccs_core_profile_events.py` to validate CCCS core profile event-kind coverage in runtime ledger emission.
- Added `tests/test_daemon_ipc_docs_parity.py` to prevent daemon-IPC standards drift from reappearing.
- Added `tests/test_event_kind_model_parity.py` and expanded `contracts.v1.event` models for missing core `group.*` event kinds (`group.set_state`, `group.settings_update`, `group.automation_update`).
- `RC19_EXECUTION_CHECKLIST.md` now marks R2 contract audit items as complete.
