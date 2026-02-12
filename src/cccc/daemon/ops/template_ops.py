from __future__ import annotations

import re
from pathlib import Path
from typing import Any, Dict, List, Optional, Tuple

from ... import __version__
from ...contracts.v1.ipc import DaemonError, DaemonResponse
from ...kernel.actors import add_actor, find_actor, list_actors, remove_actor, reorder_actors, update_actor, validate_actor_id
from ...kernel.context import ContextStorage
from ...kernel.group import Group, attach_scope_to_group, create_group, load_group
from ...kernel.inbox import delete_cursor, set_cursor
from ...kernel.ledger import append_event
from ...kernel.permissions import require_group_permission
from ...kernel.prompt_files import (
    HELP_FILENAME,
    PREAMBLE_FILENAME,
    delete_group_prompt_file,
    read_group_prompt_file,
    write_group_prompt_file,
)
from ...kernel.runtime import get_runtime_command_with_flags
from ...kernel.scope import detect_scope
from ...kernel.terminal_transcript import apply_terminal_transcript_patch
from ...kernel.group_template import (
    build_group_template_from_group,
    dump_group_template,
    parse_group_template,
    preview_group_template_replace,
)
from ...kernel.registry import load_registry
from ...paths import ensure_home
from ...runners import headless as headless_runner
from ...runners import pty as pty_runner
from ...util.conv import coerce_bool
from ..delivery import THROTTLE, clear_preamble_sent


def _error(code: str, message: str, *, details: Optional[Dict[str, Any]] = None) -> DaemonResponse:
    return DaemonResponse(ok=False, error=DaemonError(code=code, message=message, details=details or {}))


def _slug_filename(value: str) -> str:
    s = re.sub(r"[^a-zA-Z0-9]+", "-", str(value or "").strip()).strip("-").lower()
    return s or "group"

def _remove_runner_state_files(group_id: str, actor_id: str) -> None:
    home = ensure_home()
    gid = str(group_id or "").strip()
    aid = str(actor_id or "").strip()
    if not gid or not aid:
        return

    base = home / "groups" / gid / "state" / "runners"
    for kind in ("pty", "headless"):
        p = base / kind / f"{aid}.json"
        try:
            if p.exists():
                p.unlink()
        except Exception:
            pass


def group_template_export(args: Dict[str, Any]) -> DaemonResponse:
    group_id = str(args.get("group_id") or "").strip()
    if not group_id:
        return _error("missing_group_id", "missing group_id")
    group = load_group(group_id)
    if group is None:
        return _error("group_not_found", f"group not found: {group_id}")

    try:
        tpl = build_group_template_from_group(group, cccc_version=__version__)
    except Exception as e:
        return _error("template_export_failed", str(e))
    text = dump_group_template(tpl)
    title = str(group.doc.get("title") or "").strip()
    filename = f"cccc-group-template--{_slug_filename(title)}.yaml"
    return DaemonResponse(ok=True, result={"template": text, "filename": filename})


