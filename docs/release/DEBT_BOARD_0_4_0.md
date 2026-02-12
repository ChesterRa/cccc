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
- `D4` in progress: `AutomationTab` extracted shared utilities and dedicated subcomponents (`AutomationRuleList`, `AutomationRuleEditorModal`, `AutomationSnippetModal`, `AutomationPoliciesSection`) while preserving behavior.
- Remaining items are tracked here and should be landed in small, reviewable PRs.
