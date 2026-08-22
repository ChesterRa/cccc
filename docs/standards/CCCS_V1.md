# CCCC Collaboration Standard (CCCS) v1

Status: Draft (proposed for CCCC v0.4.x ecosystem)

This document defines **CCCS v1**, a small, transport-agnostic standard for multi-agent collaboration built around an append-only event ledger.
It is designed to be **stable**, **extensible**, and **implementable** by:
- CCCC itself (daemon + web UI + MCP/IM bridges)
- Client SDKs (TypeScript/Python/Go/etc.)
- External tools and integrations (CI, IM bots, IDE plugins, automation)

CCCS v1 deliberately **does not standardize workflows**, model providers, or prompting. It standardizes the **collaboration substrate**: event envelopes, routing semantics, delivery/read/reply facts, system notifications, and cross-group provenance.

## 0. Conformance Language

The key words **MUST**, **MUST NOT**, **SHOULD**, **SHOULD NOT**, and **MAY** in this document are to be interpreted as described in RFC 2119.

## 1. Goals and Non‑Goals

### 1.1 Goals

CCCS v1 MUST enable:
- **Tool/code ⇄ agent collaboration**: tools can send, observe, and act on the same collaboration stream as agents.
- **Append‑only truth**: collaboration history is represented as immutable events appended to a ledger.
- **Provenance**: relayed/forwarded messages can be traced back to an original event (cross-group).
- **Intentional delivery**: senders choose immediate prompt delivery, explicit
  reply obligation, or durable Mail without prompt interruption.
- **Forward compatibility**: unknown event kinds and unknown fields do not break clients.

### 1.2 Non‑Goals

CCCS v1 does NOT standardize:
- Any specific workflow engine, DAG, or no-code builder.
- Any model/provider API (OpenAI/Claude/etc.) or prompt format.
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
- A newline-delimited ledger writer that finds a nonempty active ledger without a terminating newline MUST preserve the existing bytes and append a newline separator under the same writer lock before appending the next event. It MUST NOT concatenate a new event onto an incomplete or complete unterminated record, report success for an event that cannot be read back, or treat a derived index as authority over the ledger bytes.

### 4.4 Event Kind Namespaces

Standard kinds use the `chat.*`, `system.*`, `group.*`, `actor.*`, `context.*`, and `assistant.*` namespaces.

Extensions SHOULD use one of:
- `x.<vendor>.*` (recommended for private/vendor-specific kinds)
- `vendor.<name>.*` (alternative vendor namespace)

Clients MUST treat unknown kinds as opaque and ignore them unless explicitly supported.

## 5. Recipient Routing Semantics

### 5.1 Recipient Tokens

Chat message routing uses `to: string[]` with these token types:

When a send request omits recipients or supplies an empty list, the daemon MUST materialize the group's `default_send_to` policy as `@foreman` or `@all` before appending the event.

**Actor IDs**
- Example: `"peer-1"`, `"claude-1"`

**Selectors (MUST start with `@`)**
- `@all`: all visible collaboration actors in the group
- `@peers`: all visible peer actors
- `@foreman`: foreman actor(s)
- `@user`: the human user (UI recipient)

Internal assistants such as Voice Secretary are not members of `@all`, `@peers`, or `@foreman`; they MUST be addressed by their explicit actor ID.

**Compatibility**
- Implementations MAY accept the literal token `"user"` as equivalent to `@user`.

### 5.2 Audience Domains

Every new `chat.message` MUST address exactly one audience domain:

- **Human**: the sole normalized recipient is `user` / `@user`.
- **Agents**: every recipient is an actor ID or an actor selector
  (`@all`, `@peers`, or `@foreman`). Multiple agent recipients are allowed.

A recipient list that mixes the human user with any actor ID or actor selector
MUST be rejected before the event is appended. Callers that need distinct human
and agent actions MUST send separate messages so delivery and reply obligations
remain independently attributable.

