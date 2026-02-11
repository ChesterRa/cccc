# CCCC Collaboration Standard (CCCS) v1

Status: Draft (proposed for CCCC v0.4.x ecosystem)

This document defines **CCCS v1**, a small, transport-agnostic standard for multi-agent collaboration built around an append-only event ledger.
It is designed to be **stable**, **extensible**, and **implementable** by:
- CCCC itself (daemon + web UI + MCP/IM bridges)
- Client SDKs (TypeScript/Python/Go/etc.)
- External tools and integrations (CI, IM bots, IDE plugins, automation)

CCCS v1 deliberately **does not standardize workflows**, model providers, or prompting. It standardizes the **collaboration substrate**: event envelopes, routing semantics, attention/ack, system notifications, and cross-group provenance.

## 0. Conformance Language

The key words **MUST**, **MUST NOT**, **SHOULD**, **SHOULD NOT**, and **MAY** in this document are to be interpreted as described in RFC 2119.

## 1. Goals and Non‑Goals

### 1.1 Goals

CCCS v1 MUST enable:
- **Tool/code ⇄ agent collaboration**: tools can send, observe, and act on the same collaboration stream as agents.
- **Append‑only truth**: collaboration history is represented as immutable events appended to a ledger.
- **Provenance**: relayed/forwarded messages can be traced back to an original event (cross-group).
- **Attention loops**: important messages have an explicit acknowledgement mechanism independent of “read”.
- **Forward compatibility**: unknown event kinds and unknown fields do not break clients.

### 1.2 Non‑Goals

CCCS v1 does NOT standardize:
- Any specific workflow engine, DAG, or no-code builder.
- Any model/provider API (OpenAI/Claude/Gemini/etc.) or prompt format.
- Any single transport (Unix socket, HTTP, SSE, WS, gRPC). CCCS v1 is transport-agnostic.
- Multi-tenant auth schemes (but it reserves fields and rules for provenance/permissions).

## 2. Terminology

- **Group**: A collaboration namespace (working group).
- **Scope**: A project root URL attached to a group; each event is attributed to a `scope_key`.
- **Actor**: A named agent identity within a group (e.g., `foreman`, `peer-1`).
- **Principal**: Any entity that can write events (`user`, an `actor_id`, `system`, or `svc:<name>`).
  - Service principals SHOULD use a stable namespace (RECOMMENDED: `svc:com.example.mybot` when disambiguation is needed).
  - The `svc:cccc.` prefix is RESERVED for CCCC ecosystem services.
- **Ledger**: An append-only sequence of events for a group.
- **Client**: Any process/UI/bot that reads or writes events via a daemon.
- **Daemon**: A single-writer authority that appends events and enforces permissions.

## 3. Core Object Model

### 3.1 Group

Each event belongs to exactly one group, identified by `group_id` (string).

### 3.2 Scope

Each group MAY have one or more scopes. Each event MUST include a `scope_key`:
- `scope_key` MAY be `""` (unknown / global / not tied to a scope).
- `scope_key` is a stable identifier for a scope assigned by the daemon.
- Clients MAY use `scope_key` for equality and filtering, but MUST treat it as an opaque string and MUST NOT parse or interpret its value.

### 3.3 Actor

Actors are identities within a group. CCCS v1 standardizes only:
- `actor_id`: stable string identifier
- `role`: `"foreman"` or `"peer"` (optional; the collaboration semantics do not require a role)

## 4. Event Envelope (Normative)

All events MUST use the envelope below. Field semantics are fixed.

```ts
interface CCCSEventV1 {
  v: 1
  id: string            // MUST be unique within the group ledger; SHOULD be globally unique (ULID/UUIDv7/UUID4)
  ts: string            // RFC3339 UTC timestamp assigned by the daemon at append time
  seq?: number          // OPTIONAL: monotonic sequence number assigned by the daemon (useful for streaming/cursors)
  kind: string          // e.g. "chat.message"
  group_id: string
  scope_key: string     // "" allowed
  by: string            // principal id ("user", "system", actor_id, or "svc:<name>") set by the daemon
  data: Record<string, unknown>
}
```

### 4.1 Forward Compatibility Rules

- Clients MUST ignore unknown `kind` values (but MAY display them as raw/unknown events).
- Clients MUST ignore unknown fields inside `data`.
- Clients MUST preserve the event envelope when relaying/forwarding (see §9).

### 4.2 Versioning

- `v` is the envelope version. CCCS v1 requires `v: 1`.
- Implementations MAY add a `data.v` field for kind-specific versioning, but MUST NOT change envelope semantics without bumping `v`.

