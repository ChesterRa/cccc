"""libp2p federation pairing/approval store.

This module owns only local state transitions for pairing:

- stable local node/peer identity
- short-lived invite code (plaintext returned once, hash persisted)
- pending request
- approve/reject request
- approved trust records backed by the existing registration store

It deliberately does not import or run a libp2p client.
"""

from __future__ import annotations

import hashlib
import secrets
import string
from datetime import timedelta
from pathlib import Path
from typing import Any, Dict, List, Optional

import yaml

from ...paths import ensure_home
from ...kernel.access_tokens import create_access_token
from ...kernel.events import publish_event
from ...util.fs import atomic_write_text
from ...util.time import parse_utc_iso, utc_now_iso
from .registration import upsert_registration
from .registration import delete_registration
from .peer_addresses import record_peer_addresses

_INVITE_PREFIX = "pinv_"
_REQUEST_PREFIX = "preq_"
_TRUST_PREFIX = "ptrust_"
_OUTBOUND_PREFIX = "pout_"
_CODE_ALPHABET = string.ascii_uppercase + string.digits
_DEFAULT_TTL_SECONDS = 600


def _identity_path(home: Optional[Path] = None) -> Path:
    base = Path(home) if home is not None else ensure_home()
    return base / "federation_identity.yaml"


def _pairing_path(home: Optional[Path] = None) -> Path:
    base = Path(home) if home is not None else ensure_home()
    return base / "federation_pairing.yaml"


def _load_yaml(path: Path) -> Dict[str, Any]:
    if not path.exists():
        return {}
    try:
        raw = yaml.safe_load(path.read_text(encoding="utf-8")) or {}
    except Exception:
        return {}
    return dict(raw) if isinstance(raw, dict) else {}


def _save_yaml(path: Path, payload: Dict[str, Any]) -> None:
    atomic_write_text(path, yaml.safe_dump(payload, allow_unicode=True, sort_keys=True, default_flow_style=False))


def _load_store(home: Optional[Path] = None) -> Dict[str, Dict[str, Dict[str, Any]]]:
    raw = _load_yaml(_pairing_path(home))
    out: Dict[str, Dict[str, Dict[str, Any]]] = {"invites": {}, "requests": {}, "trusts": {}, "outbounds": {}}
    for key in out:
        section = raw.get(key)
        if isinstance(section, dict):
            out[key] = {str(k): dict(v) for k, v in section.items() if isinstance(v, dict)}
    return out


def _save_store(store: Dict[str, Dict[str, Dict[str, Any]]], home: Optional[Path] = None) -> None:
    _save_yaml(_pairing_path(home), {key: dict(store.get(key) or {}) for key in ("invites", "requests", "trusts", "outbounds")})


def _new_id(prefix: str, existing: Dict[str, Dict[str, Any]]) -> str:
    while True:
        candidate = f"{prefix}{secrets.token_hex(8)}"
        if candidate not in existing:
            return candidate


def _new_code() -> str:
    return "{}{}{}{}-{}{}{}{}".format(*(secrets.choice(_CODE_ALPHABET) for _ in range(8)))


def _hash_code(code: str) -> str:
    return hashlib.sha256(str(code or "").strip().upper().encode("utf-8")).hexdigest()


def _parse_seconds(value: int) -> int:
    try:
        return int(value)
    except Exception:
        return _DEFAULT_TTL_SECONDS


def _expires_at(ttl_seconds: int) -> str:
    from datetime import datetime, timezone

    return (datetime.now(timezone.utc) + timedelta(seconds=ttl_seconds)).isoformat().replace("+00:00", "Z")


def _is_expired(invite: Dict[str, Any]) -> bool:
    expires = parse_utc_iso(str(invite.get("expires_at") or ""))
    if expires is None:
        return False
    from datetime import datetime, timezone

    return expires <= datetime.now(timezone.utc)


