"""
CCCC MCP Server - IM-style Agent Collaboration Tools

Core tools exposed to agents:

cccc.* namespace (collaboration control plane):
- cccc_help: CCCC help playbook (authoritative; returns effective CCCC_HELP.md if present)
- cccc_bootstrap: One-call session bootstrap (group+help+project+context+inbox)
- cccc_inbox_list: Get unread messages (supports kind_filter)
- cccc_inbox_mark_read: Mark messages as read
- cccc_inbox_mark_all_read: Mark all current unread messages as read
- cccc_message_send: Send message
- cccc_message_reply: Reply to message
- cccc_file_send: Send a local file as an attachment
- cccc_blob_path: Resolve attachment blob path
- cccc_group_info: Get group info
- cccc_actor_list: Get actor list
- cccc_actor_profile_list: List reusable Actor Profiles
- cccc_actor_add: Add new actor (foreman only)
- cccc_actor_remove: Remove an actor (foreman/peer can only remove themselves)
- cccc_actor_start: Start actor
- cccc_actor_stop: Stop actor
- cccc_runtime_list: List available agent runtimes
- cccc_capability_search: Search capability registry (progressive disclosure)
- cccc_capability_enable: Enable/disable capability packs by scope
- cccc_capability_state: Read effective visible tool surface
- cccc_capability_uninstall: Uninstall cached external capability + revoke bindings
- cccc_capability_use: One-step enable+call convenience helper
- cccc_space_status: Read Group Space provider/binding/queue status
- cccc_space_capabilities: Read Group Space local file policy + ingest schema matrix
- cccc_space_bind: Bind or unbind Group Space provider mapping
- cccc_space_ingest: Enqueue and execute Group Space ingest job
- cccc_space_query: Query provider-backed Group Space knowledge
- cccc_space_sources: List/refresh/rename/delete provider sources for the bound notebook
- cccc_space_artifact: List/generate/download provider artifacts (can auto-save to repo space/artifacts)
- cccc_space_jobs: List/retry/cancel Group Space jobs
- cccc_space_sync: Read or run repo/space synchronization reconcile
- cccc_space_provider_auth: Control provider auth flow (status/start/cancel)
- cccc_space_provider_credential_status: Read provider credential status (masked)
- cccc_space_provider_credential_update: Update/clear provider credential
- cccc_automation_state: Read automation reminders/status visible to caller
- cccc_automation_manage: Manage automation reminders (MCP actor writes are notify-only)
- cccc_project_info: Get PROJECT.md content (project goals/constraints)

context.* namespace (state sync):
- cccc_context_get: Get full context
- cccc_context_sync: Batch sync operations
- cccc_vision_update / cccc_sketch_update: Vision/sketch
- cccc_milestone_*: Milestone management (create/update/complete/remove)
- cccc_task_*: Task management (list/create/update/delete)
- cccc_note_*: Note management (add/update/remove)
- cccc_reference_*: Reference management (add/update/remove)
- cccc_presence_*: Presence status (get/update/clear)

headless.* namespace (headless runner control):
- cccc_headless_status: Get headless session status
- cccc_headless_set_status: Update status (idle/working/waiting/stopped)
- cccc_headless_ack_message: Acknowledge processed message

notify.* namespace (system notifications, separate from chat):
- cccc_notify_send: Send system notification
- cccc_notify_ack: Acknowledge system notification

terminal.* namespace (diagnostics):
- cccc_terminal_tail: Tail an actor terminal transcript (group policy)

debug.* namespace (developer mode diagnostics):
- cccc_debug_snapshot: Get a structured debug snapshot (dev mode)
- cccc_debug_tail_logs: Tail local CCCC logs (dev mode)

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
    context_sync,
    milestone_complete,
    milestone_create,
    milestone_update,
    note_add,
    note_remove,
    note_update,
    presence_clear,
    presence_get,
    presence_update,
    reference_add,
    reference_remove,
    reference_update,
    sketch_update,
    task_create,
    task_list,
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
                    return {
                        "markdown": _append_runtime_skill_digest(
                            _select_help_markdown(pf.content, role=role, actor_id=aid),
                            group_id=gid,
                            actor_id=aid,
                        ),
                        "source": str(pf.path or ""),
                    }
        return {
            "markdown": _append_runtime_skill_digest(
                _select_help_markdown(_CCCC_HELP_BUILTIN, role=role, actor_id=aid),
                group_id=gid,
                actor_id=aid,
            ),
            "source": "cccc.resources/cccc-help.md",
        }

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

    if name == "cccc_inbox_mark_read":
        gid = _resolve_group_id(arguments)
        aid = _resolve_self_actor_id(arguments)
        return inbox_mark_read(
            group_id=gid,
            actor_id=aid,
            event_id=str(arguments.get("event_id") or ""),
        )

    if name == "cccc_inbox_mark_all_read":
        gid = _resolve_group_id(arguments)
        aid = _resolve_self_actor_id(arguments)
        return inbox_mark_all_read(
            group_id=gid,
            actor_id=aid,
            kind_filter=str(arguments.get("kind_filter") or "all"),
        )

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

    if name == "cccc_file_send":
        gid = _resolve_group_id(arguments)
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

    if name == "cccc_blob_path":
        gid = _resolve_group_id(arguments)
        return blob_path(
            group_id=gid,
            rel_path=str(arguments.get("rel_path") or ""),
        )

    # --- Group / Actor ---
    if name == "cccc_group_info":
        gid = _resolve_group_id(arguments)
        return group_info(group_id=gid)

    if name == "cccc_group_list":
        return group_list()

    if name == "cccc_actor_list":
        gid = _resolve_group_id(arguments)
        return actor_list(group_id=gid)

    if name == "cccc_actor_profile_list":
        by = _resolve_caller_from_by(arguments)
        return actor_profile_list(by=by)

    if name == "cccc_actor_add":
        gid = _resolve_group_id(arguments)
        by = _resolve_caller_from_by(arguments)
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

    if name == "cccc_actor_remove":
        gid = _resolve_group_id(arguments)
        by = _resolve_caller_from_by(arguments)
        target = str(arguments.get("actor_id") or "").strip() or by
        return actor_remove(group_id=gid, by=by, actor_id=target)

    if name == "cccc_actor_start":
        gid = _resolve_group_id(arguments)
        by = _resolve_caller_from_by(arguments)
        return actor_start(
            group_id=gid,
            by=by,
            actor_id=str(arguments.get("actor_id") or ""),
        )

    if name == "cccc_actor_stop":
        gid = _resolve_group_id(arguments)
        by = _resolve_caller_from_by(arguments)
        return actor_stop(
            group_id=gid,
            by=by,
            actor_id=str(arguments.get("actor_id") or ""),
        )

    if name == "cccc_actor_restart":
        gid = _resolve_group_id(arguments)
        by = _resolve_caller_from_by(arguments)
        return actor_restart(
            group_id=gid,
            by=by,
            actor_id=str(arguments.get("actor_id") or ""),
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
            approve=coerce_bool(arguments.get("approve"), default=False),
            reason=str(arguments.get("reason") or ""),
            ttl_seconds=min(max(int(arguments.get("ttl_seconds") or 3600), 60), 24 * 3600),
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
            approve=coerce_bool(arguments.get("approve"), default=False),
            ttl_seconds=min(max(int(arguments.get("ttl_seconds") or 3600), 60), 24 * 3600),
            reason=str(arguments.get("reason") or ""),
        )

    # --- Space ---
    if name == "cccc_space_status":
        gid = _resolve_group_id(arguments)
        return space_status(
            group_id=gid,
            provider=str(arguments.get("provider") or "notebooklm"),
        )

    if name == "cccc_space_capabilities":
        gid = _resolve_group_id(arguments)
        return space_capabilities(
            group_id=gid,
            provider=str(arguments.get("provider") or "notebooklm"),
        )

    if name == "cccc_space_bind":
        gid = _resolve_group_id(arguments)
        by = _resolve_caller_from_by(arguments)
        return space_bind(
            group_id=gid,
            by=by,
            provider=str(arguments.get("provider") or "notebooklm"),
            action=str(arguments.get("action") or "bind"),
            remote_space_id=str(arguments.get("remote_space_id") or ""),
        )

    if name == "cccc_space_ingest":
        gid = _resolve_group_id(arguments)
        by = _resolve_caller_from_by(arguments)
        parsed = parse_space_ingest_args(arguments)
        return space_ingest(
            group_id=gid,
            by=by,
            provider=str(arguments.get("provider") or "notebooklm"),
            kind=parsed["kind"],
            payload=parsed["payload"],
            idempotency_key=str(arguments.get("idempotency_key") or ""),
        )

    if name == "cccc_space_query":
        gid = _resolve_group_id(arguments)
        options = _normalize_space_query_options_mcp(arguments)
        return space_query(
            group_id=gid,
            provider=str(arguments.get("provider") or "notebooklm"),
            query=str(arguments.get("query") or ""),
            options=options,
        )

    if name == "cccc_space_sources":
        gid = _resolve_group_id(arguments)
        by = _resolve_caller_from_by(arguments)
        return space_sources(
            group_id=gid,
            by=by,
            provider=str(arguments.get("provider") or "notebooklm"),
            action=str(arguments.get("action") or "list"),
            source_id=str(arguments.get("source_id") or ""),
            new_title=str(arguments.get("new_title") or ""),
        )

    if name == "cccc_space_artifact":
        gid = _resolve_group_id(arguments)
        by = _resolve_caller_from_by(arguments)
        parsed = parse_space_artifact_args(arguments)
        return space_artifact(
            group_id=gid,
            by=by,
            provider=str(arguments.get("provider") or "notebooklm"),
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

    if name == "cccc_space_jobs":
        gid = _resolve_group_id(arguments)
        by = _resolve_caller_from_by(arguments)
        return space_jobs(
            group_id=gid,
            by=by,
            provider=str(arguments.get("provider") or "notebooklm"),
            action=str(arguments.get("action") or "list"),
            job_id=str(arguments.get("job_id") or ""),
            state=str(arguments.get("state") or ""),
            limit=min(max(int(arguments.get("limit") or 50), 1), 500),
        )

    if name == "cccc_space_sync":
        gid = _resolve_group_id(arguments)
        by = _resolve_caller_from_by(arguments)
        return space_sync(
            group_id=gid,
            by=by,
            provider=str(arguments.get("provider") or "notebooklm"),
            action=str(arguments.get("action") or "run"),
            force=bool(arguments.get("force") is True),
        )

    if name == "cccc_space_provider_auth":
        by = _resolve_caller_from_by(arguments)
        timeout_raw = arguments.get("timeout_seconds")
        timeout_seconds = 900
        if timeout_raw is not None:
            try:
                timeout_seconds = int(timeout_raw)
            except Exception:
                raise MCPError(code="invalid_request", message="timeout_seconds must be an integer")
        return space_provider_auth(
            provider=str(arguments.get("provider") or "notebooklm"),
            by=by,
            action=str(arguments.get("action") or "status"),
            timeout_seconds=timeout_seconds,
        )

    if name == "cccc_space_provider_credential_status":
        by = _resolve_caller_from_by(arguments)
        return space_provider_credential_status(
            provider=str(arguments.get("provider") or "notebooklm"),
            by=by,
        )

    if name == "cccc_space_provider_credential_update":
        by = _resolve_caller_from_by(arguments)
        return space_provider_credential_update(
            provider=str(arguments.get("provider") or "notebooklm"),
            by=by,
            auth_json=str(arguments.get("auth_json") or ""),
            clear=coerce_bool(arguments.get("clear"), default=False),
        )

    # --- Group State / Automation ---
    if name == "cccc_group_set_state":
        gid = _resolve_group_id(arguments)
        by = _resolve_caller_actor_id(arguments)
        return group_set_state(
            group_id=gid,
            by=by,
            state=str(arguments.get("state") or ""),
        )

    if name == "cccc_automation_state":
        gid = _resolve_group_id(arguments)
        by = _resolve_caller_actor_id(arguments)
        return automation_state(group_id=gid, by=by)

    if name == "cccc_automation_manage":
        gid = _resolve_group_id(arguments)
        by = _resolve_caller_actor_id(arguments)
        actions: List[Dict[str, Any]] = []
        mapped = _map_simple_automation_op_to_action(arguments)
        if isinstance(mapped, dict):
            actions.append(mapped)
        actions_raw = arguments.get("actions")
        if isinstance(actions_raw, list):
            for i, action in enumerate(actions_raw):
                if not isinstance(action, dict):
                    raise MCPError(code="invalid_request", message=f"actions[{i}] must be an object")
                actions.append(action)
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

    # --- Misc ---
    if name == "cccc_project_info":
        gid = _resolve_group_id(arguments)
        return project_info(group_id=gid)

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
        sketch_update_fn=sketch_update,
        milestone_create_fn=milestone_create,
        milestone_update_fn=milestone_update,
        milestone_complete_fn=milestone_complete,
        task_list_fn=task_list,
        task_create_fn=task_create,
        task_update_fn=task_update,
        note_add_fn=note_add,
        note_update_fn=note_update,
        note_remove_fn=note_remove,
        reference_add_fn=reference_add,
        reference_update_fn=reference_update,
        reference_remove_fn=reference_remove,
        presence_get_fn=presence_get,
        presence_update_fn=presence_update,
        presence_clear_fn=presence_clear,
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


