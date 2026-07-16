"""Local MCP wrappers for Group Bridge remote access tools."""

from __future__ import annotations

import json
import math
import urllib.error
import urllib.request
from typing import Any, Dict, List, Optional

from ....kernel.group_bridge import pairing as pairing_kernel
from ....kernel.group_bridge.registration import get_registration
from ..common import MCPError

_REMOTE_MCP_TIMEOUT_SECONDS = 8.0
_REMOTE_MCP_TIMEOUT_BUFFER_SECONDS = 5.0
_REMOTE_MCP_MAX_SHELL_TIMEOUT_SECONDS = 600.0
_REMOTE_MCP_MAX_YIELD_SECONDS = 30.0


def _strip_local_fields(arguments: Dict[str, Any]) -> Dict[str, Any]:
    out = dict(arguments or {})
    for key in ("group_id", "actor_id", "by"):
        out.pop(key, None)
    return out


def _approved_outbound_token(group_id: str, remote_group_id: str) -> str:
    for outbound in pairing_kernel.list_pairing_outbounds(group_id=group_id):
        if str(outbound.get("status") or "").strip() != "approved":
            continue
        if str(outbound.get("issuer_group_id") or "").strip() != remote_group_id:
            continue
        remote_request = outbound.get("remote_request")
        if not isinstance(remote_request, dict):
            continue
        token = str(remote_request.get("remote_send_token") or "").strip()
        if token:
            return token
    return ""


def _endpoint_from_trust(trust: Dict[str, Any]) -> str:
    endpoint = str(trust.get("remote_endpoint") or "").strip()
    if endpoint:
        return endpoint
    registration_id = str(trust.get("registration_id") or "").strip()
    if registration_id:
        registration = get_registration(registration_id)
        if isinstance(registration, dict):
            return str(registration.get("url") or "").strip()
    return ""


def _bridge_targets(group_id: str) -> List[Dict[str, Any]]:
    gid = str(group_id or "").strip()
    targets: List[Dict[str, Any]] = []
    for trust in pairing_kernel.list_trusts(group_id=gid):
        if str(trust.get("status") or "").strip() != "active":
            continue
        remote_group_id = str(trust.get("remote_group_id") or "").strip()
        if not remote_group_id:
            continue
        endpoint = _endpoint_from_trust(trust)
        remote_group_title = str(trust.get("remote_group_title") or "").strip()
        display_name = remote_group_title or endpoint or remote_group_id
        token = _approved_outbound_token(gid, remote_group_id)
        targets.append(
            {
                "remote_group_id": remote_group_id,
                "display_name": display_name,
                "remote_group_title": remote_group_title,
                "remote_peer_id": str(trust.get("remote_peer_id") or "").strip(),
                "registration_id": str(trust.get("registration_id") or "").strip(),
                "trust_id": str(trust.get("trust_id") or "").strip(),
                "bridge_status": str(trust.get("status") or "").strip() or "active",
                "local_grant_access_level": str(trust.get("access_level") or pairing_kernel.ACCESS_LEVEL_MESSAGES).strip()
                or pairing_kernel.ACCESS_LEVEL_MESSAGES,
                "endpoint": endpoint,
                "remote_mcp_available": bool(token and endpoint.startswith(("http://", "https://"))),
                "recommended_message_send": {
                    "tool": "cccc_message_send",
                    "dst_group_id": remote_group_id,
                    "to": ["@foreman"],
                    "note": "Use this shape to send a normal message to the remote group's foreman.",
                },
                "recommended_remote_access": {
                    "discover": 'cccc_remote_access(action="list")',
                    "read_tools": ["cccc_remote_context", "cccc_remote_repo", "cccc_remote_git"],
                    "full_tools": [
                        "cccc_remote_repo_edit",
                        "cccc_remote_apply_patch",
                        "cccc_remote_shell",
                        "cccc_remote_exec_command",
                        "cccc_remote_write_stdin",
                    ],
                    "note": "Remote MCP tools require remote_group_id and depend on the access level granted by the remote group.",
                },
                "_remote_send_token": token,
            }
        )
    targets.sort(key=lambda item: (str(item.get("remote_group_title") or ""), str(item.get("remote_group_id") or "")))
    return targets


def _target_for(group_id: str, remote_group_id: str) -> Dict[str, Any]:
    gid = str(group_id or "").strip()
    remote_gid = str(remote_group_id or "").strip()
    if not gid:
        raise MCPError(code="missing_group_id", message="group_id is required")
    if not remote_gid:
        raise MCPError(code="missing_remote_group_id", message="remote_group_id is required")
    for target in _bridge_targets(gid):
        if str(target.get("remote_group_id") or "") == remote_gid:
            token = str(target.get("_remote_send_token") or "").strip()
            endpoint = str(target.get("endpoint") or "").strip()
            if not endpoint.startswith(("http://", "https://")):
                raise MCPError(
                    code="bridge_remote_mcp_unavailable",
                    message="remote MCP requires an HTTP(S) Group Bridge endpoint",
                    details={"remote_group_id": remote_gid, "endpoint": endpoint},
                )
            if not token:
                raise MCPError(
                    code="bridge_remote_mcp_unavailable",
                    message="remote MCP token is unavailable; sync the Group Bridge approval status first",
                    details={"remote_group_id": remote_gid},
                )
            return target
    raise MCPError(code="bridge_not_found", message=f"Group Bridge target not found: {remote_gid}")


