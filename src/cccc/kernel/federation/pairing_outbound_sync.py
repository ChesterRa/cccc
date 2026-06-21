"""Joiner-side state transitions for remote pairing outbounds."""

from __future__ import annotations

from pathlib import Path
from typing import Any, Dict, Optional

from ...util.time import utc_now_iso
from . import pairing


def approve_outbound_from_remote_request(
    outbound_id: str,
    remote_request: Dict[str, Any],
    *,
    home: Optional[Path] = None,
) -> Dict[str, Any]:
    oid = str(outbound_id or "").strip()
    if not oid:
        raise ValueError("outbound_id is required")
    request = dict(remote_request or {})
    if str(request.get("status") or "") != "approved":
        raise ValueError("remote pairing request is not approved")

    store = pairing._load_store(home)  # type: ignore[attr-defined]
    outbound = store["outbounds"].get(oid)
    if not isinstance(outbound, dict):
        raise ValueError("pairing outbound not found")

    local_gid = str(outbound.get("local_group_id") or "").strip()
    issuer_gid = str(outbound.get("issuer_group_id") or request.get("group_id") or "").strip()
    issuer_group_title = str(outbound.get("issuer_group_title") or "").strip()
    issuer_endpoint = str(outbound.get("issuer_endpoint") or "").strip()
    issuer_pid = str(outbound.get("issuer_peer_id") or "").strip()
    if not local_gid or not issuer_gid or not issuer_pid:
        raise ValueError("pairing outbound is missing issuer identity")

    registration = pairing._upsert_approved_session_registration(  # type: ignore[attr-defined]
        local_gid,
        issuer_endpoint or pairing._session_only_registration_url(issuer_pid),  # type: ignore[attr-defined]
        remote_group_id=issuer_gid,
        remote_peer_id=issuer_pid,
        home=home,
    )
    now = utc_now_iso()
    raw_trust = _find_trust_for_remote(store, group_id=local_gid, remote_group_id=issuer_gid, remote_peer_id=issuer_pid)
    if raw_trust is not None:
        old_registration_id = str(raw_trust.get("registration_id") or "").strip()
        raw_trust.update({
            "request_id": str(request.get("request_id") or ""),
            "registration_id": registration["registration_id"],
            "group_id": local_gid,
            "remote_group_id": issuer_gid,
            "remote_group_title": issuer_group_title,
            "remote_endpoint": issuer_endpoint,
            "remote_peer_id": issuer_pid,
            "multiaddrs": registration.get("multiaddrs") or [],
            "transport": str(registration.get("transport") or ""),
            "access_level": str(raw_trust.get("access_level") or pairing.ACCESS_LEVEL_MESSAGES),
            "status": "active",
            "updated_at": now,
        })
        if old_registration_id and old_registration_id != registration["registration_id"]:
            pairing.delete_registration(old_registration_id, home=home)  # type: ignore[attr-defined]
        trust = pairing._project_trust(raw_trust)  # type: ignore[attr-defined]
    else:
        trust_id = pairing._new_id(pairing._TRUST_PREFIX, store["trusts"])  # type: ignore[attr-defined]
        raw_trust = {
            "trust_id": trust_id,
            "request_id": str(request.get("request_id") or ""),
            "registration_id": registration["registration_id"],
            "group_id": local_gid,
            "remote_group_id": issuer_gid,
            "remote_group_title": issuer_group_title,
            "remote_endpoint": issuer_endpoint,
            "remote_peer_id": issuer_pid,
            "multiaddrs": registration.get("multiaddrs") or [],
            "transport": str(registration.get("transport") or ""),
            "access_level": pairing.ACCESS_LEVEL_MESSAGES,
            "status": "active",
            "created_at": now,
            "updated_at": now,
        }
        store["trusts"][trust_id] = raw_trust
        trust = pairing._project_trust(raw_trust)  # type: ignore[attr-defined]

    outbound["status"] = "approved"
    outbound["remote_request"] = request
    outbound["last_error"] = ""
    outbound["updated_at"] = now
    pairing._save_store(store, home)  # type: ignore[attr-defined]
    pairing._publish_pairing_event(  # type: ignore[attr-defined]
        "federation.pairing.outbound_approved",
        {
            "group_id": local_gid,
            "outbound_id": oid,
            "trust_id": str((trust or {}).get("trust_id") or ""),
            "registration_id": registration["registration_id"],
        },
    )
    return {"outbound": pairing._project_outbound(outbound), "registration": registration, "trust": trust}  # type: ignore[attr-defined]


def _find_trust_for_remote(
    store: Dict[str, Dict[str, Dict[str, Any]]],
    *,
    group_id: str,
    remote_group_id: str,
    remote_peer_id: str,
) -> Optional[Dict[str, Any]]:
    for trust in store["trusts"].values():
        if str(trust.get("status") or "") != "active":
            continue
        if str(trust.get("group_id") or "").strip() != group_id:
            continue
        if str(trust.get("remote_group_id") or "").strip() != remote_group_id:
            continue
        if str(trust.get("remote_peer_id") or "").strip() != remote_peer_id:
            continue
        return trust
    return None
