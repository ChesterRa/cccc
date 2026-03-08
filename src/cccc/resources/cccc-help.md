# CCCC Help

This document is on-demand operational guidance.
Always-on rules live in system/preamble; this file expands details, examples, and edge cases.

Run `cccc_help` anytime to refresh the effective playbook for this group.

Cold start default: `cccc_bootstrap` gives a lean `session + recovery + inbox_preview + memory_recall_gate` packet. Pull `cccc_help`, `cccc_project_info`, or `cccc_context_get` only when you need colder detail.

## Quick Card

1. No fabrication. Investigate first and verify before claiming done.
2. Use MCP for visible coordination: `cccc_message_send` / `cccc_message_reply`.
3. Keep the control plane fresh at key transitions: `cccc_coordination`, `cccc_task`, `cccc_agent_state`.
4. Keep runtime todo current before implementation and before status replies.
5. For strategy/scope questions, align first; implement only after explicit action intent.
6. Search local context/memory before asking the user or browsing the web.
7. Expand capabilities before declaring blocked.
8. Do not claim full done while current in-scope asks remain unresolved.

## Where Things Live

### Chat

- Visible coordination belongs in `cccc_message_send` / `cccc_message_reply`.
- Targets can be `@all`, `@foreman`, `@peers`, `user`, or a specific actor id.
- Terminal output is not chat delivery.

### Coordination (shared control plane)

- Shared truth lives in:
  - `coordination.brief`
  - task cards
- Read the snapshot with `cccc_context_get`.
- Edit the brief with `cccc_coordination`:
  - `cccc_coordination(action="update_brief", ...)`
  - `cccc_coordination(action="add_decision"|"add_handoff", ...)`
- Use `cccc_task` for shared work units:
  - `list`, `create`, `update`, `move`, `restore`
- Distinction:
  - runtime todo = private scratchpad
  - `cccc_task` = shared collaboration truth

### Agent State (personal working memory)

- `cccc_agent_state` is per-actor short-term working memory.
- Keep hot fields fresh at key transitions:
  - `focus`
  - `active_task_id` (when applicable)
  - `blockers`
  - `next_action`
  - `what_changed`
- Use warm fields only when they improve recovery:
  - `open_loops`
  - `commitments`
  - `environment_summary`
  - `user_model`
  - `persona_notes`
  - `resume_hint`
- Recommended update pattern:
  - `cccc_agent_state(action="update", actor_id="<self>", focus="...", next_action="...", what_changed="...")`

### Memory (long-term)

- Long-term memory is file-based:
  - `state/memory/MEMORY.md`
  - `state/memory/daily/YYYY-MM-DD__<group_label>.md`
- Cold-start loop:
  1. read `cccc_bootstrap().memory_recall_gate`
  2. if needed, run `cccc_memory(action="search", ...)`
  3. open hits with `cccc_memory(action="get", ...)`
- Maintenance tools:
  - `cccc_memory_admin(action="context_check"|"compact"|"daily_flush", ...)`
  - `cccc_memory_admin(action="index_sync", ...)`
- Daemon may auto-run `context_check -> daily_flush` when conversation pressure is high. Keep signal high and avoid duplicate writes.

### PROJECT.md

- `PROJECT.md` is a cold background artifact, not the hot control plane.
- Use `cccc_project_info` when you need the full document.
- Keep only the hot digest inside `coordination.brief.project_brief`.

### Inbox

- Inbox is an unread queue, not a task board.
- Bootstrap includes only `inbox_preview`; use `cccc_inbox_list` for the full unread queue.
- Mark read intentionally via `cccc_inbox_mark_read`.
- If `reply_required=true`, do not stop at mark-read: send a concrete reply.

### Todo (runtime-first)

