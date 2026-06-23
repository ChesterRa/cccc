"""Group Bridge remote MCP endpoint tools.

This module is intentionally separate from the normal MCP dispatcher. Group
Bridge calls are authorized by a group_bridge trust grant, not by a Web Model
actor identity, so local-power operations call the lower-level handlers after
their bridge access check passes.
"""

from __future__ import annotations

import json
import time
from copy import deepcopy
from dataclasses import dataclass
from typing import Any, Dict, List, Optional

from ... import __version__
from ...kernel.group_bridge import pairing as pairing_kernel
from ...kernel.group import load_group
from ...kernel.ledger import append_event
from ...util.conv import coerce_bool
from .common import MCPError
from .handlers.cccc_repo import (
    apply_codex_patch_tool,
    exec_command_tool,
    git_tool,
    repo_search_tool,
    repo_tool,
    shell_tool,
    write_stdin_tool,
)
from .handlers.context import context_get
from .toolspecs import MCP_TOOLS

_SUPPORTED_PROTOCOL_VERSIONS = ("2025-06-18", "2024-11-05")
_DEFAULT_PROTOCOL_VERSION = "2024-11-05"

REMOTE_ACCESS_TOOL = "cccc_remote_access"
REMOTE_READ_TOOLS = frozenset({"cccc_remote_context", "cccc_remote_repo", "cccc_remote_git"})
REMOTE_FULL_TOOLS = frozenset(
    {
        "cccc_remote_repo_edit",
        "cccc_remote_apply_patch",
        "cccc_remote_shell",
        "cccc_remote_exec_command",
        "cccc_remote_write_stdin",
    }
)

_READ_GIT_ACTIONS = frozenset({"status", "diff", "log"})
_FULL_GIT_ACTIONS = frozenset({"add", "commit"})
_EXEC_SESSION_BINDINGS: Dict[str, Dict[str, str]] = {}


@dataclass(frozen=True)
class GroupBridgeContext:
    target_group_id: str
    remote_group_id: str
    remote_peer_id: str
    trust_id: str
    access_level: str
    credential_ref: str = ""


def _source_tool(name: str) -> Dict[str, Any]:
    for spec in MCP_TOOLS:
        if str(spec.get("name") or "") == name:
            return spec
    return {}


def _schema_props(source_name: str) -> Dict[str, Any]:
    schema = _source_tool(source_name).get("inputSchema")
    if not isinstance(schema, dict):
        return {}
    props = schema.get("properties")
    if not isinstance(props, dict):
        return {}
    return deepcopy(props)


def _remote_schema(
    source_name: str,
    *,
    action_enum: Optional[List[str]] = None,
    extra_properties: Optional[Dict[str, Any]] = None,
    required: Optional[List[str]] = None,
) -> Dict[str, Any]:
    props = _schema_props(source_name)
    for key in ("group_id", "actor_id", "by"):
        props.pop(key, None)
    if action_enum is not None:
        props["action"] = {
            "type": "string",
            "enum": action_enum,
            "default": action_enum[0] if action_enum else "",
        }
    props.update(extra_properties or {})
    props["remote_group_id"] = {
        "type": "string",
        "description": "Target remote group id for this Group Bridge call.",
    }
    return {
        "type": "object",
        "properties": props,
        "required": list(required or ["remote_group_id"]),
    }


