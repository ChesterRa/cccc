"""
CCCC MCP Server - IM-style Agent Collaboration Tools

Static MCP surface:
- cccc_help / cccc_bootstrap / cccc_project_info
- cccc_inbox_list / cccc_inbox_mark_read
- cccc_message_send / cccc_message_reply
- cccc_file / cccc_repo / cccc_repo_edit / cccc_apply_patch / cccc_shell / cccc_exec_command / cccc_write_stdin / cccc_git / cccc_voice_secretary_document / cccc_voice_secretary_request / cccc_group / cccc_actor / cccc_runtime_list
- cccc_capability_search / cccc_capability_enable / cccc_capability_block / cccc_capability_state / cccc_capability_import / cccc_capability_uninstall / cccc_capability_use
- cccc_space / cccc_automation
- cccc_context_get / cccc_coordination / cccc_task / cccc_agent_state
- cccc_context_sync (advanced batch op)
- cccc_memory / cccc_memory_admin
- cccc_headless / cccc_notify / cccc_terminal / cccc_debug
- cccc_im_bind
- cccc_code_exec / cccc_code_wait (ChatGPT Web Model code-mode orchestration)

All operations go through daemon IPC to ensure single-writer principle.
"""

from __future__ import annotations

import json
import os
from typing import Any, Dict, List, Optional

# Kernel/util imports needed by routing
from ...kernel.actors import find_actor, get_effective_role, is_pet_actor, is_voice_secretary_actor
from ...kernel.blobs import resolve_blob_attachment_path, store_blob_bytes
from ...kernel.group import load_group
from ...kernel.capabilities import (
    BUILTIN_CAPABILITY_PACKS,
    CORE_ADMIN_TOOLS,
    WEB_MODEL_CORE_TOOLS,
    resolve_visible_tool_names,
    web_model_advertised_tool_names,
)
from ...kernel.memory_guide import build_memory_guide
from ...kernel.prompt_files import HELP_FILENAME, read_group_prompt_file
from ...kernel.voice_secretary_actor import VOICE_SECRETARY_ACTOR_ID
from ...util.conv import coerce_bool

# Common MCP utilities
from .common import (
    MCPError,
    _call_daemon_or_raise,
    _resolve_caller_actor_id,
    _resolve_caller_from_by,
    _resolve_group_id,
    _resolve_self_actor_id,
    _runtime_context,
)
from .toolspecs import MCP_TOOLS

# ---------------------------------------------------------------------------
# Handler re-exports for backward compatibility (tests import from server.py)
# ---------------------------------------------------------------------------
from .handlers.cccc_core import (  # noqa: F401
    _CCCC_HELP_BUILTIN,
    _append_runtime_help_addenda,
    _build_context_hygiene_hint,
    bootstrap,
    inbox_list,
    inbox_mark_all_read,
    inbox_mark_read,
    project_info,
)
from .handlers.cccc_messaging import (  # noqa: F401
    blob_info,
    blob_path,
    blob_read,
    file_send,
    message_reply,
    message_send,
    tracked_send,
)
from .handlers.cccc_repo import (  # noqa: F401
    apply_codex_patch_tool,
    exec_command_tool,
    git_tool,
    repo_tool,
    shell_tool,
    write_stdin_tool,
)
from .handlers.code_mode import (  # noqa: F401
    CODE_MODE_TOOL_NAMES,
    code_exec_tool,
    code_mode_enabled,
    code_wait_tool,
)
from .handlers.presentation import (  # noqa: F401
    presentation_clear,
    presentation_get,
    presentation_publish,
)
from .handlers.cccc_group_actor import (  # noqa: F401
    _sanitize_actors_for_agent,
    _sanitize_group_doc_for_agent,
    actor_add,
    actor_list,
    actor_profile_list,
    actor_remove,
    actor_restart,
    actor_start,
    actor_stop,
    group_info,
    group_list,
    group_set_state,
    runtime_list,
)
from .handlers.cccc_capability import (  # noqa: F401
    capability_block,
    capability_enable,
    capability_import,
    capability_search,
    capability_state,
    capability_uninstall,
    capability_use,
)
from .handlers.cccc_automation import (  # noqa: F401
    _assert_action_trigger_compat,
    _assert_agent_notify_only_actions,
    _map_simple_automation_op_to_action,
    automation_manage,
    automation_state,
)
from .handlers.cccc_im import im_bind  # noqa: F401
from .handlers.cccc_space import (  # noqa: F401
    parse_space_artifact_args,
    parse_space_ingest_args,
    space_artifact,
    space_bind,
    space_capabilities,
    space_ingest,
    space_jobs,
    space_provider_auth,
    space_provider_credential_status,
    space_provider_credential_update,
    space_query,
    space_sources,
    space_status,
    space_sync,
)

# Existing extracted handler namespace dispatchers
from .handlers.context import (  # noqa: F401
    _handle_context_namespace as _handle_context_namespace_impl,
    agent_state_clear,
    agent_state_get,
    agent_state_update,
    context_get,
    context_sync,
    coordination_add_note,
    coordination_get,
    coordination_update_brief,
    task_delete,
    role_notes_clear,
    role_notes_get,
    role_notes_set,
    task_create,
    task_list,
    task_move,
    task_restore,
    task_update,
)
from .handlers.debug import (  # noqa: F401
    _handle_debug_namespace as _handle_debug_namespace_impl,
    _handle_terminal_namespace as _handle_terminal_namespace_impl,
    debug_snapshot,
    debug_tail_logs,
    terminal_tail,
)
from .handlers.headless import (  # noqa: F401
    _handle_headless_namespace as _handle_headless_namespace_impl,
    headless_ack_message,
    headless_set_status,
    headless_status,
)
from .handlers.memory import _handle_memory_namespace as _handle_memory_namespace_impl  # noqa: F401
from .handlers.notify import (  # noqa: F401
    _handle_notify_namespace as _handle_notify_namespace_impl,
    notify_ack,
    notify_send,
)
from .utils.help_markdown import _select_help_markdown
from .utils.space_args import _normalize_space_query_options_mcp


def _normalize_to_arg(raw: Any) -> Optional[List[str]]:
    """Normalise the ``to`` argument from MCP tool calls.

    Agents sometimes serialise the array as a JSON string (e.g.
    ``'["user"]'`` instead of ``["user"]``).  Detect and recover.
    """
    if isinstance(raw, list):
        return [str(x).strip() for x in raw if str(x).strip()] or None
    if isinstance(raw, str) and raw.strip():
        s = raw.strip()
        if s.startswith("["):
            try:
                parsed = json.loads(s)
                if isinstance(parsed, list):
                    return [str(x).strip() for x in parsed if str(x).strip()] or None
            except (json.JSONDecodeError, ValueError):
                pass
        return [s]
    return None


_BUILTIN_MCP_TOOL_NAMES = frozenset(str(spec.get("name") or "") for spec in MCP_TOOLS if isinstance(spec, dict))
_WEB_MODEL_ADVERTISED_TOOL_NAMES = frozenset(web_model_advertised_tool_names(_BUILTIN_MCP_TOOL_NAMES))
_WEB_MODEL_PEER_ALLOWED_TOOL_NAMES = frozenset(WEB_MODEL_CORE_TOOLS)