def _clean_addrs(addrs: Optional[List[str]]) -> List[str]:
    return [str(addr or "").strip() for addr in (addrs or []) if str(addr or "").strip()]


def _publish_pairing_event(kind: str, data: Dict[str, Any]) -> None:
    publish_event(kind, data)


def get_local_identity(*, home: Optional[Path] = None) -> Dict[str, Any]:
    """Return stable local public identity. No secret fields are persisted."""
    try:
        from ...daemon.federation.libp2p.identity import get_libp2p_identity

        libp2p_identity = get_libp2p_identity(home=home)
        node_id = f"node_{hashlib.sha256(libp2p_identity.peer_id.encode('utf-8')).hexdigest()[:24]}"
        identity = {"node_id": node_id, "peer_id": libp2p_identity.peer_id}
        _save_yaml(_identity_path(home), identity)
        return dict(identity)
    except Exception:
        pass
    path = _identity_path(home)
    raw = _load_yaml(path)
    node_id = str(raw.get("node_id") or "").strip()
    peer_id = str(raw.get("peer_id") or "").strip()
    if not node_id:
        node_id = f"node_{secrets.token_hex(12)}"
    if not peer_id:
        peer_id = f"peer_{secrets.token_hex(16)}"
    identity = {"node_id": node_id, "peer_id": peer_id}
    if raw.get("node_id") != node_id or raw.get("peer_id") != peer_id:
        _save_yaml(path, identity)
    return dict(identity)


def create_pairing_invite(
    *,
    group_id: str,
    remote_group_id: str = "",
    remote_peer_id: str = "",
    multiaddrs: Optional[List[str]] = None,
    ttl_seconds: int = _DEFAULT_TTL_SECONDS,
    home: Optional[Path] = None,
) -> Dict[str, Any]:
    gid = str(group_id or "").strip()
    remote_gid = str(remote_group_id or "").strip()
    remote_pid = str(remote_peer_id or "").strip()
    if not gid:
        raise ValueError("group_id is required")

    store = _load_store(home)
    code = _new_code()
    while any(str(inv.get("pairing_code_hash") or "") == _hash_code(code) for inv in store["invites"].values()):
        code = _new_code()
    now = utc_now_iso()
    invite_id = _new_id(_INVITE_PREFIX, store["invites"])
    invite = {
        "invite_id": invite_id,
        "group_id": gid,
        "remote_group_id": remote_gid,
        "remote_peer_id": remote_pid,
        "multiaddrs": _clean_addrs(multiaddrs),
        "transport": "libp2p_cccc",
        "pairing_code_hash": _hash_code(code),
        "status": "pending",
        "created_at": now,
        "updated_at": now,
        "expires_at": _expires_at(_parse_seconds(ttl_seconds)),
        "request_id": "",
    }
    store["invites"][invite_id] = invite
    _save_store(store, home)
    out = _project_invite(invite)
    out["pairing_code"] = code
    _publish_pairing_event("federation.pairing.invite_created", {
        "group_id": gid,
        "invite_id": invite_id,
    })
    return out


def _find_invite_by_code(store: Dict[str, Dict[str, Dict[str, Any]]], pairing_code: str) -> Optional[Dict[str, Any]]:
    code_hash = _hash_code(pairing_code)
    for invite in store["invites"].values():
        if str(invite.get("pairing_code_hash") or "") == code_hash:
            return invite
    return None


def get_pairing_invite_for_code(pairing_code: str, *, invite_id: str = "", home: Optional[Path] = None) -> Optional[Dict[str, Any]]:
    code = str(pairing_code or "").strip().upper()
    if not code:
        return None
    invite = _find_invite_by_code(_load_store(home), code)
    if invite is None:
        return None
    expected_invite_id = str(invite_id or "").strip()
    if expected_invite_id and str(invite.get("invite_id") or "") != expected_invite_id:
        return None
    return _project_invite(invite)


