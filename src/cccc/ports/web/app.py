from __future__ import annotations

import asyncio
from dataclasses import dataclass
import logging
import mimetypes
import os
import time
from pathlib import Path
from typing import Any, Dict, Literal, Optional

from fastapi import FastAPI, HTTPException, Request
from fastapi.exceptions import RequestValidationError
from fastapi.middleware.cors import CORSMiddleware
from fastapi.responses import JSONResponse
from fastapi.staticfiles import StaticFiles
from starlette.exceptions import HTTPException as StarletteHTTPException
from starlette.concurrency import run_in_threadpool

from ... import __version__
from ...daemon.server import call_daemon
from ...kernel.web_tokens import list_tokens, lookup_token
from ...paths import ensure_home
from ...util.obslog import setup_root_json_logging
from .schemas import RouteContext, _configured_web_token

logger = logging.getLogger("cccc.web")
_WEB_LOG_FH: Optional[Any] = None


@dataclass(frozen=True)
class Principal:
    kind: Literal["anonymous", "user"]
    user_id: str = ""
    allowed_groups: tuple[str, ...] = ()
    is_admin: bool = False


def _apply_web_logging(*, home: Path, level: str) -> None:
    global _WEB_LOG_FH
    try:
        d = home / "daemon"
        d.mkdir(parents=True, exist_ok=True)
        p = d / "cccc-web.log"
        if _WEB_LOG_FH is None:
            _WEB_LOG_FH = p.open("a", encoding="utf-8")
        setup_root_json_logging(component="web", level=level, stream=_WEB_LOG_FH, force=True)
    except Exception:
        # Fall back to stderr if file logging isn't possible.
        try:
            setup_root_json_logging(component="web", level=level, force=True)
        except Exception:
            pass


def _is_truthy_env(value: str) -> bool:
    return str(value or "").strip().lower() in ("1", "true", "yes", "y", "on")


def _web_mode() -> Literal["normal", "exhibit"]:
    """Return the web server mode.

    - normal: read/write control plane (default)
    - exhibit: read-only "public console" mode
    """
    mode = str(os.environ.get("CCCC_WEB_MODE") or "").strip().lower()
    if mode in ("exhibit", "readonly", "read-only", "ro"):
        return "exhibit"
    if _is_truthy_env(str(os.environ.get("CCCC_WEB_READONLY") or "")):
        return "exhibit"
    return "normal"


def _is_public_ui_path(request: Request) -> bool:
    path = str(request.url.path or "")
    return path.startswith("/ui/") or path == "/ui"


def _request_token_parts(request: Request) -> tuple[str, Literal["", "header", "cookie", "query"]]:
    auth = str(request.headers.get("authorization") or "").strip()
    if auth.lower().startswith("bearer "):
        return str(auth[7:] or "").strip(), "header"

    cookie = str(request.cookies.get("cccc_web_token") or "").strip()
    if cookie:
        return cookie, "cookie"

    q = str(request.query_params.get("token") or "").strip()
    if q:
        return q, "query"
    return "", ""


def _request_token(request: Request) -> str:
    return _request_token_parts(request)[0]


def _resolve_principal(request: Request) -> Principal:
    if _is_public_ui_path(request):
        return Principal(kind="anonymous")
    token = _request_token(request)
    if not token:
        return Principal(kind="anonymous")
    entry = lookup_token(token)
    if not isinstance(entry, dict) and token == _configured_web_token():
        return Principal(kind="user", user_id="admin", is_admin=True)
    if not isinstance(entry, dict):
        return Principal(kind="anonymous")
    user_id = str(entry.get("user_id") or "").strip()
    if not user_id:
        return Principal(kind="anonymous")
    groups_raw = entry.get("allowed_groups")
    allowed_groups = tuple(
        str(group_id or "").strip()
        for group_id in (groups_raw if isinstance(groups_raw, list) else [])
        if str(group_id or "").strip()
    )
    return Principal(
        kind="user",
        user_id=user_id,
        allowed_groups=allowed_groups,
        is_admin=bool(entry.get("is_admin", False)),
    )
async def _daemon(req: Dict[str, Any]) -> Dict[str, Any]:
    resp = await run_in_threadpool(call_daemon, req)
    if not resp.get("ok") and isinstance(resp.get("error"), dict) and resp["error"].get("code") == "daemon_unavailable":
        raise HTTPException(status_code=503, detail={"code": "daemon_unavailable", "message": "ccccd unavailable"})
    return resp