# =============================================================================
# Tool Call Routing
# =============================================================================


def _require_web_model_actor(group_id: str, actor_id: str) -> None:
    gid = str(group_id or "").strip()
    aid = str(actor_id or "").strip()
    if not gid or not aid:
        raise MCPError(code="missing_runtime_context", message="web-model local-power tools require group_id and actor_id")
    group = load_group(gid)
    if group is None:
        raise MCPError(code="group_not_found", message=f"group not found: {gid}")
    actor = find_actor(group, aid)
    if not isinstance(actor, dict):
        raise MCPError(code="actor_not_found", message=f"actor not found: {aid}")
    if str(actor.get("runtime") or "").strip().lower() != "web_model":
        raise MCPError(code="invalid_actor_runtime", message="local-power MCP tools are only available to web_model actors")


def _authorize_web_model_builtin_tool_call(name: str) -> None:
    """Keep Web Model schema stable while enforcing actor role at call time."""
    tool_name = str(name or "").strip()
    if tool_name in CODE_MODE_TOOL_NAMES and not code_mode_enabled():
        raise MCPError(
            code="code_mode_disabled",
            message=f"{tool_name} is disabled by CCCC_WEB_MODEL_CODE_MODE=0",
        )
    if tool_name not in _BUILTIN_MCP_TOOL_NAMES:
        return
    runtime_ctx = _runtime_context()
    gid = str(runtime_ctx.group_id or "").strip()
    aid = str(runtime_ctx.actor_id or "").strip()
    if not gid or not aid or aid == "user":
        return
    group = load_group(gid)
    if group is None:
        return
    try:
        actor = find_actor(group, aid)
    except Exception:
        return
    if not isinstance(actor, dict):
        return
    if str(actor.get("runtime") or "").strip().lower() != "web_model":
        return
    if tool_name not in _WEB_MODEL_ADVERTISED_TOOL_NAMES:
        raise MCPError(
            code="permission_denied",
            message=f"{tool_name} is not available to Web Model actors",
            details={"group_id": gid, "actor_id": aid, "tool_name": tool_name},
        )
    try:
        role = str(get_effective_role(group, aid) or "").strip().lower()
    except Exception:
        role = "peer"
    if role == "foreman":
        return
    if tool_name in _WEB_MODEL_PEER_ALLOWED_TOOL_NAMES:
        return
    raise MCPError(
        code="permission_denied",
        message=f"{tool_name} requires a Web Model foreman actor",
        details={"group_id": gid, "actor_id": aid, "role": role or "peer", "tool_name": tool_name},
    )