def create_pairing_request(
    pairing_code: str,
    *,
    requester_group_id: str,
    requester_group_title: str = "",
    requester_peer_id: str,
    requester_multiaddrs: Optional[List[str]] = None,
    invite_id: str = "",
    home: Optional[Path] = None,
) -> Dict[str, Any]:
    code = str(pairing_code or "").strip().upper()
    if not code:
        raise ValueError("pairing_code is required")
    requester_gid = str(requester_group_id or "").strip()
    requester_title = str(requester_group_title or "").strip()
    requester_pid = str(requester_peer_id or "").strip()
    if not requester_gid:
        raise ValueError("requester_group_id is required")
    if not requester_pid:
        raise ValueError("requester_peer_id is required")

    store = _load_store(home)
    invite = _find_invite_by_code(store, code)
    if invite is None:
        raise ValueError("pairing code not found")
    expected_invite_id = str(invite_id or "").strip()
    if expected_invite_id and str(invite.get("invite_id") or "") != expected_invite_id:
        raise ValueError("pairing code not found")
    if str(invite.get("status") or "") != "pending":
        raise ValueError("pairing code already used")
    if _is_expired(invite):
        invite["status"] = "expired"
        invite["updated_at"] = utc_now_iso()
        _save_store(store, home)
        raise ValueError("pairing code expired")

    now = utc_now_iso()
    request_id = _new_id(_REQUEST_PREFIX, store["requests"])
    request = {
        "request_id": request_id,
        "invite_id": invite["invite_id"],
        "group_id": str(invite.get("group_id") or ""),
        "remote_group_id": requester_gid,
        "remote_group_title": requester_title,
        "remote_peer_id": requester_pid,
        "multiaddrs": _clean_addrs(requester_multiaddrs) or _clean_addrs(invite.get("multiaddrs")),  # type: ignore[arg-type]
        "status": "pending",
        "created_at": now,
        "updated_at": now,
        "approved_by": "",
        "rejected_by": "",
        "rejection_reason": "",
        "registration_id": "",
    }
    invite["status"] = "requested"
    invite["request_id"] = request_id
    invite["updated_at"] = now
    store["requests"][request_id] = request
    _save_store(store, home)
    _publish_pairing_event("federation.pairing.request_created", {
        "group_id": str(request.get("group_id") or ""),
        "request_id": request_id,
        "remote_group_id": requester_gid,
        "remote_peer_id": requester_pid,
    })
    return _project_request(request)


def get_pairing_request(request_id: str, *, home: Optional[Path] = None) -> Optional[Dict[str, Any]]:
    rid = str(request_id or "").strip()
    if not rid:
        return None
    req = _load_store(home)["requests"].get(rid)
    return _project_request(req) if isinstance(req, dict) else None


def get_pairing_request_public_status(
    request_id: str,
    *,
    invite_id: str,
    home: Optional[Path] = None,
) -> Optional[Dict[str, Any]]:
    """Return a request projection for the remote submitter status check.

    The request id is not treated as sufficient proof on its own. The invite id
    must match the original one-time connection info nonce, which keeps the
    unauthenticated status route bounded to callers that already submitted the
    invite payload.
    """
    rid = str(request_id or "").strip()
    iid = str(invite_id or "").strip()
    if not rid or not iid:
        return None
    request = _load_store(home)["requests"].get(rid)
    if not isinstance(request, dict) or str(request.get("invite_id") or "") != iid:
        return None
    return _project_remote_status_request(request, home=home)


def list_pairing_requests(*, group_id: str = "", home: Optional[Path] = None) -> List[Dict[str, Any]]:
    gid = str(group_id or "").strip()
    requests = list(_load_store(home)["requests"].values())
    if gid:
        requests = [r for r in requests if str(r.get("group_id") or "") == gid]
    requests.sort(key=lambda r: (str(r.get("created_at") or ""), str(r.get("request_id") or "")))
    return [_project_request(r) for r in requests]