def group_bridge_tool_specs(access_level: str) -> List[Dict[str, Any]]:
    level = _normalize_access(access_level)
    tools = [
        {
            "name": REMOTE_ACCESS_TOOL,
            "description": (
                "Discover Group Bridge access and explain the current permission level. "
                "Actions: list, status, explain_permissions."
            ),
            "annotations": {"readOnlyHint": True},
            "inputSchema": {
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["list", "status", "explain_permissions"],
                        "default": "status",
                    },
                    "remote_group_id": {
                        "type": "string",
                        "description": "Target remote group id. Optional for action=list.",
                    },
                },
            },
        }
    ]
    if level in {pairing_kernel.ACCESS_LEVEL_READ, pairing_kernel.ACCESS_LEVEL_FULL}:
        git_actions = ["status", "diff", "log"]
        git_annotations = {"readOnlyHint": True}
        git_description = "Read-only git status/diff/log for a target Group Bridge group."
        if level == pairing_kernel.ACCESS_LEVEL_FULL:
            git_actions = ["status", "diff", "log", "add", "commit"]
            git_annotations = {"readOnlyHint": False, "destructiveHint": True}
            git_description = (
                "Git operations for a target Group Bridge group. "
                "Full access allows mutation actions add|commit; this is not a sandbox."
            )
        tools.extend(
            [
                {
                    "name": "cccc_remote_context",
                    "description": "Read the target Group Bridge group's context snapshot. Does not wake target actors.",
                    "annotations": {"readOnlyHint": True},
                    "inputSchema": _remote_schema(
                        "cccc_context_get",
                        action_enum=["get"],
                    ),
                },
                {
                    "name": "cccc_remote_repo",
                    "description": (
                        "Read-only repository inspection for a target Group Bridge group. "
                        "Actions: info, list, list_dir, read, search. Search is fixed under the target group's active scope."
                    ),
                    "annotations": {"readOnlyHint": True},
                    "inputSchema": _remote_schema(
                        "cccc_repo",
                        action_enum=["info", "list", "list_dir", "read", "search"],
                        extra_properties={
                            "query": {"type": "string", "description": "Required for action=search."},
                            "case_sensitive": {"type": "boolean", "default": False},
                            "max_file_bytes": {"type": "integer", "default": 200000, "minimum": 1, "maximum": 1000000},
                        },
                    ),
                },
                {
                    "name": "cccc_remote_git",
                    "description": git_description,
                    "annotations": git_annotations,
                    "inputSchema": _remote_schema(
                        "cccc_git",
                        action_enum=git_actions,
                    ),
                },
            ]
        )
    if level == pairing_kernel.ACCESS_LEVEL_FULL:
        tools.extend(
            [
                {
                    "name": "cccc_remote_repo_edit",
                    "description": (
                        "Full access repository mutation for a target Group Bridge group. "
                        "Use only for trusted groups; paths are guarded by the target active scope but this is not a sandbox."
                    ),
                    "annotations": {"readOnlyHint": False, "destructiveHint": True},
                    "inputSchema": _remote_schema("cccc_repo_edit"),
                },
                {
                    "name": "cccc_remote_apply_patch",
                    "description": (
                        "Full access Codex-style apply_patch against a target Group Bridge group. "
                        "Use only for trusted groups."
                    ),
                    "annotations": {"readOnlyHint": False, "destructiveHint": True},
                    "inputSchema": _remote_schema("cccc_apply_patch"),
                },
                {
                    "name": "cccc_remote_shell",
                    "description": (
                        "Full access one-shot shell command in the target group's active workspace. "
                        "This can change files and run local commands; it is not a security sandbox."
                    ),
                    "annotations": {"readOnlyHint": False, "destructiveHint": True},
                    "inputSchema": _remote_schema("cccc_shell", required=["remote_group_id", "command"]),
                },
                {
                    "name": "cccc_remote_exec_command",
                    "description": (
                        "Full access Codex-style long-running shell session in the target group's active workspace. "
                        "Returned session_id is bound to this bridge and rechecked on write/poll."
                    ),
                    "annotations": {"readOnlyHint": False, "destructiveHint": True},
                    "inputSchema": _remote_schema("cccc_exec_command"),
                },
                {
                    "name": "cccc_remote_write_stdin",
                    "description": (
                        "Write to, poll, or terminate a remote exec session previously returned by cccc_remote_exec_command. "
                        "Full access is rechecked on every call."
                    ),
                    "annotations": {"readOnlyHint": False, "destructiveHint": True},
                    "inputSchema": _remote_schema("cccc_write_stdin", required=["remote_group_id", "session_id"]),
                },
            ]
        )
    return tools


def _normalize_access(access_level: str) -> str:
    try:
        return pairing_kernel.normalize_access_level(access_level)
    except ValueError:
        return pairing_kernel.ACCESS_LEVEL_MESSAGES


