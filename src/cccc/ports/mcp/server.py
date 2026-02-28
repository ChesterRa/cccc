"""
CCCC MCP Server - IM-style Agent Collaboration Tools

Static MCP surface (31 entries):
- cccc_help / cccc_bootstrap / cccc_project_info
- cccc_inbox_list / cccc_inbox_mark_read
- cccc_message_send / cccc_message_reply
- cccc_file / cccc_group / cccc_actor / cccc_runtime_list
- cccc_capability_search / cccc_capability_enable / cccc_capability_block / cccc_capability_state / cccc_capability_uninstall / cccc_capability_use
- cccc_space / cccc_automation
- cccc_context_get / cccc_context_sync / cccc_context_admin / cccc_task / cccc_context_agent
- cccc_memory / cccc_memory_admin
- cccc_headless / cccc_notify / cccc_terminal / cccc_debug
- cccc_im_bind

All operations go through daemon IPC to ensure single-writer principle.
"""

from __future__ import annotations

import os
from typing import Any, Dict, List, Optional

# Kernel/util imports needed by routing
from ...kernel.actors import get_effective_role
from ...kernel.blobs import resolve_blob_attachment_path, store_blob_bytes
from ...kernel.group import load_group
from ...kernel.capabilities import CORE_TOOL_NAMES
from ...kernel.memory_guide import build_memory_guide
from ...kernel.prompt_files import HELP_FILENAME, read_group_prompt_file
from ...util.conv import coerce_bool

# Common MCP utilities
from .common import (
    MCPError,
    _call_daemon_or_raise,
    _env_str,
    _resolve_caller_actor_id,
    _resolve_caller_from_by,
    _resolve_group_id,
    _resolve_self_actor_id,
)
from .toolspecs import MCP_TOOLS

