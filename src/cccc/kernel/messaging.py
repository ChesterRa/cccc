from __future__ import annotations

from typing import Any, Dict, List, Literal

from .actors import list_actors
from .group import Group
from .inbox import is_message_for_actor
from ..util.conv import coerce_bool


DefaultSendTo = Literal["foreman", "broadcast"]


def get_default_send_to(group_doc: Dict[str, Any]) -> DefaultSendTo:
    """Return the group policy for empty-recipient sends."""
    messaging = group_doc.get("messaging")
    if not isinstance(messaging, dict):
        return "foreman"
    value = str(messaging.get("default_send_to") or "").strip()
    if value in ("foreman", "broadcast"):
        return value  # type: ignore[return-value]
    return "foreman"


def _enabled_actor_ids(group: Group) -> List[str]:
    out: List[str] = []
    for a in list_actors(group):
        if not isinstance(a, dict):
            continue
        if not coerce_bool(a.get("enabled"), default=True):
            continue
        aid = str(a.get("id") or "").strip()
        if aid:
            out.append(aid)
    return out


def _disabled_actor_ids(group: Group) -> List[str]:
    out: List[str] = []
    for a in list_actors(group):
        if not isinstance(a, dict):
            continue
        if coerce_bool(a.get("enabled"), default=True):
            continue
        aid = str(a.get("id") or "").strip()
        if aid:
            out.append(aid)
    return out


def targets_any_agent(to: List[str]) -> bool:
    """Return True if the to-list targets any agents (actors) vs user-only."""
    if not to:
        return True  # broadcast (agent-side) semantics
    for tok in to:
        t = str(tok or "").strip()
        if not t:
            continue
        if t in ("@all", "@peers", "@foreman"):
            return True
        if t == "user" or t == "@user":
            continue
        if t.startswith("@"):
            # Unknown mention selectors are ignored for routing; treat as agent-targeted to be safe.
            return True
        return True  # actor_id
    return False


def enabled_recipient_actor_ids(group: Group, to: List[str]) -> List[str]:
    """Return enabled actor ids that would receive a chat.message with the given to-list."""
    enabled_ids = _enabled_actor_ids(group)
    if not enabled_ids:
        return []

    ev = {"kind": "chat.message", "data": {"to": list(to)}}
    return [aid for aid in enabled_ids if is_message_for_actor(group, actor_id=aid, event=ev)]


def disabled_recipient_actor_ids(group: Group, to: List[str]) -> List[str]:
    """Return disabled actor ids that would receive a chat.message with the given to-list.

    Used by auto-wake: when no enabled actors match, check if disabled ones do.
    """
    disabled_ids = _disabled_actor_ids(group)
    if not disabled_ids:
        return []

    ev = {"kind": "chat.message", "data": {"to": list(to)}}
    return [aid for aid in disabled_ids if is_message_for_actor(group, actor_id=aid, event=ev)]


def default_reply_recipients(group: Group, *, by: str, original_event: Dict[str, Any]) -> List[str]:
    """Compute default recipients for a reply when 'to' is omitted.

    Rules:
    - Replying to someone else's message defaults to the original sender.
    - Replying to your own message defaults to the original message recipients (to preserve audience).
    - If the original message has no recipients, fall back to the group default send policy.
    """
    who = str(by or "").strip() or "user"
    original_by = str(original_event.get("by") or "").strip()

    # Extract original recipients (if present).
    original_to: List[str] = []
    data = original_event.get("data")
    if isinstance(data, dict):
        raw_to = data.get("to")
        if isinstance(raw_to, list):
            original_to = [str(x).strip() for x in raw_to if isinstance(x, str) and str(x).strip()]

    if original_by and original_by != who:
        return ["user"] if original_by == "user" else [original_by]

    if original_to:
        return original_to

    return ["@foreman"] if get_default_send_to(group.doc) == "foreman" else []