def _has_read(context: GroupBridgeContext) -> bool:
    return _normalize_access(context.access_level) in {pairing_kernel.ACCESS_LEVEL_READ, pairing_kernel.ACCESS_LEVEL_FULL}


def _has_full(context: GroupBridgeContext) -> bool:
    return _normalize_access(context.access_level) == pairing_kernel.ACCESS_LEVEL_FULL


def _target_group_id(arguments: Dict[str, Any], context: GroupBridgeContext, *, required: bool = True) -> str:
    target = str(arguments.get("remote_group_id") or "").strip()
    if not target:
        if required:
            raise MCPError(code="missing_remote_group_id", message="remote_group_id is required")
        return ""
    if target != context.target_group_id:
        raise MCPError(
            code="bridge_not_found",
            message="remote_group_id does not match this Group Bridge target",
            details={"remote_group_id": target},
        )
    return context.target_group_id


def _mcp_response(req_id: Any, result: Any) -> Dict[str, Any]:
    return {"jsonrpc": "2.0", "id": req_id, "result": result}


def _mcp_error(req_id: Any, code: int, message: str, data: Any = None) -> Dict[str, Any]:
    err: Dict[str, Any] = {"code": code, "message": message}
    if data is not None:
        err["data"] = data
    return {"jsonrpc": "2.0", "id": req_id, "error": err}


def _tool_result(result: Dict[str, Any]) -> Dict[str, Any]:
    return {
        "content": [
            {
                "type": "text",
                "text": json.dumps(result, ensure_ascii=False, indent=2),
            }
        ]
    }


def _tool_error(error: MCPError) -> Dict[str, Any]:
    return _tool_result({"error": {"code": error.code, "message": error.message, "details": error.details}}) | {"isError": True}


def _negotiated_protocol_version(params: Dict[str, Any]) -> str:
    requested = str(params.get("protocolVersion") or "").strip()
    return requested if requested in _SUPPORTED_PROTOCOL_VERSIONS else _DEFAULT_PROTOCOL_VERSION


def _audit(
    context: GroupBridgeContext,
    *,
    kind: str,
    tool_name: str,
    action: str = "",
    arguments: Optional[Dict[str, Any]] = None,
    ok: Optional[bool] = None,
    error_code: str = "",
    duration_ms: Optional[int] = None,
    result: Optional[Dict[str, Any]] = None,
) -> None:
    group = load_group(context.target_group_id)
    if group is None:
        return
    args = arguments or {}
    data: Dict[str, Any] = {
        "target_group_id": context.target_group_id,
        "remote_group_id": context.remote_group_id,
        "remote_peer_id": context.remote_peer_id,
        "trust_id": context.trust_id,
        "access_level": _normalize_access(context.access_level),
        "tool_name": tool_name,
        "action": action,
    }
    for key in ("path", "file_path", "cwd", "workdir", "session_id"):
        value = str(args.get(key) or "").strip()
        if value:
            data[key] = value
    if ok is not None:
        data["ok"] = bool(ok)
    if error_code:
        data["error_code"] = error_code
    if duration_ms is not None:
        data["duration_ms"] = int(duration_ms)
    if isinstance(result, dict):
        data["output_truncated"] = any(
            bool(result.get(key))
            for key in ("output_truncated", "stdout_truncated", "stderr_truncated", "truncated")
        )
    try:
        append_event(group.ledger_path, kind=kind, group_id=context.target_group_id, scope_key="", by="group_bridge", data=data)
    except Exception:
        pass


def _audit_full_if_needed(context: GroupBridgeContext, name: str, args: Dict[str, Any], result: Dict[str, Any]) -> None:
    if name in {"cccc_remote_shell", "cccc_remote_exec_command", "cccc_remote_write_stdin"}:
        _audit(
            context,
            kind="group_bridge.full_access_command",
            tool_name=name,
            action=str(args.get("action") or ""),
            arguments=args,
            ok=True,
            result=result,
        )
    elif name in {"cccc_remote_repo_edit", "cccc_remote_apply_patch"} or (
        name == "cccc_remote_git" and str(args.get("action") or "").strip().lower() in _FULL_GIT_ACTIONS
    ):
        _audit(
            context,
            kind="group_bridge.full_access_write",
            tool_name=name,
            action=str(args.get("action") or ""),
            arguments=args,
            ok=True,
            result=result,
        )