### 4.3 Ordering and Timestamps

- The authoritative ordering of events is the **ledger append order**.
- `ts` MUST be assigned by the daemon at append time. Clients MUST NOT rely on client-local timestamps for ordering.
- Implementations MAY record a client-provided timestamp (RECOMMENDED: `data.client_ts`) for diagnostics or UI display, but it MUST NOT affect ordering.

### 4.4 Event Kind Namespaces

Standard kinds use the `chat.*`, `system.*`, `group.*`, `actor.*`, and `context.*` namespaces.

Extensions SHOULD use one of:
- `x.<vendor>.*` (recommended for private/vendor-specific kinds)
- `vendor.<name>.*` (alternative vendor namespace)

Clients MUST treat unknown kinds as opaque and ignore them unless explicitly supported.

## 5. Recipient Routing Semantics

### 5.1 Recipient Tokens

Chat message routing uses `to: string[]` with these token types:

**Actor IDs**
- Example: `"peer-1"`, `"claude-1"`

**Selectors (MUST start with `@`)**
- `@all`: all actors in the group
- `@peers`: all peer actors
- `@foreman`: foreman actor(s)
- `@user`: the human user (UI recipient)

**Compatibility**
- Implementations MAY accept the literal token `"user"` as equivalent to `@user`.

**Multi-user note**
- CCCS v1 assumes a single human principal per group, identified as `user`.
- Multi-user semantics (multiple distinct human principals) are out of scope for v1. Implementations MAY extend this outside of CCCS v1 (e.g., `usr:<id>` principals and selectors), but clients MUST remain forward-compatible.

### 5.2 Empty `to`

If `to` is absent or an empty list, the message is a **broadcast**.
For compatibility with CCCC v0.4.x semantics, broadcast SHOULD be treated as equivalent to `@all`.

### 5.3 Permission and Visibility

CCCS does not mandate a single permission model, but a conforming daemon MUST ensure:
- The daemon MUST set `event.by` to the principal identity it ascribes to the event.
  - If the transport provides authentication, `event.by` MUST be derived from the authenticated principal and clients MUST NOT be able to choose `event.by` arbitrarily.
  - If the transport does not provide authentication (local-trust IPC), a daemon MAY accept a client-provided principal hint (e.g., an RPC arg like `by`) as the effective principal. Such deployments MUST document that `by` is not a security boundary.
- A principal cannot acknowledge (`chat.ack`) on behalf of another recipient.

## 6. Chat Events

### 6.1 `chat.message`

`chat.message` represents an IM-style message.

```ts
data: {
  text: string
  format?: "plain" | "markdown"               // default "plain"
  priority?: "normal" | "attention"           // default "normal"
  to?: string[]                                // recipient tokens (see §5)
  reply_to?: string | null                     // replied-to event_id
  quote_text?: string | null                   // display hint

  // Cross-group provenance (relay/forward)
  src_group_id?: string | null
  src_event_id?: string | null

  // Cross-group destination metadata (optional send record)
  dst_group_id?: string | null
  dst_to?: string[] | null

  // Attachments and references (see §8)
  attachments?: AttachmentRefV1[]
  refs?: ReferenceV1[]

  // Reserved for future threading
  thread?: string

  // Optional idempotency key (client-generated)
  client_id?: string | null
}
```

**Rules**
- `text` MUST be present (it may be empty if and only if attachments convey the message).
- `priority="attention"` MUST trigger the attention/ack rules in §6.2.
- If either `src_group_id` or `src_event_id` is present, both MUST be present.
- The `thread` field is RESERVED in v1; its semantics are undefined. Implementations MUST NOT rely on `thread` for v1 behavior. Clients MUST ignore it.
- If `client_id` is present, a daemon SHOULD provide best-effort idempotency for `(group_id, by, client_id)` within a bounded time window (RECOMMENDED: 5 minutes).
  - Duplicate submissions SHOULD return success with the original event reference, not a hard error.

### 6.2 `chat.ack` (Attention Acknowledgement)

`chat.ack` is the **only** completion signal for attention messages.

```ts
data: {
  actor_id: string  // the acknowledging recipient ("user" or an actor_id)
  event_id: string  // the acknowledged chat.message event_id
}
```

**Rules**
- A daemon MUST enforce **self-only ACK**: `event.by` MUST equal `data.actor_id`.
- `chat.ack` MUST be idempotent per `(group_id, actor_id, event_id)`; repeated ACK MUST NOT create repeated side effects.
- `chat.ack` MUST reference a valid `chat.message` whose `priority` is `"attention"`.
- A daemon MUST reject ACK attempts for non-attention messages.
  - The error `code` SHOULD be `invalid_request` (or an implementation-specific equivalent such as `not_an_attention_message`).
