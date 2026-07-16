"""Core group operation handlers for daemon."""

from __future__ import annotations

import copy
from pathlib import Path
from typing import Any, Callable, Dict, Optional

from ...contracts.v1 import DaemonError, DaemonResponse
from ..actors.private_env_ops import copy_group_private_env
from ..claude_app_sessions import SUPERVISOR as claude_app_supervisor
from ..codex_app_sessions import SUPERVISOR as codex_app_supervisor
from ...kernel.active import load_active, set_active_group_id
from ...kernel.group import (
    attach_scope_to_group,
    create_group,
    default_automation_ruleset_doc,
    delete_group,
    detach_scope_from_group,
    load_group,
    set_active_scope,
    update_group,
)
from ...kernel.ledger import append_event
from ...kernel.permissions import require_group_permission
from ...kernel.registry import load_registry
from ...kernel.scope import ScopeIdentity, detect_scope
from ...runners import headless as headless_runner
from ...runners import pty as pty_runner
from ...paths import ensure_home
from ...util.time import utc_now_iso


def _error(code: str, message: str, *, details: Optional[Dict[str, Any]] = None) -> DaemonResponse:
    return DaemonResponse(ok=False, error=DaemonError(code=code, message=message, details=(details or {})))


def _is_exact_cccc_home_path(path: Path) -> bool:
    try:
        return path.expanduser().resolve() == ensure_home().resolve()
    except Exception:
        return False


def _redact_group_doc(doc: Dict[str, Any]) -> Dict[str, Any]:
    """Redact secrets from group doc before returning to clients."""
    try:
        out = copy.deepcopy(doc)
    except Exception:
        out = dict(doc or {})

    im = out.get("im")
    if isinstance(im, dict):
        im.pop("token", None)
        im.pop("bot_token", None)
        im.pop("app_token", None)
    return out


def handle_group_show(args: Dict[str, Any]) -> DaemonResponse:
    group_id = str(args.get("group_id") or "").strip()
    if not group_id:
        return _error("missing_group_id", "missing group_id")
    group = load_group(group_id)
    if group is None:
        return _error("group_not_found", f"group not found: {group_id}")
    return DaemonResponse(ok=True, result={"group": _redact_group_doc(group.doc)})


def handle_group_update(args: Dict[str, Any]) -> DaemonResponse:
    group_id = str(args.get("group_id") or "").strip()
    by = str(args.get("by") or "user").strip()
    patch = args.get("patch") if isinstance(args.get("patch"), dict) else {}
    if not group_id:
        return _error("missing_group_id", "missing group_id")
    group = load_group(group_id)
    if group is None:
        return _error("group_not_found", f"group not found: {group_id}")
    allowed = {"title", "topic"}
    unknown = set(patch.keys()) - allowed
    if unknown:
        return _error("invalid_patch", "invalid patch keys", details={"unknown_keys": sorted(unknown)})
    if not patch:
        return _error("invalid_patch", "empty patch")
    try:
        require_group_permission(group, by=by, action="group.update")
        reg = load_registry()
        group = update_group(reg, group, patch=dict(patch))
    except Exception as e:
        return _error("group_update_failed", str(e))
    event = append_event(
        group.ledger_path,
        kind="group.update",
        group_id=group.group_id,
        scope_key="",
        by=by,
        data={"patch": dict(patch)},
    )
    return DaemonResponse(ok=True, result={"group_id": group.group_id, "group": _redact_group_doc(group.doc), "event": event})


def handle_group_detach_scope(args: Dict[str, Any]) -> DaemonResponse:
    group_id = str(args.get("group_id") or "").strip()
    by = str(args.get("by") or "user").strip()
    scope_key = str(args.get("scope_key") or "").strip()
    if not group_id:
        return _error("missing_group_id", "missing group_id")
    if not scope_key:
        return _error("missing_scope_key", "missing scope_key")
    group = load_group(group_id)
    if group is None:
        return _error("group_not_found", f"group not found: {group_id}")
    try:
        require_group_permission(group, by=by, action="group.detach_scope")
        reg = load_registry()
        group = detach_scope_from_group(reg, group, scope_key=scope_key)
    except Exception as e:
        return _error("group_detach_scope_failed", str(e))
    event = append_event(
        group.ledger_path,
        kind="group.detach_scope",
        group_id=group.group_id,
        scope_key=scope_key,
        by=by,
        data={"scope_key": scope_key},
    )
    return DaemonResponse(ok=True, result={"group_id": group.group_id, "event": event})


def handle_group_delete(
    args: Dict[str, Any],
    *,
    stop_im_bridges_for_group: Callable[[str], None],
    delete_group_private_env: Callable[[str], None],
) -> DaemonResponse:
    group_id = str(args.get("group_id") or "").strip()
    by = str(args.get("by") or "user").strip()
    if not group_id:
        return _error("missing_group_id", "missing group_id")
    group = load_group(group_id)
    if group is None:
        return _error("group_not_found", f"group not found: {group_id}")
    try:
        require_group_permission(group, by=by, action="group.delete")
        stop_im_bridges_for_group(group_id)
        codex_app_supervisor.stop_group(group_id=group_id)
        claude_app_supervisor.stop_group(group_id=group_id)
        pty_runner.SUPERVISOR.stop_group(group_id=group_id)
        headless_runner.SUPERVISOR.stop_group(group_id=group_id)
        delete_group_private_env(group_id)
        reg = load_registry()
        delete_group(reg, group_id=group_id)
        active = load_active()
        if str(active.get("active_group_id") or "") == group_id:
            set_active_group_id("")
    except Exception as e:
        return _error("group_delete_failed", str(e))
    return DaemonResponse(ok=True, result={"group_id": group_id})


