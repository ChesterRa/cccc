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

#### `capability_search`

Search capability registry records (built-in packs + local curated catalog + cached remote records).

Args:
```ts
{
  group_id: string
  actor_id?: string
  by?: string
  query?: string
  kind?: "mcp_toolpack" | "skill" | ""
  source_id?: string
  trust_tier?: string
  qualification_status?: "qualified" | "unavailable" | "blocked" | ""
  include_external?: boolean
  limit?: number
}
```

Result:
```ts
{
  group_id: string
  actor_id?: string
  default_profile: "core"
  items: Array<{
    capability_id: string
    kind: "mcp_toolpack" | "skill"
    name: string
    description_short: string
    source_id: string
    source_tier: string
    source_uri?: string
    trust_tier: string
    license?: string
    qualification_status: "qualified" | "unavailable" | "blocked"
    sync_state?: string
    enabled: boolean
    enable_supported: boolean
    install_mode?: string
    policy_level?: "indexed" | "mounted" | "enabled" | "pinned"
    enable_hint?: "enable_now" | "blocked" | "unsupported"
    blocked_reason?: string
    tags?: string[]
    tool_count?: number
    tool_names?: string[]
  }>
  count: number
  sources: Record<string, unknown>
  applied_filters: {
    kind: string
    source_id: string
    trust_tier: string
    qualification_status: string
  }
  search_diagnostics?: {
    remote_augmented: boolean
    remote_added: number
    remote_error?: string
    policy_hidden_count?: number
  }
}
```

#### `capability_enable`

Enable or disable a capability by scope.

Notes:

1. Built-in capability packs (`pack:*`) are directly enable-able and can change MCP exposure.
2. Skills (`kind=skill`) use the same `capability_enable` op for pin/unpin and can auto-apply
   declared dependencies.
3. External MCP execution path is constrained to supported installers (`remote_only` and npm package via `npx`);
   unsupported install metadata returns `state=failed`.

Args:
```ts
{
  group_id: string
  capability_id: string
  scope?: "group" | "actor" | "session"   // default: session
  enabled?: boolean                         // default: true
  cleanup?: boolean                         // default: false; disable path can also clean runtime cache
  reason?: string                           // optional short audit reason
  ttl_seconds?: number                      // session scope only
  by?: string
  actor_id?: string
}
```

Result:
```ts
{
  action_id: string
  group_id: string
  actor_id: string
  capability_id: string
  scope: "group" | "actor" | "session"
  enabled: boolean
  state: "ready" | "failed"
  refresh_required: boolean
  refresh_mode?: "relist_or_reconnect"
  wait?: "relist_or_reconnect"
  reason?: string
  policy_level?: "indexed" | "mounted" | "enabled" | "pinned"
  removed_binding_count?: number
  removed_installation?: boolean
  cleanup_skipped_reason?: string
  skill?: {
    capability_id: string
    name: string
    description_short?: string
    capsule?: string
    requires_capabilities?: string[]
    applied_dependencies?: string[]
    skipped_dependencies?: Array<{ capability_id: string; reason: string }>
    source_id?: string
    source_uri?: string
  }
}
```

Quota notes:

1. `CCCC_CAPABILITY_MAX_ENABLED_PER_ACTOR` (default `12`) limits actor/session enabled capability count.
2. `CCCC_CAPABILITY_MAX_ENABLED_PER_GROUP` (default `24`) limits group-scope enabled capability count.
3. `CCCC_CAPABILITY_MAX_INSTALLATIONS_TOTAL` (default `128`) limits total cached external artifacts.
4. Quota failures return `ok=true` with `state="failed"` and deterministic `reason` code.

#### `capability_block`

Block/unblock capabilities at runtime.

Notes:

1. `scope=group`: foreman or user can block/unblock.
2. `scope=global`: only user can block/unblock.
3. Blocking revokes enabled bindings and runtime dynamic tool exposure immediately.

Args:
```ts
{
  group_id: string
  capability_id: string
  scope?: "group" | "global" // default: group
  blocked?: boolean           // default: true
  ttl_seconds?: number        // 0 means no expiry
  reason?: string
  by?: string
  actor_id?: string
}
```

Result:
```ts
{
  action_id: string
  group_id: string
  actor_id: string
  capability_id: string
  scope: "group" | "global"
  blocked: boolean
  state: "ready"
  removed_bindings: number
  removed_runtime_bindings: number
  refresh_required: boolean
  refresh_mode?: "relist_or_reconnect"
  wait?: "relist_or_reconnect"
  block?: {
    reason?: string
    by?: string
    blocked_at?: string
    expires_at?: string
  }
}
```

#### `capability_state`

Read effective capability exposure and visible MCP tool names for caller scope.

Args:
```ts
{ group_id: string; actor_id?: string; by?: string }
```

