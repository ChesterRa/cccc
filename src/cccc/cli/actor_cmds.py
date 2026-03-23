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
    "cmd_actor_profile_list",
    "cmd_actor_profile_get",
    "cmd_actor_profile_upsert",
    "cmd_actor_profile_delete",
    "cmd_actor_profile_secrets",
    "cmd_runtime_list",
]


def _actor_profile_ref_request_args(args: argparse.Namespace) -> dict[str, str]:
    scope = str(getattr(args, "scope", "") or "global").strip().lower() or "global"
    if scope not in {"global", "user"}:
        raise ValueError("invalid profile scope")
    owner_id = str(getattr(args, "owner_id", "") or "").strip()
    if scope == "global":
        owner_id = ""
    if scope == "user" and not owner_id:
        raise ValueError("user scope profile requires owner_id")
    return {
        "profile_scope": scope,
        "profile_owner": owner_id,
    }


def _parse_profile_command_arg(raw: Any) -> list[str]:
    text = str(raw or "").strip()
    if not text:
        return []
    try:
        return shlex.split(text, posix=(os.name != "nt"))
    except Exception:
        return [text]


def _actor_profile_link_cli_args(args: argparse.Namespace) -> tuple[str, str, str]:
    profile_id = str(getattr(args, "profile_id", "") or "").strip()
    profile_scope = str(getattr(args, "profile_scope", "") or "global").strip().lower() or "global"
    if profile_scope not in {"global", "user"}:
        raise ValueError("invalid profile scope")
    profile_owner = str(getattr(args, "profile_owner_id", "") or "").strip()
    if profile_scope == "global":
        profile_owner = ""
    if profile_id and profile_scope == "user" and not profile_owner:
        raise ValueError("user scope profile requires owner_id")
    return profile_id, profile_scope, profile_owner


def cmd_actor_profile_list(args: argparse.Namespace) -> int:
    by = str(getattr(args, "by", "user") or "user").strip() or "user"
    view = str(getattr(args, "view", "global") or "global").strip().lower() or "global"
    if view not in {"global", "my", "all"}:
        _print_json({"ok": False, "error": {"code": "invalid_request", "message": "invalid view"}})
        return 2
    if not _ensure_daemon_running():
        _print_json({"ok": False, "error": {"code": "daemon_unavailable", "message": "daemon unavailable"}})
        return 2
    resp = call_daemon({"op": "actor_profile_list", "args": {"by": by, "view": view}})
    _print_json(resp)
    return 0 if resp.get("ok") else 2


def cmd_actor_profile_get(args: argparse.Namespace) -> int:
    profile_id = str(getattr(args, "profile_id", "") or "").strip()
    by = str(getattr(args, "by", "user") or "user").strip() or "user"
    if not profile_id:
        _print_json({"ok": False, "error": {"code": "missing_profile_id", "message": "missing profile_id"}})
        return 2
    try:
        ref_args = _actor_profile_ref_request_args(args)
    except ValueError as e:
        _print_json({"ok": False, "error": {"code": "invalid_request", "message": str(e)}})
        return 2
    if not _ensure_daemon_running():
        _print_json({"ok": False, "error": {"code": "daemon_unavailable", "message": "daemon unavailable"}})
        return 2
    req_args = {
        "profile_id": profile_id,
        "by": by,
        **ref_args,
    }
    resp = call_daemon({"op": "actor_profile_get", "args": req_args})
    _print_json(resp)
    return 0 if resp.get("ok") else 2


