from __future__ import annotations

"""IM bridge related CLI command handlers."""

from .common import *  # noqa: F401,F403

__all__ = [
    "cmd_im_set",
    "cmd_im_unset",
    "cmd_im_config",
    "cmd_im_start",
    "cmd_im_stop",
    "cmd_im_status",
    "cmd_im_bind",
    "cmd_im_authorized",
    "cmd_im_revoke",
    "cmd_im_logs",
]

def cmd_im_set(args: argparse.Namespace) -> int:
    """Set IM bridge configuration for a group."""
    group_id = _resolve_group_id(getattr(args, "group", ""))
    if not group_id:
        _print_json({"ok": False, "error": {"code": "missing_group_id", "message": "missing group_id (no active group?)"}})
        return 2

    platform = str(args.platform or "").strip().lower()
    if platform not in ("telegram", "slack", "discord", "feishu", "dingtalk"):
        _print_json({"ok": False, "error": {"code": "invalid_platform", "message": "platform must be telegram, slack, discord, feishu, or dingtalk"}})
        return 2

    # Get token fields
    bot_token_env = str(getattr(args, "bot_token_env", "") or "").strip()
    app_token_env = str(getattr(args, "app_token_env", "") or "").strip()
    token_env = str(args.token_env or "").strip()
    token = str(args.token or "").strip()
    # Feishu/DingTalk specific (app credentials)
    app_key_env = str(getattr(args, "app_key_env", "") or "").strip()
    app_secret_env = str(getattr(args, "app_secret_env", "") or "").strip()
    feishu_domain = str(getattr(args, "domain", "") or "").strip()
    dingtalk_robot_code_env = str(getattr(args, "robot_code_env", "") or "").strip()
    dingtalk_robot_code = str(getattr(args, "robot_code", "") or "").strip()

    # Backward compat: if only token_env provided, use as bot_token_env
    if token_env and not bot_token_env:
        bot_token_env = token_env

    # Interactive mode if required fields are missing
    if platform in ("feishu", "dingtalk"):
        # Feishu/DingTalk use app credentials (env var names by default).
        if not app_key_env or not app_secret_env:
            try:
                platform_name = "Feishu/Lark" if platform == "feishu" else "DingTalk"
                default_key = "FEISHU_APP_ID" if platform == "feishu" else "DINGTALK_APP_KEY"
                default_secret = "FEISHU_APP_SECRET" if platform == "feishu" else "DINGTALK_APP_SECRET"
                print(f"{platform_name} requires app credentials:")
                if not app_key_env:
                    print(f"Enter App Key/ID env var name (default: {default_key}):")
                    key_input = input("> ").strip()
                    app_key_env = key_input or default_key
                if not app_secret_env:
                    print(f"Enter App Secret env var name (default: {default_secret}):")
                    secret_input = input("> ").strip()
                    app_secret_env = secret_input or default_secret
            except (EOFError, KeyboardInterrupt):
                print()
                return 1
    elif not bot_token_env and not token:
        try:
            if platform == "slack":
                print(f"Slack requires two tokens:")
                print(f"  1. Bot Token (xoxb-) for outbound messages")
                print(f"  2. App Token (xapp-) for inbound messages (Socket Mode)")
                print()
                print("Enter Bot Token env var name (e.g., SLACK_BOT_TOKEN):")
                bot_input = input("> ").strip()
                if not bot_input:
                    _print_json({"ok": False, "error": {"code": "no_token", "message": "no bot token provided"}})
                    return 2
                bot_token_env = bot_input
                print("Enter App Token env var name (e.g., SLACK_APP_TOKEN):")
                app_input = input("> ").strip()
                if app_input:
                    app_token_env = app_input
            else:
                print(f"Enter token or environment variable name for {platform}:")
                user_input = input("> ").strip()
                if not user_input:
                    _print_json({"ok": False, "error": {"code": "no_token", "message": "no token provided"}})
                    return 2
                # Heuristic: if it looks like an env var name (all caps, underscores), treat as token_env
                if user_input.isupper() or "_" in user_input and not ":" in user_input:
                    bot_token_env = user_input
                else:
                    token = user_input
        except (EOFError, KeyboardInterrupt):
            print()
            return 1

    group = load_group(group_id)
    if group is None:
        _print_json({"ok": False, "error": {"code": "group_not_found", "message": f"group not found: {group_id}"}})
        return 2

    # Update group.yaml with canonical IM config.
    prev_im = group.doc.get("im") if isinstance(group.doc.get("im"), dict) else {}
    im_config: dict[str, Any] = {"platform": platform}
    if isinstance(prev_im, dict) and "enabled" in prev_im:
        im_config["enabled"] = coerce_bool(prev_im.get("enabled"), default=False)
    if isinstance(prev_im, dict) and isinstance(prev_im.get("files"), dict):
        im_config["files"] = prev_im.get("files")
    else:
        default_max_mb = 20 if platform in ("telegram", "slack") else 10
        im_config["files"] = {"enabled": True, "max_mb": default_max_mb}
    if isinstance(prev_im, dict) and "skip_pending_on_start" in prev_im:
        im_config["skip_pending_on_start"] = coerce_bool(prev_im.get("skip_pending_on_start"), default=True)

    if platform in ("telegram", "discord", "slack"):
        token_hint = bot_token_env or token_env or token
        if token_hint:
            im_config["bot_token_env"] = token_hint
        if platform == "slack" and app_token_env:
            im_config["app_token_env"] = app_token_env
    elif platform == "feishu":
        if feishu_domain:
            im_config["feishu_domain"] = feishu_domain
        if app_key_env:
            im_config["feishu_app_id"] = app_key_env
        if app_secret_env:
            im_config["feishu_app_secret"] = app_secret_env
    elif platform == "dingtalk":
        if app_key_env:
            im_config["dingtalk_app_key"] = app_key_env
        if app_secret_env:
            im_config["dingtalk_app_secret"] = app_secret_env
        if dingtalk_robot_code_env:
            im_config["dingtalk_robot_code"] = dingtalk_robot_code_env
        elif dingtalk_robot_code:
            im_config["dingtalk_robot_code"] = dingtalk_robot_code

    im_config = canonicalize_im_config(im_config)

    # Update group doc and save
    group.doc["im"] = im_config
    group.save()

    _print_json({"ok": True, "result": {"group_id": group_id, "im": im_config}})
    return 0

