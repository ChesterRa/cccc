"""Web routes for federation remote-send management (Stage 3).

Thin layer only: request parsing, principal/group authorization (re-checked
backend-side), delegation to the federation kernel store / Stage 2 daemon op,
and secret-free response projection. No business logic, no transport I/O.
"""

from __future__ import annotations

from typing import Any, Dict, List, Optional

from fastapi import APIRouter, Depends, HTTPException, Request
from pydantic import BaseModel, ConfigDict, Field

from ....daemon.federation.ops import try_handle_remote_send_op
from ....kernel.federation.receipts import safe_error_projection
from ....kernel.federation.pairing import (
    approve_pairing_request,
    create_pairing_invite,
    create_pairing_request,
    delete_pairing_outbound,
    get_pairing_invite_for_code,
    get_pairing_request_public_status,
    get_local_identity,
    get_pairing_outbound,
    list_pairing_outbounds,
    list_pairing_requests,
    list_trusts,
    reject_pairing_request,
    revoke_trust,
    upsert_pairing_outbound,
)
from ....kernel.federation.pairing_remote import sync_remote_pairing_outbound, submit_remote_pairing_request
from ....kernel.federation.registration import (
    delete_registration,
    get_registration,
    list_registrations,
    normalize_url,
    upsert_registration,
)
from ..schemas import RouteContext, check_group, get_principal, require_user

# Registration records may carry a credential reference. Public projections must
# never echo it back to UI/API callers.
_REGISTRATION_PUBLIC_FIELDS = (
    "registration_id",
    "group_id",
    "url",
    "transport",
    "remote_group_id",
    "remote_peer_id",
    "multiaddrs",
    "user_id",
    "status",
    "created_at",
    "updated_at",
    "last_sync_at",
    "last_error",
)


class FederationVerifyRequest(BaseModel):
    group_id: str
    url: str
    transport: str = "peer_cccc_http"
    remote_group_id: str = ""
    remote_peer_id: str = ""
    multiaddrs: List[str] = Field(default_factory=list)
    credential_ref: str = ""


class FederationRegisterRequest(BaseModel):
    group_id: str
    url: str
    transport: str = "peer_cccc_http"
    remote_group_id: str = ""
    remote_peer_id: str = ""
    multiaddrs: List[str] = Field(default_factory=list)
    credential_ref: str = ""


class FederationUnregisterRequest(BaseModel):
    registration_id: str


class PairingInviteCreateRequest(BaseModel):
    group_id: str
    remote_group_id: str = ""
    remote_peer_id: str = ""
    multiaddrs: List[str] = Field(default_factory=list)
    ttl_seconds: int = 600


class PairingRequestCreateRequest(BaseModel):
    pairing_code: str
    requester_group_id: str
    requester_peer_id: str
    requester_multiaddrs: List[str] = Field(default_factory=list)
    invite_id: str = ""


class RemotePairingRequestCreateRequest(BaseModel):
    model_config = ConfigDict(extra="forbid")

    pairing_code: str = Field(min_length=1, max_length=64)
    invite_id: str = Field(min_length=1, max_length=128)
    requester_group_id: str = Field(min_length=1, max_length=256)
    requester_group_title: str = Field(default="", max_length=256)
    requester_peer_id: str = Field(min_length=1, max_length=512)
    requester_node_id: str = Field(default="", max_length=512)
    requester_multiaddrs: List[str] = Field(default_factory=list, max_length=32)


class RemotePairingSubmitRequest(BaseModel):
    payload: Dict[str, Any]
    local_group_id: str
    local_group_title: str = ""


class PairingApproveRequest(BaseModel):
    approver_user_id: str = ""


class PairingRejectRequest(BaseModel):
    rejected_by: str = ""
    reason: str = ""


class TrustRevokeRequest(BaseModel):
    revoked_by: str = ""


def _project_registration(record: Dict[str, Any]) -> Dict[str, Any]:
    return {field: record.get(field) for field in _REGISTRATION_PUBLIC_FIELDS if field in record}


def _project_receipt(receipt: Optional[Dict[str, Any]]) -> Optional[Dict[str, Any]]:
    if not isinstance(receipt, dict):
        return None
    out = dict(receipt)
    if isinstance(out.get("error"), dict):
        out["error"] = safe_error_projection(out["error"])
    return out


def _require_group_or_403(request: Request, group_id: str) -> None:
    gid = str(group_id or "").strip()
    if not gid:
        raise HTTPException(status_code=400, detail={"code": "missing_group_id", "message": "group_id is required", "details": {}})
    # Re-check group authorization backend-side (admin-all vs explicit allow-list).
    check_group(request, gid)