def approve_pairing_request(
    request_id: str,
    *,
    approver_user_id: str = "",
    home: Optional[Path] = None,
) -> Dict[str, Any]:
    rid = str(request_id or "").strip()
    store = _load_store(home)
    request = store["requests"].get(rid)
    if not isinstance(request, dict):
        raise ValueError("pairing request not found")
    status = str(request.get("status") or "")
    if status == "approved" and request.get("registration_id"):
        reg = _registration_from_request(request, home=home)
        _record_request_peer_addresses(request, home=home)
        return {
            "status": "approved",
            "request": _project_request(request),
            "registration": reg,
            "trust": _trust_for_registration(str(request.get("registration_id") or ""), store),
        }
    if status != "pending":
        raise ValueError(f"pairing request is not pending (status={status})")

    registration = _registration_from_request(request, home=home)
    _record_request_peer_addresses(request, home=home)
    now = utc_now_iso()
    request["status"] = "approved"
    request["approved_by"] = str(approver_user_id or "").strip()
    request["registration_id"] = registration["registration_id"]
    if not str(request.get("remote_send_token") or "").strip():
        request["remote_send_token"] = _create_pairing_remote_send_token(request, home=home)
    request["updated_at"] = now
    trust_id = _new_id(_TRUST_PREFIX, store["trusts"])
    trust = {
        "trust_id": trust_id,
        "request_id": rid,
        "registration_id": registration["registration_id"],
        "group_id": registration["group_id"],
        "remote_group_id": registration["remote_group_id"],
        "remote_group_title": str(request.get("remote_group_title") or ""),
        "remote_peer_id": registration["remote_peer_id"],
        "multiaddrs": registration.get("multiaddrs") or [],
        "transport": "libp2p_cccc",
        "status": "active",
        "created_at": now,
        "updated_at": now,
    }
    store["trusts"][trust_id] = trust
    _save_store(store, home)
    _publish_pairing_event("federation.pairing.request_approved", {
        "group_id": str(request.get("group_id") or ""),
        "request_id": rid,
        "trust_id": trust_id,
        "registration_id": registration["registration_id"],
    })
    return {"status": "approved", "request": _project_request(request), "registration": registration, "trust": _project_trust(trust)}


def reject_pairing_request(
    request_id: str,
    *,
    rejected_by: str = "",
    reason: str = "",
    home: Optional[Path] = None,
) -> Dict[str, Any]:
    rid = str(request_id or "").strip()
    store = _load_store(home)
    request = store["requests"].get(rid)
    if not isinstance(request, dict):
        raise ValueError("pairing request not found")
    status = str(request.get("status") or "")
    if status == "approved":
        raise ValueError("approved pairing request cannot be rejected")
    if status == "rejected":
        return _project_request(request)
    if status != "pending":
        raise ValueError(f"pairing request is not pending (status={status})")
    request["status"] = "rejected"
    request["rejected_by"] = str(rejected_by or "").strip()
    request["rejection_reason"] = str(reason or "").strip()
    request["updated_at"] = utc_now_iso()
    _save_store(store, home)
    _publish_pairing_event("federation.pairing.request_rejected", {
        "group_id": str(request.get("group_id") or ""),
        "request_id": rid,
    })
    return _project_request(request)


def list_trusts(*, group_id: str = "", home: Optional[Path] = None) -> List[Dict[str, Any]]:
    gid = str(group_id or "").strip()
    store = _load_store(home)
    trusts = [_enrich_trust_display_fields(t, store) for t in store["trusts"].values()]
    if gid:
        trusts = [t for t in trusts if str(t.get("group_id") or "") == gid]
    trusts.sort(key=lambda t: (str(t.get("created_at") or ""), str(t.get("trust_id") or "")))
    return [_project_trust(t) for t in trusts]


