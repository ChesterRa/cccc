# CCCC Daemon API/IPC Contract v1

Status: Draft (for CCCC v0.4.x ecosystem)

This document defines the **daemon-facing client contract** for CCCC: how a client (CLI/Web/MCP bridge/SDK) discovers the daemon endpoint, frames requests, and calls daemon operations.

It is intentionally narrow:
- **CCCS v1** (`docs/standards/CCCS_V1.md`) defines the *semantic collaboration substrate* (event envelope + kinds + attention/ack).
- This document defines the *transport + RPC layer* used by CCCC today (newline-delimited JSON over a local socket/TCP).

## 0. Conformance Language

The key words **MUST**, **MUST NOT**, **SHOULD**, **SHOULD NOT**, and **MAY** in this document are to be interpreted as described in RFC 2119.

## 1. Goals and Non‑Goals

### 1.1 Goals

Daemon IPC v1 MUST provide:
- A **stable request/response envelope** with a **normative error shape** suitable for SDKs.
- A **cross-platform local transport** (Unix socket where available; TCP fallback).
- A **single-writer control plane** for group state, actors, messaging, inbox, and context.

### 1.2 Non‑Goals

Daemon IPC v1 does NOT standardize:
- Remote authentication/authorization or multi-tenant security.
- Any specific workflow engine or prompting strategy.
- A browser-friendly HTTP API surface (this document is socket/TCP oriented).

## 2. Terminology

- **CCCC_HOME**: The single global runtime home directory (default `~/.cccc/`).
- **Daemon**: The single-writer process that owns group state and appends to ledgers.
- **Client**: Any process calling daemon operations (CLI/Web/MCP/SDK).
- **Group / Actor / Scope / Ledger**: As defined in CCCS v1.
- **Principal (`by`)**: A string identity such as `"user"`, `"system"`, an `actor_id`, or a service principal.

## 3. Endpoint Discovery (Normative)

Clients MUST discover the daemon endpoint via a daemon-written descriptor file:

- Path: `${CCCC_HOME}/daemon/ccccd.addr.json`

`CCCC_HOME` resolution:
- If the `CCCC_HOME` environment variable is set, clients MUST use it as the base directory.
- Otherwise, clients MUST use the default `~/.cccc/`.

If the descriptor file is missing or invalid, a client MAY fall back to:

- Unix socket default: `${CCCC_HOME}/daemon/ccccd.sock` (only if AF_UNIX is supported)

### 3.1 `ccccd.addr.json` Schema

The daemon writes a JSON object with the following fields:

```json
{
  "v": 1,
  "transport": "unix",
  "path": "/home/alice/.cccc/daemon/ccccd.sock",
  "host": "",
  "port": 0,
  "pid": 12345,
  "version": "0.4.x",
  "ts": "2026-01-13T12:34:56Z"
}
```

Rules:
- `v` MUST be `1`.
- `transport` MUST be `"unix"` or `"tcp"`.
- If `transport == "unix"`, `path` MUST be a non-empty filesystem path.
- If `transport == "tcp"`, `host` MUST be a connectable host (typically `127.0.0.1`) and `port` MUST be a positive integer.
- Clients MUST treat unknown fields as ignorable metadata (but SHOULD preserve them if re-writing).

### 3.2 Daemon Runtime Files (Non-normative)

CCCC uses these files under `${CCCC_HOME}/daemon/`:
- `ccccd.addr.json`: endpoint descriptor (this spec)
- `ccccd.sock`: Unix socket path (POSIX default)
- `ccccd.pid`: daemon process id (best-effort)
- `ccccd.log`: daemon log file (best-effort)

### 3.3 Endpoint Configuration (Non-normative)

Daemon endpoint selection is controlled by environment variables:
- `CCCC_DAEMON_TRANSPORT`: `"unix"` or `"tcp"` (default: `"unix"` on POSIX, `"tcp"` on Windows)
- `CCCC_DAEMON_HOST`: bind host for TCP (default: `127.0.0.1`)
- `CCCC_DAEMON_PORT`: bind port for TCP (default: `0` meaning “choose a free port”)
- `CCCC_DAEMON_ALLOW_REMOTE`: when set truthy, allows binding to a non-loopback host (**dangerous**, no auth)

