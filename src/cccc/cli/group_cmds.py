from __future__ import annotations

"""Group/scope related CLI command handlers."""

from .common import *  # noqa: F401,F403

__all__ = [
    "cmd_attach",
    "cmd_group_create",
    "cmd_group_show",
    "cmd_group_update",
    "cmd_group_detach_scope",
    "cmd_group_delete",
    "cmd_group_use",
    "cmd_group_start",
    "cmd_group_stop",
    "cmd_group_set_state",
    "cmd_groups",
    "cmd_use",
    "cmd_active",
]

def cmd_attach(args: argparse.Namespace) -> int:
    if _ensure_daemon_running():
        resp = call_daemon(
            {"op": "attach", "args": {"path": args.path, "by": "cli", "group_id": str(args.group_id or "")}}
        )
        if resp.get("ok"):
            try:
                gid = str((resp.get("result") or {}).get("group_id") or "").strip()
                if gid:
                    set_active_group_id(gid)
            except Exception:
                pass
            _print_json(resp)
            return 0

    # Fallback: local execution (dev convenience)
    scope = detect_scope(Path(args.path))
    reg = load_registry()
    if args.group_id:
        group = load_group(str(args.group_id))
        if group is None:
            _print_json({"ok": False, "error": f"group not found: {args.group_id}"})
            return 2
        group = attach_scope_to_group(reg, group, scope, set_active=True)
    else:
        group = ensure_group_for_scope(reg, scope)
    append_event(
        group.ledger_path,
        kind="group.attach",
        group_id=group.group_id,
        scope_key=scope.scope_key,
        by="cli",
        data={"url": scope.url, "label": scope.label, "git_remote": scope.git_remote},
    )
    set_active_group_id(group.group_id)
    _print_json(
        {
            "ok": True,
            "result": {"group_id": group.group_id, "scope_key": scope.scope_key, "title": group.doc.get("title")},
        }
    )
    return 0

def cmd_group_create(args: argparse.Namespace) -> int:
    if _ensure_daemon_running():
        resp = call_daemon({"op": "group_create", "args": {"title": args.title, "topic": str(args.topic or ""), "by": "cli"}})
        if resp.get("ok"):
            try:
                gid = str((resp.get("result") or {}).get("group_id") or "").strip()
                if gid:
                    set_active_group_id(gid)
            except Exception:
                pass
            _print_json(resp)
            return 0

    reg = load_registry()
    group = create_group(reg, title=str(args.title or "working-group"), topic=str(args.topic or ""))
    ev = append_event(
        group.ledger_path,
        kind="group.create",
        group_id=group.group_id,
        scope_key="",
        by="cli",
        data={"title": group.doc.get("title", ""), "topic": group.doc.get("topic", "")},
    )
    set_active_group_id(group.group_id)
    _print_json({"ok": True, "result": {"group_id": group.group_id, "title": group.doc.get("title"), "event": ev}})
    return 0

def cmd_group_show(args: argparse.Namespace) -> int:
    group = load_group(args.group_id)
    if group is None:
        _print_json({"ok": False, "error": {"code": "group_not_found", "message": f"group not found: {args.group_id}"}})
        return 2
    _print_json({"ok": True, "result": {"group": group.doc}})
    return 0