def _principal_can_access(principal: Any, group_id: str) -> bool:
    if bool(getattr(principal, "is_admin", False)):
        return True
    allowed = getattr(principal, "allowed_groups", ()) or ()
    return str(group_id or "").strip() in {str(g or "").strip() for g in allowed}


def _filter_group_scoped_items(request: Request, items: List[Dict[str, Any]]) -> List[Dict[str, Any]]:
    principal = get_principal(request)
    return [item for item in items if _principal_can_access(principal, str(item.get("group_id") or ""))]


def _filter_local_group_scoped_items(request: Request, items: List[Dict[str, Any]]) -> List[Dict[str, Any]]:
    principal = get_principal(request)
    return [item for item in items if _principal_can_access(principal, str(item.get("local_group_id") or ""))]


def create_routers(ctx: RouteContext) -> list[APIRouter]:
    _ = ctx
    router = APIRouter(prefix="/api/federation", dependencies=[Depends(require_user)])

    @router.post("/verify")
    async def federation_verify(request: Request, req: FederationVerifyRequest) -> Dict[str, Any]:
        _require_group_or_403(request, req.group_id)
        norm = normalize_url(req.url)
        if not norm:
            raise HTTPException(status_code=400, detail={"code": "invalid_url", "message": "url is required", "details": {}})
        return {
            "ok": True,
            "result": {
                "verified": True,
                "group_id": str(req.group_id or "").strip(),
                "normalized_url": norm,
                "transport": str(req.transport or "peer_cccc_http").strip() or "peer_cccc_http",
            },
        }

    @router.post("/register")
    async def federation_register(request: Request, req: FederationRegisterRequest) -> Dict[str, Any]:
        _require_group_or_403(request, req.group_id)
        try:
            record = upsert_registration(
                req.group_id,
                req.url,
                transport=req.transport,
                remote_group_id=req.remote_group_id,
                remote_peer_id=req.remote_peer_id,
                multiaddrs=req.multiaddrs,
                credential_ref=req.credential_ref,
            )
        except ValueError as exc:
            raise HTTPException(status_code=400, detail={"code": "invalid_request", "message": str(exc), "details": {}}) from exc
        return {"ok": True, "result": {"registration": _project_registration(record)}}

    @router.get("/status")
    async def federation_status(request: Request, group_id: str = "") -> Dict[str, Any]:
        gid = str(group_id or "").strip()
        if gid:
            _require_group_or_403(request, gid)
            items = [r for r in list_registrations() if str(r.get("group_id") or "") == gid]
        else:
            principal = get_principal(request)
            items = [r for r in list_registrations() if _principal_can_access(principal, str(r.get("group_id") or ""))]
        return {"ok": True, "result": {"registrations": [_project_registration(r) for r in items]}}

    @router.post("/unregister")
    async def federation_unregister(request: Request, req: FederationUnregisterRequest) -> Dict[str, Any]:
        rid = str(req.registration_id or "").strip()
        record = get_registration(rid)
        if not record:
            raise HTTPException(status_code=404, detail={"code": "not_found", "message": "registration not found", "details": {}})
        _require_group_or_403(request, str(record.get("group_id") or ""))
        deleted = delete_registration(rid)
        return {"ok": True, "result": {"deleted": bool(deleted)}}

    @router.get("/registrations/{registration_id}/deliveries/{idempotency_key}")
    async def federation_delivery_status(request: Request, registration_id: str, idempotency_key: str) -> Dict[str, Any]:
        rid = str(registration_id or "").strip()
        record = get_registration(rid)
        if not record:
            raise HTTPException(status_code=404, detail={"code": "not_found", "message": "registration not found", "details": {}})
        # Web-layer principal authz...
        _require_group_or_403(request, str(record.get("group_id") or ""))
        # ...and reuse the Stage 2 daemon op, which re-enforces the registration
        # group scope before returning the receipt.
        resp = try_handle_remote_send_op(
            "remote_delivery_status",
            {
                "group_id": str(record.get("group_id") or ""),
                "registration_id": rid,
                "idempotency_key": str(idempotency_key or "").strip(),
            },
        )
        if resp is None:
            raise HTTPException(status_code=500, detail={"code": "internal_error", "message": "delivery status unavailable", "details": {}})
        if not resp.ok:
            err = resp.error
            raise HTTPException(
                status_code=400,
                detail={"code": getattr(err, "code", "error"), "message": getattr(err, "message", ""), "details": getattr(err, "details", {}) or {}},
            )
        receipt = (resp.result or {}).get("receipt") if isinstance(resp.result, dict) else None
        return {"ok": True, "result": {"receipt": _project_receipt(receipt)}}

    @router.get("/pairing/identity")
    async def federation_pairing_identity() -> Dict[str, Any]:
        return {"ok": True, "result": {"identity": get_local_identity()}}

    @router.post("/pairing/invites")
    async def federation_pairing_invite_create(request: Request, req: PairingInviteCreateRequest) -> Dict[str, Any]:
        _require_group_or_403(request, req.group_id)
        try:
            invite = create_pairing_invite(
                group_id=req.group_id,
                remote_group_id=req.remote_group_id,
                remote_peer_id=req.remote_peer_id,
                multiaddrs=req.multiaddrs,
                ttl_seconds=req.ttl_seconds,
            )
        except ValueError as exc:
            raise HTTPException(status_code=400, detail={"code": "invalid_request", "message": str(exc), "details": {}}) from exc
        return {"ok": True, "result": {"invite": invite}}

    @router.post("/pairing/requests")
    async def federation_pairing_request_create(request: Request, req: PairingRequestCreateRequest) -> Dict[str, Any]:
        _require_group_or_403(request, req.requester_group_id)
        invite = get_pairing_invite_for_code(req.pairing_code, invite_id=req.invite_id)
        if invite is not None:
            _require_group_or_403(request, str(invite.get("group_id") or ""))
        try:
            pairing_request = create_pairing_request(
                req.pairing_code,
                requester_group_id=req.requester_group_id,
                requester_group_title="",
                requester_peer_id=req.requester_peer_id,
                requester_multiaddrs=req.requester_multiaddrs,
                invite_id=req.invite_id,
            )
        except ValueError as exc:
            raise HTTPException(status_code=400, detail={"code": "invalid_request", "message": str(exc), "details": {}}) from exc
        return {"ok": True, "result": {"request": pairing_request}}

    @router.post("/pairing/remote-requests")
    async def federation_pairing_remote_submit(request: Request, req: RemotePairingSubmitRequest) -> Dict[str, Any]:
        _require_group_or_403(request, req.local_group_id)
        try:
            outbound = submit_remote_pairing_request(
                req.payload,
                local_group_id=req.local_group_id,
                local_group_title=req.local_group_title,
                allow_localhost=True,
            )
        except ValueError as exc:
            raise HTTPException(status_code=400, detail={"code": "invalid_request", "message": str(exc), "details": {}}) from exc
        outbound = {**outbound, "local_group_id": str(outbound.get("local_group_id") or req.local_group_id)}
        outbound = upsert_pairing_outbound(outbound)
        return {"ok": True, "result": {"outbound": outbound}}

    @router.get("/pairing/requests")
    async def federation_pairing_request_list(request: Request, group_id: str = "") -> Dict[str, Any]:
        gid = str(group_id or "").strip()
        if gid:
            _require_group_or_403(request, gid)
            items = list_pairing_requests(group_id=gid)
        else:
            items = _filter_group_scoped_items(request, list_pairing_requests())
        return {"ok": True, "result": {"requests": items}}

    @router.post("/pairing/requests/{request_id}/approve")
    async def federation_pairing_approve(request: Request, request_id: str, req: PairingApproveRequest) -> Dict[str, Any]:
        existing = list_pairing_requests()
        match = next((item for item in existing if str(item.get("request_id") or "") == str(request_id or "").strip()), None)
        if not match:
            raise HTTPException(status_code=404, detail={"code": "not_found", "message": "pairing request not found", "details": {}})
        _require_group_or_403(request, str(match.get("group_id") or ""))
        try:
            approved = approve_pairing_request(request_id, approver_user_id=req.approver_user_id)
        except ValueError as exc:
            raise HTTPException(status_code=400, detail={"code": "invalid_request", "message": str(exc), "details": {}}) from exc
        return {
            "ok": True,
            "result": {
                "request": approved.get("request"),
                "registration": _project_registration(approved.get("registration") or {}),
                "trust": approved.get("trust"),
            },
        }

    @router.post("/pairing/requests/{request_id}/reject")
    async def federation_pairing_reject(request: Request, request_id: str, req: PairingRejectRequest) -> Dict[str, Any]:
        existing = list_pairing_requests()
        match = next((item for item in existing if str(item.get("request_id") or "") == str(request_id or "").strip()), None)
        if not match:
            raise HTTPException(status_code=404, detail={"code": "not_found", "message": "pairing request not found", "details": {}})
        _require_group_or_403(request, str(match.get("group_id") or ""))
        try:
            rejected = reject_pairing_request(request_id, rejected_by=req.rejected_by, reason=req.reason)
        except ValueError as exc:
            raise HTTPException(status_code=400, detail={"code": "invalid_request", "message": str(exc), "details": {}}) from exc
        return {"ok": True, "result": {"request": rejected}}

    @router.get("/pairing/trusts")
    async def federation_pairing_trust_list(request: Request, group_id: str = "") -> Dict[str, Any]:
        gid = str(group_id or "").strip()
        if gid:
            _require_group_or_403(request, gid)
            items = list_trusts(group_id=gid)
        else:
            items = _filter_group_scoped_items(request, list_trusts())
        return {"ok": True, "result": {"trusts": items}}

    @router.post("/pairing/trusts/{trust_id}/revoke")
    async def federation_pairing_trust_revoke(request: Request, trust_id: str, req: TrustRevokeRequest) -> Dict[str, Any]:
        existing = list_trusts()
        match = next((item for item in existing if str(item.get("trust_id") or "") == str(trust_id or "").strip()), None)
        if not match:
            raise HTTPException(status_code=404, detail={"code": "not_found", "message": "trust not found", "details": {}})
        _require_group_or_403(request, str(match.get("group_id") or ""))
        try:
            revoked = revoke_trust(trust_id, revoked_by=req.revoked_by)
        except ValueError as exc:
            raise HTTPException(status_code=400, detail={"code": "invalid_request", "message": str(exc), "details": {}}) from exc
        return {"ok": True, "result": {"trust": revoked}}

    @router.get("/pairing/outbounds")
    async def federation_pairing_outbound_list(request: Request, group_id: str = "") -> Dict[str, Any]:
        gid = str(group_id or "").strip()
        if gid:
            _require_group_or_403(request, gid)
            items = list_pairing_outbounds(group_id=gid)
        else:
            items = _filter_local_group_scoped_items(request, list_pairing_outbounds())
        return {"ok": True, "result": {"outbounds": items}}

    @router.post("/pairing/outbounds/{outbound_id}/sync")
    async def federation_pairing_outbound_sync(request: Request, outbound_id: str) -> Dict[str, Any]:
        existing = get_pairing_outbound(outbound_id)
        if not existing:
            raise HTTPException(status_code=404, detail={"code": "not_found", "message": "pairing outbound not found", "details": {}})
        _require_group_or_403(request, str(existing.get("local_group_id") or ""))
        try:
            outbound = sync_remote_pairing_outbound(outbound_id, allow_localhost=True)
        except ValueError as exc:
            raise HTTPException(status_code=400, detail={"code": "invalid_request", "message": str(exc), "details": {}}) from exc
        if isinstance(outbound, dict) and isinstance(outbound.get("outbound"), dict):
            outbound = outbound["outbound"]
        return {"ok": True, "result": {"outbound": outbound}}

    @router.post("/pairing/outbounds/{outbound_id}/delete")
    async def federation_pairing_outbound_delete(request: Request, outbound_id: str) -> Dict[str, Any]:
        existing = get_pairing_outbound(outbound_id)
        if not existing:
            raise HTTPException(status_code=404, detail={"code": "not_found", "message": "pairing outbound not found", "details": {}})
        _require_group_or_403(request, str(existing.get("local_group_id") or ""))
        return {"ok": True, "result": {"deleted": delete_pairing_outbound(outbound_id)}}

    public_router = APIRouter(prefix="/api/federation")

    @public_router.post("/pairing/requests/remote")
    async def federation_pairing_remote_request_create(req: RemotePairingRequestCreateRequest) -> Dict[str, Any]:
        try:
            pairing_request = create_pairing_request(
                req.pairing_code,
                requester_group_id=req.requester_group_id,
                requester_group_title=req.requester_group_title,
                requester_peer_id=req.requester_peer_id,
                requester_multiaddrs=req.requester_multiaddrs,
                invite_id=req.invite_id,
            )
        except ValueError as exc:
            raise HTTPException(status_code=400, detail={"code": "invalid_request", "message": str(exc), "details": {}}) from exc
        return {"ok": True, "result": {"request": pairing_request}}

    @public_router.get("/pairing/requests/remote/status")
    async def federation_pairing_remote_request_status(request_id: str = "", invite_id: str = "") -> Dict[str, Any]:
        pairing_request = get_pairing_request_public_status(request_id, invite_id=invite_id)
        if not pairing_request:
            raise HTTPException(status_code=404, detail={"code": "not_found", "message": "pairing request not found", "details": {}})
        return {"ok": True, "result": {"request": pairing_request}}

    return [public_router, router]