## 4. Transport and Framing (Normative)

### 4.1 Transport

Daemon IPC v1 uses a stream transport:
- Unix domain socket (`transport="unix"`) where available.
- TCP (`transport="tcp"`) for cross-platform fallback.

Security note: there is **no authentication** at this layer. TCP bindings MUST be treated as local-only unless an implementation explicitly accepts the risk.

### 4.2 Framing: NDJSON

For all non-streaming operations, requests and responses are framed as:
- **One JSON object per line**, delimited by a single `\n` (newline).
- Encoding MUST be UTF‑8.

Baseline behavior (implemented by CCCC v0.4.x):
- Each connection processes exactly **one** request line and produces exactly **one** response line.
- The daemon then closes the connection.

Clients MUST assume the daemon may close the connection after any successful response and MUST NOT rely on persistent connections.

Forward-compatible extension (not required for v1):
- A daemon MAY accept multiple request lines over a single connection (strictly serial, no pipelining).
- Clients MUST NOT pipeline requests (there is no request id / multiplexing in v1).

### 4.3 Size Limits

Implementations MUST respect practical line limits to avoid truncation:
- **Request line limit (daemon receive):** the daemon MAY stop reading after ~2,000,000 bytes without a newline; clients MUST keep request lines comfortably below this bound.
- **Response line limit (typical clients):** the reference client reader MAY cap a response line at ~4,000,000 bytes; daemons SHOULD keep single-response payloads below this bound.

Clients SHOULD treat truncated/invalid JSON as a transport failure.

### 4.4 Streaming Upgrade: `term_attach`

`term_attach` is a special operation that **upgrades the connection**:
1) Client sends a normal request line with `op="term_attach"`.
2) Daemon sends a normal response line.
3) If the response is `ok=true`, the connection becomes a raw **PTY stream** until closed.

After upgrade, the stream is **not** NDJSON.

The stream semantics are implementation-defined but, in CCCC today:
- The client receives raw PTY output bytes.
- The client MAY write raw bytes as input.
- The daemon MAY allow only one writer at a time (others become read-only).

Out-of-band control:
- Control operations (e.g., `term_resize`) MUST be performed over a separate concurrent daemon connection.

### 4.5 Streaming Upgrade: `events_stream` (Optional)

`events_stream` is an optional operation that upgrades the connection into a **push event stream** for reactive clients (Web/IDE/bots).

1) Client sends a normal request line with `op="events_stream"`.
2) Daemon sends a normal response line.
3) If the response is `ok=true`, the connection remains open and the daemon pushes NDJSON items indefinitely.

After upgrade, the stream is **NDJSON**, but it is no longer request/response: the daemon becomes the writer.

Stream item (recommended envelope):
```ts
type EventStreamItem =
  | { t: "event"; event: CCCSEventV1 }
  | { t: "heartbeat"; ts: string }
  | { t: string; [k: string]: unknown } // forward-compatible extension
```

Rules:
- Clients MUST ignore unknown `t` values.
- `heartbeat` items MUST NOT be appended to the group ledger; they are transport-level keepalives.
- Streams are best-effort: clients MUST tolerate disconnects, duplicates, and gaps (use `inbox_list` or a ledger read to reconcile).

## 5. Request/Response Envelope (Normative)

Daemon IPC v1 uses the envelope defined in `src/cccc/contracts/v1/ipc.py`.

### 5.1 Request

```ts
interface DaemonRequestV1 {
  v: 1
  op: string
  args?: Record<string, unknown> // default {}
}
```

Rules:
- `v` MUST be `1`.
- `op` MUST be a non-empty string (snake_case in CCCC v0.4.x).
- Clients MUST NOT send unknown top-level fields (the daemon is strict at the envelope level).

### 5.2 Response

```ts
interface DaemonResponseV1 {
  v: 1
  ok: boolean
  result: Record<string, unknown> // default {}
  error?: DaemonErrorV1 | null
}

interface DaemonErrorV1 {
  code: string
  message: string
  details: Record<string, unknown> // default {}
}
```

