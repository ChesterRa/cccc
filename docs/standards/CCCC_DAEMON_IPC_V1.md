# CCCC Daemon API/IPC Contract v1

Status: Draft (for CCCC v0.5.x ecosystem)

This document defines the **daemon-facing client contract** for CCCC: how a client (CLI/Web/MCP bridge/SDK) discovers the daemon endpoint, frames requests, and calls daemon operations.

It is intentionally narrow:
- **CCCS v1** (`docs/standards/CCCS_V1.md`) defines the *semantic collaboration substrate* (event envelope + kinds + delivery/read/reply facts).
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

The native daemon rejects every non-loopback TCP host before binding. Daemon IPC
has no authentication and cannot be exposed with `0.0.0.0`, a LAN address, or a
public address. Use the authenticated Web API for remote access.

## 4. Transport and Framing (Normative)

### 4.1 Transport

Daemon IPC v1 uses a stream transport:
- Unix domain socket (`transport="unix"`) where available.
- TCP (`transport="tcp"`) for cross-platform fallback.

Security note: there is **no authentication** at this layer. TCP bindings MUST remain loopback-only.

### 4.2 Framing: NDJSON

For all non-streaming operations, requests and responses are framed as:
- **One JSON object per line**, delimited by a single `\n` (newline).
- Encoding MUST be UTF‑8.

Baseline behavior:
- A daemon MAY accept multiple request lines on one connection or close the connection after any response.
- When multiple requests are accepted, they are processed strictly serially and produce one response line each.
- Clients MUST NOT pipeline requests (there is no request id / multiplexing in v1).
- Clients MUST tolerate the daemon closing a connection after any response and reconnect through endpoint discovery before the next request. The reference non-streaming client opens a fresh local connection for each call because v1 has no request id with which to make a close-versus-write race safely replayable.

### 4.3 Size Limits

Implementations MUST respect practical line limits to avoid truncation:
- **Request line limit (daemon receive):** the native daemon stops reading after 16 MiB without a newline; clients MUST keep request lines below this bound.
- **Response line limit (typical clients):** the reference client reader MAY cap a response line at ~4,000,000 bytes; daemons SHOULD keep single-response payloads below this bound.

Clients SHOULD treat truncated/invalid JSON as a transport failure. Once any request bytes
have been written, clients MUST NOT automatically replay the request after a send, read, or
decode failure unless the operation carries a daemon-enforced idempotency key. Retrying a
failure that occurred while establishing the connection is safe because no request was sent.

### 4.4 Streaming Upgrade: `term_attach`

`term_attach` is a special operation that **upgrades the connection**:
1) Client sends a normal request line with `op="term_attach"`.
2) Daemon sends a normal response line.
3) If the response is `ok=true`, the connection becomes a **terminal stream** until closed.

After upgrade, the stream is **not** NDJSON.

The stream semantics are implementation-defined but, in CCCC today:
- By default, the client receives raw PTY output bytes.
- A client that requests `bootstrap="snapshot_v1"` can receive one negotiated ANSI screen
  snapshot first, followed by raw PTY bytes after the snapshot's raw cursor fence.
- The client MAY write raw bytes as input.
- The daemon MAY allow only one writer at a time (others become read-only).
- A daemon MAY close an attachment that falls behind its bounded retained-output
  window. A reconnecting client SHOULD resume from its last fully consumed byte
  cursor using `since`; the handshake clamps an expired cursor to retained history.

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
- Streams are best-effort: clients MUST tolerate disconnects, duplicates, and gaps (use `inbox_peek` or a ledger read to reconcile).

### 4.6 Streaming Upgrade: `presentation_browser_attach` / VNC attach (Optional)

`presentation_browser_attach` upgrades the connection into a daemon-local **browser-surface control stream** for a slot-scoped Presentation browser session.
The same stream envelope is reused by other projected browser attach operations, including provider-auth and Web Model browser surfaces.

1) Client sends a normal request line with `op="presentation_browser_attach"`.
2) Daemon sends a normal response line.
3) If the response is `ok=true`, the connection remains open and becomes a bidirectional NDJSON stream.

After upgrade:
- The daemon pushes `state` and `frame` items for the active browser surface session.
- The client MAY send browser-control commands such as navigation, click, scroll, key, text, resize, and close.
- Only one active controller MAY be attached at a time for a given slot browser surface session.
- If a matching `*_vnc_attach` operation succeeds, the connection upgrades into a raw RFB/VNC byte stream instead of NDJSON. VNC attach is an optional viewer transport; browser control and delivery semantics remain owned by the daemon runtime.

Recommended daemon-to-client items (CCCC v0.4.x behavior):
```ts
type PresentationBrowserStreamItem =
  | {
      t: "state"
      active: boolean
      state: "starting" | "ready" | "failed" | "closed" | "idle"
      message: string
      error?: Record<string, unknown>
      strategy?: string
      url?: string
      width?: number
      height?: number
      started_at?: string
      updated_at?: string
      last_frame_seq?: number
      last_frame_at?: string
      controller_attached?: boolean
    }
  | {
      t: "frame"
      seq: number
      captured_at: string
      mime: "image/jpeg"
      data_base64: string
      width: number
      height: number
      url: string
    }
  | {
      t: "error"
      code: string
      message: string
    }
```

Recommended client-to-daemon commands (CCCC v0.4.x behavior):
```ts
type PresentationBrowserCommand =
  | { t: "ping" }
  | { t: "navigate"; url: string }
  | { t: "back" }
  | { t: "refresh" }
  | { t: "click"; x: number; y: number; button?: "left" | "middle" | "right" }
  | { t: "scroll"; dx?: number; dy?: number }
  | { t: "key"; key: string }
  | { t: "text"; text: string }
  | { t: "resize"; width: number; height: number }
  | { t: "close" | "disconnect" }
```

Rules:
- Clients MUST treat unknown `t` values as ignorable forward-compatible extensions.
- The browser-surface stream is best-effort and ephemeral; clients MUST be able to reconnect and recover via `presentation_browser_info` / `presentation_browser_open`.
- The stream is daemon-local runtime state and MUST NOT be treated as persisted Presentation card state.

## 5. Request/Response Envelope (Normative)

Daemon IPC v1 uses the envelope defined in `crates/cccc-contracts/src/ipc.rs`.

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
- `missing_actor_id`, `actor_not_found`, `actor_not_running`, `not_pty_actor`
- `permission_denied`
- `invalid_patch`, `invalid_template`, `confirmation_required`

## 7. Operation Conventions

### 7.1 Identity and Permission Parameters

Many operations accept:
- `group_id`: target group identifier (string)
- `actor_id`: target actor identifier (string)
- `by`: principal string indicating who is acting (default varies by op)

Authorization is enforced by the daemon (see `crates/cccc-core/src/permissions.rs`
and the operation-level checks in `crates/cccc-daemon/src/ops/`).
Daemon IPC v1 has **no authentication**. The practical trust boundary is OS-level access control to the local socket / localhost port.

Local-trust model (CCCC v0.4.x behavior):
- If an operation accepts `args.by`, the daemon treats it as a caller-provided principal hint and uses it for attribution (ledger `event.by`) and permission checks.
- If `by` is omitted or blank, the daemon uses an operation-specific default (often `"user"`).

Security note:
- In a local-trust deployment, any process that can connect to the daemon can spoof `by`. Do not treat `by` as a security boundary.
- Remote/multi-tenant authentication is out of scope for v1.

### 7.2 Event Objects

Many operations return or include ledger events. Event envelopes follow the
CCCC/CCCS v1 shape (see `crates/cccc-contracts/src/event.rs` and
`docs/standards/CCCS_V1.md`).

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
{
  version: string;
  implementation: "rust";
  pid: number;
  ts: string;
  ipc_v: 1;
  capabilities: Record<string, unknown>;
  compatibility?: string;
}
```

Notes:
- SDK-compatible daemons MUST return `ipc_v: 1`; omitting it is interpreted as IPC version `0`.
- The bundled daemon MUST identify its implementation as `rust`.
- `compatibility`, when present, is an implementation-specific compatibility identity; clients MUST NOT infer compatibility from the implementation name alone.
- SDK-compatible daemons MUST return a `capabilities` feature map. The native
  daemon advertises supported `events_stream`, `remote_access`, optional
  browser-attach operations, and optional terminal-attach extensions here.
- Each optional browser stream is advertised under its exact operation name (`presentation_browser_attach`, `presentation_browser_vnc_attach`, `space_provider_auth_browser_attach`, `space_provider_auth_browser_vnc_attach`, `web_model_browser_attach`, or `web_model_browser_vnc_attach`). `true` means the daemon recognizes that streaming upgrade; `false` means callers MUST use another product surface or treat the operation as unavailable.
- A product implementation MAY serve an equivalent ephemeral browser surface directly through its local Web port. That does not make the daemon IPC upgrade supported: the exact daemon capability MUST remain `false` unless that daemon recognizes and serves the operation itself.
- `term_attachment_status=true` means `term_attach` returns a positive
  `attachment_id` and the daemon implements writer-ownership status checks for
  that ID. `term_attach_snapshot_v1=true` means callers may request
  `bootstrap="snapshot_v1"` and receive `initial_output` metadata. Clients MUST
  retain the baseline replay-stream behavior when either extension is false.
- Clients SHOULD probe operation support independently; a recognized operation may reject empty probe arguments, but MUST NOT return `unknown_op`.
- Clients MUST NOT probe an unadvertised browser attach operation merely to discover support: a successful probe upgrades the connection and may acquire the only controller. They SHOULD consult the exact capability first.
- Clients MUST use protocol, compatibility, and capability fields instead of exact product-version equality.
- Ordinary business commands MUST NOT stop, signal, or replace a reachable daemon. Implementation replacement is restricted to explicit daemon lifecycle commands.

#### `shutdown`

Args:
```ts
{ expected_pid?: number }
```

When `expected_pid` is present, it MUST be a positive integer matching the
receiving daemon's current process ID. A mismatch MUST return
`daemon_owner_mismatch` and MUST NOT begin shutdown. This optional fence lets a
lifecycle owner avoid stopping a replacement daemon after an IPC descriptor
handoff. Omitting `expected_pid` preserves the normal administrator shutdown
behavior.

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

#### `branding_get`

Args: none

Result:
```ts
{
  branding: {
    product_name: string
    logo_icon_asset_path?: string
    favicon_asset_path?: string
    updated_at?: string
  }
}
```

#### `branding_update`

Args:
```ts
{ by?: "user"; patch: Record<string, unknown> }
```

Result:
```ts
{
  branding: {
    product_name: string
    logo_icon_asset_path?: string
    favicon_asset_path?: string
    updated_at?: string
  }
}
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

#### `capability_overview`

Return a global capability library snapshot for Settings/Policy views (no actor scope required).

Args:
```ts
{
  query?: string
  limit?: number          // default 400, max 2000
  include_indexed?: boolean // default true
}
```

Result:
```ts
{
  items: Array<{
    capability_id: string
    kind: "mcp_toolpack" | "skill" | ""
    name: string
    description_short?: string
    source_id?: string
    source_uri?: string
    source_tier?: string
    trust_tier?: string
    license?: string
    sync_state?: string
    policy_level: "indexed" | "mounted" | "enabled" | "pinned"
    policy_visible: boolean
    blocked_global: boolean
    blocked_reason?: string
    enable_supported: boolean
    qualification_status: "qualified" | "unavailable" | "blocked"
    qualification_reasons?: string[] // currently exposed for agent_self_proposed skill management
    capsule_text?: string            // currently exposed for agent_self_proposed skill management
    install_mode?: string
    autoload_candidate: boolean
    tags?: string[]
    tool_count?: number
    tool_names?: string[]
    cached_install_state?: string
    cached_install_error_code?: string
    cached_install_error?: string
    recent_success?: {
      success_count: number
      last_success_at?: string
      last_group_id?: string
      last_actor_id?: string
      last_action?: string
    }
  }>
  count: number
  query: string
  sources: Record<string, {
    source_id: string
    enabled: boolean
    source_level: "indexed" | "mounted" | "enabled" | "pinned"
    rationale?: string
    sync_state: string
    last_synced_at?: string
    staleness_seconds: number
    record_count: number
    error?: string
  }>
  blocked_capabilities: Array<{
    capability_id: string
    scope: "global"
    reason?: string
    by?: string
    blocked_at?: string
    expires_at?: string
  }>
  allowlist_revision: string
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
    enable_hint?: "enable_now" | "blocked" | "unsupported" | "active"
    blocked_reason?: string
    readiness_preview?: {
      preview_status: "blocked" | "enableable" | "active" | "needs_inspect"
      next_step: string
      already_active?: boolean
    }
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
2. Skills (`kind=skill`) use the same `capability_enable` op for activate/deactivate and can auto-apply
   declared dependencies.
3. External MCP execution path supports `remote_only`, `package`, and `command`.
4. `package` mode supports npm (`npx`), pypi (`uvx`/`pipx`), OCI (`docker`/`podman`) and can fall back to
   command candidates when package metadata is incomplete.
5. External enable runs preflight first (required env, runtime binary availability, remote URL sanity) and returns
   `reason=preflight_failed:<code>` on deterministic blockers.
6. `activation_pending` means relist/reconnect is still required; `runnable` means binding is live enough to try; `verified` is reserved for post-call proof, not plain enable.

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
  state: "activation_pending" | "runnable" | "blocked" | "disabled"
  refresh_required: boolean
  refresh_mode?: "relist_or_reconnect"
  wait?: "relist_or_reconnect"
  reason?: string
  error?: string
  retryable?: boolean
  install_error_code?: string
  required_env?: string[]
  missing_binaries?: string[]
  policy_level?: "indexed" | "mounted" | "enabled" | "pinned"
  install_state?: "installed" | "installed_degraded" | "install_failed"
  degraded?: boolean
  degraded_reason?: string
  degraded_call_hint?: string
  fallback_from?: "package"
  fallback_reason?: string
  preflight?: {
    ok: boolean
    code: string
    message: string
    required_env?: string[]
    missing_binaries?: string[]
  }
  diagnostics?: Array<{
    code: string
    message: string
    retryable?: boolean
    required_env?: string[]
    action_hints?: string[]
  }>
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

1. `CCCC_CAPABILITY_MAX_ENABLED_PER_ACTOR` (default `20`) limits actor/session enabled non-skill capability count.
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
  state: "blocked" | "unblocked"
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
{
  group_id: string
  actor_id?: string
  by?: string
  capability_id?: string // optional; returns capability_usage for this id
  view?: "mcp_catalog" // reserved for actor-scoped MCP tool discovery
}
```

The `mcp_catalog` view is read from the latest atomic Group snapshot without acquiring the
outer Group lifecycle lock. Managed runtimes can initialize their actor-scoped CCCC MCP server
while `actor_start` is still materializing the provider, so serializing this internal catalog
read behind that same lifecycle lock would deadlock provider startup. All other
`capability_state` reads retain normal Group read serialization; capability-store updates keep
their own locking and are not relaxed by this view.

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
  active_capsule_skills?: Array<{
    capability_id: string
    name: string
    description_short?: string
    capsule_preview?: string
    capsule_text?: string
    source_id?: string
    source_uri?: string
    policy_level?: "indexed" | "mounted" | "enabled" | "pinned"
    activation_sources?: Array<{
      scope: "group" | "actor" | "session"
      actor_id?: string
      expires_at?: string
      ttl_seconds?: number
    }>
  }>
  autoload_skills?: Array<{
    capability_id: string
    name: string
    description_short?: string
    capsule_preview?: string
    capsule_text?: string
    source_id?: string
    policy_level?: "indexed" | "mounted" | "enabled" | "pinned"
  }>
  autoload_capabilities?: string[]
  actor_autoload_capabilities?: string[]
  profile_autoload_capabilities?: string[]
  actor_hidden_capabilities?: string[] // actor-level UI/menu hide preferences, including Web user slash menu; does not disable the capability
  hidden_capabilities: Array<{
    capability_id: string
    reason: string
    name?: string
    description_short?: string
    kind?: "mcp_toolpack" | "skill"
    source_id?: string
    policy_level?: "indexed" | "mounted" | "enabled" | "pinned" | "blocked"
    state?: string
    install_error_code?: string
    install_error?: string
  }>
  external_binding_states?: Record<string, {
    mode: "mcp" | "skill"
    state: string
    install_state?: string
    artifact_id?: string
    last_error?: string
    last_error_code?: string
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
  capability_usage?: {
    capability_id: string
    used: boolean
    group_enabled: boolean
    group_actor_count: number
    actor_enabled: Array<{ actor_id: string; actor_title?: string; label?: string }>
    session_enabled: Array<{ actor_id: string; actor_title?: string; label?: string; expires_at: string; ttl_seconds: number }>
    actor_autoload: Array<{ actor_id: string; actor_title?: string; label?: string }>
    profile_autoload: Array<{ actor_id: string; actor_title?: string; label?: string; profile_id?: string; profile_name?: string }>
    blocked: boolean
    blocked_scope?: "group" | "global"
    blocked_reason?: string
  }
  is_foreman: boolean
}
```

Operational notes:

1. Capability catalog is daemon-owned local state seeded from allowlist and runtime discoveries.
2. Search uses local curated catalog + cached remote results; no periodic capability sync loop.
3. Source gates:
   - `CCCC_CAPABILITY_SOURCE_MCP_REGISTRY_ENABLED` (default `1`)
   - `CCCC_CAPABILITY_SOURCE_ANTHROPIC_SKILLS_ENABLED` (default `1`)
   - `github_skills_curated` is allowlist-curated (no periodic source crawler).
   - `agent_self_proposed` is for agent-generated procedural skill candidates; default policy keeps MCP
     toolpacks indexed while allowing capsule skills to be validated and enabled at narrow scope.
   - `skillsmp_remote` is on-demand SkillsMP remote search (API key mode + proxy fallback).
   - `clawhub_remote` is on-demand ClawHub remote search (official API).
   - `openclaw_skills_remote` is on-demand OpenClaw GitHub corpus search.
   - `clawskills_remote` is on-demand clawskills.co index search.
4. Dynamic tool exposure is capped by `CCCC_CAPABILITY_MAX_DYNAMIC_TOOLS_VISIBLE`
   (default `32`).
5. Catalog snapshot size is capped by `CCCC_CAPABILITY_CATALOG_MAX_RECORDS`
   (default `20000`); prune is applied during explicit sync operations.
6. Search may perform remote augmentation (MCP + skill) when local hits are insufficient:
   - `CCCC_CAPABILITY_SEARCH_REMOTE_FALLBACK` (default `1`)
   - `CCCC_CAPABILITY_SEARCH_REMOTE_FALLBACK_LIMIT` (default `40`, max `100`)
   - `CCCC_CAPABILITY_SOURCE_SKILLSMP_REMOTE_ENABLED` (default `1`)
   - `CCCC_CAPABILITY_SOURCE_CLAWHUB_REMOTE_ENABLED` (default `1`)
   - `CCCC_CAPABILITY_SOURCE_OPENCLAW_SKILLS_REMOTE_ENABLED` (default `1`)
   - `CCCC_CAPABILITY_SOURCE_CLAWSKILLS_REMOTE_ENABLED` (default `1`)
   - `CCCC_CAPABILITY_SEARCH_REMOTE_SKILL_LIMIT` (default follows remote fallback limit)
   - `CCCC_CAPABILITY_SEARCH_REMOTE_SKILLSMP_LIMIT` (default follows remote fallback limit)
   - `CCCC_CAPABILITY_SEARCH_REMOTE_CLAWHUB_LIMIT` (default follows remote fallback limit)
   - `CCCC_CAPABILITY_SEARCH_REMOTE_OPENCLAW_LIMIT` (default follows remote fallback limit)
   - `CCCC_CAPABILITY_SEARCH_REMOTE_CLAWSKILLS_LIMIT` (default follows remote fallback limit)
   - `CCCC_CAPABILITY_SKILLSMP_PROXY_BASE` (default `https://r.jina.ai/http://skillsmp.com/search`)
   - `CCCC_CAPABILITY_SKILLSMP_API_BASE` (default `https://skillsmp.com/api/v1/skills/search`)
   - `CCCC_CAPABILITY_SKILLSMP_API_KEY` (optional; enables direct SkillsMP API)
   - `CCCC_CAPABILITY_CLAWHUB_API_BASE` (default `https://clawhub.ai/api/v1/skills`)
   - `CCCC_CAPABILITY_CLAWSKILLS_DATA_URL` (default `https://clawskills.co/skills-data.js`)
7. Allowlist override env/path compatibility (`CCCC_CAPABILITY_ALLOWLIST_PATH` and
   `CCCC_HOME/config/capability-allowlist.yaml`) is removed. Policy now always uses:
   - packaged default: `crates/cccc-daemon/resources/capability-allowlist.default.yaml`
   - user overlay: `CCCC_HOME/config/capability-allowlist.user.yaml`
   - effective policy: deterministic merge (`default <- overlay`).
8. The authenticated local user control plane (`actor_id="user"`, `by="user"`) includes
   `cccc_group` and `cccc_actor` in its baseline visible tools. Actor sessions do not inherit this
   user-only baseline and continue to require `pack:group-runtime` or their existing role/profile
   defaults.

#### `capability_visibility`

Hide or show a capability for one actor's UI/menu surfaces without changing enabled bindings.
The Web UI uses `actor_id="user"` to control whether an enabled capsule skill appears in the `/` command menu.

Args:
```ts
{
  group_id: string
  by?: string
  actor_id?: string // default: by or "user"
  capability_id: string
  hidden: boolean
  reason?: string
}
```

Result:
```ts
{
  action_id: string
  group_id: string
  actor_id: string
  capability_id: string
  hidden: boolean
  actor_hidden_capabilities: string[]
  state: "hidden" | "visible"
}
```

The Web slash-command adapter MUST keep `slash_skill_dispatch.task_text` non-empty. For a bare
capsule command such as `/cccc-self-evolution`, it sends the canonical task
`Run the skill's default workflow.`; explicit text after the command is forwarded unchanged. This
keeps the existing daemon validation contract compatible across independently restarted Web and
daemon processes.

#### `capability_import`

Import one normalized capability record prepared by the caller (agent-driven parsing), then optionally enable it.

Notes:

1. This op does not parse arbitrary web/forum text; caller must provide structured `record`.
2. `kind=mcp_toolpack` requires `install_mode` + `install_spec`.
3. `kind=skill` requires `capsule_text`.
4. `dry_run=true` validates/probes only (no catalog persistence).
5. `command*` and `fallback_command*` may be provided as top-level shortcuts; daemon copies them into
   `install_spec` when missing.
6. `record.source_id` is optional; empty or unknown source ids are normalized to `manual_import`.
7. `record.source_id=agent_self_proposed` preserves autonomous skill-proposal provenance. Default policy treats
   `kind=skill` capsule records from this source as mounted, while non-skill toolpacks remain indexed unless
   policy explicitly promotes them.