Result:
```ts
{
  group_id: string
  actor_id: string
  default_profile: "core"
  core_tool_count: number
  visible_tool_count: number
  visible_tools: string[]
  dynamic_tools?: Array<{
    name: string
    description?: string
    inputSchema: Record<string, unknown>
    capability_id: string
    real_tool_name: string
  }>
  dynamic_tool_limit: number
  dynamic_tool_dropped: number
  enabled_capabilities: string[]
  active_skills?: Array<{
    capability_id: string
    name: string
    description_short?: string
    source_id?: string
    source_uri?: string
    policy_level?: "indexed" | "mounted" | "enabled" | "pinned"
  }>
  pinned_skills?: Array<{
    capability_id: string
    name: string
    description_short?: string
    source_id?: string
    policy_level?: "indexed" | "mounted" | "enabled" | "pinned"
  }>
  hidden_capabilities: Array<{
    capability_id: string
    reason: string
    policy_level?: "indexed" | "mounted" | "enabled" | "pinned"
    state?: string
  }>
  external_binding_states?: Record<string, {
    mode: "mcp" | "skill"
    state: string
    artifact_id?: string
    last_error?: string
  }>
  precedence_chain: ["session", "actor", "group"]
  session_bindings: Array<{
    capability_id: string
    expires_at: string
    ttl_seconds: number
  }>
  source_states: Record<string, unknown>
  blocked_capabilities?: Array<{
    capability_id: string
    scope: "group" | "global"
    reason?: string
    by?: string
    blocked_at?: string
    expires_at?: string
  }>
  is_foreman: boolean
}
```

Operational notes:

1. Capability catalog is daemon-owned local state seeded from allowlist and runtime discoveries.
2. Search uses local curated catalog + cached remote results; no periodic capability sync loop.
3. Source gates:
   - `CCCC_CAPABILITY_SOURCE_MCP_REGISTRY_ENABLED` (default `1`)
   - `CCCC_CAPABILITY_SOURCE_ANTHROPIC_SKILLS_ENABLED` (default `1`)
   - `CCCC_CAPABILITY_SOURCE_AGENTSKILLS_VALIDATOR_ENABLED` (default `1`)
   - `github_skills_curated` is allowlist-curated (no periodic source crawler).
   - `github_skills_remote` is on-demand remote search (GitHub `SKILL.md` candidates).
   - `agentskills_remote` is on-demand AgentSkills-aligned remote search.
   - `skillsmp_remote` is on-demand SkillsMP remote search.
   - `clawhub_remote` is on-demand ClawHub remote search.
4. Dynamic tool exposure is capped by `CCCC_CAPABILITY_MAX_DYNAMIC_TOOLS_VISIBLE`
   (default `32`).
5. Catalog snapshot size is capped by `CCCC_CAPABILITY_CATALOG_MAX_RECORDS`
   (default `20000`); prune is applied during explicit sync operations.
6. Search may perform remote augmentation (MCP + skill) when local hits are insufficient:
   - `CCCC_CAPABILITY_SEARCH_REMOTE_FALLBACK` (default `1`)
   - `CCCC_CAPABILITY_SEARCH_REMOTE_FALLBACK_LIMIT` (default `40`, max `100`)
   - `CCCC_CAPABILITY_SOURCE_GITHUB_SKILLS_REMOTE_ENABLED` (default `1`)
   - `CCCC_CAPABILITY_SOURCE_AGENTSKILLS_REMOTE_ENABLED` (default `1`)
   - `CCCC_CAPABILITY_SOURCE_SKILLSMP_REMOTE_ENABLED` (default `1`)
   - `CCCC_CAPABILITY_SOURCE_CLAWHUB_REMOTE_ENABLED` (default `1`)
   - `CCCC_CAPABILITY_SEARCH_REMOTE_SKILL_LIMIT` (default follows remote fallback limit)
   - `CCCC_CAPABILITY_SEARCH_REMOTE_AGENTSKILLS_LIMIT` (default follows remote fallback limit)
   - `CCCC_CAPABILITY_SEARCH_REMOTE_SKILLSMP_LIMIT` (default follows remote fallback limit)
   - `CCCC_CAPABILITY_SEARCH_REMOTE_CLAWHUB_LIMIT` (default follows remote fallback limit)
   - `CCCC_CAPABILITY_SKILLSMP_PROXY_BASE` (default `https://r.jina.ai/http://skillsmp.com/search`)
   - `CCCC_CAPABILITY_CLAWHUB_PROXY_BASE` (default `https://r.jina.ai/http://clawhub.ai/skills?focus=search`)
7. Allowlist override env/path compatibility (`CCCC_CAPABILITY_ALLOWLIST_PATH` and
   `CCCC_HOME/config/capability-allowlist.yaml`) is removed. Policy now always uses:
   - packaged default: `cccc.resources/capability-allowlist.default.yaml`
   - user overlay: `CCCC_HOME/config/capability-allowlist.user.yaml`
   - effective policy: deterministic merge (`default <- overlay`).