def cmd_im_unset(args: argparse.Namespace) -> int:
    """Remove IM bridge configuration from a group."""
    group_id = _resolve_group_id(getattr(args, "group", ""))
    if not group_id:
        _print_json({"ok": False, "error": {"code": "missing_group_id", "message": "missing group_id (no active group?)"}})
        return 2

    group = load_group(group_id)
    if group is None:
        _print_json({"ok": False, "error": {"code": "group_not_found", "message": f"group not found: {group_id}"}})
        return 2

    if "im" in group.doc:
        del group.doc["im"]
        group.save()

    _print_json({"ok": True, "result": {"group_id": group_id, "im": None}})
    return 0

def cmd_im_config(args: argparse.Namespace) -> int:
    """Show IM bridge configuration for a group."""
    group_id = _resolve_group_id(getattr(args, "group", ""))
    if not group_id:
        _print_json({"ok": False, "error": {"code": "missing_group_id", "message": "missing group_id (no active group?)"}})
        return 2

    group = load_group(group_id)
    if group is None:
        _print_json({"ok": False, "error": {"code": "group_not_found", "message": f"group not found: {group_id}"}})
        return 2

    raw_im = group.doc.get("im")
    im_config = canonicalize_im_config(raw_im) if isinstance(raw_im, dict) else raw_im
    _print_json({"ok": True, "result": {"group_id": group_id, "im": im_config}})
    return 0

def _im_find_bridge_pid(group: Any) -> Optional[int]:
    """Find running bridge PID for a group."""
    pid_path = group.path / "state" / "im_bridge.pid"
    if not pid_path.exists():
        return None
    try:
        pid = int(pid_path.read_text(encoding="utf-8").strip())
        # Check if process is alive
        os.kill(pid, 0)
        return pid
    except (ValueError, ProcessLookupError, PermissionError):
        return None

def _im_find_bridge_pids_by_script(group_id: str) -> list[int]:
    """Find all bridge processes for a group by scanning /proc."""
    pids: list[int] = []
    proc = Path("/proc")
    try:
        for d in proc.iterdir():
            if not d.is_dir() or not d.name.isdigit():
                continue
            pid = int(d.name)
            try:
                cmdline = (d / "cmdline").read_bytes().decode("utf-8", "ignore")
                # Look for our bridge module with this group_id.
                # We support both historical entrypoints:
                # - python -m cccc.ports.im.bridge <group_id> ...
                # - python -m cccc.ports.im <group_id> ...
                if (
                    ("cccc.ports.im.bridge" in cmdline or "cccc.ports.im" in cmdline)
                    and group_id in cmdline
                ):
                    pids.append(pid)
            except Exception:
                continue
    except Exception:
        pass
    return pids

