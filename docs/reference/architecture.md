# Architecture

> CCCC = Collaborative Code Coordination Center
>
> A global AI Agent collaboration hub: a single daemon manages multiple working groups, with Web/CLI/IM as entry points.

## Core Concepts

### Working Group

- Like an IM group chat, but with execution/delivery capabilities
- Each group has an append-only ledger (event stream)
- Can bind multiple Scopes (project directories)

### Actor

- **Foreman**: Coordinator + Executor (the first enabled actor automatically becomes foreman)
- **Peer**: Independent expert (other actors)
- CLI Runtimes expose their native terminal; capable Runtimes pair it with a structured background protocol on the same session. Web Model uses browser/remote-MCP delivery.

### Ledger

- Authoritative message and collaboration event history: `~/.cccc/groups/<group_id>/ledger.jsonl`
- Append-only events support history, delivery/read/reply projections, and audit
- Group configuration lives in `group.yaml`; tasks and coordination state live in `context/`. The ledger alone is not a backup of all CCCC state.

## Directory Layout

Default: `CCCC_HOME=~/.cccc`

```
~/.cccc/
├── registry.json                 # Working group index
├── daemon/
│   ├── ccccd.pid
│   ├── ccccd.log
│   └── ccccd.sock               # IPC socket
└── groups/<group_id>/
    ├── group.yaml               # Metadata
    ├── ledger.jsonl             # Event stream (append-only)
    ├── context/                 # Durable coordination store
    │   ├── context.yaml         # Brief, decisions, handoffs, metadata
    │   ├── tasks/T*.yaml        # One durable task per file
    │   ├── agents.yaml          # Per-actor hot/warm context
    │   └── version_state.json   # ctxv:* optimistic concurrency revisions
    └── state/                   # Runtime state
        └── blobs/               # Large text/attachments (referenced in ledger)
```

The `context/` files are the one authoritative coordination store. Older preview
`state/context.json` files are imported once without deleting the source; new
writes never create a second implementation-specific task store.

The native daemon retains the final 0.4.35 control-plane paths and schemas:

| State | Authoritative path |
|---|---|
| Global settings | `settings.yaml` |
| Actor profiles | `state/actor_profiles/profiles.json` |
| Profile private environment | `state/secrets/actor_profiles/*.json` |
| Actor private environment | `state/secrets/actors/<group_id>/*.json` |
| Mail Inbox cursors | `groups/<group_id>/state/read_cursors.json` |
| Automation runtime state | `groups/<group_id>/state/automation.json` |
| Capability catalog and bindings | `state/capabilities/catalog.json`, `state/capabilities/state.json` |
| Capability allowlist overlay | `config/capability-allowlist.user.yaml` |
| Group Space providers, bindings, and jobs | `state/space/providers.json`, `bindings.json`, `jobs.json` |
| Group Space credentials | `state/secrets/space_providers/*.json` |

Files from the earlier preview layout are migration inputs, not parallel runtime
stores. Canonical data wins on conflicts, migration is idempotent, and subsequent
writes go only to the canonical path. Frozen 0.4.35 homes and native tests cover
the supported migration boundary, including group-copy packages.

## Architecture and Ownership

CCCC is a modular native application. One daemon owns the shared collaboration
control plane; entry points and integration hosts use its IPC contract. A Rust
crate boundary is not necessarily a process boundary.

```text
Browser UI ── HTTP / WebSocket / SSE ── Web host
                                           │
CLI and MCP collaboration tools ─────────── IPC ── Daemon
                                                    │
                                      Group / Context / Ledger
                                                    │
                                      Actor runtime supervision
```

The Web host also owns browser and media integrations, including Voice Analyst
sessions and IM workers. These resources are not roster Actors. MCP additionally
hosts local command tools. Persistent command sessions carry their owning Home;
MCP shutdown releases only those sessions, leaving other hosts and Actor/Analyst
runtimes alone. Their process-local state does not replace Group
configuration, message history, authorization, or delivery facts.

