"""Canonical runtime handoff evidence for chat messages."""

from __future__ import annotations

import hashlib
from typing import Any, Optional

from ...kernel.inbox import is_message_for_actor, iter_events, iter_events_reverse
from ...kernel.ledger import append_event
from ...util.file_lock import acquire_lockfile, release_lockfile


TERMINAL_NO_RETRY_STATES = frozenset({"accepted", "ambiguous"})
_DIRECT_MESSAGE_MODES = frozenset({"send", "request_reply"})
_MAIL_NOTICE_KINDS = frozenset({"mail_notice", "reply_notice"})


def delivery_id_for(
    *,
    group_id: str,
    actor_id: str,
    actor_created_at: str,
    source_event_id: str,
) -> str:
    seed = "\0".join((group_id, actor_id, actor_created_at, source_event_id))
    digest = hashlib.sha256(seed.encode("utf-8", errors="replace")).hexdigest()[:24]
    return f"delivery:{actor_id}:{digest}"


def latest_delivery_state(
    group: Any,
    *,
    actor_id: str,
    source_event_id: str,
) -> Optional[dict[str, Any]]:
    for event in iter_events_reverse(group.ledger_path):
        if str(event.get("kind") or "") != "runtime.delivery":
            continue
        data = event.get("data") if isinstance(event.get("data"), dict) else {}
        if (
            str(data.get("actor_id") or "").strip() == actor_id
            and str(data.get("source_event_id") or "").strip() == source_event_id
        ):
            return event
    return None


def append_delivery_state(
    group: Any,
    *,
    actor_id: str,
    actor_created_at: str,
    source_event_id: str,
    state: str,
    transport: str,
    reason: str = "",
) -> dict[str, Any]:
    return append_event(
        group.ledger_path,
        kind="runtime.delivery",
        group_id=group.group_id,
        scope_key="",
        by="system",
        data={
            "actor_id": actor_id,
            "source_event_id": source_event_id,
            "delivery_id": delivery_id_for(
                group_id=group.group_id,
                actor_id=actor_id,
                actor_created_at=actor_created_at,
                source_event_id=source_event_id,
            ),
            "state": state,
            "transport": transport,
            "reason": reason or None,
        },
    )


def claim_delivery(
    group: Any,
    *,
    actor_id: str,
    actor_created_at: str,
    source_event_id: str,
    transport: str,
    force_ambiguous: bool = False,
) -> tuple[bool, str]:
    claimed, states = claim_deliveries(
        group,
        deliveries=[(actor_id, actor_created_at, transport)],
        source_event_id=source_event_id,
        force_ambiguous=force_ambiguous,
    )
    return claimed, states.get(actor_id, "claimed")


def claim_deliveries(
    group: Any,
    *,
    deliveries: list[tuple[str, str, str]],
    source_event_id: str,
    force_ambiguous: bool = False,
) -> tuple[bool, dict[str, str]]:
    """Atomically reserve one source event for all requested recipients."""

    lock = acquire_lockfile(
        group.path / "state" / "ledger" / "runtime_delivery.lock",
        blocking=True,
    )
    try:
        states: dict[str, str] = {}
        for actor_id, _actor_created_at, _transport in deliveries:
            existing = latest_delivery_state(
                group,
                actor_id=actor_id,
                source_event_id=source_event_id,
            )
            existing_data = (
                existing.get("data")
                if isinstance(existing, dict) and isinstance(existing.get("data"), dict)
                else {}
            )
            existing_state = str(existing_data.get("state") or "").strip()
            states[actor_id] = existing_state
            if existing_state == "claimed":
                return False, states
            if existing_state == "accepted" or (
                existing_state == "ambiguous" and not force_ambiguous
            ):
                return False, states
        for actor_id, actor_created_at, transport in deliveries:
            append_delivery_state(
                group,
                actor_id=actor_id,
                actor_created_at=actor_created_at,
                source_event_id=source_event_id,
                state="claimed",
                transport=transport,
            )
            states[actor_id] = "claimed"
        return True, states
    finally:
        release_lockfile(lock)


def settle_stranded_claims(group: Any) -> int:
    """Close claims left without an outcome by a previous daemon process."""
    lock = acquire_lockfile(
        group.path / "state" / "ledger" / "runtime_delivery.lock",
        blocking=True,
    )
    try:
        latest: dict[tuple[str, str], tuple[str, str]] = {}
        for event in iter_events(group.ledger_path):
            kind = str(event.get("kind") or "")
            data = event.get("data") if isinstance(event.get("data"), dict) else {}
            if kind == "actor.add":
                actor = data.get("actor") if isinstance(data.get("actor"), dict) else {}
                actor_id = str(actor.get("id") or "").strip()
                if actor_id:
                    latest = {
                        key: value for key, value in latest.items() if key[0] != actor_id
                    }
                continue
            if kind != "runtime.delivery":
                continue
            actor_id = str(data.get("actor_id") or "").strip()
            source_event_id = str(data.get("source_event_id") or "").strip()
            if actor_id and source_event_id:
                latest[(actor_id, source_event_id)] = (
                    str(data.get("state") or "").strip(),
                    str(data.get("transport") or "").strip(),
                )

        actors = {
            str(actor.get("id") or "").strip(): actor
            for actor in group.doc.get("actors", [])
            if isinstance(actor, dict) and str(actor.get("id") or "").strip()
        }
        settled = 0
        for (actor_id, source_event_id), (state, transport) in latest.items():
            actor = actors.get(actor_id)
            if state != "claimed" or not isinstance(actor, dict):
                continue
            append_delivery_state(
                group,
                actor_id=actor_id,
                actor_created_at=str(actor.get("created_at") or "").strip(),
                source_event_id=source_event_id,
                state="ambiguous",
                transport=transport,
                reason="daemon restarted before the claimed handoff recorded an outcome",
            )
            settled += 1
        return settled
    finally:
        release_lockfile(lock)


