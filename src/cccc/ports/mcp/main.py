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
from typing import Any, Dict, Optional

from ... import __version__
from .server import MCPError, handle_tool_call, list_tools_for_caller
from .toolspecs import MCP_TOOLS

_SUPPORTED_PROTOCOL_VERSIONS = ("2025-11-25", "2025-06-18", "2024-11-05")
_DEFAULT_PROTOCOL_VERSION = _SUPPORTED_PROTOCOL_VERSIONS[0]
# Match the response framing to the inbound stdio framing. Some clients send
# newline JSON; MCP Content-Length framing is enabled only after seeing it.
_STDIO_WRITE_CONTENT_LENGTH = False
_BUILTIN_TOOL_NAMES = frozenset(
    str(tool.get("name") or "").strip()
    for tool in MCP_TOOLS
    if isinstance(tool, dict) and str(tool.get("name") or "").strip()
)


def _reset_session_state_for_tests() -> None:
    global _STDIO_WRITE_CONTENT_LENGTH
    _STDIO_WRITE_CONTENT_LENGTH = False


def _negotiated_protocol_version(params: Dict[str, Any]) -> str:
    """Return a protocol version the client can accept (MCP lifecycle negotiation)."""
    requested = str(params.get("protocolVersion") or "").strip()
    if requested in _SUPPORTED_PROTOCOL_VERSIONS:
        return requested
    return _DEFAULT_PROTOCOL_VERSION


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


def _stdin_buffer() -> Any:
    return getattr(sys.stdin, "buffer", None)


def _stdout_buffer() -> Any:
    return getattr(sys.stdout, "buffer", None)


def _parse_content_length_header(line: str) -> Optional[int]:
    stripped = line.strip()
    if not stripped.lower().startswith("content-length:"):
        return None
    try:
        return int(stripped.split(":", 1)[1].strip())
    except Exception:
        return None


def _read_content_length_body(raw_stdin: Any, first_line: str) -> Optional[bytes]:
    """Read MCP message body after the first Content-Length header line."""
    content_length = _parse_content_length_header(first_line)
    if content_length is None:
        return None
    while True:
        header_line = raw_stdin.readline()
        if not header_line:
            return None
        if header_line in (b"\r\n", b"\n"):
            break
        decoded = header_line.decode("utf-8", errors="replace").strip()
        if not decoded:
            break
        extra_len = _parse_content_length_header(decoded)
        if extra_len is not None:
            content_length = extra_len
    if content_length < 0:
        return None
    body = raw_stdin.read(content_length)
    if body is None or len(body) < content_length:
        return None
    return body


def _read_message() -> Optional[Dict[str, Any]]:
    """Read a single JSON-RPC message from stdin (newline or Content-Length framing)."""
    try:
        global _STDIO_WRITE_CONTENT_LENGTH
        raw_stdin = _stdin_buffer()
        if raw_stdin is not None:
            raw_line = raw_stdin.readline()
            if not raw_line:
                return None
            line = raw_line.decode("utf-8", errors="replace")
            if _parse_content_length_header(line) is not None:
                _STDIO_WRITE_CONTENT_LENGTH = True
                body = _read_content_length_body(raw_stdin, line)
                if not body:
                    return None
                return json.loads(body.decode("utf-8"))
            stripped = line.strip()
            if not stripped:
                return None
            return json.loads(stripped)
        line = sys.stdin.readline()
        if not line:
            return None
        stripped = line.strip()
        if not stripped:
            return None
        if _parse_content_length_header(stripped) is not None:
            _STDIO_WRITE_CONTENT_LENGTH = True
            content_length = _parse_content_length_header(stripped)
            if content_length is None:
                return None
            while True:
                header_line = sys.stdin.readline()
                if not header_line:
                    return None
                if not header_line.strip():
                    break
                extra_len = _parse_content_length_header(header_line)
                if extra_len is not None:
                    content_length = extra_len
            body = sys.stdin.buffer.read(content_length) if hasattr(sys.stdin, "buffer") else sys.stdin.read(content_length)
            if not body:
                return None
            return json.loads(body)
        return json.loads(stripped)
    except Exception:
        return None


