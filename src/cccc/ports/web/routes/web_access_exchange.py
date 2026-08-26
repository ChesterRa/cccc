from __future__ import annotations

from fastapi import APIRouter, HTTPException, Request
from fastapi.responses import RedirectResponse

from ....kernel.access_tokens import list_access_tokens
from ....kernel.web_login_grants import access_token_id, consume_web_login_grant
from ..middleware import served_request_origin, set_access_token_cookie
from ..schemas import RouteContext


def create_routers(ctx: RouteContext) -> list[APIRouter]:
    router = APIRouter(prefix="/api/v1")

    @router.get("/web_access/exchange")
    async def exchange(request: Request, code: str = "") -> RedirectResponse:
        return await exchange_web_access(ctx, request, code)

    return [router]


async def exchange_web_access(
    ctx: RouteContext,
    request: Request,
    code: str,
) -> RedirectResponse:
    origin = served_request_origin(request)
    try:
        token_id = consume_web_login_grant(code, origin, home=ctx.home)
    except (OSError, ValueError) as exc:
        raise HTTPException(
            status_code=500,
            detail={
                "code": "web_login_grant_store_error",
                "message": "Web login grant store is unavailable",
                "details": {},
            },
        ) from exc
    if not token_id:
        raise _invalid_grant()
    token = next(
        (
            str(item.get("token") or "").strip()
            for item in list_access_tokens(ctx.home)
            if isinstance(item, dict)
            and bool(item.get("is_admin"))
            and access_token_id(str(item.get("token") or "")) == token_id
        ),
        "",
    )
    if not token:
        raise _invalid_grant()
    request.state.skip_token_cookie_refresh = True
    response = RedirectResponse(url="/ui/", status_code=303)
    response.headers["Cache-Control"] = "no-store"
    response.headers["Referrer-Policy"] = "no-referrer"
    set_access_token_cookie(response, request, token)
    return response


def _invalid_grant() -> HTTPException:
    return HTTPException(
        status_code=401,
        detail={
            "code": "web_login_grant_invalid",
            "message": "Web login link is invalid or expired",
            "details": {},
        },
    )
