"""Project successful remote-send receipts into source group chat ledger."""

from __future__ import annotations

import logging
from pathlib import Path
from typing import Any, Dict, Optional

import yaml

from ...kernel.group import Group, load_group
from ...kernel.group_bridge.registration import get_registration
from ...kernel.inbox import iter_events
from ...kernel.ledger import append_event
from ...kernel.ledger_segments import ensure_ledger_layout

LOGGER = logging.getLogger("cccc.group_bridge.receipt_projection")


def project_remote_send_receipt(receipt: Dict[str, Any], *, home: Optional[Path] = None) -> bool:
    try:
        return _project_remote_send_receipt(receipt, home=home)
    except Exception:
        LOGGER.exception("failed to project remote send receipt")
        return False


def _project_remote_send_receipt(receipt: Dict[str, Any], *, home: Optional[Path] = None) -> bool:
    source = receipt if isinstance(receipt, dict) else {}
    status = str(source.get("status") or "").strip()
    if status != "sent":
        return False
    source_event_id = str(source.get("source_event_id") or "").strip()
    remote_event_id = str(source.get("remote_event_id") or "").strip()
    registration_id = str(source.get("registration_id") or "").strip()
    idempotency_key = str(source.get("idempotency_key") or "").strip()
    src_group_id = str(source.get("src_group_id") or source.get("group_id") or "").strip()
    if not source_event_id or not remote_event_id or not registration_id or not idempotency_key or not src_group_id:
        return False

    reg = get_registration(registration_id, home=home)
    dst_group_id = str((reg or {}).get("remote_group_id") or source.get("dst_group_id") or "").strip()
    if not dst_group_id:
        return False
    src_group = _load_group(src_group_id, home=home)
    if src_group is None:
        return False
    if _has_projected_receipt(
        src_group,
        registration_id=registration_id,
        idempotency_key=idempotency_key,
        remote_event_id=remote_event_id,
    ):
        return False

    append_event(
        src_group.ledger_path,
        kind="chat.cross_group_receipt",
        group_id=src_group.group_id,
        scope_key="",
        by="system",
        data={
            "source_event_id": source_event_id,
            "dst_group_id": dst_group_id,
            "dst_event_id": "",
            "remote_event_id": remote_event_id,
            "registration_id": registration_id,
            "idempotency_key": idempotency_key,
            "status": status,
        },
    )
    return True


def _load_group(group_id: str, *, home: Optional[Path]) -> Optional[Group]:
    if home is None:
        return load_group(group_id)
    gid = str(group_id or "").strip()
    if not gid:
        return None
    group_path = Path(home) / "groups" / gid
    group_doc_path = group_path / "group.yaml"
    if not group_doc_path.exists():
        return None
    try:
        doc = yaml.safe_load(group_doc_path.read_text(encoding="utf-8")) or {}
    except Exception:
        return None
    if not isinstance(doc, dict):
        return None
    ensure_ledger_layout(group_path)
    return Group(group_id=gid, path=group_path, doc=doc)


def _has_projected_receipt(
    src_group: Group,
    *,
    registration_id: str,
    idempotency_key: str,
    remote_event_id: str,
) -> bool:
    for event in iter_events(src_group.ledger_path):
        if str(event.get("kind") or "").strip() != "chat.cross_group_receipt":
            continue
        data = event.get("data") if isinstance(event.get("data"), dict) else {}
        if (
            str(data.get("registration_id") or "").strip() == registration_id
            and str(data.get("idempotency_key") or "").strip() == idempotency_key
            and str(data.get("remote_event_id") or "").strip() == remote_event_id
        ):
            return True
    return False