#### `capability_allowlist_get`

Read allowlist default/overlay/effective snapshots and revision hash.

Args:
```ts
{ by?: string } // write ops still enforce by=user; read is open
```

Result:
```ts
{
  default: Record<string, unknown>
  overlay: Record<string, unknown>
  effective: Record<string, unknown>
  revision: string
  default_source: string
  overlay_source: string
  overlay_error: string
  policy_source: string
  policy_error: string
}
```

#### `capability_allowlist_validate`

Dry-run allowlist overlay validation (no persistence).

Args:
```ts
{
  mode?: "patch" | "replace" // default: patch
  patch?: Record<string, unknown>   // required when mode=patch
  overlay?: Record<string, unknown> // required when mode=replace
}
```

Result:
```ts
{
  valid: boolean
  reason: string
  default: Record<string, unknown>
  overlay: Record<string, unknown>
  effective: Record<string, unknown>
  revision: string
}
```

#### `capability_allowlist_update`

Persist allowlist overlay with optimistic concurrency.

Args:
```ts
{
  by?: string // must be "user"
  mode?: "patch" | "replace" // default: patch
  expected_revision?: string
  patch?: Record<string, unknown>   // required when mode=patch
  overlay?: Record<string, unknown> // required when mode=replace
}
```

Result:
```ts
{
  updated: true
  revision: string
  default: Record<string, unknown>
  overlay: Record<string, unknown>
  effective: Record<string, unknown>
  policy_source: string
  policy_error: string
}
```

Errors:
- `allowlist_revision_mismatch`
- `allowlist_validation_failed`

#### `capability_allowlist_reset`

Reset overlay to empty (removes `CCCC_HOME/config/capability-allowlist.user.yaml` when present).

Args:
```ts
{ by?: string } // must be "user"
```

Result:
```ts
{
  reset: true
  removed_overlay_file: boolean
  revision: string
  default: Record<string, unknown>
  overlay: Record<string, unknown>
  effective: Record<string, unknown>
  default_source: string
  overlay_source: string
  overlay_error: string
  policy_source: string
  policy_error: string
}
```

#### `capability_uninstall`

Remove capability installation cache and revoke all bindings for a group.

Args:
```ts
{
  group_id: string
  capability_id: string
  reason?: string
  by?: string
  actor_id?: string
}
```

Result:
```ts
{
  action_id: string
  group_id: string
  actor_id: string
  capability_id: string
  state: "ready"
  removed_bindings: number
  removed_installation: boolean
  removed_runtime_bindings?: number
  refresh_required: boolean
  refresh_mode?: "relist_or_reconnect"
  wait?: "relist_or_reconnect"
}
```

#### `capability_tool_call`

Invoke an enabled dynamic external capability tool by synthetic tool name.

Args:
```ts
{
  group_id: string
  actor_id?: string
  by?: string
  tool_name: string
  arguments?: Record<string, unknown>
}
```

Result:
```ts
{
  tool_name: string
  capability_id: string
  result: Record<string, unknown>
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
  profile_id?: string            // optional Actor Profile link (runtime/runner/command/submit/env + secrets)
  default_scope_key?: string
  submit?: "enter" | "newline" | "none"
  by?: string
}
```

Notes:
- `env_private` is restricted to `by="user"` and values are never returned.
- If `env_private` is provided (even empty), it is treated as authoritative for this create: it clears any existing private keys for that actor_id, then sets the provided keys.
- `profile_id` links the actor to a global Actor Profile and applies profile-controlled runtime fields + profile secrets.
- When `profile_id` is used, `env_private` is rejected (linked actor private env is profile-controlled).

Result:
```ts
{ actor: Record<string, unknown>; event: CCCSEventV1 }
```

#### `actor_update`