8. `agent_self_proposed` skill capsule text must include required proposal sections: `When to use`, `Avoid when`,
   `Procedure`, `Pitfalls`, and `Verification`; non-dry-run imports missing sections are rejected before catalog
   persistence so the last valid active record is preserved.
9. `agent_self_proposed` skill capability ids must use `skill:agent_self_proposed:<stable-slug>` to avoid
   colliding with curated namespaces such as `skill:anthropic:*` or `skill:github:*`.
10. For low-risk, syntax-valid `agent_self_proposed` capsule skills, direct import is allowed. Use `dry_run=true`
   first when enabling immediately, scope/risk is unclear, or probe diagnostics are useful; high-risk candidates
   should be recorded as `qualification_status=blocked` with explicit `qualification_reasons`.
11. Re-importing the same `capability_id` updates the catalog record. Agents should use that path for stale,
    incomplete, wrong, or duplicative `agent_self_proposed` skills instead of creating near-duplicates or silently
    deleting records.
12. Import results use `import_action`, `record_changed`, `already_active`, and `active_after_import` to distinguish
    create/update/no-op and whether the target actor had an effective binding before and after import. Local sync
    timestamps do not count as semantic changes, while an explicitly supplied `updated_at_source` still participates
    in the comparison. `import_action` is the primary create/update/unchanged signal; `record_changed` only compares
    existing records. `already_active` is pre-import state; `active_after_import` is the post-import runnable binding.
13. If `readiness_preview.preview_status=active` or `active_after_import=true`, agents must not re-enable the same skill
    just to refresh its capsule text. Use `capability_state.active_capsule_skills[].capsule_text` for full post-import
    verification; `capsule_preview` is only a compact display summary.

Args:
```ts
{
  group_id: string
  by?: string
  actor_id?: string
  record: {
    capability_id: string                 // mcp:* or skill:*
    kind: "mcp_toolpack" | "skill"
    name?: string
    description_short?: string
    source_id?: string // optional; unknown/empty -> manual_import; agent_self_proposed preserves skill-proposal provenance
    source_uri?: string
    source_record_id?: string
    source_record_version?: string
    updated_at_source?: string
    source_tier?: string
    trust_tier?: string
    qualification_status?: "qualified" | "unavailable" | "blocked"
    qualification_reasons?: string[]
    tags?: string[]
    license?: string
    install_mode?: "remote_only" | "package" | "command" // mcp_toolpack only
    install_spec?: Record<string, unknown>    // mcp_toolpack only
    command?: string | string[]               // command mode shortcut
    command_candidates?: Array<string | string[]> // command mode/fallback candidates
    fallback_command?: string | string[]      // optional package->command fallback
    fallback_command_candidates?: Array<string | string[]> // optional package->command fallback candidates
    capsule_text?: string                     // skill only
    requires_capabilities?: string[]          // skill only
  }
  dry_run?: boolean                // default false
  probe?: boolean                  // default true
  enable_after_import?: boolean    // default false
  scope?: "group" | "actor" | "session"
  ttl_seconds?: number
  reason?: string
}
```

