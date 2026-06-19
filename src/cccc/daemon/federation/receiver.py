"""Inbound federation session remote-send authorization and ledger append."""

from __future__ import annotations

from pathlib import Path
import threading
from typing import Any, Dict, Optional

from ...contracts.v1.federation import RemoteSendPayload
from ...contracts.v1.message import ChatMessageData
from ...kernel.federation.pairing import list_trusts
from ...kernel.actors import resolve_recipient_tokens
from ...kernel.group import load_group
from ...kernel.ledger import append_event, read_last_lines
from ..messaging.chat_delivery_ops import deliver_appended_chat_message
from ..messaging.post_commit import run_group_chat_post_commit
from .peer_address_sync import sync_federation_peer_multiaddrs
from .remote_dispatch import retry_remote_send_for_peer


def receive_address_announce(
    *,
    target_group_id: str,
    src_group_id: str,
    remote_peer_id: str,
    multiaddrs: list[str],
    home: Optional[Path] = None,
    retry_pending: bool = True,
) -> Dict[str, Any]:
    target_gid = str(target_group_id or "").strip()
    src_gid = str(src_group_id or "").strip()
    peer_id = str(remote_peer_id or "").strip()
    addrs = [str(addr or "").strip() for addr in (multiaddrs or []) if str(addr or "").strip()]
    if not target_gid:
        return _error("missing_group_id", "target group id is required")
    if not src_gid:
        return _error("missing_src_group_id", "source group id is required")
    if not peer_id:
        return _error("missing_remote_peer_id", "remote peer id is required")
    if not addrs:
        return _error("missing_multiaddr", "remote multiaddr is required")
    if _load_group(target_gid, home=home) is None:
        return _error("group_not_found", "target group was not found")
    if _find_active_trust(target_group_id=target_gid, src_group_id=src_gid, remote_peer_id=peer_id, home=home) is None:
        return _error("unauthorized_peer", "remote peer is not trusted for this group")
    updated = sync_federation_peer_multiaddrs(
        group_id=target_gid,
        remote_group_id=src_gid,
        remote_peer_id=peer_id,
        multiaddrs=addrs,
        home=home,
    )
    retry = (
        retry_remote_send_for_peer(
            group_id=target_gid,
            remote_group_id=src_gid,
            remote_peer_id=peer_id,
            home=home,
        )
        if retry_pending
        else {}
    )
    return {
        "ok": True,
        "updated": True,
        "trust_updates": int(updated.get("trust_updates") or 0),
        "registration_updates": int(updated.get("registration_updates") or 0),
        "retried": int(retry.get("sent") or 0),
    }


def defer_pending_retry_for_peer(
    *,
    target_group_id: str,
    src_group_id: str,
    remote_peer_id: str,
    home: Optional[Path] = None,
) -> None:
    target_gid = str(target_group_id or "").strip()
    src_gid = str(src_group_id or "").strip()
    peer_id = str(remote_peer_id or "").strip()
    if not target_gid or not src_gid or not peer_id:
        return

    def run() -> None:
        retry_remote_send_for_peer(
            group_id=target_gid,
            remote_group_id=src_gid,
            remote_peer_id=peer_id,
            home=home,
        )

    threading.Thread(target=run, name="cccc-federation-address-announce-retry", daemon=True).start()


