from __future__ import annotations

import asyncio
from contextlib import asynccontextmanager
from dataclasses import dataclass
import logging
import mimetypes
import os
import threading
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
from ...kernel.access_tokens import list_access_tokens, lookup_access_token
from ...paths import ensure_home
from ...util.obslog import apply_logger_levels, setup_root_json_logging
from ...util.process import pid_is_alive, terminate_pid
from .runtime_control import (
    WEB_RUNTIME_RESTART_EXIT_CODE,
    clear_web_runtime_state,
    write_web_runtime_state,
)
from .middleware import AuthMiddleware, ReadOnlyGuardMiddleware, UiCacheControlMiddleware
from .schemas import RouteContext

logger = logging.getLogger("cccc.web")
_WEB_LOG_FH: Optional[Any] = None
_WEB_LOG_PATH: Optional[Path] = None
_SIGNED_OUT_COOKIE = "cccc_signed_out"


@dataclass(frozen=True)
class Principal:
    kind: Literal["anonymous", "user"]
    user_id: str = ""
    allowed_groups: tuple[str, ...] = ()
    is_admin: bool = False


def _close_web_logging() -> None:
    global _WEB_LOG_FH, _WEB_LOG_PATH
    try:
        if _WEB_LOG_FH is not None:
            _WEB_LOG_FH.close()
    except Exception:
        pass
    _WEB_LOG_FH = None
    _WEB_LOG_PATH = None


def _apply_web_logging(*, home: Path, level: str, logger_levels: Optional[Dict[str, str]] = None) -> None:
    global _WEB_LOG_FH, _WEB_LOG_PATH
    try:
        d = home / "daemon"
        d.mkdir(parents=True, exist_ok=True)
        p = d / "cccc-web.log"
        if _WEB_LOG_FH is not None and _WEB_LOG_PATH is not None and _WEB_LOG_PATH != p:
            _close_web_logging()
        if _WEB_LOG_FH is None:
            _WEB_LOG_FH = p.open("a", encoding="utf-8")
            _WEB_LOG_PATH = p
        setup_root_json_logging(component="web", level=level, stream=_WEB_LOG_FH, force=True)
        apply_logger_levels(logger_levels)
    except Exception:
        # Fall back to stderr if file logging isn't possible.
        try:
            setup_root_json_logging(component="web", level=level, force=True)
            apply_logger_levels(logger_levels)
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


_PUBLIC_API_PATHS = frozenset({"/api/v1/health", "/api/v1/branding"})


def _is_public_ui_path(request: Request) -> bool:
    path = str(request.url.path or "")
    return path.startswith("/ui/") or path == "/ui"


def _is_public_path(request: Request) -> bool:
    """Routes that bypass token authentication (UI assets + health check)."""
    path = str(request.url.path or "")
    return (
        _is_public_ui_path(request)
        or path in _PUBLIC_API_PATHS
        or path.startswith("/api/v1/branding/assets/")
        or path.startswith("/mcp/web-model/")
    )


def _request_token_parts(request: Request) -> tuple[str, Literal["", "header", "cookie", "query"]]:
    auth = str(request.headers.get("authorization") or "").strip()
    if auth.lower().startswith("bearer "):
        return str(auth[7:] or "").strip(), "header"

    cookie = str(request.cookies.get("cccc_access_token") or "").strip()
    if cookie:
        return cookie, "cookie"

    q = str(request.query_params.get("token") or "").strip()
    if q:
        return q, "query"
    return "", ""


def _request_token(request: Request) -> str:
    return _request_token_parts(request)[0]


def _resolve_principal(request: Request) -> Principal:
    token = _request_token(request)
    if not token:
        return Principal(kind="anonymous")
    entry = lookup_access_token(token)
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
        err = resp.get("error") if isinstance(resp.get("error"), dict) else {}
        detail = {
            "code": "daemon_unavailable",
            "message": "ccccd unavailable",
        }
        err_details = err.get("details") if isinstance(err.get("details"), dict) else None
        if err_details:
            detail["details"] = err_details
        raise HTTPException(status_code=503, detail=detail)
    return resp