def _im_group_dir(group_id: str) -> Path:
    return ensure_home() / "groups" / group_id

def cmd_im_start(args: argparse.Namespace) -> int:
    """Start IM bridge for a group."""
    group_id = _resolve_group_id(getattr(args, "group", ""))
    if not group_id:
        _print_json({"ok": False, "error": {"code": "missing_group_id", "message": "missing group_id (no active group?)"}})
        return 2

    group = load_group(group_id)
    if group is None:
        _print_json({"ok": False, "error": {"code": "group_not_found", "message": f"group not found: {group_id}"}})
        return 2

    # Check if already running
    existing_pid = _im_find_bridge_pid(group)
    if existing_pid:
        _print_json({"ok": False, "error": {"code": "already_running", "message": f"bridge already running (pid={existing_pid})"}})
        return 2
    orphan_pids = _im_find_bridge_pids_by_script(group_id)
    if orphan_pids:
        _print_json({"ok": False, "error": {"code": "already_running", "message": f"bridge already running (pid={orphan_pids[0]})"}})
        return 2

    # Check IM config
    im_config = canonicalize_im_config(group.doc.get("im", {}))
    if not im_config:
        _print_json({"ok": False, "error": {"code": "no_im_config", "message": "no IM configuration. Run: cccc im set <platform>"}})
        return 2

    # Persist desired run-state for restart/autostart.
    im_config["enabled"] = True
    group.doc["im"] = im_config
    try:
        group.save()
    except Exception:
        pass

    platform = im_config.get("platform", "telegram")

    # Prepare environment
    env = os.environ.copy()
    bot_token_env = str(im_config.get("bot_token_env") or "").strip()
    bot_token = str(im_config.get("bot_token") or "").strip()
    if bot_token and bot_token_env:
        env[bot_token_env] = bot_token
    elif bot_token:
        # Set default env var based on platform
        default_env = {"telegram": "TELEGRAM_BOT_TOKEN", "slack": "SLACK_BOT_TOKEN", "discord": "DISCORD_BOT_TOKEN"}
        env[default_env.get(platform, "BOT_TOKEN")] = bot_token
    if str(platform) == "slack":
        app_token_env = str(im_config.get("app_token_env") or "").strip()
        app_token = str(im_config.get("app_token") or "").strip()
        if app_token and app_token_env:
            env[app_token_env] = app_token

    # Feishu/DingTalk: set credentials from config
    # Supports both direct values and env var names (for Web UI compatibility)
    if platform == "feishu":
        # Direct values
        app_id = im_config.get("feishu_app_id", "")
        app_secret = im_config.get("feishu_app_secret", "")
        # Env var names
        app_id_env = im_config.get("feishu_app_id_env", "")
        app_secret_env = im_config.get("feishu_app_secret_env", "")
        # Set env vars (direct value takes precedence)
        if app_id:
            env["FEISHU_APP_ID"] = app_id
        elif app_id_env and app_id_env in os.environ:
            env["FEISHU_APP_ID"] = os.environ[app_id_env]
        if app_secret:
            env["FEISHU_APP_SECRET"] = app_secret
        elif app_secret_env and app_secret_env in os.environ:
            env["FEISHU_APP_SECRET"] = os.environ[app_secret_env]
    elif platform == "dingtalk":
        # Direct values
        app_key = im_config.get("dingtalk_app_key", "")
        app_secret = im_config.get("dingtalk_app_secret", "")
        robot_code = im_config.get("dingtalk_robot_code", "")
        # Env var names
        app_key_env = im_config.get("dingtalk_app_key_env", "")
        app_secret_env = im_config.get("dingtalk_app_secret_env", "")
        robot_code_env = im_config.get("dingtalk_robot_code_env", "")
        # Set env vars (direct value takes precedence)
        if app_key:
            env["DINGTALK_APP_KEY"] = app_key
        elif app_key_env and app_key_env in os.environ:
            env["DINGTALK_APP_KEY"] = os.environ[app_key_env]
        if app_secret:
            env["DINGTALK_APP_SECRET"] = app_secret
        elif app_secret_env and app_secret_env in os.environ:
            env["DINGTALK_APP_SECRET"] = os.environ[app_secret_env]
        if robot_code:
            env["DINGTALK_ROBOT_CODE"] = robot_code
        elif robot_code_env and robot_code_env in os.environ:
            env["DINGTALK_ROBOT_CODE"] = os.environ[robot_code_env]

    # Start bridge as subprocess
    state_dir = group.path / "state"
    state_dir.mkdir(parents=True, exist_ok=True)
    log_path = state_dir / "im_bridge.log"

    log_file = None
    try:
        log_file = log_path.open("a", encoding="utf-8")
        proc = subprocess.Popen(
            [sys.executable, "-m", "cccc.ports.im", group_id, platform],
            env=env,
            stdout=log_file,
            stderr=log_file,
            start_new_session=True,
        )
        # Give the bridge a moment to acquire locks / validate tokens.
        time.sleep(0.25)
        rc = proc.poll()
        try:
            log_file.close()
        except Exception:
            pass

        if rc is not None:
            _print_json({
                "ok": False,
                "error": {
                    "code": "start_failed",
                    "message": f"bridge exited immediately (code={rc}). See log: {log_path}",
                },
            })
            return 2

        # Write PID file only after we know it stayed up.
        pid_path = state_dir / "im_bridge.pid"
        pid_path.write_text(str(proc.pid), encoding="utf-8")

        _print_json({"ok": True, "result": {"group_id": group_id, "platform": platform, "pid": proc.pid, "log": str(log_path)}})
        return 0
    except Exception as e:
        try:
            if log_file:
                log_file.close()
        except Exception:
            pass
        _print_json({"ok": False, "error": {"code": "start_failed", "message": str(e)}})
        return 2