- A recipient MUST NOT be required to ACK a message that was not addressed to them.
- `chat.ack` MUST be independent from read cursors. Marking read MUST NOT automatically clear the need for ACK.
  - Implementations MAY provide a convenience gesture where a recipient’s explicit “mark read” action on an attention message results in emitting `chat.ack`, but the `chat.ack` event MUST still exist as a distinct record.

### 6.3 `chat.read` (Read Cursor / Watermark)

`chat.read` records a recipient’s read watermark up to a given event.

```ts
data: {
  actor_id: string  // the reader/recipient ("user" or an actor_id)
  event_id: string  // the last read event_id (inclusive)
}
```

**Rules**
- Read is a **cursor**, not an acknowledgement.
- `event_id` MUST reference an event that exists in the group ledger.
- For the Core Collaboration Profile, `event_id` SHOULD reference an addressable event (RECOMMENDED: `chat.message` or `system.notify`) and the daemon SHOULD reject watermarks for events that are not addressed to `actor_id`.
- A daemon MUST enforce authorization: only the recipient (`event.by == data.actor_id`) or an authorized privileged principal (e.g., `user`) MAY emit `chat.read` for `data.actor_id`.
- “Inclusive” means the referenced `event_id` itself is considered read.
- If a client cannot efficiently determine ordering, it SHOULD treat `event_id` as an opaque watermark maintained by the daemon.

### 6.4 `chat.reaction` (Optional)

```ts
data: {
  event_id: string
  actor_id: string
  emoji: string
}
```

## 7. System Notification Events

System notifications are separated from chat to avoid polluting conversations.

### 7.1 `system.notify`

```ts
data: {
  kind: "nudge" | "keepalive" | "help_nudge" | "actor_idle" | "silence_check" | "automation" | "status_change" | "error" | "info" | string
  priority?: "low" | "normal" | "high" | "urgent"   // default "normal"
  title?: string
  message?: string
  target_actor_id?: string | null                   // null = broadcast
  context?: Record<string, unknown>                 // implementation-defined
  requires_ack?: boolean                            // default false
  related_event_id?: string | null                  // optional correlation
}
```

**Rules**
- Clients MUST ignore unknown `data.kind` values within `system.notify` (open enum).
- Implementations MAY enforce an allowlist of `data.kind` values, but should not assume clients understand new kinds.

### 7.2 `system.notify_ack`

```ts
data: {
  notify_event_id: string
  actor_id: string
}
```

**Rules**
- `system.notify_ack` MUST be self-only: `event.by` MUST equal `data.actor_id`.
- A daemon MUST NOT allow a principal to ack on behalf of another recipient.

## 8. Attachments and References

### 8.1 `AttachmentRefV1`

```ts
type AttachmentRefV1 = {
  kind?: "text" | "image" | "file"     // default "file"
  path: string                         // group-scoped path (implementation-defined)
  title?: string
  mime_type?: string
  bytes?: number
  sha256?: string
  // extra fields MAY exist; clients MUST ignore unknown fields
}
```

### 8.2 `ReferenceV1`

```ts
type ReferenceV1 = {
  kind?: "file" | "url" | "commit" | "text"  // default "url"
  url?: string
  path?: string
  title?: string
  sha?: string
  bytes?: number
  // extra fields MAY exist; clients MUST ignore unknown fields
}
```

**Rules**
- Attachments SHOULD include content hashes where possible (`sha256`) to enable reproducibility/auditing.
- `path` MUST be stable and retrievable within the group’s storage scope.

### 8.3 Attachment Resolution (Non‑Normative Guidance)

CCCS v1 does not mandate a transport, but implementations SHOULD provide a way to resolve `AttachmentRefV1.path` to bytes, for example:
- An HTTP endpoint (e.g., `GET /groups/{group_id}/blobs/{path}`), or
- An RPC/IPC operation that returns attachment metadata and streams bytes.

## 9. Cross‑Group Relay / Forward (Provenance)

CCCS v1 standardizes cross-group provenance via `src_group_id/src_event_id` on the **destination** message.

### 9.1 Relay Semantics

To relay a message from group A into group B:
- In group B, append a `chat.message` whose `data.src_group_id` and `data.src_event_id` reference the original event in group A.
- The relayed message MUST either:
  - (a) include the original text verbatim, or
  - (b) clearly indicate truncation/summarization in the message (e.g., prefix with `[Summarized]`) and include `src_group_id/src_event_id` for full content retrieval.

