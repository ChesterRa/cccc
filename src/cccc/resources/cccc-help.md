# CCCC Help

This document is on-demand operational guidance.
Always-on rules are enforced by system/preamble; this file expands details, examples, and edge-case handling.

Run `cccc_help` anytime to refresh the effective playbook for this group.

## Quick Card (Most Runs)

1. No fabrication. Do not invent facts, steps, results, sources, quotes, or tool outputs.
2. Investigate first; verify before claiming done.
3. Use MCP for visible coordination: `cccc_message_send` / `cccc_message_reply`.
4. Terminal output is not delivered as chat.
5. Keep Context current at key transitions (start/milestone/blocker/resume/done).
6. Keep runtime todo current before implementation and before status replies.
7. For strategy/scope discussion, align first; implement only after explicit action intent.
8. If information is insufficient, search local context/memory first, then web if allowed.
9. If capability is insufficient, use capability tools before declaring blocked.
10. If asks remain unresolved, report pending + next step; do not send a full-done summary.

## Where Things Live

### Chat (visible coordination)

- Use `cccc_message_send` / `cccc_message_reply`.
- Targets: `@all`, `@foreman`, `@peers`, `user`, or a specific actor id.
- Use normal/attention/task (`reply_required`) intentionally; avoid both overuse and underuse.

### Context (shared short-horizon memory)

- Keep working state here: DoD, tasks, overview, and per-agent short-term state.
- Minimum agent-state upkeep at key transitions:
  - `focus`
  - `active_task_id` (when applicable)
  - `blockers`
  - `next_action`
  - `what_changed`
- Recommended update command:
  - `cccc_context_agent(action="update", agent_id="<self>", focus="...", next_action="...", what_changed="...")`
- Distinction: runtime todo is private execution scratchpad; `cccc_task` is shared collaboration truth.

### Memory (long-term)

- `Context` is short-term execution memory.
- `memory.db` is long-term reusable memory for facts/decisions/patterns.
- Practical loop:
  - recall first: `cccc_memory(action=search, ...)`
  - ingest at milestones: `cccc_memory_admin(action=ingest, mode="signal")`
  - store stable outcomes: `cccc_memory(action=store, ...)`
  - clean intentionally: `cccc_memory_admin(action=decay|delete, ...)`

### Inbox (unread queue)

- Inbox is "messages since cursor", not a full task system.
- Read via `cccc_inbox_list`.
- Mark read intentionally via `cccc_inbox_mark_read`.
- For `reply_required=true`, do not stop at mark-read: send a concrete reply.

### Todo (runtime-first)

- Every concrete user ask/question (even simple) = one runtime todo item; keep parallel asks separate.
- Capture implicit asks too (e.g. `first.../next.../also.../by the way...`) as pending todo items.
- Treat todo as a rolling notebook across turns; do not force-clear each reply.
- Before implementation, reconcile all plan parts + approved scope; do not execute only the latest discussed part by default.
- If new evidence overturns prior assumptions, refactor todo immediately (split/merge/reorder/defer).
- Anti-drip delivery: once implementation is approved, finish the agreed scope in one pass; stop early only for real blockers.
- In-scope polish rule: include obvious low-risk in-scope polish in the same pass; do not defer it behind "if you want, I can...".
- Scope boundary: do not use polish to expand scope; ask first if a change is beyond agreed scope.
- Keep todo capacity high when parallel asks are common (recommended soft `>=80`, hard `>=120` if runtime supports).
- Promote to shared `cccc_task` only for shared/long-horizon/user-requested tracking; do not mirror every runtime todo into `cccc_task`.
- For status replies, map current approved scope items explicitly to `done` / `pending` / `blocked(owner)`.
- Completion gate: no full-done summary until every current approved scope ask is `done` / `blocked(owner)` / `deferred(reason)`; out-of-scope pending memo items may remain.

### Intent & scope alignment

- For strategy/scope questions, align first; do not implement until explicit action intent.
- Before implementation, verify facts and restate target + constraints in one line.
- If objective/facts are unclear, mark `pending_confirm` in todo and ask one concise clarification.

### Planning balance (6D)