def update_trust_multiaddrs_for_peer(
    *,
    group_id: str,
    remote_group_id: str,
    remote_peer_id: str,
    multiaddrs: Optional[List[str]] = None,
    home: Optional[Path] = None,
) -> int:
    gid = str(group_id or "").strip()
    remote_gid = str(remote_group_id or "").strip()
    remote_pid = str(remote_peer_id or "").strip()
    addrs = _clean_addrs(multiaddrs)
    if not gid or not remote_gid or not remote_pid or not addrs:
        return 0
    store = _load_store(home)
    updated = 0
    now = utc_now_iso()
    for trust in store["trusts"].values():
        if str(trust.get("status") or "") != "active":
            continue
        if str(trust.get("group_id") or "").strip() != gid:
            continue
        if str(trust.get("remote_group_id") or "").strip() != remote_gid:
            continue
        if str(trust.get("remote_peer_id") or "").strip() != remote_pid:
            continue
        if _clean_addrs(trust.get("multiaddrs")) == addrs:  # type: ignore[arg-type]
            continue
        trust["multiaddrs"] = list(addrs)
        trust["updated_at"] = now
        updated += 1
    if updated:
        _save_store(store, home)
    return updated


def _enrich_trust_display_fields(
    trust: Dict[str, Any],
    store: Dict[str, Dict[str, Dict[str, Any]]],
) -> Dict[str, Any]:
    if trust.get("remote_endpoint") and trust.get("remote_group_title"):
        return trust
    local_gid = str(trust.get("group_id") or "").strip()
    remote_gid = str(trust.get("remote_group_id") or "").strip()
    remote_pid = str(trust.get("remote_peer_id") or "").strip()
    if not local_gid or not remote_gid:
        return trust
    for outbound in store.get("outbounds", {}).values():
        if str(outbound.get("status") or "").strip() != "approved":
            continue
        if str(outbound.get("local_group_id") or "").strip() != local_gid:
            continue
        if str(outbound.get("issuer_group_id") or "").strip() != remote_gid:
            continue
        if remote_pid and str(outbound.get("issuer_peer_id") or "").strip() != remote_pid:
            continue
        return {
            **trust,
            "remote_endpoint": str(trust.get("remote_endpoint") or outbound.get("issuer_endpoint") or "").strip(),
            "remote_group_title": str(trust.get("remote_group_title") or outbound.get("issuer_group_title") or "").strip(),
        }
    return trust


def revoke_trust(
    trust_id: str,
    *,
    revoked_by: str = "",
    home: Optional[Path] = None,
) -> Dict[str, Any]:
    tid = str(trust_id or "").strip()
    if not tid:
        raise ValueError("trust_id is required")
    store = _load_store(home)
    trust = store["trusts"].get(tid)
    if not isinstance(trust, dict):
        raise ValueError("trust not found")
    if str(trust.get("status") or "") == "revoked":
        return _project_trust(trust)
    trust["status"] = "revoked"
    trust["revoked_by"] = str(revoked_by or "").strip()
    trust["updated_at"] = utc_now_iso()
    registration_id = str(trust.get("registration_id") or "").strip()
    if registration_id:
        delete_registration(registration_id, home=home)
    _save_store(store, home)
    _publish_pairing_event("federation.pairing.trust_revoked", {
        "group_id": str(trust.get("group_id") or ""),
        "trust_id": tid,
        "registration_id": registration_id,
    })
    return _project_trust(trust)