def _parse_tool_payload(response: Dict[str, Any]) -> Dict[str, Any]:
    if isinstance(response.get("error"), dict):
        err = response["error"]
        raise MCPError(code="bridge_remote_mcp_error", message=str(err.get("message") or "remote MCP error"), details=dict(err))
    result = response.get("result")
    if not isinstance(result, dict):
        return {}
    content = result.get("content")
    text = ""
    if isinstance(content, list) and content and isinstance(content[0], dict):
        text = str(content[0].get("text") or "")
    parsed: Dict[str, Any] = {}
    if text:
        try:
            raw = json.loads(text)
            parsed = raw if isinstance(raw, dict) else {"value": raw}
        except Exception:
            parsed = {"text": text}
    if bool(result.get("isError")):
        err = parsed.get("error") if isinstance(parsed.get("error"), dict) else {}
        raise MCPError(
            code=str(err.get("code") or "bridge_remote_mcp_error"),
            message=str(err.get("message") or "remote MCP tool call failed"),
            details=dict(err.get("details") or {}) if isinstance(err.get("details"), dict) else {},
        )
    return parsed


def _numeric_argument(arguments: Dict[str, Any], key: str, default: float) -> float:
    try:
        value = float(arguments.get(key, default))
    except (TypeError, ValueError):
        return default
    if not math.isfinite(value):
        return default
    return value


def _bounded(value: float, *, minimum: float, maximum: float) -> float:
    return min(max(value, minimum), maximum)


def _remote_mcp_timeout_seconds(tool_name: str, arguments: Dict[str, Any]) -> float:
    if tool_name == "cccc_remote_shell":
        requested = _bounded(
            _numeric_argument(arguments, "timeout_s", 60.0),
            minimum=1.0,
            maximum=_REMOTE_MCP_MAX_SHELL_TIMEOUT_SECONDS,
        )
        return max(_REMOTE_MCP_TIMEOUT_SECONDS, requested + _REMOTE_MCP_TIMEOUT_BUFFER_SECONDS)
    if tool_name in {"cccc_remote_exec_command", "cccc_remote_write_stdin"}:
        yield_seconds = _bounded(
            _numeric_argument(arguments, "yield_time_ms", 1000.0) / 1000.0,
            minimum=0.0,
            maximum=_REMOTE_MCP_MAX_YIELD_SECONDS,
        )
        return max(_REMOTE_MCP_TIMEOUT_SECONDS, yield_seconds + _REMOTE_MCP_TIMEOUT_BUFFER_SECONDS)
    return _REMOTE_MCP_TIMEOUT_SECONDS


def _call_remote_tool(target: Dict[str, Any], tool_name: str, arguments: Dict[str, Any]) -> Dict[str, Any]:
    endpoint = str(target.get("endpoint") or "").rstrip("/")
    token = str(target.get("_remote_send_token") or "").strip()
    url = f"{endpoint}/mcp/group-bridge"
    payload = {
        "jsonrpc": "2.0",
        "id": "cccc-group-bridge-call",
        "method": "tools/call",
        "params": {
            "name": tool_name,
            "arguments": arguments,
        },
    }
    request = urllib.request.Request(
        url,
        data=json.dumps(payload, ensure_ascii=False).encode("utf-8"),
        headers={
            "Authorization": f"Bearer {token}",
            "Content-Type": "application/json",
            "Accept": "application/json",
        },
        method="POST",
    )
    try:
        timeout_seconds = _remote_mcp_timeout_seconds(tool_name, arguments)
        with urllib.request.urlopen(request, timeout=timeout_seconds) as resp:  # noqa: S310 - endpoint is user-configured bridge target
            raw = resp.read()
    except urllib.error.HTTPError as exc:
        try:
            body = exc.read().decode("utf-8", errors="replace")
            parsed = json.loads(body)
        except Exception:
            parsed = {}
        detail = parsed.get("detail") if isinstance(parsed, dict) else {}
        raise MCPError(
            code=str((detail if isinstance(detail, dict) else {}).get("code") or "bridge_remote_http_error"),
            message=str((detail if isinstance(detail, dict) else {}).get("message") or f"remote MCP HTTP error {exc.code}"),
            details={"status": int(exc.code), "remote_group_id": str(target.get("remote_group_id") or "")},
        ) from exc
    except OSError as exc:
        raise MCPError(
            code="bridge_remote_mcp_unreachable",
            message=f"remote MCP endpoint is unreachable: {exc}",
            details={"remote_group_id": str(target.get("remote_group_id") or ""), "endpoint": endpoint},
        ) from exc
    try:
        response = json.loads(raw.decode("utf-8", errors="replace"))
    except Exception as exc:
        raise MCPError(code="bridge_remote_mcp_invalid_response", message="remote MCP returned invalid JSON") from exc
    if not isinstance(response, dict):
        raise MCPError(code="bridge_remote_mcp_invalid_response", message="remote MCP returned a non-object response")
    return _parse_tool_payload(response)