Args:
```ts
{
  group_id: string
  actor_id: string
  by?: string
  patch: Record<string, unknown>
  profile_id?: string                      // attach/replace profile link
  profile_action?: "convert_to_custom"     // snapshot profile config + secrets, then unlink
}
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

Notes:
- For linked actors (`profile_id` set), `actor_start` and `actor_restart` first resolve profile runtime config and profile secrets.
- If the linked profile includes `capability_defaults`, daemon applies baseline capability enables through capability control plane before launch.

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

### 8.5 Actor Profiles (Global)

Actor Profiles are global reusable runtime profiles stored under `CCCC_HOME/state/actor_profiles/`.
They are not group-local settings.

#### `actor_profile_list`

Args:
```ts
{ by?: string }
```

Result:
```ts
{ profiles: Array<Record<string, unknown>> } // each profile includes usage_count
```

#### `actor_profile_get`

Args:
```ts
{ profile_id: string; by?: string }
```

Result:
```ts
{
  profile: Record<string, unknown>
  usage: Array<{
    group_id: string
    group_title?: string
    actor_id: string
    actor_title?: string
  }>
}
```

#### `actor_profile_upsert`

Create/update a profile with optimistic concurrency.

Args:
```ts
{
  by?: string
  profile: {
    id?: string
    name: string
    runtime: string
    runner: "pty" | "headless"
    command?: string[] | string
    submit?: "enter" | "newline" | "none"
    env?: Record<string, string> // deprecated legacy input; values are migrated into profile secrets
    capability_defaults?: {
      pinned_capabilities?: string[]
      default_scope?: "actor" | "session" // default actor
      session_ttl_seconds?: number         // clamped to 60..86400
    } | null
  }
  expected_revision?: number
}
```

Notes:
- Runtime variables are unified as profile secrets (`actor_profile_secret_*`).
- `profile.env` is accepted only as a legacy bridge and migrated into profile secrets; stored profile `env` is kept empty.

Result:
```ts
{ profile: Record<string, unknown> }
```

#### `actor_profile_delete`

Args:
```ts
{ profile_id: string; by?: string; force_detach?: boolean }
```

Notes:
- Default behavior rejects delete when the profile is still used by linked actors (`profile_in_use`).
- With `force_detach: true`, linked actors are converted to custom first, then the profile is deleted.

Result:
```ts
{
  deleted: true
  profile_id: string
  detached_count: number
  detached: Array<{ group_id: string; actor_id: string }>
}
```

#### `actor_profile_secret_keys`

List profile secret keys (masked previews only).

Args:
```ts
{ profile_id: string; by?: string }
```

Result:
```ts
{ profile_id: string; keys: string[]; masked_values: Record<string, string> }
```

#### `actor_profile_secret_update`

Update profile-level secrets (write-only values).

Args:
```ts
{
  profile_id: string
  by?: string
  set?: Record<string, string>
  unset?: string[]
  clear?: boolean
}
```

Result:
```ts
{ profile_id: string; keys: string[] }
```

#### `actor_profile_secret_copy_from_actor`

Copy an actor's current private env map into a profile's secrets (server-side copy, values are never returned).

Args:
```ts
{
  profile_id: string
  group_id: string
  actor_id: string
  by?: string
}
```

Result:
```ts
{ profile_id: string; group_id: string; actor_id: string; keys: string[] }
```

#### `actor_profile_secret_copy_from_profile`

Copy one profile's current secret map into another profile (server-side copy, values are never returned).

Args:
```ts
{
  profile_id: string
  source_profile_id: string
  by?: string
}
```

Result:
```ts
{ profile_id: string; source_profile_id: string; keys: string[] }
```

### 8.6 Chat Messaging

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

### 8.7 Inbox (Read Cursor)

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

### 8.8 Context and Tasks

#### `context_get`

Args:
```ts
{ group_id: string }
```

Result:
```ts
{
  version: string
  vision?: string
  overview: {
    manual: {
      roles: string[]
      collaboration_mode: string
      current_focus: string
      updated_by: string
      updated_at: string
    }
    mermaid: string
  }
  tasks_summary: {
    total: number
    done: number
    active: number
    planned: number
    root_count: number
  }
  active_tasks: Array<Record<string, unknown>>
  presence: {
    agents: Array<{
      id: string
      active_task_id?: string | null
      focus: string
      blockers: string[]
      next_action: string
      what_changed: string
      decision_delta: string
      environment: string
      user_profile: string
      notes: string
      updated_at: string
    }>
  }
}
```

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
- See `docs/standards/CCCC_CONTEXT_OPS_V1.md` for the v2 operation list.

Result:
```ts
{
  success: true
  dry_run: boolean
  changes: Array<Record<string, unknown>>
  version: string
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
}
```

#### `memory_store`

Args:
```ts
{
  group_id: string
  id?: string           // memory ID for update mode (omit for create)
  content?: string      // required for create, optional for update
  kind?: string
  status?: string
  confidence?: string
  source_type?: string
  source_ref?: string
  scope_key?: string
  actor_id?: string
  task_id?: string
  event_ts?: string
  tags?: string[]
  strategy?: string     // "aggressive" | "conservative" | "task-completion"
  solidify?: boolean    // immediately solidify after store/update
}
```

Result (create): `{ id, content_hash, created_at, status, deduplicated }`
Result (update): `{ memory: Record<string, unknown>, updated: true }`

#### `memory_search`

Args:
```ts
{
  group_id: string
  query?: string        // FTS5 full-text query
  status?: string
  kind?: string
  actor_id?: string
  task_id?: string
  confidence?: string
  tags?: string[]
  since?: string        // ISO 8601
  until?: string        // ISO 8601
  track_hit?: boolean   // default false; when true increments hit_count and may auto-solidify
  limit?: number        // 1-100, default 20
}
```

Result: `{ memories: Array<Record<string, unknown>>, count: number }`

#### `memory_stats`

Args:
```ts
{ group_id: string }
```

Result: `{ total, by_status, by_kind, tag_count, relation_count }`

#### `memory_ingest`

Args:
```ts
{
  group_id: string
  mode?: "signal" | "raw"    // default "signal"
  limit?: number             // ledger lines to read (1-200, default 50)
  actor_id?: string          // filter by actor (raw mode only)
  reset_watermark?: boolean  // re-process previously ingested events
}
```

Result (signal mode): `{ signals: Array<{actor_id, messages_count, suggested_kind, key_phrases, time_range, topic}>, events_processed, watermark, mode }`
Result (raw mode): `{ imported, skipped, watermark, mode }`

#### `memory_solidify_batch`

Args:
```ts
{
  group_id: string
  task_id?: string       // filter by task
  kind?: string          // filter by kind
}
```

Result: `{ solidified: number, ids: string[] }`

#### `memory_export`

Args:
```ts
{
  group_id: string
  include_draft?: boolean  // include draft memories (default: false)
  output_dir?: string      // output directory override
}
```

Result: `{ manifest: { group_id, sha256, memory_count, exported_at, format }, md_path: string, manifest_path: string }`

#### `memory_delete`

Args:
```ts
{
  group_id: string
  id?: string     // single memory ID
  ids?: string[]  // batch memory IDs
}
```

Result: `{ deleted: boolean, deleted_count: number, ids: string[] }`

#### `memory_decay`

Find stale memory candidates for cleanup/decay (safe: read-only, no auto-delete).

Args:
```ts
{
  group_id: string
  draft_days?: number        // default 30
  zero_hit_days?: number     // default 14
  solid_review_days?: number // default 120
  solid_max_hit?: number     // default 1
  limit?: number             // default 100
}
```

Result:
```ts
{
  candidates: Array<{
    id: string
    content_preview: string
    kind: string
    status: "draft" | "solid"
    hit_count: number
    created_at: string
    last_recalled_at: string
    age_days: number
    reasons: string[]
    recommended_action: "delete_candidate" | "review_candidate"
    priority: "low" | "medium" | "high"
    score: number
  }>
  count: number
  delete_candidate_count: number
  review_candidate_count: number
  config: {
    draft_days: number
    zero_hit_days: number
    solid_review_days: number
    solid_max_hit: number
    limit: number
  }
}
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