def _handle_cccc_namespace(name: str, arguments: Dict[str, Any]) -> Optional[Dict[str, Any]]:
    if name == "cccc_code_exec":
        gid = _resolve_group_id(arguments)
        aid = _resolve_self_actor_id(arguments)
        _require_web_model_actor(gid, aid)
        return code_exec_tool(arguments, nested_tool_caller=handle_tool_call, list_tools=list_tools_for_caller)

    if name == "cccc_code_wait":
        gid = _resolve_group_id(arguments)
        aid = _resolve_self_actor_id(arguments)
        _require_web_model_actor(gid, aid)
        return code_wait_tool(arguments, nested_tool_caller=handle_tool_call)

    # --- Help ---
    if name == "cccc_help":
        runtime_ctx = _runtime_context()
        gid = runtime_ctx.group_id
        aid = runtime_ctx.actor_id
        role: Optional[str] = None
        help_result: Dict[str, Any]

        def _safe_find_actor(group_obj: Any, actor_id: str | None) -> Optional[Dict[str, Any]]:
            if not actor_id:
                return None
            try:
                actor = find_actor(group_obj, actor_id)
            except Exception:
                return None
            return actor if isinstance(actor, dict) else None

        if gid:
            g = load_group(gid)
            if g is not None:
                if aid:
                    try:
                        role = get_effective_role(g, aid)
                    except Exception:
                        role = None
                actor = _safe_find_actor(g, aid)
                actor_is_pet = bool(isinstance(actor, dict) and is_pet_actor(actor))
                actor_is_voice_secretary = aid == "voice-secretary" or bool(isinstance(actor, dict) and is_voice_secretary_actor(actor))
                if actor_is_voice_secretary:
                    role = "voice_secretary"
                pf = read_group_prompt_file(g, HELP_FILENAME)
                if pf.found and isinstance(pf.content, str) and pf.content.strip():
                    help_result = {
                        "markdown": _append_runtime_help_addenda(
                            _select_help_markdown(
                                pf.content,
                                role=role,
                                actor_id=aid,
                                include_pet=actor_is_pet,
                                include_voice_secretary=actor_is_voice_secretary,
                            ),
                            group_id=gid,
                            actor_id=aid,
                        ),
                        "source": str(pf.path or ""),
                    }
                else:
                    help_result = {
                        "markdown": _append_runtime_help_addenda(
                            _select_help_markdown(
                                _CCCC_HELP_BUILTIN,
                                role=role,
                                actor_id=aid,
                                include_pet=actor_is_pet,
                                include_voice_secretary=actor_is_voice_secretary,
                            ),
                            group_id=gid,
                            actor_id=aid,
                        ),
                        "source": "cccc.resources/cccc-help.md",
                    }
            else:
                help_result = {
                    "markdown": _append_runtime_help_addenda(
                        _select_help_markdown(_CCCC_HELP_BUILTIN, role=role, actor_id=aid),
                        group_id=gid,
                        actor_id=aid,
                    ),
                    "source": "cccc.resources/cccc-help.md",
                }
        else:
            help_result = {
                "markdown": _append_runtime_help_addenda(
                    _select_help_markdown(_CCCC_HELP_BUILTIN, role=role, actor_id=aid),
                    group_id=gid,
                    actor_id=aid,
                ),
                "source": "cccc.resources/cccc-help.md",
            }
        if gid and aid:
            try:
                context_payload = _call_daemon_or_raise(
                    {"op": "context_get", "args": {"group_id": gid, "by": aid}},
                )
                help_result["context_hygiene"] = _build_context_hygiene_hint(
                    context=context_payload if isinstance(context_payload, dict) else {},
                    actor_id=aid,
                    group_id=gid,
                )
            except Exception:
                pass
        return help_result

    # --- Session bootstrap / project ---
    if name == "cccc_bootstrap":
        gid = _resolve_group_id(arguments)
        aid = _resolve_self_actor_id(arguments)
        return bootstrap(
            group_id=gid,
            actor_id=aid,
            inbox_limit=min(max(int(arguments.get("inbox_limit") or 50), 1), 1000),
            inbox_kind_filter=str(arguments.get("inbox_kind_filter") or "all"),
        )

    if name == "cccc_project_info":
        gid = _resolve_group_id(arguments)
        return project_info(group_id=gid)

    # --- Inbox ---
    if name == "cccc_inbox_list":
        gid = _resolve_group_id(arguments)
        aid = _resolve_self_actor_id(arguments)
        return inbox_list(
            group_id=gid,
            actor_id=aid,
            limit=min(max(int(arguments.get("limit") or 50), 1), 1000),
            kind_filter=str(arguments.get("kind_filter") or "all"),
        )

    if name == "cccc_inbox_mark_read":
        gid = _resolve_group_id(arguments)
        aid = _resolve_self_actor_id(arguments)
        action = str(arguments.get("action") or "read").strip().lower()
        if action == "read_all":
            return inbox_mark_all_read(
                group_id=gid,
                actor_id=aid,
                kind_filter=str(arguments.get("kind_filter") or "all"),
            )
        if action == "read":
            return inbox_mark_read(
                group_id=gid,
                actor_id=aid,
                event_id=str(arguments.get("event_id") or ""),
            )
        raise MCPError(code="invalid_request", message="cccc_inbox_mark_read action must be 'read' or 'read_all'")

    # --- Messaging ---
    if name == "cccc_message_send":
        gid = _resolve_group_id(arguments)
        aid = _resolve_self_actor_id(arguments)
        to_raw = arguments.get("to")
        refs_raw = arguments.get("refs")
        to_val = _normalize_to_arg(to_raw)
        refs_val = [item for item in refs_raw if isinstance(item, dict)] if isinstance(refs_raw, list) else None
        return message_send(
            group_id=gid,
            dst_group_id=arguments.get("dst_group_id"),
            actor_id=aid,
            text=str(arguments.get("text") or ""),
            to=to_val,
            priority=str(arguments.get("priority") or "normal"),
            reply_required=coerce_bool(arguments.get("reply_required"), default=False),
            refs=refs_val,
        )

    if name == "cccc_tracked_send":
        gid = _resolve_group_id(arguments)
        aid = _resolve_self_actor_id(arguments)
        to_raw = arguments.get("to")
        refs_raw = arguments.get("refs")
        checklist_raw = arguments.get("checklist")
        to_val = _normalize_to_arg(to_raw)
        refs_val = [item for item in refs_raw if isinstance(item, dict)] if isinstance(refs_raw, list) else None
        checklist_val = [item for item in checklist_raw if isinstance(item, dict)] if isinstance(checklist_raw, list) else None
        return tracked_send(
            group_id=gid,
            actor_id=aid,
            title=str(arguments.get("title") or ""),
            text=str(arguments.get("text") or ""),
            to=to_val,
            outcome=str(arguments.get("outcome") or ""),
            checklist=checklist_val,
            assignee=str(arguments.get("assignee") or ""),
            waiting_on=str(arguments.get("waiting_on") or ""),
            handoff_to=str(arguments.get("handoff_to") or ""),
            notes=str(arguments.get("notes") or ""),
            priority=str(arguments.get("priority") or "normal"),
            reply_required=coerce_bool(arguments.get("reply_required"), default=True),
            idempotency_key=str(arguments.get("idempotency_key") or ""),
            refs=refs_val,
        )

    if name == "cccc_message_reply":
        gid = _resolve_group_id(arguments)
        aid = _resolve_self_actor_id(arguments)
        to_raw = arguments.get("to")
        refs_raw = arguments.get("refs")
        to_val_reply = _normalize_to_arg(to_raw)
        refs_val_reply = [item for item in refs_raw if isinstance(item, dict)] if isinstance(refs_raw, list) else None
        reply_to = str(arguments.get("event_id") or arguments.get("reply_to") or "").strip()
        return message_reply(
            group_id=gid,
            actor_id=aid,
            reply_to=reply_to,
            text=str(arguments.get("text") or ""),
            to=to_val_reply,
            priority=str(arguments.get("priority") or "normal"),
            reply_required=coerce_bool(arguments.get("reply_required"), default=False),
            refs=refs_val_reply,
        )

    if name == "cccc_pet_decisions":
        gid = _resolve_group_id(arguments)
        aid = _resolve_self_actor_id(arguments)
        action = str(arguments.get("action") or "get").strip().lower()
        if action == "get":
            return _call_daemon_or_raise({"op": "pet_decisions_get", "args": {"group_id": gid, "actor_id": aid}})
        if action == "replace":
            return _call_daemon_or_raise(
                {
                    "op": "pet_decisions_replace",
                    "args": {
                        "group_id": gid,
                        "actor_id": aid,
                        "decisions": list(arguments.get("decisions") or []) if isinstance(arguments.get("decisions"), list) else [],
                    },
                }
            )
        if action == "clear":
            return _call_daemon_or_raise({"op": "pet_decisions_clear", "args": {"group_id": gid, "actor_id": aid}})
        raise MCPError(code="invalid_request", message="cccc_pet_decisions action must be get|replace|clear")

    if name == "cccc_voice_secretary_document":
        gid = _resolve_group_id(arguments)
        aid = _resolve_self_actor_id(arguments)
        if aid != VOICE_SECRETARY_ACTOR_ID:
            raise MCPError(code="permission_denied", message="cccc_voice_secretary_document is only available to the voice-secretary actor")
        action = str(arguments.get("action") or "list").strip().lower()
        if action == "list":
            if coerce_bool(arguments.get("include_content"), default=False):
                raise MCPError(
                    code="invalid_request",
                    message="cccc_voice_secretary_document list is compact; read repository markdown directly at document_path",
                )
            return _call_daemon_or_raise(
                {
                    "op": "assistant_voice_document_list",
                    "args": {
                        "group_id": gid,
                        "include_archived": coerce_bool(arguments.get("include_archived"), default=False),
                        "include_content": False,
                        "include_documents_by_id": False,
                        "include_documents_by_path": False,
                        "document_path": str(arguments.get("document_path") or arguments.get("workspace_path") or ""),
                    },
                }
            )
        if action == "read_new_input":
            return _call_daemon_or_raise(
                {
                    "op": "assistant_voice_document_input_read",
                    "args": {
                        "group_id": gid,
                        "by": "assistant:voice_secretary",
                    },
                }
            )
        if action == "create":
            if any(key in arguments for key in ("content", "new_source", "markdown")):
                raise MCPError(
                    code="invalid_request",
                    message="cccc_voice_secretary_document create only creates a markdown file; edit document content directly at document_path",
                )
            create_args = {
                "group_id": gid,
                "title": str(arguments.get("title") or ""),
                "create_new": True,
                "by": "assistant:voice_secretary",
            }
            return _call_daemon_or_raise(
                {
                    "op": "assistant_voice_document_save",
                    "args": create_args,
                }
            )
        if action == "archive":
            return _call_daemon_or_raise(
                {
                    "op": "assistant_voice_document_archive",
                    "args": {
                        "group_id": gid,
                        "document_path": str(arguments.get("document_path") or arguments.get("workspace_path") or ""),
                        "by": "assistant:voice_secretary",
                    },
                }
            )
        raise MCPError(code="invalid_request", message="cccc_voice_secretary_document action must be list|create|read_new_input|archive")

    if name == "cccc_voice_secretary_request":
        gid = _resolve_group_id(arguments)
        aid = _resolve_self_actor_id(arguments)
        if aid != VOICE_SECRETARY_ACTOR_ID:
            raise MCPError(code="permission_denied", message="cccc_voice_secretary_request is only available to the voice-secretary actor")
        action = str(arguments.get("action") or "handoff").strip().lower()
        if action == "report":
            return _call_daemon_or_raise(
                {
                    "op": "assistant_voice_instruction_feedback",
                    "args": {
                        "group_id": gid,
                        "request_id": str(arguments.get("request_id") or arguments.get("source_request_id") or ""),
                        "status": str(arguments.get("status") or ""),
                        "reply_text": str(arguments.get("reply_text") or arguments.get("result_text") or arguments.get("message") or ""),
                        "document_path": str(arguments.get("document_path") or arguments.get("workspace_path") or ""),
                        "artifact_paths": arguments.get("artifact_paths") or [],
                        "source_summary": str(arguments.get("source_summary") or ""),
                        "checked_at": str(arguments.get("checked_at") or ""),
                        "source_urls": arguments.get("source_urls") or [],
                        "by": VOICE_SECRETARY_ACTOR_ID,
                    },
                }
            )
        if action != "handoff":
            raise MCPError(code="invalid_request", message="cccc_voice_secretary_request action must be handoff or report")
        target = str(arguments.get("target") or "").strip()
        if not target:
            raise MCPError(code="invalid_request", message="cccc_voice_secretary_request target is required; use @foreman or one concrete actor id")
        return _call_daemon_or_raise(
            {
                "op": "assistant_voice_request",
                "args": {
                    "group_id": gid,
                    "action": "handoff",
                    "target": target,
                    "request_text": str(arguments.get("request_text") or ""),
                    "summary": str(arguments.get("summary") or ""),
                    "document_path": str(arguments.get("document_path") or arguments.get("workspace_path") or ""),
                    "source_event_id": str(arguments.get("source_event_id") or ""),
                    "source_request_id": str(arguments.get("source_request_id") or ""),
                    "priority": str(arguments.get("priority") or "normal"),
                    "requires_ack": coerce_bool(arguments.get("requires_ack"), default=True),
                    "by": VOICE_SECRETARY_ACTOR_ID,
                },
            }
        )

    if name == "cccc_voice_secretary_composer":
        gid = _resolve_group_id(arguments)
        aid = _resolve_self_actor_id(arguments)
        if aid != VOICE_SECRETARY_ACTOR_ID:
            raise MCPError(code="permission_denied", message="cccc_voice_secretary_composer is only available to the voice-secretary actor")
        action = str(arguments.get("action") or "submit_prompt_draft").strip().lower()
        if action != "submit_prompt_draft":
            raise MCPError(code="invalid_request", message="cccc_voice_secretary_composer action must be submit_prompt_draft")
        return _call_daemon_or_raise(
            {
                "op": "assistant_voice_prompt_draft_submit",
                "args": {
                    "group_id": gid,
                    "request_id": str(arguments.get("request_id") or ""),
                    "draft_text": str(arguments.get("draft_text") or ""),
                    "summary": str(arguments.get("summary") or ""),
                    "operation": str(arguments.get("operation") or ""),
                    "composer_snapshot_hash": str(arguments.get("composer_snapshot_hash") or ""),
                    "by": VOICE_SECRETARY_ACTOR_ID,
                },
            }
        )

    if name == "cccc_file":
        gid = _resolve_group_id(arguments)
        action = str(arguments.get("action") or "send").strip().lower()
        if action == "blob_path":
            return blob_path(group_id=gid, rel_path=str(arguments.get("rel_path") or ""))
        if action == "info":
            return blob_info(group_id=gid, rel_path=str(arguments.get("rel_path") or arguments.get("path") or ""))
        if action == "read":
            return blob_read(
                group_id=gid,
                rel_path=str(arguments.get("rel_path") or arguments.get("path") or ""),
                max_bytes=arguments.get("max_bytes") or 200000,
            )
        if action == "send":
            aid = _resolve_self_actor_id(arguments)
            to_raw = arguments.get("to")
            if isinstance(to_raw, list):
                to_val_file: Optional[List[str]] = [str(x).strip() for x in to_raw if str(x).strip()]
            elif isinstance(to_raw, str) and to_raw.strip():
                to_val_file = [to_raw.strip()]
            else:
                to_val_file = None
            return file_send(
                group_id=gid,
                actor_id=aid,
                path=str(arguments.get("path") or ""),
                text=str(arguments.get("text") or ""),
                to=to_val_file,
                priority=str(arguments.get("priority") or "normal"),
                reply_required=coerce_bool(arguments.get("reply_required"), default=False),
            )
        raise MCPError(code="invalid_request", message="cccc_file action must be send|blob_path|info|read")

    if name == "cccc_repo":
        gid = _resolve_group_id(arguments)
        action = str(arguments.get("action") or "info").strip().lower()
        if action not in {"info", "list", "list_dir", "read"}:
            raise MCPError(code="invalid_action", message="cccc_repo is read-only; use cccc_repo_edit/cccc_apply_patch for writes")
        return repo_tool(
            group_id=gid,
            action=action,
            path=str(arguments.get("path") or arguments.get("file_path") or ""),
            max_bytes=arguments.get("max_bytes") or 200000,
            limit=arguments.get("limit") or 200,
            offset=arguments.get("offset") or 1,
            depth=arguments.get("depth") or 2,
            start_line=arguments.get("start_line"),
            end_line=arguments.get("end_line"),
            include_hidden=coerce_bool(arguments.get("include_hidden"), default=False),
        )

    if name == "cccc_repo_edit":
        gid = _resolve_group_id(arguments)
        aid = _resolve_self_actor_id(arguments)
        _require_web_model_actor(gid, aid)
        action = str(arguments.get("action") or "").strip().lower()
        if not action:
            if str(arguments.get("patch") or "").strip():
                action = "apply_patch"
            elif isinstance(arguments.get("replacements"), list):
                action = "multi_replace"
            elif str(arguments.get("content") or ""):
                action = "write"
            else:
                action = "replace"
        if action not in {"replace", "multi_replace", "write", "apply_patch", "mkdir", "delete", "move"}:
            raise MCPError(code="invalid_action", message="cccc_repo_edit action must be replace|multi_replace|write|apply_patch|mkdir|delete|move")
        return repo_tool(
            group_id=gid,
            action=action,
            path=str(arguments.get("path") or arguments.get("file_path") or ""),
            dest_path=str(arguments.get("dest_path") or arguments.get("to_path") or ""),
            content=arguments.get("replacements") if action == "multi_replace" else str(arguments.get("content") or ""),
            old_text=str(arguments.get("old_text") or ""),
            new_text=str(arguments.get("new_text") or ""),
            expected_sha256=str(arguments.get("expected_sha256") or arguments.get("expected_hash") or ""),
            expected_replacements=arguments.get("expected_replacements"),
            replace_all=coerce_bool(arguments.get("replace_all"), default=False),
            patch=str(arguments.get("patch") or ""),
            recursive=coerce_bool(arguments.get("recursive"), default=False),
            exist_ok=coerce_bool(arguments.get("exist_ok"), default=True),
        )

    if name == "cccc_apply_patch":
        gid = _resolve_group_id(arguments)
        aid = _resolve_self_actor_id(arguments)
        _require_web_model_actor(gid, aid)
        return apply_codex_patch_tool(group_id=gid, patch=str(arguments.get("patch") or arguments.get("input") or ""))

    if name == "cccc_shell":
        gid = _resolve_group_id(arguments)
        aid = _resolve_self_actor_id(arguments)
        _require_web_model_actor(gid, aid)
        return shell_tool(
            group_id=gid,
            command=str(arguments.get("command") or ""),
            cwd=str(arguments.get("cwd") or "."),
            timeout_s=arguments.get("timeout_s") or 60,
            max_output_bytes=arguments.get("max_output_bytes") or 200000,
            env=arguments.get("env") if isinstance(arguments.get("env"), dict) else None,
        )

    if name == "cccc_exec_command":
        gid = _resolve_group_id(arguments)
        aid = _resolve_self_actor_id(arguments)
        _require_web_model_actor(gid, aid)
        return exec_command_tool(
            group_id=gid,
            command=str(arguments.get("command") or arguments.get("cmd") or ""),
            cwd=str(arguments.get("cwd") or arguments.get("workdir") or "."),
            yield_time_ms=arguments.get("yield_time_ms") or 1000,
            max_output_bytes=arguments.get("max_output_bytes") or 200000,
            timeout_s=arguments.get("timeout_s") or 600,
            env=arguments.get("env") if isinstance(arguments.get("env"), dict) else None,
        )

    if name == "cccc_write_stdin":
        gid = _resolve_group_id(arguments)
        aid = _resolve_self_actor_id(arguments)
        _require_web_model_actor(gid, aid)
        return write_stdin_tool(
            session_id=str(arguments.get("session_id") or ""),
            chars=str(arguments.get("chars") or ""),
            yield_time_ms=arguments.get("yield_time_ms") or 1000,
            max_output_bytes=arguments.get("max_output_bytes") or 200000,
            terminate=coerce_bool(arguments.get("terminate"), default=False),
        )

    if name == "cccc_git":
        gid = _resolve_group_id(arguments)
        aid = _resolve_self_actor_id(arguments)
        _require_web_model_actor(gid, aid)
        return git_tool(
            group_id=gid,
            action=str(arguments.get("action") or "status"),
            paths=arguments.get("paths"),
            path=str(arguments.get("path") or ""),
            message=str(arguments.get("message") or ""),
            staged=coerce_bool(arguments.get("staged"), default=False),
            count=arguments.get("count") or 20,
            all_changes=coerce_bool(arguments.get("all_changes"), default=False),
            max_output_bytes=arguments.get("max_output_bytes") or 200000,
        )

    if name == "cccc_presentation":
        gid = _resolve_group_id(arguments)
        aid = _resolve_self_actor_id(arguments)
        action = str(arguments.get("action") or "get").strip().lower()
        if action == "get":
            return presentation_get(group_id=gid)
        if action == "publish":
            return presentation_publish(
                group_id=gid,
                actor_id=aid,
                slot=str(arguments.get("slot") or "auto"),
                card_type=str(arguments.get("card_type") or ""),
                title=str(arguments.get("title") or ""),
                summary=str(arguments.get("summary") or ""),
                source_label=str(arguments.get("source_label") or ""),
                source_ref=str(arguments.get("source_ref") or ""),
                content=str(arguments.get("content") or ""),
                table=arguments.get("table"),
                path=str(arguments.get("path") or ""),
                url=str(arguments.get("url") or ""),
                blob_rel_path=str(arguments.get("blob_rel_path") or ""),
            )
        if action == "clear":
            return presentation_clear(
                group_id=gid,
                actor_id=aid,
                slot=str(arguments.get("slot") or ""),
                clear_all=coerce_bool(arguments.get("all"), default=False),
            )
        raise MCPError(code="invalid_request", message="cccc_presentation action must be 'get', 'publish', or 'clear'")

    # --- Group / Actor ---
    if name == "cccc_group":
        action = str(arguments.get("action") or "info").strip().lower()
        if action == "list":
            return group_list()
        if action == "info":
            gid = _resolve_group_id(arguments)
            return group_info(group_id=gid)
        if action == "set_state":
            gid = _resolve_group_id(arguments)
            by = _resolve_caller_actor_id(arguments)
            return group_set_state(
                group_id=gid,
                by=by,
                state=str(arguments.get("state") or ""),
            )
        raise MCPError(code="invalid_request", message="cccc_group action must be one of: info/list/set_state")

    if name == "cccc_actor":
        gid = _resolve_group_id(arguments)
        by = _resolve_caller_from_by(arguments)
        action = str(arguments.get("action") or "list").strip().lower()
        if action == "list":
            return actor_list(group_id=gid)
        if action == "profile_list":
            return actor_profile_list(group_id=gid, by=by)
        if action == "add":
            cmd_raw = arguments.get("command")
            env_raw = arguments.get("env")
            autoload_raw = arguments.get("capability_autoload")
            return actor_add(
                group_id=gid,
                by=by,
                actor_id=str(arguments.get("actor_id") or ""),
                runtime=str(arguments.get("runtime") or "codex"),
                runner=str(arguments.get("runner") or "pty"),
                title=str(arguments.get("title") or ""),
                command=list(cmd_raw) if isinstance(cmd_raw, list) else None,
                env=dict(env_raw) if isinstance(env_raw, dict) else None,
                profile_id=str(arguments.get("profile_id") or ""),
                capability_autoload=list(autoload_raw) if isinstance(autoload_raw, list) else None,
            )
        if action == "remove":
            target = str(arguments.get("actor_id") or "").strip() or by
            return actor_remove(group_id=gid, by=by, actor_id=target)
        if action == "start":
            return actor_start(
                group_id=gid,
                by=by,
                actor_id=str(arguments.get("actor_id") or ""),
            )
        if action == "stop":
            return actor_stop(
                group_id=gid,
                by=by,
                actor_id=str(arguments.get("actor_id") or ""),
            )
        if action == "restart":
            return actor_restart(
                group_id=gid,
                by=by,
                actor_id=str(arguments.get("actor_id") or ""),
            )
        raise MCPError(
            code="invalid_request",
            message="cccc_actor action must be one of: list/profile_list/add/remove/start/stop/restart",
        )

    if name == "cccc_runtime_list":
        return runtime_list()

    if name == "cccc_runtime_wait_next_turn":
        gid = _resolve_group_id(arguments)
        aid = _resolve_self_actor_id(arguments)
        try:
            limit = int(arguments.get("limit") or 20)
        except Exception:
            limit = 20
        return _call_daemon_or_raise(
            {
                "op": "web_model_runtime_wait_next_turn",
                "args": {
                    "group_id": gid,
                    "actor_id": aid,
                    "by": aid,
                    "limit": min(max(limit, 1), 20),
                    "kind_filter": str(arguments.get("kind_filter") or "all"),
                },
            },
            timeout_s=120.0,
        )

    if name == "cccc_runtime_complete_turn":
        gid = _resolve_group_id(arguments)
        aid = _resolve_self_actor_id(arguments)
        raw_event_ids = arguments.get("event_ids")
        event_ids = [str(item or "").strip() for item in raw_event_ids] if isinstance(raw_event_ids, list) else []
        return _call_daemon_or_raise(
            {
                "op": "web_model_runtime_complete_turn",
                "args": {
                    "group_id": gid,
                    "actor_id": aid,
                    "by": aid,
                    "turn_id": str(arguments.get("turn_id") or ""),
                    "event_ids": event_ids,
                    "latest_event_id": str(arguments.get("latest_event_id") or ""),
                    "status": str(arguments.get("status") or "done"),
                    "summary": str(arguments.get("summary") or ""),
                },
            },
            timeout_s=120.0,
        )

    # --- Capability ---
    if name == "cccc_capability_search":
        gid = _resolve_group_id(arguments)
        aid = _resolve_self_actor_id(arguments)
        return capability_search(
            group_id=gid,
            actor_id=aid,
            query=str(arguments.get("query") or ""),
            kind=str(arguments.get("kind") or ""),
            source_id=str(arguments.get("source_id") or ""),
            trust_tier=str(arguments.get("trust_tier") or ""),
            qualification_status=str(arguments.get("qualification_status") or ""),
            limit=min(max(int(arguments.get("limit") or 30), 1), 200),
            include_external=coerce_bool(arguments.get("include_external"), default=True),
        )

    if name == "cccc_capability_enable":
        gid = _resolve_group_id(arguments)
        by = _resolve_caller_actor_id(arguments)
        actor_id = str(arguments.get("actor_id") or by).strip()
        return capability_enable(
            group_id=gid,
            by=by,
            actor_id=actor_id,
            capability_id=str(arguments.get("capability_id") or ""),
            scope=str(arguments.get("scope") or "session"),
            enabled=coerce_bool(arguments.get("enabled"), default=True),
            cleanup=coerce_bool(arguments.get("cleanup"), default=False),
            reason=str(arguments.get("reason") or ""),
            ttl_seconds=min(max(int(arguments.get("ttl_seconds") or 3600), 60), 24 * 3600),
        )

    if name == "cccc_capability_block":
        gid = _resolve_group_id(arguments)
        by = _resolve_caller_actor_id(arguments)
        actor_id = str(arguments.get("actor_id") or by).strip()
        return capability_block(
            group_id=gid,
            by=by,
            actor_id=actor_id,
            capability_id=str(arguments.get("capability_id") or ""),
            scope=str(arguments.get("scope") or "group"),
            blocked=coerce_bool(arguments.get("blocked"), default=True),
            reason=str(arguments.get("reason") or ""),
            ttl_seconds=max(int(arguments.get("ttl_seconds") or 0), 0),
        )

    if name == "cccc_capability_state":
        gid = _resolve_group_id(arguments)
        aid = _resolve_self_actor_id(arguments)
        return capability_state(group_id=gid, actor_id=aid)

    if name == "cccc_capability_uninstall":
        gid = _resolve_group_id(arguments)
        by = _resolve_caller_actor_id(arguments)
        return capability_uninstall(
            group_id=gid,
            by=by,
            capability_id=str(arguments.get("capability_id") or ""),
            reason=str(arguments.get("reason") or ""),
        )

    if name == "cccc_capability_import":
        gid = _resolve_group_id(arguments)
        by = _resolve_caller_actor_id(arguments)
        actor_id = str(arguments.get("actor_id") or by).strip()
        raw_record = arguments.get("record")
        record = dict(raw_record) if isinstance(raw_record, dict) else {}
        return capability_import(
            group_id=gid,
            by=by,
            actor_id=actor_id,
            record=record,
            dry_run=coerce_bool(arguments.get("dry_run"), default=False),
            probe=coerce_bool(arguments.get("probe"), default=True),
            enable_after_import=coerce_bool(arguments.get("enable_after_import"), default=False),
            scope=str(arguments.get("scope") or "session"),
            ttl_seconds=min(max(int(arguments.get("ttl_seconds") or 3600), 60), 24 * 3600),
            reason=str(arguments.get("reason") or ""),
        )

    if name == "cccc_capability_use":
        gid = _resolve_group_id(arguments)
        by = _resolve_caller_actor_id(arguments)
        actor_id = str(arguments.get("actor_id") or by).strip()
        raw_tool_args = arguments.get("tool_arguments")
        tool_args = dict(raw_tool_args) if isinstance(raw_tool_args, dict) else {}
        return capability_use(
            group_id=gid,
            by=by,
            actor_id=actor_id,
            capability_id=str(arguments.get("capability_id") or ""),
            tool_name=str(arguments.get("tool_name") or ""),
            tool_arguments=tool_args,
            scope=str(arguments.get("scope") or "session"),
            ttl_seconds=min(max(int(arguments.get("ttl_seconds") or 3600), 60), 24 * 3600),
            reason=str(arguments.get("reason") or ""),
        )

    # --- Space ---
    if name == "cccc_space":
        gid = _resolve_group_id(arguments)
        provider = str(arguments.get("provider") or "notebooklm")
        lane = str(arguments.get("lane") or "").strip()
        action = str(arguments.get("action") or "status").strip().lower()
        if action == "status":
            return space_status(group_id=gid, provider=provider)
        if action == "capabilities":
            return space_capabilities(group_id=gid, provider=provider)
        if action in {"bind", "ingest", "query", "sources", "artifact", "jobs", "sync"} and not lane:
            raise MCPError(code="invalid_request", message="cccc_space requires explicit lane for bind/ingest/query/sources/artifact/jobs/sync")
        if action == "bind":
            by = _resolve_caller_from_by(arguments)
            return space_bind(
                group_id=gid,
                by=by,
                provider=provider,
                lane=(lane or "work"),
                action="bind",
                remote_space_id=str(arguments.get("remote_space_id") or ""),
            )
        if action == "ingest":
            by = _resolve_caller_from_by(arguments)
            parsed = parse_space_ingest_args(arguments)
            return space_ingest(
                group_id=gid,
                by=by,
                provider=provider,
                lane=(lane or "work"),
                kind=parsed["kind"],
                payload=parsed["payload"],
                idempotency_key=str(arguments.get("idempotency_key") or ""),
            )
        if action == "query":
            query_args = dict(arguments)
            query_args.pop("action", None)
            query_args.pop("sub_action", None)
            options = _normalize_space_query_options_mcp(query_args)
            return space_query(
                group_id=gid,
                provider=provider,
                lane=(lane or "work"),
                query=str(arguments.get("query") or ""),
                options=options,
            )
        if action == "sources":
            by = _resolve_caller_from_by(arguments)
            return space_sources(
                group_id=gid,
                by=by,
                provider=provider,
                lane=(lane or "work"),
                action=str(arguments.get("source_action") or arguments.get("sub_action") or "list"),
                source_id=str(arguments.get("source_id") or ""),
                new_title=str(arguments.get("new_title") or ""),
            )
        if action == "artifact":
            by = _resolve_caller_from_by(arguments)
            artifact_args = dict(arguments)
            sub_action = str(arguments.get("sub_action") or "").strip()
            if sub_action:
                artifact_args["action"] = sub_action
            else:
                artifact_args.pop("action", None)
            parsed = parse_space_artifact_args(artifact_args)
            return space_artifact(
                group_id=gid,
                by=by,
                provider=provider,
                lane=(lane or "work"),
                action=parsed["action"],
                kind=str(arguments.get("kind") or ""),
                options=parsed["options"],
                wait=coerce_bool(arguments.get("wait"), default=False),
                save_to_space=coerce_bool(arguments.get("save_to_space"), default=True),
                output_path=str(arguments.get("output_path") or ""),
                output_format=str(arguments.get("output_format") or ""),
                artifact_id=str(arguments.get("artifact_id") or ""),
                timeout_seconds=parsed["timeout_seconds"],
                initial_interval=parsed["initial_interval"],
                max_interval=parsed["max_interval"],
            )
        if action == "jobs":
            by = _resolve_caller_from_by(arguments)
            return space_jobs(
                group_id=gid,
                by=by,
                provider=provider,
                lane=(lane or "work"),
                action=str(arguments.get("job_action") or arguments.get("sub_action") or "list"),
                job_id=str(arguments.get("job_id") or ""),
                state=str(arguments.get("state") or ""),
                limit=min(max(int(arguments.get("limit") or 50), 1), 500),
            )
        if action == "sync":
            by = _resolve_caller_from_by(arguments)
            return space_sync(
                group_id=gid,
                by=by,
                provider=provider,
                lane=(lane or "work"),
                action=str(arguments.get("sync_action") or arguments.get("sub_action") or "run"),
                force=bool(arguments.get("force") is True),
            )
        if action == "provider_auth":
            by = _resolve_caller_from_by(arguments)
            timeout_raw = arguments.get("timeout_seconds")
            timeout_seconds = 900
            if timeout_raw is not None:
                try:
                    timeout_seconds = int(timeout_raw)
                except Exception:
                    raise MCPError(code="invalid_request", message="timeout_seconds must be an integer")
            return space_provider_auth(
                provider=provider,
                by=by,
                action=str(arguments.get("provider_action") or arguments.get("sub_action") or "status"),
                timeout_seconds=timeout_seconds,
                force_reauth=coerce_bool(arguments.get("force_reauth"), default=False),
            )
        if action == "provider_credential_status":
            by = _resolve_caller_from_by(arguments)
            return space_provider_credential_status(provider=provider, by=by)
        if action == "provider_credential_update":
            by = _resolve_caller_from_by(arguments)
            return space_provider_credential_update(
                provider=provider,
                by=by,
                auth_json=str(arguments.get("auth_json") or ""),
                clear=coerce_bool(arguments.get("clear"), default=False),
            )
        raise MCPError(
            code="invalid_request",
            message=(
                "cccc_space action must be one of: status/capabilities/bind/ingest/query/sources/"
                "artifact/jobs/sync/provider_auth/provider_credential_status/provider_credential_update"
            ),
        )

    # --- Automation ---
    if name == "cccc_automation":
        gid = _resolve_group_id(arguments)
        by = _resolve_caller_actor_id(arguments)
        action = str(arguments.get("action") or "state").strip().lower()
        if action == "state":
            return automation_state(group_id=gid, by=by)
        if action != "manage":
            raise MCPError(code="invalid_request", message="cccc_automation action must be 'state' or 'manage'")
        actions: List[Dict[str, Any]] = []
        mapped = _map_simple_automation_op_to_action(arguments)
        if isinstance(mapped, dict):
            actions.append(mapped)
        actions_raw = arguments.get("actions")
        if isinstance(actions_raw, list):
            for i, item in enumerate(actions_raw):
                if not isinstance(item, dict):
                    raise MCPError(code="invalid_request", message=f"actions[{i}] must be an object")
                actions.append(item)
        if not actions:
            raise MCPError(code="invalid_request", message="provide op (simple mode) or actions[] (advanced mode)")
        _assert_action_trigger_compat(actions)
        if by != "user":
            _assert_agent_notify_only_actions(actions)
        expected_version_raw = arguments.get("expected_version")
        expected_version: Optional[int] = None
        if expected_version_raw is not None:
            try:
                expected_version = int(expected_version_raw)
            except Exception:
                raise MCPError(code="invalid_request", message="expected_version must be an integer")
        return automation_manage(group_id=gid, by=by, actions=actions, expected_version=expected_version)

    if name == "cccc_im_bind":
        gid = _resolve_group_id(arguments)
        return im_bind(group_id=gid, key=str(arguments.get("key") or ""))

    return None