def create_app() -> FastAPI:
    app = FastAPI(title="cccc web", version=__version__)
    home = ensure_home()
    web_mode = _web_mode()
    read_only = web_mode == "exhibit"
    try:
        exhibit_cache_ttl_s = float(str(os.environ.get("CCCC_WEB_EXHIBIT_CACHE_SECONDS") or "1.0").strip() or "1.0")
    except Exception:
        exhibit_cache_ttl_s = 1.0
    exhibit_allow_terminal = _is_truthy_env(str(os.environ.get("CCCC_WEB_EXHIBIT_ALLOW_TERMINAL") or ""))

    # Tiny in-process cache for high-fanout read endpoints (exhibit mode only).
    cache: Dict[str, tuple[float, Dict[str, Any]]] = {}
    inflight: Dict[str, asyncio.Future[Dict[str, Any]]] = {}
    cache_lock = asyncio.Lock()

    async def _cached_json(key: str, ttl_s: float, fetcher) -> Dict[str, Any]:  # type: ignore[no-untyped-def]
        if not read_only or ttl_s <= 0:
            return await fetcher()
        now = time.monotonic()
        fut: asyncio.Future[Dict[str, Any]] | None = None
        do_fetch = False
        async with cache_lock:
            hit = cache.get(key)
            if hit is not None and hit[0] > now:
                return hit[1]
            fut = inflight.get(key)
            if fut is None or fut.done():
                loop = asyncio.get_running_loop()
                fut = loop.create_future()
                inflight[key] = fut
                do_fetch = True
        if fut is not None and not do_fetch:
            return await fut
        try:
            val = await fetcher()
            async with cache_lock:
                cache[key] = (time.monotonic() + ttl_s, val)
                if fut is not None and not fut.done():
                    fut.set_result(val)
            return val
        except Exception as e:
            async with cache_lock:
                if fut is not None and not fut.done():
                    fut.set_exception(e)
            raise
        finally:
            async with cache_lock:
                inflight.pop(key, None)

    # Some environments don't register the standard PWA manifest extension.
    mimetypes.add_type("application/manifest+json", ".webmanifest")

    # Configure web logging (best-effort) based on daemon observability settings.
    try:
        resp = call_daemon({"op": "observability_get"})
        obs = (resp.get("result") or {}).get("observability") if resp.get("ok") else None
        level = "INFO"
        if isinstance(obs, dict):
            level = str(obs.get("log_level") or "INFO").strip().upper() or "INFO"
            if obs.get("developer_mode") and level == "INFO":
                level = "DEBUG"
        _apply_web_logging(home=home, level=level)
    except Exception:
        try:
            _apply_web_logging(home=home, level="INFO")
        except Exception:
            pass

    dist = str(os.environ.get("CCCC_WEB_DIST") or "").strip()
    dist_dir: Optional[Path] = None
    if dist:
        try:
            candidate = Path(dist).expanduser().resolve()
            if candidate.exists():
                dist_dir = candidate
        except Exception:
            dist_dir = None
    else:
        # Prefer packaged UI under `cccc/ports/web/dist`.
        try:
            packaged = Path(__file__).resolve().parent / "dist"
            if packaged.exists():
                dist_dir = packaged
        except Exception:
            dist_dir = None

        # Dev fallback: repo-root `web/dist`.
        if dist_dir is None:
            try:
                for parent in Path(__file__).resolve().parents:
                    candidate = parent / "web" / "dist"
                    if candidate.exists():
                        dist_dir = candidate
                        break
            except Exception:
                dist_dir = None
    if dist_dir is not None:
        app.mount("/ui", StaticFiles(directory=str(dist_dir), html=True), name="ui")

    cors = str(os.environ.get("CCCC_WEB_CORS_ORIGINS") or "").strip()
    if cors:
        allow_origins = [o.strip() for o in cors.split(",") if o.strip()]
        if allow_origins:
            app.add_middleware(
                CORSMiddleware,
                allow_origins=allow_origins,
                allow_methods=["*"],
                allow_headers=["*"],
            )

    @app.middleware("http")
    async def _auth(request: Request, call_next):  # type: ignore[no-untyped-def]
        provided_token, token_source = _request_token_parts(request)
        principal = _resolve_principal(request)
        stale_cookie = False
        tokens_active = bool(list_tokens()) or bool(_configured_web_token())
        # header/query 是用户显式提供的认证材料，仍然严格按 401 收口；
        # cookie 在无 token 配置时允许匿名放行，并顺手清掉残留脏 cookie。
        if not _is_public_ui_path(request) and provided_token and principal.kind != "user":
            if token_source in ("header", "query") or tokens_active:
                return JSONResponse(
                    status_code=401,
                    content={"ok": False, "error": {"code": "unauthorized", "message": "missing/invalid token", "details": {}}},
                )
            if token_source == "cookie":
                stale_cookie = True
        # 未提供任何 token 但 token 认证已启用 → 对 API 路径返回 401，让前端显示登录框。
        if not _is_public_ui_path(request) and not provided_token and tokens_active and principal.kind != "user":
            path = str(request.url.path or "")
            if path.startswith("/api/"):
                return JSONResponse(
                    status_code=401,
                    content={"ok": False, "error": {"code": "unauthorized", "message": "authentication required", "details": {}}},
                )
        request.state.principal = principal

        resp = await call_next(request)
        if stale_cookie:
            resp.delete_cookie(key="cccc_web_token", path="/")
        if principal.kind == "user" and provided_token and str(request.cookies.get("cccc_web_token") or "").strip() != provided_token:
            # Detect real protocol: env override > proxy header > request scheme
            # Set CCCC_WEB_SECURE=1 when behind HTTPS proxy that doesn't send X-Forwarded-Proto
            force_secure = str(os.environ.get("CCCC_WEB_SECURE") or "").strip().lower() in ("1", "true", "yes")
            forwarded_proto = str(request.headers.get("x-forwarded-proto") or "").strip().lower()
            actual_scheme = "https" if force_secure else (forwarded_proto if forwarded_proto in ("http", "https") else str(getattr(request.url, "scheme", "") or "").lower())
            resp.set_cookie(
                key="cccc_web_token",
                value=provided_token,
                httponly=True,
                samesite="none" if actual_scheme == "https" else "lax",
                secure=actual_scheme == "https",
                path="/",
            )
        return resp

    @app.middleware("http")
    async def _read_only_guard(request: Request, call_next):  # type: ignore[no-untyped-def]
        if read_only:
            m = str(request.method or "").upper()
            if m not in ("GET", "HEAD", "OPTIONS"):
                return JSONResponse(
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
        return await call_next(request)

    @app.exception_handler(HTTPException)
    async def _handle_fastapi_http_exception(_request: Request, exc: HTTPException) -> JSONResponse:
        detail = exc.detail
        if isinstance(detail, dict):
            code = str(detail.get("code") or "http_error")
            msg = str(detail.get("message") or "HTTP error")
            details: Any = detail.get("details") if "details" in detail else detail
        else:
            code = "http_error"
            msg = str(detail) if detail else "HTTP error"
            details = detail
        return JSONResponse(status_code=int(getattr(exc, "status_code", 500) or 500), content={"ok": False, "error": {"code": code, "message": msg, "details": details}})

    @app.exception_handler(StarletteHTTPException)
    async def _handle_starlette_http_exception(_request: Request, exc: StarletteHTTPException) -> JSONResponse:
        code = "not_found" if int(getattr(exc, "status_code", 500) or 500) == 404 else "http_error"
        msg = str(getattr(exc, "detail", "") or "HTTP error")
        return JSONResponse(status_code=int(getattr(exc, "status_code", 500) or 500), content={"ok": False, "error": {"code": code, "message": msg, "details": {}}})

    @app.exception_handler(RequestValidationError)
    async def _handle_request_validation_error(_request: Request, exc: RequestValidationError) -> JSONResponse:
        # Never echo request inputs back to the client in validation errors (could include secrets).
        safe: list[dict[str, Any]] = []
        try:
            for err in exc.errors():
                if not isinstance(err, dict):
                    continue
                out: dict[str, Any] = {}
                for k in ("loc", "msg", "type"):
                    if k in err:
                        out[k] = err.get(k)
                if out:
                    safe.append(out)
        except Exception:
            safe = []
        return JSONResponse(
            status_code=422,
            content={"ok": False, "error": {"code": "validation_error", "message": "invalid request", "details": safe}},
        )

    @app.exception_handler(Exception)
    async def _handle_unexpected_exception(_request: Request, exc: Exception) -> JSONResponse:
        logger.exception("unhandled exception in cccc web")
        return JSONResponse(status_code=500, content={"ok": False, "error": {"code": "internal_error", "message": "internal error", "details": {}}})

    @app.middleware("http")
    async def _ui_cache_control(request: Request, call_next):  # type: ignore[no-untyped-def]
        resp = await call_next(request)
        # Avoid "why didn't my UI update?" confusion during local development.
        # Vite config uses stable filenames, so we force revalidation.
        if str(request.url.path or "").startswith("/ui"):
            resp.headers["Cache-Control"] = "no-cache"
        return resp

    from .routes.base import register_base_routes
    from .routes.space import create_routers as create_space_routers
    from .routes.groups import register_group_routes
    from .routes.messaging import create_routers as create_messaging_routers
    from .routes.actors import create_routers as create_actor_routers
    from .routes.im import register_im_routes
    from .routes.tokens import create_routers as create_token_routers

    route_ctx = RouteContext(
        home=home,
        version=__version__,
        web_mode=web_mode,
        read_only=read_only,
        exhibit_cache_ttl_s=exhibit_cache_ttl_s,
        exhibit_allow_terminal=exhibit_allow_terminal,
        dist_dir=dist_dir,
        daemon=_daemon,
        cached_json=_cached_json,
        apply_web_logging=_apply_web_logging,
        configured_web_token=_configured_web_token,
    )

    register_base_routes(app, ctx=route_ctx)
    for router in create_space_routers(route_ctx):
        app.include_router(router)
    register_group_routes(app, ctx=route_ctx)
    for router in create_messaging_routers(route_ctx):
        app.include_router(router)
    for router in create_actor_routers(route_ctx):
        app.include_router(router)
    register_im_routes(app, ctx=route_ctx)
    for router in create_token_routers(route_ctx):
        app.include_router(router)

    return app
