"""Daemon request queue routing policy."""

from __future__ import annotations

from typing import Any

from ..util.conv import coerce_bool


FAST_QUEUE_OPS = {"send", "reply", "chat_ack"}

READ_QUEUE_OPS = {
    "branding_get",
    "capability_overview",
    "context_get",
    "groups",
    "group_space_status",
    "im_list_authorized",
    "im_list_pending",
    "actor_list",
    "actor_profile_list",
    "observability_get",
    "ping",
}

_SLASH_COMMAND_VIEWS = {"slash", "slash_commands"}


def _clean(value: Any) -> str:
    return str(value or "").strip().lower()


def should_use_read_queue(op: str, args: Any) -> bool:
    clean_op = _clean(op)
    if clean_op in READ_QUEUE_OPS:
        return True
    if clean_op == "group_space_provider_auth":
        action = _clean(args.get("action")) if isinstance(args, dict) else ""
        return action == "status"
    if clean_op == "capability_state":
        view = _clean(args.get("view")) if isinstance(args, dict) else ""
        return view in _SLASH_COMMAND_VIEWS
    if clean_op == "assistant_state":
        return coerce_bool(args.get("suppress_retry_notify"), default=False) if isinstance(args, dict) else False
    return False