def upsert_pairing_outbound(outbound: Dict[str, Any], *, home: Optional[Path] = None) -> Dict[str, Any]:
    local_gid = str(outbound.get("local_group_id") or outbound.get("group_id") or "").strip()
    if not local_gid:
        raise ValueError("local_group_id is required")
    store = _load_store(home)
    outbound_id = str(outbound.get("outbound_id") or "").strip() or _new_id(_OUTBOUND_PREFIX, store["outbounds"])
    now = utc_now_iso()
    existing = store["outbounds"].get(outbound_id) or {}
    record = {
        **existing,
        "outbound_id": outbound_id,
        "local_group_id": local_gid,
        "issuer_endpoint": str(outbound.get("issuer_endpoint") or "").strip(),
        "issuer_group_id": str(outbound.get("issuer_group_id") or "").strip(),
        "issuer_group_title": str(outbound.get("issuer_group_title") or "").strip(),
        "issuer_peer_id": str(outbound.get("issuer_peer_id") or "").strip(),
        "invite_id": str(outbound.get("invite_id") or "").strip(),
        "status": str(outbound.get("status") or "submitted").strip(),
        "remote_request": dict(outbound.get("remote_request") or {}) if isinstance(outbound.get("remote_request"), dict) else {},
        "last_error": str(outbound.get("last_error") or "").strip(),
        "created_at": str(existing.get("created_at") or outbound.get("created_at") or now),
        "updated_at": str(outbound.get("updated_at") or now),
    }
    store["outbounds"][outbound_id] = record
    _save_store(store, home)
    _publish_pairing_event("federation.pairing.outbound_changed", {
        "group_id": local_gid,
        "outbound_id": outbound_id,
        "status": record["status"],
    })
    return _project_outbound(record)


def list_pairing_outbounds(*, group_id: str = "", home: Optional[Path] = None) -> List[Dict[str, Any]]:
    gid = str(group_id or "").strip()
    outbounds = list(_load_store(home)["outbounds"].values())
    if gid:
        outbounds = [o for o in outbounds if str(o.get("local_group_id") or "") == gid]
    outbounds.sort(key=lambda o: (str(o.get("created_at") or ""), str(o.get("outbound_id") or "")))
    return [_project_outbound(o) for o in outbounds]


def get_pairing_outbound(outbound_id: str, *, home: Optional[Path] = None) -> Optional[Dict[str, Any]]:
    oid = str(outbound_id or "").strip()
    if not oid:
        return None
    outbound = _load_store(home)["outbounds"].get(oid)
    return _project_outbound(outbound) if isinstance(outbound, dict) else None


def delete_pairing_outbound(outbound_id: str, *, home: Optional[Path] = None) -> bool:
    oid = str(outbound_id or "").strip()
    if not oid:
        return False
    store = _load_store(home)
    outbound = store["outbounds"].pop(oid, None)
    if not isinstance(outbound, dict):
        return False
    _save_store(store, home)
    _publish_pairing_event("federation.pairing.outbound_deleted", {
        "group_id": str(outbound.get("local_group_id") or ""),
        "outbound_id": oid,
    })
    return True


def _registration_from_request(request: Dict[str, Any], *, home: Optional[Path]) -> Dict[str, Any]:
    remote_peer_id = str(request.get("remote_peer_id") or "").strip()
    return _upsert_approved_libp2p_registration(
        str(request.get("group_id") or ""),
        f"libp2p://{remote_peer_id}",
        remote_group_id=str(request.get("remote_group_id") or ""),
        remote_peer_id=remote_peer_id,
        multiaddrs=_clean_addrs(request.get("multiaddrs")),  # type: ignore[arg-type]
        home=home,
    )


def _record_request_peer_addresses(request: Dict[str, Any], *, home: Optional[Path]) -> None:
    remote_peer_id = str(request.get("remote_peer_id") or "").strip()
    if not remote_peer_id:
        return
    addrs = _clean_addrs(request.get("multiaddrs"))  # type: ignore[arg-type]
    if not addrs:
        return
    record_peer_addresses(
        remote_peer_id,
        addrs,
        remote_group_id=str(request.get("remote_group_id") or "").strip(),
        home=home,
    )


def _upsert_approved_libp2p_registration(
    group_id: str,
    url: str,
    *,
    remote_group_id: str,
    remote_peer_id: str,
    multiaddrs: Optional[List[str]] = None,
    home: Optional[Path] = None,
) -> Dict[str, Any]:
    return upsert_registration(
        group_id,
        url,
        transport="libp2p_cccc",
        remote_group_id=remote_group_id,
        remote_peer_id=remote_peer_id,
        multiaddrs=_clean_addrs(multiaddrs),
        status="active",
        home=home,
        _approved_by_pairing=True,
    )