# =============================================================================
# Namespace Wrappers (delegate to extracted handler modules)
# =============================================================================


def _handle_context_namespace(name: str, arguments: Dict[str, Any]) -> Optional[Dict[str, Any]]:
    if name == "cccc_role_notes":
        gid = _resolve_group_id(arguments)
        by = _resolve_caller_from_by(arguments)
        action = str(arguments.get("action") or "get").strip().lower()
        target = str(arguments.get("target_actor_id") or "").strip() or None
        if action == "get":
            return role_notes_get(group_id=gid, caller_actor_id=by, target_actor_id=target)
        if action == "set":
            if not target:
                raise MCPError(code="invalid_request", message="target_actor_id is required for set")
            content = str(arguments.get("content") or "")
            return role_notes_set(group_id=gid, target_actor_id=target, content=content, by=by)
        if action == "clear":
            if not target:
                raise MCPError(code="invalid_request", message="target_actor_id is required for clear")
            return role_notes_clear(group_id=gid, target_actor_id=target, by=by)
        raise MCPError(code="invalid_request", message="cccc_role_notes action must be get|set|clear")
    return _handle_context_namespace_impl(
        name,
        arguments,
        resolve_group_id=_resolve_group_id,
        resolve_self_actor_id=_resolve_self_actor_id,
        coerce_bool=coerce_bool,
        context_get_fn=context_get,
        context_sync_fn=context_sync,
        coordination_get_fn=coordination_get,
        coordination_update_brief_fn=coordination_update_brief,
        coordination_add_note_fn=coordination_add_note,
        task_list_fn=task_list,
        task_create_fn=task_create,
        task_update_fn=task_update,
        task_move_fn=task_move,
        task_restore_fn=task_restore,
        task_delete_fn=task_delete,
        agent_state_get_fn=agent_state_get,
        agent_state_update_fn=agent_state_update,
        agent_state_clear_fn=agent_state_clear,
    )