def cmd_im_stop(args: argparse.Namespace) -> int:
    """Stop IM bridge for a group."""
    import signal as sig

    group_id = _resolve_group_id(getattr(args, "group", ""))
    if not group_id:
        _print_json({"ok": False, "error": {"code": "missing_group_id", "message": "missing group_id (no active group?)"}})
        return 2

    # Persist desired run-state for restart/autostart (best-effort).
    try:
        group = load_group(group_id)
        if group is not None:
            raw_im_cfg = group.doc.get("im")
            if isinstance(raw_im_cfg, dict):
                im_cfg = canonicalize_im_config(raw_im_cfg)
                im_cfg["enabled"] = False
                group.doc["im"] = im_cfg
                group.save()
    except Exception:
        pass

    stopped = 0
    group_dir = _im_group_dir(group_id)
    pid_path = group_dir / "state" / "im_bridge.pid"
    killed: set[int] = set()

    # Stop by PID file
    if pid_path.exists():
        try:
            pid = int(pid_path.read_text(encoding="utf-8").strip())
            if pid not in killed:
                try:
                    os.killpg(os.getpgid(pid), sig.SIGTERM)
                except Exception:
                    try:
                        os.kill(pid, sig.SIGTERM)
                    except Exception:
                        pass
                killed.add(pid)
                stopped += 1
        except Exception:
            pass
        try:
            pid_path.unlink(missing_ok=True)
        except Exception:
            pass

    # Also scan for any orphan processes
    orphan_pids = _im_find_bridge_pids_by_script(group_id)
    for pid in orphan_pids:
        if pid in killed:
            continue
        try:
            os.killpg(os.getpgid(pid), sig.SIGTERM)
        except Exception:
            try:
                os.kill(pid, sig.SIGTERM)
            except Exception:
                pass
        killed.add(pid)
        stopped += 1

    _print_json({"ok": True, "result": {"group_id": group_id, "stopped": stopped}})
    return 0

def cmd_im_status(args: argparse.Namespace) -> int:
    """Show IM bridge status for a group."""
    group_id = _resolve_group_id(getattr(args, "group", ""))
    if not group_id:
        _print_json({"ok": False, "error": {"code": "missing_group_id", "message": "missing group_id (no active group?)"}})
        return 2
    group = load_group(group_id)
    group_exists = group is not None

    raw_im = group.doc.get("im", {}) if group_exists else {}
    im_config = canonicalize_im_config(raw_im) if isinstance(raw_im, dict) else {}
    platform = im_config.get("platform") if im_config else None

    # Check if running
    pid = _im_find_bridge_pid(group) if group_exists else None
    if pid is None:
        orphan_pids = _im_find_bridge_pids_by_script(group_id)
        if orphan_pids:
            pid = orphan_pids[0]
    running = pid is not None

    # Get subscriber count
    subscribers_path = _im_group_dir(group_id) / "state" / "im_subscribers.json"
    subscriber_count = 0
    if subscribers_path.exists():
        try:
            subs = json.loads(subscribers_path.read_text(encoding="utf-8"))
            subscriber_count = sum(1 for s in subs.values() if isinstance(s, dict) and s.get("subscribed"))
        except Exception:
            pass

    result = {
        "group_id": group_id,
        "group_exists": group_exists,
        "configured": bool(im_config),
        "platform": platform,
        "running": running,
        "pid": pid,
        "subscribers": subscriber_count,
    }

    _print_json({"ok": True, "result": result})
    return 0