Rules:
- `v` MUST be `1`.
- If `ok == true`, `error` MUST be omitted or `null`.
- If `ok == false`, `error` MUST be present.
- Clients MUST NOT expect a stable schema for `result` beyond what each `op` specifies.

## 6. Error Model (Normative)

The error envelope shape in §5.2 is **normative**: daemons MUST return errors using this shape for all application-level failures.

### 6.1 Error Code Conventions

- `error.code` MUST be a stable, machine-readable token.
- `error.message` MUST be human-readable.
- `error.details` MUST be a JSON object (may be empty).
- The set of `error.code` values is an open set; clients MUST handle unknown codes gracefully.

Common codes used by CCCC v0.4.x include (non-exhaustive):
- `invalid_request`, `unknown_op`
- `missing_group_id`, `group_not_found`
- `missing_actor_id`, `actor_not_found`, `actor_not_running`
- `permission_denied`
- `invalid_patch`, `invalid_template`, `confirmation_required`

## 7. Operation Conventions

### 7.1 Identity and Permission Parameters

Many operations accept:
- `group_id`: target group identifier (string)
- `actor_id`: target actor identifier (string)
- `by`: principal string indicating who is acting (default varies by op)

Authorization is enforced by the daemon (see implementation in `src/cccc/kernel/permissions.py`).
Daemon IPC v1 has **no authentication**. The practical trust boundary is OS-level access control to the local socket / localhost port.

Local-trust model (CCCC v0.4.x behavior):
- If an operation accepts `args.by`, the daemon treats it as a caller-provided principal hint and uses it for attribution (ledger `event.by`) and permission checks.
- If `by` is omitted or blank, the daemon uses an operation-specific default (often `"user"`).

Security note:
- In a local-trust deployment, any process that can connect to the daemon can spoof `by`. Do not treat `by` as a security boundary.
- Remote/multi-tenant authentication is out of scope for v1.

### 7.2 Event Objects

Many operations return or include ledger events. Event envelopes follow the CCCC/CCCS v1 shape (see `src/cccc/contracts/v1/event.py` and `docs/standards/CCCS_V1.md`).

## 8. Operation Catalog (Normative for v1)

Unless otherwise stated:
- All operations use the request/response envelope in §5.
- All args live under `request.args`.
- All returned values live under `response.result`.

### 8.1 Core

#### `ping`

Args: none

Result:
```ts
{ version: string; pid: number; ts: string; ipc_v?: 1; capabilities?: Record<string, unknown> }
```

Notes:
- `ipc_v` is RECOMMENDED for SDK compatibility checks.
- `capabilities` is RECOMMENDED as a best-effort feature map (e.g., `{ "events_stream": true }`).

#### `shutdown`

Args: none

Result:
```ts
{ message: string } // "shutting down"
```

### 8.2 Observability (Global)

#### `observability_get`

Args: none

Result:
```ts
{ observability: Record<string, unknown> }
```

#### `observability_update`

Args:
```ts
{ by?: "user"; patch: Record<string, unknown> }
```

Result:
```ts
{ observability: Record<string, unknown> }
```

#### `debug_snapshot`

Developer-mode diagnostic snapshot (global + optional group context).

Args:
```ts
{ group_id?: string; by?: string }
```

Result:
```ts
{
  developer_mode: true
  observability: Record<string, unknown>
  daemon: { pid: number; version: string; ts: string }
  group?: { group_id: string; state: string; active_scope_key: string; title: string }
  actors?: Array<{ id: string; role: string; runtime: string; runner: string; runner_effective: string; enabled: boolean; running: boolean; unread_count: number }>
  delivery?: Record<string, unknown>
}
```

Notes:
- Requires developer mode.
- Permission is `user`, or `foreman` when `group_id` is provided.

### 8.3 Groups and Scopes

#### `attach`

Attach a directory scope to a group (or auto-create/select a group for this scope).

Args:
```ts
{ path: string; group_id?: string; by?: string }
```