def _handle_headless_namespace(name: str, arguments: Dict[str, Any]) -> Optional[Dict[str, Any]]:
    return _handle_headless_namespace_impl(
        name,
        arguments,
        resolve_group_id=_resolve_group_id,
        resolve_self_actor_id=_resolve_self_actor_id,
        headless_status_fn=headless_status,
        headless_set_status_fn=headless_set_status,
        headless_ack_message_fn=headless_ack_message,
    )


def _handle_notify_namespace(name: str, arguments: Dict[str, Any]) -> Optional[Dict[str, Any]]:
    return _handle_notify_namespace_impl(
        name,
        arguments,
        resolve_group_id=_resolve_group_id,
        resolve_self_actor_id=_resolve_self_actor_id,
        notify_send_fn=notify_send,
        notify_ack_fn=notify_ack,
        coerce_bool_fn=coerce_bool,
    )


def _handle_terminal_namespace(name: str, arguments: Dict[str, Any]) -> Optional[Dict[str, Any]]:
    return _handle_terminal_namespace_impl(
        name,
        arguments,
        resolve_group_id=_resolve_group_id,
        resolve_self_actor_id=_resolve_self_actor_id,
        terminal_tail_fn=terminal_tail,
        coerce_bool_fn=coerce_bool,
    )


