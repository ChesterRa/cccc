"""Canonical remote chat payload construction.

Federation dispatch owns provenance semantics. Transports only serialize the
standard payload to their wire protocol.
"""

from __future__ import annotations

from typing import Any, Dict, TYPE_CHECKING

from .remote_attachments import build_remote_attachment_payloads
from ...kernel.group import load_group

if TYPE_CHECKING:
    from .transports.base import RemoteMessageEnvelope


def build_remote_chat_payload(envelope: "RemoteMessageEnvelope") -> Dict[str, Any]:
    payload = envelope.payload
    body: Dict[str, Any] = {
        "text": payload.text,
        "to": list(payload.to),
        "priority": payload.priority,
        "reply_required": payload.reply_required,
        "idempotency_key": envelope.idempotency_key,
        "source_platform": envelope.transport,
        "source_user_id": envelope.source_peer_id,
        "src_group_id": envelope.src_group_id,
        "src_event_id": envelope.source_event_id or envelope.idempotency_key,
    }
    source_peer_id = str(envelope.source_peer_id or "").strip()
    if source_peer_id:
        body["by"] = f"federation:{source_peer_id}"
    source_multiaddrs = [str(addr).strip() for addr in (envelope.source_multiaddrs or ()) if str(addr).strip()]
    if source_multiaddrs:
        body["source_multiaddrs"] = source_multiaddrs
    if payload.attachments:
        group = load_group(envelope.src_group_id)
        if group is None:
            raise ValueError(f"group not found: {envelope.src_group_id}")
        body["attachments"] = build_remote_attachment_payloads(group, payload.attachments)
    if envelope.reply_to_remote_event_id:
        body["reply_to"] = envelope.reply_to_remote_event_id
    if envelope.federation_thread:
        body["federation_thread"] = envelope.federation_thread
    return body
