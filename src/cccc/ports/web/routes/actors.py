from __future__ import annotations

import asyncio
import json
import os
import subprocess
import time
import threading
from typing import Any, Dict, Optional

from fastapi import APIRouter, Depends, File, Form, HTTPException, Request, UploadFile, WebSocket, WebSocketDisconnect
from fastapi.responses import FileResponse
from starlette.concurrency import run_in_threadpool

from ....daemon.server import call_daemon, get_daemon_endpoint
from ....daemon.codex_app_sessions import SUPERVISOR as codex_app_supervisor
from ....daemon.actors.actor_profile_store import get_actor_profile, get_actor_profile_by_ref
from ....daemon.actors.web_model_runtime_ops import decorate_web_model_queued_turn_info
from ....daemon.context.context_ops import _agent_state_to_dict
from ....daemon.runner_state_ops import headless_state_path, headless_state_running, pty_state_path, read_headless_state
from ....kernel.group import load_group
from ....kernel.actors import find_actor
from ....kernel.context import ContextStorage
from ....kernel.inbox import get_indexed_unread_counts
from ....kernel.query_projections import get_actor_list_projection
from ....kernel.working_state import DEFAULT_PTY_TERMINAL_SIGNAL_TAIL_BYTES, derive_effective_working_state
from ....runners import headless as headless_runner
from ....runners import pty as pty_runner
from ....util.fs import read_json
from ....util.process import pid_is_alive
from ....util.conv import coerce_bool
from ..actor_avatar import (
    build_actor_web_payload,
    delete_actor_avatar,
    resolve_actor_avatar_path,
    store_actor_avatar,
)
from .groups import invalidate_context_read
from ..schemas import (
    ActorCreateRequest,
    ActorProfileUpsertRequest,
    ActorUpdateRequest,
    RouteContext,
    _normalize_command,
    check_admin,
    check_group,
    get_principal,
    require_admin,
    require_group,
    require_user,
    resolve_websocket_principal,
    websocket_tokens_active,
)

_READONLY_ACTOR_CACHE: Dict[str, tuple[float, Dict[str, Any]]] = {}
_READONLY_ACTOR_INFLIGHT: Dict[str, Dict[str, Any]] = {}
_READONLY_ACTOR_HANDOFF_ONCE: set[str] = set()
_READONLY_ACTOR_GENERATION: Dict[str, int] = {}
_READONLY_ACTOR_CACHE_LOCK = threading.Lock()
_READONLY_ACTOR_TTL_S = 0.8
_STANDARD_WEB_HEADLESS_RUNTIMES = frozenset({"codex", "claude", "web_model"})


def _pid_matches_actor_context(pid: int, *, group_id: str, actor_id: str) -> bool:
    target_pid = int(pid or 0)
    gid = str(group_id or "").strip()
    aid = str(actor_id or "").strip()
    if target_pid <= 0 or not gid or not aid:
        return False

    expected_group = f"CCCC_GROUP_ID={gid}"
    expected_actor = f"CCCC_ACTOR_ID={aid}"

    if os.name != "nt":
        proc_environ = f"/proc/{target_pid}/environ"
        try:
            with open(proc_environ, "rb") as handle:
                raw = handle.read()
            entries = {
                chunk.decode("utf-8", errors="ignore")
                for chunk in raw.split(b"\0")
                if chunk
            }
            if expected_group in entries and expected_actor in entries:
                return True
        except Exception:
            pass

    try:
        result = subprocess.run(
            ["ps", "eww", "-p", str(target_pid), "-o", "command="],
            capture_output=True,
            text=True,
            check=False,
            timeout=1.0,
        )
        command_parts = [part for part in str(result.stdout or "").strip().split() if part]
        if expected_group in command_parts and expected_actor in command_parts:
            return True
    except Exception:
        pass

    return False


async def invalidate_readonly_actor_list(group_id: str) -> None:
    gid = str(group_id or "").strip()
    if not gid:
        return
    with _READONLY_ACTOR_CACHE_LOCK:
        for suffix in ("readonly", "unread", "readonly_internal", "unread_internal"):
            cache_key = f"actors:{gid}:{suffix}"
            _READONLY_ACTOR_GENERATION[cache_key] = int(_READONLY_ACTOR_GENERATION.get(cache_key, 0)) + 1
            _READONLY_ACTOR_CACHE.pop(cache_key, None)
            _READONLY_ACTOR_HANDOFF_ONCE.discard(cache_key)
            _READONLY_ACTOR_INFLIGHT.pop(cache_key, None)