def _remote_access(arguments: Dict[str, Any], context: GroupBridgeContext) -> Dict[str, Any]:
    action = str(arguments.get("action") or "status").strip().lower()
    if action not in {"list", "status", "explain_permissions"}:
        raise MCPError(code="invalid_action", message="cccc_remote_access action must be list|status|explain_permissions")
    if action != "list":
        _target_group_id(arguments, context)
    permissions = {
        "messages": True,
        "read": _has_read(context),
        "full": _has_full(context),
    }
    group = load_group(context.target_group_id)
    remote_group_title = ""
    if group is not None:
        remote_group_title = str(group.doc.get("title") or "").strip()
    target = {
        "remote_group_id": context.target_group_id,
        "remote_group_title": remote_group_title,
        "bridge_status": "active",
        "access_level": _normalize_access(context.access_level),
        "permissions": permissions,
        "trust_id": context.trust_id,
        "remote_peer_id": context.remote_peer_id,
    }
    if action == "list":
        return {"targets": [target]}
    if action == "explain_permissions":
        return {
            **target,
            "message": (
                "Messages only can send explicit-recipient remote messages. Read access can inspect context, repo, search, and git status. "
                "Full access can change files and run local commands; it is not a sandbox."
            ),
        }
    return {
        **target,
        "recommended_next_action": "Use read tools for inspection." if permissions["read"] else "Ask the target owner to enable Read access or use explicit-recipient messages.",
    }


def _remote_context(arguments: Dict[str, Any], context: GroupBridgeContext) -> Dict[str, Any]:
    gid = _target_group_id(arguments, context)
    action = str(arguments.get("action") or "get").strip().lower()
    if action != "get":
        raise MCPError(code="invalid_action", message="cccc_remote_context action must be get")
    return context_get(group_id=gid, include_archived=coerce_bool(arguments.get("include_archived"), default=False))


def _remote_repo(arguments: Dict[str, Any], context: GroupBridgeContext) -> Dict[str, Any]:
    gid = _target_group_id(arguments, context)
    action = str(arguments.get("action") or "info").strip().lower()
    if action == "search":
        return repo_search_tool(
            group_id=gid,
            query=str(arguments.get("query") or ""),
            path=str(arguments.get("path") or arguments.get("file_path") or ""),
            limit=arguments.get("limit") or 100,
            include_hidden=coerce_bool(arguments.get("include_hidden"), default=False),
            case_sensitive=coerce_bool(arguments.get("case_sensitive"), default=False),
            max_file_bytes=arguments.get("max_file_bytes") or 200000,
        )
    if action not in {"info", "list", "list_dir", "read"}:
        raise MCPError(code="invalid_action", message="cccc_remote_repo action must be info|list|list_dir|read|search")
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


def _remote_git_read(arguments: Dict[str, Any], context: GroupBridgeContext) -> Dict[str, Any]:
    gid = _target_group_id(arguments, context)
    action = str(arguments.get("action") or "status").strip().lower()
    if action not in _READ_GIT_ACTIONS:
        raise MCPError(code="invalid_action", message="Read access cccc_remote_git action must be status|diff|log")
    return git_tool(
        group_id=gid,
        action=action,
        paths=arguments.get("paths"),
        path=str(arguments.get("path") or ""),
        staged=coerce_bool(arguments.get("staged"), default=False),
        count=arguments.get("count") or 20,
        max_output_bytes=arguments.get("max_output_bytes") or 200000,
    )


def _remote_repo_edit(arguments: Dict[str, Any], context: GroupBridgeContext) -> Dict[str, Any]:
    gid = _target_group_id(arguments, context)
    action = str(arguments.get("action") or "").strip().lower()
    if not action:
        if isinstance(arguments.get("replacements"), list):
            action = "multi_replace"
        elif str(arguments.get("content") or ""):
            action = "write"
        else:
            action = "replace"
    if action not in {"replace", "multi_replace", "write", "mkdir", "delete", "move"}:
        raise MCPError(code="invalid_action", message="cccc_remote_repo_edit action must be replace|multi_replace|write|mkdir|delete|move")
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
        recursive=coerce_bool(arguments.get("recursive"), default=False),
        exist_ok=coerce_bool(arguments.get("exist_ok"), default=True),
    )