`presence_get` has been removed in v2. Presence/agent state is returned in `context_get.result.presence.agents`.

### 8.9 Headless Runner

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

### 8.10 System Notifications (Not Chat)

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

### 8.11 Terminal Diagnostics and PTY Attach

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

### 8.12 Ledger Maintenance

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

### 8.13 Group Templates

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

### 8.14 Event Streaming (Optional)

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

### 8.15 IM Authentication

#### `im_bind_chat`

Bind a pending one-time key to authorize an IM chat. On success the chat is also auto-subscribed for outbound message delivery.

Args:
```ts
{ group_id: string; key: string }
```

Result:
```ts
{ chat_id: string; thread_id: number; platform: string }
```

Errors:
- `missing_key` – `key` is empty.
- `missing_group_id` – `group_id` is empty.
- `group_not_found` – group does not exist.
- `invalid_key` – key not found or expired.

#### `im_list_authorized`

List all authorized IM chats for a group.

Args:
```ts
{ group_id: string }
```

Result:
```ts
{ authorized: Array<Record<string, unknown>> }
```

Errors:
- `missing_group_id` – `group_id` is empty.
- `group_not_found` – group does not exist.

#### `im_list_pending`

List pending one-time bind requests for a group (expired keys are omitted).

Args:
```ts
{ group_id: string }
```

Result:
```ts
{
  pending: Array<{
    key: string
    chat_id: string
    thread_id: number
    platform: string
    created_at: number
    expires_at: number
    expires_in_seconds: number
  }>
}
```

Errors:
- `missing_group_id` – `group_id` is empty.
- `group_not_found` – group does not exist.

#### `im_reject_pending`

Reject a pending one-time bind key.

Args:
```ts
{ group_id: string; key: string }
```

Result:
```ts
{ rejected: boolean } // idempotent: false when key is already absent/expired
```

Errors:
- `missing_key` – `key` is empty.
- `missing_group_id` – `group_id` is empty.
- `group_not_found` – group does not exist.

#### `im_revoke_chat`

Revoke authorization for an IM chat.

Args:
```ts
{ group_id: string; chat_id: string; thread_id?: number }
```

Result:
```ts
{ revoked: boolean; unsubscribed?: boolean }
```

Notes:
- `thread_id` defaults to `0` if omitted or invalid.

Errors:
- `missing_chat_id` – `chat_id` is empty.
- `missing_group_id` – `group_id` is empty.
- `group_not_found` – group does not exist.

### 8.16 Remote Access (Contract-Gated)

These operations are optional extensions for productized remote-access control.
Deployments without this feature MAY return `unknown_op`.

