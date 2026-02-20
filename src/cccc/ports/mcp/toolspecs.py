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