def _write_message(msg: Dict[str, Any]) -> None:
    """Write a single JSON-RPC message to stdout."""
    body = json.dumps(msg, ensure_ascii=False)
    if _STDIO_WRITE_CONTENT_LENGTH:
        encoded = body.encode("utf-8")
        payload = f"Content-Length: {len(encoded)}\r\n\r\n".encode("utf-8") + encoded
    else:
        payload = (body + "\n").encode("utf-8")
    raw_stdout = _stdout_buffer()
    if raw_stdout is not None:
        raw_stdout.write(payload)
        raw_stdout.flush()
        return
    sys.stdout.write(payload.decode("utf-8"))
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


def _make_tool_result(payload: Dict[str, Any], *, is_error: bool = False) -> Dict[str, Any]:
    result: Dict[str, Any] = {
        "content": [
            {
                "type": "text",
                "text": json.dumps(payload, ensure_ascii=False, indent=2),
            }
        ],
        "structuredContent": payload,
    }
    if is_error:
        result["isError"] = True
    return result


def handle_request(req: Any) -> Dict[str, Any]:
    """Handle an MCP JSON-RPC request."""
    if not isinstance(req, dict):
        return _make_error(None, -32600, "Invalid Request")
    req_id = req.get("id")
    method = str(req.get("method") or "")
    params = req.get("params")
    if params is None:
        params = {}

    # MCP protocol methods
    if method == "initialize":
        init_params = params if isinstance(params, dict) else {}
        return _make_response(req_id, {
            "protocolVersion": _negotiated_protocol_version(init_params),
            "capabilities": {
                "tools": {
                    "listChanged": False,
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
        if not isinstance(params, dict):
            return _make_error(req_id, -32602, "tools/list params must be an object")
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
        result = {"tools": page}
        if next_cursor:
            result["nextCursor"] = next_cursor
        return _make_response(req_id, result)

    # Optional MCP surfaces (return empty to avoid noisy "Method not found" in some runtimes)
    if method == "resources/list":
        return _make_response(req_id, {"resources": []})

    if method == "prompts/list":
        return _make_response(req_id, {"prompts": []})

    if method == "roots/list":
        return _make_response(req_id, {"roots": []})

    # Common no-op requests some clients send
    if method == "ping":
        return _make_response(req_id, {})

    if method == "logging/setLevel":
        return _make_response(req_id, {})

    if method == "tools/call":
        if not isinstance(params, dict):
            return _make_error(req_id, -32602, "tools/call params must be an object")
        tool_name_raw = params.get("name")
        if not isinstance(tool_name_raw, str) or not tool_name_raw:
            return _make_error(req_id, -32602, "tools/call name must be a non-empty string")
        tool_name = tool_name_raw
        arguments_raw = params.get("arguments")
        if arguments_raw is None:
            arguments: Dict[str, Any] = {}
        elif isinstance(arguments_raw, dict):
            arguments = arguments_raw
        else:
            return _make_error(req_id, -32602, "tools/call arguments must be an object")
        if tool_name not in _BUILTIN_TOOL_NAMES:
            visible_names = {
                str(tool.get("name") or "").strip()
                for tool in list_tools_for_caller()
                if isinstance(tool, dict) and str(tool.get("name") or "").strip()
            }
            if tool_name not in visible_names:
                return _make_error(req_id, -32602, f"Unknown tool: {tool_name}")

        try:
            result = handle_tool_call(tool_name, arguments)
            return _make_response(req_id, _make_tool_result(result))
        except MCPError as e:
            if e.code == "unknown_tool":
                return _make_error(req_id, -32602, f"Unknown tool: {tool_name}")
            return _make_response(
                req_id,
                _make_tool_result(
                    {
                        "error": {
                            "code": e.code,
                            "message": e.message,
                            "details": e.details,
                        }
                    },
                    is_error=True,
                ),
            )
        except Exception as e:
            return _make_response(
                req_id,
                _make_tool_result(
                    {
                        "error": {
                            "code": "internal_error",
                            "message": str(e),
                        }
                    },
                    is_error=True,
                ),
            )

    # Unknown method
    return _make_error(req_id, -32601, f"Method not found: {method}")


def main() -> int:
    """MCP server main loop (stdio mode)."""
    # Silence routine logs (e.g. expected "no daemon yet" for top-level collab MCP)
    # so they do not pollute stderr (Grok captures MCP stderr and may treat output as failure).
    import logging
    for name in ("cccc", "cccc.daemon", "cccc.daemon.server", "cccc.kernel", "cccc.util"):
        logging.getLogger(name).setLevel(logging.ERROR)
    while True:
        msg = _read_message()
        if msg is None:
            break

        resp = handle_request(msg)
        if resp:  # Notifications return {}
            _write_message(resp)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
