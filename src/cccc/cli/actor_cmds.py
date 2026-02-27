from __future__ import annotations

"""Actor related CLI command handlers."""

from .common import *  # noqa: F401,F403

__all__ = [
    "cmd_actor_list",
    "cmd_actor_add",
    "cmd_actor_remove",
    "cmd_actor_start",
    "cmd_actor_stop",
    "cmd_actor_restart",
    "cmd_actor_update",
    "cmd_actor_secrets",
    "cmd_runtime_list",
]

def cmd_actor_list(args: argparse.Namespace) -> int:
    group_id = _resolve_group_id(getattr(args, "group", ""))
    if not group_id:
        _print_json({"ok": False, "error": {"code": "missing_group_id", "message": "missing group_id (no active group?)"}})
        return 2

    if _ensure_daemon_running():
        resp = call_daemon({"op": "actor_list", "args": {"group_id": group_id}})
        if resp.get("ok"):
            _print_json(resp)
            return 0

    group = load_group(group_id)
    if group is None:
        _print_json({"ok": False, "error": {"code": "group_not_found", "message": f"group not found: {group_id}"}})
        return 2
    _print_json({"ok": True, "result": {"actors": list_actors(group)}})
    return 0

def cmd_actor_add(args: argparse.Namespace) -> int:
    group_id = _resolve_group_id(getattr(args, "group", ""))
    if not group_id:
        _print_json({"ok": False, "error": {"code": "missing_group_id", "message": "missing group_id (no active group?)"}})
        return 2
    group = load_group(group_id)
    if group is None:
        _print_json({"ok": False, "error": {"code": "group_not_found", "message": f"group not found: {group_id}"}})
        return 2

    actor_id = str(args.actor_id or "").strip()
    title = str(args.title or "").strip()
    by = str(args.by or "user").strip()
    submit = str(args.submit or "enter").strip() or "enter"
    runner = str(getattr(args, "runner", "") or "pty").strip() or "pty"
    runtime = str(getattr(args, "runtime", "") or "codex").strip() or "codex"
    command: list[str] = []
    if args.command:
        try:
            command = shlex.split(str(args.command), posix=(os.name != "nt"))
        except Exception:
            command = [str(args.command)]
    
    # Auto-set command based on runtime if not provided
    if not command:
        from ..kernel.runtime import get_runtime_command_with_flags
        command = get_runtime_command_with_flags(runtime)
    if runtime == "custom" and runner != "headless" and not command:
        _print_json({
            "ok": False,
            "error": {"code": "missing_command", "message": "custom runtime requires a command (PTY runner)"},
        })
        return 2
    
    env: dict[str, str] = {}
    if isinstance(args.env, list):
        for item in args.env:
            if not isinstance(item, str) or "=" not in item:
                continue
            k, v = item.split("=", 1)
            k = k.strip()
            if not k:
                continue
            env[k] = v
    default_scope_key = ""
    if args.scope:
        default_scope_key = detect_scope(Path(args.scope)).scope_key
        scopes = group.doc.get("scopes") if isinstance(group.doc.get("scopes"), list) else []
        attached = any(isinstance(s, dict) and s.get("scope_key") == default_scope_key for s in scopes)
        if not attached:
            _print_json({"ok": False, "error": {"code": "scope_not_attached", "message": f"scope not attached: {default_scope_key}"}})
            return 2

    if _ensure_daemon_running():
        resp = call_daemon(
            {
                "op": "actor_add",
                "args": {
                    "group_id": group_id,
                    "actor_id": actor_id,
                    "title": title,
                    "submit": submit,
                    "runner": runner,
                    "runtime": runtime,
                    "by": by,
                    "command": command,
                    "env": env,
                    "default_scope_key": default_scope_key,
                },
            }
        )
        if resp.get("ok"):
            _print_json(resp)
            return 0

    try:
        require_actor_permission(group, by=by, action="actor.add")
        # Note: role is auto-determined by position (first enabled = foreman)
        if runner not in ("pty", "headless"):
            raise ValueError("invalid runner (must be 'pty' or 'headless')")
        if runtime not in ("amp", "auggie", "claude", "codex", "cursor", "droid", "neovate", "gemini", "kilocode", "opencode", "copilot", "custom"):
            raise ValueError("invalid runtime")
        if runtime == "custom" and runner != "headless" and not command:
            raise ValueError("custom runtime requires a command (PTY runner)")
        actor = add_actor(
            group,
            actor_id=actor_id,
            title=title,
            command=command,
            env=env,
            default_scope_key=default_scope_key,
            submit=submit,
            runner=runner,  # type: ignore
            runtime=runtime,  # type: ignore
        )
    except Exception as e:
        _print_json({"ok": False, "error": {"code": "actor_add_failed", "message": str(e)}})
        return 2
    ev = append_event(group.ledger_path, kind="actor.add", group_id=group.group_id, scope_key="", by=by, data={"actor": actor})
    _print_json({"ok": True, "result": {"actor": actor, "event": ev}})
    return 0

