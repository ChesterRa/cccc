from __future__ import annotations

from pathlib import Path
from typing import Any, Dict

from fastapi import FastAPI, HTTPException, Request
from fastapi.responses import FileResponse, HTMLResponse

from ....kernel.scope import detect_scope
from ..schemas import (
    DebugClearLogsRequest,
    ObservabilityUpdateRequest,
    RegistryReconcileRequest,
    RemoteAccessConfigureRequest,
    RouteContext,
)


def register_base_routes(app: FastAPI, *, ctx: RouteContext) -> None:
    @app.get("/", response_class=HTMLResponse)
    async def index() -> str:
        if ctx.dist_dir is not None:
            return '<meta http-equiv="refresh" content="0; url=/ui/">'
        return (
            "<h3>cccc web</h3>"
            "<p>This is a minimal control-plane port. UI will live under <code>/ui</code> later.</p>"
            "<p>Try <code>/api/v1/ping</code> and <code>/api/v1/groups</code>.</p>"
        )

    @app.get("/favicon.ico")
    async def favicon_ico() -> Any:
        if ctx.dist_dir is not None and (ctx.dist_dir / "favicon.ico").exists():
            return FileResponse(ctx.dist_dir / "favicon.ico")
        raise HTTPException(status_code=404)

    @app.get("/favicon.png")
    async def favicon_png() -> Any:
        if ctx.dist_dir is not None and (ctx.dist_dir / "favicon.png").exists():
            return FileResponse(ctx.dist_dir / "favicon.png")
        raise HTTPException(status_code=404)

    @app.get("/api/v1/ping")
    async def ping() -> Dict[str, Any]:
        resp = await ctx.daemon({"op": "ping"})
        return {
            "ok": True,
            "result": {
                "home": str(ctx.home),
                "daemon": resp.get("result", {}),
                "version": ctx.version,
                "web": {"mode": ctx.web_mode, "read_only": ctx.read_only},
            },
        }

    @app.get("/api/v1/health")
    async def health() -> Dict[str, Any]:
        """Health check endpoint for monitoring."""
        daemon_resp = await ctx.daemon({"op": "ping"})
        daemon_ok = daemon_resp.get("ok", False)

        return {
            "ok": daemon_ok,
            "result": {
                "version": ctx.version,
                "home": str(ctx.home),
                "daemon": "running" if daemon_ok else "stopped",
            }
        }

    @app.get("/api/v1/observability")
    async def observability_get() -> Dict[str, Any]:
        """Get global observability settings (developer mode, log level)."""
        return await ctx.daemon({"op": "observability_get"})

    @app.get("/api/v1/capabilities/allowlist")
    async def capability_allowlist_get(by: str = "user") -> Dict[str, Any]:
        """Get effective capability allowlist (default + overlay + merge result)."""
        return await ctx.daemon({"op": "capability_allowlist_get", "args": {"by": str(by or "user")}})

    @app.post("/api/v1/capabilities/allowlist/validate")
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

    @app.put("/api/v1/capabilities/allowlist")
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

    @app.delete("/api/v1/capabilities/allowlist")
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

    @app.get("/api/v1/capabilities/overview")
    async def capability_overview(
        query: str = "",
        limit: int = 400,
        include_indexed: bool = True,
    ) -> Dict[str, Any]:
        """Get global capability overview (policy + blocked + recent-success + source states)."""
        args = {
            "query": str(query or ""),
            "limit": int(limit or 400),
            "include_indexed": bool(include_indexed),
        }
        return await ctx.daemon({"op": "capability_overview", "args": args})

    @app.post("/api/v1/capabilities/block")
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

    @app.put("/api/v1/observability")
    async def observability_update(req: ObservabilityUpdateRequest) -> Dict[str, Any]:
        """Update global observability settings (daemon-owned persistence)."""
        patch: Dict[str, Any] = {}
        if req.developer_mode is not None:
            patch["developer_mode"] = bool(req.developer_mode)
        if req.log_level is not None:
            patch["log_level"] = str(req.log_level or "").strip().upper()
        if req.terminal_transcript_per_actor_bytes is not None:
            patch.setdefault("terminal_transcript", {})["per_actor_bytes"] = int(req.terminal_transcript_per_actor_bytes)
        if req.terminal_ui_scrollback_lines is not None:
            patch.setdefault("terminal_ui", {})["scrollback_lines"] = int(req.terminal_ui_scrollback_lines)

        resp = await ctx.daemon({"op": "observability_update", "args": {"by": req.by, "patch": patch}})

        # Apply web-side logging immediately as well (best-effort).
        try:
            obs = (resp.get("result") or {}).get("observability") if resp.get("ok") else None
            if isinstance(obs, dict):
                level = str(obs.get("log_level") or "INFO").strip().upper() or "INFO"
                if obs.get("developer_mode") and level == "INFO":
                    level = "DEBUG"
                ctx.apply_web_logging(home=ctx.home, level=level)
        except Exception:
            pass

        return resp

    @app.get("/api/v1/remote_access")
    async def remote_access_get() -> Dict[str, Any]:
        """Get global remote-access state."""
        return await ctx.daemon({"op": "remote_access_state", "args": {"by": "user"}})

    @app.put("/api/v1/remote_access")
    async def remote_access_configure(req: RemoteAccessConfigureRequest) -> Dict[str, Any]:
        """Update global remote-access config."""
        args: Dict[str, Any] = {"by": str(req.by or "user")}
        if req.provider is not None:
            args["provider"] = str(req.provider)
        if req.mode is not None:
            args["mode"] = str(req.mode or "").strip()
        if req.enforce_web_token is not None:
            args["enforce_web_token"] = bool(req.enforce_web_token)
        if req.web_host is not None:
            args["web_host"] = str(req.web_host or "").strip()
        if req.web_port is not None:
            args["web_port"] = int(req.web_port)
        if req.web_public_url is not None:
            args["web_public_url"] = str(req.web_public_url or "").strip()
        if req.clear_web_token:
            args["clear_web_token"] = True
        elif req.web_token is not None:
            args["web_token"] = str(req.web_token or "").strip()
        return await ctx.daemon({"op": "remote_access_configure", "args": args})

    @app.post("/api/v1/remote_access/start")
    async def remote_access_start(by: str = "user") -> Dict[str, Any]:
        """Start remote access service."""
        return await ctx.daemon({"op": "remote_access_start", "args": {"by": str(by or "user")}})

    @app.post("/api/v1/remote_access/stop")
    async def remote_access_stop(by: str = "user") -> Dict[str, Any]:
        """Stop remote access service."""
        return await ctx.daemon({"op": "remote_access_stop", "args": {"by": str(by or "user")}})

    @app.get("/api/v1/registry/reconcile")
    async def registry_reconcile_preview() -> Dict[str, Any]:
        """Preview registry health (missing/corrupt groups) without mutating registry."""
        return await ctx.daemon({"op": "registry_reconcile", "args": {"remove_missing": False, "by": "user"}})

    @app.post("/api/v1/registry/reconcile")
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

    # ---------------------------------------------------------------------
    # Terminal transcript endpoints (group-scoped)
    # ---------------------------------------------------------------------

    @app.get("/api/v1/groups/{group_id}/terminal/tail")
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

    @app.post("/api/v1/groups/{group_id}/terminal/clear")
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

    # ---------------------------------------------------------------------
    # Debug endpoints (developer mode only; gated by daemon)
    # ---------------------------------------------------------------------

    @app.get("/api/v1/debug/snapshot")
    async def debug_snapshot(group_id: str) -> Dict[str, Any]:
        """Get a structured debug snapshot for a group (developer mode only)."""
        return await ctx.daemon({"op": "debug_snapshot", "args": {"group_id": group_id, "by": "user"}})

    @app.get("/api/v1/debug/tail_logs")
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

    @app.post("/api/v1/debug/clear_logs")
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

    @app.get("/api/v1/runtimes")
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
                        "command": rt.command,
                        "recommended_command": " ".join(get_runtime_command_with_flags(rt.name)),
                        "available": rt.available,
                        "path": rt.path,
                        "capabilities": rt.capabilities,
                    }
                    for rt in all_runtimes
                ],
                "available": [rt.name for rt in all_runtimes if rt.available],
            },
        }

    @app.get("/api/v1/fs/list")
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

    @app.get("/api/v1/fs/recent")
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

    @app.get("/api/v1/fs/scope_root")
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
