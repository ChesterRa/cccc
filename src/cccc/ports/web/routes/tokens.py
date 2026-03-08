from __future__ import annotations

import hashlib
from typing import Any, Dict, List, Optional

from fastapi import APIRouter, Depends, HTTPException
from pydantic import BaseModel, Field

from ....kernel.web_tokens import create_token, delete_token, list_tokens, update_token
from ..schemas import RouteContext, require_admin, _configured_web_token


class TokenCreateRequest(BaseModel):
    user_id: str
    allowed_groups: List[str] = Field(default_factory=list)
    is_admin: bool = False
    custom_token: Optional[str] = None


class TokenUpdateRequest(BaseModel):
    allowed_groups: Optional[List[str]] = None
    is_admin: Optional[bool] = None


def _token_id(token: str) -> str:
    return hashlib.sha256(token.encode("utf-8")).hexdigest()[:16]


def _resolve_raw_token(token_id: str) -> str:
    """Resolve a token_id (sha256 prefix) back to the raw token string."""
    target = str(token_id or "").strip()
    if not target:
        return ""
    if len(target) != 16:
        return target
    for item in list_tokens():
        raw = str((item or {}).get("token") or "").strip()
        if raw and _token_id(raw) == target:
            return raw
    return target


def create_routers(ctx: RouteContext) -> list[APIRouter]:
    global_router = APIRouter(prefix="/api/v1")

    @global_router.get("/tokens", dependencies=[Depends(require_admin)])
    async def tokens_list() -> Dict[str, Any]:
        """List all user tokens (admin only)."""
        items = list_tokens()
        # Mask token values: show only prefix + last 4 chars
        safe_items: list[dict[str, Any]] = []
        for item in items:
            entry = dict(item)
            tok = str(entry.get("token") or "")
            entry["token_id"] = _token_id(tok) if tok else ""
            if len(tok) > 8:
                entry["token_preview"] = tok[:4] + "..." + tok[-4:]
            else:
                entry["token_preview"] = "****"
            entry.pop("token", None)
            safe_items.append(entry)
        return {"ok": True, "result": {"tokens": safe_items}}

    @global_router.post("/tokens", dependencies=[Depends(require_admin)])
    async def tokens_create(req: TokenCreateRequest) -> Dict[str, Any]:
        """Create a new user token (admin only)."""
        user_id = str(req.user_id or "").strip()
        if not user_id:
            raise HTTPException(
                status_code=400,
                detail={"code": "invalid_request", "message": "user_id is required", "details": {}},
            )
        # 首个 token 必须是 admin，防止创建普通 token 后锁死自己。
        if not req.is_admin:
            existing = list_tokens()
            has_admin = any(bool((t or {}).get("is_admin")) for t in existing if isinstance(t, dict))
            if not has_admin and not _configured_web_token():
                raise HTTPException(
                    status_code=400,
                    detail={"code": "admin_required_first", "message": "The first token must have admin privileges", "details": {}},
                )
        custom = str(req.custom_token or "").strip() or None
        try:
            entry = create_token(
                user_id,
                allowed_groups=list(req.allowed_groups),
                is_admin=bool(req.is_admin),
                custom_token=custom,
            )
        except ValueError as exc:
            raise HTTPException(
                status_code=400,
                detail={"code": "invalid_request", "message": str(exc), "details": {}},
            ) from exc
        return {"ok": True, "result": {"token": entry}}

    @global_router.patch("/tokens/{token_id}", dependencies=[Depends(require_admin)])
    async def tokens_update(token_id: str, req: TokenUpdateRequest) -> Dict[str, Any]:
        """Update a user token (admin only)."""
        tok = _resolve_raw_token(token_id)
        if not tok:
            raise HTTPException(
                status_code=400,
                detail={"code": "invalid_request", "message": "token_id is required", "details": {}},
            )
        entry = update_token(
            tok,
            allowed_groups=req.allowed_groups,
            is_admin=req.is_admin,
        )
        if entry is None:
            raise HTTPException(
                status_code=404,
                detail={"code": "not_found", "message": "token not found", "details": {}},
            )
        # 返回时隐藏完整 token
        result = dict(entry)
        raw = str(result.get("token") or "")
        result["token_id"] = _token_id(raw) if raw else ""
        if len(raw) > 8:
            result["token_preview"] = raw[:4] + "..." + raw[-4:]
        else:
            result["token_preview"] = "****"
        result.pop("token", None)
        return {"ok": True, "result": {"token": result}}

    @global_router.get("/tokens/{token_id}/reveal", dependencies=[Depends(require_admin)])
    async def tokens_reveal(token_id: str) -> Dict[str, Any]:
        """Reveal the full token value (admin only)."""
        tok = _resolve_raw_token(token_id)
        if not tok:
            raise HTTPException(
                status_code=400,
                detail={"code": "invalid_request", "message": "token_id is required", "details": {}},
            )
        from ....kernel.web_tokens import lookup_token as _lookup
        entry = _lookup(tok)
        if entry is None:
            raise HTTPException(
                status_code=404,
                detail={"code": "not_found", "message": "token not found", "details": {}},
            )
        return {"ok": True, "result": {"token": tok}}

    @global_router.delete("/tokens/{token_id}", dependencies=[Depends(require_admin)])
    async def tokens_delete(token_id: str) -> Dict[str, Any]:
        """Delete a user token (admin only)."""
        tok = _resolve_raw_token(token_id)
        if not tok:
            raise HTTPException(
                status_code=400,
                detail={"code": "invalid_request", "message": "token_id is required", "details": {}},
            )
        deleted = delete_token(tok)
        if not deleted:
            raise HTTPException(
                status_code=404,
                detail={"code": "not_found", "message": "token not found", "details": {}},
            )
        # Check if all tokens are now gone → system reverts to open access.
        remaining = list_tokens()
        tokens_remain = bool(remaining) or bool(_configured_web_token())
        return {"ok": True, "result": {"deleted": True, "tokens_remain": tokens_remain}}

    return [global_router]