Result:
```ts
{ group_id: string; scope_key: string; title?: string }
```

#### `groups`

List known groups (registry summaries).

Args: none

Result:
```ts
{ groups: Array<Record<string, unknown>> } // includes at least group_id/title/created_at/updated_at + running/state
```

#### `registry_reconcile`

Scan registry entries for missing/corrupt groups, and optionally remove missing entries.

Args:
```ts
{ remove_missing?: boolean }
```

Result:
```ts
{
  dry_run: boolean
  scanned_groups: number
  missing_group_ids: string[]
  corrupt_group_ids: string[]
  removed_group_ids: string[]
  removed_default_scope_keys: string[]
}
```

#### `group_show`

Args:
```ts
{ group_id: string }
```

Result:
```ts
{ group: Record<string, unknown> } // group.yaml content, redacted
```

#### `group_create`

Args:
```ts
{ title?: string; topic?: string; by?: string }
```

Result:
```ts
{ group_id: string; title?: string; event?: CCCSEventV1 }
```

#### `group_update`

Args:
```ts
{ group_id: string; by?: string; patch: { title?: string; topic?: string } }
```

Result:
```ts
{ group_id: string; group: Record<string, unknown>; event: CCCSEventV1 }
```

#### `group_delete`

Args:
```ts
{ group_id: string; by?: string }
```

Result:
```ts
{ group_id: string }
```

#### `group_use`

Set the active scope for a group using `path` (must already be attached).

Args:
```ts
{ group_id: string; path: string; by?: string }
```

Result:
```ts
{ group_id: string; active_scope_key: string; event: CCCSEventV1 }
```

#### `group_detach_scope`

Args:
```ts
{ group_id: string; scope_key: string; by?: string }
```

Result:
```ts
{ group_id: string; ruleset: { rules: Array<unknown>; snippets: Record<string, string> }; event: CCCSEventV1 }
```

#### `group_set_state`

Args:
```ts
{ group_id: string; state: "active" | "idle" | "paused"; by?: string }
```

Notes:
- `stopped` is not a valid `group_set_state` value in daemon IPC v1.
- Higher-level surfaces (CLI/MCP) MAY expose `stopped` as a convenience alias that maps to `group_stop`.

Result:
```ts
{ group_id: string; state: string; event: CCCSEventV1 }
```

#### `group_settings_update`

Update group-scoped messaging/automation/delivery/transcript settings.

Args:
```ts
{ group_id: string; by?: string; patch: Record<string, unknown> }
```

Patch keys used by CCCC v0.4.x include:
- Messaging: `default_send_to`
- Delivery: `min_interval_seconds`, `auto_mark_on_delivery`
- Automation: `nudge_after_seconds`, `reply_required_nudge_after_seconds`, `attention_ack_nudge_after_seconds`, `unread_nudge_after_seconds`, `nudge_digest_min_interval_seconds`, `nudge_max_repeats_per_obligation`, `nudge_escalate_after_repeats`, `actor_idle_timeout_seconds`, `keepalive_delay_seconds`, `keepalive_max_per_actor`, `silence_timeout_seconds`, `help_nudge_interval_seconds`, `help_nudge_min_messages`
- Terminal transcript: `terminal_transcript_visibility`, `terminal_transcript_notify_tail`, `terminal_transcript_notify_lines`

Result:
```ts
{ group_id: string; settings: Record<string, unknown>; event: CCCSEventV1 }
```

#### `group_automation_update`

Replace group automation rules + snippets (scheduled `system.notify`).

Args:
```ts
{
  group_id: string
  by?: string
  expected_version?: number
  ruleset: {
    rules: Array<{
      id: string
      enabled?: boolean
      scope?: "group" | "personal"
      owner_actor_id?: string | null
      to?: string[]
      trigger?:
        | { kind: "interval"; every_seconds: number }
        | { kind: "cron"; cron: string; timezone?: string }
        | { kind: "at"; at: string } // RFC3339
      action?: {
        kind?: "notify"
        title?: string
        snippet_ref?: string | null
        message?: string
        priority?: "low" | "normal" | "high" | "urgent"
        requires_ack?: boolean
      }
    }>
    snippets: Record<string, string>
  }
}
```