def _remote_apply_patch(arguments: Dict[str, Any], context: GroupBridgeContext) -> Dict[str, Any]:
    gid = _target_group_id(arguments, context)
    return apply_codex_patch_tool(group_id=gid, patch=str(arguments.get("patch") or arguments.get("input") or ""))


def _remote_shell(arguments: Dict[str, Any], context: GroupBridgeContext) -> Dict[str, Any]:
    gid = _target_group_id(arguments, context)
    return shell_tool(
        group_id=gid,
        command=str(arguments.get("command") or ""),
        cwd=str(arguments.get("cwd") or "."),
        timeout_s=arguments.get("timeout_s") or 60,
        max_output_bytes=arguments.get("max_output_bytes") or 200000,
        env=arguments.get("env") if isinstance(arguments.get("env"), dict) else None,
    )


def _remote_exec_command(arguments: Dict[str, Any], context: GroupBridgeContext) -> Dict[str, Any]:
    gid = _target_group_id(arguments, context)
    result = exec_command_tool(
        group_id=gid,
        command=str(arguments.get("command") or arguments.get("cmd") or ""),
        cwd=str(arguments.get("cwd") or arguments.get("workdir") or "."),
        yield_time_ms=arguments.get("yield_time_ms") if "yield_time_ms" in arguments else 1000,
        max_output_bytes=arguments.get("max_output_bytes") or 200000,
        timeout_s=arguments.get("timeout_s") or 600,
        env=arguments.get("env") if isinstance(arguments.get("env"), dict) else None,
    )
    session_id = str(result.get("session_id") or "").strip()
    if session_id:
        _EXEC_SESSION_BINDINGS[session_id] = {
            "target_group_id": context.target_group_id,
            "remote_group_id": context.remote_group_id,
            "trust_id": context.trust_id,
        }
    return result


def _remote_write_stdin(arguments: Dict[str, Any], context: GroupBridgeContext) -> Dict[str, Any]:
    _target_group_id(arguments, context)
    session_id = str(arguments.get("session_id") or "").strip()
    binding = _EXEC_SESSION_BINDINGS.get(session_id)
    expected = {
        "target_group_id": context.target_group_id,
        "remote_group_id": context.remote_group_id,
        "trust_id": context.trust_id,
    }
    if binding != expected:
        raise MCPError(code="bridge_session_not_found", message="remote exec session is not bound to this active bridge")
    result = write_stdin_tool(
        session_id=session_id,
        chars=str(arguments.get("chars") or ""),
        yield_time_ms=arguments.get("yield_time_ms") if "yield_time_ms" in arguments else 1000,
        max_output_bytes=arguments.get("max_output_bytes") or 200000,
        terminate=coerce_bool(arguments.get("terminate"), default=False),
    )
    if not bool(result.get("running")):
        _EXEC_SESSION_BINDINGS.pop(session_id, None)
    return result


