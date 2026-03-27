# Minion Handover Cherry-pick Assessment

Date: `2026-03-17`

Status: `assessed, not yet merged`

Current branch at assessment time: `feature/web-and-daemon-version-badges`

## Purpose

This note preserves the assessment of a proposed `minion handover / reasoning capture` import so that implementation can resume later without reconstructing the analysis from chat history.

The goal of the assessment was to determine:

1. Whether the requested commits can be `git cherry-pick`-ed onto the current codebase.
2. Which parts are structurally low-risk versus behaviorally high-risk.
3. What import order is safest for later implementation.

## Assessed Commits

The following commits were assessed in this exact order:

1. `a312003`
   `feat(contracts): add metadata payload structure to Event and ChatMessageData`
2. `61cc86b`
   `feat(minion): add Dual-Pipeline JSONL collector and eval track`
3. `9692df1`
   `docs(minion): add agent-sessions deep research report for thinking block extraction`
4. `fa9238f`
   `feat(minion): add JSONL thinking block parser (ported from agent-sessions)`
5. `f59763b`
   `feat(minion): add PTY ANSI stream cleaner for reasoning extraction`
6. `05621ee`
   `feat(minion): refine ANSIStreamCleaner to handle Erase In Line and hook into pty`
7. `596886e`
   `feat(minion-v2): Phase 0-5 complete implementation of PTY Interceptor & Watchdog`
8. `2752d35`
   `test(minion): commit remaining test cases and validation fixes for Handover Orchestrator`

## Verified Facts

### Cherry-pick result

- A real dry-run was performed in a temporary worktree created from the assessment branch tip.
- All `8` commits above were cherry-picked successfully.
- No textual merge conflicts occurred.
- The temporary worktree stayed clean after the cherry-pick sequence completed.

### Syntax and import smoke checks

The assessed stack passed the following lightweight validation in the temporary worktree:

- Python `compileall` over the imported `minion` modules and related tests.
- Direct import and execution smoke checks for:
  - `Event` envelope metadata
  - `ChatMessageData.metadata`
  - `ChatStreamData.metadata`
  - `normalize_event_data(...)` metadata retention
  - `ANSIStreamCleaner`
  - `UnifiedEventMerger`
  - `_format_facts_preamble(...)`

### What could not be fully verified in the assessment environment

- Full `pytest` execution was not completed because the available virtual environment did not provide a working `pytest` installation in that assessment context.
- Therefore, this assessment confirms:
  - cherry-pick compatibility
  - syntax/import viability
  - targeted smoke behavior
- This assessment does **not** claim full runtime or regression safety.

## Key Integration Surfaces

Most files in this stack are additive. The main existing surfaces touched by the proposed import are:

### Contracts

- `src/cccc/contracts/v1/event.py`
- `src/cccc/contracts/v1/message.py`

Notable effect:

- Introduces metadata-bearing payload support for `Event`, `ChatMessageData`, and `ChatStreamData`.

Assessment:

- Low structural risk.
- Good candidate for early import.

### PTY runner

- `src/cccc/runners/pty.py`

Notable effect:

- Adds PTY output hook support via `on_output` / `set_output_hook`.

Assessment:

- Low-to-medium structural risk.
- Important foundation for later reasoning capture.
- Needs care because it introduces a new shared hook slot into the PTY supervision path.

### Daemon startup path

- `src/cccc/daemon/server.py`

Notable effect:

- `596886e` wires `logger`, `watchdog`, and `orchestrator` into daemon startup.

Assessment:

- Highest behavior risk in the whole stack.
- This is the main reason the full import should not be treated as a zero-risk bulk cherry-pick, even though the patches apply cleanly.

## Risk Summary

### Low-risk group

These are mostly additive and comparatively safe to import early:

- `a312003`
- `9692df1`
- `61cc86b`
- `fa9238f`
- `f59763b`
- `05621ee`

Why they are lower risk:

- Mostly new files or local contract extensions.
- Minimal direct coupling to current daemon default behavior.
- The only shared-path change in this set is the PTY output hook addition in `src/cccc/runners/pty.py`.

### Higher-risk group

- `596886e`
- `2752d35`

Why they are higher risk:

- They move the work from “available capability” to “default daemon behavior”.
- They introduce watchdog/orchestrator side effects during ordinary daemon and actor lifecycle.

## Behavior Risks To Re-check Before Real Merge

These are the main risks that should be deliberately tested during implementation.

### 1. Single PTY output hook slot

The imported PTY changes add a shared output hook mechanism on the supervisor.

Risk:

- If later features also need PTY stream interception, there may be overwrite or composition issues.

Implementation note:

- Consider whether the final design should support hook fan-out instead of a single hook slot.

### 2. Reasoning logger starts background JSONL watchers

The imported logger stack sets up background watchers and currently hardcodes a probe under `~/.claude/projects`.

Risk:

- This is not runtime-neutral.
- It may be mismatched with Codex-focused workflows.
- It may create persistent background thread / resource behavior even when the feature is not actively needed.

Implementation note:

- Re-check whether the watcher should be runtime-aware and gated by actor runtime rather than always probing a Claude-specific path.

### 3. Watchdog writes back into actor PTY

The watchdog can inject dehydration messages into live actor PTY sessions when it detects flood / repeat / error patterns.

Risk:

- False positives may interrupt legitimate long-running agent output.
- This directly changes agent behavior, not just observability.

Implementation note:

- Treat watchdog thresholds and enablement as rollout-sensitive.

### 4. Orchestrator is not purely passive

Even with `auto_restart=False`, the orchestrator still enters handover logic when an actor exits.

Risk:

- If environment keys are available, it can perform LLM extraction and MCP synchronization paths.
- This means the feature is not merely “recording”; it can become part of the default lifecycle.

Implementation note:

- Prefer explicit gating for initial rollout.

## Recommended Import Strategy

The safest later implementation path is **not** to import everything in one step on a busy branch.

### Recommended phase split

Phase A: import the low-risk foundations first

1. `a312003`
2. `9692df1`
3. `61cc86b`
4. `fa9238f`
5. `f59763b`
6. `05621ee`

Goal:

- Land contracts, parser, ANSI cleaner, PTY hook, and collector groundwork without enabling daemon-side lifecycle behavior by default.

Phase B: import daemon behavior last

7. `596886e`
8. `2752d35`

Goal:

- Review and possibly refactor daemon startup integration before enabling logger/watchdog/orchestrator by default.

### Strong recommendation

Before merging `596886e` into an actively used branch, consider adding an explicit feature gate such as an environment flag or settings switch so that:

- code can land,
- tests can be written,
- and daemon startup behavior does not change for every user immediately.

## Suggested Follow-up Validation

When implementation resumes, the following validation should be done in addition to normal unit tests:

1. Start daemon with feature disabled and confirm existing startup behavior is unchanged.
2. Start daemon with feature enabled and confirm:
   - PTY output interception works
   - transcript files are produced as expected
   - watchdog does not trigger on normal agent activity
   - orchestrator does not unexpectedly restart or mutate actors
3. Validate Codex-focused scenarios separately from Claude/JSONL scenarios.
4. Re-run resume/session workflows if the feature touches PTY lifecycle in the same branch.

## Decision Snapshot

Final assessment conclusion:

- `Cherry-pick compatibility`: `high`
- `Text conflict risk`: `low`
- `Behavior change risk`: `medium to high`
- `Recommended execution style`: `phased import, not bulk merge`

In short:

- The stack can be imported.
- The stack should not be treated as operationally harmless.
- The daemon startup wiring should be reviewed and likely gated before real rollout.
