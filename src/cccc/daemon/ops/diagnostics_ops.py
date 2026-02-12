"""Diagnostics operation handlers for daemon."""

from __future__ import annotations

import os
from pathlib import Path
from typing import Any, Callable, Dict, Optional

from ...contracts.v1 import DaemonError, DaemonResponse
from ...kernel.actors import find_actor, get_effective_role, list_actors
from ...kernel.group import load_group
from ...kernel.terminal_transcript import get_terminal_transcript_settings
from ...paths import ensure_home
from ...runners import headless as headless_runner
from ...runners import pty as pty_runner
from ...util.conv import coerce_bool
from ...util.time import utc_now_iso
from ... import __version__


def _error(code: str, message: str, *, details: Optional[Dict[str, Any]] = None) -> DaemonResponse:
    return DaemonResponse(ok=False, error=DaemonError(code=code, message=message, details=(details or {})))


def handle_debug_snapshot(
    args: Dict[str, Any],
    *,
    developer_mode_enabled: Callable[[], bool],
    get_observability: Callable[[], Dict[str, Any]],
    effective_runner_kind: Callable[[str], str],
    throttle_debug_summary: Callable[[str], Dict[str, Any]],
) -> DaemonResponse:
    if not developer_mode_enabled():
        return _error("developer_mode_required", "developer mode is disabled")
    group_id = str(args.get("group_id") or "").strip()
    by = str(args.get("by") or "user").strip()
    group = load_group(group_id) if group_id else None
    if group_id and group is None:
        return _error("group_not_found", f"group not found: {group_id}")
    if group is not None and by and by != "user":
        role = get_effective_role(group, by)
        if role != "foreman":
            return _error("permission_denied", "debug tools are restricted to user + foreman")

    try:
        out: Dict[str, Any] = {
            "developer_mode": True,
            "observability": get_observability(),
            "daemon": {"pid": os.getpid(), "version": __version__, "ts": utc_now_iso()},
        }
        if group is not None:
            out["group"] = {
                "group_id": group.group_id,
                "state": str(group.doc.get("state") or "active"),
                "active_scope_key": str(group.doc.get("active_scope_key") or ""),
                "title": str(group.doc.get("title") or ""),
            }
            actors = []
            for actor in list_actors(group):
                if not isinstance(actor, dict):
                    continue
                aid = str(actor.get("id") or "").strip()
                if not aid:
                    continue
                runner_kind = str(actor.get("runner") or "pty")
                runner_effective = effective_runner_kind(runner_kind)
                running = False
                try:
                    if runner_effective == "pty":
                        running = pty_runner.SUPERVISOR.actor_running(group.group_id, aid)
                    elif runner_effective == "headless":
                        running = headless_runner.SUPERVISOR.actor_running(group.group_id, aid)
                except Exception:
                    running = False
                actors.append(
                    {
                        "id": aid,
                        "role": get_effective_role(group, aid),
                        "runtime": str(actor.get("runtime") or ""),
                        "runner": runner_kind,
                        "runner_effective": (runner_effective if runner_effective != runner_kind else runner_kind),
                        "enabled": coerce_bool(actor.get("enabled"), default=True),
                        "running": bool(running),
                        "unread_count": int(actor.get("unread_count") or 0),
                    }
                )
            out["actors"] = actors
            try:
                out["delivery"] = throttle_debug_summary(group.group_id)
            except Exception:
                out["delivery"] = {}
        return DaemonResponse(ok=True, result=out)
    except Exception as e:
        return _error("debug_snapshot_failed", str(e))