def _call_tool(name: str, arguments: Dict[str, Any], context: GroupBridgeContext) -> Dict[str, Any]:
    if name == REMOTE_ACCESS_TOOL:
        return _remote_access(arguments, context)
    if name in REMOTE_READ_TOOLS and not _has_read(context):
        raise MCPError(code="bridge_read_not_granted", message="Read access is not enabled for this Group Bridge")
    if name in REMOTE_FULL_TOOLS and not _has_full(context):
        raise MCPError(code="bridge_full_access_not_granted", message="Full access is not enabled for this Group Bridge")
    if name == "cccc_remote_context":
        return _remote_context(arguments, context)
    if name == "cccc_remote_repo":
        return _remote_repo(arguments, context)
    if name == "cccc_remote_git":
        action = str(arguments.get("action") or "status").strip().lower()
        if action in _READ_GIT_ACTIONS:
            if not _has_read(context):
                raise MCPError(code="bridge_read_not_granted", message="Read access is not enabled for this Group Bridge")
            return _remote_git_read(arguments, context)
        if action in _FULL_GIT_ACTIONS:
            if not _has_full(context):
                raise MCPError(code="bridge_full_access_not_granted", message="Full access is required for git mutation")
            gid = _target_group_id(arguments, context)
            return git_tool(
                group_id=gid,
                action=action,
                paths=arguments.get("paths"),
                path=str(arguments.get("path") or ""),
                message=str(arguments.get("message") or ""),
                all_changes=coerce_bool(arguments.get("all_changes"), default=False),
                max_output_bytes=arguments.get("max_output_bytes") or 200000,
            )
        raise MCPError(code="invalid_action", message="cccc_remote_git action must be status|diff|log|add|commit")
    if name == "cccc_remote_repo_edit":
        return _remote_repo_edit(arguments, context)
    if name == "cccc_remote_apply_patch":
        return _remote_apply_patch(arguments, context)
    if name == "cccc_remote_shell":
        return _remote_shell(arguments, context)
    if name == "cccc_remote_exec_command":
        return _remote_exec_command(arguments, context)
    if name == "cccc_remote_write_stdin":
        return _remote_write_stdin(arguments, context)
    raise MCPError(code="unknown_tool", message=f"unknown Group Bridge tool: {name}")


def handle_group_bridge_request(req: Dict[str, Any], context: GroupBridgeContext) -> Dict[str, Any]:
    req_id = req.get("id")
    method = str(req.get("method") or "")
    params = req.get("params") if isinstance(req.get("params"), dict) else {}

    if method == "initialize":
        return _mcp_response(
            req_id,
            {
                "protocolVersion": _negotiated_protocol_version(params),
                "capabilities": {
                    "tools": {"listChanged": True},
                    "resources": {},
                    "prompts": {},
                },
                "serverInfo": {
                    "name": "cccc-group-bridge-mcp",
                    "version": __version__,
                },
            },
        )
    if method.startswith("notifications/"):
        return {}
    if method == "tools/list":
        return _mcp_response(req_id, {"tools": group_bridge_tool_specs(context.access_level)})
    if method == "resources/list":
        return _mcp_response(req_id, {"resources": []})
    if method == "prompts/list":
        return _mcp_response(req_id, {"prompts": []})
    if method == "roots/list":
        return _mcp_response(req_id, {"roots": []})
    if method in {"ping", "logging/setLevel"}:
        return _mcp_response(req_id, {})
    if method != "tools/call":
        return _mcp_error(req_id, -32601, f"Method not found: {method}")

    tool_name = str(params.get("name") or "").strip()
    arguments = params.get("arguments") if isinstance(params.get("arguments"), dict) else {}
    action = str(arguments.get("action") or "").strip().lower()
    started = time.monotonic()
    try:
        result = _call_tool(tool_name, arguments, context)
        duration_ms = int((time.monotonic() - started) * 1000)
        _audit(
            context,
            kind="group_bridge.mcp_call",
            tool_name=tool_name,
            action=action,
            arguments=arguments,
            ok=True,
            duration_ms=duration_ms,
            result=result,
        )
        _audit_full_if_needed(context, tool_name, arguments, result)
        return _mcp_response(req_id, _tool_result(result))
    except MCPError as exc:
        duration_ms = int((time.monotonic() - started) * 1000)
        audit_kind = "group_bridge.path_guardrail_denied" if exc.code == "invalid_path" else "group_bridge.mcp_denied"
        _audit(
            context,
            kind=audit_kind,
            tool_name=tool_name,
            action=action,
            arguments=arguments,
            ok=False,
            error_code=exc.code,
            duration_ms=duration_ms,
        )
        return _mcp_response(req_id, _tool_error(exc))
    except Exception as exc:
        duration_ms = int((time.monotonic() - started) * 1000)
        _audit(
            context,
            kind="group_bridge.mcp_denied",
            tool_name=tool_name,
            action=action,
            arguments=arguments,
            ok=False,
            error_code="internal_error",
            duration_ms=duration_ms,
        )
        return _mcp_response(
            req_id,
            _tool_error(MCPError(code="internal_error", message=str(exc))),
        )
