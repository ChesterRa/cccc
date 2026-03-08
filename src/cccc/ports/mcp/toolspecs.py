"""MCP tool schemas for CCCC consolidated surface."""

from __future__ import annotations

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
            "Cold-start bootstrap: session + recovery + inbox_preview + memory_recall_gate + next_calls. "
            "Use cccc_help / cccc_project_info / cccc_context_get on demand for cold detail."
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
        "description": "Actor operations: list/profile_list/add/remove/start/stop/restart. Standard actor creation uses PTY only.",
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
        "name": "cccc_capability_import",
        "description": (
            "Import an agent-prepared normalized capability record (mcp_toolpack or skill) from any external source. "
            "Daemon performs validation/probe/persist and can optionally enable after import. "
            "record.source_id is optional; empty/unknown values are normalized to manual_import. "
            "Dry runs return readiness_preview; external capability actionability follows external capability safety mode."
        ),
        "inputSchema": _obj(
            {
                **_COMMON_GROUP,
                **_COMMON_BY,
                "actor_id": {"type": "string"},
                "record": _obj(
                    {
                        "capability_id": {"type": "string", "description": "mcp:* or skill:*"},
                        "kind": {"type": "string", "enum": ["mcp_toolpack", "skill"]},
                        "name": {"type": "string"},
                        "description_short": {"type": "string"},
                        "source_id": {
                            "type": "string",
                            "description": "Optional source id; empty/unknown values are normalized to manual_import.",
                        },
                        "source_uri": {"type": "string"},
                        "source_record_id": {"type": "string"},
                        "source_record_version": {"type": "string"},
                        "updated_at_source": {"type": "string"},
                        "trust_tier": {"type": "string"},
                        "source_tier": {"type": "string"},
                        "qualification_status": {"type": "string", "enum": ["qualified", "unavailable", "blocked"]},
                        "qualification_reasons": {"type": "array", "items": {"type": "string"}},
                        "tags": {"type": "array", "items": {"type": "string"}},
                        "license": {"type": "string"},
                        "install_mode": {
                            "type": "string",
                            "enum": ["remote_only", "package", "command"],
                            "description": "Required for mcp_toolpack imports",
                        },
                        "install_spec": {
                            "type": "object",
                            "description": "Required for mcp_toolpack imports",
                        },
                        "command": {
                            "anyOf": [
                                {"type": "string"},
                                {"type": "array", "items": {"type": "string"}},
                            ],
                            "description": "Command mode shortcut; alternatively provide install_spec.command",
                        },
                        "command_candidates": {
                            "type": "array",
                            "items": {
                                "anyOf": [
                                    {"type": "string"},
                                    {"type": "array", "items": {"type": "string"}},
                                ]
                            },
                            "description": "Optional command candidates for command mode/fallback",
                        },
                        "fallback_command": {
                            "anyOf": [
                                {"type": "string"},
                                {"type": "array", "items": {"type": "string"}},
                            ],
                            "description": "Optional package->command fallback command (top-level shortcut or install_spec.fallback_command).",
                        },
                        "fallback_command_candidates": {
                            "type": "array",
                            "items": {
                                "anyOf": [
                                    {"type": "string"},
                                    {"type": "array", "items": {"type": "string"}},
                                ]
                            },
                            "description": "Optional package->command fallback candidates (top-level shortcut or install_spec.fallback_command_candidates).",
                        },
                        "capsule_text": {
                            "type": "string",
                            "description": "Required for skill imports",
                        },
                        "requires_capabilities": {"type": "array", "items": {"type": "string"}},
                    },
                    required=["capability_id", "kind"],
                ),
                "dry_run": {"type": "boolean", "default": False},
                "probe": {"type": "boolean", "default": True},
                "enable_after_import": {"type": "boolean", "default": False},
                "scope": {"type": "string", "enum": ["session", "actor", "group"], "default": "session"},
                "ttl_seconds": {"type": "integer", "default": 3600, "minimum": 60, "maximum": 86400},
                "reason": {"type": "string", "default": ""},
            },
            required=["record"],
        ),
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
            "If enable returns activation_pending, relist/reconnect before claiming success; inspect diagnostics/resolution_plan for blockers. "
            "For skill:* capsule runtime, success is primarily visible in capability_state.active_capsule_skills, not necessarily in dynamic_tools."
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
        "description": "Get the context control-plane snapshot (coordination, agent states, attention, board, panorama projection).",
        "inputSchema": _obj(
            {
                **_COMMON_GROUP,
                "include_archived": {"type": "boolean", "default": False},
            }
        ),
    },
    {
        "name": "cccc_context_sync",
        "description": "Advanced atomic batch sync for context ops. Prefer higher-level coordination/task/agent_state tools unless you need one-shot multi-op writes.",
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
        "name": "cccc_coordination",
        "description": "Shared control-plane tool: action=get|update_brief|add_decision|add_handoff.",
        "inputSchema": _obj(
            {
                **_COMMON_GROUP,
                **_COMMON_ACTOR,
                "action": {
                    "type": "string",
                    "enum": ["get", "update_brief", "add_decision", "add_handoff"],
                    "default": "get",
                },
                "include_archived": {"type": "boolean", "default": False},
                "objective": {"type": "string"},
                "current_focus": {"type": "string"},
                "constraints": {"type": "array", "items": {"type": "string"}},
                "project_brief": {"type": "string"},
                "project_brief_stale": {"type": "boolean"},
                "summary": {"type": "string"},
                "task_id": {"type": "string"},
            }
        ),
    },
    {
        "name": "cccc_task",
        "description": "Shared collaboration task hub (not runtime todo): action=list|create|update|move|restore. Use for multi-actor, long-horizon, or user-tracked work.",
        "inputSchema": _obj(
            {
                **_COMMON_GROUP,
                **_COMMON_ACTOR,
                "action": {
                    "type": "string",
                    "enum": ["list", "create", "update", "move", "restore"],
                    "default": "list",
                },
                "task_id": {"type": "string"},
                "include_archived": {"type": "boolean", "default": False},
                "title": {"type": "string"},
                "outcome": {"type": "string"},
                "status": {"type": "string", "enum": ["planned", "active", "done", "archived"]},
                "parent_id": {"type": "string"},
                "assignee": {"type": "string"},
                "priority": {"type": "string"},
                "blocked_by": {"type": "array", "items": {"type": "string"}},
                "waiting_on": {"type": "string", "enum": ["none", "user", "actor", "external"]},
                "handoff_to": {"type": "string"},
                "notes": {"type": "string"},
                "checklist": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "id": {"type": "string"},
                            "text": {"type": "string"},
                            "status": {"type": "string", "enum": ["pending", "in_progress", "done"]},
                        },
                        "required": ["text"],
                    },
                },
            }
        ),
    },
    {
        "name": "cccc_agent_state",
        "description": "Per-actor working-memory tool: action=get|update|clear. Keep hot fields fresh; use warm fields only when they improve recovery, recall, or signal quality.",
        "inputSchema": _obj(
            {
                **_COMMON_GROUP,
                **_COMMON_ACTOR,
                "action": {"type": "string", "enum": ["get", "update", "clear"], "default": "get"},
                "actor_id": {"type": "string"},
                "include_warm": {"type": "boolean", "default": True},
                "active_task_id": {"type": "string"},
                "focus": {"type": "string"},
                "blockers": {"type": "array", "items": {"type": "string"}},
                "next_action": {"type": "string"},
                "what_changed": {"type": "string"},
                "open_loops": {"type": "array", "items": {"type": "string"}},
                "commitments": {"type": "array", "items": {"type": "string"}},
                "environment_summary": {"type": "string"},
                "user_model": {"type": "string"},
                "persona_notes": {"type": "string"},
                "resume_hint": {"type": "string"},
            }
        ),
    },
    {
        "name": "cccc_memory",
        "description": "ReMe file-memory primary ops: action=layout_get|search|get|write.",
        "inputSchema": _obj(
            {
                **_COMMON_GROUP,
                "action": {"type": "string", "enum": ["layout_get", "search", "get", "write"], "default": "search"},
                "query": {"type": "string"},
                "max_results": {"type": "integer", "minimum": 1, "maximum": 50, "default": 5},
                "min_score": {"type": "number", "minimum": 0, "maximum": 1, "default": 0.1},
                "sources": {"type": "array", "items": {"type": "string"}},
                "vector_weight": {"type": "number", "minimum": 0, "maximum": 1},
                "candidate_multiplier": {"type": "number", "minimum": 1, "maximum": 20},
                "path": {"type": "string"},
                "offset": {"type": "integer", "minimum": 1},
                "limit": {"type": "integer", "minimum": 1, "maximum": 5000},
                "target": {"type": "string", "enum": ["memory", "daily"]},
                "content": {"type": "string"},
                "date": {"type": "string", "description": "YYYY-MM-DD (required when target=daily)"},
                "mode": {"type": "string", "enum": ["append", "replace"], "default": "append"},
                "idempotency_key": {"type": "string"},
                "actor_id": {"type": "string"},
                "source_refs": {"type": "array", "items": {"type": "string"}},
                "tags": {"type": "array", "items": {"type": "string"}},
                "supersedes": {"type": "array", "items": {"type": "string"}},
                "dedup_intent": {"type": "string", "enum": ["new", "update", "supersede", "silent"], "default": "new"},
                "dedup_query": {"type": "string"},
            }
        ),
    },
    {
        "name": "cccc_memory_admin",
        "description": "ReMe file-memory admin ops: action=index_sync|context_check|compact|daily_flush.",
        "inputSchema": _obj(
            {
                **_COMMON_GROUP,
                "action": {
                    "type": "string",
                    "enum": ["index_sync", "context_check", "compact", "daily_flush"],
                    "default": "index_sync",
                },
                "mode": {"type": "string", "enum": ["scan", "rebuild"], "default": "scan"},
                "messages": {"type": "array", "items": {"type": "object"}},
                "messages_to_summarize": {"type": "array", "items": {"type": "object"}},
                "turn_prefix_messages": {"type": "array", "items": {"type": "object"}},
                "previous_summary": {"type": "string"},
                "context_window_tokens": {"type": "integer", "minimum": 1024},
                "reserve_tokens": {"type": "integer", "minimum": 0},
                "keep_recent_tokens": {"type": "integer", "minimum": 256},
                "return_prompt": {"type": "boolean", "default": False},
                "date": {"type": "string", "description": "YYYY-MM-DD"},
                "version": {"type": "string", "default": "default"},
                "language": {"type": "string", "default": "en"},
                "actor_id": {"type": "string"},
                "signal_pack": {"type": "object"},
                "signal_pack_token_budget": {"type": "integer", "minimum": 64, "maximum": 4096, "default": 320},
                "dedup_intent": {"type": "string", "enum": ["new", "update", "supersede", "silent"], "default": "new"},
                "dedup_query": {"type": "string"},
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
