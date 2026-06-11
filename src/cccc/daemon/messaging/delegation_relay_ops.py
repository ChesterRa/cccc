"""Deterministic local-group delegation relay (T402).

When a user types ``#<group>`` in a message, the user's message itself stays in
the local group (T400 keeps it from being a direct cross-group send). This op
performs the *contact* the user actually asked for: a deterministic relay sent
into the target group, authored by a local-group agent (not the user), carrying
the original request + target context.

It reuses the existing ``send_cross_group`` capability via ``dispatch_send`` —
the relay therefore appears in the target group's ledger as ``by=<agent>`` with
RELAYED FROM provenance, never as a direct user cross-send.
"""

from __future__ import annotations

from typing import Any, Callable, Dict, Optional, Tuple

from ...contracts.v1 import DaemonError, DaemonResponse
from ...kernel.actors import find_actor, find_foreman, is_internal_actor, list_visible_actors
from ...kernel.group import load_group
from .delegation_contract import new_delegation_id, render_delegation_request
from .delegation_targets import resolve_target_delegatee


def _error(code: str, message: str, *, details: Optional[Dict[str, Any]] = None) -> DaemonResponse:
    return DaemonResponse(ok=False, error=DaemonError(code=code, message=message, details=(details or {})))


def pick_relay_sender(group: Any, *, preferred: str = "") -> str:
    """Choose the local agent that issues the contact.

    Priority: an explicitly delegated/responsible agent, then the foreman, then
    any visible non-user actor. Returns "" when none can act.
    """
    pref = str(preferred or "").strip()
    if pref and pref != "user":
        actor = find_actor(group, pref)
        if isinstance(actor, dict) and not is_internal_actor(actor):
            return pref
    foreman = find_foreman(group)
    if isinstance(foreman, dict) and not is_internal_actor(foreman):
        fid = str(foreman.get("id") or "").strip()
        if fid:
            return fid
    for actor in list_visible_actors(group):
        aid = str(actor.get("id") or "").strip()
        if aid and aid != "user":
            return aid
    return ""


def handle_relay_user_delegation(
    args: Dict[str, Any],
    *,
    dispatch_send: Callable[[str, Dict[str, Any]], Tuple[DaemonResponse, bool]],
) -> DaemonResponse:
    src_group_id = str(args.get("group_id") or "").strip()
    dst_group_id = str(args.get("dst_group_id") or "").strip()
    user_request = str(args.get("text") or "")
    requester = str(args.get("by") or "user").strip() or "user"
    preferred_sender = str(args.get("relay_sender") or "").strip()

    if not src_group_id:
        return _error("missing_group_id", "missing group_id")
    if not dst_group_id:
        return _error("missing_dst_group_id", "missing dst_group_id")
    if src_group_id == dst_group_id:
        return _error("invalid_dst_group_id", "dst_group_id must be different from group_id")

    src_group = load_group(src_group_id)
    if src_group is None:
        return _error("group_not_found", f"group not found: {src_group_id}")
    dst_group = load_group(dst_group_id)
    if dst_group is None:
        return _error("group_not_found", f"target group not found: {dst_group_id}", details={"dst_group_id": dst_group_id})

    sender = pick_relay_sender(src_group, preferred=preferred_sender)
    if not sender:
        return _error(
            "no_relay_agent",
            "no local agent is available to contact the target group",
            details={"group_id": src_group_id},
        )

    # Address the delegation to the target group's foreman by default, or to an
    # explicitly requested target-group agent. Never a generic fallback or @all.
    requested_target_actor = str(args.get("target_actor") or "").strip()
    target_actor_id, target_error = resolve_target_delegatee(dst_group, requested_target_actor)
    if target_error:
        messages = {
            "no_target_foreman": "the target group has no usable foreman to take the delegated task",
            "target_agent_not_found": f"requested target agent not found in {dst_group_id}: {requested_target_actor}",
            "target_agent_unavailable": f"requested target agent is unavailable in {dst_group_id}: {requested_target_actor}",
        }
        return _error(
            target_error,
            messages.get(target_error, "no target agent available"),
            details={"dst_group_id": dst_group_id, "requested_target_actor": requested_target_actor},
        )

    delegation_id = new_delegation_id()
    contact_text = render_delegation_request(
        delegation_id=delegation_id,
        source_group_id=src_group_id,
        target_group_id=dst_group_id,
        original_request=user_request,
        source_event_id=str(args.get("source_event_id") or "").strip(),
        requested_by=requester,
        relay_sender=sender,
        target_actor_id=target_actor_id,
        contact_text=str(args.get("contact_text") or "").strip(),
    )

    relay_resp, _ = dispatch_send(
        "send_cross_group",
        {
            "group_id": src_group_id,
            "dst_group_id": dst_group_id,
            "by": sender,
            "text": contact_text,
            "to": [target_actor_id],
            "priority": "normal",
            "reply_required": False,
        },
    )
    if not relay_resp.ok:
        err = relay_resp.error
        return _error(
            "relay_failed",
            getattr(err, "message", "relay failed") if err else "relay failed",
            details={"cause": getattr(err, "code", "") if err else "", "dst_group_id": dst_group_id},
        )

    result = relay_resp.result if isinstance(relay_resp.result, dict) else {}
    src_event = result.get("src_event") if isinstance(result.get("src_event"), dict) else {}
    dst_event = result.get("dst_event") if isinstance(result.get("dst_event"), dict) else {}
    return DaemonResponse(
        ok=True,
        result={
            "relay": {
                "delegation_id": delegation_id,
                "sender": sender,
                "target_actor_id": target_actor_id,
                "src_group_id": src_group_id,
                "dst_group_id": dst_group_id,
                "src_event_id": str((src_event or {}).get("id") or ""),
                "dst_event_id": str((dst_event or {}).get("id") or ""),
            }
        },
    )


def try_handle_delegation_relay_op(
    op: str,
    args: Dict[str, Any],
    *,
    dispatch_send: Optional[Callable[[str, Dict[str, Any]], Tuple[DaemonResponse, bool]]] = None,
) -> Optional[DaemonResponse]:
    if op == "relay_user_delegation":
        if dispatch_send is None:
            return _error("internal_error", "dispatch_send callback not configured")
        return handle_relay_user_delegation(args, dispatch_send=dispatch_send)
    return None