def cmd_group_update(args: argparse.Namespace) -> int:
    group_id = _resolve_group_id(getattr(args, "group", ""))
    if not group_id:
        _print_json({"ok": False, "error": {"code": "missing_group_id", "message": "missing group_id (no active group?)"}})
        return 2
    by = str(args.by or "user").strip()

    patch: dict[str, Any] = {}
    if args.title is not None:
        title = str(args.title or "").strip()
        if not title:
            _print_json({"ok": False, "error": {"code": "invalid_title", "message": "title cannot be empty"}})
            return 2
        patch["title"] = title
    if args.topic is not None:
        patch["topic"] = str(args.topic or "")
    if not patch:
        _print_json({"ok": False, "error": {"code": "invalid_patch", "message": "provide --title and/or --topic"}})
        return 2

    if _ensure_daemon_running():
        resp = call_daemon({"op": "group_update", "args": {"group_id": group_id, "by": by, "patch": patch}})
        if resp.get("ok"):
            _print_json(resp)
            return 0

    group = load_group(group_id)
    if group is None:
        _print_json({"ok": False, "error": {"code": "group_not_found", "message": f"group not found: {group_id}"}})
        return 2
    try:
        require_group_permission(group, by=by, action="group.update")
        reg = load_registry()
        group = update_group(reg, group, patch=dict(patch))
    except Exception as e:
        _print_json({"ok": False, "error": {"code": "group_update_failed", "message": str(e)}})
        return 2
    ev = append_event(group.ledger_path, kind="group.update", group_id=group.group_id, scope_key="", by=by, data={"patch": dict(patch)})
    _print_json({"ok": True, "result": {"group_id": group.group_id, "group": group.doc, "event": ev}})
    return 0

def cmd_group_detach_scope(args: argparse.Namespace) -> int:
    group_id = _resolve_group_id(getattr(args, "group", ""))
    if not group_id:
        _print_json({"ok": False, "error": {"code": "missing_group_id", "message": "missing group_id (no active group?)"}})
        return 2
    by = str(args.by or "user").strip()
    scope_key = str(args.scope_key or "").strip()
    if not scope_key:
        _print_json({"ok": False, "error": {"code": "missing_scope_key", "message": "missing scope_key"}})
        return 2

    if _ensure_daemon_running():
        resp = call_daemon({"op": "group_detach_scope", "args": {"group_id": group_id, "scope_key": scope_key, "by": by}})
        if resp.get("ok"):
            _print_json(resp)
            return 0

    group = load_group(group_id)
    if group is None:
        _print_json({"ok": False, "error": {"code": "group_not_found", "message": f"group not found: {group_id}"}})
        return 2
    try:
        require_group_permission(group, by=by, action="group.detach_scope")
        reg = load_registry()
        group = detach_scope_from_group(reg, group, scope_key=scope_key)
    except Exception as e:
        _print_json({"ok": False, "error": {"code": "group_detach_scope_failed", "message": str(e)}})
        return 2
    ev = append_event(
        group.ledger_path,
        kind="group.detach_scope",
        group_id=group.group_id,
        scope_key=scope_key,
        by=by,
        data={"scope_key": scope_key},
    )
    _print_json({"ok": True, "result": {"group_id": group.group_id, "event": ev}})
    return 0

def cmd_group_delete(args: argparse.Namespace) -> int:
    group_id = _resolve_group_id(getattr(args, "group", ""))
    if not group_id:
        _print_json({"ok": False, "error": {"code": "missing_group_id", "message": "missing group_id (no active group?)"}})
        return 2
    by = str(args.by or "user").strip()
    confirm = str(args.confirm or "").strip()
    if confirm != group_id:
        _print_json({"ok": False, "error": {"code": "confirm_required", "message": f"pass --confirm {group_id} to delete"}})
        return 2

    if not _ensure_daemon_running():
        _print_json({"ok": False, "error": {"code": "daemon_unavailable", "message": "ccccd unavailable"}})
        return 2
    resp = call_daemon({"op": "group_delete", "args": {"group_id": group_id, "by": by}})
    _print_json(resp)
    return 0 if resp.get("ok") else 2