Result:
```ts
{ group_id: string; ruleset: Record<string, unknown>; version: number; event: CCCSEventV1 }
```

#### `group_automation_state`

Get effective automation state for a caller.

Args:
```ts
{ group_id: string; by?: string }
```

Result:
```ts
{
  group_id: string
  ruleset: {
    rules: Array<Record<string, unknown>>
    snippets: Record<string, string>
  }
  status: Record<string, {
    last_fired_at?: string
    last_error_at?: string
    last_error?: string
    next_fire_at?: string
  }>
  supported_vars: string[] // e.g. interval_minutes, group_title, actor_names, scheduled_at
  version: number
  server_now: string
  config_path: string
}
```

Notes:
- `by` as a peer receives a filtered view: group rules + own personal rules.

#### `group_automation_manage`

Incremental automation management with action list.

Args:
```ts
{
  group_id: string
  by?: string
  expected_version?: number
  actions: Array<
    | { type: "create_rule"; rule: Record<string, unknown> }
    | { type: "update_rule"; rule: Record<string, unknown> }
    | { type: "set_rule_enabled"; rule_id: string; enabled: boolean }
    | { type: "delete_rule"; rule_id: string }
    | { type: "replace_all_rules"; ruleset: { rules: Array<Record<string, unknown>>; snippets: Record<string, string> } }
  >
}
```

Result:
```ts
{
  group_id: string
  ruleset: Record<string, unknown>
  status: Record<string, Record<string, string>>
  supported_vars: string[]
  version: number
  server_now: string
  applied_actions: Array<Record<string, unknown>>
  changed: boolean
  event?: CCCSEventV1 | null
}
```

#### `group_automation_reset_baseline`

Reset automation ruleset to built-in baseline defaults.

Args:
```ts
{ group_id: string; by?: string; expected_version?: number }
```

Result:
```ts
{
  group_id: string
  ruleset: { rules: Array<Record<string, unknown>>; snippets: Record<string, string> }
  status: Record<string, Record<string, unknown>>
  supported_vars: string[]
  version: number
  server_now: string
  config_path: string
  event: CCCSEventV1
}
```

#### `group_start`

Start (enable + run) all actors in the group.

Args:
```ts
{ group_id: string; by?: string }
```

Result:
```ts
{ group_id: string; started: string[]; forced_headless?: string[]; event: CCCSEventV1 }
```

#### `group_stop`

Stop (disable + terminate) all actors in the group.

Args:
```ts
{ group_id: string; by?: string }
```

Result:
```ts
{ group_id: string; stopped: string[]; event: CCCSEventV1 }
```

### 8.4 Actors

#### `actor_list`

Args:
```ts
{ group_id: string; include_unread?: boolean }
```

Result:
```ts
{ actors: Array<Record<string, unknown>> } // includes at least id/title/runner/runtime/enabled + role/running
```

#### `actor_add`

Args:
```ts
{
  group_id: string
  actor_id?: string
  title?: string
  runtime?: string
  runner?: "pty" | "headless"
  command?: string[]
  env?: Record<string, string>
  env_private?: Record<string, string> // write-only secrets (stored under CCCC_HOME/state; never persisted into ledger)
  default_scope_key?: string
  submit?: "enter" | "newline" | "none"
  by?: string
}
```

Notes:
- `env_private` is restricted to `by="user"` and values are never returned.
- If `env_private` is provided (even empty), it is treated as authoritative for this create: it clears any existing private keys for that actor_id, then sets the provided keys.

Result:
```ts
{ actor: Record<string, unknown>; event: CCCSEventV1 }
```

#### `actor_update`

Args:
```ts
{ group_id: string; actor_id: string; by?: string; patch: Record<string, unknown> }
```

Patch keys used by CCCC v0.4.x include:
- Identity/UI: `title`
- Runtime: `runtime`, `runner`, `command`, `submit`
- Scope: `default_scope_key`
- Enable/disable: `enabled`
- Environment (use with care): `env`