def _upsert_approved_http_registration(
    group_id: str,
    issuer_endpoint: str,
    *,
    remote_group_id: str,
    remote_peer_id: str,
    credential_ref: str = "",
    home: Optional[Path] = None,
) -> Dict[str, Any]:
    return upsert_registration(
        group_id,
        issuer_endpoint,
        transport="peer_cccc_http",
        remote_group_id=remote_group_id,
        remote_peer_id=remote_peer_id,
        credential_ref=credential_ref,
        status="active",
        home=home,
    )


def _trust_for_registration(registration_id: str, store: Dict[str, Dict[str, Dict[str, Any]]]) -> Optional[Dict[str, Any]]:
    for trust in store["trusts"].values():
        if str(trust.get("registration_id") or "") == registration_id:
            return _project_trust(trust)
    return None


def _project_invite(invite: Dict[str, Any]) -> Dict[str, Any]:
    fields = (
        "invite_id",
        "group_id",
        "remote_group_id",
        "remote_group_title",
        "remote_endpoint",
        "remote_peer_id",
        "multiaddrs",
        "transport",
        "status",
        "created_at",
        "updated_at",
        "expires_at",
        "request_id",
    )
    return {field: invite.get(field) for field in fields if field in invite}


def _project_request(request: Dict[str, Any]) -> Dict[str, Any]:
    fields = (
        "request_id",
        "invite_id",
        "group_id",
        "remote_group_id",
        "remote_group_title",
        "remote_peer_id",
        "multiaddrs",
        "status",
        "created_at",
        "updated_at",
        "approved_by",
        "rejected_by",
        "rejection_reason",
        "registration_id",
    )
    return {field: request.get(field) for field in fields if field in request}


def _project_remote_status_request(request: Dict[str, Any], *, home: Optional[Path]) -> Dict[str, Any]:
    out = _project_request(request)
    if str(request.get("status") or "") == "approved":
        token = str(request.get("remote_send_token") or "").strip()
        if not token:
            token = _create_pairing_remote_send_token(request, home=home)
            store = _load_store(home)
            stored = store["requests"].get(str(request.get("request_id") or ""))
            if isinstance(stored, dict):
                stored["remote_send_token"] = token
                stored["updated_at"] = utc_now_iso()
                _save_store(store, home)
        out["remote_send_token"] = token
    return out


def _create_pairing_remote_send_token(request: Dict[str, Any], *, home: Optional[Path]) -> str:
    group_id = str(request.get("group_id") or "").strip()
    request_id = str(request.get("request_id") or "").strip()
    if not group_id or not request_id:
        return ""
    return str(
        create_access_token(
            f"federation:{request_id}",
            allowed_groups=[group_id],
            is_admin=False,
            home=home,
        ).get("token")
        or ""
    )


def _project_trust(trust: Dict[str, Any]) -> Dict[str, Any]:
    fields = (
        "trust_id",
        "request_id",
        "registration_id",
        "group_id",
        "remote_group_id",
        "remote_group_title",
        "remote_endpoint",
        "remote_peer_id",
        "multiaddrs",
        "transport",
        "status",
        "created_at",
        "updated_at",
    )
    return {field: trust.get(field) for field in fields if field in trust}


def _project_outbound(outbound: Dict[str, Any]) -> Dict[str, Any]:
    fields = (
        "outbound_id",
        "local_group_id",
        "issuer_endpoint",
        "issuer_group_id",
        "issuer_group_title",
        "issuer_peer_id",
        "invite_id",
        "status",
        "remote_request",
        "last_error",
        "created_at",
        "updated_at",
    )
    return {field: outbound.get(field) for field in fields if field in outbound}