def _handle_memory_namespace(name: str, arguments: Dict[str, Any]) -> Optional[Dict[str, Any]]:
    return _handle_memory_namespace_impl(
        name,
        arguments,
        resolve_group_id=_resolve_group_id,
        coerce_bool=coerce_bool,
        call_daemon_or_raise=_call_daemon_or_raise,
        mcp_error_cls=MCPError,
        build_memory_guide=build_memory_guide,
    )


def _handle_debug_namespace(name: str, arguments: Dict[str, Any]) -> Optional[Dict[str, Any]]:
    return _handle_debug_namespace_impl(
        name,
        arguments,
        resolve_group_id=_resolve_group_id,
        resolve_self_actor_id=_resolve_self_actor_id,
        debug_snapshot_fn=debug_snapshot,
        debug_tail_logs_fn=debug_tail_logs,
    )


# =============================================================================
# Public API
# =============================================================================


def handle_tool_call(name: str, arguments: Dict[str, Any]) -> Dict[str, Any]:
    """Handle MCP tool call."""
    _authorize_web_model_builtin_tool_call(name)
    for handler in (
        _handle_cccc_namespace,
        _handle_context_namespace,
        _handle_memory_namespace,
        _handle_headless_namespace,
        _handle_notify_namespace,
        _handle_terminal_namespace,
        _handle_debug_namespace,
    ):
        out = handler(name, arguments)
        if out is not None:
            return out
    # Dynamic capability tools are resolved by daemon capability runtime.
    runtime_ctx = _runtime_context()
    gid = runtime_ctx.group_id
    aid = runtime_ctx.actor_id
    if gid and aid:
        try:
            return _call_daemon_or_raise(
                {
                    "op": "capability_tool_call",
                    "args": {
                        "group_id": gid,
                        "actor_id": aid,
                        "by": aid,
                        "tool_name": str(name or ""),
                        "arguments": arguments if isinstance(arguments, dict) else {},
                    },
                },
                timeout_s=120.0,
            )
        except MCPError as e:
            if e.code != "capability_tool_not_found":
                raise
    raise MCPError(code="unknown_tool", message=f"unknown tool: {name}")