def group_template_preview(args: Dict[str, Any]) -> DaemonResponse:
    group_id = str(args.get("group_id") or "").strip()
    by = str(args.get("by") or "user").strip()
    template_text = str(args.get("template") or "")
    if not group_id:
        return _error("missing_group_id", "missing group_id")
    if not template_text.strip():
        return _error("missing_template", "missing template")

    group = load_group(group_id)
    if group is None:
        return _error("group_not_found", f"group not found: {group_id}")
    try:
        require_group_permission(group, by=by, action="group.update")
    except Exception as e:
        return _error("permission_denied", str(e))

    try:
        tpl = parse_group_template(template_text)
    except Exception as e:
        return _error("invalid_template", str(e))

    diff = preview_group_template_replace(group, tpl)
    def _prompt_preview(kind: str, limit: int = 2000) -> Dict[str, Any]:
        raw = getattr(tpl.prompts, kind, None)
        if raw is None:
            return {"source": "builtin"}
        txt = str(raw)
        if not txt.strip():
            return {"source": "builtin"}
        out = txt.strip()
        if len(out) > limit:
            out = out[:limit] + "\n…"
        # Prompts embedded in the template become group overrides under CCCC_HOME.
        return {"source": "home", "chars": len(txt), "preview": out}

    settings = tpl.settings.model_dump()
    actors = [
        {
            "id": a.actor_id,
            "title": a.title,
            "runtime": a.runtime,
            "runner": a.runner,
            "command": a.command,
            "submit": a.submit,
            "enabled": bool(a.enabled),
        }
        for a in tpl.actors
    ]

    settings_changed = {k: {"from": v[0], "to": v[1]} for k, v in diff.settings_changed.items()}
    prompts_changed = {
        k: {
            "changed": v[0],
            "current_chars": v[1],
            "new_chars": v[2],
            "current_source": v[3],
            "new_source": v[4],
        }
        for k, v in diff.prompts_changed.items()
    }

    return DaemonResponse(
        ok=True,
        result={
            "template": {
                "kind": tpl.kind,
                "v": tpl.v,
                "title": tpl.title,
                "topic": tpl.topic,
                "exported_at": tpl.exported_at,
                "cccc_version": tpl.cccc_version,
                "actors": actors,
                "settings": settings,
                "automation": {
                    "rules": len(tpl.automation.rules),
                    "snippets": len(tpl.automation.snippets),
                },
                "prompts": {
                    "preamble": _prompt_preview("preamble"),
                    "help": _prompt_preview("help"),
                },
                },
            "diff": {
                "actors_add": diff.actors_add,
                "actors_update": diff.actors_update,
                "actors_remove": diff.actors_remove,
                "settings_changed": settings_changed,
                "prompts_changed": prompts_changed,
            },
        },
    )


def _normalize_template_actor_command(actor: Any) -> List[str]:
    cmd = getattr(actor, "command", None)
    if isinstance(cmd, list):
        return [str(x).strip() for x in cmd if isinstance(x, str) and str(x).strip()]
    if isinstance(cmd, str):
        import shlex

        s = cmd.strip()
        return shlex.split(s) if s else []
    return []


