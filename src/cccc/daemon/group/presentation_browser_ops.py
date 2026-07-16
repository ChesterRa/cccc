"""Daemon ops for the Presentation browser-surface transport proof."""

from __future__ import annotations

from typing import Any, Dict, Optional

from ...contracts.v1 import DaemonError, DaemonResponse
from ...kernel.actors import find_actor
from ...kernel.group import load_group
from .presentation_browser_runtime import (
    close_browser_surface_session,
    get_browser_surface_session_state,
    open_browser_surface_session,
)

_SLOT_IDS = {"slot-1", "slot-2", "slot-3", "slot-4"}


def _error(code: str, message: str, *, details: Optional[Dict[str, Any]] = None) -> DaemonResponse:
    return DaemonResponse(ok=False, error=DaemonError(code=code, message=message, details=(details or {})))


def _validate_actor(group: Any, by: str) -> None:
    who = str(by or "").strip()
    if not who or who in {"user", "system"}:
        return
    if not isinstance(find_actor(group, who), dict):
        raise ValueError(f"unknown actor: {who}")


def _normalize_slot_id(value: Any) -> str:
    slot_id = str(value or "").strip().lower().replace("_", "-")
    if slot_id.isdigit():
        slot_id = f"slot-{int(slot_id)}"
    if slot_id not in _SLOT_IDS:
        raise ValueError("slot must be one of: slot-1, slot-2, slot-3, slot-4")
    return slot_id


def handle_presentation_browser_open(args: Dict[str, Any]) -> DaemonResponse:
    group_id = str(args.get("group_id") or "").strip()
    slot_id = str(args.get("slot") or "").strip()
    by = str(args.get("by") or "user").strip() or "user"
    url = str(args.get("url") or "").strip()
    width = int(args.get("width") or 1280)
    height = int(args.get("height") or 800)
    if not group_id:
        return _error("missing_group_id", "missing group_id")
    if not url:
        return _error("missing_url", "missing url")
    group = load_group(group_id)
    if group is None:
        return _error("group_not_found", f"group not found: {group_id}")
    try:
        _validate_actor(group, by)
        normalized_slot_id = _normalize_slot_id(slot_id)
        state = open_browser_surface_session(
            group_id=group.group_id,
            slot_id=normalized_slot_id,
            url=url,
            width=width,
            height=height,
        )
    except Exception as exc:
        return _error("browser_surface_open_failed", str(exc))
    return DaemonResponse(ok=True, result={"group_id": group.group_id, "slot_id": normalized_slot_id, "browser_surface": state})


def handle_presentation_browser_info(args: Dict[str, Any]) -> DaemonResponse:
    group_id = str(args.get("group_id") or "").strip()
    slot_id = str(args.get("slot") or "").strip()
    if not group_id:
        return _error("missing_group_id", "missing group_id")
    group = load_group(group_id)
    if group is None:
        return _error("group_not_found", f"group not found: {group_id}")
    try:
        normalized_slot_id = _normalize_slot_id(slot_id)
    except Exception as exc:
        return _error("browser_surface_info_failed", str(exc))
    state = get_browser_surface_session_state(group_id=group.group_id, slot_id=normalized_slot_id)
    return DaemonResponse(ok=True, result={"group_id": group.group_id, "slot_id": normalized_slot_id, "browser_surface": state})


def handle_presentation_browser_close(args: Dict[str, Any]) -> DaemonResponse:
    group_id = str(args.get("group_id") or "").strip()
    slot_id = str(args.get("slot") or "").strip()
    by = str(args.get("by") or "user").strip() or "user"
    if not group_id:
        return _error("missing_group_id", "missing group_id")
    group = load_group(group_id)
    if group is None:
        return _error("group_not_found", f"group not found: {group_id}")
    try:
        _validate_actor(group, by)
        normalized_slot_id = _normalize_slot_id(slot_id)
        result = close_browser_surface_session(group_id=group.group_id, slot_id=normalized_slot_id)
    except Exception as exc:
        return _error("browser_surface_close_failed", str(exc))
    return DaemonResponse(ok=True, result={"group_id": group.group_id, "slot_id": normalized_slot_id, **result})


def try_handle_presentation_browser_op(op: str, args: Dict[str, Any]) -> Optional[DaemonResponse]:
    if op == "presentation_browser_open":
        return handle_presentation_browser_open(args)
    if op == "presentation_browser_info":
        return handle_presentation_browser_info(args)
    if op == "presentation_browser_close":
        return handle_presentation_browser_close(args)
    return None
