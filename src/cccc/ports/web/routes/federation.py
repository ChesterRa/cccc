"""Web routes for federation remote-send management (Stage 3).

Thin layer only: request parsing, principal/group authorization (re-checked
backend-side), delegation to the federation kernel store / Stage 2 daemon op,
and secret-free response projection. No business logic, no transport I/O.
"""

from __future__ import annotations

from typing import Any, Dict, List, Optional

from fastapi import APIRouter, Depends, HTTPException, Request
from pydantic import BaseModel, Field

from ....daemon.federation.ops import try_handle_remote_send_op
from ....kernel.federation.receipts import safe_error_projection
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
    credential_ref: str = ""


class FederationRegisterRequest(BaseModel):
    group_id: str
    url: str
    transport: str = "peer_cccc_http"
    remote_group_id: str = ""
    credential_ref: str = ""


class FederationUnregisterRequest(BaseModel):
    registration_id: str


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

    return [router]