- For non-trivial plans, evaluate all six dimensions:
  - value/ROI, complexity load, feasibility, verifiability, risk/side-effects, reversibility
  - value/ROI
  - complexity load
  - feasibility
  - verifiability
  - risk/side-effects
  - reversibility
- If one dimension is critically weak, narrow scope or add mitigation before implementation.

## Gap routing (high ROI)

- If information is insufficient, search before asking:
  1. `cccc_context_get` / `cccc_project_info` / `cccc_inbox_list`
  2. `cccc_memory(action=search, ...)`
  3. external web search (if runtime/policy allows)
- If capability is insufficient, expand tools before declaring blocked:
  1. fast path: `cccc_capability_use(...)`
  2. discovery path: `cccc_capability_search(kind="mcp_toolpack"|"skill", query=...)`
  3. then `cccc_capability_use(capability_id=..., scope="session")`
  4. if response has `refresh_required=true`, relist/reconnect then retry
  5. if still not ready, read `diagnostics` + `resolution_plan`: fix agent-action items first, ask user only for real env/permission requirements

## Capability Hygiene (Keep MCP Surface Lean)

- Discover first: `cccc_capability_search`
- Discover built-in packs without guessing keywords: `cccc_capability_search(kind="mcp_toolpack")`
- Enable only what is needed now: `cccc_capability_enable` (prefer `scope=session`)
- Fast path for execution: `cccc_capability_use` (auto-enable + optional tool call)
- Verify current exposure: `cccc_capability_state`
- Emergency deny for runtime side effects: `cccc_capability_block(scope=group, blocked=true, reason=...)`
- Recovery after verification: `cccc_capability_block(scope=group, blocked=false)`
- Temporary stop only: `cccc_capability_enable(enabled=false)`
- Stop + best-effort cache cleanup: `cccc_capability_enable(enabled=false, cleanup=true)`
- Cleanup unused external capability cache/bindings after work:
  - `cccc_capability_uninstall`
- Skill note:
  - capability-skill is runtime capsule activation (not a full local skill package install)
  - if you need full local skill scripts/assets, install a normal skill package into `$CODEX_HOME/skills`

## @role: foreman

### Responsibilities

- Own outcome quality and integration.
- Keep goals/constraints/DoD coherent, and stop drift early.
- Review peer outputs with explicit basis ("what was checked" / "what remains unverified").
- Escalate to user only when decision impact is high or blockers are truly external.

### Working with peers

- Delegate with concrete deliverable + acceptance check + short timebox.
- Keep Context updated so others can continue without re-deriving intent.

## @role: peer

### Responsibilities

- Be proactive: report risks and alternatives early.
- Deliver small verifiable outputs, not vague status.
- If direction is wrong, say so and propose a better route.
- If no longer needed, remove self: `cccc_actor(action=remove, actor_id=<self>)`.

## Appendix

### Group state (delivery + automation)

| State | Meaning | Automation | Delivery to PTY |
|-------|---------|------------|-----------------|
| `active` | normal work | enabled | chat + notifications |
| `idle` | waiting / done for now | disabled | chat only; notifications suppressed |
| `paused` | user paused group | disabled | inbox only |
| `stopped` | runtimes stopped | n/a | no actor runtime delivery |

### Permissions (quick)

| Action | user | foreman | peer |
|--------|------|---------|------|
| actor_add | yes | yes | no |
| actor_start | yes | yes (any) | no |
| actor_stop | yes | yes (any) | yes (self) |
| actor_restart | yes | yes (any) | yes (self) |
| actor_remove | yes | yes (self) | yes (self) |

### Attachments

- Inbox events may include `data.attachments[]` with `path` like `state/blobs/<sha256>_<name>`.
- Resolve blob relative path to absolute path:
  - `cccc_file(action=blob_path, rel_path=...)`
- Send local file as attachment:
  - `cccc_file(action=send, path=...)`

### Terminal transcript

- Tail actor terminal transcript (subject to group policy):
  - `cccc_terminal(action=tail, target_actor_id=...)`

### Automation tools

- Read current automation: `cccc_automation(action=state)`
- Manage reminders: `cccc_automation(action=manage)`
- Use automation for objective periodic reminders, not chat spam.