def handle_terminal_tail(
    args: Dict[str, Any],
    *,
    can_read_terminal_transcript: Callable[[Any, str, str], bool],
    pty_backlog_bytes: Callable[[], int],
) -> DaemonResponse:
    group_id = str(args.get("group_id") or "").strip()
    actor_id = str(args.get("actor_id") or "").strip()
    by = str(args.get("by") or "user").strip()
    max_chars = int(args.get("max_chars") or 8000)
    strip_ansi = coerce_bool(args.get("strip_ansi"), default=True)
    compact = coerce_bool(args.get("compact"), default=True)
    if not group_id:
        return _error("missing_group_id", "missing group_id")
    if not actor_id:
        return _error("missing_actor_id", "missing actor_id")
    group = load_group(group_id)
    if group is None:
        return _error("group_not_found", f"group not found: {group_id}")
    if not can_read_terminal_transcript(group, by, actor_id):
        tt = get_terminal_transcript_settings(group.doc)
        role = get_effective_role(group, by) if by and by != "user" else ""
        return _error(
            "permission_denied",
            "terminal transcript is restricted by group settings",
            details={
                "visibility": str(tt.get("visibility") or "foreman"),
                "by": by,
                "by_role": role,
                "target_actor_id": actor_id,
                "how_to_enable": "Ask user/foreman to change Settings → Transcript → Visibility.",
            },
        )
    actor = find_actor(group, actor_id)
    if not isinstance(actor, dict):
        return _error("actor_not_found", f"actor not found: {actor_id}")
    runner_kind = str(actor.get("runner") or "pty").strip()
    if runner_kind != "pty":
        return _error("not_pty_actor", "terminal transcript is only available for PTY actors", details={"runner": runner_kind})
    if not pty_runner.SUPERVISOR.actor_running(group_id, actor_id):
        return _error("actor_not_running", "actor is not running (no live transcript available)")
    if max_chars <= 0:
        max_chars = 8000
    if max_chars > 200_000:
        max_chars = 200_000
    try:
        raw = b""
        try:
            raw = pty_runner.SUPERVISOR.tail_output(group_id=group_id, actor_id=actor_id, max_bytes=pty_backlog_bytes())
        except Exception:
            raw = b""
        raw_text = raw.decode("utf-8", errors="replace")
        text = raw_text
        hint = ""
        if strip_ansi:
            try:
                from ...util.terminal_render import render_transcript

                text = render_transcript(text, compact=compact)
            except Exception:
                pass
            if not text.strip() and raw_text.strip():
                hint = "Rendered transcript is empty; try disabling Strip ANSI for full-screen TUIs."
        if len(text) > max_chars:
            text = text[-max_chars:]
        return DaemonResponse(
            ok=True,
            result={
                "group_id": group_id,
                "actor_id": actor_id,
                "warning": "Terminal transcript may include sensitive stdout/stderr.",
                "hint": hint,
                "text": text,
            },
        )
    except Exception as e:
        return _error("terminal_tail_failed", str(e))


def handle_terminal_clear(
    args: Dict[str, Any],
    *,
    can_read_terminal_transcript: Callable[[Any, str, str], bool],
) -> DaemonResponse:
    group_id = str(args.get("group_id") or "").strip()
    actor_id = str(args.get("actor_id") or "").strip()
    by = str(args.get("by") or "user").strip()
    if not group_id:
        return _error("missing_group_id", "missing group_id")
    if not actor_id:
        return _error("missing_actor_id", "missing actor_id")
    group = load_group(group_id)
    if group is None:
        return _error("group_not_found", f"group not found: {group_id}")
    if not can_read_terminal_transcript(group, by, actor_id):
        tt = get_terminal_transcript_settings(group.doc)
        role = get_effective_role(group, by) if by and by != "user" else ""
        return _error(
            "permission_denied",
            "terminal transcript is restricted by group settings",
            details={
                "visibility": str(tt.get("visibility") or "foreman"),
                "by": by,
                "by_role": role,
                "target_actor_id": actor_id,
                "how_to_enable": "Ask user/foreman to change Settings → Transcript → Visibility.",
            },
        )
    actor = find_actor(group, actor_id)
    if not isinstance(actor, dict):
        return _error("actor_not_found", f"actor not found: {actor_id}")
    runner_kind = str(actor.get("runner") or "pty").strip()
    if runner_kind != "pty":
        return _error("not_pty_actor", "terminal transcript is only available for PTY actors", details={"runner": runner_kind})
    ok = pty_runner.SUPERVISOR.clear_backlog(group_id=group_id, actor_id=actor_id)
    if not ok:
        return _error("actor_not_running", "actor is not running (nothing to clear)")
    return DaemonResponse(ok=True, result={"group_id": group_id, "actor_id": actor_id, "cleared": True})


def _debug_component_path(*, component: str, group_id: str) -> tuple[Optional[Path], Optional[DaemonResponse]]:
    home = ensure_home()
    if component in ("daemon", "ccccd"):
        return home / "daemon" / "ccccd.log", None
    if component in ("im", "im_bridge"):
        if not group_id:
            return None, _error("missing_group_id", "missing group_id for im logs")
        return home / "groups" / group_id / "state" / "im_bridge.log", None
    if component in ("web",):
        return home / "daemon" / "cccc-web.log", None
    return None, _error("invalid_component", "unknown component", details={"component": component})