- Every concrete user ask/question (even simple) = one runtime todo item.
- Keep parallel asks separate.
- Capture implicit asks too (`first...`, `next...`, `also...`, `by the way...`).
- Treat todo as a rolling notebook across turns; do not force-clear per reply.
- Before implementation, reconcile all approved parts; do not execute only the latest discussed part by default.
- If new evidence overturns prior assumptions, refactor todo immediately (split/merge/reorder/defer).
- Anti-drip delivery: once implementation is approved, finish the agreed scope in one pass unless a real blocker stops progress.
- Include obvious low-risk in-scope polish in the same pass; do not defer it behind “if you want, I can...”.
- Promote to shared `cccc_task` only for shared, long-horizon, or user-requested tracking.
- For status replies, map current approved scope items to `done` / `pending` / `blocked(owner)`.
- No full-done summary until every current approved-scope ask is `done` / `blocked(owner)` / `deferred(reason)`.

## Intent and Scope Alignment

- For strategy/scope questions, align first; do not implement until explicit action intent.
- Before implementation, verify facts and restate target + constraints in one line.
- If objective/facts are unclear, mark `pending_confirm` in todo and ask one concise clarification.

## Planning Balance (6D)

For non-trivial plans, evaluate all six dimensions:

1. value / ROI
2. complexity & cognitive load
3. feasibility
4. verifiability
5. risk & side effects
6. reversibility

If one dimension is critically weak, narrow scope or add mitigation before implementation.

## Gap Routing

### Information gap

1. `cccc_bootstrap` / `cccc_context_get`
2. `cccc_project_info`
3. `cccc_inbox_list`
4. `cccc_memory(action="search", ...)`
5. external web search (if policy/runtime allows)

### Capability gap

1. fast path: `cccc_capability_use(...)`
2. discovery: `cccc_capability_search(kind="mcp_toolpack"|"skill", query=...)`
3. then `cccc_capability_use(capability_id=..., scope="session")`
4. if state is `activation_pending` or `refresh_required=true`, relist/reconnect then retry
5. if still not ready, read `diagnostics` + `resolution_plan`; ask the user only for real env/permission blockers

## Capability Hygiene

- Discover first: `cccc_capability_search`
- Use `readiness_preview` from search/import dry-run to spot blockers before enable retries
- Discover built-in packs without guessing keywords: `cccc_capability_search(kind="mcp_toolpack")`
- Enable only what is needed now: `cccc_capability_enable` (prefer `scope=session`)
- Fast path for execution: `cccc_capability_use`
- Verify current exposure: `cccc_capability_state`
- Emergency deny for runtime side effects: `cccc_capability_block(scope=group, blocked=true, reason=...)`
- Recovery after verification: `cccc_capability_block(scope=group, blocked=false)`
- Temporary stop only: `cccc_capability_enable(enabled=false)`
- Stop + best-effort cache cleanup: `cccc_capability_enable(enabled=false, cleanup=true)`
- Cleanup unused external capability cache/bindings after work: `cccc_capability_uninstall`
- Skill note:
  - capsule skill is runtime capsule activation, not a full local skill-package install
  - skill runtime success is primarily visible via `capability_state.active_capsule_skills`; `dynamic_tools` may stay unchanged
  - if you need full local skill scripts/assets, install a normal skill package into `$CODEX_HOME/skills`

## Role Notes

### Foreman

- Own outcome quality and integration.
- Keep objective/focus/constraints coherent and stop drift early.
- Review peer outputs with explicit basis: what was checked, what remains unverified.
- Escalate only when the decision impact is high or the blocker is truly external.

### Peer

- Be proactive: report risks and alternatives early.
- Deliver small verifiable outputs, not vague status.
- If direction is wrong, say so and propose a better route.
- If no longer needed, remove self: `cccc_actor(action="remove", actor_id=<self>)`.

## Appendix

### Group State

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
- Resolve blob relative path to absolute path: `cccc_file(action=blob_path, rel_path=...)`
- Send local file as attachment: `cccc_file(action=send, path=...)`

### Terminal Transcript

- Tail actor terminal transcript (subject to group policy):
  - `cccc_terminal(action=tail, target_actor_id=...)`

### Automation Tools

- Read current automation: `cccc_automation(action=state)`
- Manage reminders: `cccc_automation(action=manage)`
- Use automation for objective periodic reminders, not chat spam.
