# CCCC Context Ops Contract v3

Status: Active (for CCCC v0.5.x ecosystem)

This document defines the operation list and payload shapes for daemon IPC:
- `context_sync` (see `docs/standards/CCCC_DAEMON_IPC_V1.md`)

The goal is to make `context_sync` usable from SDKs and MCP/Web ports without reading daemon source code.

## 0. Conformance Language

The key words **MUST**, **MUST NOT**, **SHOULD**, **SHOULD NOT**, and **MAY** are to be interpreted as described in RFC 2119.

## 1. Overview

`context_sync` applies a batch of small operations to a group's context v3 storage:
- `coordination` = shared control plane (`brief`, recent notes)
- `tasks` = shared dispatch truth
- `agent_states` = per-actor short-term working memory
- `meta` = restricted projection-support metadata

Read-only projections such as `board` and `attention` are daemon-computed and are not directly writable via context ops.

All operations are applied in order. If any op is invalid, the daemon rejects the entire batch.

## 2. Operation Item Shape

Each item in `args.ops` MUST be a JSON object with:

```ts
type ContextOpV3 = { op: string } & Record<string, unknown>
```

Rules:
- `op` MUST be one of the strings listed in §3.
- Unknown `op` values MUST be rejected.
- Unknown fields MAY be ignored unless an op explicitly forbids them.

## 3. Operation List

### 3.1 Coordination

#### `coordination.brief.update`

```ts
{
  op: "coordination.brief.update"
  objective?: string
  current_focus?: string
  constraints?: string[]
  project_brief?: string
  project_brief_stale?: boolean
}
```

Permission: `foreman` or `user`.

Notes:
- Partial patch: only provided fields are updated.
- Daemon sets `updated_by` and `updated_at` automatically.

#### `coordination.note.add`

```ts
{
  op: "coordination.note.add"
  kind: "decision" | "handoff"
  summary: string
  task_id?: string | null
}
```

Permission: any actor.

Notes:
- Used for compact recent decisions / handoffs.
- Daemon keeps only the newest bounded slice in context.

### 3.2 Tasks

Task statuses:

```ts
type TaskStatus = "planned" | "active" | "done" | "archived"
```

Checklist statuses:

```ts
type ChecklistStatus = "pending" | "in_progress" | "done"
```

Waiting values:

```ts
type TaskWaitingOn = "none" | "user" | "actor" | "external"
```

Task type values:

```ts
type TaskType =
  | "free"
  | "standard"
  | "optimization"
```

#### `task.create`

```ts
{
  op: "task.create"
  title: string
  outcome?: string
  status?: TaskStatus
  parent_id?: string | null
  assignee?: string | null
  priority?: string
  blocked_by?: string[]
  waiting_on?: TaskWaitingOn
  handoff_to?: string | null
  task_type?: TaskType
  notes?: string
  checklist?: Array<{ id?: string; text: string; status?: ChecklistStatus }>
}
```

Permission: any actor.

Rules:
- `title` is required.
- If `parent_id` is provided, the parent task MUST exist.
- Peer actors MUST NOT create a task assigned to another peer.
- If `task_type` is provided, it MUST be one of the allowed task-type ids above.
- If `task_type` is omitted, the daemon MUST persist `standard` for a root task and `free` for a subtask.
- `task_type` stores the durable work kind. It MUST NOT be treated as lifecycle state or parent/child structure.
- `task_type` does not auto-seed `notes` or `checklist`; those remain ordinary editable task content.

#### `task.update`

```ts
{
  op: "task.update"
  task_id: string
  title?: string
  outcome?: string
  parent_id?: string | null
  assignee?: string | null
  priority?: string
  blocked_by?: string[]
  waiting_on?: TaskWaitingOn
  handoff_to?: string | null
  task_type?: TaskType
  notes?: string
  checklist?: Array<{ id?: string; text: string; status?: ChecklistStatus }>
}
```

Permission: assignee / handoff target / foreman / user.

Rules:
- Partial patch.
- If `parent_id` changes, the daemon MUST reject cycles.
- `task.update` does not change lifecycle status.
- If `task_type` is provided, it MUST be one of the allowed task-type ids above.

