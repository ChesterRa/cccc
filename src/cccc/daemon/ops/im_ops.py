"""IM authentication operations for daemon."""

from __future__ import annotations

from typing import Any, Dict, Optional, Tuple

from ...contracts.v1 import DaemonError, DaemonResponse
from ...kernel.group import load_group
from ...ports.im.auth import KeyManager
from ...ports.im.subscribers import SubscriberManager


def _error(code: str, message: str) -> DaemonResponse:
    return DaemonResponse(ok=False, error=DaemonError(code=code, message=message))


def _load_km(args: Dict[str, Any]) -> Tuple[Optional[DaemonResponse], Optional[KeyManager]]:
    """Validate group_id, load group, and return a KeyManager.

    Returns (error_response, None) on failure, or (None, km) on success.
    """
    group_id = str(args.get("group_id") or "").strip()
    if not group_id:
        return _error("missing_group_id", "group_id is required"), None
    group = load_group(group_id)
    if group is None:
        return _error("group_not_found", f"group not found: {group_id}"), None
    return None, KeyManager(group.path / "state")


def handle_im_bind_chat(args: Dict[str, Any]) -> DaemonResponse:
    """Bind a pending key to authorize a chat."""
    key = str(args.get("key") or "").strip()
    if not key:
        return _error("missing_key", "key is required")

    err, km = _load_km(args)
    if err is not None:
        return err

    pending = km.get_pending_key(key)
    if pending is None:
        return _error("invalid_key", "key not found or expired")

    chat_id = str(pending["chat_id"])
    thread_id = int(pending.get("thread_id") or 0)
    platform = str(pending.get("platform") or "")

    km.authorize(chat_id, thread_id, platform, key)

    # Auto-subscribe the chat so the user doesn't need a separate /subscribe step.
    group_id = str(args.get("group_id") or "").strip()
    group = load_group(group_id)
    if group is not None:
        sm = SubscriberManager(group.path / "state")
        sm.subscribe(chat_id, chat_title="", thread_id=thread_id, platform=platform)

    return DaemonResponse(ok=True, result={
        "chat_id": chat_id,
        "thread_id": thread_id,
        "platform": platform,
    })


def handle_im_list_authorized(args: Dict[str, Any]) -> DaemonResponse:
    """List all authorized chats for a group."""
    err, km = _load_km(args)
    if err is not None:
        return err
    return DaemonResponse(ok=True, result={"authorized": km.list_authorized()})


def handle_im_revoke_chat(args: Dict[str, Any]) -> DaemonResponse:
    """Revoke authorization for a chat."""
    chat_id = str(args.get("chat_id") or "").strip()
    try:
        thread_id = int(args.get("thread_id") or 0)
    except Exception:
        thread_id = 0

    if not chat_id:
        return _error("missing_chat_id", "chat_id is required")

    err, km = _load_km(args)
    if err is not None:
        return err

    revoked = km.revoke(chat_id, thread_id)
    return DaemonResponse(ok=True, result={"revoked": revoked})


def try_handle_im_op(op: str, args: Dict[str, Any]) -> Optional[DaemonResponse]:
    if op == "im_bind_chat":
        return handle_im_bind_chat(args)
    if op == "im_list_authorized":
        return handle_im_list_authorized(args)
    if op == "im_revoke_chat":
        return handle_im_revoke_chat(args)
    return None