_RESET_ACTOR_CONFIG_KEYS = {
    "v",
    "id",
    "role",
    "title",
    "command",
    "env",
    "default_scope_key",
    "submit",
    "capability_autoload",
    "capability_hidden",
    "enabled",
    "runner",
    "runtime",
    "runtime_state_source",
    "internal_kind",
    "profile_id",
    "profile_scope",
    "profile_owner",
    "profile_revision_applied",
    "created_at",
    "updated_at",
}


_RESET_AUTOMATION_CONFIG_KEYS = {
    "version",
    "rules",
    "snippets",
    "snippet_overrides",
    "nudge_after_seconds",
    "reply_required_nudge_after_seconds",
    "attention_ack_nudge_after_seconds",
    "unread_nudge_after_seconds",
    "nudge_digest_min_interval_seconds",
    "nudge_max_repeats_per_obligation",
    "nudge_escalate_after_repeats",
    "actor_idle_timeout_seconds",
    "keepalive_delay_seconds",
    "keepalive_max_per_actor",
    "silence_timeout_seconds",
    "help_nudge_interval_seconds",
    "help_nudge_min_messages",
}


def _reset_automation_config(raw: Any) -> Optional[Dict[str, Any]]:
    if not isinstance(raw, dict):
        return None
    seed = default_automation_ruleset_doc()
    out = copy.deepcopy(seed)
    for key in _RESET_AUTOMATION_CONFIG_KEYS:
        if key in raw:
            out[key] = copy.deepcopy(raw.get(key))
    if not isinstance(out.get("rules"), list):
        out["rules"] = copy.deepcopy(seed.get("rules") if isinstance(seed.get("rules"), list) else [])
    if not isinstance(out.get("snippets"), dict):
        out["snippets"] = {}
    if not isinstance(out.get("snippet_overrides"), dict):
        out["snippet_overrides"] = {}
    try:
        version = int(out.get("version") or 0)
    except Exception:
        version = 0
    out["version"] = max(1, version)
    return out


def _reset_actor_config(raw: Any, *, now: str) -> Optional[Dict[str, Any]]:
    if not isinstance(raw, dict):
        return None
    actor_id = str(raw.get("id") or "").strip()
    if not actor_id:
        return None
    out: Dict[str, Any] = {}
    for key in _RESET_ACTOR_CONFIG_KEYS:
        if key in raw:
            out[key] = copy.deepcopy(raw.get(key))
    out["id"] = actor_id
    out["avatar_asset_path"] = ""
    out["created_at"] = str(out.get("created_at") or now)
    out["updated_at"] = now
    return out


def _reset_scope_configs(raw: Any) -> list[Dict[str, str]]:
    if not isinstance(raw, list):
        return []
    out: list[Dict[str, str]] = []
    seen: set[str] = set()
    for item in raw:
        if not isinstance(item, dict):
            continue
        scope_key = str(item.get("scope_key") or "").strip()
        url = str(item.get("url") or "").strip()
        if not scope_key or not url or scope_key in seen:
            continue
        label = str(item.get("label") or "").strip()
        if not label:
            label = Path(url).name or "scope"
        out.append(
            {
                "scope_key": scope_key,
                "url": url,
                "label": label,
                "git_remote": str(item.get("git_remote") or "").strip(),
            }
        )
        seen.add(scope_key)
    return out


def _delete_group_for_reset(
    group_id: str,
    *,
    stop_im_bridges_for_group: Callable[[str], None],
    delete_group_private_env: Callable[[str], None],
) -> Optional[str]:
    try:
        stop_im_bridges_for_group(group_id)
        codex_app_supervisor.stop_group(group_id=group_id)
        claude_app_supervisor.stop_group(group_id=group_id)
        pty_runner.SUPERVISOR.stop_group(group_id=group_id)
        headless_runner.SUPERVISOR.stop_group(group_id=group_id)
        delete_group_private_env(group_id)
        reg = load_registry()
        delete_group(reg, group_id=group_id)
        return None
    except Exception as e:
        return str(e)