def create_app() -> FastAPI:
    def _int_env(name: str, default: int) -> int:
        raw = str(os.environ.get(name) or "").strip()
        if not raw:
            return int(default)
        try:
            return int(raw)
        except Exception:
            return int(default)

    @asynccontextmanager
    async def _lifespan(_app: FastAPI):
        restart_supported = str(os.environ.get("CCCC_WEB_SUPERVISED") or "").strip().lower() in ("1", "true", "yes", "on")
        runtime_host_raw = str(os.environ.get("CCCC_WEB_EFFECTIVE_HOST") or "").strip()
        runtime_port_raw = str(os.environ.get("CCCC_WEB_EFFECTIVE_PORT") or "").strip()
        runtime_binding_known = restart_supported or bool(runtime_host_raw or runtime_port_raw)
        runtime_pid = os.getpid()
        runtime_launcher_pid = os.getppid() if restart_supported else 0
        runtime_host = runtime_host_raw or "127.0.0.1"
        runtime_port = _int_env("CCCC_WEB_EFFECTIVE_PORT", 8848)
        runtime_mode = str(os.environ.get("CCCC_WEB_EFFECTIVE_MODE") or _web_mode()).strip() or "normal"
        runtime_supervisor_pid = _int_env("CCCC_WEB_SUPERVISOR_PID", 0)
        runtime_launch_source = str(os.environ.get("CCCC_WEB_LAUNCH_SOURCE") or "").strip() or "unknown"
        supervisor_watchdog_stop = threading.Event()
        supervisor_watchdog_thread: Optional[threading.Thread] = None

        if runtime_binding_known:
            write_web_runtime_state(
                home=home,
                pid=runtime_pid,
                host=runtime_host,
                port=runtime_port,
                mode=runtime_mode,
                supervisor_managed=restart_supported,
                supervisor_pid=runtime_supervisor_pid if restart_supported and runtime_supervisor_pid > 0 else None,
                launcher_pid=runtime_launcher_pid if runtime_launcher_pid > 0 else None,
                launch_source=runtime_launch_source,
            )

        if restart_supported:
            def _request_web_restart() -> None:
                # Let the HTTP response flush before terminating this child.
                clear_web_runtime_state(home=home, pid=runtime_pid)
                time.sleep(0.2)
                os._exit(WEB_RUNTIME_RESTART_EXIT_CODE)

            _app.state.request_web_restart = _request_web_restart
        else:
            _app.state.request_web_restart = None

        if restart_supported and runtime_supervisor_pid > 0:
            def _supervisor_watchdog_loop() -> None:
                while not supervisor_watchdog_stop.is_set():
                    try:
                        if pid_is_alive(runtime_supervisor_pid):
                            supervisor_watchdog_stop.wait(0.5)
                            continue
                    except Exception:
                        pass
                    try:
                        clear_web_runtime_state(home=home, pid=runtime_pid)
                    except Exception:
                        pass
                    if runtime_launcher_pid > 0:
                        try:
                            terminate_pid(runtime_launcher_pid, timeout_s=1.0, include_group=True, force=True)
                        except Exception:
                            pass
                    os._exit(0)

            supervisor_watchdog_thread = threading.Thread(
                target=_supervisor_watchdog_loop,
                name="cccc-web-supervisor-watchdog",
                daemon=True,
            )
            supervisor_watchdog_thread.start()

        try:
            yield
        finally:
            supervisor_watchdog_stop.set()
            if supervisor_watchdog_thread is not None:
                try:
                    supervisor_watchdog_thread.join(timeout=0.2)
                except Exception:
                    pass
            if runtime_binding_known:
                clear_web_runtime_state(home=home, pid=runtime_pid)
            _close_web_logging()

    app = FastAPI(title="cccc web", version=__version__, lifespan=_lifespan)
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
        logger_levels: Dict[str, str] = {}
        if isinstance(obs, dict):
            requested_level = str(obs.get("log_level") or "INFO").strip().upper() or "INFO"
            effective_level = "DEBUG" if obs.get("developer_mode") and requested_level == "INFO" else requested_level
            level = "INFO" if effective_level == "DEBUG" else effective_level
            if isinstance(obs.get("logger_levels"), dict):
                logger_levels = {
                    str(name): str(value)
                    for name, value in obs.get("logger_levels", {}).items()
                }
            if effective_level == "DEBUG":
                logger_levels.setdefault("cccc", "DEBUG")
                for noisy_logger in (
                    "asyncio",
                    "httpcore",
                    "httpx",
                    "cccc.delivery",
                    "cccc.providers.notebooklm._vendor.notebooklm",
                ):
                    logger_levels.setdefault(noisy_logger, "INFO")
        _apply_web_logging(home=home, level=level, logger_levels=logger_levels)
    except Exception:
        try:
            _apply_web_logging(home=home, level="INFO", logger_levels={})
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

    app.add_middleware(
        AuthMiddleware,
        signed_out_cookie=_SIGNED_OUT_COOKIE,
        request_token_parts=_request_token_parts,
        is_public_path=_is_public_path,
        resolve_principal=_resolve_principal,
        tokens_active=list_access_tokens,
    )
    app.add_middleware(ReadOnlyGuardMiddleware, read_only=read_only)
    app.add_middleware(UiCacheControlMiddleware)

    from .routes.base import register_base_routes
    from .routes.space import create_routers as create_space_routers
    from .routes.groups import register_group_routes
    from .routes.messaging import create_routers as create_messaging_routers
    from .routes.actors import create_routers as create_actor_routers
    from .routes.im import register_im_routes
    from .routes.access_tokens import create_routers as create_access_token_routers

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
    for router in create_access_token_routers(route_ctx):
        app.include_router(router)

    return app
