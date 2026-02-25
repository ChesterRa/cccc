# CCCC Context Ops Contract v1

Status: Draft (for CCCC v0.4.x ecosystem)

This document defines the operation list and payload shapes for the daemon IPC operation:
- `context_sync` (see `docs/standards/CCCC_DAEMON_IPC_V1.md`)

The goal is to make `context_sync` usable from strong-typed SDKs without reading daemon source code.

## 0. Conformance Language

The key words **MUST**, **MUST NOT**, **SHOULD**, **SHOULD NOT**, and **MAY** in this document are to be interpreted as described in RFC 2119.

## 1. Overview

`context_sync` applies a batch of small “context ops” to a group’s shared context storage (vision/sketch/milestones/tasks/notes/references/presence).

All operations are applied in order. If any op is invalid, the daemon rejects the entire batch.

## 2. Operation Item Shape (Normative)

Each item in `args.ops` MUST be a JSON object with:

```ts
type ContextOpV1 = { op: string } & Record<string, unknown>
```

Rules:
- `op` MUST be one of the strings listed in §3.
- Unknown `op` values MUST be rejected.
- Unknown fields in an op item MUST be ignored (forward compatibility), unless an op explicitly forbids them.

## 3. Operation List (v1)

### 3.1 Vision / Sketch

#### `vision.update`

```ts
{ op: "vision.update"; vision: string }
```

#### `sketch.update`

```ts
{ op: "sketch.update"; sketch: string }
```

### 3.2 Milestones

Milestone statuses:
```ts
type MilestoneStatusV1 = "planned" | "active" | "done" | "archived"
```

#### `milestone.create`

```ts
{
  op: "milestone.create"
  name: string
  description?: string
  status?: MilestoneStatusV1 // default "planned"
}
```

#### `milestone.update`

```ts
{
  op: "milestone.update"
  milestone_id: string
  name?: string
  description?: string
  status?: MilestoneStatusV1
}
```

Notes:
- Changing `status` to `"active"` may set `started` if not already present.
- Changing `status` to `"archived"` records `archived_from` to support restore.

#### `milestone.complete`

```ts
{
  op: "milestone.complete"
  milestone_id: string
  outcomes?: string
}
```

#### `milestone.restore`

Restore an archived milestone to its previous status.

```ts
{ op: "milestone.restore"; milestone_id: string }
```

Rules:
- The milestone MUST currently be archived.

### 3.3 Tasks

Task statuses:
```ts
type TaskStatusV1 = "planned" | "active" | "done" | "archived"
```

Step statuses:
```ts
type StepStatusV1 = "pending" | "in_progress" | "done"
```

#### `task.create`

```ts
{
  op: "task.create"
  name: string
  goal?: string
  steps?: Array<{ name?: string; acceptance?: string }>
  milestone_id?: string | null
  milestone?: string | null
  assignee?: string | null
}
```

Notes:
- `milestone_id` and `milestone` are aliases (either may be used).

#### `task.update`

```ts
{
  op: "task.update"
  task_id: string

  // Task fields
  status?: TaskStatusV1
  name?: string
  goal?: string
  assignee?: string | null
  milestone_id?: string | null
  milestone?: string | null

  // Optional step update
  step_id?: string
  step_status?: StepStatusV1
}
```

Rules:
- If `step_id` is provided, `step_status` MUST also be provided (and vice versa).

#### `task.restore`

Restore an archived task to its previous status.

```ts
{ op: "task.restore"; task_id: string }
```

Rules:
- The task MUST currently be archived.

### 3.4 Notes

#### `note.add`

```ts
{ op: "note.add"; content: string }
```

#### `note.update`

```ts
{ op: "note.update"; note_id: string; content?: string }
```

#### `note.remove`

```ts
{ op: "note.remove"; note_id: string }
```

### 3.5 References

#### `reference.add`

```ts
{ op: "reference.add"; url: string; note?: string }
```

#### `reference.update`

```ts
{
  op: "reference.update"
  reference_id: string
  url?: string
  note?: string
}
```

#### `reference.remove`

```ts
{ op: "reference.remove"; reference_id: string }
```

### 3.6 Presence

Presence entries are keyed by `agent_id` (typically an `actor_id`).

#### `presence.update`

```ts
{ op: "presence.update"; agent_id: string; status: string }
```

#### `presence.clear`

```ts
{ op: "presence.clear"; agent_id: string }
```

## 4. Dry Run

If `context_sync.args.dry_run == true`, the daemon MUST NOT persist changes and SHOULD still return a computed `changes` list.

## 5. Result Notes (Non-normative)

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

Curated trigger allowlist (current v1 behavior):

- `vision.*`
- `sketch.*`
- `milestone.*`
- `task.*`
- `note.*`
- `reference.*`

`presence.*` updates do not trigger Group Space export.