Finite MCP shell/Git commands and daemon MCP-setup helpers share the runtime's
bounded command capture function. It feeds stdin while draining both output
streams, applies one deadline, and releases the existing process-group / Windows
Job owner on completion or cancellation. Shell results report truncation; setup
helpers reject oversized output rather than interpreting partial configuration.
The synchronous adapter is scoped to the call and adds no permanent executor.
This boundary is intentionally separate from persistent sessions and provider
commands such as Claude Agent View background launch, which must retain their
provider-owned service after the launcher exits.

### Logical Modules

| Module | Responsibility | Boundary |
|---|---|---|
| `cccc-contracts` | Event, Actor, IPC envelope and shared wire types | No port or provider dependency |
| `cccc-core` | Group, Context, ledger, permissions and durable stores | Shared semantics and persistence; no Web/CLI dependency |
| `cccc-runtime` | Native process, PTY, terminal history, input and attachment ownership | OS and terminal mechanisms; not the managed provider protocol layer |
| `cccc-client` | Daemon discovery and IPC transport | Distinguishes connection failure from an unknown result after submission |
| `cccc-daemon` | Operation authorization, lifecycle, delivery, automation and managed provider adapters | Shared control-plane authority |
| `cccc-web` | Web API, UI assets, browser/media/IM integration hosts | Uses daemon operations for shared mutations; retains its own live resources |
| `cccc-mcp` | Agent tool surface and local tool execution | Collaboration tools map to daemon semantics |
| `cccc-cli` | Public executable and process composition | Builds and launches the product entry points |

Managed provider protocols currently live under the daemon's
`ops/codex_voice_analyst/` module and are shared by Actor and Voice Analyst
sessions. The historical module name does not imply that Actor execution depends
on an active Voice call. Codex, ACP, Claude and OpenCode-family protocol differences
remain local to their adapters; the Web host reuses the Analyst library code in
its own process.

### State Authority

| Fact | Owner / authoritative representation | Derived or temporary state |
|---|---|---|
| Group and Actor configuration | Daemon operations and `group.yaml` | Registry summaries and UI snapshots |
| Tasks and coordination context | Context operations and `context/` with `ctxv:*` revisions | Rendered prompts and context views |
| Messages, delivery/read/reply events | Daemon and Group ledger | Search indexes, inbox projections and notification eligibility |
| Runtime execution | Owning host and provider session | Status snapshots, retained terminal output and activity previews |
| Browser interaction and media | Browser / Web host | Selected view, transcript display, media tracks and connections |

State files use their own atomic-write and concurrency mechanisms. This is not a
fully event-sourced application or a transaction spanning all files. Operations
must expose partial failure and preserve the identity needed for safe recovery.
For example, a Web diarization completion updates its canonical session and asks
the daemon to append the completion event in the same operation; the Web host
never appends that event independently.

Context authorizes operations in order against one locked working copy, then
persists only after the complete batch passes validation. Unreadable canonical
files fail explicitly. A revision is reserved before payload writes so an I/O
failure cannot leave changed files accepting an old compare-and-swap token;
callers reload after such an error because there is no multi-file rollback.

Ledger followers use the stable append/compaction lock for their initial
snapshot. Established polling skips a busy writer without advancing its cursor,
so another Group can still be polled. Ordinary appends use byte offsets; archive
source changes use the existing reverse reader and stop at the last event ID.
This preserves unseen events across rotation/refill without loading the old
historical prefix into an index or adding a second replay store. Compressed
archives require sequential decompression; their retained output still begins
at the cursor.

Maintenance validates and hashes ledger records in one streaming pass under the
writer lock. It does not populate the query index or retain the entire history;
unreadable Event objects fail before snapshot publication or rotation. Queries
that need random access still use the existing weighted index cache. Delivery,
turn recovery, queue counts, and reminder projections borrow that index and copy
only their results. They release the read guard before appending new events.
Reminder scans stop before loading history when no Actor is eligible. These
boundaries avoid repeated full-history copies without introducing a second cache
or changing the ledger's authority; large active query indexes still consume
memory proportional to their history.

Cold or invalidated query indexes capture source revisions and events under the
same shared append/compaction lock. Appenders release the writer lock before
updating the index; a late callback cannot apply an already-indexed event again.
Cache removal updates its bookkeeping under the global mutex, then releases the
removed index outside that mutex so deallocation does not block other Groups.
The cache budget is an eviction policy, not a process RSS ceiling: active readers
and one oversized history can retain additional memory.