def cmd_actor_profile_upsert(args: argparse.Namespace) -> int:
    by = str(getattr(args, "by", "user") or "user").strip() or "user"
    profile: dict[str, Any] = {}

    profile_id = str(getattr(args, "profile_id", "") or "").strip()
    if profile_id:
        profile["id"] = profile_id

    if getattr(args, "name", None) is not None:
        profile["name"] = str(getattr(args, "name", "") or "")
    if getattr(args, "runtime", None) is not None:
        profile["runtime"] = str(getattr(args, "runtime", "") or "").strip()
    if getattr(args, "runner", None) is not None:
        profile["runner"] = str(getattr(args, "runner", "") or "").strip()
    if getattr(args, "command", None) is not None:
        profile["command"] = _parse_profile_command_arg(getattr(args, "command", None))
    if getattr(args, "submit", None) is not None:
        profile["submit"] = str(getattr(args, "submit", "") or "").strip()

    scope = getattr(args, "scope", None)
    owner_id = getattr(args, "owner_id", None)
    if scope is not None:
        normalized_scope = str(scope or "global").strip().lower() or "global"
        if normalized_scope not in {"global", "user"}:
            _print_json({"ok": False, "error": {"code": "invalid_request", "message": "invalid profile scope"}})
            return 2
        profile["scope"] = normalized_scope
    if owner_id is not None:
        profile["owner_id"] = str(owner_id or "").strip()
    if str(profile.get("scope") or "global") == "global":
        profile["owner_id"] = ""
    if str(profile.get("scope") or "global") == "user" and not str(profile.get("owner_id") or "").strip():
        _print_json({"ok": False, "error": {"code": "invalid_request", "message": "user scope profile requires owner_id"}})
        return 2

    capability_defaults_raw = getattr(args, "capability_defaults", "")
    if str(capability_defaults_raw or "").strip():
        try:
            profile["capability_defaults"] = _parse_json_object_arg(
                capability_defaults_raw,
                field="capability_defaults",
            )
        except ValueError as e:
            _print_json({"ok": False, "error": {"code": "invalid_capability_defaults", "message": str(e)}})
            return 2

    if not profile:
        _print_json({"ok": False, "error": {"code": "empty_profile", "message": "nothing to upsert"}})
        return 2

    expected_revision = getattr(args, "expected_revision", None)
    normalized_expected_revision: int | None = None
    if expected_revision is not None:
        try:
            normalized_expected_revision = int(expected_revision)
        except Exception:
            _print_json({"ok": False, "error": {"code": "invalid_request", "message": "expected_revision must be an integer"}})
            return 2

    if not _ensure_daemon_running():
        _print_json({"ok": False, "error": {"code": "daemon_unavailable", "message": "daemon unavailable"}})
        return 2

    req_args: dict[str, Any] = {
        "by": by,
        "profile": profile,
    }
    if normalized_expected_revision is not None:
        req_args["expected_revision"] = normalized_expected_revision

    resp = call_daemon({"op": "actor_profile_upsert", "args": req_args})
    _print_json(resp)
    return 0 if resp.get("ok") else 2


def cmd_actor_profile_delete(args: argparse.Namespace) -> int:
    profile_id = str(getattr(args, "profile_id", "") or "").strip()
    by = str(getattr(args, "by", "user") or "user").strip() or "user"
    if not profile_id:
        _print_json({"ok": False, "error": {"code": "missing_profile_id", "message": "missing profile_id"}})
        return 2
    try:
        ref_args = _actor_profile_ref_request_args(args)
    except ValueError as e:
        _print_json({"ok": False, "error": {"code": "invalid_request", "message": str(e)}})
        return 2
    if not _ensure_daemon_running():
        _print_json({"ok": False, "error": {"code": "daemon_unavailable", "message": "daemon unavailable"}})
        return 2
    req_args: dict[str, Any] = {
        "profile_id": profile_id,
        "by": by,
        "force_detach": bool(getattr(args, "force_detach", False)),
        **ref_args,
    }
    resp = call_daemon({"op": "actor_profile_delete", "args": req_args})
    _print_json(resp)
    return 0 if resp.get("ok") else 2


def cmd_actor_profile_secrets(args: argparse.Namespace) -> int:
    profile_id = str(getattr(args, "profile_id", "") or "").strip()
    by = str(getattr(args, "by", "user") or "user").strip() or "user"
    if not profile_id:
        _print_json({"ok": False, "error": {"code": "missing_profile_id", "message": "missing profile_id"}})
        return 2
    try:
        ref_args = _actor_profile_ref_request_args(args)
    except ValueError as e:
        _print_json({"ok": False, "error": {"code": "invalid_request", "message": str(e)}})
        return 2
    if bool(getattr(args, "keys", False)) and (
        bool(getattr(args, "set", []) or [])
        or bool(getattr(args, "unset", []) or [])
        or bool(getattr(args, "clear", False))
    ):
        _print_json(
            {
                "ok": False,
                "error": {
                    "code": "invalid_request",
                    "message": "--keys cannot be combined with --set/--unset/--clear",
                },
            }
        )
        return 2
    if bool(getattr(args, "keys", False)):
        if not _ensure_daemon_running():
            _print_json({"ok": False, "error": {"code": "daemon_unavailable", "message": "daemon unavailable"}})
            return 2
        req_args = {
            "profile_id": profile_id,
            "by": by,
            **ref_args,
        }
        resp = call_daemon({"op": "actor_profile_secret_keys", "args": req_args})
        _print_json(resp)
        return 0 if resp.get("ok") else 2

    set_vars: dict[str, str] = {}
    for item in (getattr(args, "set", []) or []):
        if not isinstance(item, str) or "=" not in item:
            continue
        k, v = item.split("=", 1)
        k = k.strip()
        if not k:
            continue
        set_vars[k] = v

    unset_keys: list[str] = []
    for item in (getattr(args, "unset", []) or []):
        k = str(item or "").strip()
        if k:
            unset_keys.append(k)

    clear = bool(getattr(args, "clear", False))
    if not set_vars and not unset_keys and not clear:
        _print_json({"ok": False, "error": {"code": "empty_secret_update", "message": "nothing to update"}})
        return 2
    if not _ensure_daemon_running():
        _print_json({"ok": False, "error": {"code": "daemon_unavailable", "message": "daemon unavailable"}})
        return 2

    req_args = {
        "profile_id": profile_id,
        "by": by,
        **ref_args,
    }

    resp = call_daemon(
        {
            "op": "actor_profile_secret_update",
            "args": {
                **req_args,
                "set": set_vars,
                "unset": unset_keys,
                "clear": clear,
            },
        }
    )
    _print_json(resp)
    return 0 if resp.get("ok") else 2