def cmd_actor_remove(args: argparse.Namespace) -> int:
    group_id = _resolve_group_id(getattr(args, "group", ""))
    if not group_id:
        _print_json({"ok": False, "error": {"code": "missing_group_id", "message": "missing group_id (no active group?)"}})
        return 2
    actor_id = str(args.actor_id or "").strip()
    by = str(args.by or "user").strip()

    if _ensure_daemon_running():
        resp = call_daemon({"op": "actor_remove", "args": {"group_id": group_id, "actor_id": actor_id, "by": by}})
        if resp.get("ok"):
            _print_json(resp)
            return 0

    group = load_group(group_id)
    if group is None:
        _print_json({"ok": False, "error": {"code": "group_not_found", "message": f"group not found: {group_id}"}})
        return 2
    try:
        require_actor_permission(group, by=by, action="actor.remove", target_actor_id=actor_id)
        remove_actor(group, actor_id)
    except Exception as e:
        _print_json({"ok": False, "error": {"code": "actor_remove_failed", "message": str(e)}})
        return 2
    ev = append_event(group.ledger_path, kind="actor.remove", group_id=group.group_id, scope_key="", by=by, data={"actor_id": actor_id})
    _print_json({"ok": True, "result": {"actor_id": actor_id, "event": ev}})
    return 0

def cmd_actor_start(args: argparse.Namespace) -> int:
    group_id = _resolve_group_id(getattr(args, "group", ""))
    if not group_id:
        _print_json({"ok": False, "error": {"code": "missing_group_id", "message": "missing group_id (no active group?)"}})
        return 2
    actor_id = str(args.actor_id or "").strip()
    by = str(args.by or "user").strip()

    if _ensure_daemon_running():
        resp = call_daemon({"op": "actor_start", "args": {"group_id": group_id, "actor_id": actor_id, "by": by}})
        if resp.get("ok"):
            _print_json(resp)
            return 0

    group = load_group(group_id)
    if group is None:
        _print_json({"ok": False, "error": {"code": "group_not_found", "message": f"group not found: {group_id}"}})
        return 2
    try:
        require_actor_permission(group, by=by, action="actor.start", target_actor_id=actor_id)
        actor = update_actor(group, actor_id, {"enabled": True})
    except Exception as e:
        _print_json({"ok": False, "error": {"code": "actor_start_failed", "message": str(e)}})
        return 2
    ev = append_event(group.ledger_path, kind="actor.start", group_id=group.group_id, scope_key="", by=by, data={"actor_id": actor_id})
    _print_json({"ok": True, "result": {"actor": actor, "event": ev}})
    return 0