**Rules**
- If either `src_group_id` or `src_event_id` is set, both MUST be set.
- UIs SHOULD provide “Open source message” (jump-to) affordances.
- If the source is unavailable due to permissions or retention, clients MUST show a clear “source unavailable” state (not silent failure).

### 9.2 Optional Send Record

Implementations MAY also append an “outbound send record” in the source group (for auditability) by writing a local `chat.message` with:
- `dst_group_id`
- `dst_to`

This record is OPTIONAL and MUST NOT be required for the destination’s provenance correctness.

## 10. Event Stream Subscription (Transport‑Agnostic)

CCCS v1 defines an abstract “event stream” interface:
- Input: `(group_id, since_cursor?, filters?, follow?)`
- Output: an ordered stream of `CCCSEventV1`

### 10.1 Cursor

CCCS does not mandate a single cursor type. A daemon SHOULD support at least one:
- `since_ts` (timestamp cursor), or
- `since_event_id` (event-id cursor), or
- `since_seq` (monotonic sequence number)

Clients MUST treat cursors as opaque and MUST NOT infer ordering from `id`.
If `since_seq` is supported, it SHOULD correspond to the daemon-assigned `event.seq` field (when present).

### 10.2 Filters

Implementations SHOULD support filtering by:
- `kinds[]` (e.g., only `chat.message`, `system.notify`)
- `limit` (for bounded replays)

### 10.3 Capability Discovery (Non‑Normative Guidance)

Different implementations may support different cursor types and stream filters.
Implementations SHOULD expose capability discovery via their chosen transport (e.g., an `info`/`capabilities` endpoint or an IPC operation) so SDKs can adapt automatically.

## 11. Error Model (Recommended)

To make SDKs interoperable, daemons SHOULD expose errors as:

```ts
{
  code: string
  message: string
  details?: Record<string, unknown>
}
```

Recommended stable `code` values:
- `invalid_request`
- `permission_denied`
- `group_not_found`
- `actor_not_found`
- `event_not_found`
- `unknown_op`
- `daemon_unavailable`

## 12. Security Considerations (Minimal v1)

A conforming daemon MUST:
- Enforce **single-writer** semantics for the ledger.
- Set `event.by` to a principal identity consistent with the deployment’s trust model (authenticated vs. local-trust IPC) and document the security properties.
- Enforce self-only ack rules for `chat.ack` and `system.notify_ack`.

## 13. Minimal Profiles (Guidance)

To reduce implementation burden, CCCS v1 MAY be implemented in profiles:

### 13.1 Core Collaboration Profile (recommended minimum)
- `chat.message`, `chat.ack`, `chat.read`
- `system.notify`, `system.notify_ack` (optional ack)
- Recipient token semantics (§5)

### 13.2 Management Profile (optional)
- `group.*`, `actor.*`

### 13.3 Context Profile (optional)
- `context.sync` (implementation-defined ops)

## 14. Examples

The following examples use placeholder IDs for brevity. Conformance test vectors with complete values may be provided separately.

### 14.1 Attention Message + Ack

```json
{
  "v": 1,
  "id": "01HZY2... (opaque)",
  "ts": "2026-01-13T10:00:00Z",
  "kind": "chat.message",
  "group_id": "g_123",
  "scope_key": "s_abc",
  "by": "user",
  "data": {
    "text": "Please review the release checklist today.",
    "format": "plain",
    "priority": "attention",
    "to": ["@foreman"]
  }
}
```

Ack:

```json
{
  "v": 1,
  "id": "01HZY3... (opaque)",
  "ts": "2026-01-13T10:01:00Z",
  "kind": "chat.ack",
  "group_id": "g_123",
  "scope_key": "",
  "by": "foreman",
  "data": {
    "actor_id": "foreman",
    "event_id": "01HZY2... (opaque)"
  }
}
```

### 14.2 Cross‑Group Relay

Destination group message:

```json
{
  "v": 1,
  "id": "01HZY4... (opaque)",
  "ts": "2026-01-13T10:02:00Z",
  "kind": "chat.message",
  "group_id": "g_dst",
  "scope_key": "",
  "by": "svc:relay",
  "data": {
    "text": "Relayed: please review the release checklist today.",
    "priority": "attention",
    "to": ["@all"],
    "src_group_id": "g_src",
    "src_event_id": "01HZY2... (opaque)"
  }
}
```