def cmd_im_bind(args: argparse.Namespace) -> int:
    """Bind a pending authorization key to authorize an IM chat."""
    group_id = _resolve_group_id(getattr(args, "group", ""))
    if not group_id:
        _print_json({"ok": False, "error": {"code": "missing_group_id", "message": "missing group_id (no active group?)"}})
        return 2

    key = str(getattr(args, "key", "") or "").strip()
    if not key:
        _print_json({"ok": False, "error": {"code": "missing_key", "message": "missing --key argument"}})
        return 2

    if not _ensure_daemon_running():
        _print_json({"ok": False, "error": {"code": "daemon_error", "message": "cannot reach daemon"}})
        return 1

    resp = call_daemon({"op": "im_bind_chat", "args": {"group_id": group_id, "key": key}})
    _print_json(resp)
    return 0 if resp.get("ok") else 1

def cmd_im_authorized(args: argparse.Namespace) -> int:
    """List authorized chats for a group."""
    group_id = _resolve_group_id(getattr(args, "group", ""))
    if not group_id:
        _print_json({"ok": False, "error": {"code": "missing_group_id", "message": "missing group_id (no active group?)"}})
        return 2

    if not _ensure_daemon_running():
        _print_json({"ok": False, "error": {"code": "daemon_error", "message": "cannot reach daemon"}})
        return 1

    resp = call_daemon({"op": "im_list_authorized", "args": {"group_id": group_id}})
    _print_json(resp)
    return 0 if resp.get("ok") else 1

def cmd_im_revoke(args: argparse.Namespace) -> int:
    """Revoke authorization for a chat."""
    group_id = _resolve_group_id(getattr(args, "group", ""))
    if not group_id:
        _print_json({"ok": False, "error": {"code": "missing_group_id", "message": "missing group_id (no active group?)"}})
        return 2

    chat_id = str(getattr(args, "chat_id", "") or "").strip()
    if not chat_id:
        _print_json({"ok": False, "error": {"code": "missing_chat_id", "message": "missing --chat-id argument"}})
        return 2

    try:
        thread_id = int(getattr(args, "thread_id", 0) or 0)
    except Exception:
        thread_id = 0

    if not _ensure_daemon_running():
        _print_json({"ok": False, "error": {"code": "daemon_error", "message": "cannot reach daemon"}})
        return 1

    resp = call_daemon({"op": "im_revoke_chat", "args": {"group_id": group_id, "chat_id": chat_id, "thread_id": thread_id}})
    _print_json(resp)
    return 0 if resp.get("ok") else 1

def cmd_im_logs(args: argparse.Namespace) -> int:
    """Show IM bridge logs for a group."""
    from collections import deque

    group_id = _resolve_group_id(getattr(args, "group", ""))
    if not group_id:
        _print_json({"ok": False, "error": {"code": "missing_group_id", "message": "missing group_id (no active group?)"}})
        return 2

    log_path = _im_group_dir(group_id) / "state" / "im_bridge.log"
    if not log_path.exists():
        print(f"[IM] Log file not found: {log_path}")
        return 1

    lines = int(args.lines) if hasattr(args, "lines") and args.lines else 50
    follow = bool(args.follow) if hasattr(args, "follow") else False

    try:
        if follow:
            print(f"[IM] Tailing {log_path} (Ctrl-C to stop)...")
            with open(log_path, "r", encoding="utf-8") as f:
                # Show last N lines first
                dq = deque(f, maxlen=lines)
                for ln in dq:
                    print(ln.rstrip())
                # Then follow
                while True:
                    ln = f.readline()
                    if not ln:
                        time.sleep(0.5)
                        continue
                    print(ln.rstrip())
        else:
            # Print last N lines
            with open(log_path, "r", encoding="utf-8") as f:
                dq = deque(f, maxlen=lines)
                for ln in dq:
                    print(ln.rstrip())
    except KeyboardInterrupt:
        print()

    return 0
