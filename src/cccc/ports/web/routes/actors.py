from __future__ import annotations

import asyncio
import json
from typing import Any, Dict

from fastapi import FastAPI, HTTPException, Request, WebSocket, WebSocketDisconnect

from ....daemon.server import call_daemon, get_daemon_endpoint
from ....kernel.group import load_group
from ..schemas import (
    ActorCreateRequest,
    ActorProfileUpsertRequest,
    ActorUpdateRequest,
    RouteContext,
    _normalize_command,
)


def register_actor_routes(app: FastAPI, *, ctx: RouteContext) -> None:
    @app.get("/api/v1/groups/{group_id}/actors")
    async def actors(group_id: str, include_unread: bool = False) -> Dict[str, Any]:
        gid = str(group_id or "").strip()
        async def _fetch() -> Dict[str, Any]:
            return await ctx.daemon({"op": "actor_list", "args": {"group_id": gid, "include_unread": include_unread}})

        ttl = max(0.0, min(5.0, ctx.exhibit_cache_ttl_s))
        return await ctx.cached_json(f"actors:{gid}:{int(bool(include_unread))}", ttl, _fetch)

    @app.post("/api/v1/groups/{group_id}/actors")
    async def actor_create(group_id: str, req: ActorCreateRequest) -> Dict[str, Any]:
        command = _normalize_command(req.command) or []
        env_private = dict(req.env_private) if isinstance(req.env_private, dict) else None
        profile_id = str(req.profile_id or "").strip()
        return await ctx.daemon(
            {
                "op": "actor_add",
                "args": {
                    "group_id": group_id,
                    "actor_id": req.actor_id,
                    # Note: role is auto-determined by position
                    "runner": req.runner,
                    "runtime": req.runtime,
                    "title": req.title,
                    "command": command,
                    "env": dict(req.env),
                    "env_private": env_private,
                    "profile_id": profile_id,
                    "default_scope_key": req.default_scope_key,
                    "submit": req.submit,
                    "by": req.by,
                },
            }
        )

    @app.post("/api/v1/groups/{group_id}/actors/{actor_id}")
    async def actor_update(group_id: str, actor_id: str, req: ActorUpdateRequest) -> Dict[str, Any]:
        patch: Dict[str, Any] = {}
        # Note: role is ignored - auto-determined by position
        if req.title is not None:
            patch["title"] = req.title
        if req.command is not None:
            patch["command"] = _normalize_command(req.command)
        if req.env is not None:
            patch["env"] = dict(req.env)
        if req.default_scope_key is not None:
            patch["default_scope_key"] = req.default_scope_key
        if req.submit is not None:
            patch["submit"] = req.submit
        if req.runner is not None:
            patch["runner"] = req.runner
        if req.runtime is not None:
            patch["runtime"] = req.runtime
        if req.enabled is not None:
            patch["enabled"] = bool(req.enabled)
        args: Dict[str, Any] = {
            "group_id": group_id,
            "actor_id": actor_id,
            "patch": patch,
            "by": req.by,
        }
        if req.profile_id is not None:
            args["profile_id"] = str(req.profile_id or "").strip()
        if req.profile_action is not None:
            args["profile_action"] = str(req.profile_action or "").strip()
        return await ctx.daemon({"op": "actor_update", "args": args})

    @app.delete("/api/v1/groups/{group_id}/actors/{actor_id}")
    async def actor_delete(group_id: str, actor_id: str, by: str = "user") -> Dict[str, Any]:
        return await ctx.daemon({"op": "actor_remove", "args": {"group_id": group_id, "actor_id": actor_id, "by": by}})

    @app.post("/api/v1/groups/{group_id}/actors/{actor_id}/start")
    async def actor_start(group_id: str, actor_id: str, by: str = "user") -> Dict[str, Any]:
        return await ctx.daemon({"op": "actor_start", "args": {"group_id": group_id, "actor_id": actor_id, "by": by}})

    @app.post("/api/v1/groups/{group_id}/actors/{actor_id}/stop")
    async def actor_stop(group_id: str, actor_id: str, by: str = "user") -> Dict[str, Any]:
        return await ctx.daemon({"op": "actor_stop", "args": {"group_id": group_id, "actor_id": actor_id, "by": by}})

    @app.post("/api/v1/groups/{group_id}/actors/{actor_id}/restart")
    async def actor_restart(group_id: str, actor_id: str, by: str = "user") -> Dict[str, Any]:
        return await ctx.daemon({"op": "actor_restart", "args": {"group_id": group_id, "actor_id": actor_id, "by": by}})

    @app.get("/api/v1/groups/{group_id}/actors/{actor_id}/env_private")
    async def actor_env_private_keys(group_id: str, actor_id: str, by: str = "user") -> Dict[str, Any]:
        """List configured private env keys + masked previews (never returns raw values)."""
        if ctx.read_only:
            raise HTTPException(
                status_code=403,
                detail={
                    "code": "read_only",
                    "message": "Private env endpoints are disabled in read-only (exhibit) mode.",
                    "details": {"endpoint": "actor_env_private_keys"},
                },
            )
        return await ctx.daemon({"op": "actor_env_private_keys", "args": {"group_id": group_id, "actor_id": actor_id, "by": by}})

    @app.post("/api/v1/groups/{group_id}/actors/{actor_id}/env_private")
    async def actor_env_private_update(request: Request, group_id: str, actor_id: str) -> Dict[str, Any]:
        """Update private env (runtime-only). Values are never returned."""
        if ctx.read_only:
            raise HTTPException(
                status_code=403,
                detail={
                    "code": "read_only",
                    "message": "Private env endpoints are disabled in read-only (exhibit) mode.",
                    "details": {"endpoint": "actor_env_private_update"},
                },
            )
        try:
            payload = await request.json()
        except Exception:
            raise HTTPException(status_code=400, detail={"code": "invalid_request", "message": "invalid JSON body", "details": {}})
        if not isinstance(payload, dict):
            raise HTTPException(status_code=400, detail={"code": "invalid_request", "message": "request body must be an object", "details": {}})

        by = str(payload.get("by") or "user").strip() or "user"
        clear = bool(payload.get("clear") is True)

        set_raw = payload.get("set")
        unset_raw = payload.get("unset")

        if set_raw is not None and not isinstance(set_raw, dict):
            raise HTTPException(status_code=400, detail={"code": "invalid_request", "message": "set must be an object", "details": {}})
        if unset_raw is not None and not isinstance(unset_raw, list):
            raise HTTPException(status_code=400, detail={"code": "invalid_request", "message": "unset must be a list", "details": {}})

        set_vars: Dict[str, str] = {}
        if isinstance(set_raw, dict):
            for k, v in set_raw.items():
                kk = str(k or "").strip()
                if not kk:
                    continue
                # Keep value as string; never echo it back.
                if v is None:
                    continue
                set_vars[kk] = str(v)

        unset_keys: list[str] = []
        if isinstance(unset_raw, list):
            for item in unset_raw:
                kk = str(item or "").strip()
                if kk:
                    unset_keys.append(kk)

        return await ctx.daemon(
            {
                "op": "actor_env_private_update",
                "args": {
                    "group_id": group_id,
                    "actor_id": actor_id,
                    "by": by,
                    "set": set_vars,
                    "unset": unset_keys,
                    "clear": clear,
                },
            }
        )

    @app.get("/api/v1/actor_profiles")
    async def actor_profiles_list(by: str = "user") -> Dict[str, Any]:
        return await ctx.daemon({"op": "actor_profile_list", "args": {"by": by}})

    @app.get("/api/v1/actor_profiles/{profile_id}")
    async def actor_profiles_get(profile_id: str, by: str = "user") -> Dict[str, Any]:
        return await ctx.daemon({"op": "actor_profile_get", "args": {"profile_id": profile_id, "by": by}})

    @app.post("/api/v1/actor_profiles")
    async def actor_profiles_upsert(req: ActorProfileUpsertRequest) -> Dict[str, Any]:
        if ctx.read_only:
            raise HTTPException(
                status_code=403,
                detail={
                    "code": "read_only",
                    "message": "Actor profile write endpoints are disabled in read-only (exhibit) mode.",
                },
            )
        args: Dict[str, Any] = {
            "profile": dict(req.profile or {}),
            "by": req.by,
        }
        if req.expected_revision is not None:
            args["expected_revision"] = int(req.expected_revision)
        return await ctx.daemon({"op": "actor_profile_upsert", "args": args})

    @app.delete("/api/v1/actor_profiles/{profile_id}")
    async def actor_profiles_delete(profile_id: str, by: str = "user", force_detach: bool = False) -> Dict[str, Any]:
        if ctx.read_only:
            raise HTTPException(
                status_code=403,
                detail={
                    "code": "read_only",
                    "message": "Actor profile write endpoints are disabled in read-only (exhibit) mode.",
                },
            )
        return await ctx.daemon(
            {
                "op": "actor_profile_delete",
                "args": {"profile_id": profile_id, "by": by, "force_detach": bool(force_detach)},
            }
        )

    @app.get("/api/v1/actor_profiles/{profile_id}/env_private")
    async def actor_profile_secret_keys(profile_id: str, by: str = "user") -> Dict[str, Any]:
        return await ctx.daemon({"op": "actor_profile_secret_keys", "args": {"profile_id": profile_id, "by": by}})

    @app.post("/api/v1/actor_profiles/{profile_id}/env_private")
    async def actor_profile_secret_update(request: Request, profile_id: str) -> Dict[str, Any]:
        if ctx.read_only:
            raise HTTPException(
                status_code=403,
                detail={
                    "code": "read_only",
                    "message": "Actor profile secret write endpoints are disabled in read-only (exhibit) mode.",
                },
            )
        try:
            payload = await request.json()
        except Exception:
            raise HTTPException(status_code=400, detail={"code": "invalid_request", "message": "invalid JSON body", "details": {}})
        if not isinstance(payload, dict):
            raise HTTPException(status_code=400, detail={"code": "invalid_request", "message": "request body must be an object", "details": {}})

        by = str(payload.get("by") or "user").strip() or "user"
        clear = bool(payload.get("clear") is True)
        set_raw = payload.get("set")
        unset_raw = payload.get("unset")

        if set_raw is not None and not isinstance(set_raw, dict):
            raise HTTPException(status_code=400, detail={"code": "invalid_request", "message": "set must be an object", "details": {}})
        if unset_raw is not None and not isinstance(unset_raw, list):
            raise HTTPException(status_code=400, detail={"code": "invalid_request", "message": "unset must be a list", "details": {}})

        set_vars: Dict[str, str] = {}
        if isinstance(set_raw, dict):
            for key, value in set_raw.items():
                k = str(key or "").strip()
                if not k or value is None:
                    continue
                set_vars[k] = str(value)

        unset_keys: list[str] = []
        if isinstance(unset_raw, list):
            for item in unset_raw:
                k = str(item or "").strip()
                if k:
                    unset_keys.append(k)

        return await ctx.daemon(
            {
                "op": "actor_profile_secret_update",
                "args": {
                    "profile_id": profile_id,
                    "by": by,
                    "set": set_vars,
                    "unset": unset_keys,
                    "clear": clear,
                },
            }
        )

    @app.post("/api/v1/actor_profiles/{profile_id}/copy_actor_secrets")
    async def actor_profile_secret_copy_from_actor(request: Request, profile_id: str) -> Dict[str, Any]:
        if ctx.read_only:
            raise HTTPException(
                status_code=403,
                detail={
                    "code": "read_only",
                    "message": "Actor profile write endpoints are disabled in read-only (exhibit) mode.",
                },
            )
        try:
            payload = await request.json()
        except Exception:
            raise HTTPException(status_code=400, detail={"code": "invalid_request", "message": "invalid JSON body", "details": {}})
        if not isinstance(payload, dict):
            raise HTTPException(status_code=400, detail={"code": "invalid_request", "message": "request body must be an object", "details": {}})

        by = str(payload.get("by") or "user").strip() or "user"
        group_id = str(payload.get("group_id") or "").strip()
        actor_id = str(payload.get("actor_id") or "").strip()
        if not group_id:
            raise HTTPException(status_code=400, detail={"code": "missing_group_id", "message": "missing group_id", "details": {}})
        if not actor_id:
            raise HTTPException(status_code=400, detail={"code": "missing_actor_id", "message": "missing actor_id", "details": {}})

        return await ctx.daemon(
            {
                "op": "actor_profile_secret_copy_from_actor",
                "args": {
                    "profile_id": profile_id,
                    "group_id": group_id,
                    "actor_id": actor_id,
                    "by": by,
                },
            }
        )

    @app.post("/api/v1/actor_profiles/{profile_id}/copy_profile_secrets")
    async def actor_profile_secret_copy_from_profile(request: Request, profile_id: str) -> Dict[str, Any]:
        if ctx.read_only:
            raise HTTPException(
                status_code=403,
                detail={
                    "code": "read_only",
                    "message": "Actor profile write endpoints are disabled in read-only (exhibit) mode.",
                },
            )
        try:
            payload = await request.json()
        except Exception:
            raise HTTPException(status_code=400, detail={"code": "invalid_request", "message": "invalid JSON body", "details": {}})
        if not isinstance(payload, dict):
            raise HTTPException(status_code=400, detail={"code": "invalid_request", "message": "request body must be an object", "details": {}})

        by = str(payload.get("by") or "user").strip() or "user"
        source_profile_id = str(payload.get("source_profile_id") or "").strip()
        if not source_profile_id:
            raise HTTPException(
                status_code=400,
                detail={"code": "missing_source_profile_id", "message": "missing source_profile_id", "details": {}},
            )

        return await ctx.daemon(
            {
                "op": "actor_profile_secret_copy_from_profile",
                "args": {
                    "profile_id": profile_id,
                    "source_profile_id": source_profile_id,
                    "by": by,
                },
            }
        )

    @app.websocket("/api/v1/groups/{group_id}/actors/{actor_id}/term")
    async def actor_terminal(websocket: WebSocket, group_id: str, actor_id: str) -> None:
        token = ctx.configured_web_token()
        if token:
            provided = str(websocket.query_params.get("token") or "").strip()
            cookie = ""
            try:
                cookie = str(getattr(websocket, "cookies", {}) or {}).get("cccc_web_token") or ""
            except Exception:
                cookie = ""
            if provided != token and str(cookie).strip() != token:
                await websocket.close(code=4401)
                return

        await websocket.accept()

        if ctx.read_only and not ctx.exhibit_allow_terminal:
            try:
                await websocket.send_json(
                    {
                        "ok": False,
                        "error": {
                            "code": "read_only_terminal",
                            "message": "Terminal is disabled in read-only (exhibit) mode.",
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

        group = load_group(group_id)
        if group is None:
            await websocket.send_json({"ok": False, "error": {"code": "group_not_found", "message": f"group not found: {group_id}"}})
            await websocket.close(code=1008)
            return

        try:
            ep = get_daemon_endpoint()
            transport = str(ep.get("transport") or "").strip().lower()
            if transport == "tcp":
                host = str(ep.get("host") or "127.0.0.1").strip() or "127.0.0.1"
                port = int(ep.get("port") or 0)
                reader, writer = await asyncio.open_connection(host, port)
            else:
                sock_path = ctx.home / "daemon" / "ccccd.sock"
                path = str(ep.get("path") or sock_path)
                reader, writer = await asyncio.open_unix_connection(path)
        except Exception:
            await websocket.send_json({"ok": False, "error": {"code": "daemon_unavailable", "message": "ccccd unavailable"}})
            await websocket.close(code=1011)
            return

        try:
            req = {"op": "term_attach", "args": {"group_id": group_id, "actor_id": actor_id}}
            writer.write((json.dumps(req, ensure_ascii=False) + "\n").encode("utf-8"))
            await writer.drain()
            line = await reader.readline()
            try:
                resp = json.loads(line.decode("utf-8", errors="replace"))
            except Exception:
                resp = {}
            if not isinstance(resp, dict) or not resp.get("ok"):
                err = resp.get("error") if isinstance(resp.get("error"), dict) else {"code": "term_attach_failed", "message": "term attach failed"}
                await websocket.send_json({"ok": False, "error": err})
                await websocket.close(code=1008)
                return

            async def _pump_out() -> None:
                while True:
                    data = await reader.read(65536)
                    if not data:
                        break
                    await websocket.send_bytes(data)

            async def _pump_in() -> None:
                while True:
                    raw = await websocket.receive_text()
                    if not raw:
                        continue
                    obj: Any = None
                    try:
                        obj = json.loads(raw)
                    except Exception:
                        obj = None
                    if not isinstance(obj, dict):
                        continue
                    t = str(obj.get("t") or "")
                    if t == "i":
                        if ctx.read_only:
                            continue
                        data = str(obj.get("d") or "")
                        if data:
                            writer.write(data.encode("utf-8", errors="replace"))
                            await writer.drain()
                        continue
                    if t == "r":
                        if ctx.read_only:
                            continue
                        try:
                            cols = int(obj.get("c") or 0)
                            rows = int(obj.get("r") or 0)
                        except Exception:
                            cols = 0
                            rows = 0
                        if cols >= 10 and rows >= 2:
                            await asyncio.to_thread(
                                call_daemon,
                                {"op": "term_resize", "args": {"group_id": group_id, "actor_id": actor_id, "cols": cols, "rows": rows}},
                            )
                        continue

            out_task = asyncio.create_task(_pump_out())
            in_task = asyncio.create_task(_pump_in())
            try:
                done, pending = await asyncio.wait({out_task, in_task}, return_when=asyncio.FIRST_COMPLETED)
                for t in done:
                    try:
                        _ = t.result()
                    except Exception:
                        pass
                for t in pending:
                    t.cancel()
                try:
                    await asyncio.gather(*pending, return_exceptions=True)
                except Exception:
                    pass
            except WebSocketDisconnect:
                pass
        finally:
            try:
                writer.close()
                await writer.wait_closed()
            except Exception:
                pass
