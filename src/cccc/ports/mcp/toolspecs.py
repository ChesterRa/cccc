"""MCP tool schemas for CCCC consolidated surface."""

from __future__ import annotations

from .task_types import TASK_TYPE_IDS

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
        "annotations": {"readOnlyHint": True},
        "inputSchema": _obj({}),
    },
    {
        "name": "cccc_bootstrap",
        "description": (
            "Cold-start bootstrap: session + recovery + inbox_preview + context_hygiene + memory_recall_gate + next_calls. "
            "Use it first on cold start or resume; usually follow with cccc_help once, then pull "
            "cccc_project_info / cccc_context_get only when colder detail is needed."
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
        "annotations": {"readOnlyHint": True},
        "inputSchema": _obj({**_COMMON_GROUP}),
    },
    {
        "name": "cccc_inbox_list",
        "description": "List unread inbox entries (chat/notify/all).",
        "annotations": {"readOnlyHint": True},
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
        "description": "Send a visible chat message. Choose `to` deliberately; use @all only when the whole group needs it.",
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
                "priority": {"type": "string", "enum": ["normal", "attention"], "default": "normal"},
                "reply_required": {"type": "boolean", "default": False},
                "refs": {"type": "array", "items": {"type": "object"}},
            },
            required=["text"],
        ),
    },
    {
        "name": "cccc_tracked_send",
        "description": "Create a durable task and send one linked visible delegation message. Use only when owner/scope/done/evidence must survive chat; do not use for ordinary discussion or quick solo work.",
        "inputSchema": _obj(
            {
                **_COMMON_GROUP,
                **_COMMON_ACTOR,
                "title": {"type": "string", "description": "Short task title"},
                "text": {"type": "string", "description": "Visible message to send to the recipient"},
                "to": {
                    "anyOf": [
                        {"type": "string"},
                        {"type": "array", "items": {"type": "string"}},
                    ]
                },
                "outcome": {"type": "string", "description": "Done criterion; defaults to text when omitted"},
                "checklist": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "text": {"type": "string"},
                            "status": {"type": "string", "enum": ["pending", "in_progress", "done"]},
                        },
                        "required": ["text"],
                    },
                },
                "assignee": {"type": "string", "description": "Optional explicit owner; defaults from a single concrete `to` actor"},
                "waiting_on": {"type": "string", "enum": ["none", "user", "actor", "external"], "default": "actor"},
                "handoff_to": {"type": "string"},
                "notes": {"type": "string"},
                "priority": {"type": "string", "enum": ["normal", "attention"], "default": "normal"},
                "reply_required": {"type": "boolean", "default": True},
                "idempotency_key": {"type": "string", "description": "Stable caller retry key to avoid duplicate successful sends"},
                "refs": {"type": "array", "items": {"type": "object"}},
            },
            required=["title", "text"],
        ),
    },
    {
        "name": "cccc_message_reply",
        "description": "Reply to a visible chat message (by event_id/reply_to). Use only for the thread you are answering; set `to` explicitly if the audience differs.",
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
                "priority": {"type": "string", "enum": ["normal", "attention"], "default": "normal"},
                "reply_required": {"type": "boolean", "default": False},
                "refs": {"type": "array", "items": {"type": "object"}},
            },
            required=["text"],
        ),
    },
    {
        "name": "cccc_pet_decisions",
        "description": "Pet-only decision surface: action=get|replace|clear. The pet actor should write structured Web Pet decisions here instead of sending reminder-like chat messages.",
        "inputSchema": _obj(
            {
                **_COMMON_GROUP,
                **_COMMON_ACTOR,
                "action": {"type": "string", "enum": ["get", "replace", "clear"], "default": "get"},
                "decisions": {"type": "array", "items": {"type": "object"}},
            }
        ),
    },
    {
        "name": "cccc_voice_secretary_document",
        "description": "Voice Secretary-only input/document surface. voice_secretary_input notifications normally include a daemon-delivered input_envelope rendered as Work orders; use read_new_input only for legacy pointer notifications, recovery, or manual debugging. Use list/create/archive for document lifecycle. Edit repo markdown directly; this tool has no save action.",
        "inputSchema": _obj(
            {
                **_COMMON_GROUP,
                **_COMMON_ACTOR,
                "action": {"type": "string", "enum": ["list", "create", "read_new_input", "archive"], "default": "list"},
                "document_path": {"type": "string", "description": "Repository-relative markdown path returned by list/create/read_new_input."},
                "title": {"type": "string"},
                "include_archived": {"type": "boolean", "default": False},
            }
        ),
    },
    {
        "name": "cccc_voice_secretary_request",
        "description": (
            "Voice Secretary-only request surface. Use report for user-visible Ask replies; use handoff only for explicit non-secretary work "
            "that must go to foreman or one concrete actor."
        ),
        "inputSchema": _obj(
            {
                **_COMMON_GROUP,
                **_COMMON_ACTOR,
                "action": {"type": "string", "enum": ["handoff", "report"], "default": "handoff"},
                "target": {"type": "string", "description": "Required for action=handoff: @foreman or one concrete actor id. Do not omit it for handoff."},
                "request_text": {"type": "string", "description": "Short actionable handoff request. Do not include raw transcript dumps or secretary-scope work."},
                "summary": {"type": "string", "description": "For action=handoff only: optional one-line context summary for the receiving peer."},
                "request_id": {"type": "string", "description": "Required for action=report: Request id from Target: secretary/document input."},
                "source_request_id": {"type": "string", "description": "For action=handoff, the original Ask request id being handed off."},
                "status": {"type": "string", "enum": ["working", "done", "needs_user", "failed"], "description": "Required for action=report."},
                "reply_text": {"type": "string", "description": "For action=report, the concise user-visible reply shown near the composer."},
                "document_path": {"type": "string"},
                "artifact_paths": {"type": "array", "items": {"type": "string"}, "description": "For action=report, optional repo-relative document/artifact paths to show as links instead of repeating them in reply_text."},
                "source_summary": {"type": "string", "description": "For action=report, optional concise source/evidence note for factual answers."},
                "checked_at": {"type": "string", "description": "For action=report, optional ISO timestamp or short freshness note for factual answers."},
                "source_urls": {"type": "array", "items": {"type": "string"}, "description": "For action=report, optional source URLs for factual answers."},
                "source_event_id": {"type": "string"},
                "priority": {"type": "string", "enum": ["low", "normal", "high", "urgent"], "default": "normal"},
                "requires_ack": {"type": "boolean", "default": True},
            },
        ),
    },
    {
        "name": "cccc_voice_secretary_composer",
        "description": (
            "Voice Secretary-only composer result surface. Use submit_prompt_draft for prompt_refine results instead of sending chat."
        ),
        "inputSchema": _obj(
            {
                **_COMMON_GROUP,
                **_COMMON_ACTOR,
                "action": {"type": "string", "enum": ["submit_prompt_draft"], "default": "submit_prompt_draft"},
                "request_id": {"type": "string", "description": "Request id from the prompt_refine input batch."},
                "draft_text": {"type": "string", "description": "Composer text to insert. Follow the prompt_refine Operation: append operations return an addition; replace operations return a complete replacement."},
                "summary": {"type": "string", "description": "Optional one-line summary of what changed."},
                "operation": {"type": "string", "description": "Optional; omit to inherit the Operation from the prompt_refine input."},
                "composer_snapshot_hash": {"type": "string"},
            },
            required=["request_id", "draft_text"],
        ),
    },
    {
        "name": "cccc_file",
        "description": "File operations: action=send(path,text,...), blob_path(rel_path), info(rel_path), or read(rel_path).",
        "inputSchema": _obj(
            {
                **_COMMON_GROUP,
                **_COMMON_ACTOR,
                "action": {"type": "string", "enum": ["send", "blob_path", "info", "read"], "default": "send"},
                "path": {"type": "string", "description": "Required for action=send"},
                "text": {"type": "string"},
                "to": {
                    "anyOf": [
                        {"type": "string"},
                        {"type": "array", "items": {"type": "string"}},
                    ]
                },
                "priority": {"type": "string", "enum": ["normal", "attention"], "default": "normal"},
                "reply_required": {"type": "boolean", "default": False},
                "rel_path": {"type": "string", "description": "Required for action=blob_path/info/read. Can be just the blob filename (e.g. 'sha256_image.png') or full relative path ('state/blobs/sha256_image.png')."},
                "max_bytes": {"type": "integer", "default": 200000, "minimum": 1, "maximum": 1000000},
            }
        ),
    },
    {
        "name": "cccc_code_exec",
        "description": (
            "Default-on ChatGPT Web Model code mode. Run JavaScript that orchestrates CCCC MCP tools through "
            "global tools.<toolName>(args), with ALL_TOOLS, text(), store(), load(), and yield_control(). "
            "Use this for multi-step local development loops such as read -> patch -> test -> diff -> report. "
            "The JS runtime has no Node require/import/fs/network/console access; use nested MCP tools for all real work. "
            "If the result says running with a cell_id, call cccc_code_wait."
        ),
        "annotations": {"readOnlyHint": False, "destructiveHint": True},
        "inputSchema": _obj(
            {
                "source": {"type": "string", "description": "Raw JavaScript source. Do not wrap in markdown fences or JSON strings."},
                "code": {"type": "string", "description": "Alias for source."},
                "yield_time_ms": {
                    "type": "integer",
                    "default": 10000,
                    "minimum": 0,
                    "maximum": 60000,
                    "description": "Return early with a running cell_id if the script is still active after this many milliseconds.",
                },
                "max_output_tokens": {
                    "type": "integer",
                    "default": 10000,
                    "minimum": 1,
                    "maximum": 50000,
                    "description": "Approximate token budget for direct cccc_code_exec output.",
                },
            },
        ),
    },
    {
        "name": "cccc_code_wait",
        "description": (
            "Wait for or terminate a yielded cccc_code_exec cell. Use only when cccc_code_exec returned "
            "status=running and a cell_id. Returns new output since the previous yield or the final result."
        ),
        "annotations": {"readOnlyHint": False, "destructiveHint": True},
        "inputSchema": _obj(
            {
                "cell_id": {"type": "string", "description": "Identifier returned by cccc_code_exec."},
                "yield_time_ms": {
                    "type": "integer",
                    "default": 10000,
                    "minimum": 0,
                    "maximum": 60000,
                    "description": "How long to wait for more output before yielding again.",
                },
                "max_tokens": {
                    "type": "integer",
                    "default": 10000,
                    "minimum": 1,
                    "maximum": 50000,
                    "description": "Approximate token budget for this wait result.",
                },
                "terminate": {"type": "boolean", "default": False, "description": "Terminate the running code cell."},
            },
            required=["cell_id"],
        ),
    },
    {
        "name": "cccc_repo",
        "description": (
            "Read-only active workspace repository inspection for remote runtimes: "
            "action=info|list|list_dir|read. All paths are constrained to the group's active scope root. "
            "Read returns sha256 and supports start_line/end_line; pass sha256 back as expected_sha256 before writing. "
            "Use cccc_repo_edit or cccc_apply_patch for writes."
        ),
        "annotations": {"readOnlyHint": True},
        "inputSchema": _obj(
            {
                **_COMMON_GROUP,
                "action": {
                    "type": "string",
                    "enum": ["info", "list", "list_dir", "read"],
                    "default": "info",
                },
                "path": {"type": "string", "description": "Relative path under the active workspace root."},
                "file_path": {"type": "string", "description": "Alias for path."},
                "max_bytes": {"type": "integer", "default": 200000, "minimum": 1, "maximum": 1000000},
                "limit": {"type": "integer", "default": 200, "minimum": 1, "maximum": 500},
                "offset": {"type": "integer", "default": 1, "minimum": 1, "description": "For action=list_dir, 1-indexed entry offset."},
                "depth": {"type": "integer", "default": 2, "minimum": 1, "maximum": 8, "description": "For action=list_dir, maximum directory depth."},
                "start_line": {"type": "integer", "minimum": 1, "description": "For action=read, first 1-indexed line to return."},
                "end_line": {"type": "integer", "minimum": 1, "description": "For action=read, final 1-indexed line to return."},
                "include_hidden": {"type": "boolean", "default": False},
            }
        ),
    },
    {
        "name": "cccc_repo_edit",
        "description": (
            "Write to the active workspace repository for remote runtimes: action=replace|multi_replace|write|apply_patch|mkdir|delete|move. "
            "For small code edits, prefer read -> replace/multi_replace with expected_sha256, then cccc_git diff. "
            "For file-oriented patches, prefer cccc_apply_patch with Codex *** Begin Patch format."
        ),
        "annotations": {"readOnlyHint": False, "destructiveHint": True},
        "inputSchema": _obj(
            {
                **_COMMON_GROUP,
                "action": {
                    "type": "string",
                    "enum": ["replace", "multi_replace", "write", "apply_patch", "mkdir", "delete", "move"],
                    "default": "replace",
                },
                "path": {"type": "string", "description": "Relative path under the active workspace root; required for write/delete/move/mkdir."},
                "file_path": {"type": "string", "description": "Alias for path."},
                "dest_path": {"type": "string", "description": "Destination path under the active workspace root; required for action=move."},
                "to_path": {"type": "string", "description": "Alias for dest_path."},
                "content": {"type": "string", "description": "Required for action=write. Use expected_sha256 after reading unless intentionally creating a new file."},
                "old_text": {"type": "string", "description": "Required for action=replace. Must exactly match current file text."},
                "new_text": {"type": "string", "description": "Replacement text for action=replace."},
                "replacements": {"type": "array", "items": {"type": "object"}, "description": "For action=multi_replace: ordered objects with old_text, new_text, optional expected_replacements, replace_all."},
                "expected_sha256": {"type": "string", "description": "Optional sha256 from cccc_repo(action=read); rejects stale writes/replaces."},
                "expected_replacements": {"type": "integer", "minimum": 1, "maximum": 10000, "description": "Optional exact old_text match count for action=replace."},
                "replace_all": {"type": "boolean", "default": False, "description": "For action=replace, replace every old_text match instead of requiring a single exact match."},
                "patch": {"type": "string", "description": "For action=apply_patch; must be Codex *** Begin Patch format. Git unified diff is not accepted here."},
                "recursive": {"type": "boolean", "default": False, "description": "Required true to delete directories."},
                "exist_ok": {"type": "boolean", "default": True, "description": "For action=mkdir."},
            }
        ),
    },
    {
        "name": "cccc_apply_patch",
        "description": (
            "Codex-style file patch tool for Web Model local development. "
            "Use *** Begin Patch / *** Add File / *** Update File / *** Delete File / *** End Patch. "
            "File paths must be relative to the active workspace root."
        ),
        "annotations": {"readOnlyHint": False, "destructiveHint": True},
        "inputSchema": _obj(
            {
                **_COMMON_GROUP,
                "patch": {"type": "string", "description": "Complete Codex-style patch text starting with *** Begin Patch."},
                "input": {"type": "string", "description": "Alias for patch."},
            }
        ),
    },
    {
        "name": "cccc_shell",
        "description": (
            "Web Model local-power shell execution in the group's active workspace. "
            "Run tests, builds, rg, scripts, and other local commands from a cwd constrained to the active scope root. "
            "Returns ok, returncode, stdout, stderr, and truncation flags. For long-running commands, prefer cccc_exec_command."
        ),
        "annotations": {"readOnlyHint": False, "destructiveHint": True},
        "inputSchema": _obj(
            {
                **_COMMON_GROUP,
                "command": {"type": "string", "description": "Shell command to run under the active workspace root."},
                "cwd": {"type": "string", "description": "Relative cwd under the active workspace root.", "default": "."},
                "timeout_s": {"type": "integer", "default": 60, "minimum": 1, "maximum": 600},
                "max_output_bytes": {"type": "integer", "default": 200000, "minimum": 1, "maximum": 1000000},
                "env": {"type": "object", "additionalProperties": {"type": "string"}},
            },
            required=["command"],
        ),
    },
    {
        "name": "cccc_exec_command",
        "description": (
            "Codex-style session shell execution in the group's active workspace. "
            "Starts a command and returns output plus session_id when it is still running; use cccc_write_stdin to poll or send input."
        ),
        "annotations": {"readOnlyHint": False, "destructiveHint": True},
        "inputSchema": _obj(
            {
                **_COMMON_GROUP,
                "command": {"type": "string", "description": "Shell command to run under the active workspace root."},
                "cmd": {"type": "string", "description": "Alias for command."},
                "cwd": {"type": "string", "description": "Relative cwd under the active workspace root.", "default": "."},
                "workdir": {"type": "string", "description": "Alias for cwd."},
                "yield_time_ms": {"type": "integer", "default": 1000, "minimum": 0, "maximum": 30000},
                "timeout_s": {"type": "integer", "default": 600, "minimum": 1, "maximum": 600},
                "max_output_bytes": {"type": "integer", "default": 200000, "minimum": 1, "maximum": 1000000},
                "env": {"type": "object", "additionalProperties": {"type": "string"}},
            }
        ),
    },
    {
        "name": "cccc_write_stdin",
        "description": (
            "Writes characters to an existing cccc_exec_command session or polls it for more output."
        ),
        "annotations": {"readOnlyHint": False, "destructiveHint": True},
        "inputSchema": _obj(
            {
                "session_id": {"type": "string"},
                "chars": {"type": "string", "description": "Bytes/text to write to stdin; omit or empty to poll."},
                "yield_time_ms": {"type": "integer", "default": 1000, "minimum": 0, "maximum": 30000},
                "max_output_bytes": {"type": "integer", "default": 200000, "minimum": 1, "maximum": 1000000},
                "terminate": {"type": "boolean", "default": False, "description": "Terminate the running session instead of waiting."},
            },
            required=["session_id"],
        ),
    },
    {
        "name": "cccc_git",
        "description": (
            "Web Model local-power git operations in the group's active workspace: "
            "action=status|diff|log|add|commit. Returns ok, returncode, stdout, stderr. "
            "Use cccc_shell for unusual git commands."
        ),
        "annotations": {"readOnlyHint": False, "destructiveHint": True},
        "inputSchema": _obj(
            {
                **_COMMON_GROUP,
                "action": {
                    "type": "string",
                    "enum": ["status", "diff", "log", "add", "commit"],
                    "default": "status",
                },
                "path": {"type": "string", "description": "Optional single repo-relative path for diff/add."},
                "paths": {"type": "array", "items": {"type": "string"}, "description": "Optional repo-relative paths for diff/add."},
                "staged": {"type": "boolean", "default": False, "description": "For action=diff."},
                "all_changes": {"type": "boolean", "default": False, "description": "For action=add, stage all changes under the active workspace."},
                "message": {"type": "string", "description": "Required for action=commit."},
                "count": {"type": "integer", "default": 20, "minimum": 1, "maximum": 100, "description": "For action=log."},
                "max_output_bytes": {"type": "integer", "default": 200000, "minimum": 1, "maximum": 1000000},
            }
        ),
    },
    {
        "name": "cccc_presentation",
        "description": (
            "Group Presentation surface tool: action=get|publish|clear. "
            "Use it to put a persistent report/preview/file on the Chat-tab Presentation rail. "
            "When publishing with path, keep the file linked to the group's active workspace; "
            "blob_rel_path is for snapshot-style uploaded assets."
        ),
        "inputSchema": _obj(
            {
                **_COMMON_GROUP,
                **_COMMON_ACTOR,
                "action": {"type": "string", "enum": ["get", "publish", "clear"], "default": "get"},
                "slot": {
                    "type": "string",
                    "description": "Target slot: auto | slot-1 | slot-2 | slot-3 | slot-4. clear without slot clears all slots.",
                },
                "card_type": {
                    "type": "string",
                    "enum": ["markdown", "table", "image", "pdf", "file", "web_preview"],
                },
                "title": {"type": "string"},
                "summary": {"type": "string"},
                "source_label": {"type": "string"},
                "source_ref": {"type": "string"},
                "content": {"type": "string"},
                "table": {"type": "object"},
                "path": {"type": "string"},
                "url": {"type": "string"},
                "blob_rel_path": {"type": "string"},
                "all": {"type": "boolean", "default": False},
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
        "description": "Actor operations: list/profile_list/add/remove/start/stop/restart. Actor creation follows the caller's allowed runner/profile surface.",
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
                "runner": {"type": "string", "enum": ["pty", "headless"], "default": "pty"},
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
        "annotations": {"readOnlyHint": True},
        "inputSchema": _obj({}),
    },
    {
        "name": "cccc_runtime_wait_next_turn",
        "description": (
            "Pull one coalesced unread CCCC turn for a website-model runtime actor when no turn was "
            "browser-delivered into the web chat. This does not mark messages read; call "
            "cccc_runtime_complete_turn after processing."
        ),
        "annotations": {"readOnlyHint": True},
        "inputSchema": _obj(
            {
                **_COMMON_GROUP,
                **_COMMON_ACTOR,
                "limit": {"type": "integer", "default": 20, "minimum": 1, "maximum": 20},
                "kind_filter": {
                    "type": "string",
                    "enum": ["all", "chat", "notify"],
                    "default": "all",
                },
            }
        ),
    },
    {
        "name": "cccc_runtime_complete_turn",
        "description": (
            "Commit a processed website-model runtime turn, whether it was browser-delivered or pulled. "
            "status=done or partial marks the supplied event_ids read for the actor; failed/cancelled "
            "leaves them unread for retry."
        ),
        "inputSchema": _obj(
            {
                **_COMMON_GROUP,
                **_COMMON_ACTOR,
                "turn_id": {"type": "string"},
                "event_ids": {"type": "array", "items": {"type": "string"}},
                "latest_event_id": {"type": "string"},
                "status": {
                    "type": "string",
                    "enum": ["done", "partial", "failed", "cancelled"],
                    "default": "done",
                },
                "summary": {"type": "string"},
            },
            required=["status"],
        ),
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
            "Use source_id=agent_self_proposed for autonomous low-risk capsule skill proposals; "
            "self-proposed skill capability_id values must use skill:agent_self_proposed:<stable-slug>; "
            "include required When to use/Avoid when/Procedure/Pitfalls/Verification sections; "
            "real imports missing them are rejected to preserve the last valid active version, "
            "direct import is acceptable for low-risk syntax-valid proposals, "
            "while dry_run is recommended before immediate enablement or unclear-risk records; "
            "reuse the same capability_id to update stale/incomplete/wrong self-proposed skills instead of duplicating; "
            "import results report scope/import_action/record_changed/already_active/active_after_import; "
            "import_action is the primary create/update/unchanged signal, while record_changed only compares an existing record; "
            "already_active is the pre-import binding and active_after_import is the post-import runnable binding; "
            "when active/readiness_preview.active, do not enable again; "
            "verify full active capsule updates via capability_state.active_capsule_skills[].capsule_text; "
            "use scope=session for a temporary trial, scope=actor for cross-session reusable self-proposed skills, "
            "and scope=group only for shared team-wide behavior. "
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
                            "description": (
                                "Optional source id; empty/unknown values are normalized to manual_import. "
                                "Use agent_self_proposed for autonomous capsule skill proposals."
                            ),
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
        "description": (
            "Uninstall a capability from local use: revoke bindings and runtime cache, remove current-group actor autoload references, "
            "and for source_id=agent_self_proposed skill records also remove the generated local catalog record plus all actor/profile autoload references. "
            "External registry catalog records are not deleted."
        ),
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
            "Use cccc_capability_import to create or update self-proposed capsule skills; use capability_use only to activate an existing valid skill id. "
            "Returns top-level scope/requested_scope so callers do not have to infer activation scope from nested enable_result. "
            "Use scope=session for temporary activation and scope=actor for cross-session reuse by the selected actor. "
            "Legacy self-proposed ids under skill:agent:* are invalid; re-import under skill:agent_self_proposed:<stable-slug>, then call cccc_capability_uninstall on the legacy id. "
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
            "Group Space hub tool. NotebookLM has two lanes: work and memory. action: status|capabilities|bind|ingest|query|sources|artifact|jobs|sync|"
            "provider_auth|provider_credential_status|provider_credential_update. provider_auth sub_action: status|start|cancel|disconnect; "
            "use force_reauth=true with provider_auth/start to switch Google account. For artifact runs with wait=false, accepted=true plus "
            "status=pending|queued means background accepted: do not poll in a loop; wait for the later system.notify."
        ),
        "inputSchema": _obj(
            {
                **_COMMON_GROUP,
                **_COMMON_BY,
                "provider": {"type": "string", "default": "notebooklm"},
                "lane": {"type": "string", "enum": ["work", "memory"]},
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
                "force_reauth": {
                    "type": "boolean",
                    "description": "Only for provider_auth/start. Skip saved credential reuse and start with a fresh auth browser profile.",
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
        "description": "Get the context control-plane snapshot (coordination, agent states, attention, board).",
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
        "description": "Shared collaboration task hub (not runtime todo): action=list|create|update|move|restore|delete. Use for multi-actor, long-horizon, or user-tracked work. Lifecycle transitions are canonical via move; update with status auto-applies the matching lifecycle move.",
        "inputSchema": _obj(
            {
                **_COMMON_GROUP,
                **_COMMON_ACTOR,
                "action": {
                    "type": "string",
                    "enum": ["list", "create", "update", "move", "restore", "delete"],
                    "default": "list",
                },
                "task_id": {"type": "string"},
                "include_archived": {"type": "boolean", "default": False},
                "title": {"type": "string"},
                "outcome": {"type": "string"},
                "type": {
                    "type": "string",
                    "enum": list(TASK_TYPE_IDS),
                    "description": "Optional durable task type. Use `free` for lightweight work, `standard` for normal closed-loop tasks, or `optimization` for metric-sensitive work.",
                },
                "status": {
                    "type": "string",
                    "enum": ["planned", "active", "done", "archived"],
                    "description": "Lifecycle status. Required for action=move. If passed with action=update, the wrapper also applies the corresponding lifecycle transition. Use action=update when the same call must also change outcome, notes, checklist, or type.",
                },
                "parent_id": {"type": "string"},
                "assignee": {"type": "string"},
                "priority": {"type": "string"},
                "blocked_by": {"type": "array", "items": {"type": "string"}},
                "waiting_on": {"type": "string", "enum": ["none", "user", "actor", "external"]},
                "handoff_to": {"type": "string"},
                "notes": {
                    "type": "string",
                    "description": "Freeform task notes. Keep them compact; use structured sections only when the workflow genuinely needs them.",
                },
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
        "name": "cccc_role_notes",
        "description": (
            "Manage actor-scoped role notes in CCCC_HELP.md (`## @actor: <actor_id>` blocks): action=get|set|clear. "
            "Foreman can read/write any actor's role notes; other actors can only read their own notes."
        ),
        "inputSchema": _obj(
            {
                **_COMMON_GROUP,
                "action": {"type": "string", "enum": ["get", "set", "clear"], "default": "get"},
                "target_actor_id": {
                    "type": "string",
                    "description": "The actor whose help-scoped role notes to read/write. Omit for get only when listing all as foreman.",
                },
                "content": {
                    "type": "string",
                    "description": "New markdown body for this actor's `## @actor:` help block (required for set).",
                },
                "by": {"type": "string", "description": "Caller actor id override (normally auto-resolved)"},
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
