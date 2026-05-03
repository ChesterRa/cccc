from __future__ import annotations

import asyncio
import json
import socket
from pathlib import Path
from typing import Any, Dict, Optional

from fastapi import APIRouter, BackgroundTasks, Depends, FastAPI, File, Form, HTTPException, Request, UploadFile, WebSocket
from fastapi.responses import FileResponse, HTMLResponse, JSONResponse, Response, StreamingResponse
from pydantic import BaseModel
from starlette.concurrency import run_in_threadpool

from ....kernel.access_tokens import list_access_tokens
from ....kernel.actors import find_actor
from ....kernel.group import load_group
from ....kernel.scope import detect_scope
from ....kernel.settings import get_observability_settings, get_web_branding_settings, resolve_remote_access_web_binding
from ....kernel.web_model_connectors import (
    create_web_model_connector,
    load_web_model_connectors,
    mask_web_model_connector,
    record_web_model_connector_activity,
    revoke_web_model_connector,
    verify_web_model_connector_secret,
)
from ....daemon.actors.web_model_actor_policy import require_no_other_chatgpt_web_model_actor
from ....daemon.runner_state_ops import headless_state_running
from ....util.conv import coerce_bool
from ....util.time import utc_now_iso
from ..branding import (
    build_branding_payload,
    delete_branding_asset,
    normalize_branding_asset_kind,
    resolve_branding_asset_path,
    store_branding_asset,
)
from ...mcp.common import runtime_context_override
from ..schemas import (
    BrandingUpdateRequest,
    DebugClearLogsRequest,
    ObservabilityUpdateRequest,
    RegistryReconcileRequest,
    RemoteAccessConfigureRequest,
    RouteContext,
    check_group,
    check_admin,
    get_principal,
    require_admin,
    require_group,
    require_user,
    resolve_websocket_principal,
    websocket_tokens_active,
)

_WEB_MODEL_BROWSER_STREAM_LIMIT_BYTES = 16 * 1024 * 1024


class WebModelConnectorCreateRequest(BaseModel):
    group_id: str
    actor_id: str
    provider: str = ""
    label: str = ""


class WebModelBrowserSessionRequest(BaseModel):
    group_id: str
    actor_id: str
    visibility: str = "visible"
    width: int = 1366
    height: int = 900


class WebModelBrowserBindRequest(BaseModel):
    group_id: str
    actor_id: str
    conversation_url: str = ""
    new_chat: bool = False
    clear: bool = False