def cmd_actor_list(args: argparse.Namespace) -> int:
    group_id = _resolve_group_id(getattr(args, "group", ""))
    if not group_id:
        _print_json({"ok": False, "error": {"code": "missing_group_id", "message": "missing group_id (no active group?)"}})
        return 2

    if _ensure_daemon_running():
        resp = call_daemon({"op": "actor_list", "args": {"group_id": group_id}})
        _print_json(resp)
        return 0 if resp.get("ok") else 2

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
    try:
        profile_id, profile_scope, profile_owner = _actor_profile_link_cli_args(args)
    except ValueError as e:
        _print_json({"ok": False, "error": {"code": "invalid_request", "message": str(e)}})
        return 2
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
    if runtime == "custom" and not command:
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
                    "profile_id": profile_id,
                    "profile_scope": profile_scope,
                    "profile_owner": profile_owner,
                },
            }
        )
        _print_json(resp)
        return 0 if resp.get("ok") else 2

    if profile_id:
        _print_json({"ok": False, "error": {"code": "daemon_unavailable", "message": "daemon unavailable for profile-linked actor add"}})
        return 2

    try:
        require_actor_permission(group, by=by, action="actor.add")
        # Note: role is auto-determined by position (first enabled = foreman)
        if runner != "pty":
            raise ValueError("invalid runner (must be 'pty')")
        if runtime not in ("amp", "auggie", "claude", "codex", "droid", "gemini", "kimi", "neovate", "custom"):
            raise ValueError("invalid runtime")
        if runtime == "custom" and not command:
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
        _print_json(resp)
        return 0 if resp.get("ok") else 2

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
        _print_json(resp)
        return 0 if resp.get("ok") else 2

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
        _print_json(resp)
        return 0 if resp.get("ok") else 2

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
        _print_json(resp)
        return 0 if resp.get("ok") else 2

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
    try:
        profile_id, profile_scope, profile_owner = _actor_profile_link_cli_args(args)
    except ValueError as e:
        _print_json({"ok": False, "error": {"code": "invalid_request", "message": str(e)}})
        return 2
    profile_action = str(getattr(args, "profile_action", "") or "").strip()

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

    if not patch and not profile_id and not profile_action:
        _print_json({"ok": False, "error": {"code": "empty_patch", "message": "nothing to update"}})
        return 2

    if _ensure_daemon_running():
        req_args: dict[str, Any] = {"group_id": group_id, "actor_id": actor_id, "patch": patch, "by": by}
        if profile_id:
            req_args["profile_id"] = profile_id
            req_args["profile_scope"] = profile_scope
            req_args["profile_owner"] = profile_owner
        if profile_action:
            req_args["profile_action"] = profile_action
        resp = call_daemon({"op": "actor_update", "args": req_args})
        _print_json(resp)
        return 0 if resp.get("ok") else 2

    if profile_id or profile_action:
        _print_json({"ok": False, "error": {"code": "daemon_unavailable", "message": "daemon unavailable for actor profile linkage update"}})
        return 2

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
    if bool(getattr(args, "keys", False)) and (
        bool(getattr(args, "set", []) or [])
        or bool(getattr(args, "unset", []) or [])
        or bool(getattr(args, "clear", False))
        or bool(getattr(args, "restart", False))
    ):
        _print_json(
            {
                "ok": False,
                "error": {
                    "code": "invalid_request",
                    "message": "--keys cannot be combined with --set/--unset/--clear/--restart",
                },
            }
        )
        return 2

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
    if not getattr(args, "keys", False) and not set_vars and not unset_keys and not clear:
        _print_json({"ok": False, "error": {"code": "empty_secret_update", "message": "nothing to update"}})
        return 2

    if not _ensure_daemon_running():
        _print_json({"ok": False, "error": {"code": "daemon_unavailable", "message": "daemon unavailable"}})
        return 2

    if getattr(args, "keys", False):
        resp = call_daemon({"op": "actor_env_private_keys", "args": {"group_id": group_id, "actor_id": actor_id, "by": by}})
        _print_json(resp)
        return 0 if resp.get("ok") else 2

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