Result:
```ts
{ actor: Record<string, unknown>; event: CCCSEventV1 }
```

#### `actor_remove`

Args:
```ts
{ group_id: string; actor_id: string; by?: string }
```

Result:
```ts
{ actor_id: string; event: CCCSEventV1 }
```

#### `actor_start` / `actor_stop` / `actor_restart`

Args:
```ts
{ group_id: string; actor_id: string; by?: string }
```

Result:
```ts
{ actor: Record<string, unknown>; event: CCCSEventV1 }
```

#### `actor_env_private_keys`

List configured **private** env keys for an actor (keys only; never returns values).

Notes:
- Private env is **runtime-only** and MUST NOT be persisted into the append-only group ledger.
- Intended for secrets like API keys/tokens that may vary per actor.
- Effective env at process start is: `daemon_env` (inherited) → `actor.env` → `private_env` → injected `CCCC_GROUP_ID`/`CCCC_ACTOR_ID`.
- This operation is restricted to `by="user"` (agents should not be able to read/inspect secrets metadata).

Args:
```ts
{ group_id: string; actor_id: string; by?: string }
```

Result:
```ts
{ group_id: string; actor_id: string; keys: string[] }
```

#### `actor_env_private_update`

Update an actor's private env map (set/unset/clear). Values are **never** returned.

Args:
```ts
{
  group_id: string
  actor_id: string
  by?: string
  set?: Record<string, string>  // set/overwrite keys
  unset?: string[]              // remove keys
  clear?: boolean               // remove all keys (wins)
}
```

Result:
```ts
{ group_id: string; actor_id: string; keys: string[] }
```

### 8.5 Chat Messaging

#### `send`

Append a `chat.message` event to the group ledger and trigger best-effort delivery to running actors.

Args (core):
```ts
{
  group_id: string
  text: string
  by?: string
  to?: string[]                 // recipient tokens (empty = broadcast)
  priority?: "normal" | "attention"
  path?: string                 // optional filesystem path to attribute scope_key
  attachments?: unknown[]       // attachment refs (implementation-defined)
  src_group_id?: string         // relay provenance (both required if either is set)
  src_event_id?: string
  dst_group_id?: string         // optional "send record" metadata (source messages)
  dst_to?: string[]
}
```

Result:
```ts
{ event: CCCSEventV1 } // kind="chat.message"
```

#### `reply`

Append a `chat.message` with `reply_to` and `quote_text`.

Args:
```ts
{
  group_id: string
  reply_to: string
  text: string
  by?: string
  to?: string[]                 // defaults to original sender if omitted
  priority?: "normal" | "attention"
  attachments?: unknown[]
}
```

Result:
```ts
{ event: CCCSEventV1 } // kind="chat.message"
```

#### `send_cross_group`

Cross-group send implemented as:
1) Write a source `chat.message` in the origin group (with `dst_group_id` / `dst_to` metadata).
2) Write a forwarded `chat.message` in the destination group with `src_group_id` / `src_event_id` provenance.

Args:
```ts
{ group_id: string; dst_group_id: string; text: string; by?: string; to?: string[]; priority?: "normal" | "attention" }
```

Result:
```ts
{ src_event: CCCSEventV1; dst_event: CCCSEventV1 }
```

Notes:
- Attachments are not supported in cross-group send in v1.

#### `chat_ack`

Append a `chat.ack` event (attention acknowledgement).

Args:
```ts
{ group_id: string; actor_id: string; event_id: string; by?: string }
```

Result:
```ts
{ acked: boolean; already: boolean; event: CCCSEventV1 | null }
```

### 8.6 Inbox (Read Cursor)

#### `inbox_list`

Return unread `chat.message` and/or `system.notify` events for an actor based on its read cursor.

Args:
```ts
{ group_id: string; actor_id: string; by?: string; limit?: number; kind_filter?: "all" | "chat" | "notify" }
```

Result:
```ts
{ messages: CCCSEventV1[]; cursor: { event_id: string; ts: string } }
```

#### `inbox_mark_read`

Advance the actor read cursor to at least `event_id` and append a `chat.read` event.

