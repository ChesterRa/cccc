from __future__ import annotations

import asyncio
import json
from typing import Any, Dict

from fastapi import APIRouter, Depends, HTTPException, Query, WebSocket

from ....daemon.server import get_daemon_endpoint

from ..schemas import (
    GroupSpaceArtifactActionRequest,
    GroupSpaceBindRequest,
    GroupSpaceIngestRequest,
    GroupSpaceJobActionRequest,
    GroupSpaceProviderAuthRequest,
    GroupSpaceProviderCredentialUpdateRequest,
    GroupSpaceQueryRequest,
    GroupSpaceSourceActionRequest,
    GroupSpaceSyncRequest,
    RouteContext,
    check_admin,
    require_admin,
    require_group,
    resolve_websocket_principal,
    websocket_tokens_active,
)
from .browser_surface_proxy import (
    open_daemon_stream,
    proxy_daemon_raw_stream_to_websocket,
    send_daemon_attach_request,
)

_PROJECTED_BROWSER_STREAM_LIMIT_BYTES = 16 * 1024 * 1024


def create_routers(ctx: RouteContext) -> list[APIRouter]:
    group_router = APIRouter(prefix="/api/v1/groups/{group_id}", dependencies=[Depends(require_group)])
    global_router = APIRouter(prefix="/api/v1")

    @group_router.get("/space/status")
    async def group_space_status(group_id: str, provider: str = "notebooklm") -> Dict[str, Any]:
        return await ctx.daemon(
            {
                "op": "group_space_status",
                "args": {
                    "group_id": group_id,
                    "provider": str(provider or "notebooklm").strip() or "notebooklm",
                },
            }
        )

    @group_router.get("/space/spaces")
    async def group_space_spaces(group_id: str, provider: str = "notebooklm") -> Dict[str, Any]:
        return await ctx.daemon(
            {
                "op": "group_space_spaces",
                "args": {
                    "group_id": group_id,
                    "provider": str(provider or "notebooklm").strip() or "notebooklm",
                },
            }
        )

    @group_router.post("/space/bind")
    async def group_space_bind(group_id: str, req: GroupSpaceBindRequest) -> Dict[str, Any]:
        if ctx.read_only:
            raise HTTPException(
                status_code=403,
                detail={
                    "code": "read_only",
                    "message": "Group Space write endpoints are disabled in read-only (exhibit) mode.",
                },
            )
        return await ctx.daemon(
            {
                "op": "group_space_bind",
                "args": {
                    "group_id": group_id,
                    "provider": str(req.provider or "notebooklm").strip() or "notebooklm",
                    "lane": str(req.lane).strip(),
                    "action": str(req.action or "bind").strip() or "bind",
                    "remote_space_id": str(req.remote_space_id or "").strip(),
                    "by": str(req.by or "user").strip() or "user",
                },
            }
        )

    @group_router.post("/space/ingest")
    async def group_space_ingest(group_id: str, req: GroupSpaceIngestRequest) -> Dict[str, Any]:
        if ctx.read_only:
            raise HTTPException(
                status_code=403,
                detail={
                    "code": "read_only",
                    "message": "Group Space write endpoints are disabled in read-only (exhibit) mode.",
                },
            )
        return await ctx.daemon(
            {
                "op": "group_space_ingest",
                "args": {
                    "group_id": group_id,
                    "provider": str(req.provider or "notebooklm").strip() or "notebooklm",
                    "lane": str(req.lane).strip(),
                    "kind": str(req.kind or "context_sync").strip() or "context_sync",
                    "payload": dict(req.payload or {}),
                    "idempotency_key": str(req.idempotency_key or "").strip(),
                    "by": str(req.by or "user").strip() or "user",
                },
            }
        )

    @group_router.post("/space/query")
    async def group_space_query(group_id: str, req: GroupSpaceQueryRequest) -> Dict[str, Any]:
        return await ctx.daemon(
            {
                "op": "group_space_query",
                "args": {
                    "group_id": group_id,
                    "provider": str(req.provider or "notebooklm").strip() or "notebooklm",
                    "lane": str(req.lane).strip(),
                    "query": str(req.query or "").strip(),
                    "options": dict(req.options or {}),
                },
            }
        )

    @group_router.get("/space/sources")
    async def group_space_sources_list(
        group_id: str,
        provider: str = "notebooklm",
        lane: str = Query(...),
        fresh: bool = False,
    ) -> Dict[str, Any]:
        return await ctx.daemon(
            {
                "op": "group_space_sources",
                "args": {
                    "group_id": group_id,
                    "provider": str(provider or "notebooklm").strip() or "notebooklm",
                    "lane": str(lane).strip(),
                    "action": "list",
                    "fresh": bool(fresh),
                },
            }
        )

    @group_router.post("/space/sources")
    async def group_space_sources_action(group_id: str, req: GroupSpaceSourceActionRequest) -> Dict[str, Any]:
        if ctx.read_only:
            raise HTTPException(
                status_code=403,
                detail={
                    "code": "read_only",
                    "message": "Group Space write endpoints are disabled in read-only (exhibit) mode.",
                },
            )
        return await ctx.daemon(
            {
                "op": "group_space_sources",
                "args": {
                    "group_id": group_id,
                    "provider": str(req.provider or "notebooklm").strip() or "notebooklm",
                    "lane": str(req.lane).strip(),
                    "action": str(req.action or "refresh").strip() or "refresh",
                    "source_id": str(req.source_id or "").strip(),
                    "new_title": str(req.new_title or "").strip(),
                    "by": str(req.by or "user").strip() or "user",
                },
            }
        )

    @group_router.get("/space/artifacts")
    async def group_space_artifacts_list(
        group_id: str,
        provider: str = "notebooklm",
        lane: str = Query(...),
        kind: str = "",
        fresh: bool = False,
    ) -> Dict[str, Any]:
        return await ctx.daemon(
            {
                "op": "group_space_artifact",
                "args": {
                    "group_id": group_id,
                    "provider": str(provider or "notebooklm").strip() or "notebooklm",
                    "lane": str(lane).strip(),
                    "action": "list",
                    "kind": str(kind or "").strip().lower(),
                    "fresh": bool(fresh),
                },
            }
        )

    @group_router.post("/space/artifacts")
    async def group_space_artifacts_action(group_id: str, req: GroupSpaceArtifactActionRequest) -> Dict[str, Any]:
        if ctx.read_only:
            raise HTTPException(
                status_code=403,
                detail={
                    "code": "read_only",
                    "message": "Group Space write endpoints are disabled in read-only (exhibit) mode.",
                },
            )
        return await ctx.daemon(
            {
                "op": "group_space_artifact",
                "args": {
                    "group_id": group_id,
                    "provider": str(req.provider or "notebooklm").strip() or "notebooklm",
                    "lane": str(req.lane).strip(),
                    "action": str(req.action or "generate").strip() or "generate",
                    "kind": str(req.kind or "").strip().lower(),
                    "options": dict(req.options or {}),
                    "wait": bool(req.wait),
                    "save_to_space": bool(req.save_to_space),
                    "output_path": str(req.output_path or "").strip(),
                    "output_format": str(req.output_format or "").strip().lower(),
                    "artifact_id": str(req.artifact_id or "").strip(),
                    "timeout_seconds": float(req.timeout_seconds),
                    "initial_interval": float(req.initial_interval),
                    "max_interval": float(req.max_interval),
                    "by": str(req.by or "user").strip() or "user",
                },
            }
        )

    @group_router.get("/space/jobs")
    async def group_space_jobs_list(
        group_id: str,
        provider: str = "notebooklm",
        lane: str = Query(...),
        state: str = "",
        limit: int = 50,
    ) -> Dict[str, Any]:
        args: Dict[str, Any] = {
            "group_id": group_id,
            "provider": str(provider or "notebooklm").strip() or "notebooklm",
            "lane": str(lane).strip(),
            "action": "list",
            "limit": int(limit or 50),
        }
        state_value = str(state or "").strip()
        if state_value:
            args["state"] = state_value
        return await ctx.daemon({"op": "group_space_jobs", "args": args})

    @group_router.post("/space/jobs")
    async def group_space_jobs_action(group_id: str, req: GroupSpaceJobActionRequest) -> Dict[str, Any]:
        if ctx.read_only:
            raise HTTPException(
                status_code=403,
                detail={
                    "code": "read_only",
                    "message": "Group Space write endpoints are disabled in read-only (exhibit) mode.",
                },
            )
        return await ctx.daemon(
            {
                "op": "group_space_jobs",
                "args": {
                    "group_id": group_id,
                    "provider": str(req.provider or "notebooklm").strip() or "notebooklm",
                    "lane": str(req.lane).strip(),
                    "action": str(req.action or "retry").strip() or "retry",
                    "job_id": str(req.job_id or "").strip(),
                    "by": str(req.by or "user").strip() or "user",
                },
            }
        )

    @group_router.post("/space/sync")
    async def group_space_sync(group_id: str, req: GroupSpaceSyncRequest) -> Dict[str, Any]:
        if ctx.read_only:
            raise HTTPException(
                status_code=403,
                detail={
                    "code": "read_only",
                    "message": "Group Space write endpoints are disabled in read-only (exhibit) mode.",
                },
            )
        return await ctx.daemon(
            {
                "op": "group_space_sync",
                "args": {
                    "group_id": group_id,
                    "provider": str(req.provider or "notebooklm").strip() or "notebooklm",
                    "lane": str(req.lane).strip(),
                    "action": str(req.action or "run").strip() or "run",
                    "force": bool(req.force),
                    "by": str(req.by or "user").strip() or "user",
                },
            }
        )

    @global_router.get("/space/providers/{provider}/credential", dependencies=[Depends(require_admin)])
    async def group_space_provider_credential_status(provider: str, by: str = "user") -> Dict[str, Any]:
        return await ctx.daemon(
            {
                "op": "group_space_provider_credential_status",
                "args": {
                    "provider": str(provider or "notebooklm").strip() or "notebooklm",
                    "by": str(by or "user").strip() or "user",
                },
            }
        )

    @global_router.post("/space/providers/{provider}/credential", dependencies=[Depends(require_admin)])
    async def group_space_provider_credential_update(
        provider: str,
        req: GroupSpaceProviderCredentialUpdateRequest,
    ) -> Dict[str, Any]:
        if ctx.read_only:
            raise HTTPException(
                status_code=403,
                detail={
                    "code": "read_only",
                    "message": "Provider credential write endpoints are disabled in read-only (exhibit) mode.",
                },
            )
        return await ctx.daemon(
            {
                "op": "group_space_provider_credential_update",
                "args": {
                    "provider": str(provider or "notebooklm").strip() or "notebooklm",
                    "by": str(req.by or "user").strip() or "user",
                    "auth_json": str(req.auth_json or ""),
                    "clear": bool(req.clear),
                },
            }
        )

    @global_router.post("/space/providers/{provider}/health", dependencies=[Depends(require_admin)])
    async def group_space_provider_health_check(provider: str, by: str = "user") -> Dict[str, Any]:
        if ctx.read_only:
            raise HTTPException(
                status_code=403,
                detail={
                    "code": "read_only",
                    "message": "Provider health-check endpoints are disabled in read-only (exhibit) mode.",
                },
            )
        return await ctx.daemon(
            {
                "op": "group_space_provider_health_check",
                "args": {
                    "provider": str(provider or "notebooklm").strip() or "notebooklm",
                    "by": str(by or "user").strip() or "user",
                },
            }
        )

    @global_router.post("/space/providers/{provider}/auth", dependencies=[Depends(require_admin)])
    async def group_space_provider_auth(provider: str, req: GroupSpaceProviderAuthRequest) -> Dict[str, Any]:
        action = str(req.action or "status").strip() or "status"
        if ctx.read_only and action != "status":
            raise HTTPException(
                status_code=403,
                detail={
                    "code": "read_only",
                    "message": "Provider auth-flow write endpoints are disabled in read-only (exhibit) mode.",
                },
            )
        return await ctx.daemon(
            {
                "op": "group_space_provider_auth",
                "args": {
                    "provider": str(provider or "notebooklm").strip() or "notebooklm",
                    "by": str(req.by or "user").strip() or "user",
                    "action": action,
                    "timeout_seconds": int(req.timeout_seconds or 900),
                    "force_reauth": bool(req.force_reauth),
                    "projected": bool(req.projected),
                },
            }
        )

    @global_router.get("/space/providers/{provider}/auth", dependencies=[Depends(require_admin)])
    async def group_space_provider_auth_status(provider: str, by: str = "user") -> Dict[str, Any]:
        return await ctx.daemon(
            {
                "op": "group_space_provider_auth",
                "args": {
                    "provider": str(provider or "notebooklm").strip() or "notebooklm",
                    "by": str(by or "user").strip() or "user",
                    "action": "status",
                },
            }
        )

    @global_router.websocket("/space/providers/{provider}/auth/browser_surface/ws")
    async def group_space_provider_auth_browser_surface_ws(websocket: WebSocket, provider: str) -> None:
        await websocket.accept()
        provider_name = str(provider or "").strip().lower() or "notebooklm"

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
                            "message": "Provider auth browser surface is disabled in read-only mode.",
                            "details": {},
                        },
                    }
                )
            except Exception:
                pass
            try:
                await websocket.close(code=1000)
            except Exception:
                pass
            return

        mode = str(websocket.query_params.get("mode") or "").strip().lower()
        if mode == "vnc":
            try:
                reader, writer = await open_daemon_stream(home=ctx.home, limit=_PROJECTED_BROWSER_STREAM_LIMIT_BYTES)
            except Exception:
                await websocket.close(code=1011)
                return
            try:
                resp = await send_daemon_attach_request(
                    reader,
                    writer,
                    op="space_provider_auth_browser_vnc_attach",
                    args={"provider": provider_name, "by": "user"},
                )
                if not isinstance(resp, dict) or not resp.get("ok"):
                    await websocket.close(code=1008)
                    return
                await proxy_daemon_raw_stream_to_websocket(websocket, reader, writer)
            finally:
                try:
                    writer.close()
                    await writer.wait_closed()
                except Exception:
                    pass
            return

        try:
            ep = get_daemon_endpoint()
            transport = str(ep.get("transport") or "").strip().lower()
            if transport == "tcp":
                host = str(ep.get("host") or "127.0.0.1").strip() or "127.0.0.1"
                port = int(ep.get("port") or 0)
                reader, writer = await asyncio.open_connection(host, port, limit=_PROJECTED_BROWSER_STREAM_LIMIT_BYTES)
            else:
                sock_path = ctx.home / "daemon" / "ccccd.sock"
                path = str(ep.get("path") or sock_path)
                reader, writer = await asyncio.open_unix_connection(path, limit=_PROJECTED_BROWSER_STREAM_LIMIT_BYTES)
        except Exception:
            await websocket.send_json({"ok": False, "error": {"code": "daemon_unavailable", "message": "ccccd unavailable"}})
            await websocket.close(code=1011)
            return

        try:
            req = {
                "op": "space_provider_auth_browser_attach",
                "args": {
                    "provider": provider_name,
                    "by": "user",
                    "viewer_mode": str(websocket.query_params.get("viewer_mode") or "auto"),
                },
            }
            writer.write((json.dumps(req, ensure_ascii=False) + "\n").encode("utf-8"))
            await writer.drain()
            line = await reader.readline()
            try:
                resp = json.loads(line.decode("utf-8", errors="replace"))
            except Exception:
                resp = {}
            if not isinstance(resp, dict) or not resp.get("ok"):
                err = resp.get("error") if isinstance(resp.get("error"), dict) else {"code": "browser_surface_attach_failed", "message": "browser surface attach failed"}
                await websocket.send_json({"ok": False, "error": err})
                await websocket.close(code=1008)
                return

            async def _pump_out() -> None:
                while True:
                    line2 = await reader.readline()
                    if not line2:
                        break
                    await websocket.send_text(line2.decode("utf-8", errors="replace").rstrip("\n"))

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
                try:
                    await asyncio.gather(*pending, return_exceptions=True)
                except Exception:
                    pass
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
            try:
                writer.close()
                await writer.wait_closed()
            except Exception:
                pass

    return [group_router, global_router]