def _read_actor_list_local(group_id: str, *, include_unread: bool) -> Dict[str, Any]:
    gid = str(group_id or "").strip()
    if not gid:
        return {"ok": False, "error": {"code": "missing_group_id", "message": "missing group_id"}}
    group = load_group(gid)
    if group is None:
        return {"ok": False, "error": {"code": "group_not_found", "message": f"group not found: {gid}"}}

    actors = get_actor_list_projection(group)
    storage = ContextStorage(group)
    agent_rows = [_agent_state_to_dict(agent) for agent in storage.load_agents().agents]
    agent_state_by_id = {
        str(item.get("id") or "").strip(): item
        for item in agent_rows
        if isinstance(item, dict) and str(item.get("id") or "").strip()
    } if isinstance(agent_rows, list) else {}

    for actor in actors:
        aid = str(actor.get("id") or "").strip()
        if not aid:
            continue
        runner_kind = str(actor.get("runner") or "pty").strip().lower() or "pty"
        effective_runner = "headless" if runner_kind == "headless" else "pty"
        runtime = str(actor.get("runtime") or "").strip()
        running = False
        idle_seconds = None
        headless_state = None
        if runtime.lower() == "web_model" and effective_runner == "headless":
            headless_state = read_headless_state(gid, aid)
            running = bool(coerce_bool(actor.get("enabled"), default=True) and headless_state_running(gid, aid))
        if not running and runtime.lower() == "codex":
            try:
                state_doc = read_json(headless_state_path(gid, aid))
            except Exception:
                state_doc = {}
            pid = int(state_doc.get("pid") or 0) if isinstance(state_doc, dict) else 0
            headless_state = dict(state_doc) if isinstance(state_doc, dict) else None
            running = bool(
                isinstance(state_doc, dict)
                and str(state_doc.get("kind") or "") == "headless"
                and str(state_doc.get("group_id") or "") == gid
                and str(state_doc.get("actor_id") or "") == aid
                and str(state_doc.get("runtime") or "").strip().lower() == "codex"
                and pid > 0
                and pid_is_alive(pid)
                and str(state_doc.get("status") or "").strip().lower() != "stopped"
            )
            if not running:
                state = codex_app_supervisor.get_state(group_id=gid, actor_id=aid)
                headless_state = state.model_dump() if hasattr(state, "model_dump") else (dict(state) if isinstance(state, dict) else headless_state)
                running = bool(state is not None and codex_app_supervisor.actor_running(gid, aid))
            if running:
                effective_runner = "headless"
        if not running and runtime.lower() == "claude" and effective_runner == "headless":
            try:
                state_doc = read_json(headless_state_path(gid, aid))
            except Exception:
                state_doc = {}
            pid = int(state_doc.get("pid") or 0) if isinstance(state_doc, dict) else 0
            headless_state = dict(state_doc) if isinstance(state_doc, dict) else None
            running = bool(
                isinstance(state_doc, dict)
                and str(state_doc.get("kind") or "") == "headless"
                and str(state_doc.get("group_id") or "") == gid
                and str(state_doc.get("actor_id") or "") == aid
                and str(state_doc.get("runtime") or "").strip().lower() == "claude"
                and pid > 0
                and pid_is_alive(pid)
                and str(state_doc.get("status") or "").strip().lower() != "stopped"
            )
        if not running and runtime.lower() != "web_model" and effective_runner == "headless":
            state = headless_runner.SUPERVISOR.get_state(group_id=gid, actor_id=aid)
            headless_state = state.model_dump() if hasattr(state, "model_dump") else (dict(state) if isinstance(state, dict) else None)
            running = bool(state is not None and headless_runner.SUPERVISOR.actor_running(gid, aid))
        if not running and effective_runner != "headless":
            running = bool(pty_runner.SUPERVISOR.actor_running(gid, aid))
            if not running:
                try:
                    state_doc = read_json(pty_state_path(gid, aid))
                    pid = int(state_doc.get("pid") or 0) if isinstance(state_doc, dict) else 0
                except Exception:
                    state_doc = {}
                    pid = 0
                running = bool(
                    isinstance(state_doc, dict)
                    and str(state_doc.get("kind") or "") == "pty"
                    and str(state_doc.get("group_id") or "") == gid
                    and str(state_doc.get("actor_id") or "") == aid
                    and pid > 0
                    and pid_is_alive(pid)
                    and _pid_matches_actor_context(pid, group_id=gid, actor_id=aid)
                )
            idle_seconds = pty_runner.SUPERVISOR.idle_seconds(group_id=gid, actor_id=aid) if running else None
        pty_terminal_text = ""
        if effective_runner == "pty" and running:
            try:
                pty_terminal_text = pty_runner.SUPERVISOR.tail_output(
                    group_id=gid,
                    actor_id=aid,
                    max_bytes=DEFAULT_PTY_TERMINAL_SIGNAL_TAIL_BYTES,
                ).decode("utf-8", errors="replace")
            except Exception:
                pty_terminal_text = ""
        actor["running"] = running
        actor["idle_seconds"] = idle_seconds
        actor["runner_effective"] = effective_runner
        actor.update(
            derive_effective_working_state(
                running=running,
                effective_runner=effective_runner,
                runtime=runtime,
                idle_seconds=idle_seconds,
                pty_terminal_text=pty_terminal_text,
                agent_state=agent_state_by_id.get(aid),
                headless_state=headless_state,
            )
        )
        if runtime.lower() == "web_model":
            decorate_web_model_queued_turn_info(actor, group, actor_id=aid, headless_state=headless_state)

    if include_unread:
        counts = get_indexed_unread_counts(group, actors=actors)
        for actor in actors:
            aid = str(actor.get("id") or "").strip()
            if aid:
                actor["unread_count"] = counts.get(aid, 0)

    return {"ok": True, "result": {"actors": actors}}