Result:
```ts
{
  action_id: string
  group_id: string
  actor_id: string
  capability_id: string
  kind: "mcp_toolpack" | "skill"
  dry_run: boolean
  imported: boolean
  scope: "group" | "actor" | "session"
  import_action?: "created" | "updated" | "unchanged"
  record_changed?: boolean
  already_active?: boolean           // target actor had an effective binding before optional enable_after_import
  active_after_import?: boolean      // target actor has a runnable binding after import/optional enablement
  record: Record<string, unknown>
  probe: {
    state: "runnable" | "failed" | "skipped"
    kind?: "mcp_toolpack" | "skill"
    reason?: string
    tool_count?: number
    tool_names?: string[]
    install_error_code?: string
    install_error?: string
  }
  diagnostics: Array<{
    code: string
    message: string
    retryable?: boolean
    required_env?: string[]
    action_hints?: string[]
  }>
  effective_policy_level: "indexed" | "mounted" | "enabled" | "pinned"
  enableable_now: boolean
  enable_block_reason?: "policy_level_indexed" | "qualification_blocked" | "capability_unavailable"
  readiness_preview?: {
    preview_status: "blocked" | "enableable" | "active" | "needs_inspect"
    next_step: string
    already_active?: boolean
    preview_basis?: string[]
    required_env?: string[]
    missing_env?: string[]
    cached_install_state?: string
    install_error_code?: string
    enable_block_reason?: "policy_level_indexed" | "qualification_blocked" | "capability_unavailable" | "missing_required_env"
    policy_source?: "external_capability_safety_mode"
    policy_mode?: "conservative"
  }
  enable_after_import: boolean
  enable_result?: Record<string, unknown> // same shape family as capability_enable
  refresh_required: boolean
  state: "blocked" | "enableable" | "needs_inspect" | "activation_pending" | "runnable" | "verified"
  reason?: string
}
```

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
  external_capability_safety_mode: "normal" | "conservative"
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
  external_capability_safety_mode: "normal" | "conservative"
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
  external_capability_safety_mode: "normal" | "conservative"
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
  external_capability_safety_mode: "normal" | "conservative"
}
```

#### `capability_install_target`

Install and enable either an existing capability id or one or more `SKILL.md` records from a local
path, a direct HTTP(S) URL, or a GitHub repository. GitHub repositories import a root `SKILL.md`
and files matching `skills/*/SKILL.md` (up to 64 records). Imported records retain their source,
qualification, capsule, and installation metadata.

The native daemon commits catalog, binding, and actor slash-visibility state before appending one
`capability.changed` event to the target Group ledger for the complete install batch. Semantically
unchanged reinstalls do not append a duplicate event. Failed or rolled-back installs do not append
one either. Event publication is a recoverable notification boundary: if the state commit succeeds
but the ledger append fails, the operation remains successful and reports `event_publish_error`;
Web clients catch up the authoritative slash-command capability view when their global event stream
opens or reconnects.

Args:
```ts
{
  group_id: string
  target: string
  actor_id?: string
  by?: string
  scope?: "actor" | "group" | "session"
  ttl_seconds?: number
}
```

Result:
```ts
{
  action_id: string
  group_id: string
  actor_id: string
  target: string
  target_kind: "capability_id" | "local_path" | "url" | "github"
  scope: "actor" | "group" | "session"
  installed_capability_ids: string[]
  enabled_capability_ids: string[]
  use_ready_capability_ids: string[]
  requires_setup: boolean
  refresh_required: boolean // true only when the effective runtime/slash catalog changed
  state: "ready" | "needs_setup"
  event_publish_error?: string
}
```

#### `capability_uninstall`

Revoke capability bindings for the target group, mark the capability removed from that group's catalog view,
remove current-group actor autoload references, and remove runtime cache when no other group/actor bindings
remain. The catalog record, block policy, other groups, and profile defaults are preserved. Use
`capability_source_delete` for an explicit global deletion of records owned by a removable import source.

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
  removed_record: boolean
  removed_bindings: number
  removed_blocked?: number
  removed_group_marker: boolean
  removed_installation: boolean
  removed_runtime_bindings?: number
  removed_recent_success?: boolean
  removed_actor_autoload: number
  removed_profile_autoload: number
  cleanup_skipped_reason?: "cleanup_skipped_capability_still_bound"
  refresh_required: boolean
  refresh_mode?: "relist_or_reconnect"
  wait?: "relist_or_reconnect"
}
```

#### `capability_source_delete`

Explicitly delete every catalog record owned by a removable import source and clean its bindings,
runtime state, actor autoload references, and profile defaults across all groups. Built-in and curated
sources are protected. Only the user or a group foreman may perform this global operation.

Args:
```ts
{
  group_id: string
  source_id: "manual_import" | "agent_self_proposed" | "github_import" | "url_import" | "local_import"
  reason?: string
  by?: string
  actor_id?: string
}
```

Result:
```ts
{
  group_id: string
  actor_id: string
  source_id: string
  removed_records: number
  removed_capability_ids: string[]
  removed_runtime_bindings: number
  removed_installations: number
  removed_actor_autoload: number
  removed_profile_autoload: number
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
{ group_id: string; detail?: "summary" | "full" }
```

Result:
```ts
{ group: Record<string, unknown> } // group.yaml content, redacted
```

#### `group_preamble_get`

Read the effective group startup preamble. A non-empty group override replaces
the built-in preamble body on the next preamble delivery; the fixed CCCC
identity and protocol frame remains in place.

Args:
```ts
{ group_id: string }
```

Result:
```ts
{
  group_id: string
  source: "builtin" | "home"
  filename: "CCCC_PREAMBLE.md"
  overridden: boolean
  content: string
}
```

#### `group_preamble_set`

Create or replace the non-empty group preamble override. The UTF-8 encoded
content must not exceed 512 KiB. Existing sessions that have already received
their preamble are not reinjected; start a fresh session when the new guidance
must apply immediately. `group_reset` creates a new group id and does not carry
this override forward, so provisioners must set the desired preamble on the
replacement group before starting its actors. This operation manages prompt
content only; consumers requiring a distinct standby turn must observe the
actor return to `waiting` or `idle` before sending the authoritative mission.

Args:
```ts
{ group_id: string; content: string; by?: string }
```

Result: the `group_preamble_get` result plus `changed: boolean`. When `changed`
is false, the stored override is not rewritten.

#### `group_preamble_reset`

Delete the group override and restore the built-in preamble body. The explicit
confirmation avoids accidental removal.

Args:
```ts
{ group_id: string; confirm: "preamble"; by?: string }
```

Result: the `group_preamble_get` result plus `changed: boolean`.

#### `group_help_get`

Read the effective group collaboration reference. The built-in
`## Canonical Message Delivery` section is always authoritative and is composed
with the group's `CCCC_HELP.md` as an additive overlay; an overlay section with
the same heading is ignored. When `actor_id` is supplied,
the daemon MUST apply the document's `## @role:`, `## @actor:`, and
`## @voice_secretary` visibility rules before returning `markdown`. Runtime-only
MCP addenda are outside this operation and MAY be appended by the MCP adapter.
`user` and the foreman may request any actor's effective help; a peer may request
only its own actor view and MUST NOT use this operation to read another actor's
scoped note.

Args:
```ts
{ group_id: string; actor_id?: string; by?: string }
```

Result:
```ts
{
  group_id: string
  actor_id: string | null
  source: "builtin" | "home"
  source_path: string
  filename: "CCCC_HELP.md"
  overridden: boolean
  markdown: string
}
```

#### `actor_notes_get`

Read actor-scoped notes from `## @actor: <actor_id>` blocks in the canonical
group `CCCC_HELP.md`. `user` and the foreman may read any actor or omit
`target_actor_id` to list all notes. A peer MUST provide its own actor id and
MUST NOT read another actor's note.

Args:
```ts
{ group_id: string; target_actor_id?: string; by?: string }
```

Result when a target is supplied:
```ts
{
  target_actor_id: string
  content: string
  source: "builtin" | "home"
  path: string
}
```

Result when listing:
```ts
{
  actor_notes: Array<{ actor_id: string; content: string }>
  source: "builtin" | "home"
  path: string
}
```

#### `actor_notes_set`

Create or replace one existing actor's scoped note in the canonical group help
document. Only `user` or the foreman may mutate actor notes. The daemon MUST
preserve common, role, other-actor, Voice Secretary, and unknown tagged blocks,
write atomically, and MUST NOT create a Context or actor-record copy.

Args:
```ts
{ group_id: string; target_actor_id: string; content: string; by?: string }
```

Result: the targeted `actor_notes_get` result plus `changed: boolean`.

#### `actor_notes_clear`

Remove one existing actor's scoped note without changing other help content.
Permission and preservation rules are identical to `actor_notes_set`.

Args:
```ts
{ group_id: string; target_actor_id: string; by?: string }
```

Result: the targeted `actor_notes_get` result plus `changed: boolean`.

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

Notes:
- Successful deletion MUST revoke every remote connector credential bound to the deleted group. A failure that leaves the group registered and available MUST preserve its pre-delete connector authority.
- Successful deletion MUST retire every local external-space binding, queued job, and referenced job payload owned by the deleted group. It MUST NOT delete the user's remote notebook or other provider space. A failure that leaves the group registered and available MUST restore the pre-delete local binding and queue state.

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
{ group_id: string; event: CCCSEventV1 }
```

#### `group_set_state`

Args:
```ts
{ group_id: string; state: "active" | "idle" | "paused"; by?: string }
```

Notes:
- `stopped` is not a valid `group_set_state` value in daemon IPC v1.
- Higher-level surfaces (CLI/MCP) MAY expose `stopped` as a convenience alias that maps to `group_stop`.
- While a group remains `paused`, the daemon MUST NOT submit queued
  `chat.message` or `system.notify` work to actor runtimes.
  A user-authored Send or Request Reply is an explicit use action: it MUST first
  resume the group to `active`, enable its addressed actors, and then deliver
  through the normal runtime path. Mail does not resume the group. Canonical
  unread work remains in the ledger and MAY be surfaced through one bounded
  recovery notice after the group returns to `active` or `idle`.

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

Patch keys used by CCCC include:
- Messaging: `default_send_to`
- Delivery: `mail_notice_after_seconds` (default 1800, zero disables),
  `reply_notice_after_seconds` (default 900, zero disables)
- Automation: `actor_idle_timeout_seconds`, `keepalive_delay_seconds`,
  `keepalive_max_per_actor`,
  `silence_timeout_seconds`, `help_nudge_interval_seconds`,
  `help_nudge_min_messages`
- Terminal transcript: `terminal_transcript_visibility`, `terminal_transcript_notify_tail`, `terminal_transcript_notify_lines`

Result:
```ts
{ group_id: string; settings: Record<string, unknown>; event: CCCSEventV1 }
```

#### `assistant_state`

Read the group-scoped state for first-party built-in assistants. Voice
Secretary service-local ASR runs in-process through the Rust `sherpa-onnx`
binding. The native runtime is linked into the CCCC binary; model weights remain
explicit, checksummed downloads under `CCCC_HOME/cache/voice-models`.

Voice Secretary configuration (`enabled` and `config`) remains in
`group.yaml:assistants.voice_secretary`. Durable workflow records live in
`groups/<group_id>/state/assistants.json`: lifecycle, durable health, sessions,
prompt drafts/requests, and ask requests. Process observations such as PID,
port, live service/socket state, and actor handles MUST NOT be persisted there.
Implementations MUST preserve the reserved `rust_state` object when updating
the common records. A legacy Rust workflow embedded in `group.yaml:assistants`
is imported canonical-first; after the canonical file commits, only assistant
configuration remains in `group.yaml`.

Args:
```ts
{
  group_id: string
  assistant_id?: "voice_secretary"
  view?: "voice_session" | string
  session_id?: string
  document_path?: string
  suppress_retry_notify?: boolean
}
```

`view="voice_session"` is the canonical session projection used by the Web
meeting view. With `session_id`, it returns that document-capture
session. Without `session_id`, `document_path` first resolves the durable
cross-session transcript at
`$CCCC_HOME/voice-secretary/<group_id>/documents/<document_id>/transcript.jsonl`;
when no document transcript exists, it falls back to the latest matching
session in `state/assistants.json`. Prompt-refinement, composer, and instruction
semantic inputs are never projected as meeting transcript. The document
transcript projection has `source="document_transcript"` and may aggregate rows
from several recording sessions.

Specialized result for `view="voice_session"`:
```ts
{
  group_id: string
  session: {
    session_id: string
    capture_mode: "document"
    document_path?: string
    status?: string
    segments: Array<Record<string, unknown>>
    transcript?: string
    diarization?: Record<string, unknown>
    source?: "document_transcript"
  } | Record<string, never>
}
```

Result:
```ts
{
  group_id: string
  assistants?: Array<Record<string, unknown>>
  assistants_by_id?: Record<string, unknown>
  assistant?: Record<string, unknown>
  proposals?: Array<Record<string, unknown>>
  proposals_by_id?: Record<string, unknown>
  documents?: Array<Record<string, unknown>>
  documents_by_path?: Record<string, unknown>
  active_document_path?: string
  capture_target_document_path?: string
  documents_by_id?: Record<string, unknown>      // daemon sidecar/internal compatibility only
  active_document_id?: string                    // daemon sidecar/internal compatibility only
  capture_target_document_id?: string            // daemon sidecar/internal compatibility only
  new_input_available?: boolean
  service_runtime?: Record<string, unknown>
  service_models?: Array<Record<string, unknown>>
  service_models_by_id?: Record<string, unknown>
}
```

`service_runtime` is read-only engine metadata with the stable runtime ID
`sherpa_onnx_streaming`, readiness, and the linked sherpa-onnx version. The
engine ships inside the CCCC executable and has no independent install/remove
lifecycle; only voice models are downloaded or removed. Voice model records may include `installed_manifest_sha256`,
`update_available`, `last_update_error`, and artifact source fields (`url`,
`sha256`, `archive`) so model updates remain explicit and inspectable.

#### `assistant_settings_update`

Update group-scoped built-in assistant settings.

When `voice_secretary.enabled=true`, the daemon also materializes a hidden
internal actor with `internal_kind="voice_secretary"` and `actor_id="voice-secretary"`.
That actor is a distinct assistant identity, not the foreman and not a normal
peer. Its startup runtime config (Runtime-derived execution surface, `command`,
env/secrets, scope, submit behavior) is copied from the current stable foreman actor so the user
does not configure a second runtime profile. The foreman's enabled/running state
does not affect assistant config inheritance. If no foreman actor exists,
enabling Voice Secretary fails. If the group is already running, the daemon
starts or restarts this assistant actor as needed; disabling Voice Secretary
stops/removes the actor and its private env.

Args:
```ts
{
  group_id: string
  by?: string
  assistant_id: "voice_secretary"
  patch: {
    enabled?: boolean
    config?: {
      capture_mode?: "browser" | "service"
      recognition_backend?: "mock" | "assistant_service_local_asr" | "browser_asr" | "external_provider_asr"
      recognition_language?: "auto" | string
      retention_ttl_seconds?: number
      auto_document_enabled?: boolean
      document_default_dir?: string
      auto_document_quiet_ms?: number
      auto_document_min_chars?: number
      auto_document_max_window_seconds?: number
      service_model_id?: string
      tts_enabled?: boolean
    }
  }
}
```

`browser_asr` means browser-managed speech recognition and does not guarantee
browser-device-local model execution. `assistant_service_local_asr` means ASR
runs on the daemon host through native Rust and uses an installed local ASR
model. The returned assistant health may include `health.service` with
`status`, `alive`, `ready`, `selected_model_id`, `model`, `runtime`, and
`streaming_backend` so Web can show whether
service-local ASR is actually usable. `service_model_id` is optional and
selects a daemon-managed local ASR model for on-demand install/use.
`recognition_language="auto"` means the browser/client chooses the best language
hint; otherwise callers should pass a BCP-47-like tag such as `zh-CN`, `en-US`,
or `ja-JP`. `auto_document_enabled=true` is the default path: stable transcript
segments are compacted into the Voice Secretary input stream, then the
`voice-secretary` runtime actor pulls unread input and edits the working markdown
document directly in the repository. `auto_document_quiet_ms` is the client
silence window before flushing speech into that semantic lane;
`auto_document_min_chars` and `auto_document_max_window_seconds` are daemon-side
guardrails that keep long continuous speech from waiting forever for a pause.
The runtime actor should treat transcript as source material for
evidence-bounded reconstruction: it may use transcript, group context, existing
documents, common knowledge, and verified lightweight research to produce a
coherent artifact, but must not fabricate facts and should compactly mark
low-confidence entities, numbers, quotations, or dates.
The document loop should be incremental and non-lossy: each unread input batch
should be organized into the best current document structure while preserving
useful concrete details, and idle review should refine/reorganize/enrich rather
than replace detail-rich material with a short executive summary.
The daemon does not track per-job completion: it stores an input cursor, nudges
the actor when unread input exists, and sends idle-review nudges only on
recording stop or after enough new transcript input plus the group cooldown
(default: stop flush immediately, otherwise 8 new transcript input flushes and
at least 5 minutes since the previous idle review).
If the group has an active workspace scope,
`document_default_dir` (default `docs/voice-secretary`) is
resolved under that workspace; otherwise the daemon falls back to CCCC_HOME.
Raw transcript/source/input sidecars stay in CCCC_HOME.
`external_provider_asr` must remain explicit opt-in.

The semantic input authority is
`$CCCC_HOME/voice-secretary/<group_id>/input_events.jsonl`; its daemon-owned
read/delivery cursor and retry timing live in the sibling `input_state.json`.
Implementations MUST NOT maintain an engine-private sequence or cursor. The
former Rust `inputs.jsonl` and `groups/<group_id>/state/assistants.json:rust_state.input_*`
shape is a one-way migration source: canonical input/state commit first, then
the legacy log and cursor fields are retired. If independently written streams
must be merged, migration may conservatively replay an already-read item but
MUST NOT advance across or skip an unread item.

Result:
```ts
{ group_id: string; assistant: Record<string, unknown>; event: CCCSEventV1 }
```

#### `assistant_voice_model_install`

Download and verify a daemon-managed local Voice Secretary ASR model into
CCCC-owned cache storage. Built-in releases include a default model manifest;
tests and local development may add a local overlay at
`CCCC_HOME/config/voice-models.json`. Each artifact entry must include a fixed
URL and `sha256`. Reinstalling/updating a model downloads into staging storage
and replaces the active model only after all artifacts verify successfully.

Args:
```ts
{
  group_id: string
  by?: string
  model_id: string
}
```

Result:
```ts
{
  group_id: string
  assistant: Record<string, unknown>
  model: {
    model_id: string
    status: "not_installed" | "downloading" | "ready" | "failed" | "unknown"
    install_dir?: string
    installed_at?: string
    updated_at?: string
    error?: Record<string, unknown>
    update_available?: boolean
    installed_manifest_sha256?: string
  }
}
```

#### HTTP Voice Secretary transcription

Transcribe a push-to-talk audio payload through the daemon-managed first-party
Voice Secretary runtime. This endpoint only returns transcript text and service
health; it does not create a chat message, proposal, or working document by
itself. Call `assistant_voice_transcript_append` after transcription so the
daemon can append stable transcript source material and update the current
working document.

Request:
```ts
POST /api/v1/groups/{group_id}/assistants/voice_secretary/transcriptions
  ?language={language}&by={actor_id}
Content-Type: audio/pcm | audio/wav | application/octet-stream

<streamed binary audio body>
```

Preconditions:
- `voice_secretary` is enabled for the group.
- `recognition_backend` is `assistant_service_local_asr`.
- The selected offline `service_model_id` is installed and its manifest exposes
  a supported sherpa-onnx model configuration. HTTP transcription accepts mono
  PCM16 or WAV up to 100 MiB. The HTTP body and WebSocket PCM16 frames are
  streamed to auto-deleted temporary files; browser service capture sends binary
  PCM16 WebSocket frames.

Result:
```ts
{
  group_id: string
  assistant: Record<string, unknown>
  transcript: string
  mime_type: string
  language?: string
  bytes?: number
  backend: "assistant_service_local_asr"
  service: Record<string, unknown>
  asr?: Record<string, unknown>
}
```

#### WebSocket Voice Secretary transcription

The service-local ASR browser transport keeps one recording lease and one
microphone capture active while raw PCM is rolled into bounded server-side
files:

```ts
GET /api/v1/groups/{group_id}/assistants/voice_secretary/transcriptions/ws
  ?owner_id={owner_id}&lease_id={lease_id}
```

After upgrade, the client sends a JSON `start` command, then 16 kHz mono PCM16
binary frames, and finally a JSON `stop` command. The server returns a `ready`
event whose `recording_segment_duration_ms` is currently `1500000`. Whenever a
full segment has been flushed and data-synced, the server emits:

```ts
{
  type: "recording_segment_saved"
  ok: true
  seq: number
  segment_index: number
  start_ms: number
  end_ms: number
  duration_ms: number
  bytes: number
}
```

Segment rollover MUST NOT stop live recognition or require a new microphone
capture. Rust stores 48,000,000 PCM bytes per segment (25 minutes) and caps one
WebSocket session at 800 MiB (about 7 hours 17 minutes). On `stop` or an
unexpected disconnect, persistent recordings longer than 30 seconds MUST defer
final transcription when speaker analysis is available. Short persistent recordings MAY run
immediate final ASR. Final ASR paths that cannot defer MUST process segment files
sequentially and reuse one offline recognizer per recording segment across
inference ranges no longer than 30 seconds. When final ASR is deferred because
the recording is long or the native inference worker is occupied, WebSocket stop
MUST complete promptly with `final_asr_status.status` set to
`deferred_to_speaker_analysis`, retain the durable live transcript, and queue
speaker analysis; temporary worker occupancy MUST NOT permanently skip speaker
analysis or retain the recording lease. A final ASR path that cannot defer but
finds the native inference worker occupied MUST bound its wait well below the
recording lease TTL and complete stop with an `asr_busy` `final_asr_text` error
if the worker stays occupied, so a queued stop never outlives its lease.
HTTP upload transcription MAY retain a
fail-fast busy response. The
`final_asr_text` event keeps the combined text in timeline order and includes a
`segments` array with each inference range's status and its owning
`recording_segment_index`. If at least one range succeeds and another fails, the
event keeps `ok=true` so the available text is retained,
and MUST also report `partial=true` plus `failed_segment_count`; clients MUST
surface that incompleteness rather than presenting the text as a complete
transcript. Speaker analysis is likewise
sequential so native diarization holds at most one segment waveform at a time.
Multi-segment speaker results MUST NOT imply cross-segment identity matching;
Rust marks them with `speaker_identity_scope="recording_segment"`.

#### `assistant_voice_recording_lease`

Acquire, refresh, release, or inspect the daemon-owned Voice Secretary recording
lease. Web clients may keep a local browser lock for fast UX debouncing, but the
daemon lease is the final cross-tab / cross-browser / cross-device guard that
prevents two Voice Secretary recording streams from running at the same time.
The lease is TTL-based so a crashed tab or disconnected browser eventually
expires without manual cleanup.

The service-local ASR WebSocket requires the active `owner_id` and `lease_id` as
query parameters and revalidates them while audio is streaming. Opening the
transcription WebSocket directly cannot bypass the daemon lease.
Lease mutations match `group_id`, `owner_id`, and `lease_id`; public status and
conflict payloads redact `lease_id`. The stable browser owner identifies the
lease holder, while every recording uses a fresh `session_id`.

Args:
```ts
{
  group_id: string
  by?: string
  action: "acquire" | "heartbeat" | "release" | "status"
  owner_id?: string        // required for acquire/heartbeat/release
  lease_id?: string        // returned by acquire; required to refresh/release that acquisition
  ttl_seconds?: number     // default 30; bounded by the daemon
  capture_mode?: string
  recognition_backend?: string
  dispatch_target?: string
}
```

Result:
```ts
{
  group_id: string
  action: string
  acquired: boolean
  released: boolean
  lost: boolean
  lease_id?: string        // only returned to the acquiring/refreshing owner
  lease?: {
    owner_id: string
    group_id: string
    group_title?: string
    capture_mode?: string
    recognition_backend?: string
    dispatch_target?: string
    by?: string
    created_at?: string
    updated_at?: string
    expires_at?: string
  }
}
```

If another live lease exists, `acquire` / `heartbeat` returns
`assistant_voice_recording_busy` with `details.active_lease`.
Every successful `acquire` creates a fresh `lease_id`, including when the
`owner_id` matches the active lease, so cleanup from an older connection cannot
release its replacement.
`heartbeat` only refreshes the matching active `owner_id` + `lease_id`; it never
creates a new lease. Stale `heartbeat` / `release` requests return `lost` or
`released=false` without modifying a newer lease. An omitted heartbeat metadata
field preserves the value from the active lease. The transcription WebSocket
binds its start frame to the lease's `capture_mode`, `recognition_backend`, and
`dispatch_target`; changing capture scope requires a new lease.

The daemon MUST serialize lease mutations and every read that may
expire and clear the lease through
`$CCCC_HOME/state/voice_secretary_recording_lease.json.lock`. A process-local
lock alone is insufficient because daemon and Web processes may use the same
home. `acquire` and `heartbeat` MAY operate while Voice Secretary is
disabled only when the effective `dispatch_target` is `composer`; an omitted
heartbeat target inherits the active lease target. This direct-dictation path
MUST NOT create Voice Secretary input, session, document, or diarization state.

#### `assistant_voice_transcript_append`

Append a stable transcript segment for Voice Secretary. Web/browser ASR and
service-local ASR converge here. The daemon writes stable segments to
`$CCCC_HOME/voice-secretary/<group_id>/<session_id>/transcripts/segments.jsonl`,
updates the bounded shared session projection in
`groups/<group_id>/state/assistants.json`, appends final document-capture rows
to `$CCCC_HOME/voice-secretary/<group_id>/documents/<document_id>/transcript.jsonl`, and
by default appends a semantic input event for the current Voice Secretary
markdown working document. The working document is a user-facing repo artifact;
raw transcript/source/revision sidecars remain in CCCC_HOME. When new input is
available, the daemon emits a targeted `system.notify` to `voice-secretary` with
`context.kind="voice_secretary_input"` and a daemon-owned `input_envelope`. The
envelope is the canonical work item delivered to every actor runtime;
`assistant_voice_document_input_read` /
`cccc_voice_secretary_document(action="read_new_input")` remains a legacy,
recovery, and debugging entrypoint. Input append is durable before runtime actor
wake-up; if wake-up fails, the input remains readable and the API reports the
best-effort wake error separately. If wake-up succeeds after the notify was
created while the actor was stopped, the daemon re-dispatches that same notify
through the actor's normal runtime input so lazy startup instructions are included.

The group operation validates or creates the Markdown target before committing
transcript/session/input state. Retrying the same `session_id` and `segment_id`
is idempotent for the stable session, document transcript, and semantic input
records. Document paths must be repository-relative `.md` paths and must not
traverse symbolic links.

Idempotency is checked against the complete semantic input log, not the bounded
session display window. If the input log was committed but its ledger input or
notify event was interrupted, retrying the same segment reuses the canonical
input record and completes only the missing delivery work.

Transcript producers MAY attach the following revision metadata:

- `transcript_stage`: `live` or `final`. When omitted, the daemon infers
  `final` only from `trigger.recognition_backend=assistant_service_local_asr_final`;
  other legacy segments are treated as `live`.
- `supersedes_segment_ids`: segment IDs in the same session that the new record
  replaces in the current projection.
- `supersede_stage: "live"`: daemon shorthand for a stable `final` segment
  (`is_final=true`) that resolves all current live segment IDs in the session
  into `supersedes_segment_ids` before committing and keeps race-late live
  checkpoints in that session raw-only.
- `source_model_id`: the producer model identity retained with the raw record.
- `revision_only: true`: valid only for a stable `final` segment
  (`is_final=true`). The daemon retains this marker with the raw segment and
  updates transcript projection. Under the input-state lock it MUST reuse an
  existing ASR input for that session; when none exists and automatic document
  input is enabled, it MUST create and deliver the final segment as the
  session's first semantic input.

Revision records are append-only. The daemon MUST retain superseded live rows
in session and document transcript sidecars, while the bounded session
`transcript` projection and Web meeting view omit them. A normal document-mode
WebSocket stop and a successfully finalized disconnect MUST persist the final
SenseVoice result as a revision-only record superseding the session's live
checkpoints. A partial final result MUST NOT supersede live checkpoints. A
document-mode `final_asr_text` reports `transcript_persistence` as `persisted`,
`skipped_partial`, or `failed`, with `transcript_persisted` carrying the boolean
commit result. Callers MUST retain or retry their fallback text until persistence
is confirmed. If final ASR fails, the existing live projection remains valid.

The public document identity for Voice Secretary APIs is `document_path`, a
repository-relative markdown path. `document_id` may exist in daemon sidecar
state as an implementation detail, but runtime actors and Web clients should
route by `document_path`.

Repository markdown is the document-content authority. The canonical document
registry and active selection live at
`$CCCC_HOME/voice-secretary/<group_id>/documents/index.json`; implementations
serialize mutations with the sibling `index.json.lock`. The former Rust
`groups/<group_id>/state/assistants.json:rust_state.documents/active_document_*`
shape is a one-way migration source. Canonical index entries win path conflicts,
unique legacy entries are retained, and the legacy fields are removed only
after the canonical index commits. Implementations MUST NOT keep an
engine-private active-document selection.

`assistant_index`, `assistant_voice_document_list`, and
`assistant_voice_document_select` reconcile repository Markdown edits into the
daemon document index before returning. Reconciliation also discovers
previously unindexed `.md` files under the effective `document_default_dir`, as
runtime actors may create working documents directly in the repository; an
archived or deleted indexed path MUST NOT be rediscovered as active.
Reconciliation updates content, hash, character count, and revision only when
file content changed. Missing files do not clear indexed content, and
path/symbolic-link validation is applied before reading. The emitted
`assistant.voice.document` reconciliation event is an auxiliary signal; index
persistence and ledger append are not one atomic transaction.

Args:
```ts
{
  group_id: string
  by?: string
  session_id: string
  segment_id?: string
  text?: string
  language?: string
  document_path?: string
  is_final?: boolean
  flush?: boolean
  trigger?: {
    trigger_kind?: "push_to_talk_stop" | "service_transcript" | "meeting_window"
    mode?: "dictation" | "meeting"
    capture_mode?: "browser" | "service" | string
    recognition_backend?: string
    client_session_id?: string
    input_device_label?: string
    language?: string
  }
}
```

Result:
```ts
{
  group_id: string
  assistant: Record<string, unknown>
  session_id: string
  segment?: Record<string, unknown>
  segment_path?: string
  document?: Record<string, unknown>
  document_updated: boolean
  input_event?: Record<string, unknown>
  input_event_created: boolean
  input_notify_emitted: boolean
  input_notify_error?: string
  actor_woken?: boolean
  actor_wake_error?: string
  actor_notify_delivered?: boolean
  actor_notify_delivery_error?: string
}
```

#### `assistant_voice_session_update`

Persist a Web-owned completion projection (currently speaker diarization) into
the canonical session authority before publishing its completion
event. This is an internal daemon boundary used by browser capture; callers do
not replace transcript segments through this operation.

Voice-session mutation is limited to the user, the
`assistant:voice_secretary` principal, or a foreman allowed to update group
settings. A `session_id` used for filesystem-backed state MUST be canonicalized
to one safe path component or rejected before any state or filesystem mutation;
caller-controlled absolute paths and `.` / `..` components MUST never be joined
into the Voice Secretary storage root.

Args:
```ts
{
  group_id: string
  session_id: string
  by?: "assistant:voice_secretary" | string
  patch: {
    status?: string
    document_path?: string
    audio_duration_ms?: number
    diarization_ready?: boolean
    diarization_artifact_path?: string
    diarization?: Record<string, unknown>
    diarization_error?: Record<string, unknown>
    error?: Record<string, unknown> | null
    latest_partial?: string
  }
}
```

Result:
```ts
{ group_id: string; session: Record<string, unknown> }
```

#### `assistant_voice_session_transcript_clear`

Clear the selected session display transcript and the matching durable document
transcript. Document Markdown content is not deleted.

Args:
```ts
{ group_id: string; session_id?: string; document_path?: string; by?: string }
```

Result:
```ts
{ group_id: string; session_id: string; cleared: boolean }
```

#### `assistant_voice_document_list`

List active Voice Secretary working documents for the group. Archived documents
are excluded unless `include_archived=true`.

Args:
```ts
{ group_id: string; include_archived?: boolean }
```

Result:
```ts
{
  group_id: string
  documents: Array<Record<string, unknown>>
  documents_by_id: Record<string, unknown>
  documents_by_path: Record<string, unknown>
  active_document_id?: string
  capture_target_document_id?: string
  active_document_path?: string
  capture_target_document_path?: string
}
```

#### `assistant_voice_document_input_read`

Read all unread Voice Secretary input events since the actor's last successful
read. Reading advances the daemon-managed cursor immediately; the actor does not
see or manage cursor/sequence values. This intentionally avoids a separate
job-completion protocol. If the actor crashes after reading, the raw input log
remains in CCCC_HOME for debugging/replay, but the normal live cursor has moved.

Args:
```ts
{ group_id: string; by?: "voice-secretary" | "assistant:voice_secretary" }
```

Result:
```ts
{
  group_id: string
  item_count: number
  document_count: number
  input_text: string
  input_batches: Array<{
    document_path: string
    filename?: string
    title?: string
    item_count: number
    kinds?: string[]
    intent_hints?: string[]
    languages?: string[]
    sources?: string[]
  }>
  documents: Array<Record<string, unknown>>
  has_new_input: boolean
}
```

#### `assistant_voice_document_save`

Save or create a Voice Secretary working markdown document. This is the daemon
path used by Web when the user edits the document surface. The `voice-secretary`
actor should normally edit repository-backed markdown directly at
`document_path`; the MCP document tool intentionally has no save action.
When `content` is omitted for an unindexed path, an implementation MUST NOT
rewrite an existing repository file: it MAY read the file into the document
index or reject the request. An empty file MAY be created only when the target
does not already exist.

Args:
```ts
{
  group_id: string
  by?: string
  document_path?: string
  workspace_path?: string
  title?: string
  content?: string
  status?: "active" | "archived"
  create_new?: boolean
}
```

Result:
```ts
{ group_id: string; document: Record<string, unknown>; event: CCCSEventV1 }
```

#### `assistant_voice_document_instruction`

Append a user instruction for one active working document into the same Voice
Secretary input stream used for ASR transcript. The daemon emits a targeted
`voice_secretary_input` notify and the runtime actor works from the inline
`input_envelope`. The daemon does not directly append the instruction to a
document. Cross-peer handoff is intentionally handled only by
`assistant_voice_request`, and only when the Voice Secretary decides the work
belongs to foreman or one concrete peer.

Args:
```ts
{
  group_id: string
  by?: string
  document_path: string
  request_id?: string
  input_append_id?: string
  instruction?: string
  source_text?: string
  trigger?: Record<string, unknown>
}
```

Result:
```ts
{
  group_id: string
  assistant?: Record<string, unknown>
  document: Record<string, unknown>
  request_id: string
  input_append_id?: string
  ask_request?: Record<string, unknown>
  input_event?: Record<string, unknown>
  input_event_created?: boolean
  input_notify_emitted?: boolean
  input_notify_error?: string
  actor_woken?: boolean
  actor_wake_error?: string
  actor_notify_delivered?: boolean
  actor_notify_delivery_error?: string
  event?: CCCSEventV1
}
```

`request_id` identifies the logical Ask request. `input_append_id` identifies
one durable append attempt. A caller retrying an accepted append MUST reuse both
values. The daemon MUST then return the existing input with
`input_event_created=false` and MUST NOT append a second semantic input, request
event, or notification. When either value is omitted, the daemon may generate
it and no retry guarantee exists until the caller retains the returned values.

#### `assistant_voice_input_append`

Append a general Voice Secretary Ask or create/update a composer refinement
request. The daemon persists the request before emitting one targeted
`voice_secretary_input` notification. This operation creates work for Voice
Secretary; it does not create a prompt draft.

Args:
```ts
{
  group_id: string
  by?: string
  kind: "voice_instruction" | "prompt_refine"
  request_id?: string
  input_append_id?: string
  instruction?: string
  text?: string
  source_text?: string
  voice_transcript?: string
  composer_text?: string
  operation?: "append_to_composer_end" | "replace_with_refined_prompt" | string
  composer_context?: Record<string, unknown>
  composer_snapshot_hash?: string
}
```

For `voice_instruction`, at least one of `instruction`/`text` or `source_text`
must be non-empty. For `prompt_refine`, at least one of `voice_transcript` or
`composer_text` must be non-empty. `request_id` groups a composer refinement and
may be reused for intentional follow-up input. Each distinct follow-up MUST use
a new `input_append_id`; an exact retry MUST reuse the prior one and follows the
same no-duplicate rule as `assistant_voice_document_instruction`.

Result:
```ts
{
  group_id: string
  assistant?: Record<string, unknown>
  request_id: string
  input_append_id?: string
  prompt_request?: Record<string, unknown>
  ask_request?: Record<string, unknown>
  input_event?: Record<string, unknown>
  input_event_created: boolean
  input_notify_emitted: boolean
  input_notify_error?: string
  actor_woken?: boolean
  actor_wake_error?: string
  actor_notify_delivered?: boolean
  actor_notify_delivery_error?: string
  event?: CCCSEventV1
}
```

#### `assistant_voice_instruction_feedback`

Report progress or the terminal result for an existing Voice Secretary Ask.
Only the `voice-secretary` actor/principal may submit feedback.

Args:
```ts
{
  group_id: string
  by?: "voice-secretary" | "assistant:voice_secretary"
  request_id: string
  status: "working" | "done" | "needs_user" | "failed"
  reply_text?: string
  result_text?: string
  message?: string
  document_path?: string
  artifact_paths?: string[]
  source_summary?: string
  checked_at?: string
  source_urls?: string[]
}
```

Result:
```ts
{
  group_id: string
  assistant: Record<string, unknown>
  ask_request: Record<string, unknown>
  event: CCCSEventV1
}
```

#### `assistant_voice_ask_requests_clear`

Hide Ask history from the current projection. `keep_active=true` preserves
`pending` and `working` requests in the visible result. Clearing is a display
operation, not cancellation: the daemon MUST retain enough bounded state to
accept later feedback for a cleared in-flight request. User-visible feedback
may make that request visible again.

Args:
```ts
{ group_id: string; keep_active?: boolean; by?: string }
```

Result:
```ts
{
  group_id: string
  assistant: Record<string, unknown>
  ask_requests: Array<Record<string, unknown>>
  latest_ask_request?: Record<string, unknown>
  cleared_count: number
  removed_count: number
  kept_count: number
}
```

#### `assistant_voice_prompt_draft_submit`

Submit the Voice Secretary result for an existing prompt refinement request.
Only `voice-secretary` / `assistant:voice_secretary` may call this operation.
The daemon inherits a missing operation and composer snapshot hash from the
request, stores the result as `pending`, and emits
`assistant.voice.prompt_draft`. `no_op=true` stores `no_change` with empty draft
text. Submission MUST NOT append another semantic input or emit another
`voice_secretary_input` notification.

Args:
```ts
{
  group_id: string
  by?: "voice-secretary" | "assistant:voice_secretary"
  request_id: string
  draft_text?: string
  no_op?: boolean
  summary?: string
  operation?: string
  composer_snapshot_hash?: string
}
```

`draft_text` is required unless `no_op=true`.

#### `assistant_voice_prompt_draft_ack`

Mark a submitted draft as `applied`, `dismissed`, or `stale`. Acknowledgement
removes it from the active `prompt_draft` projection while retaining bounded
request history.

Args:
```ts
{
  group_id: string
  request_id: string
  status: "applied" | "dismissed" | "stale"
}
```

#### `assistant_voice_request`

Send a structured Voice Secretary action request to `@foreman` or one concrete
actor without exposing normal `chat.message` send tools to the
`voice-secretary` runtime actor. The daemon records an `assistant.voice.request`
event and delivers a targeted `system.notify` with
`context.kind="voice_secretary_action_request"`. This is the default path for
spoken "please do X / ask Y to do X" content; ordinary memo/document updates
MUST stay in the Voice Secretary document surface.

Args:
```ts
{
  group_id: string
  by?: "voice-secretary" | "assistant:voice_secretary"
  target?: "@foreman" | string   // one concrete actor id; no @all/user broadcast
  request_text: string           // concise actionable handoff, not raw transcript
  summary?: string
  document_path?: string
  artifact_paths?: string[]      // repo-relative produced docs/artifacts for user-visible links
  source_event_id?: string
  priority?: "low" | "normal" | "high" | "urgent"
}
```

Result:
```ts
{
  group_id: string
  assistant: Record<string, unknown>
  request: Record<string, unknown>
  notify_event: CCCSEventV1
  event: CCCSEventV1
}
```

#### `assistant_voice_document_archive`

Archive a Voice Secretary working document. The markdown file is left in place;
the assistant index hides it from the active document list, and later transcript
ingress without an explicit `document_path` creates or selects another active
document instead of appending to the archived one.

Args:
```ts
{ group_id: string; by?: string; document_path: string }
```

Result:
```ts
{ group_id: string; document: Record<string, unknown>; event: CCCSEventV1 }
```

#### `assistant_status_update`

Update lifecycle/health for a built-in assistant service. The assistant principal
(`assistant:<assistant_id>`) may update its own status; users/foremen may also
update it for control-plane repair.

Args:
```ts
{ group_id: string; by?: string; assistant_id: "voice_secretary"; lifecycle: "disabled" | "idle" | "running" | "working" | "waiting" | "failed"; health?: Record<string, unknown> }
```

Result:
```ts
{ group_id: string; assistant: Record<string, unknown>; event: CCCSEventV1 }
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
      trigger:
        | { kind: "interval"; every_seconds: number }
        | { kind: "cron"; cron: string; timezone?: string }
        | { kind: "at"; at: string } // RFC3339
      action?:
        | {
            kind?: "notify"
            title?: string
            snippet_ref?: string | null
            message?: string
            priority?: "low" | "normal" | "high" | "urgent"
          }
        | { kind: "group_state"; state?: "active" | "idle" | "paused" | "stopped" }
        | { kind: "actor_control"; operation?: "start" | "stop" | "restart"; targets?: string[] }
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
    last_fired_at: string
    last_error_at: string
    last_error: string
    next_fire_at: string
    completed: boolean
    completed_at: string
  }>
  supported_vars: string[] // exactly: interval_minutes, group_title, actor_names, scheduled_at
  version: number
  server_now: string
  config_path: string
}
```

Notes:
- `by` as a peer receives a filtered view: group rules + own personal rules.
- Rule IDs are non-empty and unique within a ruleset. Unknown fields and invalid
  trigger/action combinations are rejected instead of being persisted for one
  engine to ignore later.
- `group_state` and `actor_control` actions require an `at` trigger. Actor
  callers may manage only `notify` rules; a peer may mutate only its own
  personal notification rule targeting itself.
- The first tick of a newly enabled interval rule establishes its clock and
  does not fire immediately. A paused or stopped group runs no automation. An
  idle group runs user rules but suppresses the built-in `standup` rule.
- Resume never catches up missed work: interval and cron clocks are rebased,
  missed one-time rules are completed without execution, and future one-time
  rules remain eligible.
- A notification firing completes only after at least one `system.notify` has
  been appended durably for an enabled matching recipient. Recipient delivery
  does not require a currently running actor process. A successfully completed
  one-time rule is disabled.
- `group.yaml:automation` and `state/automation.json` are the shared config and
  runtime authorities. The single-daemon process lock owns scheduling; engine
  handoff consumes these files and does not introduce a second scheduler lease
  or retry journal.

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

Resume the Group and run every actor whose desired `enabled` state is true.
Actors explicitly disabled through `actor_stop` remain disabled.

Args:
```ts
{ group_id: string; by?: string }
```

Result:
```ts
{ group_id: string; started: string[]; event: CCCSEventV1 }
```

#### `group_stop`

Stop the Group's actor runtimes without changing any actor's desired `enabled`
state. This is a group-level suspension; use `actor_stop` to disable one actor.

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
{ actors: Array<Record<string, unknown>> } // includes at least id/title/runtime/enabled + role/running
```

#### `actor_add`

Args:
```ts
{
  group_id: string
  actor_id?: string
  title?: string
  runtime?: string
  command?: string[]
  env?: Record<string, string>
  capability_autoload?: string[] // actor startup autoload capability ids
  capability_hidden?: string[] // actor-level skill menu hide preferences; does not disable capabilities
  env_private?: Record<string, string> // write-only secrets (stored under CCCC_HOME/state; never persisted into ledger)
  profile_id?: string            // optional Actor Profile link (runtime/command/submit/env + secrets)
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
- The appended `actor.add` event starts that actor id's current generation. The daemon MUST initialize the new generation's read boundary at that append position, so events from before the add are not delivered as unread. Removing and later re-adding the same actor id starts a new generation at the later `actor.add` position.
- A new actor generation MUST NOT inherit Web Model delivery preferences or persisted runner/turn status left by an earlier generation with the same actor id.
- For a Web Model actor, successful add MUST establish the current generation's missing browser target as canonical empty state. A legacy actor-scoped browser shadow MUST NOT populate the new generation merely because it uses the same actor id.
- Adding an enabled actor to an `active` or `idle` group MAY start it immediately and transition the group's runtime to running. Adding one to a `paused` or `stopped` group MUST only persist the actor and MUST NOT change the group lifecycle state.
- When immediate startup is attempted, startup capability baselines follow the same rules as `actor_start` below.

Result:
```ts
{
  actor: Record<string, unknown>
  event: CCCSEventV1
  running?: boolean
  start_event?: CCCSEventV1
  start_error?: string
}
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
- Runtime: `runtime`, `command`, `submit`
- Scope: `default_scope_key`
- Enable/disable: `enabled`
- Environment (use with care): `env`
- Capability startup baseline: `capability_autoload`

Result:
```ts
{ actor: Record<string, unknown>; event: CCCSEventV1 }
```

#### `actor_remove`

Args:
```ts
{ group_id: string; actor_id: string; by?: string }
```

Notes:
- Removing an actor ends that actor id's current generation. Actor-generation-scoped browser target, bootstrap, delivery receipt, delivery preference, and persisted runner/turn state MUST be retired before the operation reports success. A shared provider login profile MAY remain.
- Every remote connector credential bound to the removed actor generation MUST be revoked before the operation reports success. Re-adding the same actor id MUST NOT restore authority to a connector from an earlier generation.

Result:
```ts
{ actor_id: string; event: CCCSEventV1 }
```

#### `actor_start` / `actor_stop` / `actor_restart`

Kilo uses the same managed-session ownership as OpenCode: a private authenticated
loopback ACP backend and a writable native TUI attached to the exact session.
Actor and Voice Analyst share this adapter. Kilo configuration, MCP, model/variant
synchronization, input readiness, cancellation and result settlement MUST follow
the same boundaries; Kilo storage identity MUST include `KILO_DB` and its effective
home/config roots. Starting or resuming an empty session MUST NOT submit a prompt.
Only matching Kilo managed receipts may resume; legacy terminal receipts are not
adopted.

On Windows, Kilo's official npm `kilo.cmd` entrypoint MUST be supported for both
global and project-local installations. Actor and Analyst MUST use the same
resolved launch prefix for the ACP backend and native TUI. The installed npm
JavaScript launcher retains ownership of platform/binary selection and resource
setup; CCCC launches it with Node without shell reinterpretation of arguments,
preserving the configured environment and owned process-tree containment.

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
- A daemon-launched actor whose executable is directly identified as `codex` MUST use one daemon-owned Codex app-server thread and MUST attach Codex's writable native TUI to that exact thread. Unsupported subcommands, wrappers, or prompt tails fail explicitly instead of silently selecting another transport. The app-server and TUI MUST receive the same executable, supported Codex global arguments, profile/model/provider configuration, and private environment. CCCC-owned listener, MCP identity, approval, and sandbox settings remain host-controlled. Stop/start MUST validate and resume the same version-2 managed receipt only when Runtime, workspace, command, model, and effective Codex storage identity still match. Legacy Codex receipts MUST NOT be resumed.
- A daemon-launched `claude` actor MUST use one CCCC-owned Claude Agent View background session and MUST start `claude attach` against that exact session. The resolved executable MUST report Claude Code 2.1.259 or newer; the supervisor MUST validate both the installed executable and the live worker version, the protocol-v1 control response shape, and the credential-file boundary. Version, protocol, or credential drift MUST fail closed. CCCC observes turn ownership and terminal settlement from the append-only provider transcript. A single retryable control-query failure MUST NOT invalidate a still-live session; sustained inability to verify liveness or confirmed job absence MUST disconnect it. CCCC owns background/session/attach, name, MCP identity, autonomy, and resume arguments. Runtime Profile environment values MUST be merged into one stable, owner-scoped, CCCC-protected settings file because Agent View deliberately strips arbitrary process environment from persisted jobs and stores that file path in its durable respawn metadata; raw values MUST NOT appear in the job record, terminal command, receipt, or logs. An ordinary process stop MUST retain this file while the durable session receipt remains resumable. The copy MUST be atomically replaced when that owner's effective settings change and removed when the managed session identity, Actor, or Group is retired. Stop MUST report success only after the Agent View job is confirmed absent. Start MUST validate and resume the same version-2 managed receipt only when Runtime, workspace, command, and the complete effective Claude launch identity, including content of file-backed settings and prompt inputs, still match. A live idle matching session MAY be re-adopted; an active, ambiguous, copied, or identity-mismatched session MUST fail or start fresh according to the existing receipt boundary and MUST NOT be guessed. Legacy Claude Hook and print-mode receipts MUST NOT be resumed.
- A daemon-launched `grok` actor MUST use one CCCC-owned managed session. CCCC starts a dedicated private Grok leader, connects its ACP observer, and attaches the native writable Grok TUI to the same provider session. It injects the actor-scoped CCCC MCP server at session creation and treats structured lifecycle events as working/completion authority. Stop/start MUST validate and load the same version-2 managed receipt when its Runtime, workspace, command, model, and effective provider-home identity still match. Legacy raw-terminal Grok receipts MUST NOT be resumed.
- A daemon-launched `opencode` actor MUST use one CCCC-owned managed session. CCCC starts `opencode acp` with a generation-scoped authenticated loopback backend, observes the ACP session over stdio, attaches `opencode attach` to that exact session, and injects the actor-scoped CCCC MCP server at session creation. The resolved executable MUST report OpenCode 1.18.14 or newer; older releases can return from `session/prompt` before their final output update and therefore cannot satisfy the completion fence. ACP updates and the authenticated session-status stream are lifecycle authority; a lost or malformed non-replayable stream invalidates the session. A model selection made in the native TUI becomes authoritative for later CCCC-managed prompts when the user submits the next TUI message; CCCC MUST mirror that message's exact provider/model and variant into the same ACP session. An explicit runtime-command `--model` remains the launch-time override. Stop/start MUST validate and load the same version-2 managed receipt when its Runtime, workspace, command, model, and effective OpenCode storage identity still match. Legacy raw-terminal OpenCode state MUST NOT be resumed.
- Managed runtime startup MAY synchronously enumerate the injected actor-scoped CCCC MCP tools before its provider session becomes ready. That catalog discovery MUST use `capability_state` with `view="mcp_catalog"` so it cannot wait on the same Group lifecycle lock held by `actor_start`; ordinary capability reads remain serialized normally.
- For Codex, Claude, Grok, OpenCode, and Kilo Actors, CCCC MUST hand an incoming Actor delivery to the writable native TUI as soon as that terminal is ready. CCCC MUST NOT inspect provider busy state to choose `steer` versus `queue`, and MUST NOT hold the delivery until the current turn settles. The receiving Runtime owns that policy according to its own configuration. `runtime.delivery=accepted` means the canonical input and submit sequence were written successfully to the Runtime terminal; it does not claim that the provider completed or semantically accepted the work. Structured protocols remain authoritative for session identity, lifecycle, progress, completion, cancellation, and Voice Analyst delegation.
- Realtime Voice owns the intent decision to create a Voice Analyst delegation; it does not own provider scheduling. Once `delegation.created` exists, CCCC MUST immediately hand the exact correlated input to the managed Runtime and MUST NOT hide it in a server-side wait-for-idle queue. An active Runtime with a verified exact-turn steer operation MAY receive the input through that operation; otherwise CCCC MUST write the exact payload and submit sequence to the same verified native terminal session, after which the Runtime owns the steer-versus-queue decision. CCCC MUST register correlation before the write, project whichever authoritative turn consumes it, and report success only after the Runtime control operation or complete terminal submit sequence was accepted. A missing, closed, or rejecting Runtime input path MUST return an explicit delivery error; busy state alone MUST NOT drop, delay, merge, or reject the delegation.
- Voice Analyst Runtime settings MUST NOT change while a Realtime Voice call is active. After the call stops, active or queued Analyst work MUST block an ordinary settings update rather than being discarded implicitly. An interactive administrator MAY explicitly confirm discarding that work as part of the same settings transaction; CCCC MUST then stop the old managed session before applying the replacement and MUST report whether unfinished work was discarded. Candidate-launch failure MUST restore the prior settings and Runtime, but MUST NOT claim that explicitly discarded work was recovered.
- Claude transcript entries MUST use the provider `promptId` as the durable provider-turn identity and MUST NOT infer identity from the Agent View summary headline. A human prompt observed before control acceptance is an external turn. Because the authenticated `reply` response does not expose that `promptId`, CCCC MAY return a stable local turn receipt as soon as the control request is accepted, but Voice ownership, progress, and results become authoritative only when the next transcript user record exactly matches the one pending controlled prompt and supplies its provider identifier. A competing prompt plus successful control acceptance is ambiguous and MUST invalidate the managed session rather than replaying the delivery. A controlled request that never starts, or settles without exposing the matching transcript, MUST fail within bounded post-acceptance or post-settlement intervals; active provider work MUST NOT expire solely because its turn is long. `turn_duration`, the provider interruption marker, and an explicit failure record are terminal authority. The state file and selected transcript file identity MUST be revalidated while following the session. Losing, truncating, rotating, replacing, or malformedly extending the non-replayable transcript tail, including an incomplete record that does not settle within a bounded interval, MUST invalidate the session.
- A native-terminal delivery MUST wait for the TUI's advertised input mode before writing any payload; a PTY handle alone does not prove input readiness. Readiness waiting MUST be bounded and cancellable and MUST NOT inject a probe prompt, fall through on timeout, or wait for provider work to finish. Actor deliveries remain unaccepted on readiness failure; Voice native-input deliveries report an explicit error without writing the payload.
- A Claude receipt MAY name an empty session that has never created a transcript. Before respawning it, CCCC MUST inspect the existing durable job metadata: only positive empty-input evidence with no transcript path, consumed transcript bytes, output, or token usage permits transcript-free recovery. An absent/null or numeric zero output-token counter does not indicate usage; nonzero or malformed counters MUST retain strict transcript validation. Unknown or materialized history MUST retain strict transcript validation, including a transcript written before Agent View publishes its path. This exception MUST preserve the exact provider session ID and MUST NOT replay old input or create a replacement conversation silently.
- Observer failure MUST NOT be treated as proof of provider process exit. Actor and Analyst teardown MUST use confirmed provider stop, and a failed stop MUST retain retryable ownership rather than mark the job stopped. Normal managed-client shutdown MUST explicitly terminate event readers even when the session still retains the event sender; observers MUST distinguish expected closure from a failure and MUST NOT emit duplicate stop events.
- Managed protocol output received before a protocol-originated request is admitted MUST be buffered within fixed byte and event-count bounds. This rule applies to Voice Analyst and internal control requests; Actor message delivery uses the native-TUI rule above. Once the provider authoritatively accepts a protocol request, CCCC MUST publish buffered lifecycle updates in order. A bounded post-response drain MAY be enabled only as an explicit provider-specific normalization policy.
- Voice result projection MUST reconcile the authoritative final with the exact already-projected prefix. A different final MUST NOT be discarded merely because progress was streamed. Result accumulation is bounded to 32 KiB; overflow without a bounded authoritative final MUST settle as `result_too_large`, not as successful truncated output. The Voice port MUST report that limitation without terminating the warm Analyst or the audio call. Context sends, provider context receipts, and completed speech turns MUST remain distinct observations; receipt absence MUST NOT trigger blind replay, and receipt presence MUST NOT be treated as proof that every fact was spoken.
- The browser Voice control socket MAY report `provider_error` with an `error` object containing bounded provider `code`, `type`, `event_id`, and `param` identifiers. This is transport diagnostics only: it MUST NOT start, cancel, or replay Analyst work. The Web port MUST validate these identifiers and correlate its diagnostic with the active call generation; it MUST NOT log arbitrary browser payloads, provider error messages, or credentials. The browser MUST distinguish a provider error from an Analyst failure and retain the provider code in its visible error when available. Error reporting alone MUST NOT change provider recovery policy.
- Actor start, restart, new-session, and daemon restoration MUST NOT submit a model turn solely to initialize an Actor or materialize a provider session. They create or resume the daemon-owned control session and attach its native terminal while the model remains idle. Only real input—a pending CCCC delivery or human terminal input—may start model work. The CCCC startup prompt MUST be deferred to and combined with the first successfully accepted CCCC delivery, MUST NOT be sent as a standalone turn, and MUST remain pending if that delivery is not accepted. Recovery of an actual pending Send is a valid work trigger; lifecycle operations alone are not.
- A newly created Codex thread MUST be durably resumable before CCCC records a usable receipt or attaches its native TUI. `thread/start` returning an ID and planned rollout path is insufficient. CCCC MUST materialize the empty thread through native metadata operations and verify full-history readability for that exact thread, without submitting a model turn or adding synthetic conversation items. Resumed conversations MUST retain their existing names and history.
- A provider process exit MUST record `actor.stop` with `by="system"` and `data.reason="process_exit"`, but MUST NOT disable the actor or stop the Group. A user-authored Send or Request Reply to an actor is also an explicit wake action: it MUST enable the targeted actor, move a paused or stopped Group to `active`, and start delivery through the normal runtime path whether the prior stop was automatic or user initiated. Mail and previously queued work MUST NOT independently wake a runtime while a Group remains `paused`.
- If the linked profile includes `capability_defaults`, daemon applies baseline capability enables through capability control plane before launch.
- Daemon also applies role defaults and the actor's `capability_autoload` before launch. These are durable desired capability bindings, so they remain applied when the subsequent runtime launch fails.
- A daemon-launched runtime process MUST resolve an explicit existing attached scope from the actor default or group active scope. It MUST return `missing_project_root`, `scope_not_attached`, or `invalid_project_root` as applicable and MUST NOT fall back to the daemon working directory. An explicitly external structured executor may omit a local process only when its product capability and documentation say so.
- The `kimi` runtime targets the current Kimi Code native TUI. Actor startup and CLI setup MUST share MCP configuration at `KIMI_CODE_HOME/mcp.json` (defaulting to the user's `.kimi-code/mcp.json`), preserve unrelated servers, and reject malformed documents without overwriting them. A project `.kimi-code/mcp.json` entry for `cccc` takes precedence; a conflicting project entry MUST fail setup without modification, unless it is the user-level configuration file itself. CCCC MUST NOT infer the active client from legacy directories or invoke the removed `kimi mcp add` command. MCP setup MUST NOT write provider trust records. Native input-mode signals do not establish first-use initialization readiness; users must finish startup dialogs before PTY delivery. Kimi's explicit session arguments are preserved, but CCCC does not claim managed session-ID capture or automatic resume for this runtime.
- A `deepseek` actor has no native terminal surface and MUST use CCCC's structured ACP surface. The daemon MUST install and resolve CCCC's pinned ACP composition from `CCCC_HOME/runtimes/deepseek/<release>` and MUST NOT modify the user's `DSH_HOME`, home-level npm project, or attached project.
- The managed DeepSeek root manifest and lockfile MUST declare exactly `dsh-acp`, `dsh-mcp-client`, `dsh-acp-demo`, and `dsh-llm-deepseek` as direct dependencies. Every installed `@deepseek-ai/dsh*` package MUST remain on the release declared by `crates/cccc-contracts/src/deepseek.rs`; checking only direct package manifests is insufficient.
- Each DeepSeek actor MUST set `CCCC_DEEPSEEK_SESSION_ROOT` to `groups/<group_id>/state/deepseek/<actor_id>/sessions` under the active `CCCC_HOME`. A provider turn MUST reach a successful terminal response within the shared bounded timeout before its source cursor advances; timeout cancellation MUST be durably projected as a failed turn, or the unconfirmed supervisor MUST be stopped. Output and failed-terminal idempotency keys MUST include the provider-attempt identity so a retry cannot be hidden by partial output from an earlier failed attempt; the successful terminal remains idempotent by source event. Crash recovery MUST query that durable per-source completion marker directly (or through its persistent index) and MUST NOT stop recognizing completed turns merely because the append-only headless event log crossed a size or line-count threshold. A permanent credential or context-window failure MUST persist a manual-restart gate before automatic delivery can run again. The gate MUST be bound to both the actor creation identity and the failed provider launch generation, MUST survive daemon restart, and MUST be cleared only after a lifecycle start/restart operation successfully initializes a replacement provider process; daemon restore and message-triggered auto-wake MUST NOT clear it. A late failure from a replaced generation MUST NOT close the replacement actor's gate.
- The managed `dsh-llm-deepseek` profile MUST set `maxTokens` to the shared `DEEPSEEK_MAX_OUTPUT_TOKENS` contract value (currently 65,536), preserving input/tool headroom instead of inheriting the upstream 256k output reservation. Credential absence and provider context-window overflow are permanent for the current runtime session: both MUST be normalized to stable, secret-free failed-turn errors and MUST stop automatic retries until a lifecycle start/restart successfully initializes the actor again.

#### `actor_new_session`

Args:
```ts
{ group_id: string; actor_id: string; by?: string }
```

Result:
```ts
{ actor: Record<string, unknown>; event: CCCSEventV1; new_session: true }
```

Notes:
- Supported for Antigravity, `claude`, `codex`, Grok, OpenCode, and Kilo actors.
- A running Antigravity actor starts a fresh provider conversation through its native `/clear` boundary while preserving the authenticated PTY process. A stopped Antigravity actor starts normally with the same runtime settings.
- Other supported runtimes stop the current actor process if present, clear CCCC's saved runtime session metadata for that actor, then start the actor with the same runtime settings.
- Does not delete provider-side conversation/session history.

#### `runtime_hermes_status`

Return Hermes runtime setup diagnostics for the selected user Hermes profile.

Args:
```ts
{}
```

Result:
```ts
{
  runtime: "hermes"
  setup_ready: boolean
  auth_ready: boolean
  launch_ready: boolean
  hermes_cli: { available: boolean; path?: string; version?: string }
  hermes_home: string
  profile: { name: "default"; dir: string; exists: boolean; config_path: string; config_exists: boolean }
  mcp: Record<string, unknown>
  auth: Record<string, unknown>
  phase0_gates: Array<Record<string, unknown>>
  issues: string[]
}
```

Notes:
- CCCC does not create or select a separate Hermes profile.
- `HERMES_HOME`, when supplied by the user, is treated as ordinary runtime environment.
- `mcp.env` must persist `${CCCC_HOME}`, `${CCCC_GROUP_ID}`, and `${CCCC_ACTOR_ID}` placeholders so each actor process resolves its own CCCC identity.

#### `runtime_hermes_prepare`

Configure the `cccc` MCP server in the selected Hermes profile through Hermes' official MCP setup flow.

Args:
```ts
{
  cwd?: string
  auto_enable_tools?: boolean // alias: yes
  force_mcp?: boolean         // alias: force
}
```

Result:
```ts
{
  ok: boolean
  commands_run?: Array<Record<string, unknown>>
  status: Record<string, unknown>
  error?: { code: string; message: string }
}
```

Notes:
- Setup MAY invoke `hermes mcp add cccc ...` and answer Hermes' discovery prompt only when `auto_enable_tools`/`yes` is true.
- Discovery uses concrete CCCC env values, then CCCC normalizes saved Hermes MCP env back to actor-time placeholders.

#### `runtime_hermes_mcp_test`

Run Hermes' MCP test command for the configured `cccc` server with probe CCCC actor env.

Args:
```ts
{
  cwd?: string
  group_id?: string
  actor_id?: string
}
```

Result:
```ts
{
  ok: boolean
  argv: string[]
  result?: { returncode: number; stdout: string; stderr: string }
  error?: { code: string; message: string }
}
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
    command?: string[] | string
    submit?: "enter" | "newline" | "none"
    env?: Record<string, string> // deprecated legacy input; values are migrated into profile secrets
    capability_defaults?: {
      autoload_capabilities?: string[]
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
- `command` accepts the historical shell-command string for compatibility, but successful writes normalize it to a `string[]`. Readers must continue to accept an existing string until that profile is saved again.

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

#### `actor_profile_copy_voice_analyst_secrets`

Copy the Voice Analyst custom private environment into a Runtime Profile without returning values.
This operation is administrator-only and is used when the Voice settings surface saves its Custom
configuration as a reusable Profile.

Args:
```ts
{
  profile_id: string
  profile_scope?: "global" | "user"
  profile_owner?: string
  by?: string
}
```

Result:
```ts
{ profile_id: string; keys: string[] }
```

### 8.6 Chat Messaging

#### `send`

Append a `chat.message` event. `message_mode` is required and is the only
chat-delivery selector.

Args (core):
```ts
{
  group_id: string
  text: string
  by?: string
  to?: string[]                 // empty/omitted materializes group default_send_to
  message_mode: "send" | "request_reply" | "mail"
  path?: string                 // optional filesystem path to attribute scope_key
  attachments?: unknown[]       // attachment refs (implementation-defined)
  refs?: ReferenceV1[]          // structured message refs, e.g. presentation_ref/task_ref
  insight?: string              // optional provisional sender perspective; max 1200 characters
  require_peer_insight?: boolean // profile gate; default false
  src_group_id?: string         // relay provenance (both required if either is set)
  src_event_id?: string
  dst_group_id?: string         // optional "send record" metadata (source messages)
  dst_to?: string[]
  dst_message_mode?: "send" | "request_reply" | "mail"
}
```

Result:
```ts
{
  event: CCCSEventV1 // kind="chat.message"
  message_mode: "send" | "request_reply" | "mail"
}
```

`send` and `request_reply` preflight the concrete runtime audience, append the
message, and then attempt prompt delivery. `mail` appends only; it MUST NOT wake,
steer, queue, open a browser, or write to a runtime input. `request_reply` MUST
resolve to explicit concrete recipients and rejects broadcast selectors. In the
Rust implementation, omitted recipients and `@foreman` resolve to the current
enabled Foreman actor ID before this validation. New daemon callers MUST send
`message_mode`; missing or old `priority`,
`reply_required`, or `requires_ack` fields fail validation.
After aliases and selectors are normalized, one message MUST address either the
human user or one or more agents, never both. `mail` is valid only for agent
recipients. Mixed audiences fail with `mixed_recipient_kinds`; Mail addressed to
the user fails with `mail_requires_actor_recipient`. Validation occurs before
the message, blobs, delivery claims, or other side effects are written.
The immediate response confirms ledger acceptance and echoes the canonical mode;
it is not transport evidence. Per-recipient delivery truth is reported only by
daemon-authored `runtime.delivery` events and the corresponding status queries.

Every daemon-rendered `chat.message` handed to an actor runtime MUST expose the
current ledger identity before the message body:

```text
[cccc] <sender> → <recipients> [event_id=<current_event_id>]: <body>
[cccc] <sender> → <recipients> (reply:<short_parent_id>) [event_id=<current_event_id>]: <body>
[cccc] <sender> → <recipients> [event_id=<current_event_id> reply_required]: <body>
```

`event_id` is the target an actor passes to `cccc_message_reply` when answering
the delivered message. A reply MAY expose its parent as the existing short
`(reply:<id>)` correlation hint; it MUST NOT repeat the full parent `reply_to`
in the runtime envelope. The ledger remains authoritative for the full parent
identity and canonical `message_mode`. Ordinary `send` and delivered `mail`
messages need no mode label because the label does not change the receiver's
next action. A `request_reply` message MUST instead add the actionable
`reply_required` marker in the same metadata block. Implementations MUST use
the same envelope in native-terminal, structured, and Web Model delivery. The metadata MUST
remain in the existing first header line so adding it does not turn a one-line
message into multiple runtime input lines. It does not add or mutate ledger
fields, and it is not required for `system.notify`.
Every delivered batch containing one or more `chat.message` events MUST append
one concise instruction telling the actor to call `cccc_message_reply` with its
`event_id` argument set to the target message's current `event_id`. The
instruction MUST reserve
`cccc_message_send` for a new message and MUST appear only once per batch.

Native MCP adapters MUST preserve a failed daemon operation's `error.code`,
`error.message`, and non-empty `error.details` in the tool result's
`structuredContent.error`; flattening the daemon error into text is not
conforming. After a message operation has durable success evidence (an accepted
event, successful queued/retrying/sent receipt, or equivalent file-send
wrapper), its MCP result MUST add `post_message_nudge` with
`kind="whole_situation_reconstruction"`. Partial failures, failed receipts, and
embedded Group Bridge reply errors MUST NOT claim completion or add that field.
This private result context is independent of the passive `mail_pending`
summary, so a successful operation MAY carry both.

#### `message_upload_preflight`

Validate a Web-owned staged upload before its temporary files are committed to
the group blob store. This operation is side-effect free and exists so both Web
implementations use the selected daemon's canonical send, reply, and Group Bridge
rules rather than reimplementing message policy in the HTTP port.

Args:
```ts
{
  operation: "send" | "reply" | "send_cross_group"
  group_id: string
  dst_group_id?: string         // required when operation="send_cross_group"
  text?: string
  by?: string
  to?: string[]
  message_mode: "send" | "request_reply" | "mail"
  reply_to?: string             // required when operation="reply"
  path?: string
  client_id?: string
  refs?: ReferenceV1[]
  insight?: string
  require_peer_insight?: boolean
  has_attachments: boolean      // temporary upload parts exist; no blob refs yet
}
```

Result:
```ts
{ ready: true }
// or, for send/reply when client_id already identifies an accepted message:
{
  ready: false
  duplicate: true
  result: { event: CCCSEventV1; message_mode: "send" | "request_reply" | "mail" }
}
```

The operation MUST perform the deterministic validation used by the eventual
`send`, `reply`, or `send_cross_group`, including mode, audience, target, scope,
Insight, and content, without waking actors, changing group state, writing the
ledger, storing blobs, or starting delivery. `send` and `reply` additionally
perform successful-idempotency lookup and MUST return a duplicate result before
an upload is committed. Group Bridge retry and receipt idempotency remain owned
by `send_cross_group`. The HTTP port MUST discard its staged files on rejection
or duplicate replay. The eventual operation MUST validate again at the commit
boundary; preflight is not a reservation.

#### `send_files`

Store one or more files from the group's active scope in the group blob store,
then append one `chat.message` carrying those files as attachments. This is the
daemon-owned upload boundary for SDK clients; callers MUST NOT write directly
to `state/blobs/` or manufacture attachment records.

Args:
```ts
{
  group_id: string
  paths: string[]               // absolute, or relative to the active scope root
  text?: string                 // defaults to a compact file notice
  by?: string
  to?: string[]
  message_mode: "send" | "request_reply" | "mail"
  insight?: string
  client_id?: string
}
```

Every resolved path MUST be a regular file beneath the group's active scope.
All paths are validated and read before any message is appended. The resulting
event uses the selected `message_mode` recipient, permission, and delivery rules.
If `client_id` already identifies an accepted message, the daemon MUST return
that message before reading or storing new source content. After source paths
have been validated and read, deterministic normal-send validation (including
message mode, recipients, and required peer insight) MUST succeed before any new
blob is stored. A request rejected by that preflight MUST NOT add a blob or
ledger event.

Result:
```ts
{
  event: CCCSEventV1 // kind="chat.message", data.attachments contains stored blobs
  message_mode: "send" | "request_reply" | "mail"
}
```

#### `reply`

Append a `chat.message` with `reply_to` and `quote_text`. When `reply_to`
references an inbound `group_bridge_session` event, the daemon MUST resolve the
active trust from the preserved source group and peer, keep one local reply
record, and relay the reply to the preserved remote event and recipient. The
`group_bridge:<peer>` provenance sender is never a local recipient token.

Args:
```ts
{
  group_id: string
  reply_to: string
  text: string
  by?: string
  to?: string[]                 // local: original sender; Group Bridge: preserved remote return target
  message_mode?: "send" | "mail" // default: "send"
  attachments?: unknown[]
  refs?: ReferenceV1[]
  insight?: string
  require_peer_insight?: boolean // profile gate; default false
}
```

Replies default to `message_mode="send"`. Callers MAY choose
`message_mode="mail"` when fulfilling the original reply obligation does not
justify immediately prompting the recipient. Both modes fulfill the original
reply obligation; `message_mode="request_reply"` is invalid for a reply and a
reply cannot create another generic reply obligation.
Mail replies are valid only when every reply recipient is an agent. A reply to
the human user MUST use Send. Replies also reject a recipient list that mixes
the human user with agents.

Result:
```ts
{
  event: CCCSEventV1 // kind="chat.message"
  message_mode: "send" | "mail"
  group_bridge_reply?: { receipt?: unknown, error?: unknown }
}
```

#### `tracked_send`

Create a durable task and send one linked visible delegation message. This is an
explicit composite write; the daemon MUST NOT infer it from arbitrary chat text.

Args:
```ts
{
  group_id: string
  title: string
  text: string
  by?: string
  to?: string[]
  outcome?: string
  checklist?: { text: string; status?: "pending" | "in_progress" | "done" | string }[]
  task_priority?: string        // task-domain priority; does not affect message delivery
  assignee?: string             // defaults from one concrete to actor when possible
  waiting_on?: "none" | "user" | "actor" | "external"
  handoff_to?: string
  notes?: string
  idempotency_key?: string
  refs?: ReferenceV1[]
  insight?: string
  require_peer_insight?: boolean // profile gate; default false
}
```

Result:
```ts
{
  task_id: string
  task_ref: ReferenceV1          // kind="task_ref"
  event?: CCCSEventV1            // present when message_sent=true
  event_id?: string
  message_mode?: "send"          // present when message_sent=true
  task_created: boolean
  message_sent: boolean
  partial_failure: boolean
  replayed?: boolean
}
```

Notes:
- `task_ref` in the emitted `chat.message.data.refs` is the canonical message-task link.
- The linked visible message always uses `message_mode="send"`; task lifecycle
  is the only completion authority for tracked work.
- `priority`, `message_priority`, and `reply_required` are rejected; callers use
  `task_priority` only for the task-domain field.
- If task creation fails, no message is sent.
- If message delivery fails after task creation, the response MUST report `partial_failure=true`.
- Successful retries SHOULD use `idempotency_key` / `client_request_id` to avoid duplicate task/message pairs.

#### `send_cross_group`

Cross-group send implemented as:
1) Write a source `chat.message` in the origin group as a local Send to `user`,
   with the actual remote `dst_group_id`, `dst_to`, and `dst_message_mode`
   metadata.
2) Write a forwarded `chat.message` in the destination group with `src_group_id` / `src_event_id` provenance.

Args:
```ts
{ group_id: string; dst_group_id: string; text: string; by?: string; to?: string[]; message_mode: "send" | "request_reply" | "mail"; insight?: string; require_peer_insight?: boolean }
```

Result:
```ts
{ src_event: CCCSEventV1; dst_event: CCCSEventV1 }
```

Notes:
- Local `user` / `system` principals and registered source actors, including
  peers, may send cross-group messages; unknown source actors are rejected. The
  foreman-only group administration permission does not apply to message delivery.
- Attachments are supported only when the destination is an active remote Group
  Bridge route. Local cross-group forwarding rejects attachments.

#### Agent Insight Profile marker

`require_peer_insight` is an internal request-profile marker, not a global message-validity rule. It defaults to `false`. When `true`, the daemon resolves the operation's real audience and rejects a new peer-facing message whose normalized `insight` is empty. User-only sends remain valid without Insight.

The check MUST occur after routing and successful-idempotency lookup, but before this request creates a new message, task, actor wake, or remote outbox entry. The recommended error code is `peer_insight_required`, with `details.delivery_state="not_sent"` and `details.new_side_effects=false`. Invalid Insight type or length SHOULD use `invalid_insight` instead. Existing accepted idempotent operations MUST replay their original result without being reinterpreted by a newer profile requirement.

Group Bridge peers MUST advertise the current message contract version before
messages are exchanged. There is no legacy field mapping or silent downgrade.

#### `reply_request_cancel`

Cancel every still-open recipient obligation for an existing
`message_mode="request_reply"` message.

Args:
```ts
{ group_id: string; source_event_id: string; by?: string }
```

Result:
```ts
{ event: CCCSEventV1 } // kind="chat.reply_request.cancelled"
```

Only the source sender or `user` may cancel. The operation is idempotent for an
already-cancelled source event.

#### `message_deliver`

Explicitly attempt prompt delivery for one existing message without appending
a second `chat.message`. This promotes Mail or retries a blocked/failed Send.

Args:
```ts
{
  group_id: string
  source_event_id: string
  actor_ids: string[]
  by?: string
  force_ambiguous?: boolean // default false; explicit warning/confirmation required
}
```

Only the source sender or `user` may request delivery. Recipients MUST be
explicit concrete recipients of the source event. Existing `accepted` evidence
is never retried. Existing `claimed` evidence reports `delivery_in_progress`.
Existing `ambiguous` evidence is rejected unless `force_ambiguous=true`.
Disabled recipients return `delivery_blocked` without creating a delivery
claim. A request that reserves new claims while the Group is paused or stopped
explicitly resumes the Group before handoff. A conflicting, already accepted,
or otherwise no-op request MUST NOT change lifecycle state. A successful
request records all claims before returning:

```ts
{
  event: CCCSEventV1
  actor_ids: string[]
  delivery_state: "claimed"
}
```

Terminal per-recipient states remain authoritative in `runtime.delivery` and
the normal message-status projection; they may settle before or after this
operation returns.

#### Delivery evidence, Mail notice, and reply notice

For each concrete recipient handoff, the daemon appends `runtime.delivery`
using the CCCS contract. `claimed` precedes external I/O; `accepted`, `failed`,
or `ambiguous` records the outcome. A transport queue accepting a payload is an
accepted handoff; it is not a claim that the model understood it. Automatic
retry is forbidden after `accepted` or `ambiguous`. Concurrent claimants treat
`claimed` as in-progress. Daemon startup settles claims stranded by the prior
daemon process to `ambiguous` before attempting runtime recovery.
Recovery MUST interpret a same-generation legacy `chat.read.event_id` as an
inclusive ledger watermark for that actor, not as a per-event receipt. It MUST
use the furthest valid referenced ledger position and exclude each legacy
`system.notify` at or before that position. A later notification MUST also be
excluded when `data.event_id`, `data.related_event_id`, or
`data.context.event_id` references an event at or before the watermark. Targets
outside the current actor generation or after their `chat.read` record are
invalid compatibility boundaries and MUST be ignored. This rule prevents
already-consumed pre-`runtime.delivery` nudges from being replayed after an
upgrade.

Mail is eligible for an active notice for one recipient only while all of these
hold: the source event belongs to the recipient's current actor generation, is
unread, has `message_mode="mail"`, has no reply from that recipient, and has no
accepted or ambiguous manual-delivery record. Reply and manual delivery suppress
only the active notice; neither advances the Mail cursor nor removes the message
from `inbox_peek` / `inbox_read`. Concrete-recipient Mail batches retain the
earliest eligible deadline. New Mail does not reset it. Broadcast-like Mail
remains visible in the Inbox but does not start an active runtime notice timer.

After `mail_notice_after_seconds`, an active/idle enabled actor may receive one
content-free `system.notify(kind="mail_notice")` for the concrete batch. The
notice states only that Mail is waiting and directs the actor to `inbox_read`;
it does not copy message bodies, repeat, escalate, or create another Inbox
obligation. Bootstrap, the next explicit Push, and low-frequency coordination
responses MAY carry a passive `mail_pending` count without writing a notice.

A `request_reply` obligation starts its timer only after an accepted runtime
delivery. If no matching `reply_to`
message or cancellation has closed it by `reply_notice_after_seconds`, an
active/idle enabled actor may receive one content-free
`system.notify(kind="reply_notice")`. It never repeats or escalates. Failed,
blocked, or ambiguous delivery does not nudge the recipient.

Paused, stopped, or disabled actors are never woken for either notice. Pending
Mail is surfaced at the next start/resume bootstrap and begins a fresh notice
window. Explicit start/resume may recover only blocked Push work from the
current actor generation that has no accepted/ambiguous delivery evidence;
Mail is never automatically promoted. No implementation may depend on a
universal runtime "idle" detector.

### 8.7 Inbox (Mail Cursor)

#### `inbox_peek`

Return unread `chat.message` events whose `message_mode="mail"` without changing
the Mail cursor. This is for UI polling, bootstrap previews, and diagnostics;
agent tooling SHOULD use `inbox_read`. Send, Send + Reply, and `system.notify`
events are never members of this projection.

The current actor generation begins at the latest `actor.add` for that actor id
in ledger append order. Inbox membership and Mail read status MUST exclude
events before that boundary, including after an actor is removed and re-added
with the same id. When a cursor contains a resolvable `event_id`, cursor
advancement and unread membership MUST use ledger append order; timestamp is
informational only.

Args:
```ts
{ group_id: string; actor_id: string; by?: string; limit?: number }
```

Result:
```ts
{ messages: CCCSEventV1[]; cursor: { event_id: string; ts: string } }
```

#### `inbox_read`

Atomically return and consume the next unread Mail prefix for an actor.

Args:
```ts
{ group_id: string; actor_id: string; by?: string; limit?: number }
```

Result:
```ts
{
  messages: CCCSEventV1[]
  cursor: { event_id: string; ts: string; updated_at: string }
  event: CCCSEventV1 | null
}
```

Selection, `mail.read` append, Mail cursor persistence, and returned messages
MUST be one consuming operation under the canonical ledger/cursor transaction
boundary. Ordering is the append order of the Mail projection: non-Mail events
between two Mail events are intentionally skipped and do not acquire read
state. If the event or cursor write fails, the operation returns no message
bodies. An empty Inbox returns `messages=[]` and `event=null` without moving the
cursor.

The ledger `mail.read` event is the authoritative commit record; the cursor file
is a rebuildable projection. Implementations MUST persist
`state/read_cursors.pending.json` before appending `mail.read`, append the event
before advancing the cursor projection, and clear the marker after both writes
commit. The shared recovery marker is:

```json
{
  "schema": 1,
  "group_id": "...",
  "actor_id": "peer1",
  "expected": {"event_id": "...", "ts": "..."},
  "target": {"event_id": "...", "ts": "...", "updated_at": "..."}
}
```

After interruption, a matching `mail.read` fact completes the target cursor;
without that fact the marker is discarded and the old cursor remains. Recovery
MUST never move an already-later cursor backward. This makes a process exit
between the two durable writes recoverable without replaying or silently
skipping Mail.

The canonical cursor file is `state/read_cursors.json` with
`{"schema":1,"cursors":{...}}`. Documents without this Mail-specific schema
are not delivery boundaries and MUST be ignored; in particular, a cursor from
the former all-message read model cannot suppress Mail.

#### `message_history`

Return an actor-visible, non-consuming history of `chat.message` events. This is
the explicit path for inspecting past Send or Send + Reply traffic; it does not
change the Mail cursor.

Args:
```ts
{
  group_id: string
  actor_id: string
  by?: string
  mode?: "all" | "send" | "request_reply" | "mail"
  query?: string
  before_event_id?: string
  limit?: number
}
```

Result:
```ts
{ messages: CCCSEventV1[]; has_more: boolean }
```

Results are newest-first, limited to the actor's current generation, and MUST
contain only messages sent by or addressed to that actor. The operation is
read-only and MUST NOT append `mail.read` or mutate any delivery state.

### 8.8 Context and Tasks

#### `context_get`

Args:
```ts
{ group_id: string; detail?: "overview" | "summary" | "full" }
```

Result:
```ts
{
  version: string
  tasks_version: string
  coordination: {
    brief: {
      objective: string
      current_focus: string
      constraints: string[]
      project_brief: string
      project_brief_stale: boolean
      updated_by: string
      updated_at: string
    }
    tasks?: Array<Record<string, unknown>>
    recent_decisions?: Array<{ at: string; by: string; summary: string; task_id?: string | null }>
    recent_handoffs?: Array<{ at: string; by: string; summary: string; task_id?: string | null }>
  }
  agent_states: Array<{
    id: string
    hot: {
      active_task_id?: string | null
      focus?: string | null
      blockers?: string[]
      next_action?: string | null
    }
    warm: {
      what_changed?: string | null
      open_loops?: string[]
      commitments?: string[]
      environment_summary?: string | null
      user_model?: string | null
      persona_notes?: string | null
    }
    updated_at?: string | null
  }>
  actors_runtime?: Array<Record<string, unknown>>
  tasks_summary?: {
    total: number
    done: number
    active: number
    planned: number
    archived: number
    root_count?: number
  }
  attention?: {
    blocked?: number | Array<Record<string, unknown>>
    waiting_user?: number | Array<Record<string, unknown>>
    pending_handoffs?: number | Array<Record<string, unknown>>
  }
  board?: {
    planned?: Array<Record<string, unknown>>
    active?: Array<Record<string, unknown>>
    done?: Array<Record<string, unknown>>
    archived?: Array<Record<string, unknown>>
  }
  meta?: Record<string, unknown>
}
```

Notes:
- Task objects returned in `coordination.tasks`, `board`, or `task_list` include `task_type`.
- Daemon IPC defaults `detail` to `full`; the Web HTTP route defaults it to
  `summary` for routine refreshes.
- `detail="overview"` does not read task files and omits `coordination.tasks`,
  `tasks_summary`, `attention`, and `board`. It retains the coordination brief,
  recent decisions/handoffs, agent states, `version`, `tasks_version`, and
  metadata. Web startup and the context modal use this projection before loading
  task pages separately.
- `detail="summary"` omits `board`, recent coordination notes, and live runtime
  probing. Its `attention` fields are counts, but each task in
  `coordination.tasks` MUST retain every task-editor field, including
  `outcome`, `notes`, and `checklist`, so a summary refresh cannot erase a
  client's editable draft.
- `detail="full"` returns the complete task objects and board projections.
- MCP convenience tools MUST preserve their focused response contracts instead
  of exposing this complete snapshot: `cccc_coordination(action="get")` returns
  only `version`, `coordination`, `attention`, `board`, and `tasks_summary`, while
  `cccc_agent_state(action="get")` returns only `version` plus the selected
  `agent_state` (or `agent_states` when no actor is selected). `include_warm=false`
  keeps only `id`, `hot`, and `updated_at`; archived tasks remain hidden from
  coordination unless `include_archived=true`.

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

#### `memory_reme_layout_get`

Args:
```ts
{ group_id: string }
```

Result:
```ts
{
  group_label: string
  memory_root: string
  memory_file: string
  daily_dir: string
  today_daily_file: string
  backend: { name: "local"; vector_enabled: false; fts_enabled: true }
}
```

#### `memory_reme_index_sync`

Args:
```ts
{
  group_id: string
  mode?: "scan" | "rebuild"   // default "scan"
}
```

Result:
```ts
{
  indexed_files: number
  indexed_chunks: number
  watched_paths: string[]
  last_sync_at: string
}
```

#### `memory_reme_search`

Args:
```ts
{
  group_id: string
  query: string
  max_results?: number           // 1..50, default 5
  min_score?: number             // 0..1, default 0.1
  sources?: string[]             // default ["memory"]
  vector_weight?: number         // 0..1 (optional)
  candidate_multiplier?: number  // 1..20 (optional)
}
```

Result:
```ts
{
  hits: Array<{
    path: string
    start_line: number
    end_line: number
    score: number
    snippet: string
    source: string
    raw_metric?: number
    metadata: Record<string, unknown>
  }>
  count: number
  took_ms: number
}
```

#### `memory_reme_get`

Args:
```ts
{
  group_id: string
  path: string
  offset?: number   // 1-indexed, default 1
  limit?: number    // default 200
}
```

Result:
```ts
{
  path: string
  offset: number
  limit: number
  total_lines: number
  content: string
}
```

#### `memory_reme_context_check`

Args:
```ts
{
  group_id: string
  messages: Array<{ role: string; name?: string; content: string }>
  context_window_tokens?: number
  reserve_tokens?: number
  keep_recent_tokens?: number
}
```

Result:
```ts
{
  needs_compaction: boolean
  token_count: number
  threshold: number
  messages_to_summarize: Array<Record<string, unknown>>
  turn_prefix_messages: Array<Record<string, unknown>>
  left_messages: Array<Record<string, unknown>>
  is_split_turn: boolean
  cut_index: number
}
```

#### `memory_reme_compact`

Args:
```ts
{
  group_id: string
  messages_to_summarize: Array<{ role: string; name?: string; content: string }>
  turn_prefix_messages?: Array<{ role: string; name?: string; content: string }>
  previous_summary?: string
  language?: string
  return_prompt?: boolean
}
```

Result:
```ts
{ summary: string } | { prompt: Record<string, string> }
```

#### `memory_reme_daily_flush`

Args:
```ts
{
  group_id: string
  messages: Array<{ role: string; name?: string; content: string }>
  date?: string               // YYYY-MM-DD
  version?: string            // default "default"
  language?: string           // default "en"
  return_prompt?: boolean
  signal_pack?: Record<string, unknown>
  signal_pack_token_budget?: number // default 320
  dedup_intent?: "new" | "update" | "supersede" | "silent" // default "new"
  dedup_query?: string
}
```

Result:
```ts
{
  status: "written" | "silent"
  reason?: "empty_summary" | "precheck_silent" | "persistence_idempotency_key" | "persistence_content_hash"
  target_file: string
  content_hash: string
  bytes_written: number
  signal_pack?: {
    schema: string
    token_budget: number
    token_estimate: number
    truncated: boolean
  }
  dedup?: {
    intent: "new" | "update" | "supersede" | "silent"
    query: string
    candidate_count: number
    top_score: number
    precheck_decision: "new" | "update" | "supersede" | "silent"
    final_decision: "new" | "update" | "supersede" | "silent"
    final_reason: "accepted" | "empty_summary" | "precheck_silent" | "persistence_idempotency_key" | "persistence_content_hash"
    decision: "new" | "update" | "supersede" | "silent" // alias of final_decision
    hits: Array<{ path: string; start_line: number; score: number }>
    error?: string
  }
}
```

#### `memory_reme_write`

Args:
```ts
{
  group_id: string
  target: "memory" | "daily"
  content: string
  date?: string               // required when target="daily"
  mode?: "append" | "replace" // default "append"
  idempotency_key?: string
  actor_id?: string
  source_refs?: string[]
  tags?: string[]
  supersedes?: string[]
  dedup_intent?: "new" | "update" | "supersede" | "silent" // default "new"
  dedup_query?: string
}
```

Result:
```ts
{
  file_path: string
  line_count: number
  content_hash: string
  status: "written" | "silent"
  reason?: "precheck_silent" | "persistence_idempotency_key" | "persistence_content_hash"
  dedup?: {
    intent: "new" | "update" | "supersede" | "silent"
    query: string
    candidate_count: number
    top_score: number
    precheck_decision: "new" | "update" | "supersede" | "silent"
    final_decision: "new" | "update" | "supersede" | "silent"
    final_reason: "accepted" | "precheck_silent" | "persistence_idempotency_key" | "persistence_content_hash"
    decision: "new" | "update" | "supersede" | "silent" // alias of final_decision
    hits: Array<{ path: string; start_line: number; score: number }>
    error?: string
  }
}
```

#### `task_list`

Args:
```ts
{
  group_id: string
  task_id?: string
  task_ids?: string // comma-separated exact ids, at most 100
  status?: "planned" | "active" | "done" | "archived"
  statuses?: string // comma-separated statuses for an atomic multi-column page
  query?: string
  assignee?: string // use "__unassigned__" for tasks without an assignee
  attention?: "blocked" | "waiting_user" | "handoff" | "unassigned"
  offset?: number
  limit?: number // 1..100
  include_index?: boolean
}
```

Result:
```ts
// Exact lookup when task_id is present:
{
  task: Record<string, unknown> & { children: Array<Record<string, unknown>> }
  tasks_version: string
  delete_info: { allowed: boolean; total: number; reason: string }
}

// Batch exact lookup when task_ids is present:
{
  tasks: Array<Record<string, unknown>> // requested order; missing ids omitted
  tasks_version: string
}

// Paged listing when limit is present:
{
  tasks: Array<Record<string, unknown>>
  count: number
  total_count: number
  offset: number
  limit: number
  has_more: boolean
  tasks_version: string
  facets: {
    status_counts: Record<string, number>
    blocked: number
    waiting_user: number
    pending_handoffs: number
    unassigned: number
    assignees: string[]
  }
}

// Atomic multi-column listing when statuses is present:
{
  pages: Partial<Record<"planned" | "active" | "done" | "archived", {
    tasks: Array<Record<string, unknown>>
    count: number
    total_count: number
    offset: number
    limit: number
    has_more: boolean
  }>>
  tasks_version: string
  facets: {
    status_counts: Record<string, number>
    blocked: number
    waiting_user: number
    pending_handoffs: number
    unassigned: number
    assignees: string[]
  }
  task_index?: Array<{
    id: string
    title: string
    status: string
    assignee?: string | null
    parent_id?: string | null
  }>
}

// Compatibility listing when task_id, task_ids, statuses, and limit are absent:
{ tasks: Array<Record<string, unknown>> }
```

Notes:
- Returned task objects include `task_type`.
- `task_id` takes precedence over other arguments, followed by `task_ids`.
  `status` and `statuses` MUST NOT be combined. A `statuses` request reads one
  task snapshot and returns every requested column at the same `tasks_version`.
- `include_index=true` adds an unfiltered, non-archived, lightweight task index.
  It is intended for relationship selectors; it is not a substitute for exact
  task detail.
- Filters are applied before pagination. Planned tasks sort newest-created first;
  other columns sort most-recently-updated first, with numeric task id as a stable
  tie-breaker.
- `offset` requires `limit`. `tasks_version` is the task-specific revision, not
  the broader context revision. Clients MUST discard a continuation response
  and restart all loaded pages when it differs from the initial page revision.

`presence_get` has been removed. Agent state is returned in `context_get.result.agent_states`.

### 8.9 Structured Runtime State

#### `headless_status`

Args:
```ts
{ group_id: string; actor_id: string }
```

Result:
```ts
{ state: Record<string, unknown> }
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
  im_visibility?: "internal" | "public" // default: "internal"
  context?: Record<string, unknown>
}
```

`system.notify` is internal by default. An IM bridge may forward it only when
`im_visibility="public"`; actor-targeted notifications are never eligible for
external IM delivery. Producers must opt in explicitly instead of relying on
`to`, `actor_id`, or `target_actor_id` inference.

Result:
```ts
{ event: CCCSEventV1 } // kind="system.notify"
```

There is no generic notification acknowledgement operation. Domain workflows
must expose domain lifecycle operations; chat reply obligations use
`request_reply` and `reply_request_cancel`.

### 8.11 Terminal Diagnostics and Attach

#### `terminal_tail`

Args:
```ts
{ group_id: string; actor_id: string; by?: string; max_chars?: number; strip_ansi?: boolean; compact?: boolean }
```

`max_chars` limits the final returned Unicode text. Implementations MUST render the complete
retained PTY backlog before applying this limit; truncating the raw ANSI/VT byte stream first can
start replay inside an escape sequence or incremental screen update and produce corrupt snapshots.

Result:
```ts
{ group_id: string; actor_id: string; warning: string; hint: string; text: string; end_cursor: number }
```

`end_cursor` is the exclusive raw PTY byte cursor captured with the backlog used to produce
`text`. A terminal client MAY display the rendered snapshot and then attach its live stream with
`since=end_cursor`; the stream must replay output produced after the snapshot so the transition is
gap-free.

#### `terminal_snapshot`

Return a bounded rendered screen snapshot and the exact raw cursor boundary used to render it.
This operation is intended for diagnostics; interactive clients use `terminal_replay` so they can
rebuild scrollback from the original ANSI stream.

Args:
```ts
{ group_id: string; actor_id: string; by?: string; limit_bytes?: number }
```

Result:
```ts
{ data: string; start_cursor: number; end_cursor: number }
```

The implementation MUST apply the same group transcript visibility policy as `terminal_tail` and
`terminal_history`.

#### `terminal_replay`

Return one bounded page of raw ANSI output from the active PTY session's in-memory ring. This
operation MUST NOT read the durable archive or a completed session. The first request atomically
captures a UTF-8-complete `replay_end_cursor`. Callers pass that value back as `end_cursor` on every
following page, so output produced during replay cannot extend the initial replay loop.

Args:
```ts
{ group_id: string; actor_id: string; by?: string; after?: number; end_cursor?: number; limit_bytes?: number }
```

Result:
```ts
{
  replay_end_cursor: number
  history: {
    data: string
    start_cursor: number
    end_cursor: number
    has_more: boolean
    cursor_expired: boolean
  }
}
```

The default page limit is 512 KiB. `history.has_more` is relative to the fixed
`replay_end_cursor`, not the moving live tail. The implementation MUST apply the same group
transcript visibility policy as `terminal_tail` and must leave an incomplete UTF-8 suffix for a
later live page.

#### `terminal_history`

Args:
```ts
{ group_id: string; actor_id: string; by?: string; before?: number; render_before?: number; limit_bytes?: number; strip_ansi?: boolean; compact?: boolean }
```

Result:
```ts
{
  group_id: string
  actor_id: string
  warning: string
  hint: string
  text: string
  start_cursor: number
  end_cursor: number
  has_more: boolean
  cursor_expired: boolean
}
```

For backward paging of rendered text, pin the first response's `end_cursor` as
`render_before` on subsequent requests. `before` and `limit_bytes` select the next
older page's start, while the returned range extends through `render_before`.
The server renders that contiguous range once; clients replace their previous
rendering rather than concatenate independently rendered pages. The cumulative
range is limited to 50 MB and excludes output written after the pinned end.
If retention or terminal clear overtakes the pinned range, return an empty
page with `cursor_expired=true`, `has_more=false`, and both cursors at the pinned
end, not `invalid_args`. Clients retain already displayed text when the older
cursor does not advance and show the existing expired-history warning.
Omitting `render_before` preserves the existing per-page contract.
With `strip_ansi=true`, history text preserves inferred pre-redraw frames in
chronological order (blank-line separated), including overwritten and erased
screens. Identical consecutive frames are collapsed; these are inferred terminal
states, not timestamped captures. Rendered frames are bounded to 50 MB; exceeding
that display budget inserts an explicit omission marker before the retained
newest frames. Scrolled-off history lines are retained separately from the 4,096-row screen,
so expanding a cumulative range MUST NOT silently discard previously returned
newer lines. This scrollback uses the same display budget and omission marker.
This does not modify raw `history.data`, cursor semantics,
`terminal_tail`, or live snapshots.

#### `terminal_since`

Args:
```ts
{ group_id: string; actor_id: string; by?: string; after: number; limit_bytes?: number }
```

Result:
```ts
{
  history: {
    data: string
    start_cursor: number
    end_cursor: number
    has_more: boolean
    cursor_expired: boolean
  }
}
```

The cursors count raw PTY bytes. Because `data` is transported as UTF-8 JSON text, an
implementation MUST NOT advance `end_cursor` through an incomplete UTF-8 code point. It MAY return
up to three bytes beyond `limit_bytes` to finish a code point. If the retained stream currently ends
inside a code point, it returns the complete prefix and leaves the incomplete suffix for a later
call.

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
{ group_id: string; actor_id: string; cols: number; rows: number; attachment_id?: number }
```

`cols` MUST be in `10..=65535` and `rows` MUST be in `2..=65535`; invalid or
missing dimensions return `invalid_size` without resizing the PTY.
When `term_attachment_status=true`, WebSocket attachment bridges MUST include the
positive `attachment_id` returned by `term_attach`. The daemon atomically verifies
that the attachment is still the current writer before resizing; a stale or viewer
attachment returns `terminal_not_writer`. Legacy clients and daemons that advertise
the capability as false omit the field.

Result:
```ts
{ group_id: string; actor_id: string; cols: number; rows: number }
```

#### `term_attachment_status`

Optional extension, advertised by `ping.capabilities.term_attachment_status`.
Clients MUST NOT assume this operation is available when that capability is false.

Args:
```ts
{ group_id: string; actor_id: string; attachment_id: number }
```

Result:
```ts
{ terminal_writable: boolean }
```

#### `term_attach` (streaming upgrade)

Args:
```ts
{
  group_id: string
  actor_id: string
  since?: number
  mode?: "control" | "viewer"
  takeover?: boolean
  bootstrap?: "snapshot_v1"
  cols?: number
  rows?: number
}
```

Result (handshake):
```ts
{
  group_id: string
  actor_id: string
  attachment_id?: number
  terminal_mode: "control" | "viewer"
  terminal_writable: boolean
  writer_replaced: boolean
  replay_cursor: number
  replay_end_cursor: number
  initial_output?: {
    kind: "replay" | "snapshot"
    bytes: number
    cursor: number
    cols?: number
    rows?: number
  }
}
```

After a successful handshake, the connection becomes a terminal stream (see §4.4).

Notes:
- `term_resize` MUST be sent over a separate daemon connection (the PTY stream is not NDJSON).
- `term_attach` returns `not_pty_actor` when the actor is not effectively running on the PTY runner.
- `attachment_id` and `initial_output` are optional extensions. Callers MUST
  consult `ping.capabilities.term_attachment_status` and
  `ping.capabilities.term_attach_snapshot_v1` before depending on them. The
  baseline handshake always provides replay cursors and streams retained output
  from `replay_cursor` through `replay_end_cursor` before live PTY bytes.
- A successful `term_attach` owns a dedicated connection and MUST NOT be returned
  to an NDJSON request pool. Rust daemon and Web implementations use this raw
  stream directly; terminal output is not transported through polling RPCs.
- `replay_cursor` and `replay_end_cursor` MUST come from the same backlog snapshot that is queued
  for this attachment; sampling either cursor before the actual attach is not sufficient.
- For `initial_output.kind="replay"`, bytes in `[replay_cursor, replay_end_cursor)` are retained history. Clients MUST NOT send
  terminal-generated query replies while rendering that historical range; only live output after
  `replay_end_cursor` may generate PTY input.
- For `initial_output.kind="snapshot"`, `replay_cursor`, `replay_end_cursor`, and
  `initial_output.cursor` MUST be equal. Exactly `initial_output.bytes` bytes at the start of the
  upgraded daemon stream encode the ANSI snapshot and do not consume raw cursor space. All bytes
  after that payload are raw PTY output beginning at `initial_output.cursor`.
- `cols` and `rows` are optional snapshot-size hints (`10..=4096` and `2..=4096`). The Rust daemon
  applies them only to a control attach with `takeover=true`; viewer and non-takeover attaches do
  not resize the shared PTY. Writer registration, this initial resize, and initial-output capture
  MUST be serialized as one runtime operation so concurrent takeovers cannot return a snapshot at
  another controller's dimensions.
- The WebSocket bridge maps a negotiated snapshot to opcode `7` in one binary frame. The browser
  MUST resize its local xterm parser to the advertised snapshot `cols`/`rows` when present, reset
  xterm, parse that frame, and only then commit/ack `initial_output.cursor`. It MAY refit the local
  viewport after parsing, but a viewer MUST NOT resize the shared PTY. Opcode `1` remains raw
  replay/live output, and its payload length advances the raw cursor.
- A WebSocket bridge MAY negotiate `output_flow=ack_v1`. The attach frame then advertises
  `output_flow_control.protocol="ack_v1"` and a bounded `window_bytes`. After xterm has parsed an
  output frame, the browser sends opcode `5` with `{cursor}`. The bridge MUST bound unacknowledged
  output and MUST continue accepting input while replay is waiting for acknowledgements. Clients
  and bridges that do not negotiate this extension retain the legacy stream behavior.
- The WebSocket bridge sends opcode `6` with `{terminal_writable}` whenever takeover or disconnect
  changes the attachment's writer ownership. Clients MUST update their writable state from this
  frame instead of retaining the handshake value for the lifetime of the connection.

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

For implementations that share a CCCC home, sealed ledger segment bytes remain
part of the append-only source of truth even when a crash occurs before their
manifest entry is published. Before rotating another active ledger, compaction
MUST reconcile every unambiguous canonical segment present on disk and allocate
a sequence greater than every physical or manifested segment sequence. It MUST
NOT reuse an unpublished physical sequence. If distinct canonical segment files
already claim the same sequence, compaction MUST fail before another rotation
rather than silently selecting, overwriting, or hiding either file.
Compaction MUST hold the same canonical ledger writer lock used by appenders
from its source snapshot through active-ledger replacement and manifest
publication. It MUST NOT rotate or report success while an earlier writer still
owns that lock; snapshot and segment metadata MUST include every write committed
before the lock is released to compaction.

### 8.13 Presentation State

The canonical group Presentation snapshot is
`groups/<group_id>/state/presentation.json`. Ports MUST use the daemon
operations below for snapshot mutations; a browser-surface session is
ephemeral and is not part of this durable state.

#### `presentation_get`

Args:
```ts
{ group_id: string }
```

Result:
```ts
{
  group_id: string
  presentation: {
    v: 1
    updated_at: string
    highlight_slot_id: "" | "slot-1" | "slot-2" | "slot-3" | "slot-4"
    slots: Array<{
      slot_id: "slot-1" | "slot-2" | "slot-3" | "slot-4"
      index: 1 | 2 | 3 | 4
      card?: Record<string, unknown>
    }>
  }
}
```

#### `presentation_publish`

Args:
```ts
{
  group_id: string
  by?: string
  slot?: "auto" | "slot-1" | "slot-2" | "slot-3" | "slot-4"
  card_type?: "markdown" | "table" | "image" | "pdf" | "file" | "web_preview"
  title?: string
  summary?: string
  source_label?: string
  source_ref?: string
  content?: string
  table?: Record<string, unknown> | Array<Record<string, unknown>>
  path?: string
  url?: string
  blob_rel_path?: string
}
```

`by` MUST identify `user`, `system`, or an actor in the group. Workspace paths
MUST resolve below the active scope. A stored remote `url` MUST be an absolute
HTTP(S) URL with a host; local or generated content uses `path`, `content`, or
`blob_rel_path` instead. `auto` selects the first empty slot, then the oldest
published slot.

Result:
```ts
{
  group_id: string
  slot_id: string
  card: Record<string, unknown>
  presentation: Record<string, unknown>
  replaced: boolean
  event: CCCSEventV1
  event_id: string // compatibility alias of event.id
}
```

The event kind is `presentation.publish`; its data is
`{slot_id,title,card_type,source_label,source_ref,summary}`.

#### `presentation_clear`

Args:
```ts
{
  group_id: string
  by?: string
  slot?: "slot-1" | "slot-2" | "slot-3" | "slot-4"
  all?: boolean
}
```

`by` has the same validation as `presentation_publish`. `all=true` clears all
slots regardless of `slot`; an omitted/empty `slot` also means all slots.

Result:
```ts
{
  group_id: string
  slot_id: string // populated only when exactly one occupied slot was cleared
  cleared_slots: string[]
  presentation: Record<string, unknown>
  event: CCCSEventV1
  event_id: string // compatibility alias of event.id
}
```

The event kind is `presentation.clear`; its data is
`{slot_id,cleared_all,cleared_slots}`. For both mutation operations, the
snapshot update and ledger event form one acknowledged transition: if event
append fails, the prior snapshot MUST be restored before failure is returned.

### 8.14 Presentation Browser Surface (Optional)

#### `presentation_browser_attach`

Attach to the currently active slot browser-surface session over a dedicated bidirectional NDJSON stream.

Args:
```ts
{
  group_id: string
  slot: "slot-1" | "slot-2" | "slot-3" | "slot-4"
  by?: string
  viewer_mode?: "auto" | "screencast" | "vnc"
}
```

Handshake result:
```ts
{ group_id: string; slot_id: string }
```

Streaming mode:
- After a successful handshake, the connection upgrades into the browser-surface stream described in §4.6.
- The daemon emits `state` items when runtime/session status changes and `frame` items for captured browser frames.
- The client MAY send browser-control commands (`navigate`, `back`, `refresh`, `click`, `scroll`, `key`, `text`, `resize`, `close`, `disconnect`).
- At most one active controller MAY be attached at a time; a second attach attempt SHOULD fail with a busy-style error.
- If no active browser-surface session exists for the slot, attach SHOULD fail with `browser_surface_not_found`.
- If the underlying browser runtime is no longer active, attach SHOULD fail with `browser_surface_not_active`.

#### `presentation_browser_vnc_attach`

Attach to the currently active slot browser-surface session over a raw RFB/VNC stream.

Args:
```ts
{
  group_id: string
  slot: "slot-1" | "slot-2" | "slot-3" | "slot-4"
  by?: string
}
```

Handshake result:
```ts
{ group_id: string; slot_id: string }
```

Streaming mode:
- After a successful handshake, the connection upgrades into a raw VNC/RFB byte stream.
- The operation SHOULD fail with `browser_vnc_unavailable` when the browser surface is not backed by a local VNC projection.

### 8.15 Event Streaming (Optional)

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
- A daemon may initially emit only a subset of event kinds. CCCC streams these kinds:
  - `chat.message`, `mail.read`, `chat.reply_request.cancelled`,
    `runtime.delivery`, `system.notify`
- When `kinds` is provided, only matching event kinds SHOULD be emitted.
- If `by` identifies an `actor_id`, a daemon MAY apply the same recipient-routing visibility rules used by messaging (e.g., only emit `chat.message`/`system.notify` addressed to that actor and exclude the actor’s own `chat.message` events). This stream filter is independent of the Mail-only Inbox projection.
- Resume (`since_event_id` / `since_ts`) is best-effort in v1; clients MUST be able to reconcile using `inbox_peek`.
- The stream ends when the client closes the connection or the daemon exits.
- To protect daemon responsiveness, a daemon MAY drop slow subscribers (clients SHOULD reconnect and reconcile).

### 8.16 IM Authentication

The durable group-local IM authority is `group.yaml:im` for provider
configuration and the sibling state files `im_pending_keys.json`,
`im_authorized_chats.json`, and `im_subscribers.json` for delivery targets.
Implementations MUST serialize reads that can cause a write and every
read-modify-write across those classes with
`groups/<group_id>/state/im_state.lock`; a long-running worker MUST refresh
authorization and subscription truth after acquiring that lock rather than
continue from a process-private startup snapshot. Binding and revocation MUST
update their coupled authorization/subscription records under one such
transaction.

The former Rust `group.yaml:im_bridge` durable fields (`config`, `enabled`,
`authorized`, `pending`, and `subscribers`) are a one-way migration source.
Canonical classes win when present, and imported fields MUST be retired after
canonical commit. An explicit IM unset MUST clear the canonical target files
and consume those legacy durable fields so a later native load cannot restore
configuration or delivery authority. Non-durable runtime diagnostics in
`im_bridge` MAY remain.

Across IM authentication and subscriber state, `thread_id` is a platform-owned
opaque identifier. Implementations MUST preserve it as either a legacy JSON
number or a non-empty JSON string and MUST NOT coerce string identifiers (for
example, Slack timestamps such as `1710000000.100`) through an integer or
floating-point representation. Omitted, null, empty, integer `0`, and string
`"0"` mean no thread; unsupported JSON value types MAY normalize to numeric `0`.

#### `im_bind_chat`

Bind a pending one-time key to authorize an IM chat. On success the chat is also auto-subscribed for outbound message delivery.

Args:
```ts
{ group_id: string; key: string }
```

Result:
```ts
{ chat_id: string; thread_id: number | string; platform: string }
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
    thread_id: number | string
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
{ group_id: string; chat_id: string; thread_id?: number | string }
```

Result:
```ts
{ revoked: boolean; unsubscribed?: boolean }
```

Notes:
- `thread_id` defaults to `0` when omitted or represented by an unsupported JSON value type.

Errors:
- `missing_chat_id` – `chat_id` is empty.
- `missing_group_id` – `group_id` is empty.
- `group_not_found` – group does not exist.

### 8.17 Remote Access (Contract-Gated)

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
    provider: "off" | "manual" | "tailscale" | "reach"
    mode: string
    require_access_token: boolean
    enabled: boolean
    status: "stopped" | "running" | "not_installed" | "not_authenticated" | "misconfigured" | "error"
    endpoint?: string | null
    updated_at?: string | null
    diagnostics?: {
      access_token_present?: boolean
      access_token_source?: "store" | "none" | string
      access_token_count?: number
      admin_access_token_present?: boolean
      admin_access_token_count?: number
      remote_listener_auth_required?: boolean
      remote_listener_auth_requirement_satisfied?: boolean
      allow_unauthenticated_listener_override?: boolean
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
      access_token_configured?: boolean
      access_token_count?: number
      admin_access_token_configured?: boolean
      admin_access_token_count?: number
      access_token_source?: "store" | "none" | string
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
  require_access_token?: boolean
  web_host?: string
  web_port?: number
  web_public_url?: string
}
```

Result:
```ts
{ remote_access: Record<string, unknown> }
```

A non-local Web binding or a configured public URL requires at least one administrator Access Token before it can be started or applied. Group-scoped tokens do not satisfy this recovery/control-plane requirement. Localhost-only configuration remains available without a token. Implementations MAY expose `CCCC_WEB_ALLOW_UNAUTHENTICATED=1` as an explicit unsafe listener override for deployments that already enforce a trusted network boundary.

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

Errors:
- `remote_access_admin_token_required` – remote exposure has no administrator Access Token and the explicit unsafe listener override is not enabled.

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

`provider=reach` is not set through `remote_access_configure`. It is owned by the membership reach verbs. Settings may persist `reach` after a successful `membership_reach_on`. While Reach is enabled or its tracked helper is still running, `remote_access_configure` MUST reject every configuration mutation; callers must complete `membership_reach_off` before changing provider, binding, or public URL.

### 8.17.1 Membership reach

Optional extension for third-party deployments. The bundled native
implementation implements the complete operation set below. Deployments without
membership MAY return `unknown_op`.

Stable error classes:

- `membership_not_logged_in` – no local device binding exists for an operation that requires one
- `membership_gate` – missing Admin Token, unauthenticated-listener override, or another remote provider is already on
- `membership_disabled`
- `membership_network`
- `membership_subprocess`
- `membership_unsupported_version`
- `membership_unavailable` – account plane origin is not configured
- `membership_not_in_reach`

#### `membership_status`

```ts
{ by?: string }
```

```ts
{
  membership: {
    logged_in: boolean
    device_id?: string | null
    hostname?: string | null
    web_url?: string | null
    online: boolean
    cut: boolean
    disabled: boolean
    in_reach: boolean
    reach_supported: boolean
    account_reachable?: boolean | null
    account_origin?: string | null
    last_error?: string | null
    warning?: string
    pending?: {
      user_code: string
      verification_uri: string
      verification_uri_complete?: string | null
      interval: number
      expires_at: string
    } | null
  }
}
```

`membership_status` is user-only because `web_url` contains a local bearer credential. Implementations MUST reject non-user callers before assembling it. The URL is assembled locally and MUST NOT be stored on the account plane. It is null while logged out. Actor-bound Web Model connector URLs remain part of the actor connector API and MUST NOT be selected or exposed through global membership status.

`account_reachable` is ephemeral evidence from the current status refresh; it is
omitted when no linked-device probe applies, `true` after a valid account-plane
response, and `false` after a transient account-plane failure. It MUST NOT be
persisted as a second freshness state machine.

`reach_supported` reports whether this CCCC build provides a pinned managed
Reach helper for the current platform. It is independent of account linkage and
helper installation: an unsupported platform can still link and manage its CCCC
Account, but MUST NOT present Reach as startable.

Status refresh may observe an account-side Cut or learn that the bearer for an
already-linked device is definitively absent. Both are terminal revocations: the
daemon MUST stop the tracked helper, persist the disabled state, and clear
Reach-owned `enabled` / `web_public_url` state before returning. A timeout, DNS
failure, 5xx response, or malformed response is transient and MUST preserve the
binding and helper state. Daemons therefore MUST serialize `membership_status`
with membership mutations rather than treating it as a side-effect-free read.

#### `membership_login` / `membership_login_poll` / `membership_logout`

```ts
{ by?: string }
```

`membership_login` starts RFC 8628 device-code login against `CCCC_ACCOUNT_ORIGIN` and returns `membership.pending` (`verification_uri`, optional `verification_uri_complete`, `user_code`, `interval`). While an unexpired, still-pollable grant exists, another `membership_login` MUST replay it rather than issue a second device code or retarget it to a different origin. Both verification URLs MUST be absolute HTTP(S) URLs on the configured account origin; clients reject an off-origin authorization URL before storing or opening it. The selected account origin is persisted with the pending login and resulting device grant. Polling and every later authenticated device or Reach request MUST use that issuer-bound origin; a changed daemon environment or per-request override MUST NOT retarget an existing bearer token. The CLI or Web client opens `verification_uri_complete` when present and otherwise presents `verification_uri` plus `user_code`. The advertised interval is a minimum and MUST NOT be capped downward. On `slow_down`, subsequent polling waits MUST increase by at least five seconds. `authorization_pending`, `slow_down`, transport failures, and 5xx responses preserve the pending grant. `access_denied` and `expired_token` are terminal for that grant: the daemon MUST clear the matching pending state before returning the error so the next `membership_login` can issue a fresh code. Once a grant has been committed, an exact or late `membership_login_poll` MUST replay the logged-in status instead of failing because the pending code was consumed.

`membership_logout` first stops any tracked Reach helper, then retires the issuer-bound account device through the account plane, and only then clears local membership secrets and retired Reach URLs. Network or account errors preserve the local credential so the user can retry; an already absent or disabled remote device is treated as retired. The result includes a warning that the next login is a new device and hostname.

Requests send `CCCC-Membership-Version: 1`. An account plane that no longer supports the client returns `membership_unsupported_version`. `CCCC_ACCOUNT_ORIGIN` MUST be an HTTP(S) origin without user information, a non-root path, query, or fragment. It MUST use HTTPS, except that loopback HTTP is allowed for local development. Clients MUST NOT follow account-plane redirects because authenticated requests carry a device bearer token.

For authenticated device endpoints, absence of an Authorization bearer remains
`401 unauthorized`. When a bearer is present but no active device exists for it
(including after device or account deletion), the account plane MUST return
`403 device_disabled`. Clients talking to an older or third-party issuer MAY
also receive `401` or `404`; if the request used a locally stored device bearer,
those responses are the same terminal revocation, not evidence of a transient
network failure. Relinking first retires the invalid local binding and then
starts a fresh device-code authorization.

#### `membership_reach_install`

```ts
{ upgrade?: boolean, by?: string }
```

Installs the pinned `cloudflared` binary under `CCCC_HOME` after verifying its platform, version, and SHA-256 digest. With `upgrade=true`, an existing unpinned or mismatched managed binary is replaced. Installation does not enable remote access or start a tunnel.

#### `membership_reach_on` / `membership_reach_off`

```ts
{ by?: string }
```

`reach on` requires an administrator Access Token, a logged-in device that is not disabled, and an account origin. It MUST refuse if `CCCC_WEB_ALLOW_UNAUTHENTICATED` is set or if `tailscale` is already enabled. An enabled `manual` public URL remains active while Reach is prepared and is replaced only after the Reach helper starts successfully; any pre-commit failure MUST preserve the manual provider and URL. It installs the pinned `cloudflared` if missing, and refuses a version/hash mismatch unless `membership_reach_install` (`cccc reach install`) was used. The account-plane request includes the port of the currently live, identity-verified Web listener as `origin_port` (1–65535), not merely the desired setting or environment default. The runtime descriptor MUST contain an unguessable Web-instance identifier and an owner-only proof key. Reach MUST send a fresh random challenge, and the loopback `/api/v1/ready` response MUST return the recorded identifier plus an HMAC-SHA256 proof bound to that challenge before Reach may start. The verifier MUST NOT send the expected identifier or proof key to the listener; a live PID, accepting TCP port, or reflected request value alone is not proof that the listener belongs to CCCC. The account plane MUST route the named tunnel to `127.0.0.1:<origin_port>` and MUST NOT accept an arbitrary origin host. A returned Reach hostname MUST normalize to one HTTPS origin without user information, a non-root path, query, or fragment before it can be stored or used to assemble local token-bearing URLs. On success it sets `remote_access.provider=reach` and writes `web_public_url`.

The tunnel token MUST NOT appear in process arguments; supported helpers use a permission-restricted token file. Before signaling a persisted helper PID, an implementation MUST verify the live executable against the exact managed executable recorded when the helper started (or use an in-process child handle it still owns); process names and argument substrings are insufficient. A mismatch preserves tracking and returns an error instead of killing an unrelated process. `reach off` keeps `provider=reach`, but reports success only after the tracked helper has exited and its tracking files are retired. A persisted `enabled` flag alone is not proof that Reach is online: status requires a live tracked helper and, when the account service supplies connection status, a connected named tunnel at the account plane. If any authenticated device-status or Reach-issuance response reports the device disabled or definitively missing, the helper is stopped, Reach-owned public state is cleared, and status is `cut` before the operation returns.

Membership state lives in `CCCC_HOME/secrets/membership.json`. Every
read-modify-write mutation MUST hold
`CCCC_HOME/secrets/membership.json.lock` and preserve the full v1 shape,
including issuer-bound `account_origin`, `device_token`, `tunnel_token`, and
`pending_login`.

### 8.17.2 Group Bridge delivery compatibility

The daemon accepts these Group Bridge operations:

- `remote_send`: send a payload through an active registration or trust. It
  requires `group_id`, `registration_id`, `idempotency_key`, and an explicit
  `payload.to` recipient list.
- `remote_delivery_status`: return the stored receipt identified by
  `registration_id` and `idempotency_key`.
- `group_bridge_receive_remote_send`: authenticate an already-resolved inbound
  session using `target_group_id`, `src_group_id`, `remote_peer_id`, and append
  its payload idempotently to the target group.

Implementations MUST persist delivery receipts and MUST NOT create duplicate
events when the same registration and idempotency key are retried.
The canonical receipt lifecycle is `queued`, `sending`, `retrying`, `sent`, or
`failed`; `sent` and `failed` are terminal.
The daemon owns bounded recovery of non-terminal receipts. It periodically
retries due `queued`/`retrying` receipts, recovers stale `sending` receipts, and
honors a persisted `next_attempt_at` when present. A live reverse-session
reconnect MAY accelerate recovery. Web process lifetime and caller activity
MUST NOT be required for an accepted outbox item to make progress.

Pairing and session authentication fail closed. A pairing invite
MUST carry a parseable future `expires_at`; missing, malformed, or expired
values are invalid. Approval MAY open a bounded credential-claim window. A POST
with the same request id, invite id, and pairing code MAY return the same raw
credential again within that window so transport failures are recoverable; the
credential MUST NOT appear in status GET responses or after the window expires.
An approved, unclaimed compatibility record that predates `claim_expires_at`
MAY receive one persisted bounded migration window. Implementations MUST NOT
start that window when a different record is accessed, MUST NOT extend the
migrated deadline on later status or claim retries, and malformed or explicitly
expired deadlines MUST remain invalid.

The Rust v2 WebSocket endpoint MUST send a fresh signed per-connection challenge
before the client hello. The challenge signature covers the v2 protocol id,
message contract version, nonce, issue/expiry times, and server peer id; the
client MUST verify that peer id against its persisted trust. The hello signature
covers the v2 protocol id, route identity, message contract version, challenge
nonce and issue time, plus a fresh client nonce. After accepting the hello, the
server MUST sign a `ready` confirmation over both signed messages, the route,
and that client nonce; the client MUST verify this confirmation before routing
messages. This transcript proof makes a captured challenge or ready unusable on
another connection. A challenge expires before session readiness.
Rust clients try v2 first and fall back to v1 only when the endpoint is absent or
requires legacy authentication. New servers MAY accept a legacy v1 hello for an
active trust that has never completed v2; a successful v2 handshake MUST persist
`min_session_protocol=2` on both peers, and every later v1 downgrade for that
trust MUST fail before opening a v1 WebSocket. The server MUST durably persist
its pin before sending the successful `ready`, and the client MUST durably
persist its pin before publishing the session as connected. An upgraded socket
that does not complete the signed hello within a bounded handshake timeout MUST
be closed without opening a daemon session.
WebSocket bearer credentials MUST use the Authorization header, never a query
parameter.

For a new outbound message, the source-group `chat.message` MUST be appended
idempotently before any remote transport side effect. Its event ID MUST be sent
as `src_event_id` alongside `src_group_id`, and every subsequent retry for the
same registration and idempotency key MUST reuse that source event. A successful
remote receipt MUST be projected into the source ledger as one idempotent
`chat.cross_group_receipt`; transport state belongs in that receipt, not in the
immutable source message. A remote reply may reuse the local reply event that was
already appended instead of creating a second source message.
The source-group record is a human-visible audit record, not a local copy of the
remote delivery contract: it MUST use local `to=["user"]` and
`message_mode="send"`, while `dst_to` and `dst_message_mode` preserve the remote
audience and mode. The remote payload and destination event independently apply
the one-audience-domain and agent-only Mail rules.
The receipt field `projected` is local bookkeeping only. Implementations MUST
ignore a peer-supplied `projected` value and establish projection from trusted
local receipt state or the source ledger.

The persistence authority is this set of purpose-specific files in `CCCC_HOME`:

- `group_bridge_identity.yaml`
- `group_bridge_pairing.yaml` for invites, requests, trusts, and outbounds
- `group_bridge_registrations.yaml`
- `group_bridge_credentials.yaml` for raw bearer and remote-send secrets
- `group_bridge_receipts.yaml`, keyed by
  `registration_id::idempotency_key`

Registrations MUST contain only an opaque `credential_ref`; raw credentials
MUST NOT be written into pairing or registration records. The former Rust
`settings.yaml:group_bridge` section is a migration source only. Canonical
records win conflicts, including terminal trust states (`revoked`, `rejected`,
`expired`, or `disabled`) matched by registration or route identity. An
implementation MUST commit the canonical files before clearing that legacy
section, and MUST NOT recreate an active registration, credential, or trust from
legacy state after a canonical terminal decision.

The Rust WebSocket owner and MCP bridge share live reverse-session state through
these daemon-internal operations:

- `group_bridge_session_open`: register a live route identified by `group_id`,
  `remote_group_id`, and `remote_peer_id`; returns a new opaque `generation`.
- `group_bridge_session_close`: remove the route only when its `generation`
  still matches. A stale socket MUST NOT close a replacement session.
- `group_bridge_session_ready`: report whether that exact route currently has a
  live session lease.
- `group_bridge_session_poll`: let the owning WebSocket take the next queued
  server-to-peer request for its generation.
- `group_bridge_session_complete`: resolve a request using `response_to` and a
  peer-provided `result`.
- `group_bridge_session_deliver`: enqueue a `remote_send` request and await its
  response for at most `timeout_ms`.

These operations are runtime-only and MUST NOT treat persisted trust status as
proof of reachability. Opening a replacement generation, closing the active
generation, and completing a response MUST wake pending callers immediately.
Delivery failures use `peer_session_unavailable` when no live lease exists or
disconnects, `peer_session_timeout` when the peer does not answer in time, and
`peer_session_failed` when a session is replaced or returns an invalid result.

### 8.18 Group Space (Provider-Backed Shared Memory, dual-lane NotebookLM)

These operations provide a thin control-plane for optional external memory providers.
Provider failures MUST NOT block core collaboration flows (chat/context/actors).

NotebookLM is modeled as two fixed daemon-owned lanes:
- `lane="work"`: project/shared external knowledge, repo `space/` sync, artifacts, general ingest/query.
- `lane="memory"`: finalized daily memory recall only; daemon syncs `state/memory/daily/*.md` asynchronously.

Normative lane rules:
- Agent-facing surfaces SHOULD pass `lane` explicitly for mutating or lane-targeted actions.
- `group_space_status` MAY omit `lane`; it returns both lanes.
- `group_space_bind|query|sources|jobs|sync` are lane-targeted.
- `group_space_ingest|artifact` are supported only on `lane="work"`.
- `MEMORY.md` MUST remain local-only and MUST NOT be uploaded to NotebookLM.

#### `group_space_status`

Read provider mode, both lane bindings, queue summaries, work-lane repo `space/` sync state,
and memory-lane daily sync summary.

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
  bindings: {
    work: {
      group_id: string
      provider: "notebooklm"
      lane: "work"
      remote_space_id: string
      bound_by: string
      bound_at: string
      status: "bound" | "unbound" | "error"
    }
    memory: {
      group_id: string
      provider: "notebooklm"
      lane: "memory"
      remote_space_id: string
      bound_by: string
      bound_at: string
      status: "bound" | "unbound" | "error"
    }
  }
  queue_summary: {
    work: { pending: number; running: number; failed: number }
    memory: { pending: number; running: number; failed: number }
  }
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
  memory_sync?: {
    lane: "memory"
    manifest_path: string
    last_scan_at?: string | null
    last_success_at?: string | null
    pending_files: number
    running_files: number
    failed_files: number
    blocked_files: number
    eligible_daily_files: number
    synced_daily_files: number
    empty_daily_skipped: number
    last_eligible_daily_date?: string | null
    last_synced_daily_date?: string | null
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
  bindings: Record<"work" | "memory", Record<string, unknown>>
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
    kinds: Array<"context_sync" | "resource_ingest" | "memory_daily_sync">
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
  capabilities: string[]
  unavailable_capabilities: string[]
}
```

Capability matrices are implementation-specific runtime truth. Callers MUST NOT
assume an ingest source type or asynchronous behavior that is absent from the
returned matrix. An implementation MUST fail an unavailable operation with
`capability_unavailable`; it MUST NOT silently coerce a file, URL, YouTube, or
Drive source into pasted text.

#### `group_space_bind`

Bind/unbind a group lane to a provider remote notebook.
When `action=bind` and `remote_space_id` is empty, daemon may auto-create
an appropriate notebook and bind it.

Args:
```ts
{
  group_id: string
  provider?: "notebooklm"
  lane: "work" | "memory"
  action?: "bind" | "unbind"
  remote_space_id?: string
  by?: string
}
```

Result:
```ts
{
  group_id: string
  lane: "work" | "memory"
  provider: Record<string, unknown>
  bindings: Record<"work" | "memory", Record<string, unknown>>
  queue_summary: {
    work: { pending: number; running: number; failed: number }
    memory: { pending: number; running: number; failed: number }
  }
  sync?: Record<string, unknown>         // work-lane repo sync view
  memory_sync?: Record<string, unknown>  // memory-lane manifest summary
  sync_result?: Record<string, unknown>
}
```

#### `group_space_ingest`

Create (or dedupe) a durable work-lane ingest job and execute one provider
attempt. `lane="memory"` MUST be rejected. The job MUST be persisted before the
provider mutation, then settled to `succeeded` or `failed`. A process exit or a
provider response whose remote commit cannot be determined MUST leave the job
`running` to represent an uncertain outcome. Reissuing the same idempotency key
MUST return that job instead of creating another source. 0.4.36 has no
background ingest retry worker: retry after a terminal failure is an explicit
`group_space_jobs action=retry` operation.

Args:
```ts
{
  group_id: string
  provider?: "notebooklm"
  lane: "work" | "memory"
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
  lane: "work"
  job_id: string
  accepted: boolean
  completed: boolean
  deduped: boolean
  job: Record<string, unknown>
  ingest_result?: Record<string, unknown>
  source_id?: string
  source_ids?: string[]
  queue_summary: { pending: number; running: number; failed: number }
  provider_mode: "disabled" | "active" | "degraded"
}
```

`accepted=true, completed=false` means durable work remains in progress.
Terminal `succeeded`, `failed`, or `canceled` jobs report
`accepted=false, completed=true`.

#### `group_space_query`

Query provider-backed knowledge for one lane. If provider is degraded, result MAY return
`ok=true` with `degraded=true` and an empty answer.

Args:
```ts
{
  group_id: string
  provider?: "notebooklm"
  lane: "work" | "memory"
  query: string
  options?: {
    source_ids?: string[] // optional remote source_id filter
  }
}
```

Validation notes:
- `options` only supports `source_ids`.
- `options.language` / `options.lang` are invalid for `group_space_query` because NotebookLM query API does not provide a language parameter.
- Recommended recall order is local memory first, then `lane="memory"` for deep recall.

Result:
```ts
{
  group_id: string
  provider: "notebooklm"
  lane: "work" | "memory"
  provider_mode: "disabled" | "active" | "degraded"
  degraded: boolean
  answer: string
  references: unknown[]
  reference_count: number
  binding_status: "bound" | "unbound" | "error"
  source_basis_hint: "requested_sources_hit" | "requested_sources_mixed" | "requested_sources_only" | "referenced_sources_present" | "context_sync_only" | "materialized_sources_present" | "mixed" | "memory_manifest_only" | "unknown"
  requested_source_ids?: string[]
  referenced_source_ids?: string[]
  references_match_requested?: boolean
  latest_context_sync_at?: string
  remote_sources?: number
  materialized_sources?: number
  memory_last_success_at?: string
  memory_pending_files?: number
  memory_failed_files?: number
  error?: { code: string; message: string } | null
}
```

Notes:
- The extra query fields above are lightweight diagnostics/provenance hints, not retrieval guarantees.
- When `options.source_ids` is provided, `source_basis_hint` SHOULD prefer explicit source scope / actual cited sources over inferred local sync state.
- Work-lane answers may come from synced coordination/context even when repo materialized sources are sparse.
- Memory-lane diagnostics describe sync-manifest health only; they do not promise a semantic hit for every query.

#### `group_space_sources`

List/refresh/rename/delete provider sources in the currently bound lane notebook.

Args:
```ts
{
  group_id: string
  provider?: "notebooklm"
  lane: "work" | "memory"
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
  lane: "work" | "memory"
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
  lane: "work" | "memory"
  provider_mode: "disabled" | "active" | "degraded"
  binding: Record<string, unknown>
  action: "refresh" | "rename" | "delete"
  source_id: string
  refresh_result?: Record<string, unknown>
  rename_result?: Record<string, unknown>
  delete_result?: Record<string, unknown>
}
```

`action=refresh` MUST invoke the provider refresh mutation. Re-listing the
source without refreshing it is not a successful refresh result. A successful
NotebookLM refresh reports `refresh_result.refreshed=true`.

#### `group_space_artifact`

List/generate/download provider artifacts (NotebookLM studio outputs) on `lane="work"`.
`lane="memory"` MUST be rejected.
For `action=generate`, daemon can optionally wait for completion and auto-save
the artifact into local `repo/space/artifacts/...`.

Args:
```ts
{
  group_id: string
  provider?: "notebooklm"
  lane: "work" | "memory"
  action?: "list" | "generate" | "download"
  kind?: "audio" | "video" | "report" | "study_guide" | "quiz" | "flashcards" | "infographic" | "slide_deck" | "data_table" | "mind_map"
  options?: Record<string, unknown> // for action=generate
  wait?: boolean // action=generate only; default false
  save_to_space?: boolean // generate/download local-save behavior; default false
  output_path?: string // optional local path override
  output_format?: "json" | "markdown" | "html" | "pdf" | "pptx" | "csv"
  artifact_id?: string // optional explicit download target
  timeout_seconds?: number // generate+wait only
  initial_interval?: number // generate+wait only
  max_interval?: number // generate+wait only
  by?: string
}
```

Result (`action=list|generate|download`) mirrors the lane-targeted binding and includes `lane: "work"`.

Generation defaults to `wait=false` and `save_to_space=false`, so a normal
request does not hold a group mutation lane while polling a remote provider and
does not perform an implicit local write. When `wait=false`, an implementation
without a background artifact worker MAY
return after remote generation starts with `saved_to_space=false`; the caller
can later list or download the artifact. It MUST NOT claim that a local file was
saved. `wait=true` plus `save_to_space=true` performs the wait and local save
before returning or reports a provider timeout/failure.

When `save_to_space=true`, the implementation MUST validate the local
destination and kind/format download capability before creating the remote
artifact. Unsupported kind/format combinations fail with
`capability_unavailable` without provider-side generation. Authenticated media
downloads MUST require HTTPS and validate an explicit provider host allowlist
for both the initial URL and every redirect hop.

Common provider error semantics:
- `space_provider_not_configured`: required provider credentials or binding configuration is absent; non-transient.
- `space_provider_auth_invalid`: credentials are malformed, expired, or rejected; non-transient until re-authentication.
- `space_provider_not_found`: requested remote resource is absent; non-transient and does not degrade the whole provider.
- `space_provider_compat_mismatch`: provider response schema cannot be decoded; non-transient and degrades the provider until compatibility is restored.
- `space_provider_timeout`: request or provider-side wait timed out; transient and does not by itself degrade the provider.
- `space_provider_rate_limited`: provider refused work because of quota/rate limits; transient and does not by itself degrade the provider.
- `space_provider_outcome_unresolved`: the provider may have committed the mutation, but CCCC cannot prove its result; the durable job remains `running` and MUST NOT be retried until a user inspects/reconciles or cancels it.
- `space_provider_upstream_error`: another provider transport/RPC failure occurred; retryability depends on the operation and provider response.

#### `group_space_jobs`

List/retry/cancel Group Space jobs for one lane.

`retry` accepts only terminal `failed`/`canceled` jobs (and legacy `pending`
jobs). It MUST reject an uncertain `running` job. Before a retry performs any
provider mutation, the job's saved `remote_space_id` MUST equal the lane's
current binding; otherwise it fails with `binding_changed` and leaves the job
unchanged. This prevents an old job from writing into a notebook the Group has
since left.

Args:
```ts
{
  group_id: string
  provider?: "notebooklm"
  lane: "work" | "memory"
  action?: "list" | "retry" | "cancel"
  job_id?: string
  state?: "pending" | "running" | "succeeded" | "failed" | "canceled"
  limit?: number
  by?: string
}
```

#### `group_space_sync`

Read legacy Python 0.4.35 synchronization state for one lane during a native
upgrade. This compatibility operation is not advertised by the 0.4.36 CLI,
Web API, or MCP surface. Automatic repo/memory mirroring is retired; callers
use explicit `group_space_ingest` and source operations instead.

Args:
```ts
{
  group_id: string
  provider?: "notebooklm"
  lane: "work" | "memory"
  action?: "status"
  by?: string
}
```

Result returns the targeted lane state in `sync`. Implementations that list
`sync.work` or `sync.memory` in `unavailable_capabilities` MUST still expose
canonical read-only status after the upgrade. A legacy client that sends
`action=run` MUST receive `capability_unavailable` before any provider-side
mutation.

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
- Updating or clearing the effective stored credential invalidates its prior verified-ready state;
  callers must run a successful health check before the provider is reported `write_ready=true`.

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
  auth_json?: string // optional candidate storage-state JSON
}
```

When `auth_json` is present it is treated as write-only credential material: it is never
returned, persisted, or used to update the current provider state. This candidate-validation
form lets browser-auth controllers verify a captured session before committing it.

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

Control provider auth flow (`status`/`start`/`cancel`/`disconnect`) for backend-managed
NotebookLM sign-in.

Args:
```ts
{
  provider?: "notebooklm"
  action?: "status" | "start" | "cancel" | "disconnect"
  timeout_seconds?: number
  projected?: boolean // when true, expose sign-in through the projected browser surface instead of a daemon-host browser window
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
    delivery?: "local_browser" | "projected_browser" | ""
    session_id?: string
    started_at?: string
    updated_at?: string
    finished_at?: string
    message?: string
    error?: { code: string; message: string } | Record<string, unknown>
    projected_browser?: {
      active: boolean
      state: string
      message?: string
      error?: { code?: string; message?: string } | Record<string, unknown>
      strategy?: string
      url?: string
      width?: number
      height?: number
      started_at?: string
      updated_at?: string
      last_frame_seq?: number
      last_frame_at?: string
      controller_attached?: boolean
    }
  }
}
```

Notes:
- `start` may open a browser on the daemon host for Google sign-in when `projected` is false.
- `start` SHOULD expose the sign-in flow through a projected browser surface when `projected=true`.
- When the daemon advertises both provider-auth browser attach capabilities as
  false and a product Web process owns the browser lifecycle, daemon-level
  `start`/`cancel`/`disconnect` MUST return `capability_unavailable`; `status`
  remains a valid durable credential/provider-state projection.
- Provider write readiness remains gated by `auth_configured` and runtime mode.

#### `space_provider_auth_browser_attach`

Attach to the currently active projected provider-auth browser surface over a dedicated bidirectional NDJSON stream.

Args:
```ts
{
  provider: "notebooklm"
  by?: string // user-only
  viewer_mode?: "auto" | "screencast" | "vnc"
}
```

Handshake result:
```ts
{ provider: "notebooklm" }
```

Streaming mode:
- After a successful handshake, the connection upgrades into the browser-surface stream described in §4.6.
- The daemon emits `state` items when runtime/session status changes and `frame` items for captured browser frames.
- The client MAY send browser-control commands (`navigate`, `back`, `refresh`, `click`, `scroll`, `key`, `text`, `resize`, `close`, `disconnect`).
- At most one active controller MAY be attached at a time; a second attach attempt SHOULD fail with a busy-style error.
- If no active projected auth browser exists, attach SHOULD fail with `browser_surface_not_found`.
- If the underlying browser runtime is no longer active, attach SHOULD fail with `browser_surface_not_active`.

#### `space_provider_auth_browser_vnc_attach`

Attach to the currently active projected provider-auth browser surface over a raw RFB/VNC stream.

Args:
```ts
{
  provider: "notebooklm"
  by?: string // user-only
}
```

Handshake result:
```ts
{ provider: "notebooklm" }
```

Streaming mode:
- After a successful handshake, the connection upgrades into a raw VNC/RFB byte stream.
- The operation SHOULD fail with `browser_vnc_unavailable` when the browser surface is not backed by a local VNC projection.

### 8.19 ChatGPT Web Model Browser Surface (Optional)

#### `web_model_delivery_preferences_get`

Read the durable browser-delivery preference for one Web Model actor.

Args:
```ts
{ group_id: string; actor_id: string }
```

Result:
```ts
{
  group_id: string
  actor_id: string
  preference: {
    mode: "standard" | "image_compat"
    updated_at: string
    updated_by: string
  }
}
```

Missing or invalid stored state MUST resolve to `standard` without mutating the
group. The preference is scoped to `(group_id, actor_id)` and MUST survive
browser-target changes and daemon restarts.

#### `web_model_delivery_preferences_update`

Update the durable browser-delivery preference for one Web Model actor.

Args:
```ts
{
  group_id: string
  actor_id: string
  mode: "standard" | "image_compat"
  by: "user"
}
```

Result has the same shape as `web_model_delivery_preferences_get`. The operation is user-only and MUST reject other modes. A runtime turn snapshots the effective mode in `turn.delivery.web_model_mode`; a preference change therefore applies to the next accepted delivery, not a delivery already in flight.

`image_compat` is an experimental ChatGPT transport workaround. The browser adapter MUST attach exactly one CCCC-owned blank PNG before invoking Send and MUST NOT use the OS clipboard. An attachment failure before a submit action MUST settle the delivery attempt as `failed`; it MUST NOT affect the Mail cursor. The mode does not select or change the ChatGPT model.

#### `runtime_wait_next_turn`

Accept one pending structured-runtime delivery turn. This operation is mutating:
it claims work, records pull delivery as accepted, and sets the actor's active
turn. It MUST NOT advance the Mail cursor.

Args:
```ts
{
  group_id: string
  actor_id: string
  by: string                 // MUST equal actor_id
  limit?: number             // 1..20, default 20
  transport?: "web_model_pull" | "web_model_browser" // internal browser owner only
}
```

`kind_filter` is not supported. Runtime delivery selects only pending direct
delivery work; ordinary `message_mode="mail"` messages remain in Inbox until an
explicit promotion or a mailbox notice creates direct work.

Result is one of:
```ts
{ status: "stopped"; turn: null }
{ status: "turn_in_progress"; turn: null; active_turn_id: string; event_ids: string[] }
{ status: "idle"; turn: null; suggested_retry_after_ms: number }
{
  status: "work_available"
  turn: {
    turn_id: string
    group_id: string
    actor_id: string
    event_ids: string[]      // exact canonical delivery batch
    latest_event_id: string
    latest_ts: string
    messages: Event[]
    coalesced_text: string
    system_prompt: string
    delivery: {
      mode: "runtime_delivery"
      transport: "web_model_pull" | "web_model_browser"
      max_events: number
      web_model_mode: "standard" | "image_compat"
    }
  }
}
```

For `web_model_pull`, every returned source event MUST already have a durable
`runtime.delivery` state of `accepted`. For `web_model_browser`, this operation
only establishes the browser claim; `web_model_browser_delivery_record` settles
the claim after the submit boundary. A second wait while the actor owns an active
turn MUST return `turn_in_progress` and MUST NOT replace that turn.
`coalesced_text` MUST preserve the complete daemon-rendered batch without a
secondary character-count truncation. Browser-originated oversized text is
converted to a `.txt` attachment before daemon delivery; the canonical
`messages` array remains the source of truth for every structured turn.

#### `runtime_complete_turn`

Close the actor's exact active structured-runtime turn after processing.

Args:
```ts
{
  group_id: string
  actor_id: string
  by: string                 // MUST equal actor_id
  turn_id: string
  event_ids: string[]        // MUST exactly equal the active turn event_ids
  delivery_id?: string       // default `runtime:<turn_id>`; part of the replay fingerprint
  status?: "done" | "partial" | "failed" | "cancelled" // default done
  summary?: string
}
```

`latest_event_id` is not supported. Every supplied event MUST already have a
terminal handoff fact (`runtime.delivery=accepted|ambiguous`) for this actor.
Completion records runtime progress and releases the active turn; every status,
including `done`, MUST leave the Mail cursor unchanged. An actor consumes
Inbox contents only through `inbox_read` / `cccc_inbox_read`.
The daemon MUST persist a deterministic `runtime.turn.completed` receipt before
acknowledging completion. An exact retry with the same actor, turn, event IDs,
status, and delivery ID MUST replay that receipt even after active-turn state was
cleared. Reusing the turn identity with a different fingerprint MUST fail with
`completion_conflict`.

Common result fields:
```ts
{
  status: "done" | "partial" | "failed" | "cancelled"
  turn_id: string
  delivery_id: string
  completion_event: CCCSEventV1 // kind="runtime.turn.completed"
  processed_event_ids: string[]
  followup_delivery_scheduled: boolean
  summary: string
}
```

#### `web_model_runtime_recover_turn`

Rebuild a previously handed-off Web Model turn without changing runtime state,
delivery state, or the actor Mail cursor. This is used only to reconcile a
persisted browser-delivery attempt after process interruption.

Args:
```ts
{ group_id: string; actor_id: string; event_ids: string[] }
```

Result:
```ts
{
  status: "recovered"
  turn: {
    turn_id: string
    group_id: string
    actor_id: string
    event_ids: string[] // canonical ledger order
    latest_event_id: string
    latest_ts: string
    messages: Event[]
    coalesced_text: string
    system_prompt: string
    delivery: {
      mode: "recovery_no_delivery_mutation"
      web_model_mode: "standard" | "image_compat"
    }
  }
}
```

Every event MUST exist, be addressed to the actor, have a supported turn kind,
and already have a terminal handoff fact (`runtime.delivery=accepted|ambiguous`).
The operation MUST NOT change delivery state, read state, active runtime state,
or completion state.

#### `web_model_browser_delivery_record` (internal)

Append a browser-delivery observation for a claimed Web Model turn. The browser
owner uses this operation to expose the canonical message status in the Web
surface and to settle the runtime handoff; it does not complete the turn or
advance the actor cursor.

Args:
```ts
{
  group_id: string
  actor_id: string
  by: string                 // MUST equal actor_id
  turn_id: string
  event_ids: string[]        // 1..20 addressed chat.message/system.notify IDs
  delivery_id: string
  browser_delivery: {
    state: "submitting" | "submitted" | "bound" | "pending" | "ambiguous" | "failed"
    detail?: string
    provider?: string
    target_url?: string
    bound_conversation_url?: string
    pending_conversation_url?: string
    auto_bind_new_chat?: boolean
    resolved_pending_new_chat?: boolean
  }
}
```

Result:
```ts
{ event: CCCSEventV1 }
```

Each call appends an ordinary `web_model.browser_delivery.<state>` event.
`submitted` and `bound` MUST settle every referenced runtime claim as
`runtime.delivery=accepted`; `ambiguous` and `failed` settle it with the matching
terminal outcome. `submitting` and `pending` remain observations only. The
browser owner MUST record the terminal handoff before `runtime_complete_turn` so
completion cannot outrun delivery evidence. If the record call itself fails, a
verified submission remains completion-pending and reconciliation retries this
operation; it MUST NOT resubmit the browser prompt.

#### `web_model_browser_attach`

Attach to the currently active daemon-owned ChatGPT Web Model browser surface over a dedicated bidirectional NDJSON stream.

Args:
```ts
{
  group_id?: string
  actor_id?: string
  by?: string
  viewer_mode?: "auto" | "screencast" | "vnc"
}
```

Handshake result:
```ts
{ group_id: string; actor_id: string }
```

Streaming mode:
- After a successful handshake, the connection upgrades into the browser-surface stream described in §4.6.
- The daemon emits `state` items when runtime/session status changes and `frame` items for captured browser frames.
- The client MAY send browser-control commands (`navigate`, `back`, `refresh`, `click`, `scroll`, `key`, `text`, `resize`, `close`, `disconnect`).
- The daemon owns the browser runtime; Web clients are surface proxies and MUST NOT create a separate ChatGPT browser runtime for the same actor.
- When `group_id` or `actor_id` is supplied, the actor MUST exist and use `runtime=web_model`.
- If no active Web Model browser surface exists, attach SHOULD fail with `browser_surface_not_found`.
- If the underlying browser runtime is no longer active, attach SHOULD fail with `browser_surface_not_active`.

#### `web_model_browser_vnc_attach`

Attach to the currently active daemon-owned ChatGPT Web Model browser surface over a raw RFB/VNC stream.

Args:
```ts
{
  group_id?: string
  actor_id?: string
  by?: string
}
```

Handshake result:
```ts
{ group_id: string; actor_id: string }
```

Streaming mode:
- After a successful handshake, the connection upgrades into a raw VNC/RFB byte stream.
- The operation SHOULD fail with `browser_vnc_unavailable` when the browser surface is not backed by a local VNC projection.

### 8.20 Copy Groups

Copy Groups operations export/import durable CCCC group state as a zip package. Copy packages contain CCCC group state only; workspace repository files are not included.

#### `group_copy_export`

Export one group as a base64-encoded zip package.

Args:
```ts
{
  group_id: string
  by?: string
}
```

Result:
```ts
{
  package_b64: string
  filename: string
  manifest: {
    kind: "cccc.group_copy"
    version: number
    source_group_id: string
    source_title?: string
    exported_at: string
    cccc_version?: string
    source_platform?: string
    export_mode: "group_state_only"
    workspace_included: false
    contains_secrets: false
    content_digest?: string
    content?: Record<string, unknown>
  }
}
```

Notes:
- Export MUST exclude live runtime state, browser profiles, credentials, connector secrets, lock files, and rebuildable caches.
- Export MUST scrub actor environment secrets from packaged `group.yaml`.
- `contains_secrets: false` means CCCC-managed live credentials and auth sessions are excluded. The package can still contain user-provided sensitive content such as ledger history, memory, blobs, and attachments.
- This compatibility operation is intended for small packages. Large packages SHOULD use `group_copy_export_file` and pass the returned `package_path` to preview/import.

#### `group_copy_export_file`

Export one group as a zip package stored on the daemon host filesystem.

Args:
```ts
{
  group_id: string
  by?: string
}
```

Result:
```ts
{
  package_path: string
  package_size_bytes: number
  filename: string
  manifest: {
    kind: "cccc.group_copy"
    version: number
    source_group_id: string
    source_title?: string
    exported_at: string
    cccc_version?: string
    source_platform?: string
    export_mode: "group_state_only"
    workspace_included: false
    contains_secrets: false
    content_digest?: string
    content?: Record<string, unknown>
  }
}
```

Notes:
- The package path is a temporary daemon-local file path intended for local download flows.
- This operation uses the large package limit. Secret-scrubbing requirements match `group_copy_export`.

#### `group_copy_preview_import`

Validate a copy package and return an import preview without writing group state.

Args:
```ts
{
  package_b64?: string
  package_path?: string
  by?: string
}
```

Exactly one of `package_b64` or `package_path` is required. `package_b64` is a small-package compatibility path; large local flows SHOULD use `package_path`.

Result:
```ts
{
  preview: {
    manifest: Record<string, unknown>
    source_group_id: string
    source_title: string
    actor_count: number
    actors: Array<Record<string, unknown>>
    source_workspace_root: string
    workspace_root_exists: boolean
    group_id_conflict: boolean
    target_default_scope_conflict?: boolean
    requires_reconnect?: Record<string, boolean>
    workspace_included: false
    contains_secrets: false
    runtime_reset?: Record<string, unknown>
  }
}
```

Errors:
- `invalid_group_copy` when the payload is not a valid supported CCCC group copy.
- `contains_secrets: false` in the preview has the same meaning as export: system credentials are excluded, but user content in ledger history, memory, blobs, and attachments can still be sensitive.

#### `group_copy_import`

Import a group copy into the current `CCCC_HOME`.

Args:
```ts
{
  package_b64?: string
  package_path?: string
  workspace_root?: string
  title?: string
  by?: string
}
```

Exactly one of `package_b64` or `package_path` is required. `package_b64` is a small-package compatibility path; large local flows SHOULD use `package_path`.

Result:
```ts
{
  group_id: string
  source_group_id: string
  group_id_conflict: boolean
  workspace_root: string
  active_scope_key: string
}
```

Notes:
- Import MUST stage and validate copy package contents before moving them into `groups/<group_id>`.
- If the source `group_id` conflicts in the target home, import MUST allocate a new group id.
- Imported groups MUST start stopped: `running=false`, `state="idle"`.
- `workspace_root`, when supplied, remaps the active workspace root during import.
- Import MUST reject unsupported copy package schema versions, workspace-including copy packages, secret-containing copy packages, path traversal, symlinks, duplicate entries, and unsafe package paths.

## 9. Appendix: Example Lines

### 9.1 Ping

Request line:
```json
{"v":1,"op":"ping","args":{}}
```

Response line:
```json
{"v":1,"ok":true,"result":{"version":"0.4.x","implementation":"rust","pid":12345,"ts":"2026-01-13T12:34:56Z","ipc_v":1,"capabilities":{"events_stream":true,"remote_access":true}},"error":null}
```

### 9.2 Error

```json
{"v":1,"ok":false,"result":{},"error":{"code":"missing_group_id","message":"missing group_id","details":{}}}
```