def list_tools_for_caller() -> List[Dict[str, Any]]:
    """Resolve visible tool specs for current caller scope.

    Behavior:
    1) full profile opt-out via CCCC_MCP_TOOL_PROFILE=full
    2) default: core + enabled capability packs from daemon capability_state
    3) daemon failure fallback: best-effort built-in actor surface
    """
    runtime_ctx = _runtime_context()
    gid = runtime_ctx.group_id
    aid = runtime_ctx.actor_id
    profile = str(os.environ.get("CCCC_MCP_TOOL_PROFILE") or "").strip().lower()
    state: Dict[str, Any] = {}
    if gid and aid:
        try:
            state = _call_daemon_or_raise(
                {
                    "op": "capability_state",
                    "args": {"group_id": gid, "actor_id": aid, "by": aid},
                },
                timeout_s=4.0,
            )
        except Exception:
            state = {}

    # Determine actor role for admin tool gating.
    actor_role = ""
    actor_is_pet = False
    actor_is_voice_secretary = False
    actor_is_web_model = False
    builtin_enabled_fallback: List[str] = []
    if gid and aid and aid != "user":
        actor_is_voice_secretary = aid == VOICE_SECRETARY_ACTOR_ID
        try:
            group = load_group(gid)
            actor_role = str(get_effective_role(group, aid) or "").strip().lower()
            actor = find_actor(group, aid)
            if isinstance(actor, dict):
                actor_is_pet = is_pet_actor(actor)
                actor_is_voice_secretary = actor_is_voice_secretary or is_voice_secretary_actor(actor)
                actor_is_web_model = str(actor.get("runtime") or "").strip().lower() == "web_model"
                autoload = actor.get("capability_autoload") if isinstance(actor.get("capability_autoload"), list) else []
                builtin_enabled_fallback = [
                    str(cap_id or "").strip()
                    for cap_id in autoload
                    if str(cap_id or "").strip() in BUILTIN_CAPABILITY_PACKS
                ]
        except Exception:
            pass
    admin_excluded = set(CORE_ADMIN_TOOLS) if actor_role == "peer" else set()

    if actor_is_web_model:
        names = set(_WEB_MODEL_ADVERTISED_TOOL_NAMES)
        if not code_mode_enabled():
            names -= CODE_MODE_TOOL_NAMES
        return [spec for spec in MCP_TOOLS if str(spec.get("name") or "") in names]

    if profile == "full":
        visible = {str(spec.get("name") or "").strip() for spec in MCP_TOOLS if isinstance(spec, dict)}
        visible -= admin_excluded
    else:
        tools_raw = state.get("visible_tools") if isinstance(state, dict) else []
        if not isinstance(tools_raw, list):
            tools_raw = []
        visible = {str(x).strip() for x in tools_raw if str(x).strip()}
        if not visible:
            visible = set(
                resolve_visible_tool_names(
                    builtin_enabled_fallback,
                    actor_role=actor_role,
                    is_pet=actor_is_pet,
                    is_voice_secretary=actor_is_voice_secretary,
                    is_web_model=actor_is_web_model,
                )
            ) - admin_excluded

    dynamic_raw = state.get("dynamic_tools") if isinstance(state, dict) else []
    dynamic_specs: List[Dict[str, Any]] = []
    if isinstance(dynamic_raw, list):
        for item in dynamic_raw:
            if not isinstance(item, dict):
                continue
            dname = str(item.get("name") or "").strip()
            if not dname:
                continue
            schema = item.get("inputSchema")
            if not isinstance(schema, dict):
                schema = {"type": "object", "properties": {}, "required": []}
            dynamic_specs.append(
                {
                    "name": dname,
                    "description": str(item.get("description") or "").strip()
                    or f"Dynamic capability tool ({dname})",
                    "inputSchema": schema,
                }
            )

    out = [spec for spec in MCP_TOOLS if str(spec.get("name") or "") in visible]
    existing = {str(spec.get("name") or "") for spec in out}
    for spec in dynamic_specs:
        dname = str(spec.get("name") or "")
        if dname and dname not in existing:
            out.append(spec)
            existing.add(dname)
    return out