def receive_remote_send(
    *,
    target_group_id: str,
    src_group_id: str,
    remote_peer_id: str,
    payload: Dict[str, Any],
    idempotency_key: str,
    home: Optional[Path] = None,
) -> Dict[str, Any]:
    target_gid = str(target_group_id or "").strip()
    src_gid = str(src_group_id or "").strip()
    peer_id = str(remote_peer_id or "").strip()
    key = str(idempotency_key or "").strip()
    if not target_gid:
        return _error("missing_group_id", "target group id is required")
    if not src_gid:
        return _error("missing_src_group_id", "source group id is required")
    if not peer_id:
        return _error("missing_remote_peer_id", "remote peer id is required")
    if not key:
        return _error("missing_idempotency_key", "idempotency key is required")

    group = _load_group(target_gid, home=home)
    if group is None:
        return _error("group_not_found", "target group was not found")

    trust = _find_active_trust(target_group_id=target_gid, src_group_id=src_gid, remote_peer_id=peer_id, home=home)
    if trust is None:
        return _error("unauthorized_peer", "remote peer is not trusted for this group")

    payload_doc = payload if isinstance(payload, dict) else {}
    try:
        msg = RemoteSendPayload(
            **{
                key: payload_doc.get(key)
                for key in ("text", "format", "priority", "reply_required", "to", "refs", "attachments")
                if key in payload_doc
            }
        )
    except Exception:
        return _error("invalid_payload", "remote payload is invalid")
    if msg.attachments:
        return _error("unsupported_attachments", "attachments are not supported by federation sessions")
    if msg.refs:
        return _error("unsupported_refs", "refs are not supported by federation sessions")
    if not msg.text.strip():
        return _error("empty_message", "message text cannot be empty")

    existing = _find_existing_event(group.ledger_path, client_id=key)
    if existing is not None:
        return {"ok": True, "event_id": str(existing.get("id") or ""), "duplicate": True}

    event = append_event(
        group.ledger_path,
        kind="chat.message",
        group_id=group.group_id,
        scope_key=str(group.doc.get("active_scope_key") or ""),
        by=f"federation:{peer_id}",
        data=ChatMessageData(
            text=msg.text,
            format=msg.format,
            priority=msg.priority,
            reply_required=msg.reply_required,
            to=msg.to,
            refs=[],
            attachments=[],
            source_platform="federation_session",
            source_user_name=str(trust.get("remote_group_title") or src_gid),
            source_user_id=peer_id,
            src_group_id=src_gid,
            src_event_id=str(payload_doc.get("src_event_id") or "").strip() or None,
            client_id=key,
        ).model_dump(),
    )
    effective_to = _resolve_remote_send_to(group, msg.to)
    run_group_chat_post_commit(
        group.group_id,
        "federation-receive-delivery",
        lambda: deliver_appended_chat_message(
            group=group,
            event=event,
            by=f"federation:{peer_id}",
            effective_to=effective_to,
            text=msg.text,
            priority=msg.priority,
            reply_required=msg.reply_required,
            refs=[],
            attachments=[],
            source_platform="federation_session",
            source_user_name=str(trust.get("remote_group_title") or src_gid),
            source_user_id=peer_id,
            src_group_id=src_gid,
            src_event_id=str(payload_doc.get("src_event_id") or "").strip(),
        ),
    )
    return {"ok": True, "event_id": str(event.get("id") or ""), "duplicate": False}


def _resolve_remote_send_to(group: Any, to: list[str]) -> list[str]:
    try:
        return resolve_recipient_tokens(group, to) if to else ["@all"]
    except Exception:
        return [str(item).strip() for item in (to or []) if str(item).strip()] or ["@all"]


def _load_group(group_id: str, *, home: Optional[Path]):  # type: ignore[no-untyped-def]
    if home is None:
        return load_group(group_id)
    import os

    old_home = os.environ.get("CCCC_HOME")
    os.environ["CCCC_HOME"] = str(home)
    try:
        return load_group(group_id)
    finally:
        if old_home is None:
            os.environ.pop("CCCC_HOME", None)
        else:
            os.environ["CCCC_HOME"] = old_home


def _find_active_trust(*, target_group_id: str, src_group_id: str, remote_peer_id: str, home: Optional[Path]) -> Optional[Dict[str, Any]]:
    for trust in list_trusts(group_id=target_group_id, home=home):
        if str(trust.get("status") or "") != "active":
            continue
        if str(trust.get("remote_group_id") or "") != src_group_id:
            continue
        if str(trust.get("remote_peer_id") or "") != remote_peer_id:
            continue
        return trust
    return None


def _find_existing_event(ledger_path: Any, *, client_id: str) -> Optional[Dict[str, Any]]:
    for line in reversed(read_last_lines(ledger_path, 1000)):
        try:
            import json

            event = json.loads(line)
        except Exception:
            continue
        if str(event.get("kind") or "") != "chat.message":
            continue
        data = event.get("data") if isinstance(event.get("data"), dict) else {}
        if str(data.get("client_id") or "") == client_id:
            return event
    return None


def _error(code: str, message: str) -> Dict[str, Any]:
    return {"ok": False, "error": {"code": code, "message": message}}
