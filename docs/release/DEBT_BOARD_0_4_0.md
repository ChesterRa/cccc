# Technical Debt Board (`0.4.0` Pre-GA)

This board tracks only **high-ROI, low-side-effect** debt work that should land before `0.4.0`.

## Decision Rules

- No speculative rewrites.
- No protocol/API behavior change unless explicitly approved.
- Refactors must be behavior-preserving and independently releasable.
- Every tranche must pass: `pytest -q`, `npm --prefix web run typecheck`, `npm --prefix web run build`, `npm --prefix docs run build`.

## Global Assessment

After full-file size/churn scan and RC19 deep audit artifacts:

- No additional `P0` architectural blocker beyond the items below.
- The biggest maintainability risk is concentrated in a small set of oversized/high-churn files.
- Priority is to reduce regression probability and review complexity before GA, not to redesign the framework.

## Must-Fix Before `0.4.0`

| ID | Area | Why It Matters | ROI | Risk | Action | Exit Criteria |
|----|------|----------------|-----|------|--------|---------------|
| D1 | Test runtime independence | CI failures from missing local binaries (`codex`, etc.) are avoidable noise and block release confidence. | Very high | Low | Convert non-runtime-behavior tests to `runner=headless` or mocked starts. | No unit test depends on external runtime binaries. |
| D2 | MCP surface maintainability (`src/cccc/ports/mcp/server.py`) | High blast radius for schema/handler drift. | High | Medium | Split by namespace (schema + handlers) while keeping tool behavior identical. | File size reduced; existing MCP parity tests unchanged and green. |
| D3 | Daemon request dispatch maintainability (`src/cccc/daemon/server.py`) | Core reliability risk due to very large all-in-one handler file. | High | Medium | Extract op-family modules (`group`, `actor`, `automation`) without behavior changes. | Dispatch behavior preserved; core tests green; reduced review surface per PR. |
| D4 | Web automation UX maintainability (`web/src/components/modals/settings/AutomationTab.tsx`) | Fast-changing feature in oversized component creates recurring UI regressions. | High | Low | Split into list/editor/modal subcomponents + small hooks. | UI behavior unchanged; smaller components; existing web checks green. |
| D5 | Contract/docs anti-drift guardrails | Manual synchronization does not scale into GA. | High | Low | Expand parity checks in tests/scripts for CLI/MCP/docs surfaces touched in RC19. | New drift introduced by PRs is caught by CI. |

## Should-Fix If Time Allows

| ID | Area | Why It Matters | ROI | Risk | Action | Exit Criteria |
|----|------|----------------|-----|------|--------|---------------|
| S1 | Web API routing modularity (`src/cccc/ports/web/app.py`) | Improves change isolation and debugging, but less urgent than D2/D3. | Medium | Medium | Move route families into dedicated routers. | No API contract regressions; build/tests green. |
| S2 | Modal-heavy UI hotspots (`web/src/components/ContextModal.tsx`, `web/src/components/SettingsModal.tsx`) | Frequent UX churn in large components. | Medium | Low | Continue extraction of reusable modal primitives and sections. | Reduced component size and fewer interaction regressions. |
| S3 | CLI maintainability (`src/cccc/cli.py`) | Large command surface increases accidental drift. | Medium | Medium | Split command registration and handlers by domain. | Existing CLI behavior unchanged and tested. |

## Known Large-File Watchlist

These files are large and should stay under active watch during GA hardening:

- `src/cccc/daemon/server.py`
- `src/cccc/cli.py`
- `src/cccc/ports/web/app.py`
- `src/cccc/ports/mcp/server.py`
- `src/cccc/daemon/automation.py`
- `web/src/components/modals/settings/AutomationTab.tsx`
- `web/src/components/ContextModal.tsx`
- `web/src/components/SettingsModal.tsx`

## Execution Tranches

1. Tranche A: `D1 + D5` (lowest risk, immediate quality gain)
2. Tranche B: `D4 + D2` (front-end and MCP maintainability)
3. Tranche C: `D3` (daemon extraction, behavior-preserving)
4. Tranche D: `S1/S2/S3` only if capacity remains before GA

## Current Progress

- `D1` started: group lifecycle invariant test and several non-runtime-behavior tests now use `runner=headless`.
  - Added lifecycle-runner guardrail test (`tests/test_lifecycle_tests_headless_runner.py`) to prevent reintroducing PTY/binary dependency in lifecycle invariants.
