from __future__ import annotations

import asyncio
import json
import os
from typing import Any, Dict, Optional

from fastapi import FastAPI, HTTPException

from ....kernel.group import load_group
from ....ports.im.config_schema import canonicalize_im_config
from ....util.conv import coerce_bool
from ..schemas import (
    IMActionRequest,
    IMBindRequest,
    IMPendingRejectRequest,
    IMSetRequest,
    InboxReadRequest,
    RouteContext,
)


def register_im_routes(app: FastAPI, *, ctx: RouteContext) -> None:
    @app.get("/api/v1/groups/{group_id}/inbox/{actor_id}")
    async def inbox_list(group_id: str, actor_id: str, by: str = "user", limit: int = 50) -> Dict[str, Any]:
        return await ctx.daemon({"op": "inbox_list", "args": {"group_id": group_id, "actor_id": actor_id, "by": by, "limit": int(limit)}})

    @app.post("/api/v1/groups/{group_id}/inbox/{actor_id}/read")
    async def inbox_mark_read(group_id: str, actor_id: str, req: InboxReadRequest) -> Dict[str, Any]:
        return await ctx.daemon(
            {"op": "inbox_mark_read", "args": {"group_id": group_id, "actor_id": actor_id, "event_id": req.event_id, "by": req.by}}
        )

    @app.post("/api/v1/groups/{group_id}/start")
    async def group_start(group_id: str, by: str = "user") -> Dict[str, Any]:
        return await ctx.daemon({"op": "group_start", "args": {"group_id": group_id, "by": by}})

    @app.post("/api/v1/groups/{group_id}/stop")
    async def group_stop(group_id: str, by: str = "user") -> Dict[str, Any]:
        return await ctx.daemon({"op": "group_stop", "args": {"group_id": group_id, "by": by}})

    @app.post("/api/v1/groups/{group_id}/state")
    async def group_set_state(group_id: str, state: str, by: str = "user") -> Dict[str, Any]:
        """Set group state (active/idle/paused) to control automation behavior."""
        return await ctx.daemon({"op": "group_set_state", "args": {"group_id": group_id, "state": state, "by": by}})

    # =========================================================================
    # IM Bridge API
    # =========================================================================

    @app.get("/api/im/status")
    async def im_status(group_id: str) -> Dict[str, Any]:
        """Get IM bridge status for a group."""
        group = load_group(group_id)
        if group is None:
            raise HTTPException(status_code=404, detail={"code": "group_not_found", "message": f"group not found: {group_id}"})

        im_config = canonicalize_im_config(group.doc.get("im", {}))
        platform = im_config.get("platform") if im_config else None

        # Check if running
        pid_path = group.path / "state" / "im_bridge.pid"
        pid = None
        running = False
        if pid_path.exists():
            try:
                pid = int(pid_path.read_text(encoding="utf-8").strip())
                # Reap if this process started the bridge and it already exited.
                try:
                    waited_pid, _ = os.waitpid(pid, os.WNOHANG)
                    if waited_pid == pid:
                        pid = None
                        pid_path.unlink(missing_ok=True)
                    else:
                        os.kill(pid, 0)  # Check if process exists
                        running = True
                except (AttributeError, ChildProcessError):
                    os.kill(pid, 0)  # Check if process exists
                    running = True
            except (ValueError, ProcessLookupError, PermissionError):
                pid = None

        # Get subscriber count
        subscribers_path = group.path / "state" / "im_subscribers.json"
        subscriber_count = 0
        if subscribers_path.exists():
            try:
                subs = json.loads(subscribers_path.read_text(encoding="utf-8"))
                subscriber_count = sum(1 for s in subs.values() if isinstance(s, dict) and s.get("subscribed"))
            except Exception:
                pass

        return {
            "ok": True,
            "result": {
                "group_id": group_id,
                "configured": bool(im_config),
                "platform": platform,
                "running": running,
                "pid": pid,
                "subscribers": subscriber_count,
            }
        }

    @app.get("/api/im/config")
    async def im_config(group_id: str) -> Dict[str, Any]:
        """Get IM bridge configuration for a group."""
        group = load_group(group_id)
        if group is None:
            raise HTTPException(status_code=404, detail={"code": "group_not_found", "message": f"group not found: {group_id}"})

        im_cfg = canonicalize_im_config(group.doc.get("im"))
        return {"ok": True, "result": {"group_id": group_id, "im": im_cfg}}

    @app.post("/api/im/set")
    async def im_set(req: IMSetRequest) -> Dict[str, Any]:
        """Set IM bridge configuration for a group."""
        group = load_group(req.group_id)
        if group is None:
            raise HTTPException(status_code=404, detail={"code": "group_not_found", "message": f"group not found: {req.group_id}"})

        prev_im = group.doc.get("im") if isinstance(group.doc.get("im"), dict) else {}
        prev_enabled = coerce_bool(prev_im.get("enabled"), default=False) if isinstance(prev_im, dict) else False

        # Build IM config draft then canonicalize to keep storage shape stable.
        im_cfg: Dict[str, Any] = {"platform": str(req.platform or "").strip().lower()}
        if prev_enabled:
            im_cfg["enabled"] = True

        platform = str(im_cfg.get("platform") or "").strip().lower()
        prev_files = prev_im.get("files") if isinstance(prev_im, dict) else None
        if isinstance(prev_files, dict):
            # Preserve non-credential settings, if any (so "Set" doesn't silently drop them).
            im_cfg["files"] = prev_files
        else:
            # Default file-transfer policy (also used by CLI).
            default_max_mb = 20 if platform in ("telegram", "slack") else 10
            im_cfg["files"] = {"enabled": True, "max_mb": default_max_mb}

        if isinstance(prev_im, dict) and "skip_pending_on_start" in prev_im:
            im_cfg["skip_pending_on_start"] = coerce_bool(prev_im.get("skip_pending_on_start"), default=True)

        token_hint = str(req.bot_token_env or req.token_env or req.token or "").strip()

        if platform in ("telegram", "discord", "slack"):
            if token_hint:
                im_cfg["bot_token_env"] = token_hint
            app_hint = str(req.app_token_env or "").strip()
            if platform == "slack" and app_hint:
                im_cfg["app_token_env"] = app_hint
        elif platform == "feishu":
            im_cfg["feishu_domain"] = str(req.feishu_domain or "").strip()
            app_id = str(req.feishu_app_id or "").strip()
            app_secret = str(req.feishu_app_secret or "").strip()
            if app_id:
                im_cfg["feishu_app_id"] = app_id
            if app_secret:
                im_cfg["feishu_app_secret"] = app_secret
        elif platform == "dingtalk":
            app_key = str(req.dingtalk_app_key or "").strip()
            app_secret = str(req.dingtalk_app_secret or "").strip()
            robot_code = str(req.dingtalk_robot_code or "").strip()
            if app_key:
                im_cfg["dingtalk_app_key"] = app_key
            if app_secret:
                im_cfg["dingtalk_app_secret"] = app_secret
            if robot_code:
                im_cfg["dingtalk_robot_code"] = robot_code

        im_cfg = canonicalize_im_config(im_cfg)

        # Update group doc and save
        group.doc["im"] = im_cfg
        group.save()

        return {"ok": True, "result": {"group_id": req.group_id, "im": im_cfg}}

    @app.post("/api/im/unset")
    async def im_unset(req: IMActionRequest) -> Dict[str, Any]:
        """Remove IM bridge configuration from a group."""
        group = load_group(req.group_id)
        if group is None:
            raise HTTPException(status_code=404, detail={"code": "group_not_found", "message": f"group not found: {req.group_id}"})

        if "im" in group.doc:
            del group.doc["im"]
            group.save()

        return {"ok": True, "result": {"group_id": req.group_id, "im": None}}

    @app.post("/api/im/start")
    async def im_start(req: IMActionRequest) -> Dict[str, Any]:
        """Start IM bridge for a group."""
        import subprocess

        group = load_group(req.group_id)
        if group is None:
            raise HTTPException(status_code=404, detail={"code": "group_not_found", "message": f"group not found: {req.group_id}"})

        # Check if already running
        pid_path = group.path / "state" / "im_bridge.pid"
        if pid_path.exists():
            try:
                pid = int(pid_path.read_text(encoding="utf-8").strip())
                # If it's our child and already exited, reap and allow restart.
                try:
                    waited_pid, _ = os.waitpid(pid, os.WNOHANG)
                    if waited_pid == pid:
                        pid_path.unlink(missing_ok=True)
                    else:
                        os.kill(pid, 0)
                        return {"ok": False, "error": {"code": "already_running", "message": f"bridge already running (pid={pid})"}}
                except (AttributeError, ChildProcessError):
                    os.kill(pid, 0)
                    return {"ok": False, "error": {"code": "already_running", "message": f"bridge already running (pid={pid})"}}
            except (ValueError, ProcessLookupError, PermissionError):
                pass

        # Check IM config
        im_cfg = canonicalize_im_config(group.doc.get("im", {}))
        if not im_cfg:
            return {"ok": False, "error": {"code": "no_im_config", "message": "no IM configuration"}}

        # Persist desired run-state for restart/autostart.
        im_cfg["enabled"] = True
        group.doc["im"] = im_cfg
        group.save()

        platform = im_cfg.get("platform", "telegram")

        # Prepare environment
        env = os.environ.copy()

        if platform == "feishu":
            # Feishu: set FEISHU_APP_ID and FEISHU_APP_SECRET
            app_id = im_cfg.get("feishu_app_id") or ""
            app_secret = im_cfg.get("feishu_app_secret") or ""
            app_id_env = im_cfg.get("feishu_app_id_env") or ""
            app_secret_env = im_cfg.get("feishu_app_secret_env") or ""
            # Set actual values to default env var names
            if app_id:
                env["FEISHU_APP_ID"] = app_id
            if app_secret:
                env["FEISHU_APP_SECRET"] = app_secret
            # Also set to custom env var names if specified
            if app_id_env and app_id:
                env[app_id_env] = app_id
            if app_secret_env and app_secret:
                env[app_secret_env] = app_secret
        elif platform == "dingtalk":
            # DingTalk: set DINGTALK_APP_KEY, DINGTALK_APP_SECRET, DINGTALK_ROBOT_CODE
            app_key = im_cfg.get("dingtalk_app_key") or ""
            app_secret = im_cfg.get("dingtalk_app_secret") or ""
            robot_code = im_cfg.get("dingtalk_robot_code") or ""
            app_key_env = im_cfg.get("dingtalk_app_key_env") or ""
            app_secret_env = im_cfg.get("dingtalk_app_secret_env") or ""
            robot_code_env = im_cfg.get("dingtalk_robot_code_env") or ""
            # Set actual values to default env var names
            if app_key:
                env["DINGTALK_APP_KEY"] = app_key
            if app_secret:
                env["DINGTALK_APP_SECRET"] = app_secret
            if robot_code:
                env["DINGTALK_ROBOT_CODE"] = robot_code
            # Also set to custom env var names if specified
            if app_key_env and app_key:
                env[app_key_env] = app_key
            if app_secret_env and app_secret:
                env[app_secret_env] = app_secret
            if robot_code_env and robot_code:
                env[robot_code_env] = robot_code
        else:
            # Telegram/Slack/Discord: token-based
            bot_token_env = str(im_cfg.get("bot_token_env") or "").strip()
            bot_token = str(im_cfg.get("bot_token") or "").strip()
            if bot_token and bot_token_env:
                env[bot_token_env] = bot_token
            elif bot_token:
                default_env = {"telegram": "TELEGRAM_BOT_TOKEN", "slack": "SLACK_BOT_TOKEN", "discord": "DISCORD_BOT_TOKEN"}
                env[default_env.get(platform, "BOT_TOKEN")] = bot_token
            if platform == "slack":
                app_token_env = str(im_cfg.get("app_token_env") or "").strip()
                app_token = str(im_cfg.get("app_token") or "").strip()
                if app_token and app_token_env:
                    env[app_token_env] = app_token

        # Start bridge as subprocess
        state_dir = group.path / "state"
        state_dir.mkdir(parents=True, exist_ok=True)
        log_path = state_dir / "im_bridge.log"

        try:
            import sys
            log_file = log_path.open("a", encoding="utf-8")
            proc = subprocess.Popen(
                [sys.executable, "-m", "cccc.ports.im.bridge", req.group_id, platform],
                env=env,
                stdout=log_file,
                stderr=log_file,
                start_new_session=True,
            )
            # If the process exits immediately (common for missing token/deps), report failure.
            await asyncio.sleep(0.25)
            exit_code = proc.poll()
            if exit_code is not None:
                try:
                    proc.wait(timeout=0.1)
                except Exception:
                    pass
                return {
                    "ok": False,
                    "error": {
                        "code": "bridge_exited",
                        "message": f"bridge exited early (code={exit_code}). Check log: {log_path}",
                    },
                }

            pid_path.write_text(str(proc.pid), encoding="utf-8")
            return {"ok": True, "result": {"group_id": req.group_id, "platform": platform, "pid": proc.pid}}
        except Exception as e:
            return {"ok": False, "error": {"code": "start_failed", "message": str(e)}}

    @app.post("/api/im/stop")
    async def im_stop(req: IMActionRequest) -> Dict[str, Any]:
        """Stop IM bridge for a group."""
        import signal as sig

        group = load_group(req.group_id)
        if group is None:
            raise HTTPException(status_code=404, detail={"code": "group_not_found", "message": f"group not found: {req.group_id}"})

        # Persist desired run-state for restart/autostart.
        raw_im_cfg = group.doc.get("im")
        if isinstance(raw_im_cfg, dict):
            im_cfg = canonicalize_im_config(raw_im_cfg)
            im_cfg["enabled"] = False
            group.doc["im"] = im_cfg
            try:
                group.save()
            except Exception:
                pass

        stopped = 0
        pid_path = group.path / "state" / "im_bridge.pid"

        if pid_path.exists():
            try:
                pid = int(pid_path.read_text(encoding="utf-8").strip())
                try:
                    os.killpg(os.getpgid(pid), sig.SIGTERM)
                except Exception:
                    try:
                        os.kill(pid, sig.SIGTERM)
                    except Exception:
                        pass
                stopped += 1
            except Exception:
                pass
            try:
                pid_path.unlink(missing_ok=True)
            except Exception:
                pass

        return {"ok": True, "result": {"group_id": req.group_id, "stopped": stopped}}

    # ----- IM auth (bind / pending / list / revoke) -----

    @app.post("/api/im/bind")
    async def im_bind(req: Optional[IMBindRequest] = None, group_id: str = "", key: str = "") -> Dict[str, Any]:
        """Bind a pending authorization key to authorize an IM chat."""
        gid = str((req.group_id if isinstance(req, IMBindRequest) else group_id) or "").strip()
        k = str((req.key if isinstance(req, IMBindRequest) else key) or "").strip()
        if not gid:
            raise HTTPException(status_code=400, detail={"code": "missing_group_id", "message": "group_id is required"})
        if not k:
            raise HTTPException(status_code=400, detail={"code": "missing_key", "message": "key is required"})
        resp = await ctx.daemon({"op": "im_bind_chat", "args": {"group_id": gid, "key": k}})
        if not resp.get("ok"):
            err = resp.get("error") if isinstance(resp.get("error"), dict) else {}
            code = str(err.get("code") or "bind_failed")
            msg = str(err.get("message") or "bind failed")
            raise HTTPException(status_code=400, detail={"code": code, "message": msg})
        return resp

    @app.get("/api/im/authorized")
    async def im_list_authorized(group_id: str) -> Dict[str, Any]:
        """List authorized chats for a group."""
        resp = await ctx.daemon({"op": "im_list_authorized", "args": {"group_id": group_id}})
        if not resp.get("ok"):
            err = resp.get("error") if isinstance(resp.get("error"), dict) else {}
            raise HTTPException(status_code=400, detail=err)
        return resp

    @app.get("/api/im/pending")
    async def im_list_pending(group_id: str) -> Dict[str, Any]:
        """List pending bind requests for a group."""
        resp = await ctx.daemon({"op": "im_list_pending", "args": {"group_id": group_id}})
        if not resp.get("ok"):
            err = resp.get("error") if isinstance(resp.get("error"), dict) else {}
            raise HTTPException(status_code=400, detail=err)
        return resp

    @app.post("/api/im/pending/reject")
    async def im_reject_pending(
        req: Optional[IMPendingRejectRequest] = None,
        group_id: str = "",
        key: str = "",
    ) -> Dict[str, Any]:
        """Reject a pending bind request key."""
        gid = str((req.group_id if isinstance(req, IMPendingRejectRequest) else group_id) or "").strip()
        k = str((req.key if isinstance(req, IMPendingRejectRequest) else key) or "").strip()
        if not gid:
            raise HTTPException(status_code=400, detail={"code": "missing_group_id", "message": "group_id is required"})
        if not k:
            raise HTTPException(status_code=400, detail={"code": "missing_key", "message": "key is required"})
        resp = await ctx.daemon({"op": "im_reject_pending", "args": {"group_id": gid, "key": k}})
        if not resp.get("ok"):
            err = resp.get("error") if isinstance(resp.get("error"), dict) else {}
            code = str(err.get("code") or "reject_failed")
            msg = str(err.get("message") or "reject failed")
            raise HTTPException(status_code=400, detail={"code": code, "message": msg})
        return resp

    @app.post("/api/im/revoke")
    async def im_revoke(group_id: str, chat_id: str, thread_id: int = 0) -> Dict[str, Any]:
        """Revoke authorization for a chat."""
        resp = await ctx.daemon({"op": "im_revoke_chat", "args": {"group_id": group_id, "chat_id": chat_id, "thread_id": thread_id}})
        if not resp.get("ok"):
            err = resp.get("error") if isinstance(resp.get("error"), dict) else {}
            raise HTTPException(status_code=400, detail=err)
        return resp
