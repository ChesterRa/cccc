# PTY Runtime Activity

CCCC projects verified Codex and Claude PTY hook events into the Web runtime ticker. This channel is intentionally separate from chat messages: activity can appear above an actor without creating a placeholder message or changing conversation history.

## Data Flow

1. A CCCC-injected provider hook submits a lifecycle event to `cccc hook`.
2. The CLI first applies the existing launch, session, turn, and operation fences.
3. Accepted events are normalized into a small structured activity record.
4. The Rust or Python Web backend serves the same snapshot and SSE contract.
5. The browser keeps activity in an independent, group-scoped store and projects it into the existing `RuntimeDockTicker`.

Codex events retain exact turn and operation fencing. Claude tool events retain the verified session
boundary and stable tool-use identity. Claude turn hooks remain observational because the PTY hook
contract does not expose one stable turn identifier across all event types, so generic Claude turn
text is not projected into the ticker.

## HTTP Contract

Both endpoints use the normal CCCC Web authentication:

```text
GET /api/v1/groups/{group_id}/runtime-activity/snapshot
GET /api/v1/groups/{group_id}/runtime-activity/stream?replay=true
```

The snapshot response contains `result.events`. The stream emits named `runtime-activity` SSE events. Each event has this shape:

```json
{
  "v": 1,
  "id": "unique-revision-id",
  "ts": "2026-07-28T03:00:00Z",
  "group_id": "g_example",
  "actor_id": "worker",
  "runtime": "codex",
  "activity_id": "codex:session:tool:operation",
  "kind": "tool",
  "status": "started",
  "event_type": "PreToolUse",
  "session_id": "session",
  "turn_id": "turn",
  "operation_id": "operation",
  "tool_name": "functions.exec_command",
  "duration_ms": null
}
```

`kind` is `session`, `turn`, `tool`, or `subagent`. Normal status values are `started`, `waiting`,
`completed`, and `failed`; the Web projection can synthesize `stuck` when an active turn or tool has
not advanced for 60 seconds. A newer event with the same `activity_id` replaces the prior display
revision. Session end, terminal interrupt, and a superseding terminal turn close any remaining child
activity for that actor and session.

The ticker displays tool-specific activity only. Its text is derived from the current sanitized
`tool_name` and status, so it changes as the runtime moves between tools. Generic session-start and
turn-start phrases do not create bubbles. Completed and failed tools remain briefly visible without
keeping the actor in a working state. A real completed or failed revision always supersedes a
synthetic `stuck` diagnosis for the same activity, regardless of delivery timestamp.

## Retention and Recovery

- The backend keeps the latest display revision plus the start timestamp needed for an active
  duration, evicts terminal history and then waiting diagnostics before a started predecessor when
  enforcing the 256-record group limit, and removes records older than five minutes. An overflow
  containing only started predecessors fails the hook activity commit instead of silently dropping
  an active tool.
- A fresh snapshot includes active records and completions from the last 15 seconds.
- SSE replay restores the current short-lived view after a brief disconnect.
- `replay=false` starts from changes observed after the stream is attached; disconnects stop
  polling without emitting a replay.
- The browser retains active records for at most five minutes and completed records for eight seconds.
- Leaving a group closes its activity stream and immediately clears that group's browser state;
  returning hydrates it again from snapshot and SSE replay.
- A stuck tool suppresses the redundant stuck turn indicator for the same actor.

This is an observability buffer, not an audit log. Restarting with an empty buffer is safe.

## Authoritative Actor Working State

PTY hook state is accepted for actor status only when the current runtime session, the committed
launch identity, and the hook state agree on the runtime, process ID, and launch token. A daemon
restart, actor restart, non-hook provider command, corrupt identity, or late event from an older
process therefore fails closed to an unavailable/pending state instead of reusing stale
`working` or `idle` data.

The daemon derives the current state once per second and appends `actor.activity` only when an
actor's effective state changes or a previously running actor stops. The selected-group ledger
stream applies these events directly. The global event stream also refreshes the authoritative
actor list, and its polling fallback refreshes both groups and actors while the page is visible.
This keeps local and LAN clients consistent after SSE reconnects without turning hook files into a
network protocol.

## Privacy Boundary

Runtime activity never stores prompts, command lines, tool inputs, tool outputs, or notification
text. Tool names are reduced to a 64-character label containing only letters, numbers, `_`, `-`,
`.`, `:`, and `/`. Events that fail the provider launch/session/turn/operation fence are discarded
before entering the activity buffer. Hook-state acceptance and activity persistence share the same
per-actor critical section, preventing an older start event from being appended after a newer
terminal event. Ambiguous post-rename filesystem errors are verified against the committed JSON
before rollback, so hook state and activity do not diverge merely because directory sync or explicit
unlock reported a late error.

If the snapshot store is corrupt, the Web route returns
`runtime_activity_unavailable` instead of serving a partial or empty snapshot. The SSE stream keeps
its last accepted view and waits for a later valid store revision.

The channel does not grant or block tool execution, send operating-system notifications, or alter runtime permission policy.
