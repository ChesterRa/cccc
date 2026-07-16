"""Daemon-owned ChatGPT Web Model browser surface operations."""

from __future__ import annotations

from typing import Any, Dict, Optional

from ...contracts.v1 import DaemonError, DaemonResponse
from ...kernel.actors import find_actor
from ...kernel.group import load_group
from . import web_model_browser_session


def _error(code: str, message: str, *, details: Optional[Dict[str, Any]] = None) -> DaemonResponse:
    return DaemonResponse(ok=False, error=DaemonError(code=code, message=message, details=(details or {})))


def _validate_web_model_actor(group_id: str, actor_id: str, *, allow_global_setup: bool = False) -> tuple[str, str] | DaemonResponse:
    gid = str(group_id or "").strip()
    aid = str(actor_id or "").strip()
    if allow_global_setup and not gid and not aid:
        return gid, aid
    if not gid:
        return _error("missing_group_id", "missing group_id")
    if not aid:
        return _error("missing_actor_id", "missing actor_id")
    group = load_group(gid)
    if group is None:
        return _error("group_not_found", f"group not found: {gid}")
    actor = find_actor(group, aid)
    if not isinstance(actor, dict):
        return _error("actor_not_found", f"actor not found: {aid}")
    if str(actor.get("runtime") or "").strip().lower() != "web_model":
        return _error(
            "invalid_actor_runtime",
            "ChatGPT browser sessions can only be bound to actors using runtime=web_model",
            details={"group_id": gid, "actor_id": aid},
        )
    return gid, aid


def handle_web_model_browser_open(args: Dict[str, Any]) -> DaemonResponse:
    checked = _validate_web_model_actor(args.get("group_id"), args.get("actor_id"), allow_global_setup=True)
    if isinstance(checked, DaemonResponse):
        return checked
    gid, aid = checked
    try:
        surface = web_model_browser_session.open_web_model_chatgpt_browser_session(
            group_id=gid,
            actor_id=aid,
            width=int(args.get("width") or 1366),
            height=int(args.get("height") or 900),
        )
    except Exception as exc:
        return _error("web_model_browser_open_failed", str(exc))
    return DaemonResponse(ok=True, result={"group_id": gid, "actor_id": aid, "browser_surface": surface})


def handle_web_model_browser_info(args: Dict[str, Any]) -> DaemonResponse:
    checked = _validate_web_model_actor(args.get("group_id"), args.get("actor_id"), allow_global_setup=True)
    if isinstance(checked, DaemonResponse):
        return checked
    gid, aid = checked
    try:
        surface = web_model_browser_session.get_web_model_chatgpt_browser_session_state(group_id=gid, actor_id=aid)
    except Exception as exc:
        return _error("web_model_browser_info_failed", str(exc))
    return DaemonResponse(ok=True, result={"group_id": gid, "actor_id": aid, "browser_surface": surface})


def handle_web_model_browser_close(args: Dict[str, Any]) -> DaemonResponse:
    checked = _validate_web_model_actor(args.get("group_id"), args.get("actor_id"), allow_global_setup=True)
    if isinstance(checked, DaemonResponse):
        return checked
    gid, aid = checked
    try:
        result = web_model_browser_session.close_web_model_chatgpt_browser_session(group_id=gid, actor_id=aid)
    except Exception as exc:
        return _error("web_model_browser_close_failed", str(exc))
    return DaemonResponse(ok=True, result={"group_id": gid, "actor_id": aid, **result})


def try_handle_web_model_browser_op(op: str, args: Dict[str, Any]) -> Optional[DaemonResponse]:
    if op == "web_model_browser_open":
        return handle_web_model_browser_open(args)
    if op == "web_model_browser_info":
        return handle_web_model_browser_info(args)
    if op == "web_model_browser_close":
        return handle_web_model_browser_close(args)
    return None