#### `task.move`

```ts
{
  op: "task.move"
  task_id: string
  status: TaskStatus
}
```

Permission: assignee / handoff target / foreman / user.

Notes:
- This is the canonical lifecycle transition op.
- `task.move` only accepts `task_id` and `status`. Fields such as `outcome`, `notes`, `checklist`, or `task_type` MUST go through `task.update`.
- Moving to `archived` records `archived_from`.
- Task lifecycle changes append memory lane events; root-task completion may promote one stable memory entry into `state/memory/MEMORY.md`.

#### `task.restore`

```ts
{ op: "task.restore"; task_id: string }
```

Permission: assignee / handoff target / foreman / user.

Rules:
- The task MUST currently be archived.

### 3.3 Agent State

Agent states are keyed by `actor_id`.

#### `agent_state.update`

```ts
{
  op: "agent_state.update"
  actor_id: string
  active_task_id?: string | null
  focus?: string
  blockers?: string[]
  next_action?: string
  what_changed?: string
  open_loops?: string[]
  commitments?: string[]
  environment_summary?: string
  user_model?: string
  persona_notes?: string
}
```

Permission: self / user.

Notes:
- Partial patch.
- Daemon stores data into `hot` (`active_task_id`, `focus`, `blockers`, `next_action`) and `warm` (the rest).
- `open_loops` is the current memo for unfinished work, unresolved facts, risks, assumptions, and follow-ups.
- `commitments` records promises made to users or other actors.
- `agent_state` is actor-owned working state. Foreman should guide peers via tasks, coordination, or help/role-notes, not by directly rewriting a peer's `agent_state`.
- `resume_hint` is not part of this contract; clients must use `open_loops` for current memo entries.
- Legacy aliases `agent_id`, `environment`, `user_profile`, and `notes` are tolerated by daemon but SHOULD NOT be used by new clients.

#### `agent_state.clear`

```ts
{ op: "agent_state.clear"; actor_id: string }
```

Permission: self / user.

Notes:
- Clears both `hot` and `warm`, then refreshes `updated_at`.

### 3.4 Restricted Meta

#### `meta.merge`

```ts
{
  op: "meta.merge"
  data: {
    project_status?: string | null
  }
}
```

Permission: `foreman` or `user`.

Rules:
- Only `project_status` is allowed.
- Resulting `meta` payload MUST stay within daemon size limits.

## 4. Optimistic Concurrency (CAS)

`context_sync` accepts an optional `if_version` field:

```ts
{ group_id: string; ops: [...]; if_version?: string }
```

Rules:
- If `if_version` is provided and does not match the current context version hash, the entire batch MUST be rejected with error code `version_conflict`.
- If `if_version` is omitted, no version check is performed.

## 5. Dry Run

If `context_sync.args.dry_run == true`, the daemon MUST NOT persist changes and SHOULD still return a computed `changes` list.

## 6. Result Notes

`context_sync` returns `changes: Array<{ index, op, detail }>` where `detail` is intended for logs/UI. SDKs SHOULD NOT parse `detail` as a stable machine contract.

When Group Space is enabled and a curated context change is detected, the daemon MAY also return:

```ts
space_sync?: {
  queued: boolean
  reason?: "not_bound" | "binding_inactive" | "missing_remote_space_id" | "provider_disabled" | "enqueue_failed"
  deduped?: boolean
  job_id?: string
  provider?: "notebooklm"
  kind?: "context_sync"
  idempotency_key?: string
  error?: string
}
```

Curated trigger allowlist:
- `coordination.*`
- `task.*`

`agent_state.*` updates do not trigger Group Space export.

## 7. Permission Model

| Role | Allowed ops |
|------|------------|
| `user` | All ops |
| `foreman` | All ops |
| `peer` | `coordination.note.add`, `task.create`, `task.update` (own assigned / handed-off), `task.move` (own assigned / handed-off), `task.restore` (own assigned / handed-off), `agent_state.update` (self), `agent_state.clear` (self) |

Permission checks use `context_sync.args.by`.