#### `remote_access_state`

Read global remote-access state.

Args:
```ts
{ by?: string }
```

Result:
```ts
{
  remote_access: {
    provider: "off" | "manual" | "tailscale"
    mode: string
    enforce_web_token: boolean
    enabled: boolean
    status: "stopped" | "running" | "not_installed" | "not_authenticated" | "misconfigured" | "error"
    endpoint?: string | null
    updated_at?: string | null
    diagnostics?: {
      web_token_present?: boolean
      web_token_source?: "settings" | "env" | "none" | string
      web_host?: string
      web_host_source?: "settings" | "env" | "default" | string
      web_port?: number
      web_port_source?: "settings" | "env" | "default" | string
      web_public_url?: string | null
      web_public_url_source?: "settings" | "env" | "none" | string
      web_bind_loopback?: boolean
      web_bind_reachable?: boolean
      mode_supported?: boolean
      tailscale_installed?: boolean | null
      tailscale_backend_state?: string | null
      [k: string]: unknown
    }
    config?: {
      web_host?: string
      web_port?: number
      web_public_url?: string | null
      web_token_configured?: boolean
      web_token_source?: "settings" | "env" | "none" | string
      [k: string]: unknown
    }
    next_steps?: string[]
  }
}
```

#### `remote_access_configure`

Update global remote-access configuration.

Args:
```ts
{
  by?: string
  provider?: "off" | "manual" | "tailscale"
  mode?: string
  enforce_web_token?: boolean
  web_host?: string
  web_port?: number
  web_public_url?: string
  web_token?: string
  clear_web_token?: boolean
}
```

Result:
```ts
{ remote_access: Record<string, unknown> }
```

#### `remote_access_start`

Start remote access according to configured provider/mode.

Args:
```ts
{ by?: string }
```

Result:
```ts
{ remote_access: Record<string, unknown> }
```

#### `remote_access_stop`

Stop remote access service.

Args:
```ts
{ by?: string }
```

Result:
```ts
{ remote_access: Record<string, unknown> }
```

### 8.17 Group Space (Provider-Backed Shared Memory, M1-lite)

These operations provide a thin control-plane for optional external memory providers.
Provider failures MUST NOT block core collaboration flows (chat/context/actors).

#### `group_space_status`

Read provider mode, group binding, queue summary, and repo `space/` sync state.

Args:
```ts
{ group_id: string; provider?: "notebooklm" }
```

Result:
```ts
{
  group_id: string
  provider: {
    provider: "notebooklm"
    enabled: boolean
    mode: "disabled" | "active" | "degraded"
    real_adapter_enabled?: boolean
    stub_adapter_enabled?: boolean
    auth_configured?: boolean
    write_ready?: boolean
    readiness_reason?: string
    last_health_at?: string | null
    last_error?: string | null
  }
  binding: {
    group_id: string
    provider: "notebooklm"
    remote_space_id: string
    bound_by: string
    bound_at: string
    status: "bound" | "unbound" | "error"
  }
  queue_summary: { pending: number; running: number; failed: number }
  sync?: {
    available?: boolean
    reason?: string
    space_root?: string
    remote_space_id?: string
    last_run_at?: string
    converged?: boolean
    unsynced_count?: number
    last_error?: string
  }
}
```

#### `group_space_spaces`

List available remote notebooks/spaces for provider selection UI.

Args:
```ts
{ group_id: string; provider?: "notebooklm" }
```

Result:
```ts
{
  group_id: string
  provider: "notebooklm"
  provider_state: Record<string, unknown>
  binding: Record<string, unknown>
  spaces: Array<{
    remote_space_id: string
    title?: string
    created_at?: string
    is_owner?: boolean
  }>
}
```

#### `group_space_capabilities`

Return Group Space capability matrix for current group/provider.

Args:
```ts
{
  group_id: string
  provider?: "notebooklm"
}
```

Result:
```ts
{
  group_id: string
  provider: "notebooklm"
  local_scope_attached: boolean
  space_root: string
  local_file_policy: {
    allowed_extensions: string[]
    max_file_size_bytes: number
    unsupported_error_code: string
    oversize_error_code: string
  }
  ingest: {
    kinds: Array<"context_sync" | "resource_ingest">
    resource_ingest: {
      source_types: string[]
      required_fields: Record<string, string[]>
      optional_fields: Record<string, string[]>
      aliases: Record<string, string>
      examples: Record<string, Record<string, unknown>>
    }
  }
  query: {
    options: {
      source_ids: string
    }
    unsupported_options: Record<string, string>
    examples: Record<string, Record<string, unknown>>
  }
  artifacts: {
    actions: string[]
    kinds: string[]
    options: Record<string, string>
    aliases: Record<string, string>
    examples: Record<string, Record<string, unknown>>
  }
  notes: string[]
}
```

#### `group_space_bind`