- `D2` completed: MCP tool dispatcher is split by namespace and tool schema moved out of `mcp/server.py` to `mcp/toolspecs.py`.
- `D3` completed:
  - `group_automation_*` operation family extracted from `daemon/server.py` into `daemon/ops/automation_ops.py`.
  - `group_settings_update` extracted into `daemon/ops/group_settings_ops.py`.
  - `attach / group_create / group_template_*` bootstrap flow extracted into `daemon/ops/group_bootstrap_ops.py`.
  - `group_show / group_update / group_detach_scope / group_use / group_delete` extracted into `daemon/ops/group_ops.py`.
  - `group_start / group_stop` extracted into `daemon/ops/group_lifecycle_ops.py`.
  - `groups / registry_reconcile` extracted into `daemon/ops/registry_ops.py`.
  - `ping / shutdown / observability_*` extracted into `daemon/ops/daemon_core_ops.py`.
  - `group_set_state` extracted into `daemon/ops/group_state_ops.py`.
  - `actor_add` extracted into `daemon/ops/actor_add_ops.py`.
  - `actor_list / actor_env_private_keys / actor_env_private_update` extracted into `daemon/ops/actor_ops.py`.
  - `actor_remove` extracted into `daemon/ops/actor_membership_ops.py`.
  - `actor_update` extracted into `daemon/ops/actor_update_ops.py`.
  - `actor_start / actor_stop / actor_restart` extracted into `daemon/ops/actor_lifecycle_ops.py`.
  - Internal actor startup routine extracted into `daemon/ops/actor_runtime_ops.py` (server keeps thin wrapper).
  - `inbox_list / inbox_mark_read` extracted into `daemon/ops/inbox_read_ops.py`.
  - `chat_ack / inbox_mark_all_read` extracted into `daemon/ops/inbox_ack_ops.py`.
  - `send / reply` extracted into `daemon/ops/chat_ops.py`.
  - `term_resize / ledger_snapshot / ledger_compact / send_cross_group` extracted into `daemon/ops/maintenance_ops.py`.
  - Socket-loop special ops `term_attach / events_stream` extracted into `daemon/ops/socket_special_ops.py`.
  - Socket accept-loop request handling extracted into `daemon/ops/socket_accept_ops.py`.
  - Context/headless inline dispatch converted to `try_handle_context_op` and `try_handle_headless_op`.
  - IM bridge stop/cleanup process logic extracted into `daemon/im_bridge_ops.py`.
  - IM bridge bootstrap helper extracted into `daemon/bootstrap_im_ops.py`.
  - Actor autostart bootstrap helper extracted into `daemon/bootstrap_actor_ops.py`.
  - Runtime MCP install/check logic extracted into `daemon/mcp_install.py`.
  - Daemon client transport helper extracted into `daemon/client_ops.py`.
  - Actor private env storage/validation/merge logic extracted into `daemon/private_env_ops.py`.
  - Runner state file read/write/cleanup logic extracted into `daemon/runner_state_ops.py`.
  - Chat flow helper logic (`auto-wake` + attachment normalization) extracted into `daemon/ops/chat_support_ops.py`.
  - Socket protocol helper logic (`recv/send json`, response dump, stream bootstrap, error factory) extracted into `daemon/socket_protocol_ops.py`.
  - Request dispatch chain extracted into `daemon/request_dispatch_ops.py`; `server.handle_request` is now a thin delegator.
  - Daemon run-loop orchestration (automation thread, bind/write endpoint, bootstrap thread, shutdown cleanup) extracted into `daemon/serve_ops.py`.
  - `system_notify / notify_ack` extracted into `daemon/ops/system_notify_ops.py`.
  - `debug_snapshot / terminal_tail / terminal_clear / debug_tail_logs / debug_clear_logs` extracted into `daemon/ops/diagnostics_ops.py`.
  - `daemon/server.py` reduced to <1000 LOC while preserving request-dispatch behavior.
  - Request-dispatch dependency wiring moved from string-key dict access to typed `RequestDispatchDeps` field access, removing key-name drift risk at compile/review time.
  - Extracted slices are behavior-preserving and validated by full regression/build checks.
- `D5` completed:
  - IPC docs parity test now scans both `daemon/server.py` and `daemon/ops/*.py` for `op` handlers, preventing modularization-era doc drift.
  - Added MCP toolspec/dispatch parity test (`tests/test_mcp_toolspec_dispatch_parity.py`) to block MCP tool-name drift between `ports/mcp/toolspecs.py` and `ports/mcp/server.py`.
  - Added MCP toolspec schema guard (`tests/test_mcp_toolspec_schema_guard.py`) to enforce stable tool schema shape and naming conventions.
  - Added CLI reference parity guard (`tests/test_cli_reference_parity.py`) to prevent reintroducing removed command/docs drift.
  - Added Web automation docs parity guard (`tests/test_web_automation_docs_parity.py`) to ensure guide/reference coverage stays aligned with current editor trigger/action surface.
- `D4` completed: `AutomationTab` extracted shared utilities and dedicated subcomponents (`AutomationRuleList`, `AutomationRuleEditorModal`, `AutomationSnippetModal`, `AutomationPoliciesSection`) while preserving behavior.
- Must-fix debt items (`D1`~`D5`) are now closed for `0.4.0` pre-GA baseline.