Args:
```ts
{ group_id: string; actor_id: string; event_id: string; by?: string }
```

Result:
```ts
{ cursor: { event_id: string; ts: string; updated_at: string }; event: CCCSEventV1 }
```

#### `inbox_mark_all_read`

Advance the actor read cursor to the latest currently-unread event (for the chosen kind filter) and append a `chat.read` event.

Args:
```ts
{ group_id: string; actor_id: string; by?: string; kind_filter?: "all" | "chat" | "notify" }
```

Result:
```ts
{ cursor: { event_id: string; ts: string; updated_at: string }; event: CCCSEventV1 | null }
```

### 8.7 Context and Tasks

#### `context_get`

Args:
```ts
{ group_id: string }
```

Result: implementation-defined JSON summary (vision/sketch/milestones/notes/references/tasks/presence).

#### `context_sync`

Args:
```ts
{ group_id: string; by?: string; ops: Array<Record<string, unknown>>; dry_run?: boolean }
```

Operation item shape (normative minimum):
```ts
type ContextOpV1 = { op: string } & Record<string, unknown>
```

Notes:
- Unknown op names SHOULD be rejected.
- See `docs/standards/CCCC_CONTEXT_OPS_V1.md` for the v1 operation list.

Result:
```ts
{ success: true; dry_run: boolean; changes: Array<Record<string, unknown>>; version: string }
```

#### `task_list`

Args:
```ts
{ group_id: string; task_id?: string }
```

Result:
```ts
{ tasks?: Array<Record<string, unknown>>; task?: Record<string, unknown> }
```

#### `presence_get`

Args:
```ts
{ group_id: string }
```

Result:
```ts
{ agents: Array<{ id: string; status: string; updated_at: string }>; heartbeat_timeout_seconds: number }
```

### 8.8 Headless Runner

#### `headless_status`

Args:
```ts
{ group_id: string; actor_id: string }
```

Result:
```ts
{ state: Record<string, unknown> } // see src/cccc/contracts/v1/actor.py HeadlessState
```

#### `headless_set_status`

Args:
```ts
{ group_id: string; actor_id: string; status: "idle" | "working" | "waiting" | "stopped"; task_id?: string | null }
```

Result:
```ts
{ state: Record<string, unknown> | null }
```

#### `headless_ack_message`

Args:
```ts
{ group_id: string; actor_id: string; message_id: string }
```

Result:
```ts
{ message_id: string; acked_at: string }
```

### 8.9 System Notifications (Not Chat)

#### `system_notify`

Args:
```ts
{
  group_id: string
  by?: string
  kind?: string
  priority?: "low" | "normal" | "high" | "urgent"
  title?: string
  message?: string
  target_actor_id?: string | null
  requires_ack?: boolean
  context?: Record<string, unknown>
}
```

Result:
```ts
{ event: CCCSEventV1 } // kind="system.notify"
```

#### `notify_ack`

Args:
```ts
{ group_id: string; actor_id: string; notify_event_id: string; by?: string }
```

Result:
```ts
{ event: CCCSEventV1 } // kind="system.notify_ack"
```

### 8.10 Terminal Diagnostics and PTY Attach

#### `terminal_tail`

Args:
```ts
{ group_id: string; actor_id: string; by?: string; max_chars?: number; strip_ansi?: boolean; compact?: boolean }
```

Result:
```ts
{ group_id: string; actor_id: string; warning: string; hint: string; text: string }
```

#### `terminal_clear`

Args:
```ts
{ group_id: string; actor_id: string; by?: string }
```

Result:
```ts
{ group_id: string; actor_id: string; cleared: true }
```

#### `debug_tail_logs`

Tail daemon/web/im-bridge log files (developer mode).

Args:
```ts
{ component: "daemon" | "ccccd" | "web" | "im" | "im_bridge"; group_id?: string; by?: string; lines?: number }
```

Result:
```ts
{ component: string; group_id: string; path: string; lines: string[] }
```

#### `debug_clear_logs`

Truncate daemon/web/im-bridge log files (developer mode).

Args:
```ts
{ component: "daemon" | "ccccd" | "web" | "im" | "im_bridge"; group_id?: string; by?: string }
```