def create_routers(ctx: RouteContext) -> list[APIRouter]:
    # --- global router (user/admin scope, per-route guard where needed) ---
    global_router = APIRouter()

    # --- group-scoped router ---
    group_router = APIRouter(prefix="/api/v1/groups/{group_id}", dependencies=[Depends(require_group)])

    def _default_branding_asset_path(asset_kind: str) -> Path | None:
        if ctx.dist_dir is None:
            return None
        if asset_kind == "logo_icon":
            candidate = ctx.dist_dir / "logo.svg"
            return candidate if candidate.exists() else None
        if asset_kind == "favicon":
            svg_candidate = ctx.dist_dir / "logo.svg"
            if svg_candidate.exists():
                return svg_candidate
            png_candidate = ctx.dist_dir / "favicon.png"
            return png_candidate if png_candidate.exists() else None
        return None

    def _branding_asset_file_response(asset_kind: str) -> FileResponse:
        raw = get_web_branding_settings()
        key = f"{asset_kind}_asset_path"
        rel_path = str(raw.get(key) or "").strip()
        if rel_path:
            try:
                return FileResponse(resolve_branding_asset_path(rel_path))
            except Exception:
                pass
        default_path = _default_branding_asset_path(asset_kind)
        if default_path is not None:
            return FileResponse(default_path)
        raise HTTPException(status_code=404)

    # ------------------------------------------------------------------ #
    # Global routes (public + admin, per-route guard where needed)
    # ------------------------------------------------------------------ #

    @global_router.get("/", response_class=HTMLResponse)
    async def index() -> str:
        if ctx.dist_dir is not None:
            return '<meta http-equiv="refresh" content="0; url=/ui/">'
        return (
            "<h3>cccc web</h3>"
            "<p>This is a minimal control-plane port. UI will live under <code>/ui</code> later.</p>"
            "<p>Try <code>/api/v1/ping</code> and <code>/api/v1/groups</code>.</p>"
        )

    @global_router.get("/favicon.ico")
    async def favicon_ico() -> Any:
        raw = get_web_branding_settings()
        if str(raw.get("favicon_asset_path") or "").strip():
            return _branding_asset_file_response("favicon")
        if ctx.dist_dir is not None and (ctx.dist_dir / "favicon.ico").exists():
            return FileResponse(ctx.dist_dir / "favicon.ico")
        raise HTTPException(status_code=404)

    @global_router.get("/favicon.png")
    async def favicon_png() -> Any:
        raw = get_web_branding_settings()
        if str(raw.get("favicon_asset_path") or "").strip():
            return _branding_asset_file_response("favicon")
        if ctx.dist_dir is not None and (ctx.dist_dir / "favicon.png").exists():
            return FileResponse(ctx.dist_dir / "favicon.png")
        raise HTTPException(status_code=404)

    @global_router.get("/api/v1/branding")
    async def branding_get() -> Dict[str, Any]:
        resp = await ctx.daemon({"op": "branding_get"})
        raw = (resp.get("result") or {}).get("branding") if resp.get("ok") else None
        if not isinstance(raw, dict):
            raw = get_web_branding_settings()
        return {"ok": True, "result": {"branding": build_branding_payload(raw)}}

    @global_router.get("/api/v1/branding/assets/{asset_kind}")
    async def branding_asset_get(asset_kind: str) -> FileResponse:
        try:
            normalized_kind = normalize_branding_asset_kind(asset_kind)
        except ValueError as exc:
            raise HTTPException(status_code=400, detail={"code": "invalid_request", "message": str(exc)}) from exc
        return _branding_asset_file_response(normalized_kind)

    @global_router.get("/api/v1/ping")
    async def ping(include_home: bool = False) -> Dict[str, Any]:
        resp = await ctx.daemon({"op": "ping"})
        result: Dict[str, Any] = {
            "daemon": resp.get("result", {}),
            "version": ctx.version,
            "web": {"mode": ctx.web_mode, "read_only": ctx.read_only},
        }
        if include_home:
            result["home"] = str(ctx.home)
        return {
            "ok": True,
            "result": result,
        }

    @global_router.get("/api/v1/health")
    async def health(request: Request) -> Dict[str, Any]:
        """Health check endpoint for monitoring (public, no auth required)."""
        daemon_resp = await ctx.daemon({"op": "ping"})
        daemon_ok = daemon_resp.get("ok", False)

        result: Dict[str, Any] = {"daemon": "running" if daemon_ok else "stopped"}
        # Only expose detailed info to authenticated users.
        principal = get_principal(request)
        if getattr(principal, "kind", "anonymous") == "user":
            result["version"] = ctx.version
            result["home"] = str(ctx.home)

        return {"ok": daemon_ok, "result": result}

    def _mcp_jsonrpc_error(req_id: Any, code: int, message: str) -> Dict[str, Any]:
        return {"jsonrpc": "2.0", "id": req_id, "error": {"code": code, "message": message}}

    def _connector_base_url(request: Request) -> str:
        binding = resolve_remote_access_web_binding()
        public_url = str(binding.get("web_public_url") or "").strip()
        if public_url:
            base = public_url.rstrip("/")
            if base.endswith("/ui"):
                base = base[: -len("/ui")]
            return base.rstrip("/")
        return str(request.base_url).rstrip("/")

    def _connector_url(request: Request, connector_id: str) -> str:
        return f"{_connector_base_url(request)}/mcp/web-model/{connector_id}"

    def _connector_url_with_token(connector_url: str, secret: str) -> str:
        url = str(connector_url or "").strip()
        token = str(secret or "").strip()
        if not url or not token:
            return ""
        sep = "&" if "?" in url else "?"
        from urllib.parse import urlencode

        return f"{url}{sep}{urlencode({'token': token})}"

    def _web_model_connector_web_payload(request: Request, item: Dict[str, Any]) -> Dict[str, Any]:
        connector_id = str(item.get("connector_id") or "").strip()
        entry = mask_web_model_connector(item)
        connector_url = _connector_url(request, connector_id) if connector_id else ""
        secret = str(item.get("secret") or "").strip()
        entry["connector_url"] = connector_url
        entry["secret_available"] = bool(secret)
        token_url = _connector_url_with_token(connector_url, secret)
        if token_url:
            entry["connector_url_with_token"] = token_url
        return entry

    def _is_global_web_model_browser_setup(group_id: str, actor_id: str) -> bool:
        return not str(group_id or "").strip() and not str(actor_id or "").strip()

    def _require_web_model_browser_actor(group_id: str, actor_id: str, *, allow_global_setup: bool = False) -> None:
        gid = str(group_id or "").strip()
        aid = str(actor_id or "").strip()
        if allow_global_setup and _is_global_web_model_browser_setup(gid, aid):
            return
        if not gid:
            raise HTTPException(status_code=400, detail={"code": "missing_group_id", "message": "missing group_id", "details": {}})
        if not aid:
            raise HTTPException(status_code=400, detail={"code": "missing_actor_id", "message": "missing actor_id", "details": {}})
        group = load_group(gid)
        if group is None:
            raise HTTPException(status_code=404, detail={"code": "group_not_found", "message": f"group not found: {gid}", "details": {}})
        actor = find_actor(group, aid)
        if not isinstance(actor, dict):
            raise HTTPException(status_code=404, detail={"code": "actor_not_found", "message": f"actor not found: {aid}", "details": {}})
        if str(actor.get("runtime") or "").strip().lower() != "web_model":
            raise HTTPException(
                status_code=400,
                detail={
                    "code": "invalid_actor_runtime",
                    "message": "ChatGPT browser sessions can only be bound to actors using runtime=web_model",
                    "details": {"group_id": gid, "actor_id": aid},
                },
            )

    def _extract_bearer_or_query_token(request: Request) -> str:
        auth = str(request.headers.get("authorization") or "").strip()
        if auth.lower().startswith("bearer "):
            return str(auth[7:] or "").strip()
        return str(request.query_params.get("token") or "").strip()

    def _resolve_web_model_connector(request: Request, connector_id: str) -> Dict[str, Any]:
        secret = _extract_bearer_or_query_token(request)
        connector = verify_web_model_connector_secret(connector_id, secret)
        if connector is None:
            raise HTTPException(
                status_code=401,
                detail={
                    "code": "unauthorized",
                    "message": "invalid web-model connector credentials",
                    "details": {"connector_id": str(connector_id or "").strip()},
                },
            )
        connector_id_clean = str(connector.get("connector_id") or connector_id or "").strip()
        group_id = str(connector.get("group_id") or "").strip()
        actor_id = str(connector.get("actor_id") or "").strip()

        def _reject_live_binding(code: str, message: str) -> None:
            if connector_id_clean:
                try:
                    record_web_model_connector_activity(
                        connector_id_clean,
                        method=str(request.method or "").strip(),
                        call_status="error",
                        error=code,
                    )
                except Exception:
                    pass
            raise HTTPException(
                status_code=403,
                detail={
                    "code": code,
                    "message": message,
                    "details": {"connector_id": connector_id_clean, "group_id": group_id, "actor_id": actor_id},
                },
            )

        group = load_group(group_id)
        if group is None:
            _reject_live_binding("connector_group_unavailable", "web-model connector group is no longer available")
        actor = find_actor(group, actor_id)
        if not isinstance(actor, dict):
            _reject_live_binding("connector_actor_unavailable", "web-model connector actor is no longer available")
        if str(actor.get("runtime") or "").strip().lower() != "web_model":
            _reject_live_binding("invalid_actor_runtime", "web-model connector actor is no longer runtime=web_model")
        if str(actor.get("runner") or "headless").strip().lower() != "headless":
            _reject_live_binding("invalid_actor_runner", "web-model connector actor is no longer headless")
        if not coerce_bool(actor.get("enabled"), default=True) or not headless_state_running(group_id, actor_id):
            _reject_live_binding("connector_actor_stopped", "web-model connector actor is stopped")
        return connector

    async def _handle_remote_mcp_payload(payload: Any, *, connector: Optional[Dict[str, Any]] = None) -> Response:
        from ...mcp.main import handle_request as handle_mcp_request

        def _tool_call_activity(item: Dict[str, Any], resp: Dict[str, Any]) -> Dict[str, str]:
            method = str(item.get("method") or "").strip()
            params = item.get("params") if isinstance(item.get("params"), dict) else {}
            tool_name = str(params.get("name") or "").strip() if method == "tools/call" else ""
            call_status = "error" if isinstance(resp, dict) and ("error" in resp or bool((resp.get("result") or {}).get("isError"))) else "ok"
            wait_status = ""
            turn_id = ""
            error = ""
            if isinstance(resp, dict) and isinstance(resp.get("error"), dict):
                error = str((resp.get("error") or {}).get("message") or "").strip()
            result = resp.get("result") if isinstance(resp, dict) else {}
            if isinstance(result, dict):
                content = result.get("content")
                text = ""
                if isinstance(content, list) and content and isinstance(content[0], dict):
                    text = str(content[0].get("text") or "").strip()
                if text:
                    try:
                        parsed = json.loads(text)
                    except Exception:
                        parsed = {}
                    if isinstance(parsed, dict):
                        if isinstance(parsed.get("error"), dict):
                            error = str((parsed.get("error") or {}).get("message") or error).strip()
                        if tool_name == "cccc_runtime_wait_next_turn":
                            wait_status = str(parsed.get("status") or "").strip()
                            turn = parsed.get("turn") if isinstance(parsed.get("turn"), dict) else {}
                            turn_id = str(turn.get("turn_id") or "").strip() if isinstance(turn, dict) else ""
                        elif tool_name == "cccc_runtime_complete_turn":
                            wait_status = str(parsed.get("status") or "").strip()
                            turn_id = str(parsed.get("turn_id") or "").strip()
            return {
                "method": method,
                "tool_name": tool_name,
                "call_status": call_status,
                "wait_status": wait_status,
                "turn_id": turn_id,
                "error": error,
            }

        async def _handle_one(item: Any) -> Dict[str, Any]:
            if not isinstance(item, dict):
                return _mcp_jsonrpc_error(None, -32600, "Invalid Request")

            method = str(item.get("method") or "").strip()
            params = item.get("params") if isinstance(item.get("params"), dict) else {}
            requested_tool_name = str(params.get("name") or "").strip() if method == "tools/call" else ""

            async def _record_browser_progress(reason: str, detail: str) -> None:
                if not isinstance(connector, dict):
                    return
                if method != "tools/call" or not requested_tool_name or requested_tool_name == "cccc_runtime_complete_turn":
                    return
                try:
                    from ....daemon.actors.web_model_tool_confirm_watcher import record_web_model_browser_progress

                    await run_in_threadpool(
                        record_web_model_browser_progress,
                        str(connector.get("group_id") or ""),
                        str(connector.get("actor_id") or ""),
                        reason=reason,
                        detail=detail,
                    )
                except Exception:
                    pass

            def _run() -> Dict[str, Any]:
                if not isinstance(connector, dict):
                    return handle_mcp_request(item)
                with runtime_context_override(
                    home=str(ctx.home),
                    group_id=str(connector.get("group_id") or ""),
                    actor_id=str(connector.get("actor_id") or ""),
                ):
                    return handle_mcp_request(item)

            await _record_browser_progress("mcp_tool_start", requested_tool_name)
            resp = await run_in_threadpool(_run)
            if isinstance(connector, dict):
                connector_id = str(connector.get("connector_id") or "").strip()
                if connector_id:
                    activity = _tool_call_activity(item, resp if isinstance(resp, dict) else {})
                    await run_in_threadpool(
                        record_web_model_connector_activity,
                        connector_id,
                        method=activity["method"],
                        tool_name=activity["tool_name"],
                        call_status=activity["call_status"],
                        wait_status=activity["wait_status"],
                        turn_id=activity["turn_id"],
                        error=activity["error"],
                    )
                    await _record_browser_progress("mcp_tool", activity["tool_name"])
            return resp

        if isinstance(payload, list):
            responses = []
            for item in payload:
                resp = await _handle_one(item)
                if resp:
                    responses.append(resp)
            if not responses:
                return Response(status_code=202)
            return JSONResponse(content=responses)
        if not isinstance(payload, dict):
            return JSONResponse(content=_mcp_jsonrpc_error(None, -32600, "Invalid Request"), status_code=400)
        response = await _handle_one(payload)
        if not response:
            return Response(status_code=202)
        return JSONResponse(content=response)

    @global_router.post("/api/v1/mcp", dependencies=[Depends(require_admin)])
    async def remote_mcp_jsonrpc(request: Request) -> Response:
        """Experimental remote MCP JSON-RPC endpoint for website-model connectors."""
        try:
            payload = await request.json()
        except Exception as exc:
            raise HTTPException(status_code=400, detail={"code": "invalid_json", "message": "invalid JSON body"}) from exc
        return await _handle_remote_mcp_payload(payload)

    @global_router.get("/api/v1/web-model/connectors", dependencies=[Depends(require_admin)])
    async def web_model_connectors_list(request: Request) -> Dict[str, Any]:
        connectors = []
        items = list(load_web_model_connectors().values())
        items.sort(key=lambda item: (str(item.get("created_at") or ""), str(item.get("connector_id") or "")), reverse=True)
        for item in items:
            connectors.append(_web_model_connector_web_payload(request, item))
        return {"ok": True, "result": {"connectors": connectors}}

    @global_router.post("/api/v1/web-model/connectors", dependencies=[Depends(require_admin)])
    async def web_model_connectors_create(req: WebModelConnectorCreateRequest, request: Request) -> Dict[str, Any]:
        group_id = str(req.group_id or "").strip()
        actor_id = str(req.actor_id or "").strip()
        if not group_id:
            raise HTTPException(status_code=400, detail={"code": "missing_group_id", "message": "missing group_id", "details": {}})
        if not actor_id:
            raise HTTPException(status_code=400, detail={"code": "missing_actor_id", "message": "missing actor_id", "details": {}})
        group = load_group(group_id)
        if group is None:
            raise HTTPException(status_code=404, detail={"code": "group_not_found", "message": f"group not found: {group_id}", "details": {}})
        actor = find_actor(group, actor_id)
        if not isinstance(actor, dict):
            raise HTTPException(status_code=404, detail={"code": "actor_not_found", "message": f"actor not found: {actor_id}", "details": {}})
        if str(actor.get("runtime") or "").strip().lower() != "web_model":
            raise HTTPException(
                status_code=400,
                detail={
                    "code": "invalid_actor_runtime",
                    "message": "web-model connectors can only be bound to actors using runtime=web_model",
                    "details": {"group_id": group_id, "actor_id": actor_id},
                },
            )
        try:
            require_no_other_chatgpt_web_model_actor(group_id=group_id, actor_id=actor_id)
        except ValueError as exc:
            raise HTTPException(
                status_code=400,
                detail={"code": "chatgpt_web_model_singleton", "message": str(exc), "details": {"group_id": group_id, "actor_id": actor_id}},
            ) from exc
        try:
            connector = create_web_model_connector(
                group_id=group_id,
                actor_id=actor_id,
                provider=str(req.provider or "").strip(),
                label=str(req.label or "").strip(),
            )
        except ValueError as exc:
            raise HTTPException(status_code=400, detail={"code": "invalid_request", "message": str(exc), "details": {}}) from exc
        connector_id = str(connector.get("connector_id") or "")
        safe_connector = _web_model_connector_web_payload(request, connector)
        replaced = connector.get("replaced_connector_ids") if isinstance(connector.get("replaced_connector_ids"), list) else []
        return {
            "ok": True,
            "result": {
                "connector": safe_connector,
                "secret": str(connector.get("secret") or ""),
                "replaced_connector_ids": [str(item or "").strip() for item in replaced if str(item or "").strip()],
            },
        }

    @global_router.delete("/api/v1/web-model/connectors/{connector_id}", dependencies=[Depends(require_admin)])
    async def web_model_connectors_revoke(connector_id: str) -> Dict[str, Any]:
        if not revoke_web_model_connector(connector_id):
            raise HTTPException(status_code=404, detail={"code": "not_found", "message": "web-model connector not found", "details": {}})
        return {"ok": True, "result": {"revoked": True, "connector_id": str(connector_id or "").strip()}}

    async def _web_model_browser_payload(group_id: str, actor_id: str, browser_surface: Dict[str, Any]) -> Dict[str, Any]:
        from ....daemon.actors.web_model_browser_session import get_web_model_chatgpt_browser_session_state
        from ...web_model_browser_sidecar import chatgpt_browser_session_status

        surface = browser_surface or await run_in_threadpool(
            get_web_model_chatgpt_browser_session_state,
            group_id=group_id,
            actor_id=actor_id,
        )
        browser = await run_in_threadpool(chatgpt_browser_session_status, group_id, actor_id)
        return {"ok": True, "result": {"browser_session": browser, "browser_surface": surface}}

    @global_router.get("/api/v1/web-model/browser-session", dependencies=[Depends(require_admin)])
    async def web_model_browser_session_status(group_id: str, actor_id: str) -> Dict[str, Any]:
        _require_web_model_browser_actor(group_id, actor_id, allow_global_setup=True)
        return await _web_model_browser_payload(group_id, actor_id, {})

    @global_router.post("/api/v1/web-model/browser-session/open", dependencies=[Depends(require_admin)])
    async def web_model_browser_session_open(req: WebModelBrowserSessionRequest) -> Dict[str, Any]:
        group_id = str(req.group_id or "").strip()
        actor_id = str(req.actor_id or "").strip()
        _require_web_model_browser_actor(group_id, actor_id, allow_global_setup=True)
        from ....daemon.actors.web_model_browser_session import open_web_model_chatgpt_browser_session

        try:
            surface = await run_in_threadpool(
                open_web_model_chatgpt_browser_session,
                group_id=group_id,
                actor_id=actor_id,
                width=int(req.width or 1366),
                height=int(req.height or 900),
            )
        except Exception as exc:
            raise HTTPException(
                status_code=500,
                detail={"code": "web_model_browser_open_failed", "message": str(exc), "details": {}},
            ) from exc
        return await _web_model_browser_payload(group_id, actor_id, surface)

    @global_router.post("/api/v1/web-model/browser-session/close", dependencies=[Depends(require_admin)])
    async def web_model_browser_session_close(req: WebModelBrowserSessionRequest) -> Dict[str, Any]:
        group_id = str(req.group_id or "").strip()
        actor_id = str(req.actor_id or "").strip()
        _require_web_model_browser_actor(group_id, actor_id, allow_global_setup=True)
        from ....daemon.actors.web_model_browser_session import close_web_model_chatgpt_browser_session

        result = await run_in_threadpool(
            close_web_model_chatgpt_browser_session,
            group_id=group_id,
            actor_id=actor_id,
        )
        surface = result.get("browser_surface") if isinstance(result, dict) else {}
        return await _web_model_browser_payload(group_id, actor_id, surface if isinstance(surface, dict) else {})

    @global_router.post("/api/v1/web-model/browser-session/bind-current", dependencies=[Depends(require_admin)])
    async def web_model_browser_session_bind_current(req: WebModelBrowserBindRequest) -> Dict[str, Any]:
        group_id = str(req.group_id or "").strip()
        actor_id = str(req.actor_id or "").strip()
        _require_web_model_browser_actor(group_id, actor_id)
        from ...web_model_browser_sidecar import (
            CHATGPT_URL,
            _conversation_url_from_tab,
            _normalize_chatgpt_url,
            chatgpt_browser_session_status,
            record_chatgpt_browser_state,
        )

        if bool(req.clear):
            await run_in_threadpool(
                record_chatgpt_browser_state,
                group_id,
                actor_id,
                {
                    "conversation_url": "",
                    "pending_new_chat_bind": False,
                    "pending_new_chat_url": "",
                    "pending_new_chat_bind_started_at": "",
                    "pending_new_chat_submitted": False,
                    "pending_new_chat_submitted_at": "",
                    "pending_new_chat_delivery_id": "",
                    "pending_new_chat_last_turn_id": "",
                    "pending_new_chat_last_event_ids": [],
                    "pending_new_chat_last_tab_url": "",
                    "new_chat_bound_at": "",
                    "bootstrap_seed_delivered_at": "",
                    "bootstrap_seed_version": "",
                    "bootstrap_seed_digest": "",
                    "bootstrap_seed_conversation_url": "",
                    "last_error": "",
                },
            )
            return await _web_model_browser_payload(group_id, actor_id, {})

        browser = await run_in_threadpool(chatgpt_browser_session_status, group_id, actor_id)
        raw_url = str(req.conversation_url or browser.get("tab_url") or browser.get("last_tab_url") or "").strip()
        conversation_url = _conversation_url_from_tab(raw_url)
        pending_url = _normalize_chatgpt_url(raw_url) if raw_url else ""
        if bool(req.new_chat):
            conversation_url = ""
            pending_url = CHATGPT_URL
        if not conversation_url and not pending_url:
            raise HTTPException(
                status_code=400,
                detail={"code": "chatgpt_tab_not_found", "message": "open ChatGPT or paste a ChatGPT conversation URL before binding", "details": {}},
            )
        pending_new_chat = not bool(conversation_url)
        await run_in_threadpool(
            record_chatgpt_browser_state,
            group_id,
            actor_id,
            {
                "conversation_url": conversation_url,
                "pending_new_chat_bind": pending_new_chat,
                "pending_new_chat_url": pending_url if pending_new_chat else "",
                "pending_new_chat_bind_started_at": utc_now_iso() if pending_new_chat else "",
                "pending_new_chat_submitted": False,
                "pending_new_chat_submitted_at": "",
                "pending_new_chat_delivery_id": "",
                "pending_new_chat_last_turn_id": "",
                "pending_new_chat_last_event_ids": [],
                "pending_new_chat_last_tab_url": "",
                "new_chat_bound_at": "",
                "bootstrap_seed_delivered_at": "",
                "bootstrap_seed_version": "",
                "bootstrap_seed_digest": "",
                "bootstrap_seed_conversation_url": "",
                "last_error": "",
            },
        )
        return await _web_model_browser_payload(group_id, actor_id, {})

    @global_router.websocket("/api/v1/web-model/browser-session/ws")
    async def web_model_browser_session_ws(websocket: WebSocket, group_id: str, actor_id: str) -> None:
        await websocket.accept()

        principal = resolve_websocket_principal(websocket)
        websocket.state.principal = principal

        auth_header = str((getattr(websocket, "headers", {}) or {}).get("authorization") or "").strip()
        has_header_token = auth_header.lower().startswith("bearer ") and bool(str(auth_header[7:] or "").strip())
        has_cookie_token = False
        try:
            cookies = getattr(websocket, "cookies", None) or {}
            has_cookie_token = bool(str(cookies.get("cccc_access_token") or "").strip())
        except Exception:
            has_cookie_token = False
        has_query_token = bool(str(websocket.query_params.get("token") or "").strip())
        if (has_header_token or has_cookie_token or has_query_token) and str(getattr(principal, "kind", "anonymous") or "anonymous") != "user" and websocket_tokens_active():
            try:
                await websocket.send_json({"ok": False, "error": {"code": "auth_required", "message": "Invalid or missing authentication token"}})
            except Exception:
                pass
            await websocket.close(code=4401)
            return

        try:
            check_admin(websocket)
            _require_web_model_browser_actor(group_id, actor_id, allow_global_setup=True)
        except HTTPException as exc:
            detail = exc.detail if isinstance(exc.detail, dict) else {"code": "permission_denied", "message": str(exc.detail or "permission denied")}
            try:
                await websocket.send_json({"ok": False, "error": detail})
            except Exception:
                pass
            await websocket.close(code=1008)
            return

        if ctx.read_only:
            try:
                await websocket.send_json(
                    {
                        "ok": False,
                        "error": {
                            "code": "read_only_browser_surface",
                            "message": "Web-model browser surface is disabled in read-only mode.",
                            "details": {},
                        },
                    }
                )
            except Exception:
                pass
            await websocket.close(code=1000)
            return

        from ....daemon.actors.web_model_browser_session import (
            attach_web_model_chatgpt_browser_socket,
            can_attach_web_model_chatgpt_browser_socket,
        )

        ok, info = can_attach_web_model_chatgpt_browser_socket(group_id=group_id, actor_id=actor_id)
        if not ok:
            try:
                await websocket.send_json({"ok": False, "error": info})
            except Exception:
                pass
            await websocket.close(code=1008)
            return

        try:
            manager_sock, web_sock = socket.socketpair()
        except Exception:
            await websocket.send_json({"ok": False, "error": {"code": "browser_surface_attach_failed", "message": "socketpair unavailable"}})
            await websocket.close(code=1011)
            return

        attached = False
        web_sock_attached_to_asyncio = False
        try:
            attached = attach_web_model_chatgpt_browser_socket(group_id=group_id, actor_id=actor_id, sock=manager_sock)
            if not attached:
                try:
                    manager_sock.close()
                except Exception:
                    pass
                await websocket.send_json({"ok": False, "error": {"code": "browser_surface_attach_failed", "message": "browser surface attach failed"}})
                await websocket.close(code=1008)
                return

            reader, writer = await asyncio.open_connection(sock=web_sock, limit=_WEB_MODEL_BROWSER_STREAM_LIMIT_BYTES)
            web_sock_attached_to_asyncio = True

            async def _pump_out() -> None:
                while True:
                    line = await reader.readline()
                    if not line:
                        break
                    await websocket.send_text(line.decode("utf-8", errors="replace").rstrip("\n"))

            async def _pump_in() -> None:
                while True:
                    raw = await websocket.receive_text()
                    if not raw:
                        continue
                    writer.write((raw + "\n").encode("utf-8", errors="replace"))
                    await writer.drain()

            out_task = asyncio.create_task(_pump_out())
            in_task = asyncio.create_task(_pump_in())
            try:
                done, pending = await asyncio.wait({out_task, in_task}, return_when=asyncio.FIRST_COMPLETED)
                for task in done:
                    try:
                        _ = task.result()
                    except Exception:
                        pass
                for task in pending:
                    task.cancel()
                await asyncio.gather(*pending, return_exceptions=True)
            finally:
                try:
                    writer.close()
                    await writer.wait_closed()
                except Exception:
                    pass
                try:
                    await websocket.close(code=1000)
                except Exception:
                    pass
        finally:
            if not web_sock_attached_to_asyncio:
                try:
                    web_sock.close()
                except Exception:
                    pass
            if not attached:
                try:
                    manager_sock.close()
                except Exception:
                    pass

    @global_router.post("/mcp/web-model/{connector_id}")
    async def web_model_mcp_jsonrpc(connector_id: str, request: Request) -> Response:
        """Stable connector URL shape for the browser web-model runtime RFC."""
        connector = _resolve_web_model_connector(request, connector_id)
        try:
            payload = await request.json()
        except Exception as exc:
            raise HTTPException(status_code=400, detail={"code": "invalid_json", "message": "invalid JSON body"}) from exc
        return await _handle_remote_mcp_payload(payload, connector=connector)

    @global_router.get("/mcp/web-model/{connector_id}")
    async def web_model_mcp_sse_probe(connector_id: str, request: Request) -> StreamingResponse:
        """Streamable-HTTP probe path for remote MCP clients that open GET/SSE."""
        connector = _resolve_web_model_connector(request, connector_id)
        connector_id_clean = str(connector.get("connector_id") or "").strip()
        if connector_id_clean:
            await run_in_threadpool(
                record_web_model_connector_activity,
                connector_id_clean,
                method="GET",
                call_status="ok",
            )

        async def _events():
            yield b": cccc web-model connector ready\n\n"

        return StreamingResponse(
            _events(),
            media_type="text/event-stream",
            headers={"Cache-Control": "no-cache", "X-Accel-Buffering": "no"},
        )

    @global_router.options("/mcp/web-model/{connector_id}")
    async def web_model_mcp_options(connector_id: str) -> Response:
        return Response(
            status_code=204,
            headers={
                "Allow": "GET, POST, OPTIONS",
                "Access-Control-Allow-Methods": "GET, POST, OPTIONS",
                "Access-Control-Allow-Headers": "authorization, content-type, accept, mcp-protocol-version, mcp-session-id",
            },
        )

    @global_router.get("/api/v1/web_access/session")
    async def web_access_session(request: Request) -> Dict[str, Any]:
        principal = get_principal(request)
        allowed_groups = getattr(principal, "allowed_groups", ()) or ()
        groups = [str(item or "").strip() for item in allowed_groups if str(item or "").strip()]
        access_tokens = list_access_tokens()
        access_token_count = len(access_tokens)
        login_active = access_token_count > 0
        principal_kind = str(getattr(principal, "kind", "anonymous") or "anonymous")
        is_admin = bool(getattr(principal, "is_admin", False))
        can_access_global_settings = access_token_count == 0 or (principal_kind == "user" and is_admin)
        observability = get_observability_settings()
        runtime_visibility = observability.get("runtime_visibility") if isinstance(observability, dict) else {}
        return {
            "ok": True,
            "result": {
                "web_access_session": {
                    "login_active": login_active,
                    "current_browser_signed_in": principal_kind == "user",
                    "principal_kind": principal_kind,
                    "user_id": str(getattr(principal, "user_id", "") or ""),
                    "is_admin": is_admin,
                    "allowed_groups": groups,
                    "access_token_count": access_token_count,
                    "can_access_global_settings": can_access_global_settings,
                    "runtime_visibility": {
                        "peer_runtime": str(
                            (runtime_visibility or {}).get("peer_runtime") or "visible"
                        ).strip().lower()
                        or "visible",
                        "pet_runtime": str(
                            (runtime_visibility or {}).get("pet_runtime") or "hidden"
                        ).strip().lower()
                        or "hidden",
                    },
                }
            },
        }

    @global_router.post("/api/v1/web_access/logout")
    async def web_access_logout(request: Request) -> JSONResponse:
        request.state.skip_token_cookie_refresh = True
        resp = JSONResponse({"ok": True, "result": {"signed_out": True}})
        resp.delete_cookie(key="cccc_access_token", path="/")
        host = str(getattr(getattr(request, "url", None), "hostname", "") or "").strip().lower()
        if host:
            resp.delete_cookie(key="cccc_access_token", path="/", domain=host)
        resp.set_cookie(key="cccc_signed_out", value="1", httponly=True, samesite="lax", path="/", max_age=300)
        return resp

    # debug/snapshot uses manual check_group (group_id from query param)
    @global_router.get("/api/v1/debug/snapshot")
    async def debug_snapshot(request: Request, group_id: str) -> Dict[str, Any]:
        """Get a structured debug snapshot for a group (developer mode only)."""
        check_group(request, group_id)
        return await ctx.daemon({"op": "debug_snapshot", "args": {"group_id": group_id, "by": "user"}})

    @global_router.get("/api/v1/observability", dependencies=[Depends(require_admin)])
    async def observability_get() -> Dict[str, Any]:
        """Get global observability settings (developer mode, log level)."""
        return await ctx.daemon({"op": "observability_get"})

    @global_router.put("/api/v1/branding", dependencies=[Depends(require_admin)])
    async def branding_update(req: BrandingUpdateRequest) -> Dict[str, Any]:
        if ctx.read_only:
            raise HTTPException(
                status_code=403,
                detail={
                    "code": "read_only",
                    "message": "Branding write endpoints are disabled in read-only (exhibit) mode.",
                },
            )
        before = get_web_branding_settings()
        patch: Dict[str, Any] = {}
        if req.product_name is not None:
            patch["product_name"] = str(req.product_name or "").strip()
        if bool(req.clear_logo_icon):
            patch["logo_icon_asset_path"] = ""
        if bool(req.clear_favicon):
            patch["favicon_asset_path"] = ""
        resp = await ctx.daemon({"op": "branding_update", "args": {"by": str(req.by or "user"), "patch": patch}})
        if not resp.get("ok"):
            return resp
        raw = (resp.get("result") or {}).get("branding") if isinstance(resp.get("result"), dict) else {}
        if bool(req.clear_logo_icon):
            old_logo = str(before.get("logo_icon_asset_path") or "").strip()
            if old_logo:
                delete_branding_asset(old_logo)
        if bool(req.clear_favicon):
            old_favicon = str(before.get("favicon_asset_path") or "").strip()
            if old_favicon:
                delete_branding_asset(old_favicon)
        return {"ok": True, "result": {"branding": build_branding_payload(raw if isinstance(raw, dict) else {})}}

    @global_router.post("/api/v1/branding/assets/{asset_kind}", dependencies=[Depends(require_admin)])
    async def branding_asset_upload(
        asset_kind: str,
        by: str = Form("user"),
        file: UploadFile = File(...),
    ) -> Dict[str, Any]:
        if ctx.read_only:
            raise HTTPException(
                status_code=403,
                detail={
                    "code": "read_only",
                    "message": "Branding write endpoints are disabled in read-only (exhibit) mode.",
                },
            )
        try:
            normalized_kind = normalize_branding_asset_kind(asset_kind)
        except ValueError as exc:
            raise HTTPException(status_code=400, detail={"code": "invalid_request", "message": str(exc)}) from exc

        raw_bytes = await file.read()
        try:
            stored = store_branding_asset(
                asset_kind=normalized_kind,
                data=raw_bytes,
                content_type=str(getattr(file, "content_type", "") or ""),
                filename=str(getattr(file, "filename", "") or ""),
            )
        except ValueError as exc:
            message = str(exc)
            status_code = 413 if "too large" in message else 400
            raise HTTPException(status_code=status_code, detail={"code": "invalid_request", "message": message}) from exc

        before = get_web_branding_settings()
        key = f"{normalized_kind}_asset_path"
        old_rel_path = str(before.get(key) or "").strip()
        resp = await ctx.daemon({"op": "branding_update", "args": {"by": str(by or "user"), "patch": {key: stored["rel_path"]}}})
        if not resp.get("ok"):
            delete_branding_asset(str(stored.get("rel_path") or ""))
            return resp
        if old_rel_path and old_rel_path != str(stored.get("rel_path") or ""):
            delete_branding_asset(old_rel_path)
        raw = (resp.get("result") or {}).get("branding") if isinstance(resp.get("result"), dict) else {}
        return {"ok": True, "result": {"branding": build_branding_payload(raw if isinstance(raw, dict) else {})}}

    @global_router.delete("/api/v1/branding/assets/{asset_kind}", dependencies=[Depends(require_admin)])
    async def branding_asset_clear(asset_kind: str, by: str = "user") -> Dict[str, Any]:
        if ctx.read_only:
            raise HTTPException(
                status_code=403,
                detail={
                    "code": "read_only",
                    "message": "Branding write endpoints are disabled in read-only (exhibit) mode.",
                },
            )
        try:
            normalized_kind = normalize_branding_asset_kind(asset_kind)
        except ValueError as exc:
            raise HTTPException(status_code=400, detail={"code": "invalid_request", "message": str(exc)}) from exc
        before = get_web_branding_settings()
        key = f"{normalized_kind}_asset_path"
        old_rel_path = str(before.get(key) or "").strip()
        resp = await ctx.daemon({"op": "branding_update", "args": {"by": str(by or "user"), "patch": {key: ""}}})
        if not resp.get("ok"):
            return resp
        if old_rel_path:
            delete_branding_asset(old_rel_path)
        raw = (resp.get("result") or {}).get("branding") if isinstance(resp.get("result"), dict) else {}
        return {"ok": True, "result": {"branding": build_branding_payload(raw if isinstance(raw, dict) else {})}}

    @global_router.get("/api/v1/capabilities/allowlist", dependencies=[Depends(require_admin)])
    async def capability_allowlist_get(by: str = "user") -> Dict[str, Any]:
        """Get effective capability allowlist (default + overlay + merge result)."""
        return await ctx.daemon({"op": "capability_allowlist_get", "args": {"by": str(by or "user")}})

    @global_router.post("/api/v1/capabilities/allowlist/validate", dependencies=[Depends(require_admin)])
    async def capability_allowlist_validate(request: Request) -> Dict[str, Any]:
        """Validate a capability allowlist overlay patch/replace request without persisting."""
        try:
            payload = await request.json()
        except Exception:
            payload = {}
        if not isinstance(payload, dict):
            raise HTTPException(status_code=400, detail={"code": "invalid_request", "message": "request body must be an object"})
        args: Dict[str, Any] = {"mode": str(payload.get("mode") or "patch").strip().lower() or "patch"}
        if "patch" in payload:
            args["patch"] = payload.get("patch")
        if "overlay" in payload:
            args["overlay"] = payload.get("overlay")
        return await ctx.daemon({"op": "capability_allowlist_validate", "args": args})

    @global_router.put("/api/v1/capabilities/allowlist", dependencies=[Depends(require_admin)])
    async def capability_allowlist_update(request: Request) -> Dict[str, Any]:
        """Update capability allowlist user overlay."""
        if ctx.read_only:
            raise HTTPException(
                status_code=403,
                detail={
                    "code": "read_only",
                    "message": "Capability allowlist write endpoints are disabled in read-only (exhibit) mode.",
                },
            )
        try:
            payload = await request.json()
        except Exception:
            payload = {}
        if not isinstance(payload, dict):
            raise HTTPException(status_code=400, detail={"code": "invalid_request", "message": "request body must be an object"})
        args: Dict[str, Any] = {
            "by": str(payload.get("by") or "user").strip() or "user",
            "mode": str(payload.get("mode") or "patch").strip().lower() or "patch",
        }
        if "expected_revision" in payload:
            args["expected_revision"] = str(payload.get("expected_revision") or "").strip()
        if "patch" in payload:
            args["patch"] = payload.get("patch")
        if "overlay" in payload:
            args["overlay"] = payload.get("overlay")
        return await ctx.daemon({"op": "capability_allowlist_update", "args": args})

    @global_router.delete("/api/v1/capabilities/allowlist", dependencies=[Depends(require_admin)])
    async def capability_allowlist_reset(by: str = "user") -> Dict[str, Any]:
        """Reset capability allowlist overlay to empty."""
        if ctx.read_only:
            raise HTTPException(
                status_code=403,
                detail={
                    "code": "read_only",
                    "message": "Capability allowlist write endpoints are disabled in read-only (exhibit) mode.",
                },
            )
        return await ctx.daemon({"op": "capability_allowlist_reset", "args": {"by": str(by or "user")}})

    @global_router.get("/api/v1/capabilities/overview", dependencies=[Depends(require_user)])
    async def capability_overview(
        query: str = "",
        limit: int = 400,
        offset: int = 0,
        include_indexed: bool = True,
        kind: str = "",
        policy: str = "",
        source_id: str = "",
    ) -> Dict[str, Any]:
        """Get global capability overview (policy + blocked + recent-success + source states)."""
        args = {
            "query": str(query or ""),
            "limit": int(limit or 400),
            "offset": max(0, int(offset or 0)),
            "include_indexed": bool(include_indexed),
            "kind": str(kind or ""),
            "policy": str(policy or ""),
            "source_id": str(source_id or ""),
        }
        return await ctx.daemon({"op": "capability_overview", "args": args})

    @global_router.post("/api/v1/capabilities/block", dependencies=[Depends(require_admin)])
    async def capability_block_global(request: Request) -> Dict[str, Any]:
        """Global block/unblock capability (user only in Web)."""
        if ctx.read_only:
            raise HTTPException(
                status_code=403,
                detail={
                    "code": "read_only",
                    "message": "Capability block write endpoints are disabled in read-only (exhibit) mode.",
                },
            )
        try:
            payload = await request.json()
        except Exception:
            payload = {}
        if not isinstance(payload, dict):
            raise HTTPException(status_code=400, detail={"code": "invalid_request", "message": "request body must be an object"})
        capability_id = str(payload.get("capability_id") or "").strip()
        group_id = str(payload.get("group_id") or "").strip()
        if not capability_id:
            raise HTTPException(status_code=400, detail={"code": "missing_capability_id", "message": "missing capability_id"})
        if not group_id:
            raise HTTPException(status_code=400, detail={"code": "missing_group_id", "message": "missing group_id"})
        args = {
            "group_id": group_id,
            "by": str(payload.get("by") or "user").strip() or "user",
            "actor_id": str(payload.get("actor_id") or payload.get("by") or "user").strip() or "user",
            "capability_id": capability_id,
            "scope": "global",
            "blocked": bool(payload.get("blocked", True)),
            "reason": str(payload.get("reason") or "").strip(),
            "ttl_seconds": int(payload.get("ttl_seconds") or 0),
        }
        return await ctx.daemon({"op": "capability_block", "args": args})

    @global_router.put("/api/v1/observability", dependencies=[Depends(require_admin)])
    async def observability_update(req: ObservabilityUpdateRequest) -> Dict[str, Any]:
        """Update global observability settings (daemon-owned persistence)."""
        patch: Dict[str, Any] = {}
        if req.developer_mode is not None:
            patch["developer_mode"] = bool(req.developer_mode)
        if req.log_level is not None:
            patch["log_level"] = str(req.log_level or "").strip().upper()
        if req.logger_levels is not None:
            patch["logger_levels"] = {
                str(name): str(level).strip().upper()
                for name, level in req.logger_levels.items()
                if str(name).strip() and str(level).strip()
            }
        if req.terminal_transcript_per_actor_bytes is not None:
            patch.setdefault("terminal_transcript", {})["per_actor_bytes"] = int(req.terminal_transcript_per_actor_bytes)
        if req.terminal_ui_scrollback_lines is not None:
            patch.setdefault("terminal_ui", {})["scrollback_lines"] = int(req.terminal_ui_scrollback_lines)
        if req.peer_runtime_visibility is not None:
            patch.setdefault("runtime_visibility", {})["peer_runtime"] = str(req.peer_runtime_visibility)
        if req.pet_runtime_visibility is not None:
            patch.setdefault("runtime_visibility", {})["pet_runtime"] = str(req.pet_runtime_visibility)

        resp = await ctx.daemon({"op": "observability_update", "args": {"by": req.by, "patch": patch}})

        # Apply web-side logging immediately as well (best-effort).
        try:
            obs = (resp.get("result") or {}).get("observability") if resp.get("ok") else None
            if isinstance(obs, dict):
                requested_level = str(obs.get("log_level") or "INFO").strip().upper() or "INFO"
                effective_level = "DEBUG" if obs.get("developer_mode") and requested_level == "INFO" else requested_level
                level = "INFO" if effective_level == "DEBUG" else effective_level
                logger_levels = {
                    str(name): str(value)
                    for name, value in obs.get("logger_levels", {}).items()
                } if isinstance(obs.get("logger_levels"), dict) else {}
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
                ctx.apply_web_logging(home=ctx.home, level=level, logger_levels=logger_levels)
        except Exception:
            pass

        return resp

    @global_router.get("/api/v1/remote_access", dependencies=[Depends(require_admin)])
    async def remote_access_get() -> Dict[str, Any]:
        """Get global remote-access state."""
        return await ctx.daemon({"op": "remote_access_state", "args": {"by": "user"}})

    @global_router.put("/api/v1/remote_access", dependencies=[Depends(require_admin)])
    async def remote_access_configure(req: RemoteAccessConfigureRequest) -> Dict[str, Any]:
        """Update global remote-access config."""
        args: Dict[str, Any] = {"by": str(req.by or "user")}
        if req.provider is not None:
            args["provider"] = str(req.provider)
        if req.mode is not None:
            args["mode"] = str(req.mode or "").strip()
        if req.enabled is not None:
            args["enabled"] = bool(req.enabled)
        if req.require_access_token is not None:
            args["require_access_token"] = bool(req.require_access_token)
        if req.web_host is not None:
            args["web_host"] = str(req.web_host or "").strip()
        if req.web_port is not None:
            args["web_port"] = int(req.web_port)
        if req.web_public_url is not None:
            args["web_public_url"] = str(req.web_public_url or "").strip()
        return await ctx.daemon({"op": "remote_access_configure", "args": args})

    @global_router.post("/api/v1/remote_access/start", dependencies=[Depends(require_admin)])
    async def remote_access_start(by: str = "user") -> Dict[str, Any]:
        """Start remote access service."""
        return await ctx.daemon({"op": "remote_access_start", "args": {"by": str(by or "user")}})

    @global_router.post("/api/v1/remote_access/stop", dependencies=[Depends(require_admin)])
    async def remote_access_stop(by: str = "user") -> Dict[str, Any]:
        """Stop remote access service."""
        return await ctx.daemon({"op": "remote_access_stop", "args": {"by": str(by or "user")}})

    @global_router.post("/api/v1/remote_access/apply", dependencies=[Depends(require_admin)])
    async def remote_access_apply(
        request: Request,
        background_tasks: BackgroundTasks,
        by: str = "user",
    ) -> Dict[str, Any]:
        """Apply saved Web binding changes by restarting the supervised Web child."""
        state_resp = await ctx.daemon({"op": "remote_access_state", "args": {"by": str(by or "user")}})
        remote = (state_resp.get("result") or {}).get("remote_access") if state_resp.get("ok") else None
        if not isinstance(remote, dict):
            raise HTTPException(status_code=503, detail={"code": "remote_access_unavailable", "message": "remote access state unavailable"})
        diagnostics = remote.get("diagnostics") if isinstance(remote.get("diagnostics"), dict) else {}
        if not bool(remote.get("restart_required")):
            return {"ok": True, "result": {"accepted": False, "remote_access": remote}}
        if not bool(remote.get("apply_supported")):
            raise HTTPException(
                status_code=409,
                detail={
                    "code": "web_apply_unsupported",
                    "message": "the running Web service is not supervisor-managed, so it cannot self-apply binding changes",
                },
            )
        restart_cb = getattr(request.app.state, "request_web_restart", None)
        if not callable(restart_cb):
            raise HTTPException(
                status_code=409,
                detail={"code": "web_apply_unavailable", "message": "web apply is not available in this runtime"},
            )
        background_tasks.add_task(restart_cb)
        return {
            "ok": True,
            "result": {
                "accepted": True,
                "remote_access": remote,
                "target_local_url": diagnostics.get("desired_local_url"),
                "target_remote_url": diagnostics.get("desired_remote_url"),
            },
        }

    @global_router.get("/api/v1/registry/reconcile", dependencies=[Depends(require_admin)])
    async def registry_reconcile_preview() -> Dict[str, Any]:
        """Preview registry health (missing/corrupt groups) without mutating registry."""
        return await ctx.daemon({"op": "registry_reconcile", "args": {"remove_missing": False, "by": "user"}})

    @global_router.post("/api/v1/registry/reconcile", dependencies=[Depends(require_admin)])
    async def registry_reconcile(req: RegistryReconcileRequest) -> Dict[str, Any]:
        """Explicitly reconcile registry (currently removes only missing entries)."""
        return await ctx.daemon(
            {
                "op": "registry_reconcile",
                "args": {
                    "remove_missing": bool(req.remove_missing),
                    "by": str(req.by or "user"),
                },
            }
        )

    @global_router.get("/api/v1/debug/tail_logs", dependencies=[Depends(require_admin)])
    async def debug_tail_logs(component: str, group_id: str = "", lines: int = 200) -> Dict[str, Any]:
        """Tail local CCCC logs (developer mode only)."""
        return await ctx.daemon(
            {
                "op": "debug_tail_logs",
                "args": {
                    "component": str(component or ""),
                    "group_id": str(group_id or ""),
                    "lines": int(lines or 200),
                    "by": "user",
                },
            }
        )

    @global_router.post("/api/v1/debug/clear_logs", dependencies=[Depends(require_admin)])
    async def debug_clear_logs(req: DebugClearLogsRequest) -> Dict[str, Any]:
        """Clear (truncate) local CCCC logs (developer mode only)."""
        return await ctx.daemon(
            {
                "op": "debug_clear_logs",
                "args": {
                    "component": str(req.component or ""),
                    "group_id": str(req.group_id or ""),
                    "by": str(req.by or "user"),
                },
            }
        )

    @global_router.get("/api/v1/runtimes", dependencies=[Depends(require_user)])
    async def runtimes() -> Dict[str, Any]:
        """List available agent runtimes on the system."""
        if ctx.read_only:
            raise HTTPException(
                status_code=403,
                detail={
                    "code": "read_only",
                    "message": "System discovery endpoints are disabled in read-only (exhibit) mode.",
                    "details": {"endpoint": "runtimes"},
                },
            )
        from ....kernel.runtime import detect_all_runtimes, get_runtime_command_with_flags

        all_runtimes = detect_all_runtimes(primary_only=False)
        return {
            "ok": True,
            "result": {
                "runtimes": [
                    {
                        "name": rt.name,
                        "display_name": rt.display_name,
                        "recommended_command": " ".join(get_runtime_command_with_flags(rt.name)),
                        "available": rt.available,
                    }
                    for rt in all_runtimes
                ],
                "available": [rt.name for rt in all_runtimes if rt.available],
            },
        }

    @global_router.get("/api/v1/fs/list", dependencies=[Depends(require_admin)])
    async def fs_list(path: str = "~", show_hidden: bool = False) -> Dict[str, Any]:
        """List directory contents for path picker UI."""
        if ctx.read_only:
            raise HTTPException(
                status_code=403,
                detail={
                    "code": "read_only",
                    "message": "File system endpoints are disabled in read-only (exhibit) mode.",
                    "details": {"endpoint": "fs_list"},
                },
            )
        try:
            target = Path(path).expanduser().resolve()
            if not target.exists():
                return {"ok": False, "error": {"code": "NOT_FOUND", "message": f"Path not found: {path}"}}
            if not target.is_dir():
                return {"ok": False, "error": {"code": "NOT_DIR", "message": f"Not a directory: {path}"}}

            items = []
            try:
                for entry in sorted(target.iterdir(), key=lambda x: (not x.is_dir(), x.name.lower())):
                    if not show_hidden and entry.name.startswith("."):
                        continue
                    items.append({
                        "name": entry.name,
                        "path": str(entry),
                        "is_dir": entry.is_dir(),
                    })
            except PermissionError:
                return {"ok": False, "error": {"code": "PERMISSION_DENIED", "message": f"Permission denied: {path}"}}

            return {
                "ok": True,
                "result": {
                    "path": str(target),
                    "parent": str(target.parent) if target.parent != target else None,
                    "items": items[:100],  # Limit to 100 items
                },
            }
        except Exception as e:
            return {"ok": False, "error": {"code": "ERROR", "message": str(e)}}

    @global_router.get("/api/v1/fs/recent", dependencies=[Depends(require_admin)])
    async def fs_recent() -> Dict[str, Any]:
        """Get recent/common directories for quick selection."""
        if ctx.read_only:
            raise HTTPException(
                status_code=403,
                detail={
                    "code": "read_only",
                    "message": "File system endpoints are disabled in read-only (exhibit) mode.",
                    "details": {"endpoint": "fs_recent"},
                },
            )
        home = Path.home()
        suggestions = []

        # Home directory
        suggestions.append({"name": "Home", "path": str(home), "icon": "🏠"})

        # Common dev directories
        for name in ["dev", "projects", "code", "src", "workspace", "repos", "github", "work"]:
            p = home / name
            if p.exists() and p.is_dir():
                suggestions.append({"name": name.title(), "path": str(p), "icon": "📁"})

        # Desktop and Documents
        for name, icon in [("Desktop", "🖥️"), ("Documents", "📄"), ("Downloads", "⬇️")]:
            p = home / name
            if p.exists() and p.is_dir():
                suggestions.append({"name": name, "path": str(p), "icon": icon})

        # Current working directory
        cwd = Path.cwd()
        if cwd != home:
            suggestions.append({"name": "Current Dir", "path": str(cwd), "icon": "📍"})

        return {"ok": True, "result": {"suggestions": suggestions[:10]}}

    @global_router.get("/api/v1/fs/scope_root", dependencies=[Depends(require_admin)])
    async def fs_scope_root(path: str = "") -> Dict[str, Any]:
        """Resolve the effective scope root for a path (git root if applicable)."""
        if ctx.read_only:
            raise HTTPException(
                status_code=403,
                detail={
                    "code": "read_only",
                    "message": "File system endpoints are disabled in read-only (exhibit) mode.",
                    "details": {"endpoint": "fs_scope_root"},
                },
            )
        p = Path(str(path or "")).expanduser()
        if not str(path or "").strip():
            return {"ok": False, "error": {"code": "missing_path", "message": "missing path"}}
        if not p.exists() or not p.is_dir():
            return {"ok": False, "error": {"code": "invalid_path", "message": f"path does not exist: {p}"}}
        try:
            scope = detect_scope(p)
            return {
                "ok": True,
                "result": {
                    "path": str(p.resolve()),
                    "scope_root": str(scope.url),
                    "scope_key": str(scope.scope_key),
                    "git_remote": str(scope.git_remote or ""),
                },
            }
        except Exception as e:
            return {"ok": False, "error": {"code": "resolve_failed", "message": str(e)}}

    # ------------------------------------------------------------------ #
    # Group-scoped routes
    # ------------------------------------------------------------------ #

    @group_router.get("/terminal/tail")
    async def terminal_tail(
        group_id: str,
        actor_id: str,
        max_chars: int = 8000,
        strip_ansi: bool = True,
        compact: bool = True,
    ) -> Dict[str, Any]:
        """Tail an actor's terminal transcript (subject to group policy)."""
        return await ctx.daemon(
            {
                "op": "terminal_tail",
                "args": {
                    "group_id": group_id,
                    "actor_id": actor_id,
                    "max_chars": int(max_chars or 8000),
                    "strip_ansi": bool(strip_ansi),
                    "compact": bool(compact),
                    "by": "user",
                },
            }
        )

    @group_router.post("/terminal/clear")
    async def terminal_clear(group_id: str, actor_id: str) -> Dict[str, Any]:
        """Clear (truncate) an actor's in-memory terminal transcript ring buffer."""
        return await ctx.daemon(
            {
                "op": "terminal_clear",
                "args": {
                    "group_id": group_id,
                    "actor_id": actor_id,
                    "by": "user",
                },
            }
        )

    @group_router.get("/capabilities/state")
    async def capability_state(group_id: str, actor_id: str = "user", capability_id: str = "") -> Dict[str, Any]:
        """Get caller-effective capability state and visible/dynamic tools for a group."""
        return await ctx.daemon(
            {
                "op": "capability_state",
                "args": {
                    "group_id": group_id,
                    "by": "user",
                    "actor_id": str(actor_id or "user").strip() or "user",
                    "capability_id": str(capability_id or "").strip(),
                },
            }
        )

    @group_router.post("/capabilities/enable")
    async def capability_enable(group_id: str, request: Request) -> Dict[str, Any]:
        """Enable/disable a capability for a group (session/actor/group scope)."""
        if ctx.read_only:
            raise HTTPException(
                status_code=403,
                detail={
                    "code": "read_only",
                    "message": "Capability enable endpoints are disabled in read-only (exhibit) mode.",
                },
            )
        try:
            payload = await request.json()
        except Exception:
            payload = {}
        if not isinstance(payload, dict):
            raise HTTPException(status_code=400, detail={"code": "invalid_request", "message": "request body must be an object"})
        capability_id = str(payload.get("capability_id") or "").strip()
        if not capability_id:
            raise HTTPException(status_code=400, detail={"code": "missing_capability_id", "message": "missing capability_id"})
        return await ctx.daemon(
            {
                "op": "capability_enable",
                "args": {
                    "group_id": group_id,
                    "by": "user",
                    "actor_id": str(payload.get("actor_id") or "user").strip() or "user",
                    "capability_id": capability_id,
                    "enabled": bool(payload.get("enabled", True)),
                    "scope": str(payload.get("scope") or "session").strip().lower() or "session",
                    "ttl_seconds": int(payload.get("ttl_seconds") or 3600),
                    "reason": str(payload.get("reason") or "").strip(),
                    "cleanup": bool(payload.get("cleanup", False)),
                },
            }
        )

    @group_router.post("/capabilities/import")
    async def capability_import(group_id: str, request: Request) -> Dict[str, Any]:
        """Import (install) a capability into a group."""
        if ctx.read_only:
            raise HTTPException(
                status_code=403,
                detail={
                    "code": "read_only",
                    "message": "Capability import endpoints are disabled in read-only (exhibit) mode.",
                },
            )
        try:
            payload = await request.json()
        except Exception:
            payload = {}
        if not isinstance(payload, dict):
            raise HTTPException(status_code=400, detail={"code": "invalid_request", "message": "request body must be an object"})
        args: Dict[str, Any] = {
            "group_id": group_id,
            "by": "user",
            "actor_id": str(payload.get("actor_id") or "user").strip() or "user",
            "dry_run": bool(payload.get("dry_run", False)),
            "probe": bool(payload.get("probe", True)),
            "enable_after_import": bool(payload.get("enable_after_import", False)),
            "scope": str(payload.get("scope") or "session").strip().lower() or "session",
            "ttl_seconds": int(payload.get("ttl_seconds") or 3600),
            "reason": str(payload.get("reason") or "").strip(),
        }
        if "record" in payload:
            args["record"] = payload["record"]
        return await ctx.daemon({"op": "capability_import", "args": args})

    @group_router.post("/capabilities/uninstall")
    async def capability_uninstall(group_id: str, request: Request) -> Dict[str, Any]:
        """Uninstall a capability and clean local runtime/autoload references."""
        if ctx.read_only:
            raise HTTPException(
                status_code=403,
                detail={
                    "code": "read_only",
                    "message": "Capability uninstall endpoints are disabled in read-only (exhibit) mode.",
                },
            )
        try:
            payload = await request.json()
        except Exception:
            payload = {}
        if not isinstance(payload, dict):
            raise HTTPException(status_code=400, detail={"code": "invalid_request", "message": "request body must be an object"})
        return await ctx.daemon(
            {
                "op": "capability_uninstall",
                "args": {
                    "group_id": group_id,
                    "by": "user",
                    "actor_id": str(payload.get("actor_id") or "user").strip() or "user",
                    "capability_id": str(payload.get("capability_id") or "").strip(),
                    "reason": str(payload.get("reason") or "").strip(),
                },
            }
        )

    return [global_router, group_router]


def register_base_routes(app: FastAPI, *, ctx: RouteContext) -> None:
    """Backward-compatible wrapper for app.py registration."""
    for router in create_routers(ctx):
        app.include_router(router)