def cmd_actor_stop(args: argparse.Namespace) -> int:
    group_id = _resolve_group_id(getattr(args, "group", ""))
    if not group_id:
        _print_json({"ok": False, "error": {"code": "missing_group_id", "message": "missing group_id (no active group?)"}})
        return 2
    actor_id = str(args.actor_id or "").strip()
    by = str(args.by or "user").strip()

    if _ensure_daemon_running():
        resp = call_daemon({"op": "actor_stop", "args": {"group_id": group_id, "actor_id": actor_id, "by": by}})
        if resp.get("ok"):
            _print_json(resp)
            return 0

    group = load_group(group_id)
    if group is None:
        _print_json({"ok": False, "error": {"code": "group_not_found", "message": f"group not found: {group_id}"}})
        return 2
    try:
        require_actor_permission(group, by=by, action="actor.stop", target_actor_id=actor_id)
        actor = update_actor(group, actor_id, {"enabled": False})
    except Exception as e:
        _print_json({"ok": False, "error": {"code": "actor_stop_failed", "message": str(e)}})
        return 2
    ev = append_event(group.ledger_path, kind="actor.stop", group_id=group.group_id, scope_key="", by=by, data={"actor_id": actor_id})
    _print_json({"ok": True, "result": {"actor": actor, "event": ev}})
    return 0

def cmd_actor_restart(args: argparse.Namespace) -> int:
    group_id = _resolve_group_id(getattr(args, "group", ""))
    if not group_id:
        _print_json({"ok": False, "error": {"code": "missing_group_id", "message": "missing group_id (no active group?)"}})
        return 2
    actor_id = str(args.actor_id or "").strip()
    by = str(args.by or "user").strip()

    if _ensure_daemon_running():
        resp = call_daemon({"op": "actor_restart", "args": {"group_id": group_id, "actor_id": actor_id, "by": by}})
        if resp.get("ok"):
            _print_json(resp)
            return 0

    group = load_group(group_id)
    if group is None:
        _print_json({"ok": False, "error": {"code": "group_not_found", "message": f"group not found: {group_id}"}})
        return 2
    try:
        require_actor_permission(group, by=by, action="actor.restart", target_actor_id=actor_id)
        actor = update_actor(group, actor_id, {"enabled": True})
    except Exception as e:
        _print_json({"ok": False, "error": {"code": "actor_restart_failed", "message": str(e)}})
        return 2
    ev = append_event(group.ledger_path, kind="actor.restart", group_id=group.group_id, scope_key="", by=by, data={"actor_id": actor_id})
    _print_json({"ok": True, "result": {"actor": actor, "event": ev}})
    return 0

def cmd_actor_update(args: argparse.Namespace) -> int:
    group_id = _resolve_group_id(getattr(args, "group", ""))
    if not group_id:
        _print_json({"ok": False, "error": {"code": "missing_group_id", "message": "missing group_id (no active group?)"}})
        return 2

    actor_id = str(args.actor_id or "").strip()
    by = str(args.by or "user").strip()

    group = load_group(group_id)
    if group is None:
        _print_json({"ok": False, "error": {"code": "group_not_found", "message": f"group not found: {group_id}"}})
        return 2

    patch: dict[str, Any] = {}
    if args.title is not None:
        patch["title"] = str(args.title or "")
    role = getattr(args, "role", None)
    if role:
        patch["role"] = str(role)
    if args.command is not None:
        cmd: list[str] = []
        if str(args.command).strip():
            try:
                cmd = shlex.split(str(args.command), posix=(os.name != "nt"))
            except Exception:
                cmd = [str(args.command)]
        patch["command"] = cmd
    if isinstance(args.env, list) and args.env:
        env: dict[str, str] = {}
        for item in args.env:
            if not isinstance(item, str) or "=" not in item:
                continue
            k, v = item.split("=", 1)
            k = k.strip()
            if not k:
                continue
            env[k] = v
        patch["env"] = env
    if args.scope:
        scope_key = detect_scope(Path(args.scope)).scope_key
        scopes = group.doc.get("scopes") if isinstance(group.doc.get("scopes"), list) else []
        attached = any(isinstance(s, dict) and s.get("scope_key") == scope_key for s in scopes)
        if not attached:
            _print_json({"ok": False, "error": {"code": "scope_not_attached", "message": f"scope not attached: {scope_key}"}})
            return 2
        patch["default_scope_key"] = scope_key
    if args.submit is not None:
        patch["submit"] = str(args.submit)
    if getattr(args, "runner", None) is not None:
        patch["runner"] = str(args.runner)
    if getattr(args, "runtime", None) is not None:
        patch["runtime"] = str(args.runtime)
    if args.enabled is not None:
        patch["enabled"] = bool(args.enabled)

    if not patch:
        _print_json({"ok": False, "error": {"code": "empty_patch", "message": "nothing to update"}})
        return 2

    if _ensure_daemon_running():
        resp = call_daemon({"op": "actor_update", "args": {"group_id": group_id, "actor_id": actor_id, "patch": patch, "by": by}})
        _print_json(resp)
        return 0 if resp.get("ok") else 2

    try:
        require_actor_permission(group, by=by, action="actor.update", target_actor_id=actor_id)
        actor = update_actor(group, actor_id, patch)
    except Exception as e:
        _print_json({"ok": False, "error": {"code": "actor_update_failed", "message": str(e)}})
        return 2
    ev = append_event(group.ledger_path, kind="actor.update", group_id=group.group_id, scope_key="", by=by, data={"actor_id": actor_id, "patch": patch})
    _print_json({"ok": True, "result": {"actor": actor, "event": ev}})
    return 0

