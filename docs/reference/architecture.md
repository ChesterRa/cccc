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
    ├── context/                 # Context (vision/sketch/tasks)
    └── state/                   # Runtime state
        └── blobs/               # Large text/attachments (referenced in ledger)
```

## Architecture Layers

```
┌─────────────────────────────────────────────────────────┐
│                      Ports (Entry)                       │
│   Web UI (React)  │  CLI  │  IM Bridge  │  MCP Server   │
├─────────────────────────────────────────────────────────┤
│                    Daemon (ccccd)                        │
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

- Pydantic models define all data structures
- Versioned: `src/cccc/contracts/v1/`
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
| `actor.remove` | Remove an actor |
| `actor.activity` | Runtime activity/status snapshot |
| `context.sync` | Context/control-plane sync event |
| `chat.message` | Chat message |
| `chat.stream` | Progressive stream chunk/update |
| `chat.read` / `chat.ack` | Read and acknowledgement events |
| `chat.reaction` | Chat reaction |
| `system.notify` / `system.notify_ack` | System notifications and acknowledgement |
| `assistant.settings_update` | Update built-in assistant settings |
| `assistant.status_update` | Update built-in assistant lifecycle/health |
| `assistant.voice.document` | Voice Secretary working document save/update/archive marker |
| `assistant.voice.input` | Voice Secretary transcript/input ingestion marker |
| `assistant.voice.prompt_draft` | Voice Secretary composer prompt draft submit/ack marker |
| `assistant.voice.request` | Voice Secretary structured action request marker |
| `assistant.voice.session` | Voice Secretary recording session status/artifact marker |
| `presentation.publish` | Publish a presentation rail card |
| `presentation.clear` | Clear presentation rail card(s) |

### chat.message Data

```python
class ChatMessageData:
    text: str
    format: "plain" | "markdown"
    to: list[str]           # Recipients (empty = broadcast)
    reply_to: str | None    # Reply to which message
    quote_text: str | None  # Quoted text
    attachments: list[dict] # Attachment metadata (content stored in CCCC_HOME blobs)
```

### Recipient Semantics (to field)

| Token | Semantics |
|-------|-----------|
| `[]` (empty) | Broadcast |
| `user` | The user |
| `@all` | All actors |
| `@peers` | All peers |
| `@foreman` | Foreman |
| `<actor_id>` | Specific actor |

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
| actor_remove | ✓ | ✓ (self) | ✓ (self) |

## MCP Server

MCP is exposed as an action-oriented surface. Tool count is intentionally not hardcoded, because optional capability packs can add more tools when enabled.

The surface is best understood as capability groups instead of a fixed namespace/tool count. Each group can expose one or more MCP tools, and some groups use action-style wrappers rather than one-tool-per-operation naming.

### Core Collaboration Capability Groups

- Session and guidance: `cccc_bootstrap`, `cccc_help`, `cccc_project_info`
- Messaging and files: `cccc_inbox_list`, `cccc_inbox_mark_read`, `cccc_message_send`, `cccc_message_reply`, `cccc_file`
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
| Kernel/Daemon | Python + Pydantic |
| Web Port | FastAPI + Uvicorn |
| Web UI | React + TypeScript + Vite + Tailwind + xterm.js |
| MCP | stdio mode, JSON-RPC |

## Source Structure

```
src/cccc/
├── contracts/v1/          # Contracts layer
├── kernel/                # Kernel
├── daemon/                # Daemon process
├── runners/               # PTY/Headless runner
├── ports/
│   ├── web/              # Web port
│   ├── im/               # IM Bridge
│   └── mcp/              # MCP Server
└── resources/            # Built-in resources
```