Bind/unbind a group to provider remote space.
When `action=bind` and `remote_space_id` is empty, daemon may auto-create
a provider notebook and bind it.

Args:
```ts
{
  group_id: string
  provider?: "notebooklm"
  action?: "bind" | "unbind"
  remote_space_id?: string
  by?: string
}
```

Result:
```ts
{
  group_id: string
  provider: Record<string, unknown>
  binding: Record<string, unknown>
  queue_summary: { pending: number; running: number; failed: number }
  sync?: Record<string, unknown>
  sync_result?: Record<string, unknown>
}
```

#### `group_space_ingest`

Create (or dedupe) an ingest job and execute it with bounded retry policy.

Args:
```ts
{
  group_id: string
  provider?: "notebooklm"
  kind?: "context_sync" | "resource_ingest"
  payload?: Record<string, unknown>
  idempotency_key?: string
  by?: string
}
```

Result:
```ts
{
  group_id: string
  job_id: string
  accepted: true
  deduped: boolean
  job: Record<string, unknown>
  queue_summary: { pending: number; running: number; failed: number }
  provider_mode: "disabled" | "active" | "degraded"
}
```

#### `group_space_query`

Query provider-backed memory for a group. If provider is degraded, result MAY return
`ok=true` with `degraded=true` and an empty answer.

Args:
```ts
{
  group_id: string
  provider?: "notebooklm"
  query: string
  options?: {
    source_ids?: string[] // optional remote source_id filter
  }
}
```

Validation notes:
- `options` only supports `source_ids`.
- `options.language` / `options.lang` are invalid for `group_space_query` because NotebookLM query API does not provide a language parameter.

Result:
```ts
{
  group_id: string
  provider: "notebooklm"
  provider_mode: "disabled" | "active" | "degraded"
  degraded: boolean
  answer: string
  references: unknown[]
  error?: { code: string; message: string } | null
}
```

#### `group_space_sources`

List/refresh/rename/delete provider sources in the currently bound notebook.

Args:
```ts
{
  group_id: string
  provider?: "notebooklm"
  action?: "list" | "refresh" | "rename" | "delete"
  source_id?: string // required for refresh/rename/delete
  new_title?: string // required for rename
  by?: string
}
```

Result (`action=list`):
```ts
{
  group_id: string
  provider: "notebooklm"
  provider_mode: "disabled" | "active" | "degraded"
  binding: Record<string, unknown>
  action: "list"
  sources: Record<string, unknown>[]
  list_result: Record<string, unknown>
}
```

Result (`action=refresh` | `rename` | `delete`):
```ts
{
  group_id: string
  provider: "notebooklm"
  provider_mode: "disabled" | "active" | "degraded"
  binding: Record<string, unknown>
  action: "refresh" | "rename" | "delete"
  source_id: string
  refresh_result?: Record<string, unknown>
  rename_result?: Record<string, unknown>
  delete_result?: Record<string, unknown>
}
```

#### `group_space_artifact`

List/generate/download provider artifacts (NotebookLM studio outputs).
For `action=generate`, daemon can optionally wait for completion and auto-save
the artifact into local `repo/space/artifacts/...`.

Args:
```ts
{
  group_id: string
  provider?: "notebooklm"
  action?: "list" | "generate" | "download"
  kind?: "audio" | "video" | "report" | "study_guide" | "quiz" | "flashcards" | "infographic" | "slide_deck" | "data_table" | "mind_map"
  options?: Record<string, unknown> // for action=generate
  wait?: boolean // action=generate only
  save_to_space?: boolean // generate/download auto-save behavior
  output_path?: string // optional local path override
  output_format?: "json" | "markdown" | "html" // quiz/flashcards
  artifact_id?: string // optional explicit download target
  timeout_seconds?: number // generate+wait only
  initial_interval?: number // generate+wait only
  max_interval?: number // generate+wait only
  by?: string
}
```

Result (`action=list`):
```ts
{
  group_id: string
  provider: "notebooklm"
  provider_mode: "disabled" | "active" | "degraded"
  binding: Record<string, unknown>
  action: "list"
  kind?: string
  artifacts: Record<string, unknown>[]
  list_result: Record<string, unknown>
}
```

Result (`action=generate`):
```ts
{
  group_id: string
  provider: "notebooklm"
  provider_mode: "disabled" | "active" | "degraded"
  binding: Record<string, unknown>
  action: "generate"
  kind: string
  task_id?: string
  status?: string
  wait: boolean
  saved_to_space: boolean
  output_path?: string
  generate_result: Record<string, unknown>
  wait_result?: Record<string, unknown>
  download_result?: Record<string, unknown>
}
```

Result (`action=download`):
```ts
{
  group_id: string
  provider: "notebooklm"
  provider_mode: "disabled" | "active" | "degraded"
  binding: Record<string, unknown>
  action: "download"
  kind: string
  saved_to_space: boolean
  output_path: string
  download_result: Record<string, unknown>
}
```

