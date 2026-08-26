from __future__ import annotations

from typing import Any, Dict

from fastapi import HTTPException, Request

from ....kernel.access_tokens import lookup_access_token
from ....kernel.web_login_grants import access_token_id, issue_web_login_grant
from ..schemas import RouteContext


async def issue_reach_web_login(ctx: RouteContext, request: Request) -> Dict[str, Any]:
    status = await ctx.daemon({"op": "membership_status", "args": {"by": "user"}})
    membership = (
        (status.get("result") or {}).get("membership")
        if isinstance(status, dict) and isinstance(status.get("result"), dict)
        else None
    )
    if not isinstance(membership, dict) or membership.get("online") is not True:
        raise HTTPException(
            status_code=503,
            detail={
                "code": "membership_reach_offline",
                "message": "membership reach is not online",
                "details": {},
            },
        )
    authorization = str(request.headers.get("authorization") or "").strip()
    raw_token = (
        str(authorization[7:] or "").strip()
        if authorization.lower().startswith("bearer ")
        else str(request.cookies.get("cccc_access_token") or "").strip()
    )
    token = lookup_access_token(raw_token, ctx.home)
    if not isinstance(token, dict) or not bool(token.get("is_admin")):
        raise HTTPException(
            status_code=403,
            detail={
                "code": "admin_required",
                "message": "administrator access is required",
                "details": {},
            },
        )
    try:
        grant = issue_web_login_grant(
            str(membership.get("hostname") or ""),
            access_token_id(raw_token),
            home=ctx.home,
        )
    except (OSError, ValueError) as exc:
        raise HTTPException(
            status_code=500,
            detail={
                "code": "web_login_grant_store_error",
                "message": "could not create the Reach Web login link",
                "details": {},
            },
        ) from exc
    return {
        "ok": True,
        "result": {
            "web_url": f"{grant['origin']}/api/v1/web_access/exchange?code={grant['code']}",
            "expires_at_epoch": grant["expires_at_epoch"],
        },
    }