### Dispatch and Lifecycle

Each ordinary operation declares its handler and access policy together in a
pure resolver. The dispatcher uses that declaration to acquire global or Group
read/write permits before executing the handler. Adding an operation does not
require updating a separate operation-name whitelist. Cross-Group mutations
retain global serialization; authorization remains in the operation itself.

Some live resources own their synchronization. Completion, polling and input
paths that must stay available during lifecycle draining use that resource
ownership explicitly. New Bridge work still participates in global draining.
MCP catalog discovery during Actor startup cannot wait behind a global writer
that is itself waiting for startup to finish. Terminal I/O and cancellation must
not retain the session state mutex while blocked on a child process.

The `term_attach` and `events_stream` operations upgrade the connection protocol
and are handled by dedicated stream owners before ordinary dispatch. Their
subscription/attachment lifetimes differ from a request/response permit.

Finite auxiliary probes also have explicit limits: DeepSeek's Node version probe
has a five-second deadline and bounded output, and Tailscale up/down has a
30-second deadline using the same owned-command capture. A Tailscale timeout is
an uncertain external outcome, not a rollback or an automatic retry. Persistent
provider services and tunnel supervisors keep their own lifecycle interfaces.


The dependency direction and single local control plane are deliberate. Moving
media into the daemon, introducing a generic provider plugin system, or turning
all durable state into event replay requires a concrete benefit beyond tidier
layer diagrams.

## Ledger Schema (v1)

### Event Envelope

```jsonc
{
  "v": 1,
  "id": "event-id",
  "ts": "2025-01-01T00:00:00.000000Z",
  "kind": "chat.message",
  "group_id": "g_xxx",
  "scope_key": "s_xxx",
  "by": "user",
  "data": {}
}
```

### Known Kinds

| Kind | Description |
|------|-------------|
| `group.create` | Create a working group |
| `group.update` | Update group metadata |
| `group.attach` | Attach a scope to a working group |
| `group.detach_scope` | Detach a scope from a working group |
| `group.set_active_scope` | Select the active scope for a group |
| `group.start` | Start group runtime actors |
| `group.stop` | Stop group runtime actors |
| `group.set_state` | Set group lifecycle state |
| `group.settings_update` | Update group settings |
| `group.automation_update` | Update group automation configuration |
| `actor.add` | Add an actor |
| `actor.update` | Update actor metadata/configuration |
| `actor.set_role` | Set actor role |
| `actor.start` | Start an actor runtime |
| `actor.stop` | Stop an actor runtime |
| `actor.restart` | Restart an actor runtime |
| `actor.new_session` | Start a fresh provider session for an actor |
| `actor.remove` | Remove an actor |
| `actor.activity` | Runtime activity/status snapshot |
| `context.sync` | Context/control-plane sync event |
| `chat.message` | Chat message |
| `chat.cross_group_receipt` | Source-group receipt that links a cross-group send to its destination event |
| `chat.stream` | Progressive stream chunk/update |
| `mail.read` | Consuming Mail cursor boundary |
| `chat.reply_request.cancelled` | Cancels remaining reply obligations |
| `runtime.delivery` | Per-recipient runtime handoff evidence |
| `chat.reaction` | Chat reaction |
| `system.notify` | System notifications, including bounded Mail/reply notices |
| `assistant.settings_update` | Update built-in assistant settings |
| `assistant.status_update` | Update built-in assistant lifecycle/health |
| `assistant.voice.document` | Voice Secretary working document save/update/archive marker |
| `assistant.voice.input` | Voice Secretary transcript/input ingestion marker |
| `assistant.voice.prompt_draft` | Voice Secretary composer prompt draft submit/ack marker |
| `assistant.voice.request` | Voice Secretary structured action request marker |
| `assistant.voice.session` | Voice Secretary recording session status/artifact marker |
| `presentation.publish` | Publish a presentation rail card |
| `presentation.clear` | Clear presentation rail card(s) |

### `chat.message` Data

```ts
data: {
  text: string
  format?: "plain" | "markdown"
  insight?: string | null
  message_mode: "send" | "request_reply" | "mail"
  to?: string[]
  reply_to?: string | null
  quote_text?: string | null
  attachments?: AttachmentRefV1[]
  refs?: ReferenceV1[]
}
```

