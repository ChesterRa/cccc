"""MCP tool schemas for CCCC."""

from __future__ import annotations

_CCCC_HELP_DESCRIPTION = (
    "CCCC Help (authoritative).\n\n"
    "Contract (non-negotiable):\n"
    "- No fabrication. Investigate first (artifacts/data/logs; web search if available/allowed).\n"
    "- If you claim done/fixed/verified, include what you checked; otherwise say not verified.\n"
    "- Visible chat MUST use MCP: cccc_message_send / cccc_message_reply (terminal output is not delivered).\n"
    "- Keep shared memory in Context; keep the inbox clean (mark read only after handling).\n"
    "- If you receive a system reminder to run cccc_help, do it.\n\n"
    "Returns the effective collaboration playbook for the current group (group override under CCCC_HOME if present)."
)

MCP_TOOLS = [
    # cccc.* namespace - collaboration
    {
        "name": "cccc_help",
        "description": _CCCC_HELP_DESCRIPTION,
        "inputSchema": {"type": "object", "properties": {}, "required": []},
    },
    {
        "name": "cccc_bootstrap",
        "description": (
            "One-call session bootstrap. Use at session start/restart to reduce tool calls.\n\n"
            "Returns: group info + actor list + effective CCCC help playbook + PROJECT.md + context + your unread inbox.\n"
            "Optionally includes a small recent chat.message tail from the ledger (useful after restarts).\n"
            "Also returns suggested_mark_read_event_id (use only after reviewing inbox; pass to cccc_inbox_mark_read)."
        ),
        "inputSchema": {
            "type": "object",
            "properties": {
                "group_id": {
                    "type": "string",
                    "description": "Working group ID (optional if CCCC_GROUP_ID is set)",
                },
                "actor_id": {
                    "type": "string",
                    "description": "Your actor ID (optional if CCCC_ACTOR_ID is set)",
                },
                "inbox_limit": {
                    "type": "integer",
                    "description": "Max unread messages to return (default 50)",
                    "default": 50,
                    "minimum": 1,
                    "maximum": 1000,
                },
                "inbox_kind_filter": {
                    "type": "string",
                    "enum": ["all", "chat", "notify"],
                    "description": "Unread message filter",
                    "default": "all",
                },
                "ledger_tail_limit": {
                    "type": "integer",
                    "description": "Number of recent chat messages to include from the ledger (default 10; 0=disable)",
                    "default": 10,
                    "minimum": 0,
                    "maximum": 1000,
                },
                "ledger_tail_max_chars": {
                    "type": "integer",
                    "description": "Max total characters across returned ledger_tail[].text (default 8000)",
                    "default": 8000,
                    "minimum": 0,
                    "maximum": 100000,
                },
            },
            "required": [],
        },
    },
    {
        "name": "cccc_inbox_list",
        "description": "Get your unread messages. Returns messages in chronological order. Supports filtering by type.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "group_id": {
                    "type": "string",
                    "description": "Working group ID (optional if CCCC_GROUP_ID is set)",
                },
                "actor_id": {
                    "type": "string",
                    "description": "Your actor ID (optional if CCCC_ACTOR_ID is set)",
                },
                "limit": {
                    "type": "integer",
                    "description": "Max messages to return (default 50)",
                    "default": 50,
                    "minimum": 1,
                    "maximum": 1000,
                },
                "kind_filter": {
                    "type": "string",
                    "enum": ["all", "chat", "notify"],
                    "description": "Filter by type: all=everything, chat=messages only, notify=system notifications only",
                    "default": "all",
                },
            },
            "required": [],
        },
    },
    {
        "name": "cccc_inbox_mark_read",
        "description": "Mark messages as read up to specified event (inclusive). Call after processing messages.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "group_id": {
                    "type": "string",
                    "description": "Working group ID (optional if CCCC_GROUP_ID is set)",
                },
                "actor_id": {
                    "type": "string",
                    "description": "Your actor ID (optional if CCCC_ACTOR_ID is set)",
                },
                "event_id": {
                    "type": "string",
                    "description": "Event ID to mark as read up to",
                },
            },
            "required": ["event_id"],
        },
    },
    {
        "name": "cccc_inbox_mark_all_read",
        "description": (
            "Bulk-ack: mark all currently-unread messages as read (safe: only up to current latest unread).\n"
            "Does not mean the messages were processed. Consider reviewing via cccc_inbox_list first."
        ),
        "inputSchema": {
            "type": "object",
            "properties": {
                "group_id": {
                    "type": "string",
                    "description": "Working group ID (optional if CCCC_GROUP_ID is set)",
                },
                "actor_id": {
                    "type": "string",
                    "description": "Your actor ID (optional if CCCC_ACTOR_ID is set)",
                },
                "kind_filter": {
                    "type": "string",
                    "enum": ["all", "chat", "notify"],
                    "description": "Unread filter to clear (default all)",
                    "default": "all",
                },
            },
            "required": [],
        },
    },
    {
        "name": "cccc_message_send",
        "description": "Send a chat message (the only visible communication channel; terminal output is not delivered).",
        "inputSchema": {
            "type": "object",
            "properties": {
                "group_id": {
                    "type": "string",
                    "description": "Working group ID (optional if CCCC_GROUP_ID is set)",
                },
                "dst_group_id": {
                    "type": "string",
                    "description": "Optional destination group ID. If set and different from group_id, CCCC will send cross-group with provenance.",
                },
                "actor_id": {
                    "type": "string",
                    "description": "Your actor ID (sender, optional if CCCC_ACTOR_ID is set)",
                },
                "text": {"type": "string", "description": "Message content"},
                "to": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Recipients. Options: user, @all, @peers, @foreman, or specific actor_id. Empty=broadcast. If dst_group_id is set, this targets the destination group.",
                },
                "priority": {
                    "type": "string",
                    "enum": ["normal", "attention"],
                    "description": "Message priority (default normal). Use attention when the message is materially important.",
                },
                "reply_required": {
                    "type": "boolean",
                    "description": "Whether recipients must reply to this message (default false). Use for concrete action/result requests.",
                    "default": False,
                },
            },
            "required": ["text"],
        },
    },
    {
        "name": "cccc_message_reply",
        "description": "Reply to a message via chat (the only visible communication channel). Automatically quotes the original message.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "group_id": {
                    "type": "string",
                    "description": "Working group ID (optional if CCCC_GROUP_ID is set)",
                },
                "actor_id": {
                    "type": "string",
                    "description": "Your actor ID (sender, optional if CCCC_ACTOR_ID is set)",
                },
                "event_id": {
                    "type": "string",
                    "description": "Event ID of message to reply to",
                },
                "reply_to": {
                    "type": "string",
                    "description": "Deprecated alias for event_id",
                },
                "text": {"type": "string", "description": "Reply content"},
                "to": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Recipients (optional, defaults to original sender)",
                },
                "priority": {
                    "type": "string",
                    "enum": ["normal", "attention"],
                    "description": "Message priority (default normal). Use attention when the message is materially important.",
                },
                "reply_required": {
                    "type": "boolean",
                    "description": "Whether recipients must reply to this message (default false). Use for concrete action/result requests.",
                    "default": False,
                },
            },
            "required": ["event_id", "text"],
        },
    },
    {
        "name": "cccc_file_send",
        "description": "Send a local file (under the group's active scope root) as a chat attachment.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "group_id": {
                    "type": "string",
                    "description": "Working group ID (optional if CCCC_GROUP_ID is set)",
                },
                "actor_id": {
                    "type": "string",
                    "description": "Your actor ID (sender, optional if CCCC_ACTOR_ID is set)",
                },
                "path": {
                    "type": "string",
                    "description": "File path (relative to active scope root, or absolute under it)",
                },
                "text": {
                    "type": "string",
                    "description": "Optional message text (caption)",
                },
                "to": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Recipients (same as cccc_message_send)",
                },
                "priority": {
                    "type": "string",
                    "enum": ["normal", "attention"],
                    "description": "Message priority (default normal). Use attention when the message is materially important.",
                },
                "reply_required": {
                    "type": "boolean",
                    "description": "Whether recipients must reply to this message (default false). Use for concrete action/result requests.",
                    "default": False,
                },
            },
            "required": ["path"],
        },
    },
    {
        "name": "cccc_blob_path",
        "description": "Resolve an attachment blob path (e.g. state/blobs/<sha>_<name>) to an absolute filesystem path.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "group_id": {
                    "type": "string",
                    "description": "Working group ID (optional if CCCC_GROUP_ID is set)",
                },
                "rel_path": {
                    "type": "string",
                    "description": "Relative attachment path from events (state/blobs/...)",
                },
            },
            "required": ["rel_path"],
        },
    },
    {
        "name": "cccc_group_info",
        "description": "Get working group information (title, scopes, actors, etc.).",
        "inputSchema": {
            "type": "object",
            "properties": {
                "group_id": {
                    "type": "string",
                    "description": "Working group ID (optional if CCCC_GROUP_ID is set)",
                }
            },
            "required": [],
        },
    },
    {
        "name": "cccc_group_list",
        "description": "List working groups (IDs, titles, and status).",
        "inputSchema": {
            "type": "object",
            "properties": {},
            "required": [],
        },
    },
    {
        "name": "cccc_actor_list",
        "description": "Get list of all actors in the group.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "group_id": {
                    "type": "string",
                    "description": "Working group ID (optional if CCCC_GROUP_ID is set)",
                }
            },
            "required": [],
        },
    },
    {
        "name": "cccc_actor_profile_list",
        "description": "List reusable Actor Profiles (global). Use before cccc_actor_add with profile_id.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "by": {
                    "type": "string",
                    "description": "Your actor ID (optional if CCCC_ACTOR_ID is set)",
                }
            },
            "required": [],
        },
    },
    {
        "name": "cccc_actor_add",
        "description": "Add a new actor to the group. Only foreman can add actors. Role is auto-determined: first enabled actor = foreman, rest = peer. Use cccc_runtime_list first to see available runtimes.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "group_id": {
                    "type": "string",
                    "description": "Working group ID (optional if CCCC_GROUP_ID is set)",
                },
                "by": {
                    "type": "string",
                    "description": "Your actor ID (optional if CCCC_ACTOR_ID is set)",
                },
                "actor_id": {
                    "type": "string",
                    "description": "New actor ID (e.g. peer-impl, peer-test)",
                },
                "runtime": {
                    "type": "string",
                    "enum": [
                        "claude",
                        "codex",
                        "droid",
                        "amp",
                        "auggie",
                        "neovate",
                        "gemini",
                        "cursor",
                        "kilocode",
                        "opencode",
                        "copilot",
                        "custom",
                    ],
                    "description": "Agent runtime (auto-sets command)",
                    "default": "codex",
                },
                "runner": {
                    "type": "string",
                    "enum": ["pty", "headless"],
                    "description": "Runner type: pty (interactive terminal) or headless (MCP-only)",
                    "default": "pty",
                },
                "title": {"type": "string", "description": "Display title (optional)"},
                "command": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Command (optional, auto-set by runtime)",
                },
                "env": {
                    "type": "object",
                    "additionalProperties": {"type": "string"},
                    "description": "Environment variables",
                },
                "profile_id": {
                    "type": "string",
                    "description": "Optional Actor Profile ID. When set, runtime/runner/command/env are taken from the profile.",
                },
            },
            "required": ["actor_id"],
        },
    },
    {
        "name": "cccc_actor_remove",
        "description": "Remove an actor from the group. Foreman and peer can only remove themselves. To remove a peer: tell them to finish up and call this on themselves.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "group_id": {
                    "type": "string",
                    "description": "Working group ID (optional if CCCC_GROUP_ID is set)",
                },
                "by": {
                    "type": "string",
                    "description": "Your actor ID (optional if CCCC_ACTOR_ID is set)",
                },
                "actor_id": {
                    "type": "string",
                    "description": "Actor ID to remove (optional; defaults to yourself)",
                },
            },
            "required": [],
        },
    },
    {
        "name": "cccc_actor_start",
        "description": "Start an actor (set enabled=true). Only foreman can start actors.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "group_id": {
                    "type": "string",
                    "description": "Working group ID (optional if CCCC_GROUP_ID is set)",
                },
                "by": {
                    "type": "string",
                    "description": "Your actor ID (optional if CCCC_ACTOR_ID is set)",
                },
                "actor_id": {"type": "string", "description": "Actor ID to start"},
            },
            "required": ["actor_id"],
        },
    },
    {
        "name": "cccc_actor_stop",
        "description": "Stop an actor (set enabled=false). Foreman can stop any actor; peer can only stop themselves.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "group_id": {
                    "type": "string",
                    "description": "Working group ID (optional if CCCC_GROUP_ID is set)",
                },
                "by": {
                    "type": "string",
                    "description": "Your actor ID (optional if CCCC_ACTOR_ID is set)",
                },
                "actor_id": {"type": "string", "description": "Actor ID to stop"},
            },
            "required": ["actor_id"],
        },
    },
    {
        "name": "cccc_actor_restart",
        "description": "Restart an actor (stop + start, clears context). Foreman can restart any actor; peer can only restart themselves. Useful when context is too long or state is confused.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "group_id": {
                    "type": "string",
                    "description": "Working group ID (optional if CCCC_GROUP_ID is set)",
                },
                "by": {
                    "type": "string",
                    "description": "Your actor ID (optional if CCCC_ACTOR_ID is set)",
                },
                "actor_id": {"type": "string", "description": "Actor ID to restart"},
            },
            "required": ["actor_id"],
        },
    },
    {
        "name": "cccc_runtime_list",
        "description": "List available agent runtimes on the system. Call before cccc_actor_add to see which runtimes can be used.",
        "inputSchema": {
            "type": "object",
            "properties": {},
            "required": [],
        },
    },
    {
        "name": "cccc_capability_search",
        "description": (
            "Search capability registry for progressive disclosure.\n"
            "Returns built-in packs and synced external catalog entries with qualification/trust/freshness metadata.\n"
            "When local hits are empty, daemon may augment from remote MCP Registry and backfill local snapshot."
        ),
        "inputSchema": {
            "type": "object",
            "properties": {
                "group_id": {
                    "type": "string",
                    "description": "Working group ID (optional if CCCC_GROUP_ID is set)",
                },
                "actor_id": {
                    "type": "string",
                    "description": "Your actor ID (optional if CCCC_ACTOR_ID is set)",
                },
                "query": {
                    "type": "string",
                    "description": "Search query across id/name/description/tags",
                    "default": "",
                },
                "kind": {
                    "type": "string",
                    "enum": ["", "mcp_toolpack", "skill"],
                    "description": "Optional kind filter",
                    "default": "",
                },
                "source_id": {
                    "type": "string",
                    "description": "Optional source filter (e.g., mcp_registry_official, anthropic_skills)",
                    "default": "",
                },
                "trust_tier": {
                    "type": "string",
                    "description": "Optional trust tier filter (e.g., builtin, official, community)",
                    "default": "",
                },
                "qualification_status": {
                    "type": "string",
                    "description": "Optional qualification status filter (qualified/manual_review/blocked)",
                    "default": "",
                },
                "include_external": {
                    "type": "boolean",
                    "description": "Include external source snapshot records in results (default true)",
                    "default": True,
                },
                "limit": {
                    "type": "integer",
                    "description": "Max results to return",
                    "default": 30,
                    "minimum": 1,
                    "maximum": 200,
                },
            },
            "required": [],
        },
    },
    {
        "name": "cccc_capability_enable",
        "description": (
            "Enable/disable a capability by scope.\n"
            "Scopes: group, actor, session (TTL).\n"
            "For skills (kind=skill), this is also the pin/unpin control (actor scope recommended).\n"
            "Use cccc_capability_state after mutation, then re-list tools if refresh_required=true."
        ),
        "inputSchema": {
            "type": "object",
            "properties": {
                "group_id": {
                    "type": "string",
                    "description": "Working group ID (optional if CCCC_GROUP_ID is set)",
                },
                "by": {
                    "type": "string",
                    "description": "Your actor ID (optional if CCCC_ACTOR_ID is set)",
                },
                "actor_id": {
                    "type": "string",
                    "description": "Target actor ID for scope=actor/session (defaults to by)",
                },
                "capability_id": {
                    "type": "string",
                    "description": "Capability ID from cccc_capability_search",
                },
                "scope": {
                    "type": "string",
                    "enum": ["group", "actor", "session"],
                    "description": "Enable scope",
                    "default": "session",
                },
                "enabled": {
                    "type": "boolean",
                    "description": "true=enable, false=disable",
                    "default": True,
                },
                "cleanup": {
                    "type": "boolean",
                    "description": (
                        "When enabled=false, also attempt cache cleanup.\n"
                        "If the capability is still bound elsewhere, cleanup is skipped safely."
                    ),
                    "default": False,
                },
                "approve": {
                    "type": "boolean",
                    "description": (
                        "Approval signal for manual-review capabilities. "
                        "Required to enable external records with qualification_status=manual_review."
                    ),
                    "default": False,
                },
                "reason": {
                    "type": "string",
                    "description": "Optional short audit reason for this capability mutation",
                    "default": "",
                },
                "ttl_seconds": {
                    "type": "integer",
                    "description": "TTL for session scope (seconds)",
                    "default": 3600,
                    "minimum": 60,
                    "maximum": 86400,
                },
            },
            "required": ["capability_id"],
        },
    },
    {
        "name": "cccc_capability_state",
        "description": (
            "Read effective capability exposure and visible MCP tool names for current caller scope.\n"
            "Also returns active/pinned skills and external binding states.\n"
            "Use this when a capability appears hidden or after enable/disable changes."
        ),
        "inputSchema": {
            "type": "object",
            "properties": {
                "group_id": {
                    "type": "string",
                    "description": "Working group ID (optional if CCCC_GROUP_ID is set)",
                },
                "actor_id": {
                    "type": "string",
                    "description": "Your actor ID (optional if CCCC_ACTOR_ID is set)",
                },
            },
            "required": [],
        },
    },
    {
        "name": "cccc_capability_uninstall",
        "description": (
            "Remove an external capability installation from CCCC runtime cache and revoke its bindings.\n"
            "This is an admin operation (user/foreman). Use when a capability is no longer needed."
        ),
        "inputSchema": {
            "type": "object",
            "properties": {
                "group_id": {
                    "type": "string",
                    "description": "Working group ID (optional if CCCC_GROUP_ID is set)",
                },
                "by": {
                    "type": "string",
                    "description": "Your actor ID (optional if CCCC_ACTOR_ID is set)",
                },
                "capability_id": {
                    "type": "string",
                    "description": "Capability ID to uninstall",
                },
                "reason": {
                    "type": "string",
                    "description": "Optional short audit reason for uninstall",
                    "default": "",
                },
            },
            "required": ["capability_id"],
        },
    },
    {
        "name": "cccc_capability_use",
        "description": (
            "One-step capability use: auto-enable capability (session scope by default) and optionally call a tool.\n"
            "For skills (kind=skill), this returns skill capsule payload and applies declared dependencies.\n"
            "For built-in pack tools, capability_id can be omitted when tool_name uniquely maps to a pack."
        ),
        "inputSchema": {
            "type": "object",
            "properties": {
                "group_id": {
                    "type": "string",
                    "description": "Working group ID (optional if CCCC_GROUP_ID is set)",
                },
                "by": {
                    "type": "string",
                    "description": "Your actor ID (optional if CCCC_ACTOR_ID is set)",
                },
                "actor_id": {
                    "type": "string",
                    "description": "Target actor ID (defaults to by)",
                },
                "capability_id": {
                    "type": "string",
                    "description": "Optional capability ID; may be inferred from built-in pack tool_name",
                    "default": "",
                },
                "tool_name": {
                    "type": "string",
                    "description": "Tool to call after enable (optional)",
                    "default": "",
                },
                "tool_arguments": {
                    "type": "object",
                    "description": "Arguments for tool call",
                    "default": {},
                },
                "scope": {
                    "type": "string",
                    "enum": ["group", "actor", "session"],
                    "description": "Enable scope",
                    "default": "session",
                },
                "approve": {
                    "type": "boolean",
                    "description": "Approval signal for manual-review capabilities",
                    "default": False,
                },
                "ttl_seconds": {
                    "type": "integer",
                    "description": "TTL for session scope (seconds)",
                    "default": 3600,
                    "minimum": 60,
                    "maximum": 86400,
                },
                "reason": {
                    "type": "string",
                    "description": "Optional short audit reason",
                    "default": "",
                },
            },
            "required": [],
        },
    },
    {
        "name": "cccc_space_status",
        "description": (
            "Read Group Space status for the current group "
            "(provider mode, binding, queue summary, sync state, failed_items)."
        ),
        "inputSchema": {
            "type": "object",
            "properties": {
                "group_id": {
                    "type": "string",
                    "description": "Working group ID (optional if CCCC_GROUP_ID is set)",
                },
                "provider": {
                    "type": "string",
                    "enum": ["notebooklm"],
                    "description": "Group Space provider (default notebooklm)",
                    "default": "notebooklm",
                },
            },
            "required": [],
        },
    },
    {
        "name": "cccc_space_capabilities",
        "description": (
            "Return Group Space capability matrix: local file whitelist policy for repo/space sync and "
            "resource_ingest source-type schema/examples."
        ),
        "inputSchema": {
            "type": "object",
            "properties": {
                "group_id": {
                    "type": "string",
                    "description": "Working group ID (optional if CCCC_GROUP_ID is set)",
                },
                "provider": {
                    "type": "string",
                    "enum": ["notebooklm"],
                    "description": "Group Space provider (default notebooklm)",
                    "default": "notebooklm",
                },
            },
            "required": [],
        },
    },
    {
        "name": "cccc_space_bind",
        "description": "Bind/unbind Group Space provider mapping for this group. Write operation (user/foreman).",
        "inputSchema": {
            "type": "object",
            "properties": {
                "group_id": {
                    "type": "string",
                    "description": "Working group ID (optional if CCCC_GROUP_ID is set)",
                },
                "by": {
                    "type": "string",
                    "description": "Your actor ID (optional if CCCC_ACTOR_ID is set)",
                },
                "provider": {
                    "type": "string",
                    "enum": ["notebooklm"],
                    "description": "Group Space provider (default notebooklm)",
                    "default": "notebooklm",
                },
                "action": {
                    "type": "string",
                    "enum": ["bind", "unbind"],
                    "description": "Bind or unbind provider mapping",
                    "default": "bind",
                },
                "remote_space_id": {
                    "type": "string",
                    "description": "Remote notebook/space ID required for action=bind",
                },
            },
            "required": [],
        },
    },
    {
        "name": "cccc_space_ingest",
        "description": (
            "Enqueue and execute a Group Space ingest job with idempotency protection.\n"
            "For kind=resource_ingest, payload supports source_type + fields:\n"
            "- file: {source_type, file_path} (or path/url as file_path alias)\n"
            "- web_page/youtube: {source_type, url, title?}\n"
            "- pasted_text: {source_type, content, title?}\n"
            "- google_docs/google_slides/google_spreadsheet: {source_type, file_id, title?, mime_type?}\n"
            "Convenience: if payload is omitted, top-level source_type/url/content/file_id/file_path/... fields are auto-packed into payload."
        ),
        "inputSchema": {
            "type": "object",
            "properties": {
                "group_id": {
                    "type": "string",
                    "description": "Working group ID (optional if CCCC_GROUP_ID is set)",
                },
                "by": {
                    "type": "string",
                    "description": "Your actor ID (optional if CCCC_ACTOR_ID is set)",
                },
                "provider": {
                    "type": "string",
                    "enum": ["notebooklm"],
                    "description": "Group Space provider (default notebooklm)",
                    "default": "notebooklm",
                },
                "kind": {
                    "type": "string",
                    "enum": ["context_sync", "resource_ingest"],
                    "description": "Ingest job kind",
                    "default": "context_sync",
                },
                "payload": {
                    "type": "object",
                    "description": (
                        "Provider-specific payload. For resource_ingest, include source_type and required fields "
                        "(url/content/file_id depending on type)."
                    ),
                    "default": {},
                },
                "source_type": {
                    "type": "string",
                    "enum": ["file", "web_page", "youtube", "pasted_text", "google_docs", "google_slides", "google_spreadsheet"],
                    "description": "Top-level convenience field when payload is omitted",
                },
                "file_path": {
                    "type": "string",
                    "description": "Top-level convenience field for source_type=file",
                },
                "path": {
                    "type": "string",
                    "description": "Alias of file_path for source_type=file",
                },
                "url": {
                    "type": "string",
                    "description": "Top-level convenience field for web_page/youtube (or file path alias when source_type=file)",
                },
                "content": {
                    "type": "string",
                    "description": "Top-level convenience field for pasted_text",
                },
                "text": {
                    "type": "string",
                    "description": "Alias of content for pasted_text",
                },
                "file_id": {
                    "type": "string",
                    "description": "Top-level convenience field for google_docs/slides/spreadsheet",
                },
                "mime_type": {
                    "type": "string",
                    "description": "Optional MIME hint for google drive source types",
                },
                "title": {
                    "type": "string",
                    "description": "Optional display title for resource_ingest source",
                },
                "idempotency_key": {
                    "type": "string",
                    "description": "Optional idempotency key for dedupe",
                },
            },
            "required": [],
        },
    },
    {
        "name": "cccc_space_query",
        "description": "Query Group Space knowledge provider. On provider degradation, returns degraded=true instead of crashing.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "group_id": {
                    "type": "string",
                    "description": "Working group ID (optional if CCCC_GROUP_ID is set)",
                },
                "provider": {
                    "type": "string",
                    "enum": ["notebooklm"],
                    "description": "Group Space provider (default notebooklm)",
                    "default": "notebooklm",
                },
                "query": {
                    "type": "string",
                    "description": "Query text",
                },
                "options": {
                    "type": "object",
                    "description": (
                        "Optional query options. Supported keys: source_ids (array of remote source_id). "
                        "Unsupported keys like language/lang are rejected; put language requirements in query text."
                    ),
                    "properties": {
                        "source_ids": {
                            "type": "array",
                            "items": {"type": "string"},
                            "description": "Optional remote source_id filter list",
                        }
                    },
                    "default": {},
                },
            },
            "required": ["query"],
        },
    },
    {
        "name": "cccc_space_sources",
        "description": "List/refresh/rename/delete NotebookLM sources in the currently bound notebook.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "group_id": {
                    "type": "string",
                    "description": "Working group ID (optional if CCCC_GROUP_ID is set)",
                },
                "by": {
                    "type": "string",
                    "description": "Your actor ID (optional if CCCC_ACTOR_ID is set)",
                },
                "provider": {
                    "type": "string",
                    "enum": ["notebooklm"],
                    "description": "Group Space provider (default notebooklm)",
                    "default": "notebooklm",
                },
                "action": {
                    "type": "string",
                    "enum": ["list", "refresh", "rename", "delete"],
                    "description": "Source lifecycle action",
                    "default": "list",
                },
                "source_id": {
                    "type": "string",
                    "description": "Required for action=refresh|rename|delete",
                },
                "new_title": {
                    "type": "string",
                    "description": "Required for action=rename",
                },
            },
            "required": [],
        },
    },
    {
        "name": "cccc_space_artifact",
        "description": (
            "List/generate/download NotebookLM artifacts for the current group.\n"
            "If action is omitted and source/options/wait/save_to_space are provided, CCCC auto-infers action=generate.\n"
            "For long generations, prefer action=generate with wait=false (default), then poll action=list and download when ready."
        ),
        "inputSchema": {
            "type": "object",
            "properties": {
                "group_id": {
                    "type": "string",
                    "description": "Working group ID (optional if CCCC_GROUP_ID is set)",
                },
                "by": {
                    "type": "string",
                    "description": "Your actor ID (optional if CCCC_ACTOR_ID is set)",
                },
                "provider": {
                    "type": "string",
                    "enum": ["notebooklm"],
                    "description": "Group Space provider (default notebooklm)",
                    "default": "notebooklm",
                },
                "action": {
                    "type": "string",
                    "enum": ["list", "generate", "download"],
                    "description": "Artifact action",
                    "default": "list",
                },
                "kind": {
                    "type": "string",
                    "enum": [
                        "audio",
                        "video",
                        "report",
                        "study_guide",
                        "quiz",
                        "flashcards",
                        "infographic",
                        "slide_deck",
                        "data_table",
                        "mind_map",
                    ],
                    "description": "Artifact kind. Required for action=generate|download, optional filter for action=list.",
                },
                "options": {
                    "type": "object",
                    "description": "Provider-specific artifact generation options (action=generate)",
                    "default": {},
                },
                "source": {
                    "type": "string",
                    "description": "Optional source hint (used for action auto-inference and provider hints)",
                },
                "language": {
                    "type": "string",
                    "description": "Preferred output language (e.g., zh-CN, ja, en). Can also be passed via options.language.",
                },
                "lang": {
                    "type": "string",
                    "description": "Alias of language",
                },
                "wait": {
                    "type": "boolean",
                    "description": "For action=generate: wait for completion before returning",
                    "default": False,
                },
                "save_to_space": {
                    "type": "boolean",
                    "description": "Auto-download completed artifact to repo/space/artifacts",
                    "default": True,
                },
                "output_path": {
                    "type": "string",
                    "description": "Optional local output path override for download/save",
                },
                "output_format": {
                    "type": "string",
                    "enum": ["json", "markdown", "html"],
                    "description": "Output format for quiz/flashcards downloads",
                    "default": "markdown",
                },
                "artifact_id": {
                    "type": "string",
                    "description": "Specific artifact ID to download (optional)",
                },
                "timeout_seconds": {
                    "type": "number",
                    "description": "For action=generate with wait=true: max wait seconds",
                    "default": 600,
                    "minimum": 10,
                    "maximum": 3600,
                },
                "initial_interval": {
                    "type": "number",
                    "description": "For wait polling: initial interval seconds",
                    "default": 2,
                    "minimum": 0.5,
                    "maximum": 60,
                },
                "max_interval": {
                    "type": "number",
                    "description": "For wait polling: max interval seconds",
                    "default": 10,
                    "minimum": 1,
                    "maximum": 120,
                },
            },
            "required": [],
        },
    },
    {
        "name": "cccc_space_jobs",
        "description": "List/retry/cancel Group Space jobs. Use action=list for read-only polling.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "group_id": {
                    "type": "string",
                    "description": "Working group ID (optional if CCCC_GROUP_ID is set)",
                },
                "by": {
                    "type": "string",
                    "description": "Your actor ID (optional if CCCC_ACTOR_ID is set)",
                },
                "provider": {
                    "type": "string",
                    "enum": ["notebooklm"],
                    "description": "Group Space provider (default notebooklm)",
                    "default": "notebooklm",
                },
                "action": {
                    "type": "string",
                    "enum": ["list", "retry", "cancel"],
                    "description": "Job control action",
                    "default": "list",
                },
                "job_id": {
                    "type": "string",
                    "description": "Required for action=retry|cancel",
                },
                "state": {
                    "type": "string",
                    "enum": ["pending", "running", "succeeded", "failed", "canceled"],
                    "description": "Optional filter for action=list",
                },
                "limit": {
                    "type": "integer",
                    "description": "Max jobs for action=list (default 50)",
                    "default": 50,
                    "minimum": 1,
                    "maximum": 500,
                },
            },
            "required": [],
        },
    },
    {
        "name": "cccc_space_sync",
        "description": "Run Group Space file reconciliation for repo/space resources (or read current sync state).",
        "inputSchema": {
            "type": "object",
            "properties": {
                "group_id": {
                    "type": "string",
                    "description": "Working group ID (optional if CCCC_GROUP_ID is set)",
                },
                "by": {
                    "type": "string",
                    "description": "Your actor ID (optional if CCCC_ACTOR_ID is set)",
                },
                "provider": {
                    "type": "string",
                    "enum": ["notebooklm"],
                    "description": "Group Space provider (default notebooklm)",
                    "default": "notebooklm",
                },
                "action": {
                    "type": "string",
                    "enum": ["status", "run"],
                    "description": "Read sync status or run a sync reconcile",
                    "default": "run",
                },
                "force": {
                    "type": "boolean",
                    "description": "For action=run, force full reconcile even without local change",
                    "default": False,
                },
            },
            "required": [],
        },
    },
    {
        "name": "cccc_space_provider_auth",
        "description": "Control Group Space provider auth flow (status/start/cancel).",
        "inputSchema": {
            "type": "object",
            "properties": {
                "provider": {
                    "type": "string",
                    "enum": ["notebooklm"],
                    "description": "Group Space provider (default notebooklm)",
                    "default": "notebooklm",
                },
                "by": {
                    "type": "string",
                    "description": "Caller identity (optional if CCCC_ACTOR_ID is set)",
                },
                "action": {
                    "type": "string",
                    "enum": ["status", "start", "cancel"],
                    "description": "Auth flow action",
                    "default": "status",
                },
                "timeout_seconds": {
                    "type": "integer",
                    "description": "For action=start: timeout seconds (60-1800, default 900)",
                    "default": 900,
                    "minimum": 60,
                    "maximum": 1800,
                },
            },
            "required": [],
        },
    },
    {
        "name": "cccc_space_provider_credential_status",
        "description": "Read Group Space provider credential status (masked metadata only).",
        "inputSchema": {
            "type": "object",
            "properties": {
                "provider": {
                    "type": "string",
                    "enum": ["notebooklm"],
                    "description": "Group Space provider (default notebooklm)",
                    "default": "notebooklm",
                },
                "by": {
                    "type": "string",
                    "description": "Caller identity (optional if CCCC_ACTOR_ID is set)",
                },
            },
            "required": [],
        },
    },
    {
        "name": "cccc_space_provider_credential_update",
        "description": "Update or clear Group Space provider credential.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "provider": {
                    "type": "string",
                    "enum": ["notebooklm"],
                    "description": "Group Space provider (default notebooklm)",
                    "default": "notebooklm",
                },
                "by": {
                    "type": "string",
                    "description": "Caller identity (optional if CCCC_ACTOR_ID is set)",
                },
                "auth_json": {
                    "type": "string",
                    "description": "Provider credential JSON payload (required when clear=false)",
                },
                "clear": {
                    "type": "boolean",
                    "description": "Clear stored credential instead of updating",
                    "default": False,
                },
            },
            "required": [],
        },
    },
    {
        "name": "cccc_group_set_state",
        "description": "Set group state to control automation behavior. States: active (normal operation), idle (task complete; internal automation is muted while scheduled rules still run), paused (user pause; all automation blocked), stopped (stop all actor runtimes). Foreman should set to 'idle' when task is complete.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "group_id": {
                    "type": "string",
                    "description": "Working group ID (optional if CCCC_GROUP_ID is set)",
                },
                "actor_id": {
                    "type": "string",
                    "description": "Your actor ID (optional if CCCC_ACTOR_ID is set)",
                },
                "state": {
                    "type": "string",
                    "enum": ["active", "idle", "paused", "stopped"],
                    "description": "New state: active (work in progress), idle (task complete), paused (user paused), stopped (stop all actor runtimes)",
                },
            },
            "required": ["state"],
        },
    },
    {
        "name": "cccc_automation_state",
        "description": (
            "Read automation reminders/status. Foreman sees all reminders; peer sees group reminders + own personal reminders."
        ),
        "inputSchema": {
            "type": "object",
            "properties": {
                "group_id": {
                    "type": "string",
                    "description": "Working group ID (optional if CCCC_GROUP_ID is set)",
                },
                "actor_id": {
                    "type": "string",
                    "description": "Your actor ID (optional if CCCC_ACTOR_ID is set)",
                },
            },
            "required": [],
        },
    },
    {
        "name": "cccc_automation_manage",
        "description": (
            "Manage automation reminders.\n"
            "Simple mode (recommended): use op=create|update|enable|disable|delete|replace_all.\n"
            "MCP actor writes are notify-only (operational actions are Web/Admin only).\n"
            "API field names keep protocol terms: rule / rule_id / ruleset.\n"
            "Rules and actions must use canonical contract fields only (no legacy aliases).\n"
            "Advanced mode: pass actions[] directly."
        ),
        "inputSchema": {
            "type": "object",
            "properties": {
                "group_id": {
                    "type": "string",
                    "description": "Working group ID (optional if CCCC_GROUP_ID is set)",
                },
                "actor_id": {
                    "type": "string",
                    "description": "Your actor ID (optional if CCCC_ACTOR_ID is set)",
                },
                "expected_version": {
                    "type": "integer",
                    "description": "Optimistic concurrency version (optional)",
                },
                "op": {
                    "type": "string",
                    "enum": [
                        "create",
                        "update",
                        "enable",
                        "disable",
                        "delete",
                        "replace_all",
                    ],
                    "description": "Recommended simple operation selector for reminders",
                },
                "rule": {
                    "type": "object",
                    "description": "Reminder object for op=create/update (protocol name: rule)",
                },
                "rule_id": {
                    "type": "string",
                    "description": "Reminder id for op=enable/disable/delete (protocol name: rule_id)",
                },
                "ruleset": {
                    "type": "object",
                    "description": "Full reminder set for op=replace_all (protocol name: ruleset; foreman only)",
                },
                "actions": {
                    "type": "array",
                    "items": {"type": "object"},
                    "description": (
                        "Advanced action objects (protocol-level). Examples: "
                        "{type:'create_rule',rule:{...}}, {type:'update_rule',rule:{...}}, "
                        "{type:'set_rule_enabled',rule_id:'r1',enabled:true}, {type:'delete_rule',rule_id:'r1'}, "
                        "{type:'replace_all_rules',ruleset:{rules:[...],snippets:{...}}}"
                    ),
                },
            },
            "required": [],
        },
    },
    {
        "name": "cccc_project_info",
        "description": "Get PROJECT.md content from the group's active scope. Use this to understand project goals, constraints, and context. Call at session start or when you need to align with project vision.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "group_id": {
                    "type": "string",
                    "description": "Working group ID (optional if CCCC_GROUP_ID is set)",
                }
            },
            "required": [],
        },
    },
    # context.* namespace - state sync
    {
        "name": "cccc_context_get",
        "description": "Get group context (vision/sketch/milestones/notes/references/presence + tasks_summary/active_task). Call at session start. Archived milestones are hidden by default.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "group_id": {
                    "type": "string",
                    "description": "Working group ID (optional if CCCC_GROUP_ID is set)",
                },
                "include_archived": {
                    "type": "boolean",
                    "description": "Include archived milestones (default false)",
                    "default": False,
                },
            },
            "required": [],
        },
    },
    {
        "name": "cccc_context_sync",
        "description": "Batch sync context operations. Supported ops: vision.update, sketch.update, milestone.*, task.*, note.*, reference.*, presence.*",
        "inputSchema": {
            "type": "object",
            "properties": {
                "group_id": {
                    "type": "string",
                    "description": "Working group ID (optional if CCCC_GROUP_ID is set)",
                },
                "ops": {
                    "type": "array",
                    "items": {"type": "object"},
                    "description": "List of operations, each is {op: string, ...params}",
                },
                "dry_run": {
                    "type": "boolean",
                    "description": "Validate only without executing",
                    "default": False,
                },
            },
            "required": ["ops"],
        },
    },
    {
        "name": "cccc_vision_update",
        "description": "Update project vision (one-sentence north star goal).",
        "inputSchema": {
            "type": "object",
            "properties": {
                "group_id": {
                    "type": "string",
                    "description": "Working group ID (optional if CCCC_GROUP_ID is set)",
                },
                "vision": {"type": "string", "description": "Project vision"},
            },
            "required": ["vision"],
        },
    },
    {
        "name": "cccc_sketch_update",
        "description": "Update execution sketch (static architecture/strategy, no TODOs/progress).",
        "inputSchema": {
            "type": "object",
            "properties": {
                "group_id": {
                    "type": "string",
                    "description": "Working group ID (optional if CCCC_GROUP_ID is set)",
                },
                "sketch": {
                    "type": "string",
                    "description": "Execution sketch (markdown)",
                },
            },
            "required": ["sketch"],
        },
    },
    {
        "name": "cccc_milestone_create",
        "description": "Create a milestone (coarse-grained phase, 2-6 total).",
        "inputSchema": {
            "type": "object",
            "properties": {
                "group_id": {
                    "type": "string",
                    "description": "Working group ID (optional if CCCC_GROUP_ID is set)",
                },
                "name": {"type": "string", "description": "Milestone name"},
                "description": {
                    "type": "string",
                    "description": "Detailed description",
                },
                "status": {
                    "type": "string",
                    "enum": ["planned", "active", "done", "archived"],
                    "description": "Status (default planned)",
                    "default": "planned",
                },
            },
            "required": ["name", "description"],
        },
    },
    {
        "name": "cccc_milestone_update",
        "description": "Update a milestone.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "group_id": {
                    "type": "string",
                    "description": "Working group ID (optional if CCCC_GROUP_ID is set)",
                },
                "milestone_id": {
                    "type": "string",
                    "description": "Milestone ID (M1, M2...)",
                },
                "name": {"type": "string", "description": "New name"},
                "description": {"type": "string", "description": "New description"},
                "status": {
                    "type": "string",
                    "enum": ["planned", "active", "done", "archived"],
                    "description": "New status",
                },
            },
            "required": ["milestone_id"],
        },
    },
    {
        "name": "cccc_milestone_complete",
        "description": "Complete a milestone and record outcomes.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "group_id": {
                    "type": "string",
                    "description": "Working group ID (optional if CCCC_GROUP_ID is set)",
                },
                "milestone_id": {"type": "string", "description": "Milestone ID"},
                "outcomes": {"type": "string", "description": "Outcomes summary"},
            },
            "required": ["milestone_id", "outcomes"],
        },
    },
    {
        "name": "cccc_task_list",
        "description": "List all tasks or get single task details. Archived tasks are hidden by default.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "group_id": {
                    "type": "string",
                    "description": "Working group ID (optional if CCCC_GROUP_ID is set)",
                },
                "task_id": {
                    "type": "string",
                    "description": "Task ID (optional, omit to list all)",
                },
                "include_archived": {
                    "type": "boolean",
                    "description": "Include archived tasks (default false)",
                    "default": False,
                },
            },
            "required": [],
        },
    },
    {
        "name": "cccc_task_create",
        "description": "Create a task (deliverable work item with 3-7 steps).",
        "inputSchema": {
            "type": "object",
            "properties": {
                "group_id": {
                    "type": "string",
                    "description": "Working group ID (optional if CCCC_GROUP_ID is set)",
                },
                "name": {"type": "string", "description": "Task name"},
                "goal": {"type": "string", "description": "Completion criteria"},
                "steps": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "name": {"type": "string"},
                            "acceptance": {"type": "string"},
                        },
                        "required": ["name", "acceptance"],
                    },
                    "description": "Step list (3-7 steps)",
                },
                "milestone_id": {
                    "type": "string",
                    "description": "Associated milestone ID",
                },
                "assignee": {"type": "string", "description": "Assignee actor ID"},
            },
            "required": ["name", "goal", "steps"],
        },
    },
    {
        "name": "cccc_task_update",
        "description": "Update task status or step progress.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "group_id": {
                    "type": "string",
                    "description": "Working group ID (optional if CCCC_GROUP_ID is set)",
                },
                "task_id": {"type": "string", "description": "Task ID (T001, T002...)"},
                "status": {
                    "type": "string",
                    "enum": ["planned", "active", "done", "archived"],
                    "description": "Task status",
                },
                "name": {"type": "string", "description": "New name"},
                "goal": {"type": "string", "description": "New completion criteria"},
                "assignee": {"type": "string", "description": "New assignee"},
                "milestone_id": {
                    "type": "string",
                    "description": "New associated milestone",
                },
                "step_id": {
                    "type": "string",
                    "description": "Step ID to update (S1, S2...)",
                },
                "step_status": {
                    "type": "string",
                    "enum": ["pending", "in_progress", "done"],
                    "description": "New step status",
                },
            },
            "required": ["task_id"],
        },
    },
    {
        "name": "cccc_note_add",
        "description": "Add a note (lessons, discoveries, warnings).",
        "inputSchema": {
            "type": "object",
            "properties": {
                "group_id": {
                    "type": "string",
                    "description": "Working group ID (optional if CCCC_GROUP_ID is set)",
                },
                "content": {"type": "string", "description": "Note content"},
            },
            "required": ["content"],
        },
    },
    {
        "name": "cccc_note_update",
        "description": "Update note content.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "group_id": {
                    "type": "string",
                    "description": "Working group ID (optional if CCCC_GROUP_ID is set)",
                },
                "note_id": {"type": "string", "description": "Note ID (N001, N002...)"},
                "content": {"type": "string", "description": "New content"},
            },
            "required": ["note_id"],
        },
    },
    {
        "name": "cccc_note_remove",
        "description": "Remove a note.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "group_id": {
                    "type": "string",
                    "description": "Working group ID (optional if CCCC_GROUP_ID is set)",
                },
                "note_id": {"type": "string", "description": "Note ID"},
            },
            "required": ["note_id"],
        },
    },
    {
        "name": "cccc_reference_add",
        "description": "Add a reference (useful file/URL).",
        "inputSchema": {
            "type": "object",
            "properties": {
                "group_id": {
                    "type": "string",
                    "description": "Working group ID (optional if CCCC_GROUP_ID is set)",
                },
                "url": {"type": "string", "description": "File path or URL"},
                "note": {"type": "string", "description": "Why this is useful"},
            },
            "required": ["url", "note"],
        },
    },
    {
        "name": "cccc_reference_update",
        "description": "Update a reference.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "group_id": {
                    "type": "string",
                    "description": "Working group ID (optional if CCCC_GROUP_ID is set)",
                },
                "reference_id": {
                    "type": "string",
                    "description": "Reference ID (R001, R002...)",
                },
                "url": {"type": "string", "description": "New URL"},
                "note": {"type": "string", "description": "New note"},
            },
            "required": ["reference_id"],
        },
    },
    {
        "name": "cccc_reference_remove",
        "description": "Remove a reference.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "group_id": {
                    "type": "string",
                    "description": "Working group ID (optional if CCCC_GROUP_ID is set)",
                },
                "reference_id": {"type": "string", "description": "Reference ID"},
            },
            "required": ["reference_id"],
        },
    },
    {
        "name": "cccc_presence_get",
        "description": "Get presence status of all agents.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "group_id": {
                    "type": "string",
                    "description": "Working group ID (optional if CCCC_GROUP_ID is set)",
                }
            },
            "required": [],
        },
    },
    {
        "name": "cccc_presence_update",
        "description": "Update your presence status (what you're doing/thinking).",
        "inputSchema": {
            "type": "object",
            "properties": {
                "group_id": {
                    "type": "string",
                    "description": "Working group ID (optional if CCCC_GROUP_ID is set)",
                },
                "agent_id": {
                    "type": "string",
                    "description": "Your agent ID (optional; defaults to yourself)",
                },
                "status": {
                    "type": "string",
                    "description": "Status description (1-2 sentences)",
                },
            },
            "required": ["status"],
        },
    },
    {
        "name": "cccc_presence_clear",
        "description": "Clear your presence status.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "group_id": {
                    "type": "string",
                    "description": "Working group ID (optional if CCCC_GROUP_ID is set)",
                },
                "agent_id": {
                    "type": "string",
                    "description": "Your agent ID (optional; defaults to yourself)",
                },
            },
            "required": [],
        },
    },
    # headless.* namespace - headless runner control (for MCP-driven agents)
    {
        "name": "cccc_headless_status",
        "description": "Get headless session status. Only for runner=headless actors.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "group_id": {
                    "type": "string",
                    "description": "Working group ID (optional if CCCC_GROUP_ID is set)",
                },
                "actor_id": {
                    "type": "string",
                    "description": "Your actor ID (optional if CCCC_ACTOR_ID is set)",
                },
            },
            "required": [],
        },
    },
    {
        "name": "cccc_headless_set_status",
        "description": "Update headless session status. Report your current work state.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "group_id": {
                    "type": "string",
                    "description": "Working group ID (optional if CCCC_GROUP_ID is set)",
                },
                "actor_id": {
                    "type": "string",
                    "description": "Your actor ID (optional if CCCC_ACTOR_ID is set)",
                },
                "status": {
                    "type": "string",
                    "enum": ["idle", "working", "waiting", "stopped"],
                    "description": "Status: idle=waiting for tasks, working=executing, waiting=blocked on decision, stopped=terminated",
                },
                "task_id": {
                    "type": "string",
                    "description": "Current task ID (optional)",
                },
            },
            "required": ["status"],
        },
    },
    {
        "name": "cccc_headless_ack_message",
        "description": "Acknowledge a processed message. For headless loop message confirmation.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "group_id": {
                    "type": "string",
                    "description": "Working group ID (optional if CCCC_GROUP_ID is set)",
                },
                "actor_id": {
                    "type": "string",
                    "description": "Your actor ID (optional if CCCC_ACTOR_ID is set)",
                },
                "message_id": {
                    "type": "string",
                    "description": "Processed message event_id",
                },
            },
            "required": ["message_id"],
        },
    },
    # notify.* namespace - system notifications
    {
        "name": "cccc_notify_send",
        "description": "Send system notification (for system-level agent communication, won't pollute chat log).",
        "inputSchema": {
            "type": "object",
            "properties": {
                "group_id": {
                    "type": "string",
                    "description": "Working group ID (optional if CCCC_GROUP_ID is set)",
                },
                "actor_id": {
                    "type": "string",
                    "description": "Your actor ID (sender, optional if CCCC_ACTOR_ID is set)",
                },
                "kind": {
                    "type": "string",
                    "enum": [
                        "nudge",
                        "keepalive",
                        "help_nudge",
                        "actor_idle",
                        "silence_check",
                        "automation",
                        "status_change",
                        "error",
                        "info",
                    ],
                    "description": "Notification type",
                },
                "title": {"type": "string", "description": "Notification title"},
                "message": {"type": "string", "description": "Notification content"},
                "target_actor_id": {
                    "type": "string",
                    "description": "Target actor ID (optional, omit=broadcast)",
                },
                "priority": {
                    "type": "string",
                    "enum": ["low", "normal", "high", "urgent"],
                    "description": "Priority (high/urgent delivered directly to PTY)",
                    "default": "normal",
                },
                "requires_ack": {
                    "type": "boolean",
                    "description": "Whether acknowledgment is required",
                    "default": False,
                },
            },
            "required": ["kind", "title", "message"],
        },
    },
    {
        "name": "cccc_notify_ack",
        "description": "Acknowledge system notification (only when requires_ack=true).",
        "inputSchema": {
            "type": "object",
            "properties": {
                "group_id": {
                    "type": "string",
                    "description": "Working group ID (optional if CCCC_GROUP_ID is set)",
                },
                "actor_id": {
                    "type": "string",
                    "description": "Your actor ID (optional if CCCC_ACTOR_ID is set)",
                },
                "notify_event_id": {
                    "type": "string",
                    "description": "Notification event_id to acknowledge",
                },
            },
            "required": ["notify_event_id"],
        },
    },
    # terminal.* namespace - transcript (policy gated by group settings)
    {
        "name": "cccc_terminal_tail",
        "description": (
            "Tail an actor terminal transcript (subject to group policy `terminal_transcript_visibility`).\n"
            "Use it to quickly see what a peer is doing/stuck on (e.g., before you nudge/coordinate).\n"
            "If you get permission_denied, ask user/foreman to enable it in Settings -> Transcript.\n"
            "Warning: may include sensitive stdout/stderr."
        ),
        "inputSchema": {
            "type": "object",
            "properties": {
                "group_id": {
                    "type": "string",
                    "description": "Working group ID (optional if CCCC_GROUP_ID is set)",
                },
                "actor_id": {
                    "type": "string",
                    "description": "Your actor ID (optional if CCCC_ACTOR_ID is set)",
                },
                "target_actor_id": {
                    "type": "string",
                    "description": "Actor ID whose transcript to read",
                },
                "max_chars": {
                    "type": "integer",
                    "description": "Max characters of transcript to return (default 8000)",
                    "default": 8000,
                    "minimum": 1,
                    "maximum": 100000,
                },
                "strip_ansi": {
                    "type": "boolean",
                    "description": "Strip ANSI control sequences (default true)",
                    "default": True,
                },
            },
            "required": ["target_actor_id"],
        },
    },
    # im.* namespace - IM bridge operations
    {
        "name": "cccc_im_bind",
        "description": (
            "Bind a Telegram (or other IM) chat using a one-time key.\n\n"
            "Typical flow: user runs /subscribe in IM chat, gets a key, opens CCCC Web > Settings > IM Bridge > Bind, "
            "and the foreman can also call this tool to complete the binding.\n\n"
            "The key expires after 10 minutes."
        ),
        "inputSchema": {
            "type": "object",
            "properties": {
                "group_id": {
                    "type": "string",
                    "description": "Working group ID (optional if CCCC_GROUP_ID is set)",
                },
                "key": {
                    "type": "string",
                    "description": "The one-time binding key from /subscribe",
                },
            },
            "required": ["key"],
        },
    },
    # memory.* namespace - agent-driven memory system
    {
        "name": "cccc_memory_guide",
        "description": "Get topic-specific best-practice guidance for complex memory operations (read-only).",
        "inputSchema": {
            "type": "object",
            "properties": {
                "topic": {
                    "type": "string",
                    "enum": ["store", "search", "consolidation", "lifecycle"],
                    "description": "Guide topic",
                },
            },
            "required": ["topic"],
        },
    },
    {
        "name": "cccc_memory_store",
        "description": (
            "Store or update a memory.\n\n"
            "Create mode (no id): stores new memory with content and optional metadata.\n"
            "Update mode (with id): updates existing memory fields.\n"
            "Set solidify=true to immediately solidify the memory after store/update.\n"
            "Use strategy param to apply predefined status/confidence presets: "
            "aggressive (solid/medium), conservative (draft/high), milestone-only (solid/high)."
        ),
        "inputSchema": {
            "type": "object",
            "properties": {
                "group_id": {
                    "type": "string",
                    "description": "Working group ID (optional if CCCC_GROUP_ID is set)",
                },
                "id": {
                    "type": "string",
                    "description": "Memory ID for update mode (omit for create)",
                },
                "content": {
                    "type": "string",
                    "description": "Memory content (required for create, optional for update)",
                },
                "kind": {
                    "type": "string",
                    "enum": ["observation", "decision", "preference", "fact", "instruction", "context", "relation"],
                    "description": "Memory kind",
                },
                "status": {
                    "type": "string",
                    "enum": ["draft", "solid"],
                    "description": "Memory status (default draft; overridden by strategy)",
                },
                "confidence": {
                    "type": "string",
                    "enum": ["low", "medium", "high"],
                    "description": "Confidence level (default medium; overridden by strategy)",
                },
                "source_type": {
                    "type": "string",
                    "enum": ["manual", "chat_ingest", "milestone_report", "agent_extract", "reflection"],
                    "description": "Source type (e.g. chat_ingest, manual, reflection)",
                },
                "source_ref": {
                    "type": "string",
                    "description": "Source reference (e.g. event_id)",
                },
                "scope_key": {
                    "type": "string",
                    "description": "Scope key for namespacing",
                },
                "actor_id": {
                    "type": "string",
                    "description": "Actor who created this memory",
                },
                "task_id": {
                    "type": "string",
                    "description": "Associated task ID",
                },
                "milestone_id": {
                    "type": "string",
                    "description": "Associated milestone ID",
                },
                "event_ts": {
                    "type": "string",
                    "description": "Event timestamp (ISO 8601)",
                },
                "tags": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Tags for categorization",
                },
                "strategy": {
                    "type": "string",
                    "enum": ["aggressive", "conservative", "milestone-only"],
                    "description": "Predefined status/confidence preset (overrides explicit status/confidence)",
                },
                "solidify": {
                    "type": "boolean",
                    "description": "Immediately solidify after store/update (default false)",
                    "default": False,
                },
                "summary": {
                    "type": "string",
                    "description": "L0 summary of the memory (manual, for token-efficient recall)",
                },
            },
            "required": [],
        },
    },
    {
        "name": "cccc_memory_search",
        "description": (
            "Search memories via full-text search and/or structured filters.\n\n"
            "FTS5 query with CJK supplement. Supports filters: status, kind, actor_id, task_id, "
            "milestone_id, confidence, tags, since/until. Results are ordered solid-first, then by recency.\n"
            "Default search has no side effects. Set track_hit=true to increment hit_count "
            "and enable auto-solidify for drafts at hit_count >= 3."
        ),
        "inputSchema": {
            "type": "object",
            "properties": {
                "group_id": {
                    "type": "string",
                    "description": "Working group ID (optional if CCCC_GROUP_ID is set)",
                },
                "query": {
                    "type": "string",
                    "description": "Full-text search query (optional; omit for filter-only mode)",
                },
                "status": {
                    "type": "string",
                    "enum": ["draft", "solid"],
                    "description": "Filter by status",
                },
                "kind": {
                    "type": "string",
                    "enum": ["observation", "decision", "preference", "fact", "instruction", "context", "relation"],
                    "description": "Filter by kind",
                },
                "actor_id": {
                    "type": "string",
                    "description": "Filter by actor ID",
                },
                "task_id": {
                    "type": "string",
                    "description": "Filter by task ID",
                },
                "milestone_id": {
                    "type": "string",
                    "description": "Filter by milestone ID",
                },
                "confidence": {
                    "type": "string",
                    "enum": ["low", "medium", "high"],
                    "description": "Filter by confidence level",
                },
                "tags": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Filter by tags (AND logic)",
                },
                "since": {
                    "type": "string",
                    "description": "Filter memories created after this ISO 8601 timestamp",
                },
                "until": {
                    "type": "string",
                    "description": "Filter memories created before this ISO 8601 timestamp",
                },
                "track_hit": {
                    "type": "boolean",
                    "default": False,
                    "description": "When true, increments hit_count for returned memories and enables auto-solidify at threshold",
                },
                "limit": {
                    "type": "integer",
                    "description": "Max results to return (default 20, max 100)",
                    "default": 20,
                    "minimum": 1,
                    "maximum": 100,
                },
                "depth": {
                    "type": "string",
                    "enum": ["L0", "L2"],
                    "description": "Return depth: L0=summary only (default, saves tokens), L2=full content+summary",
                    "default": "L0",
                },
            },
            "required": [],
        },
    },
    {
        "name": "cccc_memory_ingest",
        "description": (
            "Ingest recent chat messages into memory.\n\n"
            "Modes:\n"
            "- signal (default): returns structured summary grouped by actor with suggested_kind, "
            "key_phrases, time_range. Use this to decide what to store.\n"
            "- raw: bulk imports chat messages as source_type=chat_ingest memories.\n\n"
            "Uses watermark to skip already-processed events. Set reset_watermark=true to re-process."
        ),
        "inputSchema": {
            "type": "object",
            "properties": {
                "group_id": {
                    "type": "string",
                    "description": "Working group ID (optional if CCCC_GROUP_ID is set)",
                },
                "mode": {
                    "type": "string",
                    "enum": ["signal", "raw"],
                    "description": "Ingest mode: signal (structured summary) or raw (bulk import)",
                    "default": "signal",
                },
                "limit": {
                    "type": "integer",
                    "description": "Number of recent ledger lines to read (default 50, max 200)",
                    "default": 50,
                    "minimum": 1,
                    "maximum": 200,
                },
                "actor_id": {
                    "type": "string",
                    "description": "Filter by actor ID (raw mode only)",
                },
                "reset_watermark": {
                    "type": "boolean",
                    "description": "Reset watermark to re-process previously ingested events",
                    "default": False,
                },
            },
            "required": [],
        },
    },
    {
        "name": "cccc_memory_stats",
        "description": "Get memory statistics for the group (total count, by status, by kind, source type, tag count).",
        "inputSchema": {
            "type": "object",
            "properties": {
                "group_id": {
                    "type": "string",
                    "description": "Working group ID (optional if CCCC_GROUP_ID is set)",
                },
            },
            "required": [],
        },
    },
    # memory_export — read-only Markdown + manifest export
    {
        "name": "cccc_memory_export",
        "description": "Export solid memories as read-only Markdown (memory.md) + manifest.json with SHA-256 hash.\nWrites files to the group's state directory. Returns manifest and file paths.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "group_id": {
                    "type": "string",
                    "description": "Working group ID (optional if CCCC_GROUP_ID is set)",
                },
                "include_draft": {
                    "type": "boolean",
                    "default": False,
                    "description": "Include draft memories in export (default: solid only)",
                },
                "output_dir": {
                    "type": "string",
                    "description": "Optional output directory override (default: group state dir)",
                },
            },
            "required": [],
        },
    },
    # memory_delete — delete one or many memories by ID
    {
        "name": "cccc_memory_delete",
        "description": "Delete memory by ID(s) (scoped to group). Supports single id or batch ids for manual cleanup/compression.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "group_id": {
                    "type": "string",
                    "description": "Working group ID (optional if CCCC_GROUP_ID is set)",
                },
                "id": {
                    "type": "string",
                    "description": "Single memory ID to delete",
                },
                "ids": {
                    "type": "array",
                    "description": "Batch memory IDs to delete",
                    "items": {"type": "string"},
                },
            },
            "required": [],
            "anyOf": [{"required": ["id"]}, {"required": ["ids"]}],
        },
    },
    # memory_decay — find stale candidates (safe, non-destructive)
    {
        "name": "cccc_memory_decay",
        "description": "Find stale memory candidates for cleanup/decay using last_recalled_at + hit_count + created_at. Returns candidates only, does not delete.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "group_id": {
                    "type": "string",
                    "description": "Working group ID (optional if CCCC_GROUP_ID is set)",
                },
                "draft_days": {
                    "type": "integer",
                    "description": "Draft memories older than this threshold become stale candidates",
                    "default": 30,
                    "minimum": 1,
                    "maximum": 3650,
                },
                "zero_hit_days": {
                    "type": "integer",
                    "description": "Draft memories with hit_count=0 older than this threshold become high-priority candidates",
                    "default": 14,
                    "minimum": 1,
                    "maximum": 3650,
                },
                "solid_review_days": {
                    "type": "integer",
                    "description": "Solid memories older than this threshold are returned as review candidates (not auto-delete)",
                    "default": 120,
                    "minimum": 1,
                    "maximum": 3650,
                },
                "solid_max_hit": {
                    "type": "integer",
                    "description": "Maximum hit_count for solid review candidates",
                    "default": 1,
                    "minimum": 0,
                    "maximum": 1000000,
                },
                "limit": {
                    "type": "integer",
                    "description": "Maximum number of candidates to return",
                    "default": 100,
                    "minimum": 1,
                    "maximum": 1000,
                },
            },
            "required": [],
        },
    },
    # debug.* namespace - developer mode diagnostics (user + foreman only; dev mode required)
    {
        "name": "cccc_debug_snapshot",
        "description": "Developer diagnostics: get a structured snapshot (requires developer mode; restricted to user + foreman).",
        "inputSchema": {
            "type": "object",
            "properties": {
                "group_id": {
                    "type": "string",
                    "description": "Working group ID (optional if CCCC_GROUP_ID is set)",
                },
                "actor_id": {
                    "type": "string",
                    "description": "Your actor ID (optional if CCCC_ACTOR_ID is set)",
                },
            },
            "required": [],
        },
    },
    {
        "name": "cccc_debug_tail_logs",
        "description": "Developer diagnostics: tail local CCCC logs (requires developer mode; restricted to user + foreman).",
        "inputSchema": {
            "type": "object",
            "properties": {
                "group_id": {
                    "type": "string",
                    "description": "Working group ID (optional if CCCC_GROUP_ID is set)",
                },
                "actor_id": {
                    "type": "string",
                    "description": "Your actor ID (optional if CCCC_ACTOR_ID is set)",
                },
                "component": {
                    "type": "string",
                    "enum": ["daemon", "web", "im"],
                    "description": "Which component logs to tail",
                },
                "lines": {
                    "type": "integer",
                    "description": "Max lines to return (default 200)",
                    "default": 200,
                    "minimum": 1,
                    "maximum": 10000,
                },
            },
            "required": ["component"],
        },
    },
]
