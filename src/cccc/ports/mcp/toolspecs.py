"""MCP tool schemas for CCCC (30-entry consolidated surface)."""

from __future__ import annotations

from ...kernel.memory import MEMORY_KINDS, MEMORY_SOURCE_TYPES

_CCCC_HELP_DESCRIPTION = (
    "Load the effective collaboration playbook for this group "
    "(role-aware, on-demand, with runtime quick-use hints). "
    "Use when workflow or capability-routing details are unclear."
)


def _obj(properties: dict, required: list[str] | None = None) -> dict:
    return {
        "type": "object",
        "properties": properties,
        "required": list(required or []),
    }


_COMMON_GROUP = {
    "group_id": {"type": "string", "description": "Working group ID (optional if CCCC_GROUP_ID is set)"},
}
_COMMON_ACTOR = {
    "actor_id": {"type": "string", "description": "Actor ID (optional if CCCC_ACTOR_ID is set)"},
}
_COMMON_BY = {
    "by": {"type": "string", "description": "Caller actor id override (normally auto-resolved)"},
}


MCP_TOOLS = [
    {
        "name": "cccc_help",
        "description": _CCCC_HELP_DESCRIPTION,
        "inputSchema": _obj({}),
    },
    {
        "name": "cccc_bootstrap",
        "description": (
            "One-call bootstrap: group + actors + help + PROJECT.md + context + inbox + optional ledger tail."
        ),
        "inputSchema": _obj(
            {
                **_COMMON_GROUP,
                **_COMMON_ACTOR,
                "inbox_limit": {
                    "type": "integer",
                    "default": 50,
                    "minimum": 1,
                    "maximum": 1000,
                },
                "inbox_kind_filter": {
                    "type": "string",
                    "enum": ["all", "chat", "notify"],
                    "default": "all",
                },
                "ledger_tail_limit": {
                    "type": "integer",
                    "default": 10,
                    "minimum": 0,
                    "maximum": 1000,
                },
                "ledger_tail_max_chars": {
                    "type": "integer",
                    "default": 8000,
                    "minimum": 0,
                    "maximum": 100000,
                },
            }
        ),
    },
    {
        "name": "cccc_project_info",
        "description": "Get PROJECT.md content for the active scope.",
        "inputSchema": _obj({**_COMMON_GROUP}),
    },
    {
        "name": "cccc_inbox_list",
        "description": "List unread inbox entries (chat/notify/all).",
        "inputSchema": _obj(
            {
                **_COMMON_GROUP,
                **_COMMON_ACTOR,
                "limit": {"type": "integer", "default": 50, "minimum": 1, "maximum": 1000},
                "kind_filter": {
                    "type": "string",
                    "enum": ["all", "chat", "notify"],
                    "default": "all",
                },
            }
        ),
    },
    {
        "name": "cccc_inbox_mark_read",
        "description": "Mark inbox as read: action=read(event_id) or action=read_all(kind_filter).",
        "inputSchema": _obj(
            {
                **_COMMON_GROUP,
                **_COMMON_ACTOR,
                "action": {
                    "type": "string",
                    "enum": ["read", "read_all"],
                    "default": "read",
                },
                "event_id": {"type": "string", "description": "Required when action=read"},
                "kind_filter": {
                    "type": "string",
                    "enum": ["all", "chat", "notify"],
                    "default": "all",
                },
            }
        ),
    },
    {
        "name": "cccc_message_send",
        "description": "Send a visible chat message.",
        "inputSchema": _obj(
            {
                **_COMMON_GROUP,
                **_COMMON_ACTOR,
                "dst_group_id": {"type": "string"},
                "text": {"type": "string"},
                "to": {
                    "anyOf": [
                        {"type": "string"},
                        {"type": "array", "items": {"type": "string"}},
                    ]
                },
                "priority": {"type": "string", "enum": ["low", "normal", "high", "urgent"], "default": "normal"},
                "reply_required": {"type": "boolean", "default": False},
            },
            required=["text"],
        ),
    },
    {
        "name": "cccc_message_reply",
        "description": "Reply to a visible chat message (by event_id/reply_to).",
        "inputSchema": _obj(
            {
                **_COMMON_GROUP,
                **_COMMON_ACTOR,
                "event_id": {"type": "string", "description": "Reply target event id"},
                "reply_to": {"type": "string", "description": "Alias of event_id"},
                "text": {"type": "string"},
                "to": {
                    "anyOf": [
                        {"type": "string"},
                        {"type": "array", "items": {"type": "string"}},
                    ]
                },
                "priority": {"type": "string", "enum": ["low", "normal", "high", "urgent"], "default": "normal"},
                "reply_required": {"type": "boolean", "default": False},
            },
            required=["text"],
        ),
    },
    {
        "name": "cccc_file",
        "description": "File operations: action=send(path,text,...) or action=blob_path(rel_path).",
        "inputSchema": _obj(
            {
                **_COMMON_GROUP,
                **_COMMON_ACTOR,
                "action": {"type": "string", "enum": ["send", "blob_path"], "default": "send"},
                "path": {"type": "string", "description": "Required for action=send"},
                "text": {"type": "string"},
                "to": {
                    "anyOf": [
                        {"type": "string"},
                        {"type": "array", "items": {"type": "string"}},
                    ]
                },
                "priority": {"type": "string", "enum": ["low", "normal", "high", "urgent"], "default": "normal"},
                "reply_required": {"type": "boolean", "default": False},
                "rel_path": {"type": "string", "description": "Required for action=blob_path"},
            }
        ),
    },
    {
        "name": "cccc_group",
        "description": "Group operations: action=info|list|set_state.",
        "inputSchema": _obj(
            {
                **_COMMON_GROUP,
                **_COMMON_ACTOR,
                "action": {"type": "string", "enum": ["info", "list", "set_state"], "default": "info"},
                "state": {
                    "type": "string",
                    "enum": ["active", "idle", "paused", "stopped"],
                    "description": "Required when action=set_state",
                },
            }
        ),
    },
    {
        "name": "cccc_actor",
        "description": "Actor operations: list/profile_list/add/remove/start/stop/restart.",
        "inputSchema": _obj(
            {
                **_COMMON_GROUP,
                **_COMMON_BY,
                "action": {
                    "type": "string",
                    "enum": ["list", "profile_list", "add", "remove", "start", "stop", "restart"],
                    "default": "list",
                },
                "actor_id": {"type": "string"},
                "runtime": {"type": "string", "default": "codex"},
                "runner": {"type": "string", "default": "pty"},
                "title": {"type": "string"},
                "command": {"type": "array", "items": {"type": "string"}},
                "env": {"type": "object", "additionalProperties": {"type": "string"}},
                "capability_autoload": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Actor startup autoload capability ids (applies on actor start/restart).",
                },
                "profile_id": {"type": "string"},
            }
        ),
    },
    {
        "name": "cccc_runtime_list",
        "description": "List available runtimes and runtime pool configuration.",
        "inputSchema": _obj({}),
    },
    {
        "name": "cccc_capability_search",
        "description": "Search capability registry (built-in + external sources).",
        "inputSchema": _obj(
            {
                **_COMMON_GROUP,
                **_COMMON_ACTOR,
                "query": {"type": "string", "default": ""},
                "kind": {"type": "string", "default": ""},
                "source_id": {"type": "string", "default": ""},
                "trust_tier": {"type": "string", "default": ""},
                "qualification_status": {"type": "string", "default": ""},
                "limit": {"type": "integer", "default": 30, "minimum": 1, "maximum": 200},
                "include_external": {"type": "boolean", "default": True},
            }
        ),
    },
    {
        "name": "cccc_capability_enable",
        "description": "Enable/disable a capability for session/actor/group scope.",
        "inputSchema": _obj(
            {
                **_COMMON_GROUP,
                **_COMMON_BY,
                "actor_id": {"type": "string"},
                "capability_id": {"type": "string"},
                "scope": {"type": "string", "enum": ["session", "actor", "group"], "default": "session"},
                "enabled": {"type": "boolean", "default": True},
                "cleanup": {"type": "boolean", "default": False},
                "reason": {"type": "string", "default": ""},
                "ttl_seconds": {"type": "integer", "default": 3600, "minimum": 60, "maximum": 86400},
            },
            required=["capability_id"],
        ),
    },
    {
        "name": "cccc_capability_block",
        "description": "Block/unblock a capability at group/global scope (foreman can mutate group scope).",
        "inputSchema": _obj(
            {
                **_COMMON_GROUP,
                **_COMMON_BY,
                "actor_id": {"type": "string"},
                "capability_id": {"type": "string"},
                "scope": {"type": "string", "enum": ["group", "global"], "default": "group"},
                "blocked": {"type": "boolean", "default": True},
                "ttl_seconds": {"type": "integer", "default": 0, "minimum": 0, "maximum": 2592000},
                "reason": {"type": "string", "default": ""},
            },
            required=["capability_id"],
        ),
    },
    {
        "name": "cccc_capability_state",
        "description": "Get caller-effective capability state and visible/dynamic tools.",
        "inputSchema": _obj({**_COMMON_GROUP, **_COMMON_ACTOR}),
    },
    {
        "name": "cccc_capability_uninstall",
        "description": "Uninstall external capability runtime cache and revoke bindings.",
        "inputSchema": _obj(
            {
                **_COMMON_GROUP,
                **_COMMON_BY,
                "capability_id": {"type": "string"},
                "reason": {"type": "string", "default": ""},
            },
            required=["capability_id"],
        ),
    },
    {
        "name": "cccc_capability_use",
        "description": (
            "One-step capability use: enable capability and optionally call a target tool. "
            "For skill:* capabilities this is runtime capsule activation (not full local skill package install). "
            "If enable is not ready, inspect diagnostics/resolution_plan and resolve blockers before retry."
        ),
        "inputSchema": _obj(
            {
                **_COMMON_GROUP,
                **_COMMON_BY,
                "actor_id": {"type": "string"},
                "capability_id": {"type": "string", "default": ""},
                "tool_name": {"type": "string", "default": ""},
                "tool_arguments": {"type": "object", "default": {}},
                "scope": {"type": "string", "enum": ["session", "actor", "group"], "default": "session"},
                "ttl_seconds": {"type": "integer", "default": 3600, "minimum": 60, "maximum": 86400},
                "reason": {"type": "string", "default": ""},
            }
        ),
    },
    {
        "name": "cccc_space",
        "description": (
            "Group Space hub tool. action: status|capabilities|bind|ingest|query|sources|artifact|jobs|sync|"
            "provider_auth|provider_credential_status|provider_credential_update"
        ),
        "inputSchema": _obj(
            {
                **_COMMON_GROUP,
                **_COMMON_BY,
                "provider": {"type": "string", "default": "notebooklm"},
                "action": {
                    "type": "string",
                    "enum": [
                        "status",
                        "capabilities",
                        "bind",
                        "ingest",
                        "query",
                        "sources",
                        "artifact",
                        "jobs",
                        "sync",
                        "provider_auth",
                        "provider_credential_status",
                        "provider_credential_update",
                    ],
                    "default": "status",
                },
                "sub_action": {
                    "type": "string",
                    "description": "Optional secondary action for sources/jobs/sync/provider_auth/artifact branches.",
                },
                "remote_space_id": {"type": "string"},
                "kind": {"type": "string"},
                "payload": {"type": "object"},
                "idempotency_key": {"type": "string"},
                "query": {"type": "string"},
                "options": {
                    "type": "object",
                    "properties": {
                        "source_ids": {"type": "array", "items": {"type": "string"}},
                    },
                    "required": [],
                },
                "source": {"type": "string"},
                "language": {"type": "string"},
                "source_id": {"type": "string"},
                "new_title": {"type": "string"},
                "job_id": {"type": "string"},
                "state": {"type": "string"},
                "limit": {"type": "integer", "minimum": 1, "maximum": 500},
                "force": {"type": "boolean", "default": False},
                "wait": {"type": "boolean", "default": False},
                "save_to_space": {"type": "boolean", "default": True},
                "output_path": {"type": "string"},
                "output_format": {"type": "string"},
                "artifact_id": {"type": "string"},
                "timeout_seconds": {"type": "integer"},
                "initial_interval": {"type": "number"},
                "max_interval": {"type": "number"},
                "auth_json": {"type": "string"},
                "clear": {"type": "boolean", "default": False},
            }
        ),
    },
    {
        "name": "cccc_automation",
        "description": "Automation hub tool: action=state|manage.",
        "inputSchema": _obj(
            {
                **_COMMON_GROUP,
                **_COMMON_BY,
                "action": {"type": "string", "enum": ["state", "manage"], "default": "state"},
                "op": {"type": "string", "description": "Simple mode op for manage"},
                "actions": {
                    "type": "array",
                    "items": {"type": "object"},
                    "description": "Advanced manage actions",
                },
                "expected_version": {"type": "integer"},
            }
        ),
    },
    {
        "name": "cccc_context_get",
        "description": "Get context snapshot (vision/overview/tasks/agents).",
        "inputSchema": _obj(
            {
                **_COMMON_GROUP,
                "include_archived": {"type": "boolean", "default": False},
            }
        ),
    },
    {
        "name": "cccc_context_sync",
        "description": "Batch context operations (atomic).",
        "inputSchema": _obj(
            {
                **_COMMON_GROUP,
                **_COMMON_ACTOR,
                "ops": {"type": "array", "items": {"type": "object"}},
                "dry_run": {"type": "boolean", "default": False},
                "if_version": {"type": "string"},
            },
            required=["ops"],
        ),
    },
    {
        "name": "cccc_context_admin",
        "description": "Context admin ops: action=vision_update|overview_update.",
        "inputSchema": _obj(
            {
                **_COMMON_GROUP,
                **_COMMON_ACTOR,
                "action": {
                    "type": "string",
                    "enum": ["vision_update", "overview_update"],
                    "default": "vision_update",
                },
                "vision": {"type": "string"},
                "roles": {"type": "array", "items": {"type": "string"}},
                "collaboration_mode": {"type": "string"},
                "current_focus": {"type": "string"},
            }
        ),
    },
    {
        "name": "cccc_task",
        "description": "Shared collaboration task hub (not runtime todo): action=list|create|update|status|move|restore. Use for multi-actor/long-horizon/user-tracked work.",
        "inputSchema": _obj(
            {
                **_COMMON_GROUP,
                **_COMMON_ACTOR,
                "action": {
                    "type": "string",
                    "enum": ["list", "create", "update", "status", "move", "restore"],
                    "default": "list",
                },
                "task_id": {"type": "string"},
                "include_archived": {"type": "boolean", "default": False},
                "name": {"type": "string"},
                "goal": {"type": "string"},
                "steps": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "id": {"type": "string"},
                            "text": {"type": "string"},
                            "status": {"type": "string"},
                        },
                        "required": [],
                    },
                },
                "parent_id": {"type": "string"},
                "assignee": {"type": "string"},
                "step_id": {"type": "string"},
                "step_status": {"type": "string"},
                "status": {"type": "string"},
                "new_parent_id": {"type": "string"},
            }
        ),
    },
    {
        "name": "cccc_context_agent",
        "description": "Per-agent short-term execution state: action=update|clear. Keep focus/next_action/what_changed fresh; when evidence/scope changes, update or restructure immediately (use pending_confirm in text fields when objective is unclear). Use blockers for execution impediments (task UI 'Blocked' projection source).",
        "inputSchema": _obj(
            {
                **_COMMON_GROUP,
                **_COMMON_ACTOR,
                "action": {"type": "string", "enum": ["update", "clear"], "default": "update"},
                "agent_id": {"type": "string"},
                "active_task_id": {"type": "string"},
                "focus": {"type": "string"},
                "blockers": {"type": "array", "items": {"type": "string"}},
                "next_action": {"type": "string"},
                "what_changed": {"type": "string"},
                "decision_delta": {"type": "string"},
                "environment": {"type": "string"},
                "user_profile": {"type": "string"},
                "notes": {"type": "string"},
            },
            required=["agent_id"],
        ),
    },
    {
        "name": "cccc_memory",
        "description": "Memory primary ops: action=guide|store|search|stats.",
        "inputSchema": _obj(
            {
                **_COMMON_GROUP,
                "action": {"type": "string", "enum": ["guide", "store", "search", "stats"], "default": "search"},
                "topic": {"type": "string", "enum": ["store", "search", "consolidation", "lifecycle"]},
                "id": {"type": "string"},
                "content": {"type": "string"},
                "kind": {"type": "string", "enum": list(MEMORY_KINDS)},
                "status": {"type": "string"},
                "confidence": {"type": "string"},
                "source_type": {"type": "string", "enum": list(MEMORY_SOURCE_TYPES)},
                "source_ref": {"type": "string"},
                "scope_key": {"type": "string"},
                "actor_id": {"type": "string"},
                "task_id": {"type": "string"},
                "event_ts": {"type": "string"},
                "strategy": {"type": "string"},
                "tags": {"type": "array", "items": {"type": "string"}},
                "solidify": {"type": "boolean", "default": False},
                "query": {"type": "string"},
                "since": {"type": "string"},
                "until": {"type": "string"},
                "limit": {"type": "integer", "minimum": 1, "maximum": 500},
                "track_hit": {"type": "boolean", "default": False},
            }
        ),
    },
    {
        "name": "cccc_memory_admin",
        "description": "Memory admin ops: action=ingest|export|delete|decay.",
        "inputSchema": _obj(
            {
                **_COMMON_GROUP,
                "action": {
                    "type": "string",
                    "enum": ["ingest", "export", "delete", "decay"],
                    "default": "ingest",
                },
                "mode": {"type": "string"},
                "limit": {"type": "integer", "minimum": 1, "maximum": 2000},
                "actor_id": {"type": "string"},
                "reset_watermark": {"type": "boolean", "default": False},
                "include_draft": {"type": "boolean", "default": False},
                "output_dir": {"type": "string"},
                "id": {"type": "string"},
                "ids": {"type": "array", "items": {"type": "string"}},
                "draft_days": {"type": "integer"},
                "zero_hit_days": {"type": "integer"},
                "solid_review_days": {"type": "integer"},
                "solid_max_hit": {"type": "integer"},
            }
        ),
    },
    {
        "name": "cccc_headless",
        "description": "Headless runner control: action=status|set_status|ack_message.",
        "inputSchema": _obj(
            {
                **_COMMON_GROUP,
                **_COMMON_ACTOR,
                "action": {
                    "type": "string",
                    "enum": ["status", "set_status", "ack_message"],
                    "default": "status",
                },
                "status": {"type": "string", "enum": ["idle", "working", "waiting", "stopped"]},
                "task_id": {"type": "string"},
                "message_id": {"type": "string"},
            }
        ),
    },
    {
        "name": "cccc_notify",
        "description": "System notifications: action=send|ack.",
        "inputSchema": _obj(
            {
                **_COMMON_GROUP,
                **_COMMON_ACTOR,
                "action": {"type": "string", "enum": ["send", "ack"], "default": "send"},
                "kind": {"type": "string", "default": "info"},
                "title": {"type": "string"},
                "message": {"type": "string"},
                "target_actor_id": {"type": "string"},
                "priority": {"type": "string", "enum": ["low", "normal", "high", "urgent"], "default": "normal"},
                "requires_ack": {"type": "boolean", "default": False},
                "notify_event_id": {"type": "string"},
            }
        ),
    },
    {
        "name": "cccc_terminal",
        "description": "Terminal diagnostics: action=tail.",
        "inputSchema": _obj(
            {
                **_COMMON_GROUP,
                **_COMMON_ACTOR,
                "action": {"type": "string", "enum": ["tail"], "default": "tail"},
                "target_actor_id": {"type": "string"},
                "max_chars": {"type": "integer", "default": 8000, "minimum": 1, "maximum": 100000},
                "strip_ansi": {"type": "boolean", "default": True},
            },
            required=["target_actor_id"],
        ),
    },
    {
        "name": "cccc_debug",
        "description": "Developer diagnostics: action=snapshot|tail_logs.",
        "inputSchema": _obj(
            {
                **_COMMON_GROUP,
                **_COMMON_ACTOR,
                "action": {"type": "string", "enum": ["snapshot", "tail_logs"], "default": "snapshot"},
                "component": {"type": "string"},
                "lines": {"type": "integer", "default": 200, "minimum": 1, "maximum": 10000},
            }
        ),
    },
    {
        "name": "cccc_im_bind",
        "description": "Bind IM integration key for current group.",
        "inputSchema": _obj(
            {
                **_COMMON_GROUP,
                "key": {"type": "string"},
            },
            required=["key"],
        ),
    },
]
