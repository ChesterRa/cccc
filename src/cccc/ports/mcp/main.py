"""
CCCC MCP Server — entrypoint

Runs in stdio mode for agent runtimes.

Usage:
    python -m cccc.ports.mcp.main

Or via CLI:
    cccc mcp
"""

from __future__ import annotations

import json
import sys
from typing import Any, Dict, List, Optional

from ... import __version__
from .server import MCPError, handle_tool_call, list_tools_for_caller

_SESSION_SUPPORTS_TOOLS_LIST_CHANGED = False
_PENDING_NOTIFICATIONS: List[Dict[str, Any]] = []


def _set_session_client_capabilities(params: Dict[str, Any]) -> None:
    global _SESSION_SUPPORTS_TOOLS_LIST_CHANGED, _PENDING_NOTIFICATIONS
    supports = False
    caps = params.get("capabilities")
    if isinstance(caps, dict):
        tools_caps = caps.get("tools")
        if isinstance(tools_caps, dict):
            supports = bool(tools_caps.get("listChanged"))
    _SESSION_SUPPORTS_TOOLS_LIST_CHANGED = supports
    _PENDING_NOTIFICATIONS = []


def _enqueue_tools_list_changed_notification() -> None:
    _PENDING_NOTIFICATIONS.append(
        {
            "jsonrpc": "2.0",
            "method": "notifications/tools/list_changed",
            "params": {},
        }
    )


def _drain_pending_notifications() -> List[Dict[str, Any]]:
    global _PENDING_NOTIFICATIONS
    out = list(_PENDING_NOTIFICATIONS)
    _PENDING_NOTIFICATIONS = []
    return out


def _reset_session_state_for_tests() -> None:
    global _SESSION_SUPPORTS_TOOLS_LIST_CHANGED, _PENDING_NOTIFICATIONS
    _SESSION_SUPPORTS_TOOLS_LIST_CHANGED = False
    _PENDING_NOTIFICATIONS = []


def _decode_cursor(cursor: Any) -> int:
    raw = str(cursor or "").strip()
    if not raw:
        return 0
    try:
        value = int(raw)
    except Exception:
        value = 0
    return max(0, value)


def _encode_cursor(offset: int) -> str:
    return str(max(0, int(offset)))


def _read_message() -> Optional[Dict[str, Any]]:
    """Read a single JSON-RPC message from stdin."""
    try:
        line = sys.stdin.readline()
        if not line:
            return None
        return json.loads(line.strip())
    except Exception:
        return None


def _write_message(msg: Dict[str, Any]) -> None:
    """Write a single JSON-RPC message to stdout."""
    sys.stdout.write(json.dumps(msg, ensure_ascii=False) + "\n")
    sys.stdout.flush()


def _make_response(id: Any, result: Any) -> Dict[str, Any]:
    """Build a JSON-RPC success response."""
    return {"jsonrpc": "2.0", "id": id, "result": result}


def _make_error(id: Any, code: int, message: str, data: Any = None) -> Dict[str, Any]:
    """Build a JSON-RPC error response."""
    error: Dict[str, Any] = {"code": code, "message": message}
    if data is not None:
        error["data"] = data
    return {"jsonrpc": "2.0", "id": id, "error": error}


def handle_request(req: Dict[str, Any]) -> Dict[str, Any]:
    """Handle an MCP JSON-RPC request."""
    req_id = req.get("id")
    method = str(req.get("method") or "")
    params = req.get("params") or {}

    # MCP protocol methods
    if method == "initialize":
        _set_session_client_capabilities(params if isinstance(params, dict) else {})
        return _make_response(req_id, {
            "protocolVersion": "2024-11-05",
            "capabilities": {
                "tools": {
                    "listChanged": True,
                },
                # Some MCP clients probe these even if unused; return empty lists below.
                "resources": {},
                "prompts": {},
            },
            "serverInfo": {
                "name": "cccc-mcp",
                "version": __version__,
            },
        })

    if method.startswith("notifications/"):
        # Notifications do not require a response.
        return {}

    if method == "tools/list":
        tools = list_tools_for_caller()
        cursor = _decode_cursor(params.get("cursor"))
        try:
            limit = int(params.get("limit") or 100)
        except Exception:
            limit = 100
        limit = max(1, min(limit, 200))
        page = tools[cursor : cursor + limit]
        next_cursor = ""
        if cursor + limit < len(tools):
            next_cursor = _encode_cursor(cursor + limit)
        return _make_response(req_id, {"tools": page, "nextCursor": next_cursor})

    # Optional MCP surfaces (return empty to avoid noisy "Method not found" in some runtimes)
    if method == "resources/list":
        return _make_response(req_id, {"resources": []})

    if method == "prompts/list":
        return _make_response(req_id, {"prompts": []})

    # Common no-op requests some clients send
    if method == "ping":
        return _make_response(req_id, {})

    if method == "logging/setLevel":
        return _make_response(req_id, {})

    if method == "tools/call":
        tool_name = str(params.get("name") or "")
        arguments = params.get("arguments") or {}
        if not isinstance(arguments, dict):
            arguments = {}

        try:
            result = handle_tool_call(tool_name, arguments)
            refresh_required = False
            if isinstance(result, dict):
                if bool(result.get("refresh_required")):
                    refresh_required = True
                enable_result = result.get("enable_result")
                if isinstance(enable_result, dict) and bool(enable_result.get("refresh_required")):
                    refresh_required = True
            if (
                tool_name
                in {"cccc_capability_enable", "cccc_capability_import", "cccc_capability_uninstall", "cccc_capability_use"}
                and refresh_required
                and _SESSION_SUPPORTS_TOOLS_LIST_CHANGED
            ):
                _enqueue_tools_list_changed_notification()
            return _make_response(req_id, {
                "content": [
                    {
                        "type": "text",
                        "text": json.dumps(result, ensure_ascii=False, indent=2),
                    }
                ],
            })
        except MCPError as e:
            return _make_response(req_id, {
                "content": [
                    {
                        "type": "text",
                        "text": json.dumps({
                            "error": {
                                "code": e.code,
                                "message": e.message,
                                "details": e.details,
                            }
                        }, ensure_ascii=False, indent=2),
                    }
                ],
                "isError": True,
            })
        except Exception as e:
            return _make_response(req_id, {
                "content": [
                    {
                        "type": "text",
                        "text": json.dumps({
                            "error": {
                                "code": "internal_error",
                                "message": str(e),
                            }
                        }, ensure_ascii=False, indent=2),
                    }
                ],
                "isError": True,
            })

    # Unknown method
    return _make_error(req_id, -32601, f"Method not found: {method}")


def main() -> int:
    """MCP server main loop (stdio mode)."""
    while True:
        msg = _read_message()
        if msg is None:
            break

        resp = handle_request(msg)
        if resp:  # Notifications return {}
            _write_message(resp)
        for note in _drain_pending_notifications():
            _write_message(note)

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