def _apply_settings_replace(group: Group, settings: Dict[str, Any]) -> Dict[str, Any]:
    """Apply settings to group.doc and return the effective patch we wrote."""
    patch: Dict[str, Any] = {}

    # Messaging policy
    if "default_send_to" in settings:
        v = str(settings.get("default_send_to") or "").strip()
        if v in ("foreman", "broadcast"):
            patch["default_send_to"] = v

    def _int(k: str, *, min_v: int = 0, max_v: Optional[int] = None) -> None:
        if k not in settings:
            return
        try:
            v = int(settings.get(k))
        except Exception:
            return
        if v < min_v:
            v = min_v
        if max_v is not None and v > max_v:
            v = max_v
        patch[k] = v

    _int("nudge_after_seconds", min_v=0)
    _int("reply_required_nudge_after_seconds", min_v=0)
    _int("attention_ack_nudge_after_seconds", min_v=0)
    _int("unread_nudge_after_seconds", min_v=0)
    _int("nudge_digest_min_interval_seconds", min_v=0)
    _int("nudge_max_repeats_per_obligation", min_v=0)
    _int("nudge_escalate_after_repeats", min_v=0)
    _int("actor_idle_timeout_seconds", min_v=0)
    _int("keepalive_delay_seconds", min_v=0)
    _int("keepalive_max_per_actor", min_v=0)
    _int("silence_timeout_seconds", min_v=0)
    _int("help_nudge_interval_seconds", min_v=0)
    _int("help_nudge_min_messages", min_v=0)
    _int("min_interval_seconds", min_v=0)

    # Delivery toggles
    if "auto_mark_on_delivery" in settings:
        patch["auto_mark_on_delivery"] = coerce_bool(settings.get("auto_mark_on_delivery"), default=False)

    # Terminal transcript policy
    if "terminal_transcript_visibility" in settings:
        patch["terminal_transcript_visibility"] = str(settings.get("terminal_transcript_visibility") or "").strip()
    if "terminal_transcript_notify_tail" in settings:
        patch["terminal_transcript_notify_tail"] = coerce_bool(
            settings.get("terminal_transcript_notify_tail"), default=False
        )
    if "terminal_transcript_notify_lines" in settings:
        try:
            n = int(settings.get("terminal_transcript_notify_lines"))
        except Exception:
            n = 20
        patch["terminal_transcript_notify_lines"] = max(1, min(80, n))

    delivery_keys = {"min_interval_seconds", "auto_mark_on_delivery"}
    automation_keys = {
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
    messaging_keys = {"default_send_to"}

    delivery = group.doc.get("delivery") if isinstance(group.doc.get("delivery"), dict) else {}
    automation = group.doc.get("automation") if isinstance(group.doc.get("automation"), dict) else {}
    messaging = group.doc.get("messaging") if isinstance(group.doc.get("messaging"), dict) else {}

    for k, v in patch.items():
        if k in delivery_keys:
            if k == "auto_mark_on_delivery":
                delivery[k] = coerce_bool(v, default=False)
            else:
                delivery[k] = int(v)
        if k in automation_keys:
            automation[k] = int(v)
        if k in messaging_keys:
            messaging["default_send_to"] = str(v)

    group.doc["delivery"] = delivery
    group.doc["automation"] = automation
    group.doc["messaging"] = messaging

    tt_patch: Dict[str, Any] = {}
    if "terminal_transcript_visibility" in patch:
        tt_patch["visibility"] = patch.get("terminal_transcript_visibility")
    if "terminal_transcript_notify_tail" in patch:
        tt_patch["notify_tail"] = patch.get("terminal_transcript_notify_tail")
    if "terminal_transcript_notify_lines" in patch:
        tt_patch["notify_lines"] = patch.get("terminal_transcript_notify_lines")
    if tt_patch:
        apply_terminal_transcript_patch(group.doc, tt_patch)

    group.save()
    return patch


def _apply_prompts_replace(group: Group, prompts: Any) -> List[str]:
    modified: list[str] = []
    for kind, filename in (
        ("preamble", PREAMBLE_FILENAME),
        ("help", HELP_FILENAME),
    ):
        body = getattr(prompts, kind, None)
        if body is None or not str(body).strip():
            pf = read_group_prompt_file(group, filename)
            if pf.found:
                pf2 = delete_group_prompt_file(group, filename)
                modified.append(str(pf2.path or pf.path or filename))
            continue
        pf2 = write_group_prompt_file(group, filename, str(body))
        modified.append(str(pf2.path or filename))
    return modified


def group_template_import_replace(args: Dict[str, Any]) -> DaemonResponse:
    group_id = str(args.get("group_id") or "").strip()
    by = str(args.get("by") or "user").strip()
    confirm = str(args.get("confirm") or "").strip()
    template_text = str(args.get("template") or "")
    if not group_id:
        return _error("missing_group_id", "missing group_id")
    if confirm != group_id:
        return _error("confirmation_required", f"confirm must equal group_id: {group_id}")
    if not template_text.strip():
        return _error("missing_template", "missing template")

    group = load_group(group_id)
    if group is None:
        return _error("group_not_found", f"group not found: {group_id}")
    try:
        require_group_permission(group, by=by, action="group.update")
    except Exception as e:
        return _error("permission_denied", str(e))

    try:
        tpl = parse_group_template(template_text)
    except Exception as e:
        return _error("invalid_template", str(e))

    running_before: Dict[str, bool] = {}
    try:
        for a in list_actors(group):
            aid = str(a.get("id") or "").strip()
            if not aid:
                continue
            running_before[aid] = bool(
                pty_runner.SUPERVISOR.actor_running(group.group_id, aid)
                or headless_runner.SUPERVISOR.actor_running(group.group_id, aid)
            )
    except Exception:
        running_before = {}

    # Stop all runtimes before applying a destructive replace.
    try:
        pty_runner.SUPERVISOR.stop_group(group_id=group.group_id)
        headless_runner.SUPERVISOR.stop_group(group_id=group.group_id)
    except Exception:
        pass

    # Validate actor ids are unique and non-empty.
    seen = set()
    template_ids: List[str] = []
    for a in tpl.actors:
        aid = validate_actor_id(a.actor_id)
        if aid in seen:
            return _error("invalid_template", f"duplicate actor id: {aid}")
        seen.add(aid)
        template_ids.append(aid)

    existing_ids = [str(a.get("id") or "").strip() for a in list_actors(group)]
    existing_ids = [x for x in existing_ids if x]

    remove_ids = [aid for aid in existing_ids if aid not in seen]

    storage = ContextStorage(group)

    removed: list[str] = []
    for aid in remove_ids:
        try:
            pty_runner.SUPERVISOR.stop_actor(group_id=group.group_id, actor_id=aid)
        except Exception:
            pass
        try:
            headless_runner.SUPERVISOR.stop_actor(group_id=group.group_id, actor_id=aid)
        except Exception:
            pass
        try:
            remove_actor(group, aid)
            removed.append(aid)
        except Exception:
            continue

        try:
            _remove_runner_state_files(group.group_id, aid)
        except Exception:
            pass

        try:
            storage.delete_agent_presence(aid)
        except Exception:
            pass
        try:
            delete_cursor(group, aid)
        except Exception:
            pass
        try:
            clear_preamble_sent(group, aid)
        except Exception:
            pass
        try:
            THROTTLE.clear_actor(group.group_id, aid)
        except Exception:
            pass

        try:
            append_event(
                group.ledger_path,
                kind="actor.remove",
                group_id=group.group_id,
                scope_key="",
                by=by,
                data={"actor_id": aid, "runner": ""},
            )
        except Exception:
            pass

    added: list[str] = []
    updated: list[str] = []

    # Apply adds/updates.
    for actor_tpl in tpl.actors:
        aid = validate_actor_id(actor_tpl.actor_id)
        patch = {
            "title": str(actor_tpl.title or "").strip(),
            "runtime": str(actor_tpl.runtime or "codex"),
            "runner": str(actor_tpl.runner or "pty"),
            "enabled": bool(actor_tpl.enabled),
            "submit": str(actor_tpl.submit or "enter"),
            "command": _normalize_template_actor_command(actor_tpl),
        }

        existing = find_actor(group, aid)
        if existing is None:
            # Ensure non-custom PTY actors always have a default command when omitted.
            runner = str(patch.get("runner") or "pty").strip() or "pty"
            runtime = str(patch.get("runtime") or "codex").strip() or "codex"
            command = patch.get("command") if isinstance(patch.get("command"), list) else []
            if runner != "headless" and runtime != "custom" and not command:
                patch["command"] = get_runtime_command_with_flags(runtime)
            if runtime == "custom" and runner != "headless" and not patch.get("command"):
                return _error("invalid_template", f"custom runtime requires a command: {aid}")

            try:
                actor = add_actor(
                    group,
                    actor_id=aid,
                    title=str(patch.get("title") or ""),
                    command=list(patch.get("command") or []),
                    env={},
                    default_scope_key="",
                    submit=str(patch.get("submit") or "enter"),  # type: ignore[arg-type]
                    enabled=bool(patch.get("enabled", True)),
                    runner=str(patch.get("runner") or "pty"),  # type: ignore[arg-type]
                    runtime=str(patch.get("runtime") or "codex"),  # type: ignore[arg-type]
                )
                added.append(aid)
            except Exception as e:
                return _error("template_apply_failed", str(e), details={"actor_id": aid})

            try:
                ev = append_event(
                    group.ledger_path,
                    kind="actor.add",
                    group_id=group.group_id,
                    scope_key="",
                    by=by,
                    data={"actor": actor},
                )
                set_cursor(group, aid, event_id=str(ev.get("id") or ""), ts=str(ev.get("ts") or ""))
            except Exception:
                pass
            continue

        try:
            update_actor(group, aid, patch)
            updated.append(aid)
        except Exception as e:
            return _error("template_apply_failed", str(e), details={"actor_id": aid})

        try:
            append_event(
                group.ledger_path,
                kind="actor.update",
                group_id=group.group_id,
                scope_key="",
                by=by,
                data={"actor_id": aid, "patch": patch},
            )
        except Exception:
            pass

    # Reorder to match template order (and therefore foreman ordering).
    try:
        reorder_actors(group, template_ids)
    except Exception as e:
        return _error("template_apply_failed", str(e))

    # Apply group settings.
    settings_patch = _apply_settings_replace(group, tpl.settings.model_dump())
    if settings_patch:
        try:
            append_event(
                group.ledger_path,
                kind="group.settings_update",
                group_id=group.group_id,
                scope_key="",
                by=by,
                data={"patch": settings_patch},
            )
        except Exception:
            pass

    # Apply group prompt overrides under CCCC_HOME (write custom; delete when built-in).
    try:
        prompt_paths = _apply_prompts_replace(group, tpl.prompts)
    except Exception as e:
        return _error("template_apply_failed", str(e))

    # Apply automation rules/snippets from template (replace semantics, including empty).
    automation_result: Dict[str, Any] = {}
    try:
        ruleset = tpl.automation
        automation = group.doc.get("automation") if isinstance(group.doc.get("automation"), dict) else {}
        template_rules = [r.model_dump(exclude_none=True) for r in (ruleset.rules or [])]
        template_snippets = dict(ruleset.snippets or {})
        automation["rules"] = template_rules
        automation["snippets"] = template_snippets
        try:
            old_version = int(automation.get("version") or 0)
        except Exception:
            old_version = 0
        automation["version"] = max(1, old_version) + 1
        group.doc["automation"] = automation
        group.save()

        automation_result = {
            "rule_ids": [str(r.id) for r in (ruleset.rules or []) if str(r.id or "").strip()],
            "snippet_ids": sorted([str(k) for k in (ruleset.snippets or {}).keys() if str(k or "").strip()]),
        }
        append_event(
            group.ledger_path,
            kind="group.automation_update",
            group_id=group.group_id,
            scope_key="",
            by=by,
            data={
                "rules": list(automation_result["rule_ids"]),
                "snippets": list(automation_result["snippet_ids"]),
            },
        )
    except Exception as e:
        return _error("template_apply_failed", f"failed to apply automation rules: {e}")

    # If we stopped a running actor, clear its status to avoid stale presence.
    try:
        for aid, was_running in running_before.items():
            if not was_running:
                continue
            try:
                storage.clear_agent_status_if_present(aid)
            except Exception:
                pass
    except Exception:
        pass

    return DaemonResponse(
        ok=True,
        result={
            "group_id": group.group_id,
            "applied": True,
            "removed": removed,
            "added": added,
            "updated": updated,
            "settings_patch": settings_patch,
            "prompt_paths": prompt_paths,
            "automation": automation_result,
        },
    )


def group_create_from_template(args: Dict[str, Any]) -> DaemonResponse:
    path = str(args.get("path") or "").strip()
    by = str(args.get("by") or "user").strip()
    title = str(args.get("title") or "working-group").strip()
    topic = str(args.get("topic") or "").strip()
    template_text = str(args.get("template") or "")
    if not path:
        return _error("missing_path", "missing path")
    if not template_text.strip():
        return _error("missing_template", "missing template")

    p = Path(path).expanduser()
    if not p.exists() or not p.is_dir():
        return _error("invalid_path", f"path does not exist: {p}")

    try:
        tpl = parse_group_template(template_text)
    except Exception as e:
        return _error("invalid_template", str(e))

    scope = detect_scope(p)
    reg = load_registry()

    existing_id = str(reg.defaults.get(scope.scope_key) or "").strip()
    if existing_id:
        existing_group = load_group(existing_id)
        if existing_group is not None:
            return _error(
                "scope_already_attached",
                "This directory already has a working group.",
                details={"group_id": existing_id, "path": scope.url},
            )

    group = create_group(reg, title=title, topic=topic)
    group = attach_scope_to_group(reg, group, scope, set_active=True)

    # Ledger events: create + attach (match normal flows).
    try:
        append_event(
            group.ledger_path,
            kind="group.create",
            group_id=group.group_id,
            scope_key="",
            by=by,
            data={"title": group.doc.get("title", ""), "topic": group.doc.get("topic", "")},
        )
    except Exception:
        pass
    try:
        append_event(
            group.ledger_path,
            kind="group.attach",
            group_id=group.group_id,
            scope_key=scope.scope_key,
            by=by,
            data={"url": scope.url, "label": scope.label, "git_remote": scope.git_remote},
        )
    except Exception:
        pass

    # Apply template (no confirmation needed for a new group).
    apply_resp = group_template_import_replace(
        {
            "group_id": group.group_id,
            "by": by,
            "confirm": group.group_id,
            "template": template_text,
        }
    )
    if not apply_resp.ok:
        return apply_resp

    return DaemonResponse(ok=True, result={"group_id": group.group_id, "applied": True})
