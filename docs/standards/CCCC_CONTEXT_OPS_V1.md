# CCCC Context Ops Contract v2

Status: Active (for CCCC v0.5.x ecosystem)

This document defines the operation list and payload shapes for the daemon IPC operation:
- `context_sync` (see `docs/standards/CCCC_DAEMON_IPC_V1.md`)

The goal is to make `context_sync` usable from strong-typed SDKs without reading daemon source code.

## 0. Conformance Language

The key words **MUST**, **MUST NOT**, **SHOULD**, **SHOULD NOT**, and **MAY** in this document are to be interpreted as described in RFC 2119.

## 1. Overview

`context_sync` applies a batch of small "context ops" to a group's shared context storage (vision/overview/tasks/agents).

All operations are applied in order. If any op is invalid, the daemon rejects the entire batch.

## 2. Operation Item Shape (Normative)

Each item in `args.ops` MUST be a JSON object with:

```ts
type ContextOpV2 = { op: string } & Record<string, unknown>
```

Rules:
- `op` MUST be one of the strings listed in §3.
- Unknown `op` values MUST be rejected.
- Unknown fields in an op item MUST be ignored (forward compatibility), unless an op explicitly forbids them.

## 3. Operation List (v2)

### 3.1 Vision / Overview

#### `vision.update`

```ts
{ op: "vision.update"; vision: string }
```

Permission: foreman or user.

#### `overview.manual.update`

```ts
{
  op: "overview.manual.update"
  roles?: string[]
  collaboration_mode?: string
  current_focus?: string
}
```

Permission: foreman or user.
Notes:
- Only provided fields are updated (partial patch).
- `updated_by` and `updated_at` are set automatically by daemon.

### 3.2 Tasks (multi-level tree)

Task statuses:
```ts
type TaskStatusV2 = "planned" | "active" | "done" | "archived"
```

Step statuses:
```ts
type StepStatusV2 = "pending" | "in_progress" | "done"
```

#### `task.create`

```ts
{
  op: "task.create"
  name: string
  goal?: string
  steps?: Array<{ name?: string; acceptance?: string }>
  parent_id?: string | null    // null or omitted = root task
  assignee?: string | null
}
```

Permission: any actor.
Notes:
- Root tasks (parent_id=null) carry stage/phase semantics (old milestone).
- Child tasks carry execution semantics.
- If `parent_id` is provided, the parent task MUST exist.

#### `task.update`

```ts
{
  op: "task.update"
  task_id: string

  // Metadata fields (partial patch)
  name?: string
  goal?: string
  assignee?: string | null

  // Optional step update
  step_id?: string
  step_status?: StepStatusV2
}
```

Permission: assignee or foreman.
Rules:
- If `step_id` is provided, `step_status` MUST also be provided (and vice versa).
- Does NOT change task status — use `task.status` for that.

#### `task.status`

```ts
{
  op: "task.status"
  task_id: string
  status: TaskStatusV2
}
```

Permission: assignee or foreman.
Notes:
- Changing to `"archived"` records `archived_from` to support restore.
- Root task completion (`status="done"` on root task) triggers memory solidify+export hook.

#### `task.move`

```ts
{
  op: "task.move"
  task_id: string
  new_parent_id: string | null    // null = promote to root
}
```

Permission: foreman or user.
Rules:
- MUST reject if `new_parent_id` creates a cycle (ancestor traversal check).
- If `new_parent_id` is not null, the target parent MUST exist.

#### `task.restore`

Restore an archived task to its previous status.

```ts
{ op: "task.restore"; task_id: string }
```

Permission: foreman or user.
Rules:
- The task MUST currently be archived.

### 3.3 Agent State (short-term working memory)

Agent state entries are keyed by `agent_id` (typically an `actor_id`).

#### `agent.update`

```ts
{
  op: "agent.update"
  agent_id: string
  active_task_id?: string | null
  focus?: string
  blockers?: string[]
  next_action?: string
  what_changed?: string
  decision_delta?: string
  environment?: string
  user_profile?: string
  notes?: string
}
```

Permission: self or foreman.
Notes:
- Only provided fields are updated (partial patch).
- Daemon sets `updated_at` automatically.
- Legacy `status` alias is removed in v2; write `focus` explicitly.

#### `agent.clear`

```ts
{ op: "agent.clear"; agent_id: string }
```

Permission: self or foreman.
Notes:
- Clears all short-term fields for the target agent and refreshes `updated_at`.

### 3.4 Removed ops (from v1)

The following ops are no longer supported in v2:
- `sketch.update` → use `overview.manual.update`
- `milestone.create` / `milestone.update` / `milestone.complete` / `milestone.restore` → use `task.*` with `parent_id=null` for root tasks
- `note.add` / `note.update` / `note.remove` → removed (use memory store)
- `reference.add` / `reference.update` / `reference.remove` → removed (use memory store)

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

## 6. Result Notes (Non-normative)

`context_sync` returns `changes: Array<{ index, op, detail }>` where `detail` is intended for logs/UI.
SDKs SHOULD NOT parse `detail` as a stable machine contract.

When Group Space is enabled and a curated Context change is detected, the daemon MAY also return:

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

Curated trigger allowlist (v2):

- `vision.*`
- `overview.*`
- `task.*`

`agent.*` updates do not trigger Group Space export.

## 7. Permission Model

| Role | Allowed ops |
|------|------------|
| `user` | All ops |
| `foreman` | All ops |
| `peer` | `task.create`, `task.update` (own assigned), `task.status` (own assigned), `agent.update` (self), `agent.clear` (self) |

Permission check uses the `by` field in `context_sync` args.