**Multi-user note**
- CCCS v1 assumes a single human principal per group, identified as `user`.
- Multi-user semantics (multiple distinct human principals) are out of scope for v1. Implementations MAY extend this outside of CCCS v1 (e.g., `usr:<id>` principals and selectors), but clients MUST remain forward-compatible.

### 5.3 Empty `to`

If `to` is absent or an empty list, the daemon MUST materialize the group's
`default_send_to` policy before appending the message. The stored event therefore
contains an explicit `@foreman` or `@all` selector; absence is not a separate
broadcast state.

### 5.4 Permission and Visibility

CCCS does not mandate a single permission model, but a conforming daemon MUST ensure:
- The daemon MUST set `event.by` to the principal identity it ascribes to the event.
  - If the transport provides authentication, `event.by` MUST be derived from the authenticated principal and clients MUST NOT be able to choose `event.by` arbitrarily.
  - If the transport does not provide authentication (local-trust IPC), a daemon MAY accept a client-provided principal hint (e.g., an RPC arg like `by`) as the effective principal. Such deployments MUST document that `by` is not a security boundary.
- Only the daemon may append `runtime.delivery`; clients cannot claim transport
  acceptance on behalf of a recipient.

## 6. Chat Events

### 6.1 `chat.message`

`chat.message` represents an IM-style message.