Result:
```ts
{ component: string; group_id: string; path: string; cleared: true }
```

#### `term_resize`

Args:
```ts
{ group_id: string; actor_id: string; cols: number; rows: number }
```

Result:
```ts
{ group_id: string; actor_id: string; cols: number; rows: number }
```

#### `term_attach` (streaming upgrade)

Args:
```ts
{ group_id: string; actor_id: string }
```

Result (handshake):
```ts
{ group_id: string; actor_id: string }
```

After a successful handshake, the connection becomes a raw PTY stream (see §4.4).

Notes:
- `term_resize` MUST be sent over a separate daemon connection (the PTY stream is not NDJSON).

### 8.11 Ledger Maintenance

#### `ledger_snapshot`

Args:
```ts
{ group_id: string; by?: string; reason?: string }
```

Result:
```ts
{ snapshot: Record<string, unknown> }
```

#### `ledger_compact`

Args:
```ts
{ group_id: string; by?: string; reason?: string; force?: boolean }
```

Result: implementation-defined compaction report.

### 8.12 Group Templates

Templates use the portable schema in `src/cccc/contracts/v1/group_template.py`.

#### `group_template_export`

Args:
```ts
{ group_id: string }
```

Result:
```ts
{ template: string; filename: string } // YAML text
```

#### `group_template_preview`

Args:
```ts
{ group_id: string; by?: string; template: string }
```

Result:
```ts
{ template: Record<string, unknown>; diff: Record<string, unknown> }
```

#### `group_template_import_replace`

Destructive replace of actors/settings/group prompt overrides (does not delete ledger history).

Args:
```ts
{ group_id: string; by?: string; confirm: string; template: string }
```

Rules:
- `confirm` MUST equal `group_id` (prevents accidental destructive import).

Result:
```ts
{ group_id: string; applied: true; removed: string[]; added: string[]; updated: string[]; settings_patch: Record<string, unknown>; prompt_paths: string[] }
```

#### `group_create_from_template`

Create a new group attached to `path`, then apply a template (no confirmation).

Args:
```ts
{ path: string; by?: string; title?: string; topic?: string; template: string }
```

Result:
```ts
{ group_id: string; applied: true }
```

### 8.13 Event Streaming (Optional)

#### `events_stream`

Subscribe to new ledger events for a group.

Args:
```ts
{
  group_id: string
  by?: string
  since_event_id?: string | null  // resume strictly after this event (preferred)
  since_ts?: string | null        // best-effort resume using timestamps
  kinds?: string[] | null         // optional kind allowlist (exact match)
}
```

Handshake result:
```ts
{ group_id: string }
```

Streaming mode:
- The daemon pushes NDJSON `EventStreamItem` lines (see §4.5).
- A daemon may initially emit only a subset of event kinds. CCCC v0.4.x streams these kinds:
  - `chat.message`, `chat.ack`, `system.notify`, `system.notify_ack`
- When `kinds` is provided, only matching event kinds SHOULD be emitted.
- If `by` identifies an `actor_id`, a daemon MAY apply the same visibility rules as inbox delivery (e.g., only deliver `chat.message`/`system.notify` addressed to that actor and exclude the actor’s own `chat.message` events).
- Resume (`since_event_id` / `since_ts`) is best-effort in v1; clients MUST be able to reconcile using `inbox_list`.
- The stream ends when the client closes the connection or the daemon exits.
- To protect daemon responsiveness, a daemon MAY drop slow subscribers (clients SHOULD reconnect and reconcile).

## 9. Appendix: Example Lines

### 9.1 Ping

Request line:
```json
{"v":1,"op":"ping","args":{}}
```

Response line:
```json
{"v":1,"ok":true,"result":{"version":"0.4.x","pid":12345,"ts":"2026-01-13T12:34:56Z","ipc_v":1,"capabilities":{"events_stream":true}},"error":null}
```

### 9.2 Error

```json
{"v":1,"ok":false,"result":{},"error":{"code":"missing_group_id","message":"missing group_id","details":{}}}
```