def cmd_group_use(args: argparse.Namespace) -> int:
    if _ensure_daemon_running():
        resp = call_daemon({"op": "group_use", "args": {"group_id": args.group_id, "path": args.path, "by": "cli"}})
        if resp.get("ok"):
            _print_json(resp)
            return 0

    group = load_group(args.group_id)
    if group is None:
        _print_json({"ok": False, "error": {"code": "group_not_found", "message": f"group not found: {args.group_id}"}})
        return 2
    scope = detect_scope(Path(args.path))
    reg = load_registry()
    try:
        group = set_active_scope(reg, group, scope_key=scope.scope_key)
    except ValueError as e:
        _print_json(
            {
                "ok": False,
                "error": {"code": "scope_not_attached", "message": str(e), "details": {"hint": "cccc attach <path> --group <id>"}},
            }
        )
        return 2
    ev = append_event(
        group.ledger_path,
        kind="group.set_active_scope",
        group_id=group.group_id,
        scope_key=scope.scope_key,
        by="cli",
        data={"path": scope.url},
    )
    _print_json({"ok": True, "result": {"group_id": group.group_id, "active_scope_key": scope.scope_key, "event": ev}})
    return 0

def cmd_group_start(args: argparse.Namespace) -> int:
    group_id = _resolve_group_id(getattr(args, "group", ""))
    if not group_id:
        _print_json({"ok": False, "error": {"code": "missing_group_id", "message": "missing group_id (no active group?)"}})
        return 2
    by = str(args.by or "user").strip()

    if not _ensure_daemon_running():
        _print_json({"ok": False, "error": {"code": "daemon_unavailable", "message": "ccccd unavailable"}})
        return 2
    resp = call_daemon({"op": "group_start", "args": {"group_id": group_id, "by": by}})
    _print_json(resp)
    return 0 if resp.get("ok") else 2

def cmd_group_stop(args: argparse.Namespace) -> int:
    group_id = _resolve_group_id(getattr(args, "group", ""))
    if not group_id:
        _print_json({"ok": False, "error": {"code": "missing_group_id", "message": "missing group_id (no active group?)"}})
        return 2
    by = str(args.by or "user").strip()

    if not _ensure_daemon_running():
        _print_json({"ok": False, "error": {"code": "daemon_unavailable", "message": "ccccd unavailable"}})
        return 2
    resp = call_daemon({"op": "group_stop", "args": {"group_id": group_id, "by": by}})
    _print_json(resp)
    return 0 if resp.get("ok") else 2

def cmd_group_set_state(args: argparse.Namespace) -> int:
    """Set group state (active/idle/paused/stopped)."""
    group_id = _resolve_group_id(getattr(args, "group", ""))
    if not group_id:
        _print_json({"ok": False, "error": {"code": "missing_group_id", "message": "missing group_id (no active group?)"}})
        return 2
    state = str(args.state or "").strip()
    by = str(args.by or "user").strip()

    if not _ensure_daemon_running():
        _print_json({"ok": False, "error": {"code": "daemon_unavailable", "message": "ccccd unavailable"}})
        return 2
    if state == "stopped":
        resp = call_daemon({"op": "group_stop", "args": {"group_id": group_id, "by": by}})
    else:
        resp = call_daemon({"op": "group_set_state", "args": {"group_id": group_id, "state": state, "by": by}})
    _print_json(resp)
    return 0 if resp.get("ok") else 2

def cmd_groups(args: argparse.Namespace) -> int:
    resp = call_daemon({"op": "groups"})
    if resp.get("ok"):
        _print_json(resp)
        return 0
    reg = load_registry()
    groups = list(reg.groups.values())
    groups.sort(key=lambda g: (g.get("updated_at") or "", g.get("created_at") or ""), reverse=True)
    _print_json({"ok": True, "result": {"groups": groups}})
    return 0

def cmd_use(args: argparse.Namespace) -> int:
    group_id = str(args.group_id or "").strip()
    group = load_group(group_id)
    if group is None:
        _print_json({"ok": False, "error": {"code": "group_not_found", "message": f"group not found: {group_id}"}})
        return 2
    doc = set_active_group_id(group_id)
    _print_json({"ok": True, "result": doc})
    return 0

def cmd_active(_: argparse.Namespace) -> int:
    doc = load_active()
    _print_json({"ok": True, "result": doc})
    return 0
