"""Strict, bounded ACP/NDJSON contract shared by the Python runner tests."""
from __future__ import annotations

import json
from pathlib import Path, PureWindowsPath
from typing import Any, Dict, Optional, Set, Union

from ..contracts.v1.deepseek import DEEPSEEK_PROTOCOL_VERSION

MAX_FRAME_BYTES = 64 * 1024
MAX_PENDING_REQUESTS = 256
JsonRpcId = Union[int, str]


class ACPProtocolError(ValueError):
    """A malformed or out-of-generation ACP frame; the session must stop."""


def _request_id(value: Any) -> JsonRpcId:
    if isinstance(value, bool) or not isinstance(value, (int, str)):
        raise ACPProtocolError("json-rpc id must be a string or number")
    return value


class NDJSONSession:
    """Parse one ACP generation and track its bounded request/response set."""

    def __init__(self, *, max_frame_bytes: int = MAX_FRAME_BYTES, max_pending: int = MAX_PENDING_REQUESTS) -> None:
        self.max_frame_bytes = max(1, int(max_frame_bytes))
        self.max_pending = max(1, int(max_pending))
        self.pending: Set[JsonRpcId] = set()

    def register(self, request_id: Any) -> JsonRpcId:
        request_id = _request_id(request_id)
        if len(self.pending) >= self.max_pending:
            raise ACPProtocolError("pending request cap exceeded")
        if request_id in self.pending:
            raise ACPProtocolError("duplicate request id")
        self.pending.add(request_id)
        return request_id

    def feed_line(self, raw: Union[str, bytes]) -> Dict[str, Any]:
        data = raw.encode("utf-8") if isinstance(raw, str) else bytes(raw)
        if len(data) > self.max_frame_bytes:
            raise ACPProtocolError("frame exceeds byte cap")
        try:
            text = data.decode("utf-8")
            value = json.loads(text)
        except (UnicodeDecodeError, json.JSONDecodeError) as exc:
            raise ACPProtocolError("invalid UTF-8/JSON frame") from exc
        if not isinstance(value, dict):
            raise ACPProtocolError("NDJSON frame must be an object")
        if value.get("jsonrpc") != "2.0":
            raise ACPProtocolError("jsonrpc must be 2.0")
        has_id = "id" in value
        if has_id:
            request_id = _request_id(value["id"])
            if "method" in value:
                # ACP permission prompts are agent->client requests. They
                # carry an id for the client's response but are not pending
                # responses and must not consume a request slot.
                if value.get("method") != "session/request_permission":
                    raise ACPProtocolError("response frame cannot contain method")
                return value
            if request_id not in self.pending:
                raise ACPProtocolError("unknown response id")
            self.pending.remove(request_id)
        elif "method" not in value:
            raise ACPProtocolError("frame must be a response or notification")
        return value


def initialize_request(*, client_version: str = "0.4.34") -> Dict[str, Any]:
    return {
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": DEEPSEEK_PROTOCOL_VERSION,
            "clientCapabilities": {},
            "clientInfo": {"name": "cccc", "version": client_version},
        },
    }


def session_new_request(cwd: str) -> Dict[str, Any]:
    normalized_cwd = str(cwd or "")
    if not (Path(normalized_cwd).is_absolute() or PureWindowsPath(normalized_cwd).is_absolute()):
        raise ValueError("session/new cwd must be absolute")
    return {
        "jsonrpc": "2.0",
        "id": 2,
        "method": "session/new",
        "params": {"cwd": normalized_cwd, "mcpServers": [], "additionalDirectories": []},
    }


def validate_initialize_result(message: Dict[str, Any]) -> Dict[str, Any]:
    result = message.get("result")
    if not isinstance(result, dict) or result.get("protocolVersion") != DEEPSEEK_PROTOCOL_VERSION:
        raise ACPProtocolError(
            f"initialize protocolVersion must be {DEEPSEEK_PROTOCOL_VERSION}"
        )
    agent_info = result.get("agentInfo")
    if not isinstance(agent_info, dict) or not str(agent_info.get("name") or "").strip():
        raise ACPProtocolError("initialize agentInfo.name is required")
    return result


def validate_session_new_result(message: Dict[str, Any], *, seen: Optional[Set[str]] = None) -> str:
    result = message.get("result")
    session_id = str(result.get("sessionId") or "").strip() if isinstance(result, dict) else ""
    if not session_id:
        raise ACPProtocolError("session/new returned an empty sessionId")
    if seen is not None and session_id in seen:
        raise ACPProtocolError("session/new returned a duplicate sessionId")
    if seen is not None:
        seen.add(session_id)
    return session_id


def permission_outcome(options: Any, *, stopping: bool = False) -> Dict[str, Any]:
    if stopping or not isinstance(options, list):
        return {"outcome": {"outcome": "cancelled"}}
    for option in options:
        if isinstance(option, dict) and option.get("optionId") == "reject-once":
            return {"outcome": {"outcome": "selected", "optionId": "reject-once"}}
    return {"outcome": {"outcome": "cancelled"}}


def validate_session_update(message: Dict[str, Any], expected_session_id: str) -> Dict[str, Any]:
    if message.get("method") != "session/update":
        raise ACPProtocolError("expected session/update notification")
    params = message.get("params")
    if not isinstance(params, dict) or not expected_session_id or params.get("sessionId") != expected_session_id:
        raise ACPProtocolError("session/update belongs to another session")
    return params


def permission_request_id(message: Dict[str, Any], expected_session_id: str) -> JsonRpcId:
    if message.get("method") != "session/request_permission":
        raise ACPProtocolError("expected session/request_permission")
    params = message.get("params")
    if not isinstance(params, dict) or not expected_session_id or params.get("sessionId") != expected_session_id:
        raise ACPProtocolError("permission request belongs to another session")
    return _request_id(message.get("id"))


def terminal_stop_reason(message: Dict[str, Any]) -> str:
    result = message.get("result")
    return str(result.get("stopReason") or "").strip() if isinstance(result, dict) else ""
