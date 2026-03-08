from __future__ import annotations

import asyncio
import json
from typing import Any, Dict, Optional

from fastapi import APIRouter, Depends, HTTPException, Request, WebSocket, WebSocketDisconnect

from ....daemon.server import call_daemon, get_daemon_endpoint
from ....kernel.group import load_group
from ..schemas import (
    ActorCreateRequest,
    ActorProfileUpsertRequest,
    ActorUpdateRequest,
    RouteContext,
    _normalize_command,
    check_admin,
    check_group,
    require_admin,
    require_group,
    resolve_websocket_principal,
    websocket_tokens_active,
)


def create_routers(ctx: RouteContext) -> list[APIRouter]:
    group_router = APIRouter(prefix="/api/v1/groups/{group_id}", dependencies=[Depends(require_group)])
    global_router = APIRouter(prefix="/api/v1")

    async def _developer_mode_enabled() -> bool:
        try:
            resp = await ctx.daemon({"op": "observability_get"})
            obs = (resp.get("result") or {}).get("observability") if isinstance(resp, dict) else None
            return bool(obs.get("developer_mode")) if isinstance(obs, dict) else False
        except Exception:
            return False

    def _runner_is_headless(value: Any) -> bool:
        return str(value or "").strip().lower() == "headless"

    def _headless_error(*, source: str) -> HTTPException:
        return HTTPException(
            status_code=400,
            detail={
                "code": "headless_internal_only",
                "message": "Headless runner is internal-only. Standard Web uses PTY actors only.",
                "details": {
                    "source": source,
                    "hint": "Use PTY actors/profiles in standard Web mode. Headless is reserved for internal/developer workflows.",
                },
            },
        )

    async def _profile_runner(profile_id: str) -> Optional[str]:
        pid = str(profile_id or "").strip()
        if not pid:
            return None
        try:
            resp = await ctx.daemon({"op": "actor_profile_get", "args": {"profile_id": pid, "by": "user"}})
        except Exception:
            return None
        if not bool(resp.get("ok")):
            return None
        profile = (resp.get("result") or {}).get("profile") if isinstance(resp, dict) else None
        if not isinstance(profile, dict):
            return None
        return str(profile.get("runner") or "pty").strip().lower() or "pty"

    async def _actor_runner(group_id: str, actor_id: str) -> Optional[str]:
        gid = str(group_id or "").strip()
        aid = str(actor_id or "").strip()
        if not gid or not aid:
            return None
        try:
            resp = await ctx.daemon({"op": "actor_list", "args": {"group_id": gid, "include_unread": False}})
        except Exception:
            return None
        if not bool(resp.get("ok")):
            return None
        actors = (resp.get("result") or {}).get("actors") if isinstance(resp, dict) else None
        if not isinstance(actors, list):
            return None
        for actor in actors:
            if not isinstance(actor, dict):
                continue
            if str(actor.get("id") or "").strip() != aid:
                continue
            return str(actor.get("runner_effective") or actor.get("runner") or "pty").strip().lower() or "pty"
        return None

    async def _ensure_standard_web_runner(*, source: str, runner: Optional[str] = None, profile_id: str = "") -> None:
        if await _developer_mode_enabled():
            return
        if _runner_is_headless(runner):
            raise _headless_error(source=source)
        profile_runner = await _profile_runner(profile_id)
        if _runner_is_headless(profile_runner):
            raise _headless_error(source=f"{source}:profile")

    @group_router.get("/actors")
    async def actors(group_id: str, include_unread: bool = False) -> Dict[str, Any]:
        gid = str(group_id or "").strip()
        async def _fetch() -> Dict[str, Any]:
            return await ctx.daemon({"op": "actor_list", "args": {"group_id": gid, "include_unread": include_unread}})

        ttl = max(0.0, min(5.0, ctx.exhibit_cache_ttl_s))
        return await ctx.cached_json(f"actors:{gid}:{int(bool(include_unread))}", ttl, _fetch)

    @group_router.post("/actors")
    async def actor_create(group_id: str, req: ActorCreateRequest) -> Dict[str, Any]:
        command = _normalize_command(req.command) or []
        env_private = dict(req.env_private) if isinstance(req.env_private, dict) else None
        profile_id = str(req.profile_id or "").strip()
        await _ensure_standard_web_runner(source="actor_create", runner=str(req.runner or "pty"), profile_id=profile_id)
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
                    "capability_autoload": list(req.capability_autoload or []),
                    "env_private": env_private,
                    "profile_id": profile_id,
                    "default_scope_key": req.default_scope_key,
                    "submit": req.submit,
                    "by": req.by,
                },
            }
        )

    @group_router.post("/actors/{actor_id}")
    async def actor_update(group_id: str, actor_id: str, req: ActorUpdateRequest) -> Dict[str, Any]:
        await _ensure_standard_web_runner(
            source="actor_update",
            runner=str(req.runner or "") if req.runner is not None else None,
            profile_id=str(req.profile_id or "").strip(),
        )
        patch: Dict[str, Any] = {}
        # Note: role is ignored - auto-determined by position
        if req.title is not None:
            patch["title"] = req.title
        if req.command is not None:
            patch["command"] = _normalize_command(req.command)
        if req.env is not None:
            patch["env"] = dict(req.env)
        if req.capability_autoload is not None:
            patch["capability_autoload"] = list(req.capability_autoload)
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

    @group_router.delete("/actors/{actor_id}")
    async def actor_delete(group_id: str, actor_id: str, by: str = "user") -> Dict[str, Any]:
        return await ctx.daemon({"op": "actor_remove", "args": {"group_id": group_id, "actor_id": actor_id, "by": by}})

    @group_router.post("/actors/{actor_id}/start")
    async def actor_start(group_id: str, actor_id: str, by: str = "user") -> Dict[str, Any]:
        if not await _developer_mode_enabled() and _runner_is_headless(await _actor_runner(group_id, actor_id)):
            raise _headless_error(source="actor_start")
        return await ctx.daemon({"op": "actor_start", "args": {"group_id": group_id, "actor_id": actor_id, "by": by}})

    @group_router.post("/actors/{actor_id}/stop")
    async def actor_stop(group_id: str, actor_id: str, by: str = "user") -> Dict[str, Any]:
        return await ctx.daemon({"op": "actor_stop", "args": {"group_id": group_id, "actor_id": actor_id, "by": by}})

    @group_router.post("/actors/{actor_id}/restart")
    async def actor_restart(group_id: str, actor_id: str, by: str = "user") -> Dict[str, Any]:
        if not await _developer_mode_enabled() and _runner_is_headless(await _actor_runner(group_id, actor_id)):
            raise _headless_error(source="actor_restart")
        return await ctx.daemon({"op": "actor_restart", "args": {"group_id": group_id, "actor_id": actor_id, "by": by}})

    @group_router.get("/actors/{actor_id}/env_private")
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

    @group_router.post("/actors/{actor_id}/env_private")
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

    @global_router.get("/actor_profiles", dependencies=[Depends(require_admin)])
    async def actor_profiles_list(by: str = "user") -> Dict[str, Any]:
        resp = await ctx.daemon({"op": "actor_profile_list", "args": {"by": by}})
        if await _developer_mode_enabled():
            return resp
        result = resp.get("result") if isinstance(resp, dict) else None
        profiles = result.get("profiles") if isinstance(result, dict) else None
        if isinstance(profiles, list):
            result["profiles"] = [
                item for item in profiles if isinstance(item, dict) and not _runner_is_headless(item.get("runner"))
            ]
        return resp

    @global_router.get("/actor_profiles/{profile_id}", dependencies=[Depends(require_admin)])
    async def actor_profiles_get(profile_id: str, by: str = "user") -> Dict[str, Any]:
        resp = await ctx.daemon({"op": "actor_profile_get", "args": {"profile_id": profile_id, "by": by}})
        if not await _developer_mode_enabled():
            profile = (resp.get("result") or {}).get("profile") if isinstance(resp, dict) else None
            if isinstance(profile, dict) and _runner_is_headless(profile.get("runner")):
                raise _headless_error(source="actor_profile_get")
        return resp

    @global_router.post("/actor_profiles", dependencies=[Depends(require_admin)])
    async def actor_profiles_upsert(req: ActorProfileUpsertRequest) -> Dict[str, Any]:
        if ctx.read_only:
            raise HTTPException(
                status_code=403,
                detail={
                    "code": "read_only",
                    "message": "Actor profile write endpoints are disabled in read-only (exhibit) mode.",
                },
            )
        profile_payload = dict(req.profile or {})
        if not await _developer_mode_enabled():
            if _runner_is_headless(profile_payload.get("runner")):
                raise _headless_error(source="actor_profile_upsert")
            profile_payload["runner"] = "pty"
        args: Dict[str, Any] = {
            "profile": profile_payload,
            "by": req.by,
        }
        if req.expected_revision is not None:
            args["expected_revision"] = int(req.expected_revision)
        return await ctx.daemon({"op": "actor_profile_upsert", "args": args})

    @global_router.delete("/actor_profiles/{profile_id}", dependencies=[Depends(require_admin)])
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

    @global_router.get("/actor_profiles/{profile_id}/env_private", dependencies=[Depends(require_admin)])
    async def actor_profile_secret_keys(profile_id: str, by: str = "user") -> Dict[str, Any]:
        return await ctx.daemon({"op": "actor_profile_secret_keys", "args": {"profile_id": profile_id, "by": by}})

    @global_router.post("/actor_profiles/{profile_id}/env_private", dependencies=[Depends(require_admin)])
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

    @global_router.post("/actor_profiles/{profile_id}/copy_actor_secrets")
    async def actor_profile_secret_copy_from_actor(request: Request, profile_id: str) -> Dict[str, Any]:
        check_admin(request)
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
        check_group(request, group_id)
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

    @global_router.post("/actor_profiles/{profile_id}/copy_profile_secrets", dependencies=[Depends(require_admin)])
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

    @global_router.websocket("/groups/{group_id}/actors/{actor_id}/term")
    async def actor_terminal(websocket: WebSocket, group_id: str, actor_id: str) -> None:
        # Accept WebSocket first — closing before accept violates the protocol
        # and causes the browser to see code 1006 instead of our intended close code.
        await websocket.accept()

        principal = resolve_websocket_principal(websocket)
        websocket.state.principal = principal

        auth_header = str((getattr(websocket, "headers", {}) or {}).get("authorization") or "").strip()
        has_header_token = auth_header.lower().startswith("bearer ") and bool(str(auth_header[7:] or "").strip())
        has_cookie_token = False
        try:
            cookies = getattr(websocket, "cookies", None) or {}
            has_cookie_token = bool(str(cookies.get("cccc_web_token") or "").strip())
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
            check_group(websocket, group_id)
        except HTTPException as exc:
            detail = exc.detail if isinstance(exc.detail, dict) else {"code": "permission_denied", "message": str(exc.detail or "permission denied")}
            try:
                await websocket.send_json({"ok": False, "error": detail})
            except Exception:
                pass
            await websocket.close(code=1008)
            return

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

    return [group_router, global_router]