# ---------------------------------------------------------------------------
# Handler re-exports for backward compatibility (tests import from server.py)
# ---------------------------------------------------------------------------
from .handlers.cccc_core import (  # noqa: F401
    _CCCC_HELP_BUILTIN,
    _build_context_hygiene_hint,
    _append_runtime_skill_digest,
    bootstrap,
    inbox_list,
    inbox_mark_all_read,
    inbox_mark_read,
    project_info,
)
from .handlers.cccc_messaging import (  # noqa: F401
    blob_path,
    file_send,
    message_reply,
    message_send,
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
    context_get,
    context_agent_clear,
    context_agent_update,
    context_sync,
    overview_manual_update,
    task_create,
    task_list,
    task_move,
    task_restore,
    task_status,
    task_update,
    vision_update,
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


# =============================================================================
# Tool Call Routing
# =============================================================================


def _handle_cccc_namespace(name: str, arguments: Dict[str, Any]) -> Optional[Dict[str, Any]]:
    # --- Help ---
    if name == "cccc_help":
        gid = _env_str("CCCC_GROUP_ID")
        aid = _env_str("CCCC_ACTOR_ID")
        role: Optional[str] = None
        help_result: Dict[str, Any]
        if gid:
            g = load_group(gid)
            if g is not None:
                if aid:
                    try:
                        role = get_effective_role(g, aid)
                    except Exception:
                        role = None
                pf = read_group_prompt_file(g, HELP_FILENAME)
                if pf.found and isinstance(pf.content, str) and pf.content.strip():
                    help_result = {
                        "markdown": _append_runtime_skill_digest(
                            _select_help_markdown(pf.content, role=role, actor_id=aid),
                            group_id=gid,
                            actor_id=aid,
                        ),
                        "source": str(pf.path or ""),
                    }
                else:
                    help_result = {
                        "markdown": _append_runtime_skill_digest(
                            _select_help_markdown(_CCCC_HELP_BUILTIN, role=role, actor_id=aid),
                            group_id=gid,
                            actor_id=aid,
                        ),
                        "source": "cccc.resources/cccc-help.md",
                    }
            else:
                help_result = {
                    "markdown": _append_runtime_skill_digest(
                        _select_help_markdown(_CCCC_HELP_BUILTIN, role=role, actor_id=aid),
                        group_id=gid,
                        actor_id=aid,
                    ),
                    "source": "cccc.resources/cccc-help.md",
                }
        else:
            help_result = {
                "markdown": _append_runtime_skill_digest(
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
            ledger_tail_limit=min(max(int(arguments.get("ledger_tail_limit") or 10), 0), 1000),
            ledger_tail_max_chars=min(max(int(arguments.get("ledger_tail_max_chars") or 8000), 0), 100000),
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
        if isinstance(to_raw, list):
            to_val: Optional[List[str]] = [str(x).strip() for x in to_raw if str(x).strip()]
        elif isinstance(to_raw, str) and to_raw.strip():
            to_val = [to_raw.strip()]
        else:
            to_val = None
        return message_send(
            group_id=gid,
            dst_group_id=arguments.get("dst_group_id"),
            actor_id=aid,
            text=str(arguments.get("text") or ""),
            to=to_val,
            priority=str(arguments.get("priority") or "normal"),
            reply_required=coerce_bool(arguments.get("reply_required"), default=False),
        )

    if name == "cccc_message_reply":
        gid = _resolve_group_id(arguments)
        aid = _resolve_self_actor_id(arguments)
        to_raw = arguments.get("to")
        if isinstance(to_raw, list):
            to_val_reply: Optional[List[str]] = [str(x).strip() for x in to_raw if str(x).strip()]
        elif isinstance(to_raw, str) and to_raw.strip():
            to_val_reply = [to_raw.strip()]
        else:
            to_val_reply = None
        reply_to = str(arguments.get("event_id") or arguments.get("reply_to") or "").strip()
        return message_reply(
            group_id=gid,
            actor_id=aid,
            reply_to=reply_to,
            text=str(arguments.get("text") or ""),
            to=to_val_reply,
            priority=str(arguments.get("priority") or "normal"),
            reply_required=coerce_bool(arguments.get("reply_required"), default=False),
        )

    if name == "cccc_file":
        gid = _resolve_group_id(arguments)
        aid = _resolve_self_actor_id(arguments)
        action = str(arguments.get("action") or "send").strip().lower()
        if action == "blob_path":
            return blob_path(group_id=gid, rel_path=str(arguments.get("rel_path") or ""))
        if action == "send":
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
        raise MCPError(code="invalid_request", message="cccc_file action must be 'send' or 'blob_path'")

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
            return actor_profile_list(by=by)
        if action == "add":
            cmd_raw = arguments.get("command")
            env_raw = arguments.get("env")
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
        by = _resolve_caller_from_by(arguments)
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
        by = _resolve_caller_from_by(arguments)
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
        by = _resolve_caller_from_by(arguments)
        return capability_uninstall(
            group_id=gid,
            by=by,
            capability_id=str(arguments.get("capability_id") or ""),
            reason=str(arguments.get("reason") or ""),
        )

    if name == "cccc_capability_use":
        gid = _resolve_group_id(arguments)
        by = _resolve_caller_from_by(arguments)
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
        action = str(arguments.get("action") or "status").strip().lower()
        if action == "status":
            return space_status(group_id=gid, provider=provider)
        if action == "capabilities":
            return space_capabilities(group_id=gid, provider=provider)
        if action == "bind":
            by = _resolve_caller_from_by(arguments)
            return space_bind(
                group_id=gid,
                by=by,
                provider=provider,
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
                query=str(arguments.get("query") or ""),
                options=options,
            )
        if action == "sources":
            by = _resolve_caller_from_by(arguments)
            return space_sources(
                group_id=gid,
                by=by,
                provider=provider,
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
    return _handle_context_namespace_impl(
        name,
        arguments,
        resolve_group_id=_resolve_group_id,
        resolve_self_actor_id=_resolve_self_actor_id,
        coerce_bool=coerce_bool,
        context_get_fn=context_get,
        context_sync_fn=context_sync,
        vision_update_fn=vision_update,
        overview_manual_update_fn=overview_manual_update,
        task_list_fn=task_list,
        task_create_fn=task_create,
        task_update_fn=task_update,
        task_status_fn=task_status,
        task_move_fn=task_move,
        task_restore_fn=task_restore,
        context_agent_update_fn=context_agent_update,
        context_agent_clear_fn=context_agent_clear,
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
    gid = _env_str("CCCC_GROUP_ID")
    aid = _env_str("CCCC_ACTOR_ID")
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
    3) daemon failure fallback: core-only
    """
    gid = _env_str("CCCC_GROUP_ID")
    aid = _env_str("CCCC_ACTOR_ID")
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

    if profile == "full":
        visible = {str(spec.get("name") or "").strip() for spec in MCP_TOOLS if isinstance(spec, dict)}
    else:
        tools_raw = state.get("visible_tools") if isinstance(state, dict) else []
        if not isinstance(tools_raw, list):
            tools_raw = []
        visible = {str(x).strip() for x in tools_raw if str(x).strip()}
        if not visible:
            visible = set(CORE_TOOL_NAMES)

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
