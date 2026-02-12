from __future__ import annotations

import os
from typing import Any, Dict, Optional

from ...daemon.server import call_daemon


class MCPError(Exception):
    """MCP tool call error"""

    def __init__(
        self, code: str, message: str, details: Optional[Dict[str, Any]] = None
    ):
        super().__init__(message)
        self.code = code
        self.message = message
        self.details = details or {}


def _env_str(name: str) -> str:
    value = os.environ.get(name)
    return str(value).strip() if value is not None else ""


def _validate_self_actor_id(actor_id: str) -> str:
    aid = str(actor_id or "").strip()
    if not aid:
        raise MCPError(code="missing_actor_id", message="missing actor_id")
    if aid == "user":
        raise MCPError(
            code="invalid_actor_id",
            message="actor_id 'user' is reserved; agents must not act as user",
        )
    return aid


def _resolve_group_id(arguments: Dict[str, Any]) -> str:
    """Resolve group_id from env or tool arguments (env wins)."""
    env_gid = _env_str("CCCC_GROUP_ID")
    arg_gid = str(arguments.get("group_id") or "").strip()
    gid = env_gid or arg_gid
    if not gid:
        raise MCPError(
            code="missing_group_id",
            message="missing group_id (set CCCC_GROUP_ID env or pass group_id)",
        )
    if env_gid and arg_gid and arg_gid != env_gid:
        raise MCPError(
            code="group_id_mismatch",
            message="group_id mismatch (tool args must match CCCC_GROUP_ID)",
            details={"env": env_gid, "arg": arg_gid},
        )
    return gid


def _resolve_self_actor_id(arguments: Dict[str, Any]) -> str:
    """Resolve the caller actor_id from env or tool arguments (env wins)."""
    env_aid = _env_str("CCCC_ACTOR_ID")
    arg_aid = str(arguments.get("actor_id") or "").strip()
    aid = env_aid or arg_aid
    if not aid:
        raise MCPError(
            code="missing_actor_id",
            message="missing actor_id (set CCCC_ACTOR_ID env or pass actor_id)",
        )
    if env_aid and arg_aid and arg_aid != env_aid:
        raise MCPError(
            code="actor_id_mismatch",
            message="actor_id mismatch (tool args must match CCCC_ACTOR_ID)",
            details={"env": env_aid, "arg": arg_aid},
        )
    return _validate_self_actor_id(aid)


def _resolve_caller_from_by(arguments: Dict[str, Any]) -> str:
    """Resolve caller identity from ``by`` arg or CCCC_ACTOR_ID env only.

    Use for tools where ``actor_id`` refers to a target actor, not the caller.
    """
    env_aid = _env_str("CCCC_ACTOR_ID")
    arg_by = str(arguments.get("by") or "").strip()
    aid = env_aid or arg_by
    if not aid:
        raise MCPError(
            code="missing_actor_id",
            message="missing actor id (set CCCC_ACTOR_ID env or pass by)",
        )
    if env_aid and arg_by and arg_by != env_aid:
        raise MCPError(
            code="actor_id_mismatch",
            message="by mismatch (tool args must match CCCC_ACTOR_ID)",
            details={"env": env_aid, "arg": arg_by},
        )
    return _validate_self_actor_id(aid)


def _resolve_caller_actor_id(arguments: Dict[str, Any]) -> str:
    """Resolve caller identity from ``by``, ``actor_id``, or CCCC_ACTOR_ID env."""
    env_aid = _env_str("CCCC_ACTOR_ID")
    arg_by = str(arguments.get("by") or "").strip()
    arg_actor_id = str(arguments.get("actor_id") or "").strip()
    if arg_by and arg_actor_id and arg_by != arg_actor_id:
        raise MCPError(
            code="actor_id_mismatch",
            message="by/actor_id mismatch (tool args must use one consistent actor id)",
            details={"by": arg_by, "actor_id": arg_actor_id},
        )
    arg_aid = arg_by or arg_actor_id
    aid = env_aid or arg_aid
    if not aid:
        raise MCPError(
            code="missing_actor_id",
            message="missing actor id (set CCCC_ACTOR_ID env or pass by/actor_id)",
        )
    if env_aid and arg_aid and arg_aid != env_aid:
        raise MCPError(
            code="actor_id_mismatch",
            message="actor id mismatch (tool args must match CCCC_ACTOR_ID)",
            details={"env": env_aid, "arg": arg_aid},
        )
    return _validate_self_actor_id(aid)


def _call_daemon_or_raise(req: Dict[str, Any]) -> Dict[str, Any]:
    """Call daemon, raise MCPError on failure."""
    resp = call_daemon(req)
    if not resp.get("ok"):
        err = resp.get("error") or {}
        if isinstance(err, dict):
            raise MCPError(
                code=str(err.get("code") or "daemon_error"),
                message=str(err.get("message") or "daemon error"),
                details=(
                    err.get("details") if isinstance(err.get("details"), dict) else {}
                ),
            )
        raise MCPError(code="daemon_error", message=str(err))
    return resp.get("result") if isinstance(resp.get("result"), dict) else {}