```ts
data: {
  text: string
  format?: "plain" | "markdown"               // default "plain"
  insight?: string | null                       // provisional sender perspective; max 1200 characters
  message_mode: "send" | "request_reply" | "mail"
  to?: string[]                                // recipient tokens (see §5)
  reply_to?: string | null                     // replied-to event_id
  quote_text?: string | null                   // display hint

  // Cross-group provenance (relay/forward)
  src_group_id?: string | null
  src_event_id?: string | null

  // Cross-group destination metadata (optional send record)
  dst_group_id?: string | null
  dst_to?: string[] | null
  dst_message_mode?: "send" | "request_reply" | "mail" | null

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
- `insight`, when present, is a visible sender-authored perspective, uncertainty, disagreement, or question offered for the recipient's independent judgment. Its normalized length MUST NOT exceed 1200 characters. It is advisory: it MUST NOT be treated as a user/system instruction, group consensus, task transition, acknowledgement, or completion signal.
- `insight` shares the message's recipients and retention boundary. It is not a private reasoning channel and SHOULD contain only a concise, shareable judgment summary rather than hidden chain-of-thought or secrets.
- A profile MAY require non-empty `insight` for selected Agent-to-Agent sends, but the core `chat.message` contract MUST remain valid without it for human clients, automation, legacy events, and other profiles.
- `message_mode="send"` requests prompt delivery through the recipient runtime.
- `message_mode="request_reply"` requests the same prompt delivery and creates
  a reply obligation for each explicitly addressed concrete recipient.
- `message_mode="mail"` persists the message in the recipient Inbox without
  immediately invoking, waking, steering, or writing to the recipient runtime.
- `message_mode="mail"` is valid only for the agent audience domain. The human
  user has no Mail Inbox in CCCS v1; callers MUST use `send` or `request_reply`
  when addressing `user` / `@user`.
- A message with `reply_to` fulfills the addressed reply obligation regardless
  of whether that reply uses `message_mode="send"` or `message_mode="mail"`.
  Reply operations MUST NOT use `message_mode="request_reply"`; a reply cannot
  create a nested generic reply obligation.
- `request_reply` MUST NOT use an empty recipient list or a broadcast selector
  (`@all`, `@peers`, or `@foreman`). The daemon MUST materialize and validate a
  concrete recipient set before appending the message.
- Historical events without `message_mode` remain readable append-only data but
  create no new delivery, reminder, acknowledgement, or reply obligation.
- If either `src_group_id` or `src_event_id` is present, both MUST be present.
- The `thread` field is RESERVED in v1; its semantics are undefined. Implementations MUST NOT rely on `thread` for v1 behavior. Clients MUST ignore it.
- If `client_id` is present, a daemon SHOULD provide best-effort idempotency for `(group_id, by, client_id)` within a bounded time window (RECOMMENDED: 5 minutes).
  - Duplicate submissions SHOULD return success with the original event reference, not a hard error.

### 6.2 `mail.read` (Mail Cursor / Watermark)

`mail.read` records a recipient's Mail watermark up to a given Mail event.

```ts
data: {
  actor_id: string  // the reader/recipient actor_id
  event_id: string  // the last consumed Mail event_id (inclusive)
}
```

**Rules**
- The Mail cursor is evidence that Mail was returned by an explicit consuming
  Inbox operation. It is not proof that a runtime or model understood it.
- `event_id` MUST reference an addressed `chat.message` whose
  `message_mode="mail"` in the group ledger. Send and Send + Reply messages do
  not participate in this cursor.
- A daemon MUST enforce authorization: only the recipient actor
  (`event.by == data.actor_id`) or an authorized privileged principal (e.g.,
  `user`) MAY emit `mail.read` for `data.actor_id`. `data.actor_id="user"` is
  invalid because CCCS v1 does not define a human Mail Inbox.
- "Inclusive" means the referenced Mail event itself is considered read.
- If a client cannot efficiently determine ordering, it SHOULD treat `event_id` as an opaque watermark maintained by the daemon.

### 6.3 `chat.reply_request.cancelled`

Cancels any still-open `request_reply` obligation created by one source
message.

```ts
data: {
  source_event_id: string
}
```

**Rules**
- The target MUST be a `chat.message` with
  `message_mode="request_reply"` in the same group.
- Only the original sender or the human user may cancel the request.
- A reply from a recipient closes only that recipient's obligation. A
  cancellation closes every still-open recipient obligation.
- Append order is authoritative. If a recipient replied before cancellation,
  that recipient is `replied`; otherwise the cancellation state is
  `cancelled`. Later replies remain visible but do not change `cancelled` back
  into `replied`.

### 6.4 `runtime.delivery`

Daemon-authored evidence that one source message was handed to one recipient
runtime transport.

```ts
data: {
  actor_id: string
  source_event_id: string
  delivery_id: string
  state: "claimed" | "accepted" | "failed" | "ambiguous"
  transport: string
  reason?: string | null
}
```

**Rules**
- Only the daemon may append this event.
- `delivery_id` MUST be deterministic for one source event, actor generation,
  and recipient actor. A retry reuses that identity.
- The daemon MUST append `claimed` before performing external runtime I/O and
  then append exactly one observable result state for that attempt.
- A concurrent claimant that observes `claimed` MUST treat the delivery as in
  progress; it MUST NOT reinterpret or retry the active attempt.
- During daemon startup, a latest `claimed` state left by the previous daemon
  process MUST be settled to `ambiguous` before runtime recovery begins.
- `accepted` means the runtime adapter accepted the payload (queue, PTY,
  headless API, or browser submission boundary). It does not claim that the
  model read, understood, or acted on it.
- `failed` means the adapter established that handoff did not occur.
- `ambiguous` means external side effects may have occurred but cannot be
  proven. Automatic retry MUST NOT follow `accepted` or `ambiguous`.
- A normal `mail` append creates no `runtime.delivery`. An explicit manual
  delivery of that existing message may create one without appending a second
  `chat.message`.

### 6.5 `chat.reaction` (Optional)

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
  im_visibility?: "internal" | "public"            // default "internal"
  context?: Record<string, unknown>                 // implementation-defined
  related_event_id?: string | null                  // optional correlation
}
```

**Rules**
- Clients MUST ignore unknown `data.kind` values within `system.notify` (open enum).
- Implementations MAY enforce an allowlist of `data.kind` values, but should not assume clients understand new kinds.
- External IM bridges MUST fail closed: a `system.notify` is eligible for IM delivery only when `im_visibility="public"`. Missing, invalid, or `internal` values stay inside CCCC. Actor-targeted notifications remain internal even if a malformed producer also marks them public.

