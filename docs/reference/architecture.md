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
- Supports PTY (terminal), Headless (MCP-only), and Web Model browser/remote-MCP delivery paths

### Ledger

- Single source of truth: `~/.cccc/groups/<group_id>/ledger.jsonl`
- All messages, events, and decisions are recorded here
- Supports snapshot/compaction

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

## Architecture Layers

```
┌─────────────────────────────────────────────────────────┐
│                      Ports (Entry)                       │
│   Web UI (React)  │  CLI  │  IM Bridge  │  MCP Server   │
├─────────────────────────────────────────────────────────┤
│                    Native Daemon                         │
│   IPC Server  │  Delivery  │  Automation  │  Runners    │
│               │            │              │  Browser    │
├─────────────────────────────────────────────────────────┤
│                      Kernel                              │
│   Group  │  Actor  │  Ledger  │  Inbox  │  Permissions  │
├─────────────────────────────────────────────────────────┤
│                    Contracts (v1)                        │
│   Event  │  Message  │  Actor  │  IPC                   │
└─────────────────────────────────────────────────────────┘
```

### Contracts Layer

- Rust contract types define wire structures
- Versioned standards: `docs/standards/`; implementation: `crates/cccc-contracts/`
- Stable boundary, no business implementation

### Kernel

- Group/Scope/Ledger/Inbox/Permissions
- Depends on contracts, not on specific ports

### Daemon

- Single-writer principle: all ledger writes go through the daemon
- IPC + supervision + delivery/automation
- Manages actor lifecycle, including CLI runtimes and ChatGPT Web Model browser delivery

### Ports (Entry)

- Only interact with daemon via IPC
- Hold no business state
- Web Model remote MCP is an actor-bound web port surface; authorization still resolves through the daemon and group actor state

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
├── cccc-daemon/           # Single-writer daemon and delivery
├── cccc-runtime/          # PTY/headless provider runtimes
├── cccc-web/              # Native Web API and embedded UI
├── cccc-mcp/              # MCP server
└── cccc-cli/              # Public cccc executable
```
