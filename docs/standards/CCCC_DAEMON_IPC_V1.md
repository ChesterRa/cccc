# CCCC Daemon IPC v1

Status: implemented by the Rust workspace on the `rust` branch.

The Rust structs in `crates/cccc-contracts/src/ipc.rs` are the canonical wire definition. This document describes framing, envelopes, compatibility, and operation families.

## Transport

- Unix: Unix domain socket under `CCCC_RUST_HOME/daemon`.
- Windows: loopback TCP.
- Optional: set `CCCC_DAEMON_TRANSPORT=tcp` for loopback TCP on any platform.

The daemon writes its selected address to `daemon/ccccd.addr.json` inside Rust Home. The address document includes transport, path or host/port, PID, version, and timestamp.

Each connection carries one UTF-8 JSON request followed by `\n`. The server returns one UTF-8 JSON response followed by `\n` and closes the connection. Requests larger than 2,000,000 bytes are rejected.

## Request

```json
{
  "v": 1,
  "op": "group_show",
  "args": {"group_id": "g_example"}
}
```

Fields:

| Field | Type | Meaning |
|---|---|---|
| `v` | integer | Protocol version; defaults to 1 |
| `op` | string | Operation name |
| `args` | object | Operation arguments; defaults to `{}` |

Unknown top-level fields are rejected.

## Success Response

```json
{
  "v": 1,
  "ok": true,
  "result": {"group": {"group_id": "g_example"}}
}
```

## Error Response

```json
{
  "v": 1,
  "ok": false,
  "result": {},
  "error": {
    "code": "group_not_found",
    "message": "group not found: g_example",
    "details": {}
  }
}
```

Clients must branch on `ok`, not on message text. Error codes are stable machine-readable identifiers; `details` may add structured context.

## Core Operations

| Operation | Purpose |
|---|---|
| `ping` | Process ID, version, implementation |
| `version` | Daemon version and implementation |
| `home_get` | Rust Home path and environment variable |
| `shutdown` | Graceful daemon shutdown |

## Group And Scope Operations

```text
groups
group_create
group_show
group_update
group_delete
group_reset
group_start
group_stop
group_set_state
group_use
attach
group_detach_scope
group_settings_update
group_copy_export
group_copy_export_file
group_copy_preview_import
group_copy_import
```

Mutating operations accept `by` where an audit identity is required. Delete/reset operations are protected by their calling port's explicit confirmation flow.

## Actor And Profile Operations

```text
actor_list
actor_prompt
actor_add
actor_update
actor_remove
actor_start
actor_stop
actor_restart
actor_new_session
actor_env_private_keys
actor_env_private_update
actor_profile_list
actor_profile_get
actor_profile_upsert
actor_profile_delete
actor_profile_env_private_keys
actor_profile_env_private_update
actor_profile_copy_actor_secrets
actor_profile_copy_profile_secrets
```

Profile upsert/delete supports revision checks. Secrets are returned as keys or write results, never through public actor/profile documents.

## Messaging Operations

```text
send
tracked_send
reply
send_cross_group
send_cross_group_remote_record
stream_emit
system_notify
event_append
ledger_tail
inbox_list
inbox_mark_read
inbox_mark_all_read
chat_ack
notify_ack
slash_skill_dispatch
```

`send` requires text or attachments. `to` is an array of actor IDs or recipient tokens. Cross-group remote transport is coordinated by the Web Group Bridge session and records a local source event through `send_cross_group_remote_record`.

## Context And Memory Operations

```text
context_get
context_sync
task_list
memory_search
memory_get
memory_write
memory_health
memory_profile_get
memory_reme_layout_get
memory_reme_index_sync
memory_reme_context_check
memory_reme_compact
memory_reme_daily_flush
```

`context_sync` accepts the Context Ops batch contract. Memory writes are confined to the selected group under Rust Home.

## Runtime And Terminal Operations

```text
terminal_status
terminal_tail
terminal_history
terminal_write
terminal_resize
terminal_clear
headless_status
headless_set_status
headless_ack_message
web_model_runtime_wait_next_turn
web_model_runtime_complete_turn
```

Web Model completion with `done` or `partial` commits a contiguous unread prefix. `failed` and `cancelled` leave messages unread.

## Capability And Automation Operations

```text
capability_search
capability_state
capability_overview
capability_enable
capability_visibility
capability_import
capability_install
capability_install_target
capability_tool_call
capability_block
capability_uninstall
capability_source_delete
capability_allowlist_get
capability_allowlist_update
capability_allowlist_reset
capability_allowlist_validate
group_automation_state
group_automation_update
group_automation_manage
group_automation_reset_baseline
```

## Integration Operations

Presentation:

```text
presentation_get
presentation_publish
presentation_clear
```

Group Space:

```text
group_space_status
group_space_capabilities
group_space_bind
group_space_ingest
group_space_query
group_space_sources
group_space_artifact
group_space_jobs
group_space_sync
group_space_provider_auth
```

IM control:

```text
im_status
im_config
im_set
im_unset
im_start
im_stop
im_bind_chat
im_list_pending
im_list_authorized
im_reject_pending
im_revoke_chat
```

Voice Secretary:

```text
assistant_voice_document_list
assistant_voice_document_select
assistant_voice_document_input_read
assistant_voice_document_save
assistant_voice_document_instruction
assistant_voice_document_archive
assistant_voice_prompt_draft_submit
assistant_voice_prompt_draft_ack
assistant_voice_instruction_feedback
assistant_voice_ask_requests_clear
assistant_voice_request
```

## Administration Operations

```text
branding_get
branding_update
observability_get
observability_update
remote_access_state
remote_access_configure
remote_access_start
remote_access_stop
debug_snapshot
debug_tail_logs
debug_clear_logs
registry_reconcile
ledger_snapshot
ledger_compact
```

## Compatibility Rules

1. Existing v1 fields keep their type and meaning.
2. New optional fields may be added inside `args`, `result`, or `error.details`.
3. New operations do not require a protocol version bump.
4. A breaking envelope or framing change requires IPC v2.
5. Ports must tolerate unknown result fields.
6. The daemon rejects unknown operation names with `unknown_op`.

## Security Rules

- IPC files and addresses are created only under `CCCC_RUST_HOME`.
- `CCCC_RUST_HOME` cannot be the legacy `~/.cccc` directory or its descendant.
- Remote callers do not connect to daemon IPC directly; Web access tokens, Web Model connector credentials, or Group Bridge credentials enforce their scope first.
- File and command operations remain confined to an attached scope and their operation-specific policy.