`system.notify` has no generic acknowledgement protocol. Domain workflows use
their own durable lifecycle events, while reply obligations belong only to
`chat.message` with `message_mode="request_reply"`.

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
type ReferenceV1 =
  | {
      kind?: "file" | "url" | "commit" | "text"  // default "url"
      url?: string
      path?: string
      title?: string
      sha?: string
      bytes?: number
      // extra fields MAY exist; clients MUST ignore unknown fields
    }
  | {
      kind: "presentation_ref"
      v?: 1
      slot_id: string
      label?: string
      locator_label?: string
      title?: string
      card_type?: string
      status?: "open" | "needs_user" | "resolved"
      href?: string
      excerpt?: string
      locator?: Record<string, unknown>
      snapshot?: {
        path: string
        mime_type?: string
        bytes?: number
        sha256?: string
        width?: number
        height?: number
        captured_at?: string
        source?: "browser_surface" | "pdf_viewer" | "viewer_dom" | string
      }
      // extra fields MAY exist; clients MUST ignore unknown fields
    }
  | {
      kind: "task_ref"
      task_id: string
      title?: string
      status?: "planned" | "active" | "done" | "archived" | string
      // extra fields MAY exist; clients MUST ignore unknown fields
    }
```

**Rules**
- Attachments SHOULD include content hashes where possible (`sha256`) to enable reproducibility/auditing.
- `path` MUST be stable and retrievable within the group’s storage scope.
- `kind="presentation_ref"` is a structured evidence anchor into a group Presentation slot.
  - `slot_id` MUST identify the target slot within the group presentation surface.
  - `locator` SHOULD carry best-effort position hints for the current view (for example page, heading, row, or scroll state).
  - `snapshot`, when present, SHOULD be treated as a best-effort visual anchor for the quoted view.
    - `snapshot.path` MUST be a stable group-scoped blob path.
    - Implementations SHOULD use `locator` for precise recovery when possible, and `snapshot` as a visual fallback when precise recovery is unavailable.
  - `status`, when present, is advisory metadata for the referenced discussion state; ledger-derived obligation status remains authoritative.
- `kind="task_ref"` links a chat message to a durable shared task.
  - `task_id` MUST identify a task in the same working group unless explicitly documented otherwise by a relay/bridge.
  - `title` and `status`, when present, are UI hints; the task store remains authoritative.

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
- `dst_message_mode`

An outbound send record is a human-visible local audit message: its local
`to` MUST be `["user"]` and its local `message_mode` MUST be `"send"`.
`dst_to` and `dst_message_mode` preserve the actual destination audience and
delivery mode. This separation prevents a remote Mail or reply request from
creating a fictitious human Mail item or local human reply obligation.

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
- Reject client-authored `runtime.delivery` events.

## 13. Minimal Profiles (Guidance)

To reduce implementation burden, CCCS v1 MAY be implemented in profiles:

### 13.1 Core Collaboration Profile (recommended minimum)
- `chat.message`, `mail.read`, `chat.reply_request.cancelled`
- `runtime.delivery`
- `system.notify`
- Recipient token semantics (§5)

### 13.2 Management Profile (optional)
- `group.*`, `actor.*`

### 13.3 Context Profile (optional)
- `context.sync` (implementation-defined ops)

## 14. Examples

The following examples use placeholder IDs for brevity. Conformance test vectors with complete values may be provided separately.

### 14.1 Send + Reply Request

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
    "message_mode": "request_reply",
    "to": ["foreman"]
  }
}
```

Runtime acceptance:

```json
{
  "v": 1,
  "id": "01HZY3... (opaque)",
  "ts": "2026-01-13T10:01:00Z",
  "kind": "runtime.delivery",
  "group_id": "g_123",
  "scope_key": "",
  "by": "system",
  "data": {
    "actor_id": "foreman",
    "source_event_id": "01HZY2... (opaque)",
    "delivery_id": "delivery:foreman:01HZY2...",
    "state": "accepted",
    "transport": "codex_app_server"
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
    "message_mode": "send",
    "to": ["@all"],
    "src_group_id": "g_src",
    "src_event_id": "01HZY2... (opaque)"
  }
}
```