def cmd_actor_secrets(args: argparse.Namespace) -> int:
    """Manage per-actor runtime-only secrets env (stored under CCCC_HOME/state, not in ledger)."""
    group_id = _resolve_group_id(getattr(args, "group", ""))
    if not group_id:
        _print_json({"ok": False, "error": {"code": "missing_group_id", "message": "missing group_id (no active group?)"}})
        return 2

    actor_id = str(args.actor_id or "").strip()
    by = str(args.by or "user").strip() or "user"

    if not _ensure_daemon_running():
        _print_json({"ok": False, "error": {"code": "daemon_unavailable", "message": "daemon unavailable"}})
        return 2

    if getattr(args, "keys", False):
        resp = call_daemon({"op": "actor_env_private_keys", "args": {"group_id": group_id, "actor_id": actor_id, "by": by}})
        _print_json(resp)
        return 0 if resp.get("ok") else 2

    set_vars: dict[str, str] = {}
    for item in (args.set or []):
        if not isinstance(item, str) or "=" not in item:
            continue
        k, v = item.split("=", 1)
        k = k.strip()
        if not k:
            continue
        set_vars[k] = v

    unset_keys: list[str] = []
    for item in (args.unset or []):
        k = str(item or "").strip()
        if k:
            unset_keys.append(k)

    clear = bool(getattr(args, "clear", False))
    restart = bool(getattr(args, "restart", False))

    resp = call_daemon(
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
    if not resp.get("ok"):
        _print_json(resp)
        return 2

    if restart:
        r = call_daemon({"op": "actor_restart", "args": {"group_id": group_id, "actor_id": actor_id, "by": by}})
        if not r.get("ok"):
            _print_json(r)
            return 2
        _print_json({"ok": True, "result": {"secrets": resp.get("result", {}), "restart": r.get("result", {})}})
        return 0

    _print_json(resp)
    return 0

def cmd_runtime_list(args: argparse.Namespace) -> int:
    """List available agent runtimes."""
    from ..kernel.runtime import detect_all_runtimes
    
    all_runtimes = args.all if hasattr(args, 'all') else False
    runtimes = detect_all_runtimes(primary_only=not all_runtimes)
    
    result = {
        "runtimes": [
            {
                "name": rt.name,
                "display_name": rt.display_name,
                "command": rt.command,
                "available": rt.available,
                "path": rt.path,
                "capabilities": rt.capabilities,
            }
            for rt in runtimes
        ],
        "available": [rt.name for rt in runtimes if rt.available],
    }
    
    _print_json({"ok": True, "result": result})
    return 0