def remote_access(*, group_id: str, arguments: Dict[str, Any]) -> Dict[str, Any]:
    action = str(arguments.get("action") or "list").strip().lower()
    if action not in {"list", "status", "explain_permissions"}:
        raise MCPError(code="invalid_action", message="cccc_remote_access action must be list|status|explain_permissions")
    if action == "list" and not str(arguments.get("remote_group_id") or "").strip():
        targets: List[Dict[str, Any]] = []
        for target in _bridge_targets(group_id):
            projected = {k: v for k, v in target.items() if not k.startswith("_")}
            if bool(target.get("remote_mcp_available")):
                try:
                    remote_status = _call_remote_tool(
                        target,
                        "cccc_remote_access",
                        {"action": "status", "remote_group_id": str(target.get("remote_group_id") or "")},
                    )
                    projected["remote_access_level"] = str(remote_status.get("access_level") or "").strip()
                    projected["remote_permissions"] = remote_status.get("permissions") if isinstance(remote_status.get("permissions"), dict) else {}
                except MCPError as exc:
                    projected["remote_status_error"] = {"code": exc.code, "message": exc.message}
            targets.append(projected)
        return {"targets": targets}
    remote_gid = str(arguments.get("remote_group_id") or "").strip()
    target = _target_for(group_id, remote_gid)
    remote_args = _strip_local_fields(arguments)
    remote_args["remote_group_id"] = remote_gid
    return _call_remote_tool(target, "cccc_remote_access", remote_args)


def remote_context(*, group_id: str, arguments: Dict[str, Any]) -> Dict[str, Any]:
    remote_gid = str(arguments.get("remote_group_id") or "").strip()
    target = _target_for(group_id, remote_gid)
    remote_args = _strip_local_fields(arguments)
    remote_args["remote_group_id"] = remote_gid
    return _call_remote_tool(target, "cccc_remote_context", remote_args)


def remote_repo(*, group_id: str, arguments: Dict[str, Any]) -> Dict[str, Any]:
    remote_gid = str(arguments.get("remote_group_id") or "").strip()
    target = _target_for(group_id, remote_gid)
    remote_args = _strip_local_fields(arguments)
    remote_args["remote_group_id"] = remote_gid
    return _call_remote_tool(target, "cccc_remote_repo", remote_args)


def remote_git(*, group_id: str, arguments: Dict[str, Any]) -> Dict[str, Any]:
    remote_gid = str(arguments.get("remote_group_id") or "").strip()
    target = _target_for(group_id, remote_gid)
    remote_args = _strip_local_fields(arguments)
    remote_args["remote_group_id"] = remote_gid
    return _call_remote_tool(target, "cccc_remote_git", remote_args)


def remote_repo_edit(*, group_id: str, arguments: Dict[str, Any]) -> Dict[str, Any]:
    remote_gid = str(arguments.get("remote_group_id") or "").strip()
    target = _target_for(group_id, remote_gid)
    remote_args = _strip_local_fields(arguments)
    remote_args["remote_group_id"] = remote_gid
    return _call_remote_tool(target, "cccc_remote_repo_edit", remote_args)


def remote_apply_patch(*, group_id: str, arguments: Dict[str, Any]) -> Dict[str, Any]:
    remote_gid = str(arguments.get("remote_group_id") or "").strip()
    target = _target_for(group_id, remote_gid)
    remote_args = _strip_local_fields(arguments)
    remote_args["remote_group_id"] = remote_gid
    return _call_remote_tool(target, "cccc_remote_apply_patch", remote_args)


def remote_shell(*, group_id: str, arguments: Dict[str, Any]) -> Dict[str, Any]:
    remote_gid = str(arguments.get("remote_group_id") or "").strip()
    target = _target_for(group_id, remote_gid)
    remote_args = _strip_local_fields(arguments)
    remote_args["remote_group_id"] = remote_gid
    return _call_remote_tool(target, "cccc_remote_shell", remote_args)


def remote_exec_command(*, group_id: str, arguments: Dict[str, Any]) -> Dict[str, Any]:
    remote_gid = str(arguments.get("remote_group_id") or "").strip()
    target = _target_for(group_id, remote_gid)
    remote_args = _strip_local_fields(arguments)
    remote_args["remote_group_id"] = remote_gid
    return _call_remote_tool(target, "cccc_remote_exec_command", remote_args)


def remote_write_stdin(*, group_id: str, arguments: Dict[str, Any]) -> Dict[str, Any]:
    remote_gid = str(arguments.get("remote_group_id") or "").strip()
    target = _target_for(group_id, remote_gid)
    remote_args = _strip_local_fields(arguments)
    remote_args["remote_group_id"] = remote_gid
    return _call_remote_tool(target, "cccc_remote_write_stdin", remote_args)