def pending_runtime_delivery_events(
    group: Any,
    *,
    actor_id: str,
    actor_created_at: str,
    transport: str,
    limit: int,
    claim_unclaimed_chat: bool = False,
) -> list[dict[str, Any]]:
    """Return current-generation events whose runtime handoff is claimed.

    Browser delivery and pull-based Web Model delivery share this projection.
    Unclaimed Mail is deliberately absent. A manually promoted Mail already
    carrying a matching claim is direct runtime work. A pull adapter may claim
    otherwise-unclaimed Send messages because it is itself the transport boundary.
    """

    aid = str(actor_id or "").strip()
    if not aid:
        return []
    sources, latest_states = _current_generation_sources(group, actor_id=aid)

    out: list[dict[str, Any]] = []
    max_events = max(1, int(limit or 1))
    for event in sources:
        event_id = str(event.get("id") or "").strip()
        state, claimed_transport = latest_states.get(event_id, ("", ""))
        data = event.get("data") if isinstance(event.get("data"), dict) else {}
        direct_message = (
            str(event.get("kind") or "") == "chat.message"
            and str(data.get("message_mode") or "").strip() in _DIRECT_MESSAGE_MODES
        )
        if (
            state in {"", "failed"}
            and claim_unclaimed_chat
            and direct_message
        ):
            claimed, state = claim_delivery(
                group,
                actor_id=aid,
                actor_created_at=str(actor_created_at or "").strip(),
                source_event_id=event_id,
                transport=transport,
            )
            if claimed:
                state = "claimed"
                claimed_transport = transport
        if state != "claimed" or claimed_transport != transport:
            continue
        out.append(event)
        if len(out) >= max_events:
            break
    return out


def pending_runtime_delivery_sources(
    group: Any,
    *,
    actor_id: str,
    transport: str,
    limit: int,
) -> list[dict[str, Any]]:
    """Return direct work available to one transport without claiming it."""

    aid = str(actor_id or "").strip()
    if not aid:
        return []
    sources, latest_states = _current_generation_sources(group, actor_id=aid)
    out: list[dict[str, Any]] = []
    for event in sources:
        state, claimed_transport = latest_states.get(
            str(event.get("id") or "").strip(), ("", "")
        )
        data = event.get("data") if isinstance(event.get("data"), dict) else {}
        if (
            str(event.get("kind") or "") == "chat.message"
            and str(data.get("message_mode") or "").strip() == "mail"
            and state != "claimed"
        ):
            continue
        if state in TERMINAL_NO_RETRY_STATES:
            continue
        if state == "claimed" and transport and claimed_transport != transport:
            continue
        out.append(event)
        if len(out) >= max(1, int(limit or 1)):
            break
    return out


def _current_generation_sources(
    group: Any, *, actor_id: str
) -> tuple[list[dict[str, Any]], dict[str, tuple[str, str]]]:
    aid = str(actor_id or "").strip()
    sources: list[dict[str, Any]] = []
    latest_states: dict[str, tuple[str, str]] = {}
    for event in iter_events(group.ledger_path):
        kind = str(event.get("kind") or "")
        data = event.get("data") if isinstance(event.get("data"), dict) else {}
        added_actor = data.get("actor") if isinstance(data.get("actor"), dict) else {}
        if kind == "actor.add" and str(added_actor.get("id") or "").strip() == aid:
            sources = []
            latest_states = {}
            continue
        if kind == "runtime.delivery" and str(data.get("actor_id") or "").strip() == aid:
            source_event_id = str(data.get("source_event_id") or "").strip()
            if source_event_id:
                latest_states[source_event_id] = (
                    str(data.get("state") or "").strip(),
                    str(data.get("transport") or "").strip(),
                )
            continue
        event_id = str(event.get("id") or "").strip()
        if not event_id or str(event.get("by") or "").strip() == aid:
            continue
        if kind == "chat.message":
            if str(data.get("message_mode") or "").strip() not in {*_DIRECT_MESSAGE_MODES, "mail"}:
                continue
            if not is_message_for_actor(group, actor_id=aid, event=event):
                continue
            sources.append(event)
            continue
        if kind != "system.notify":
            continue
        notify_kind = str(data.get("kind") or "").strip()
        is_daemon_notice = notify_kind in _MAIL_NOTICE_KINDS
        if is_daemon_notice:
            addressed = str(data.get("target_actor_id") or "").strip() == aid
        else:
            addressed = is_message_for_actor(group, actor_id=aid, event=event)
        if not addressed:
            continue
        sources.append(event)

    return sources, latest_states