def handle_debug_tail_logs(args: Dict[str, Any], *, developer_mode_enabled: Callable[[], bool]) -> DaemonResponse:
    if not developer_mode_enabled():
        return _error("developer_mode_required", "developer mode is disabled")
    component = str(args.get("component") or "").strip().lower()
    by = str(args.get("by") or "user").strip()
    group_id = str(args.get("group_id") or "").strip()
    lines = int(args.get("lines") or 200)
    if lines <= 0:
        lines = 200
    if lines > 2000:
        lines = 2000

    group = load_group(group_id) if group_id else None
    if group_id and group is None:
        return _error("group_not_found", f"group not found: {group_id}")
    if group is not None and by and by != "user":
        role = get_effective_role(group, by)
        if role != "foreman":
            return _error("permission_denied", "debug tools are restricted to user + foreman")
    try:
        from ...kernel.ledger import read_last_lines

        path, err = _debug_component_path(component=component, group_id=group_id)
        if err is not None:
            return err
        items = read_last_lines(path, int(lines)) if path is not None else []
        return DaemonResponse(ok=True, result={"component": component, "group_id": group_id, "path": str(path) if path else "", "lines": items})
    except Exception as e:
        return _error("debug_tail_logs_failed", str(e))


def handle_debug_clear_logs(args: Dict[str, Any], *, developer_mode_enabled: Callable[[], bool]) -> DaemonResponse:
    if not developer_mode_enabled():
        return _error("developer_mode_required", "developer mode is disabled")
    component = str(args.get("component") or "").strip().lower()
    by = str(args.get("by") or "user").strip()
    group_id = str(args.get("group_id") or "").strip()
    group = load_group(group_id) if group_id else None
    if group_id and group is None:
        return _error("group_not_found", f"group not found: {group_id}")
    if group is not None and by and by != "user":
        role = get_effective_role(group, by)
        if role != "foreman":
            return _error("permission_denied", "debug tools are restricted to user + foreman")
    try:
        path, err = _debug_component_path(component=component, group_id=group_id)
        if err is not None:
            return err
        if path is None:
            return _error("invalid_component", "unknown component", details={"component": component})
        try:
            path.parent.mkdir(parents=True, exist_ok=True)
        except Exception:
            pass
        try:
            with open(path, "w", encoding="utf-8"):
                pass
        except Exception as e:
            return _error("debug_clear_logs_failed", str(e), details={"path": str(path)})
        return DaemonResponse(ok=True, result={"component": component, "group_id": group_id, "path": str(path), "cleared": True})
    except Exception as e:
        return _error("debug_clear_logs_failed", str(e))


def try_handle_diagnostics_op(
    op: str,
    args: Dict[str, Any],
    *,
    developer_mode_enabled: Callable[[], bool],
    get_observability: Callable[[], Dict[str, Any]],
    effective_runner_kind: Callable[[str], str],
    throttle_debug_summary: Callable[[str], Dict[str, Any]],
    can_read_terminal_transcript: Callable[[Any, str, str], bool],
    pty_backlog_bytes: Callable[[], int],
) -> Optional[DaemonResponse]:
    if op == "debug_snapshot":
        return handle_debug_snapshot(
            args,
            developer_mode_enabled=developer_mode_enabled,
            get_observability=get_observability,
            effective_runner_kind=effective_runner_kind,
            throttle_debug_summary=throttle_debug_summary,
        )
    if op == "terminal_tail":
        return handle_terminal_tail(
            args,
            can_read_terminal_transcript=can_read_terminal_transcript,
            pty_backlog_bytes=pty_backlog_bytes,
        )
    if op == "terminal_clear":
        return handle_terminal_clear(
            args,
            can_read_terminal_transcript=can_read_terminal_transcript,
        )
    if op == "debug_tail_logs":
        return handle_debug_tail_logs(args, developer_mode_enabled=developer_mode_enabled)
    if op == "debug_clear_logs":
        return handle_debug_clear_logs(args, developer_mode_enabled=developer_mode_enabled)
    return None