The authoritative shape and validation rules live in
[CCCS v1](../standards/CCCS_V1.md#61-chatmessage).

### Recipient Semantics (`to` field)

| Token | Semantics |
|-------|-----------|
| omitted / `[]` | Materialize the group's `default_send_to` as `@foreman` or `@all` before append |
| `user` / `@user` | The human user |
| `@all` | All actors |
| `@peers` | All peers |
| `@foreman` | Foreman |
| `<actor_id>` | Specific actor |

A message addresses either the human user or one or more actors, never both.
`request_reply` requires concrete actor recipients, and `mail` is actor-only.

## Files and Attachments

### Design Principles

- **Ledger stores only references, not large binaries**: Large text/attachments go to `CCCC_HOME` blobs (e.g., `groups/<group_id>/state/blobs/`).
- **No automatic writes to repo by default**: Attachments belong to the runtime domain (`CCCC_HOME`); if needed in scope/repo, user/agent explicitly copies/exports.
- **Content is portable**: Attachments use `sha256` as stable identity, allowing future cross-group/repo copy and reference rewriting.

## Roles and Permissions

### Role Definitions

- **Foreman = Coordinator + Worker**
  - Does actual work, not just task assignment
  - Extra coordination duties (receives actor_idle and quiet-review `silence_check` notifications)
  - Can add/start/stop any actor

- **Peer = Independent Expert**
  - Has independent professional judgment
  - Can challenge foreman decisions
  - Can only manage self

### Permission Matrix

| Action | user | foreman | peer |
|--------|------|---------|------|
| actor_add | ✓ | ✓ | ✗ |
| actor_start | ✓ | ✓ (any) | ✗ |
| actor_stop | ✓ | ✓ (any) | ✓ (self) |
| actor_restart | ✓ | ✓ (any) | ✓ (self) |
| actor_remove | ✓ | ✓ (self/peer) | ✓ (self) |

## MCP Server

MCP is exposed as an action-oriented surface. Tool count is intentionally not hardcoded, because optional capability packs can add more tools when enabled.

The surface is best understood as capability groups instead of a fixed namespace/tool count. Each group can expose one or more MCP tools, and some groups use action-style wrappers rather than one-tool-per-operation naming.

### Core Collaboration Capability Groups

- Session and guidance: `cccc_bootstrap`, `cccc_help`, `cccc_project_info`
- Messaging and files: `cccc_inbox_read`, `cccc_message_history`, `cccc_message_send`, `cccc_message_reply`, `cccc_file`
- Group and actor control: `cccc_group`, `cccc_actor`
- Coordination and state: `cccc_context_get`, `cccc_coordination`, `cccc_task`, `cccc_agent_state`, `cccc_context_sync`
- Automation and memory: `cccc_automation`, `cccc_automation_manage`, `cccc_memory`, `cccc_memory_admin`

### Capability-Managed and Optional Groups

- These capability groups expand the surface without hardcoding a fixed namespace count. The current grouped tools include lifecycle and pack control (`cccc_capability_search`, `cccc_capability_enable`, `cccc_capability_block`, `cccc_capability_state`, `cccc_capability_import`, `cccc_capability_uninstall`, `cccc_capability_use`).
- Space / notebook integrations: `cccc_space`
- Terminal and diagnostics: `cccc_terminal`, `cccc_terminal_tail`, `cccc_debug_*`
- IM binding: `cccc_im_bind`

## Tech Stack

| Layer | Technology |
|-------|------------|
| Kernel/Daemon | Rust |
| Web Port | Rust + Axum |
| Web UI | React + TypeScript + Vite + Tailwind + xterm.js |
| MCP | stdio mode, JSON-RPC |

## Source Structure

```
crates/
├── cccc-contracts/        # Versioned wire types
├── cccc-core/             # Durable state and kernel
├── cccc-daemon/           # Control plane, delivery and managed provider adapters
├── cccc-runtime/          # Native process and terminal mechanisms
├── cccc-client/           # Daemon IPC transport
├── cccc-web/              # Web API, embedded UI and integration hosts
├── cccc-mcp/              # MCP server
└── cccc-cli/              # Public cccc executable
```