#### `group_space_jobs`

List/retry/cancel Group Space jobs.

Args:
```ts
{
  group_id: string
  provider?: "notebooklm"
  action?: "list" | "retry" | "cancel"
  job_id?: string
  state?: "pending" | "running" | "succeeded" | "failed" | "canceled"
  limit?: number
  by?: string
}
```

#### `group_space_sync`

Run/read `repo/space/` reconciliation status for the bound provider notebook.

Args:
```ts
{
  group_id: string
  provider?: "notebooklm"
  action?: "status" | "run"
  force?: boolean
  by?: string
}
```

Result (`action=status`):
```ts
{
  group_id: string
  provider: "notebooklm"
  sync: Record<string, unknown>
}
```

Result (`action=run`):
```ts
{
  group_id: string
  provider: "notebooklm"
  sync: Record<string, unknown>
  sync_result: Record<string, unknown>
}
```

Result (`action=list`):
```ts
{
  group_id: string
  provider: "notebooklm"
  jobs: Record<string, unknown>[]
  queue_summary: { pending: number; running: number; failed: number }
}
```

Result (`action=retry|cancel`):
```ts
{
  group_id: string
  provider: "notebooklm"
  job: Record<string, unknown>
  queue_summary: { pending: number; running: number; failed: number }
}
```

#### `group_space_provider_credential_status`

Read provider credential status (masked metadata only, no secret values).

Args:
```ts
{
  provider?: "notebooklm"
  by?: string // user-only
}
```

Result:
```ts
{
  provider: "notebooklm"
  credential: {
    provider: "notebooklm"
    key: string
    configured: boolean
    source: "none" | "store" | "env"
    env_configured: boolean
    store_configured: boolean
    updated_at?: string | null
    masked_value?: string | null
  }
}
```

#### `group_space_provider_credential_update`

Update or clear provider credentials in the daemon secret store.

Args:
```ts
{
  provider?: "notebooklm"
  by?: string // user-only
  auth_json?: string
  clear?: boolean
}
```

Notes:
- `clear=true` removes stored credentials for this provider.
- `auth_json` is write-only and never returned in response payloads.
- Environment credential (`CCCC_NOTEBOOKLM_AUTH_JSON`) has higher precedence than stored credentials.

Result:
```ts
{
  provider: "notebooklm"
  credential: {
    provider: "notebooklm"
    key: string
    configured: boolean
    source: "none" | "store" | "env"
    env_configured: boolean
    store_configured: boolean
    updated_at?: string | null
    masked_value?: string | null
  }
}
```

#### `group_space_provider_health_check`

Run provider health check and update provider state (`active`/`degraded`/`disabled`) accordingly.

Args:
```ts
{
  provider?: "notebooklm"
  by?: string // user-only
}
```

Result:
```ts
{
  provider: "notebooklm"
  healthy: boolean
  health?: Record<string, unknown>
  error?: { code: string; message: string }
  provider_state: Record<string, unknown>
  credential: {
    provider: "notebooklm"
    key: string
    configured: boolean
    source: "none" | "store" | "env"
    env_configured: boolean
    store_configured: boolean
    updated_at?: string | null
    masked_value?: string | null
  }
}
```

#### `group_space_provider_auth`

Control provider auth flow (`status`/`start`/`cancel`) for backend-managed
NotebookLM sign-in.

Args:
```ts
{
  provider?: "notebooklm"
  action?: "status" | "start" | "cancel"
  timeout_seconds?: number
  by?: string // user-only
}
```

Result:
```ts
{
  provider: "notebooklm"
  provider_state: Record<string, unknown>
  credential: {
    provider: "notebooklm"
    key: string
    configured: boolean
    source: "none" | "store" | "env"
    env_configured: boolean
    store_configured: boolean
    updated_at?: string | null
    masked_value?: string | null
  }
  auth: {
    provider: "notebooklm"
    state: "idle" | "running" | "succeeded" | "failed" | "canceled"
    phase?: string
    session_id?: string
    started_at?: string
    updated_at?: string
    finished_at?: string
    message?: string
    error?: { code: string; message: string } | Record<string, unknown>
  }
}
```

Notes:
- `start` may open a browser on the daemon host for Google sign-in.
- Provider write readiness remains gated by `auth_configured` and runtime mode.

## 9. Appendix: Example Lines

### 9.1 Ping

Request line:
```json
{"v":1,"op":"ping","args":{}}
```

Response line:
```json
{"v":1,"ok":true,"result":{"version":"0.4.x","pid":12345,"ts":"2026-01-13T12:34:56Z","ipc_v":1,"capabilities":{"events_stream":true,"remote_access":true}},"error":null}
```

### 9.2 Error

```json
{"v":1,"ok":false,"result":{},"error":{"code":"missing_group_id","message":"missing group_id","details":{}}}
```
