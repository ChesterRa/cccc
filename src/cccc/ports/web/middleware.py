from __future__ import annotations

import os
from typing import Callable

from fastapi import Request
from fastapi.responses import JSONResponse, Response
from starlette.datastructures import MutableHeaders
from starlette.types import ASGIApp, Message, Receive, Scope, Send

StateGetter = Callable[[Scope, str, object], object]
TokenPartsGetter = Callable[[Request], tuple[str, str]]
PublicPathChecker = Callable[[Request], bool]
PrincipalResolver = Callable[[Request], object]
TokensActiveGetter = Callable[[], bool]


def _scope_state_get(scope: Scope, key: str, default: object = None) -> object:
    state = scope.setdefault("state", {})
    if not isinstance(state, dict):
        return default
    return state.get(key, default)


def _actual_request_scheme(request: Request) -> str:
    force_secure = str(os.environ.get("CCCC_WEB_SECURE") or "").strip().lower() in ("1", "true", "yes")
    forwarded_proto = str(request.headers.get("x-forwarded-proto") or "").strip().lower()
    return "https" if force_secure else (
        forwarded_proto if forwarded_proto in ("http", "https") else str(getattr(request.url, "scheme", "") or "").lower()
    )


def set_access_token_cookie(response: Response, request: Request, token: str) -> None:
    actual_scheme = _actual_request_scheme(request)
    response.set_cookie(
        key="cccc_access_token",
        value=token,
        httponly=True,
        samesite="none" if actual_scheme == "https" else "lax",
        secure=actual_scheme == "https",
        path="/",
    )


class AuthMiddleware:
    def __init__(
        self,
        app: ASGIApp,
        *,
        signed_out_cookie: str,
        request_token_parts: TokenPartsGetter,
        is_public_path: PublicPathChecker,
        resolve_principal: PrincipalResolver,
        tokens_active: TokensActiveGetter,
        state_getter: StateGetter = _scope_state_get,
    ):
        self.app = app
        self._signed_out_cookie = signed_out_cookie
        self._request_token_parts = request_token_parts
        self._is_public_path = is_public_path
        self._resolve_principal = resolve_principal
        self._tokens_active = tokens_active
        self._state_getter = state_getter

    async def __call__(self, scope: Scope, receive: Receive, send: Send) -> None:
        if scope.get("type") != "http":
            await self.app(scope, receive, send)
            return

        request = Request(scope, receive=receive)
        provided_token, token_source = self._request_token_parts(request)
        logout_marker = str(request.cookies.get(self._signed_out_cookie) or "").strip() == "1"
        if logout_marker and token_source == "cookie":
            provided_token = ""
            token_source = ""
        principal = self._resolve_principal(request if not logout_marker else request)
        stale_cookie = logout_marker and bool(str(request.cookies.get("cccc_access_token") or "").strip())
        if logout_marker and stale_cookie:
            principal = type(principal)(kind="anonymous")
        tokens_active = bool(self._tokens_active())

        if not self._is_public_path(request) and provided_token and principal.kind != "user":
            if token_source in ("header", "query") or tokens_active:
                resp = JSONResponse(
                    status_code=401,
                    content={"ok": False, "error": {"code": "unauthorized", "message": "missing/invalid token", "details": {}}},
                )
                await resp(scope, receive, send)
                return
            if token_source == "cookie":
                stale_cookie = True

        if not self._is_public_path(request) and not provided_token and tokens_active and principal.kind != "user":
            path = str(request.url.path or "")
            if path.startswith("/api/"):
                resp = JSONResponse(
                    status_code=401,
                    content={"ok": False, "error": {"code": "unauthorized", "message": "authentication required", "details": {}}},
                )
                await resp(scope, receive, send)
                return

        scope.setdefault("state", {})
        scope["state"]["principal"] = principal

        async def send_with_auth(message: Message) -> None:
            if message["type"] == "http.response.start":
                cookie_headers: list[tuple[bytes, bytes]] = []
                if stale_cookie:
                    temp = Response()
                    temp.delete_cookie(key="cccc_access_token", path="/")
                    cookie_headers.extend(temp.raw_headers)

                if logout_marker and principal.kind == "user" and token_source in ("header", "query"):
                    temp = Response()
                    temp.delete_cookie(key=self._signed_out_cookie, path="/")
                    cookie_headers.extend(temp.raw_headers)

                skip_cookie_refresh = bool(self._state_getter(scope, "skip_token_cookie_refresh", False))
                if (
                    not skip_cookie_refresh
                    and principal.kind == "user"
                    and provided_token
                    and str(request.cookies.get("cccc_access_token") or "").strip() != provided_token
                ):
                    temp = Response()
                    set_access_token_cookie(temp, request, provided_token)
                    cookie_headers.extend(temp.raw_headers)

                if cookie_headers:
                    headers = MutableHeaders(raw=message["headers"])
                    for header_name, header_value in cookie_headers:
                        if header_name.lower() == b"set-cookie":
                            headers.append("set-cookie", header_value.decode("latin-1"))
            await send(message)

        await self.app(scope, receive, send_with_auth)


class ReadOnlyGuardMiddleware:
    def __init__(self, app: ASGIApp, *, read_only: bool):
        self.app = app
        self.read_only = read_only

    async def __call__(self, scope: Scope, receive: Receive, send: Send) -> None:
        if scope.get("type") != "http":
            await self.app(scope, receive, send)
            return
        if self.read_only:
            method = str(scope.get("method") or "").upper()
            if method not in ("GET", "HEAD", "OPTIONS"):
                resp = JSONResponse(
                    status_code=403,
                    content={
                        "ok": False,
                        "error": {
                            "code": "read_only",
                            "message": "CCCC Web is running in read-only (exhibit) mode.",
                            "details": {},
                        },
                    },
                )
                await resp(scope, receive, send)
                return
        await self.app(scope, receive, send)


class UiCacheControlMiddleware:
    def __init__(self, app: ASGIApp):
        self.app = app

    async def __call__(self, scope: Scope, receive: Receive, send: Send) -> None:
        if scope.get("type") != "http":
            await self.app(scope, receive, send)
            return
        path = str(scope.get("path") or "")

        async def send_with_cache_control(message: Message) -> None:
            if message["type"] == "http.response.start" and path.startswith("/ui"):
                headers = MutableHeaders(raw=message["headers"])
                headers["Cache-Control"] = "no-cache"
            await send(message)

        await self.app(scope, receive, send_with_cache_control)