def create_routers(ctx: RouteContext) -> list[APIRouter]:
    group_router = APIRouter(prefix="/api/v1/groups/{group_id}", dependencies=[Depends(require_group)])
    global_router = APIRouter(prefix="/api/v1")

    async def _cached_actor_list(group_id: str, fetcher, *, cache_suffix: str = "readonly") -> Dict[str, Any]:  # type: ignore[no-untyped-def]
        gid = str(group_id or "").strip()
        if not gid:
            return await fetcher()

        ttl_s = _READONLY_ACTOR_TTL_S if ctx.read_only else 0.0
        suffix = str(cache_suffix or "readonly").strip() or "readonly"
        cache_key = f"actors:{gid}:{suffix}"
        now = time.monotonic()
        inflight_entry: Optional[Dict[str, Any]] = None
        fetch_generation = 0
        do_fetch = False

        with _READONLY_ACTOR_CACHE_LOCK:
            hit = _READONLY_ACTOR_CACHE.get(cache_key)
            if ttl_s > 0 and hit is not None and hit[0] > now:
                return hit[1]
            if ttl_s <= 0 and hit is not None and cache_key in _READONLY_ACTOR_HANDOFF_ONCE:
                _READONLY_ACTOR_HANDOFF_ONCE.discard(cache_key)
                _READONLY_ACTOR_CACHE.pop(cache_key, None)
                return hit[1]
            inflight_entry = _READONLY_ACTOR_INFLIGHT.get(cache_key)
            if inflight_entry is None:
                inflight_entry = {"event": threading.Event(), "result": None, "error": None, "waiters": 1}
                _READONLY_ACTOR_INFLIGHT[cache_key] = inflight_entry
                fetch_generation = int(_READONLY_ACTOR_GENERATION.get(cache_key, 0))
                do_fetch = True
            else:
                inflight_entry["waiters"] = int(inflight_entry.get("waiters", 1)) + 1

        try:
            if inflight_entry is not None and not do_fetch:
                await asyncio.to_thread(inflight_entry["event"].wait)
                error = inflight_entry.get("error")
                if error is not None:
                    raise error
                result = inflight_entry.get("result")
                if isinstance(result, dict):
                    return result
                return await fetcher()

            val = await fetcher()
            with _READONLY_ACTOR_CACHE_LOCK:
                current_generation = int(_READONLY_ACTOR_GENERATION.get(cache_key, 0))
                if current_generation == fetch_generation and _READONLY_ACTOR_INFLIGHT.get(cache_key) is inflight_entry:
                    if ttl_s > 0:
                        _READONLY_ACTOR_CACHE[cache_key] = (time.monotonic() + ttl_s, val)
                    elif int(inflight_entry.get("waiters", 1)) <= 1:
                        _READONLY_ACTOR_CACHE[cache_key] = (time.monotonic(), val)
                        _READONLY_ACTOR_HANDOFF_ONCE.add(cache_key)
                if inflight_entry is not None:
                    inflight_entry["result"] = val
                    inflight_entry["event"].set()
            return val
        except Exception as exc:
            with _READONLY_ACTOR_CACHE_LOCK:
                if inflight_entry is not None:
                    inflight_entry["error"] = exc
                    inflight_entry["event"].set()
            raise
        finally:
            with _READONLY_ACTOR_CACHE_LOCK:
                if inflight_entry is not None:
                    remaining_waiters = int(inflight_entry.get("waiters", 1)) - 1
                    if remaining_waiters > 0:
                        inflight_entry["waiters"] = remaining_waiters
                    elif _READONLY_ACTOR_INFLIGHT.get(cache_key) is inflight_entry:
                        _READONLY_ACTOR_INFLIGHT.pop(cache_key, None)

    async def _developer_mode_enabled() -> bool:
        try:
            resp = await ctx.daemon({"op": "observability_get"})
            obs = (resp.get("result") or {}).get("observability") if isinstance(resp, dict) else None
            return bool(obs.get("developer_mode")) if isinstance(obs, dict) else False
        except Exception:
            return False

    def _normalize_runtime_id(value: Any) -> str:
        return str(value or "").strip().lower()

    def _supports_standard_web_headless_runtime(value: Any) -> bool:
        return _normalize_runtime_id(value) in _STANDARD_WEB_HEADLESS_RUNTIMES

    def _runner_is_headless(value: Any) -> bool:
        return str(value or "").strip().lower() == "headless"

    def _headless_error(*, source: str) -> HTTPException:
        return HTTPException(
            status_code=400,
            detail={
                "code": "headless_internal_only",
                "message": "This headless runtime is internal-only. Standard Web headless mode currently supports codex, claude, and web_model.",
                "details": {
                    "source": source,
                    "hint": "Use PTY for unsupported runtimes in standard Web mode. Other headless runtimes remain reserved for internal/developer workflows.",
                },
            },
        )

    async def _profile_runtime_meta(
        profile_id: str,
        *,
        scope: str = "",
        owner_id: str = "",
    ) -> Optional[Dict[str, str]]:
        pid = str(profile_id or "").strip()
        if not pid:
            return None
        try:
            if str(scope or "").strip():
                profile = get_actor_profile_by_ref(
                    {
                        "profile_id": pid,
                        "profile_scope": scope,
                        "profile_owner": owner_id,
                    }
                )
            else:
                profile = get_actor_profile(pid)
        except Exception:
            return None
        if not isinstance(profile, dict):
            return None
        return {
            "runner": str(profile.get("runner") or "pty").strip().lower() or "pty",
            "runtime": str(profile.get("runtime") or "codex").strip().lower() or "codex",
        }

    async def _actor_runtime_meta(group_id: str, actor_id: str) -> Optional[Dict[str, str]]:
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
            return {
                "runner": str(actor.get("runner_effective") or actor.get("runner") or "pty").strip().lower() or "pty",
                "runtime": str(actor.get("runtime") or "codex").strip().lower() or "codex",
            }
        return None

    def _is_internal_headless_runtime(meta: Optional[Dict[str, str]]) -> bool:
        if not isinstance(meta, dict):
            return False
        runtime = _normalize_runtime_id(meta.get("runtime"))
        runner = str(meta.get("runner") or "").strip().lower()
        return runner == "headless" and not _supports_standard_web_headless_runtime(runtime)

    async def _ensure_standard_web_runner(
        request: Request,
        *,
        source: str,
        runner: Optional[str] = None,
        runtime: Optional[str] = None,
        profile_id: str = "",
        profile_scope: str = "",
        profile_owner: str = "",
        group_id: str = "",
        actor_id: str = "",
    ) -> None:
        if await _developer_mode_enabled():
            return
        runtime_kind = _normalize_runtime_id(runtime)
        profile_meta = await _profile_runtime_meta(profile_id, scope=profile_scope, owner_id=profile_owner)
        if _is_internal_headless_runtime(profile_meta):
            raise _headless_error(source=f"{source}:profile")
        if _runner_is_headless(runner) and runtime_kind and not _supports_standard_web_headless_runtime(runtime_kind):
            raise _headless_error(source=source)
        if not runtime_kind and group_id and actor_id:
            actor_meta = await _actor_runtime_meta(group_id, actor_id)
            if _is_internal_headless_runtime(actor_meta):
                raise _headless_error(source=f"{source}:actor")
            runtime_kind = _normalize_runtime_id((actor_meta or {}).get("runtime"))
        if _runner_is_headless(runner) and runtime_kind and not _supports_standard_web_headless_runtime(runtime_kind):
            raise _headless_error(source=source)

    def _profile_auth_args(request: Request) -> Dict[str, Any]:
        if not websocket_tokens_active():
            return {}
        principal = get_principal(request)
        return {
            "caller_id": str(getattr(principal, "user_id", "") or "").strip(),
            "is_admin": bool(getattr(principal, "is_admin", False)),
        }

    def _profile_ref_args(*, scope: Optional[str] = None, owner_id: Optional[str] = None) -> Dict[str, Any]:
        out: Dict[str, Any] = {}
        normalized_scope = str(scope or "").strip().lower()
        normalized_owner = str(owner_id or "").strip()
        if normalized_scope:
            out["profile_scope"] = normalized_scope
        if normalized_owner:
            out["profile_owner"] = normalized_owner
        return out

    async def _filter_standard_profiles_response(resp: Dict[str, Any]) -> Dict[str, Any]:
        if await _developer_mode_enabled():
            return resp
        result = resp.get("result") if isinstance(resp, dict) else None
        profiles = result.get("profiles") if isinstance(result, dict) else None
        if isinstance(profiles, list):
            result["profiles"] = [
                item
                for item in profiles
                if isinstance(item, dict)
                and not _is_internal_headless_runtime(
                    {
                        "runner": str(item.get("runner") or "pty"),
                        "runtime": str(item.get("runtime") or "codex"),
                    }
                )
            ]
        return resp

    def _actor_or_404(group_id: str, actor_id: str) -> tuple[Any, Dict[str, Any]]:
        group = load_group(str(group_id or "").strip())
        if group is None:
            raise HTTPException(status_code=404, detail={"code": "group_not_found", "message": f"group not found: {group_id}"})
        actor = find_actor(group, str(actor_id or "").strip())
        if not isinstance(actor, dict):
            raise HTTPException(status_code=404, detail={"code": "actor_not_found", "message": f"actor not found: {actor_id}"})
        return group, actor

    def _decorate_actor_result(group_id: str, resp: Dict[str, Any]) -> Dict[str, Any]:
        if not bool(resp.get("ok")):
            return resp
        result = resp.get("result")
        if not isinstance(result, dict):
            return resp
        actor = result.get("actor")
        if isinstance(actor, dict):
            result["actor"] = build_actor_web_payload(group_id, actor)
        actors = result.get("actors")
        if isinstance(actors, list):
            result["actors"] = [build_actor_web_payload(group_id, item) for item in actors if isinstance(item, dict)]
        return resp

    async def _actor_profile_upsert_impl(
        request: Request,
        *,
        profile_payload: Dict[str, Any],
        by: str,
        expected_revision: Optional[int],
    ) -> Dict[str, Any]:
        if ctx.read_only:
            raise HTTPException(
                status_code=403,
                detail={
                    "code": "read_only",
                    "message": "Actor profile write endpoints are disabled in read-only (exhibit) mode.",
                },
            )
        if not await _developer_mode_enabled():
            if _is_internal_headless_runtime(
                {
                    "runner": str(profile_payload.get("runner") or "pty"),
                    "runtime": str(profile_payload.get("runtime") or "codex"),
                }
            ):
                raise _headless_error(source="actor_profile_upsert")
            if not _supports_standard_web_headless_runtime(profile_payload.get("runtime")):
                profile_payload["runner"] = "pty"
        args: Dict[str, Any] = {
            "profile": profile_payload,
            "by": by,
            **_profile_auth_args(request),
        }
        if expected_revision is not None:
            args["expected_revision"] = int(expected_revision)
        return await ctx.daemon({"op": "actor_profile_upsert", "args": args})

    @group_router.get("/actors")
    async def actors(group_id: str, include_unread: bool = False, include_internal: bool = False) -> Dict[str, Any]:
        gid = str(group_id or "").strip()

        async def _fetch() -> Dict[str, Any]:
            async def _fallback_local() -> Dict[str, Any]:
                return await run_in_threadpool(_read_actor_list_local, gid, include_unread=include_unread)

            try:
                return await ctx.daemon(
                    {
                        "op": "actor_list",
                        "args": {
                            "group_id": gid,
                            "include_unread": include_unread,
                            "include_internal": include_internal,
                        },
                    }
                )
            except HTTPException as exc:
                detail = exc.detail if isinstance(exc.detail, dict) else {}
                if int(exc.status_code or 0) == 503 and str(detail.get("code") or "") == "daemon_unavailable":
                    return await _fallback_local()
                raise
            except Exception:
                return await _fallback_local()

        cache_suffix = "unread_internal" if include_internal and include_unread else (
            "readonly_internal" if include_internal else ("unread" if include_unread else "readonly")
        )
        return _decorate_actor_result(gid, await _cached_actor_list(gid, _fetch, cache_suffix=cache_suffix))

    @group_router.post("/actors")
    async def actor_create(request: Request, group_id: str, req: ActorCreateRequest) -> Dict[str, Any]:
        await invalidate_readonly_actor_list(group_id)
        command = _normalize_command(req.command) or []
        env_private = dict(req.env_private) if isinstance(req.env_private, dict) else None
        profile_id = str(req.profile_id or "").strip()
        await _ensure_standard_web_runner(
            request,
            source="actor_create",
            runner=str(req.runner or "pty"),
            runtime=str(req.runtime or "codex"),
            profile_id=profile_id,
            profile_scope=str(req.profile_scope or ""),
            profile_owner=str(req.profile_owner or ""),
        )
        resp = await ctx.daemon(
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
                    **_profile_ref_args(scope=req.profile_scope, owner_id=req.profile_owner),
                    "default_scope_key": req.default_scope_key,
                    "submit": req.submit,
                    "by": req.by,
                    **_profile_auth_args(request),
                },
            }
        )
        if bool(resp.get("ok")):
            await invalidate_context_read(group_id)
        return _decorate_actor_result(group_id, resp)

    @group_router.post("/actors/{actor_id}")
    async def actor_update(request: Request, group_id: str, actor_id: str, req: ActorUpdateRequest) -> Dict[str, Any]:
        await invalidate_readonly_actor_list(group_id)
        await _ensure_standard_web_runner(
            request,
            source="actor_update",
            runner=str(req.runner or "") if req.runner is not None else None,
            runtime=str(req.runtime or "") if req.runtime is not None else None,
            profile_id=str(req.profile_id or "").strip(),
            profile_scope=str(req.profile_scope or ""),
            profile_owner=str(req.profile_owner or ""),
            group_id=group_id,
            actor_id=actor_id,
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
        if req.avatar_asset_path is not None:
            patch["avatar_asset_path"] = str(req.avatar_asset_path or "").strip()
        args: Dict[str, Any] = {
            "group_id": group_id,
            "actor_id": actor_id,
            "patch": patch,
            "by": req.by,
            **_profile_auth_args(request),
        }
        if req.profile_id is not None:
            args["profile_id"] = str(req.profile_id or "").strip()
            args.update(_profile_ref_args(scope=req.profile_scope, owner_id=req.profile_owner))
        if req.profile_action is not None:
            args["profile_action"] = str(req.profile_action or "").strip()
        resp = await ctx.daemon({"op": "actor_update", "args": args})
        return _decorate_actor_result(group_id, resp)

    @group_router.get("/actors/{actor_id}/avatar")
    async def actor_avatar_get(group_id: str, actor_id: str) -> FileResponse:
        _, actor = _actor_or_404(group_id, actor_id)
        rel_path = str(actor.get("avatar_asset_path") or "").strip()
        if not rel_path:
            raise HTTPException(status_code=404, detail={"code": "actor_avatar_not_found", "message": "actor avatar not found"})
        try:
            response = FileResponse(resolve_actor_avatar_path(rel_path))
            # Avatar URLs are versioned with ?v=<actor.updated_at>, so they are safe to cache aggressively.
            response.headers["Cache-Control"] = "private, max-age=31536000, immutable"
            return response
        except Exception as exc:
            raise HTTPException(status_code=404, detail={"code": "actor_avatar_not_found", "message": "actor avatar not found"}) from exc

    @group_router.post("/actors/{actor_id}/avatar")
    async def actor_avatar_upload(
        request: Request,
        group_id: str,
        actor_id: str,
        by: str = Form("user"),
        file: UploadFile = File(...),
    ) -> Dict[str, Any]:
        if ctx.read_only:
            raise HTTPException(
                status_code=403,
                detail={
                    "code": "read_only",
                    "message": "Actor avatar write endpoints are disabled in read-only (exhibit) mode.",
                },
            )
        await invalidate_readonly_actor_list(group_id)
        _, actor = _actor_or_404(group_id, actor_id)
        old_rel_path = str(actor.get("avatar_asset_path") or "").strip()
        raw_bytes = await file.read()
        try:
            stored = store_actor_avatar(
                group_id=group_id,
                data=raw_bytes,
                content_type=str(getattr(file, "content_type", "") or ""),
                filename=str(getattr(file, "filename", "") or ""),
            )
        except ValueError as exc:
            message = str(exc)
            status_code = 413 if "too large" in message else 400
            raise HTTPException(status_code=status_code, detail={"code": "invalid_request", "message": message}) from exc
        resp = await ctx.daemon(
            {
                "op": "actor_update",
                "args": {
                    "group_id": group_id,
                    "actor_id": actor_id,
                    "patch": {"avatar_asset_path": str(stored.get("rel_path") or "")},
                    "by": str(by or "user"),
                    **_profile_auth_args(request),
                },
            }
        )
        if not bool(resp.get("ok")):
            delete_actor_avatar(str(stored.get("rel_path") or ""))
            return resp
        new_rel_path = str(stored.get("rel_path") or "").strip()
        if old_rel_path and old_rel_path != new_rel_path:
            delete_actor_avatar(old_rel_path)
        return _decorate_actor_result(group_id, resp)

    @group_router.delete("/actors/{actor_id}/avatar")
    async def actor_avatar_clear(request: Request, group_id: str, actor_id: str, by: str = "user") -> Dict[str, Any]:
        if ctx.read_only:
            raise HTTPException(
                status_code=403,
                detail={
                    "code": "read_only",
                    "message": "Actor avatar write endpoints are disabled in read-only (exhibit) mode.",
                },
            )
        await invalidate_readonly_actor_list(group_id)
        _, actor = _actor_or_404(group_id, actor_id)
        old_rel_path = str(actor.get("avatar_asset_path") or "").strip()
        resp = await ctx.daemon(
            {
                "op": "actor_update",
                "args": {
                    "group_id": group_id,
                    "actor_id": actor_id,
                    "patch": {"avatar_asset_path": ""},
                    "by": str(by or "user"),
                    **_profile_auth_args(request),
                },
            }
        )
        if not bool(resp.get("ok")):
            return resp
        if old_rel_path:
            delete_actor_avatar(old_rel_path)
        return _decorate_actor_result(group_id, resp)

    @group_router.delete("/actors/{actor_id}")
    async def actor_delete(group_id: str, actor_id: str, by: str = "user") -> Dict[str, Any]:
        await invalidate_readonly_actor_list(group_id)
        old_rel_path = ""
        try:
            _, actor = _actor_or_404(group_id, actor_id)
            old_rel_path = str(actor.get("avatar_asset_path") or "").strip()
        except HTTPException:
            old_rel_path = ""
        resp = await ctx.daemon({"op": "actor_remove", "args": {"group_id": group_id, "actor_id": actor_id, "by": by}})
        if bool(resp.get("ok")):
            if old_rel_path:
                delete_actor_avatar(old_rel_path)
            await invalidate_context_read(group_id)
        return resp

    @group_router.post("/actors/{actor_id}/start")
    async def actor_start(request: Request, group_id: str, actor_id: str, by: str = "user") -> Dict[str, Any]:
        await invalidate_readonly_actor_list(group_id)
        if not await _developer_mode_enabled() and _is_internal_headless_runtime(await _actor_runtime_meta(group_id, actor_id)):
            raise _headless_error(source="actor_start")
        return await ctx.daemon(
            {
                "op": "actor_start",
                "args": {"group_id": group_id, "actor_id": actor_id, "by": by, **_profile_auth_args(request)},
            }
        )

    @group_router.post("/actors/{actor_id}/stop")
    async def actor_stop(group_id: str, actor_id: str, by: str = "user") -> Dict[str, Any]:
        await invalidate_readonly_actor_list(group_id)
        return await ctx.daemon({"op": "actor_stop", "args": {"group_id": group_id, "actor_id": actor_id, "by": by}})

    @group_router.post("/actors/{actor_id}/restart")
    async def actor_restart(request: Request, group_id: str, actor_id: str, by: str = "user") -> Dict[str, Any]:
        await invalidate_readonly_actor_list(group_id)
        if not await _developer_mode_enabled() and _is_internal_headless_runtime(await _actor_runtime_meta(group_id, actor_id)):
            raise _headless_error(source="actor_restart")
        return await ctx.daemon(
            {
                "op": "actor_restart",
                "args": {"group_id": group_id, "actor_id": actor_id, "by": by, **_profile_auth_args(request)},
            }
        )

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

    @global_router.get("/actor_profiles", dependencies=[Depends(require_user)])
    @global_router.get("/profiles", dependencies=[Depends(require_user)])
    async def actor_profiles_list(request: Request, by: str = "user", view: str = "global") -> Dict[str, Any]:
        resp = await ctx.daemon(
            {
                "op": "actor_profile_list",
                "args": {"by": by, "view": view, **_profile_auth_args(request)},
            }
        )
        return await _filter_standard_profiles_response(resp)

    @global_router.get("/actor_profiles/{profile_id}", dependencies=[Depends(require_user)])
    @global_router.get("/profiles/{profile_id}", dependencies=[Depends(require_user)])
    async def actor_profiles_get(
        request: Request,
        profile_id: str,
        by: str = "user",
        scope: str = "global",
        owner_id: str = "",
    ) -> Dict[str, Any]:
        resp = await ctx.daemon(
            {
                "op": "actor_profile_get",
                "args": {
                    "profile_id": profile_id,
                    "by": by,
                    **_profile_ref_args(scope=scope, owner_id=owner_id),
                    **_profile_auth_args(request),
                },
            }
        )
        if not await _developer_mode_enabled():
            profile = (resp.get("result") or {}).get("profile") if isinstance(resp, dict) else None
            if isinstance(profile, dict) and _runner_is_headless(profile.get("runner")):
                raise _headless_error(source="actor_profile_get")
        return resp

    @global_router.post("/actor_profiles", dependencies=[Depends(require_user)])
    async def actor_profiles_upsert(request: Request, req: ActorProfileUpsertRequest) -> Dict[str, Any]:
        profile_payload = dict(req.profile or {})
        return await _actor_profile_upsert_impl(
            request,
            profile_payload=profile_payload,
            by=req.by,
            expected_revision=req.expected_revision,
        )

    @global_router.put("/profiles/{profile_id}", dependencies=[Depends(require_user)])
    async def profiles_put(request: Request, profile_id: str) -> Dict[str, Any]:
        try:
            payload = await request.json()
        except Exception:
            raise HTTPException(status_code=400, detail={"code": "invalid_request", "message": "invalid JSON body", "details": {}})
        if not isinstance(payload, dict):
            raise HTTPException(status_code=400, detail={"code": "invalid_request", "message": "request body must be an object", "details": {}})

        by = str(payload.get("by") or "user").strip() or "user"
        expected_revision_raw = payload.get("expected_revision")
        expected_revision: Optional[int] = None
        if expected_revision_raw is not None:
            try:
                expected_revision = int(expected_revision_raw)
            except Exception:
                raise HTTPException(status_code=400, detail={"code": "invalid_request", "message": "expected_revision must be an integer", "details": {}})

        profile_payload = payload.get("profile") if isinstance(payload.get("profile"), dict) else dict(payload)
        if not isinstance(profile_payload, dict):
            raise HTTPException(status_code=400, detail={"code": "invalid_request", "message": "profile must be an object", "details": {}})
        profile_payload = dict(profile_payload)
        profile_payload["id"] = profile_id
        profile_payload.pop("expected_revision", None)
        profile_payload.pop("by", None)
        return await _actor_profile_upsert_impl(
            request,
            profile_payload=profile_payload,
            by=by,
            expected_revision=expected_revision,
        )

    @global_router.delete("/actor_profiles/{profile_id}", dependencies=[Depends(require_user)])
    @global_router.delete("/profiles/{profile_id}", dependencies=[Depends(require_user)])
    async def actor_profiles_delete(
        request: Request,
        profile_id: str,
        by: str = "user",
        force_detach: bool = False,
        scope: str = "global",
        owner_id: str = "",
    ) -> Dict[str, Any]:
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
                "args": {
                    "profile_id": profile_id,
                    "by": by,
                    "force_detach": bool(force_detach),
                    **_profile_ref_args(scope=scope, owner_id=owner_id),
                    **_profile_auth_args(request),
                },
            }
        )

    async def _actor_profile_secret_keys_impl(
        request: Request,
        *,
        profile_id: str,
        by: str,
        scope: str,
        owner_id: str,
    ) -> Dict[str, Any]:
        return await ctx.daemon(
            {
                "op": "actor_profile_secret_keys",
                "args": {
                    "profile_id": profile_id,
                    "by": by,
                    **_profile_ref_args(scope=scope, owner_id=owner_id),
                    **_profile_auth_args(request),
                },
            }
        )

    @global_router.get("/actor_profiles/{profile_id}/env_private", dependencies=[Depends(require_admin)])
    async def actor_profile_secret_keys_legacy(request: Request, profile_id: str, by: str = "user") -> Dict[str, Any]:
        return await _actor_profile_secret_keys_impl(request, profile_id=profile_id, by=by, scope="global", owner_id="")

    @global_router.get("/profiles/{profile_id}/env_private", dependencies=[Depends(require_user)])
    async def actor_profile_secret_keys(
        request: Request,
        profile_id: str,
        by: str = "user",
        scope: str = "global",
        owner_id: str = "",
    ) -> Dict[str, Any]:
        return await _actor_profile_secret_keys_impl(request, profile_id=profile_id, by=by, scope=scope, owner_id=owner_id)

    async def _actor_profile_secret_update_impl(
        request: Request,
        *,
        profile_id: str,
        default_scope: str,
        default_owner_id: str,
    ) -> Dict[str, Any]:
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
        scope = str(payload.get("scope") or default_scope or "global").strip() or "global"
        owner_id = str(payload.get("owner_id") or default_owner_id or "").strip()
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
                    **_profile_ref_args(scope=scope, owner_id=owner_id),
                    **_profile_auth_args(request),
                    "set": set_vars,
                    "unset": unset_keys,
                    "clear": clear,
                },
            }
        )

    @global_router.post("/actor_profiles/{profile_id}/env_private", dependencies=[Depends(require_admin)])
    async def actor_profile_secret_update_legacy(request: Request, profile_id: str) -> Dict[str, Any]:
        return await _actor_profile_secret_update_impl(
            request,
            profile_id=profile_id,
            default_scope="global",
            default_owner_id="",
        )

    @global_router.post("/profiles/{profile_id}/env_private", dependencies=[Depends(require_user)])
    async def actor_profile_secret_update(request: Request, profile_id: str) -> Dict[str, Any]:
        return await _actor_profile_secret_update_impl(
            request,
            profile_id=profile_id,
            default_scope="global",
            default_owner_id="",
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

    async def _actor_profile_secret_copy_from_profile_impl(
        request: Request,
        *,
        profile_id: str,
        default_scope: str,
        default_owner_id: str,
    ) -> Dict[str, Any]:
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
        scope = str(payload.get("scope") or default_scope or "global").strip() or "global"
        owner_id = str(payload.get("owner_id") or default_owner_id or "").strip()
        source_scope = str(payload.get("source_scope") or payload.get("source_profile_scope") or scope or "global").strip() or "global"
        source_owner_id = str(payload.get("source_owner_id") or payload.get("source_profile_owner") or owner_id or "").strip()
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
                    **_profile_ref_args(scope=scope, owner_id=owner_id),
                    "source_profile_scope": source_scope,
                    "source_profile_owner": source_owner_id,
                    **_profile_auth_args(request),
                },
            }
        )

    @global_router.post("/actor_profiles/{profile_id}/copy_profile_secrets", dependencies=[Depends(require_admin)])
    async def actor_profile_secret_copy_from_profile_legacy(request: Request, profile_id: str) -> Dict[str, Any]:
        return await _actor_profile_secret_copy_from_profile_impl(
            request,
            profile_id=profile_id,
            default_scope="global",
            default_owner_id="",
        )

    @global_router.post("/profiles/{profile_id}/copy_profile_secrets", dependencies=[Depends(require_user)])
    async def actor_profile_secret_copy_from_profile(request: Request, profile_id: str) -> Dict[str, Any]:
        return await _actor_profile_secret_copy_from_profile_impl(
            request,
            profile_id=profile_id,
            default_scope="global",
            default_owner_id="",
        )

    @global_router.websocket("/groups/{group_id}/actors/{actor_id}/term")
    async def actor_terminal(websocket: WebSocket, group_id: str, actor_id: str) -> None:
        # Accept WebSocket first — closing before accept violates the protocol
        # and causes the browser to see code 1006 instead of our intended close code.
        await websocket.accept()

        async def _safe_send_json(payload: Dict[str, Any]) -> bool:
            try:
                await websocket.send_json(payload)
                return True
            except (WebSocketDisconnect, RuntimeError):
                return False
            except Exception:
                return False

        async def _safe_close(code: int) -> None:
            try:
                await websocket.close(code=code)
            except Exception:
                pass

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
            await _safe_send_json({"ok": False, "error": {"code": "auth_required", "message": "Invalid or missing authentication token"}})
            await _safe_close(4401)
            return

        try:
            check_group(websocket, group_id)
        except HTTPException as exc:
            detail = exc.detail if isinstance(exc.detail, dict) else {"code": "permission_denied", "message": str(exc.detail or "permission denied")}
            await _safe_send_json({"ok": False, "error": detail})
            await _safe_close(1008)
            return

        if ctx.read_only and not ctx.exhibit_allow_terminal:
            await _safe_send_json(
                {
                    "ok": False,
                    "error": {
                        "code": "read_only_terminal",
                        "message": "Terminal is disabled in read-only (exhibit) mode.",
                        "details": {},
                    },
                }
            )
            await _safe_close(1000)
            return

        group = load_group(group_id)
        if group is None:
            await _safe_send_json({"ok": False, "error": {"code": "group_not_found", "message": f"group not found: {group_id}"}})
            await _safe_close(1008)
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
            await _safe_send_json({"ok": False, "error": {"code": "daemon_unavailable", "message": "ccccd unavailable"}})
            await _safe_close(1011)
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
                await _safe_send_json({"ok": False, "error": err})
                await _safe_close(1008)
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