def handle_group_reset(
    args: Dict[str, Any],
    *,
    stop_im_bridges_for_group: Callable[[str], None],
    delete_group_private_env: Callable[[str], None],
) -> DaemonResponse:
    group_id = str(args.get("group_id") or "").strip()
    by = str(args.get("by") or "user").strip()
    confirm = str(args.get("confirm") or "").strip()
    if not group_id:
        return _error("missing_group_id", "missing group_id")
    if confirm != group_id:
        return _error("confirm_required", f"confirm must equal group_id: {group_id}")

    source = load_group(group_id)
    if source is None:
        return _error("group_not_found", f"group not found: {group_id}")

    try:
        require_group_permission(source, by=by, action="group.delete")
    except Exception as e:
        return _error("group_reset_forbidden", str(e))

    title = str(source.doc.get("title") or "").strip() or "working-group"
    topic = str(source.doc.get("topic") or "").strip()
    now = utc_now_iso()
    scopes = _reset_scope_configs(source.doc.get("scopes"))
    active_scope_key = str(source.doc.get("active_scope_key") or "").strip()
    actors = [
        actor
        for actor in (_reset_actor_config(item, now=now) for item in source.doc.get("actors", []))
        if actor is not None
    ]
    automation = _reset_automation_config(source.doc.get("automation"))
    was_active = str(load_active().get("active_group_id") or "").strip() == group_id

    try:
        reg = load_registry()
        replacement = create_group(reg, title=title, topic=topic)
        for scope in scopes:
            replacement = attach_scope_to_group(
                reg,
                replacement,
                ScopeIdentity(
                    url=scope["url"],
                    scope_key=scope["scope_key"],
                    label=scope["label"],
                    git_remote=scope["git_remote"],
                ),
                set_active=scope["scope_key"] == active_scope_key,
            )
        replacement.doc["title"] = title
        replacement.doc["topic"] = topic
        replacement.doc["running"] = False
        replacement.doc["state"] = "active"
        replacement.doc["actors"] = actors
        if automation is not None:
            replacement.doc["automation"] = automation
        if scopes and not str(replacement.doc.get("active_scope_key") or "").strip():
            replacement.doc["active_scope_key"] = scopes[0]["scope_key"]
        replacement.save()
        copied_private_env_files = copy_group_private_env(group_id, replacement.group_id)
        create_event = append_event(
            replacement.ledger_path,
            kind="group.create",
            group_id=replacement.group_id,
            scope_key="",
            by=by,
            data={"title": title, "topic": topic},
        )
    except Exception as e:
        return _error("group_reset_failed", str(e))

    old_delete_error = _delete_group_for_reset(
        group_id,
        stop_im_bridges_for_group=stop_im_bridges_for_group,
        delete_group_private_env=delete_group_private_env,
    )
    if was_active:
        set_active_group_id(replacement.group_id)

    result: Dict[str, Any] = {
        "old_group_id": group_id,
        "new_group_id": replacement.group_id,
        "group_id": replacement.group_id,
        "deleted_old": old_delete_error is None,
        "private_env_files_copied": copied_private_env_files,
        "event": create_event,
    }
    if old_delete_error:
        result["old_delete_error"] = old_delete_error
    if was_active:
        result["active_group_id"] = replacement.group_id
    return DaemonResponse(ok=True, result=result)


def handle_group_use(args: Dict[str, Any]) -> DaemonResponse:
    group_id = str(args.get("group_id") or "").strip()
    if not group_id:
        return _error("missing_group_id", "missing group_id")
    group = load_group(group_id)
    if group is None:
        return _error("group_not_found", f"group not found: {group_id}")
    path = Path(str(args.get("path") or "."))
    if _is_exact_cccc_home_path(path):
        return _error("invalid_scope_path", "workspace scope must be a project directory, not CCCC_HOME")
    scope = detect_scope(path)
    reg = load_registry()
    try:
        group = set_active_scope(reg, group, scope_key=scope.scope_key)
    except ValueError as e:
        return _error(
            "scope_not_attached",
            str(e),
            details={"hint": "attach scope first (cccc attach <path> --group <id>)"},
        )
    event = append_event(
        group.ledger_path,
        kind="group.set_active_scope",
        group_id=group.group_id,
        scope_key=scope.scope_key,
        by=str(args.get("by") or "cli"),
        data={"path": scope.url},
    )
    return DaemonResponse(
        ok=True,
        result={"group_id": group.group_id, "active_scope_key": scope.scope_key, "event": event},
    )


def try_handle_group_core_op(
    op: str,
    args: Dict[str, Any],
    *,
    stop_im_bridges_for_group: Optional[Callable[[str], None]] = None,
    delete_group_private_env: Optional[Callable[[str], None]] = None,
) -> Optional[DaemonResponse]:
    if op == "group_show":
        return handle_group_show(args)
    if op == "group_update":
        return handle_group_update(args)
    if op == "group_detach_scope":
        return handle_group_detach_scope(args)
    if op == "group_delete":
        if stop_im_bridges_for_group is None or delete_group_private_env is None:
            return _error("internal_error", "group_delete callbacks not configured")
        return handle_group_delete(
            args,
            stop_im_bridges_for_group=stop_im_bridges_for_group,
            delete_group_private_env=delete_group_private_env,
        )
    if op == "group_reset":
        if stop_im_bridges_for_group is None or delete_group_private_env is None:
            return _error("internal_error", "group_reset callbacks not configured")
        return handle_group_reset(
            args,
            stop_im_bridges_for_group=stop_im_bridges_for_group,
            delete_group_private_env=delete_group_private_env,
        )
    if op == "group_use":
        return handle_group_use(args)
    return None
