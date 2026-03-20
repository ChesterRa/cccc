"""Diagnostics operation handlers for daemon."""

from __future__ import annotations

import io
import json
import os
import re
import zipfile
from pathlib import Path
from typing import Any, Callable, Dict, Optional

from ...contracts.v1 import DaemonError, DaemonResponse
from ...kernel.actors import find_actor, get_effective_role, list_actors
from ...kernel.blobs import sanitize_filename, store_blob_bytes
from ...kernel.group import load_group
from ...kernel.ledger import read_last_lines
from ...kernel.settings import get_remote_access_settings, resolve_remote_access_web_binding
from ...kernel.terminal_transcript import get_terminal_transcript_settings
from ...paths import ensure_home
from ...ports.web.runtime_control import (
    http_url,
    is_loopback_host,
    is_wildcard_host,
    local_display_url,
    read_web_runtime_state,
    web_runtime_log_path,
    web_runtime_pid_candidates,
)
from ...runners import headless as headless_runner
from ...runners import pty as pty_runner
from ...util.conv import coerce_bool
from ...util.process import pid_is_alive
from ...util.time import utc_now_iso
from ... import __version__


_ASSIGNMENT_RE = re.compile(
    r"(?im)(\b[A-Z0-9_]*(?:TOKEN|SECRET|PASSWORD|PASSWD|API_KEY|ACCESS_KEY|PRIVATE_KEY|SESSION|COOKIE|AUTH)[A-Z0-9_]*\b\s*[:=]\s*)([^\s,;]+)"
)
_JSON_SECRET_RE = re.compile(
    r'(?im)("?[A-Z0-9_]*(?:TOKEN|SECRET|PASSWORD|PASSWD|API_KEY|ACCESS_KEY|PRIVATE_KEY|SESSION|COOKIE|AUTH)[A-Z0-9_]*"?\s*:\s*)(".*?"|\'.*?\'|[^,\s}]+)'
)
_AUTH_BEARER_RE = re.compile(r"(?im)(authorization\s*:\s*bearer\s+)([^\s]+)")
_COOKIE_RE = re.compile(r"(?im)(cookie\s*:\s*)(.+)")
_OPENAI_KEY_RE = re.compile(r"\bsk-[A-Za-z0-9_-]{8,}\b")
_ANTHROPIC_KEY_RE = re.compile(r"\bsk-ant-[A-Za-z0-9_-]{8,}\b")


def _error(code: str, message: str, *, details: Optional[Dict[str, Any]] = None) -> DaemonResponse:
    return DaemonResponse(ok=False, error=DaemonError(code=code, message=message, details=(details or {})))


def _safe_int(value: Any) -> int:
    try:
        return int(value or 0)
    except Exception:
        return 0


def _binding_remote_url(host: str, port: int, public_url: str) -> Optional[str]:
    direct_url = str(public_url or "").strip()
    if direct_url:
        return direct_url
    normalized_host = str(host or "").strip() or "127.0.0.1"
    if is_loopback_host(normalized_host):
        return None
    if is_wildcard_host(normalized_host):
        return http_url("<your-lan-ip>", int(port))
    return http_url(normalized_host, int(port))


def _web_exposure_class(host: str, public_url: str) -> str:
    if str(public_url or "").strip():
        return "public"
    return "local" if is_loopback_host(host) else "private"


def _authorize_debug_group_access(*, group_id: str, by: str) -> tuple[Any | None, DaemonResponse | None]:
    group = load_group(group_id) if group_id else None
    if group_id and group is None:
        return None, _error("group_not_found", f"group not found: {group_id}")
    if group is not None and by and by != "user":
        role = get_effective_role(group, by)
        if role != "foreman":
            return None, _error("permission_denied", "debug tools are restricted to user + foreman")
    return group, None


def _build_web_debug_snapshot(*, home: Path) -> Dict[str, Any]:
    binding = resolve_remote_access_web_binding()
    remote_cfg = get_remote_access_settings()
    runtime = read_web_runtime_state(home)

    host = str(binding.get("web_host") or "").strip() or "127.0.0.1"
    port = _safe_int(binding.get("web_port")) or 8848
    public_url = str(binding.get("web_public_url") or "").strip()
    exposure_class = _web_exposure_class(host, public_url)

    runtime_pid = _safe_int(runtime.get("pid"))
    launcher_pid = _safe_int(runtime.get("launcher_pid"))
    runtime_host = str(runtime.get("host") or "").strip()
    runtime_port = _safe_int(runtime.get("port"))
    runtime_pid_alive = runtime_pid > 0 and pid_is_alive(runtime_pid)
    launcher_pid_alive = launcher_pid > 0 and pid_is_alive(launcher_pid)
    pid_candidates = web_runtime_pid_candidates(runtime)
    live_pid_candidates = [pid for pid in pid_candidates if pid > 0 and pid_is_alive(pid)]

    runtime_matches_binding = bool(
        runtime_pid_alive
        and runtime_host == host
        and runtime_port == port
    )

    issues: list[str] = []
    if runtime_pid > 0 and not runtime_pid_alive:
        issues.append("runtime_pid_stale")
    if launcher_pid > 0 and not launcher_pid_alive:
        issues.append("launcher_pid_stale")
    if runtime_pid_alive and not runtime_matches_binding:
        issues.append("binding_apply_pending")
    if str(runtime.get("last_apply_error") or "").strip():
        issues.append("last_apply_error")

    return {
        "configured": {
            "host": host,
            "host_source": str(binding.get("web_host_source") or "default"),
            "port": int(port),
            "port_source": str(binding.get("web_port_source") or "default"),
            "public_url": (public_url or None),
            "public_url_source": str(binding.get("web_public_url_source") or "none"),
            "exposure_class": exposure_class,
            "desired_local_url": local_display_url(host, port),
            "desired_remote_url": _binding_remote_url(host, port, public_url),
        },
        "remote_access": {
            "provider": str(remote_cfg.get("provider") or "off"),
            "enabled": bool(remote_cfg.get("enabled")),
            "require_access_token": bool(remote_cfg.get("require_access_token")),
        },
        "runtime": {
            "pid": (runtime_pid if runtime_pid > 0 else None),
            "pid_alive": bool(runtime_pid_alive),
            "launcher_pid": (launcher_pid if launcher_pid > 0 else None),
            "launcher_pid_alive": bool(launcher_pid_alive),
            "pid_candidates": pid_candidates,
            "live_pid_candidates": live_pid_candidates,
            "host": (runtime_host or None),
            "port": (runtime_port if runtime_port > 0 else None),
            "mode": (str(runtime.get("mode") or "").strip() or None),
            "started_at": (str(runtime.get("started_at") or "").strip() or None),
            "supervisor_managed": bool(runtime.get("supervisor_managed")),
            "supervisor_pid": (_safe_int(runtime.get("supervisor_pid")) or None),
            "launch_source": (str(runtime.get("launch_source") or "").strip() or None),
            "last_apply_error": (str(runtime.get("last_apply_error") or "").strip() or None),
        },
        "runtime_matches_configured_binding": bool(runtime_matches_binding),
        "log_path": str(web_runtime_log_path(home)),
        "issues": issues,
    }


def _build_debug_snapshot_result(
    *,
    home: Path,
    group: Any | None,
    get_observability: Callable[[], Dict[str, Any]],
    effective_runner_kind: Callable[[str], str],
    throttle_debug_summary: Callable[[str], Dict[str, Any]],
) -> Dict[str, Any]:
    out: Dict[str, Any] = {
        "developer_mode": True,
        "observability": get_observability(),
        "daemon": {
            "pid": os.getpid(),
            "version": __version__,
            "ts": utc_now_iso(),
            "log_path": str(home / "daemon" / "ccccd.log"),
        },
        "web": _build_web_debug_snapshot(home=home),
    }
    if group is None:
        return out
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
    return out


def _redact_sensitive_text(text: str) -> str:
    if not text:
        return ""
    out = str(text)
    out = _AUTH_BEARER_RE.sub(r"\1[REDACTED]", out)
    out = _COOKIE_RE.sub(r"\1[REDACTED]", out)
    out = _ASSIGNMENT_RE.sub(r"\1[REDACTED]", out)
    out = _JSON_SECRET_RE.sub(r'\1"[REDACTED]"', out)
    out = _OPENAI_KEY_RE.sub("[REDACTED]", out)
    out = _ANTHROPIC_KEY_RE.sub("[REDACTED]", out)
    return out


def _collect_terminal_snapshot(*, group_id: str, actor_id: str, max_chars: int, compact: bool = True) -> str:
    try:
        raw = pty_runner.SUPERVISOR.tail_output(group_id=group_id, actor_id=actor_id, max_bytes=max_chars * 4)
    except Exception:
        return ""
    raw_text = raw.decode("utf-8", errors="replace")
    text = raw_text
    try:
        from ...util.terminal_render import render_transcript

        text = render_transcript(raw_text, compact=compact)
    except Exception:
        text = raw_text
    if len(text) > max_chars:
        text = text[-max_chars:]
    return _redact_sensitive_text(text)


def _collect_log_tail(path: Path | None, *, lines: int) -> str:
    if path is None or not path.exists() or not path.is_file():
        return ""
    try:
        return _redact_sensitive_text("\n".join(read_last_lines(path, int(lines))))
    except Exception:
        return ""


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
    group, auth_err = _authorize_debug_group_access(group_id=group_id, by=by)
    if auth_err is not None:
        return auth_err

    try:
        home = ensure_home()
        out = _build_debug_snapshot_result(
            home=home,
            group=group,
            get_observability=get_observability,
            effective_runner_kind=effective_runner_kind,
            throttle_debug_summary=throttle_debug_summary,
        )
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


def handle_feedback_bundle_export(
    args: Dict[str, Any],
    *,
    developer_mode_enabled: Callable[[], bool],
    get_observability: Callable[[], Dict[str, Any]],
    effective_runner_kind: Callable[[str], str],
    throttle_debug_summary: Callable[[str], Dict[str, Any]],
    pty_backlog_bytes: Callable[[], int],
) -> DaemonResponse:
    if not developer_mode_enabled():
        return _error("developer_mode_required", "developer mode is disabled")
    group_id = str(args.get("group_id") or "").strip()
    by = str(args.get("by") or "user").strip()
    if not group_id:
        return _error("missing_group_id", "missing group_id")
    group, auth_err = _authorize_debug_group_access(group_id=group_id, by=by)
    if auth_err is not None:
        return auth_err
    if group is None:
        return _error("group_not_found", f"group not found: {group_id}")

    log_lines = int(args.get("log_lines") or 400)
    if log_lines <= 0:
        log_lines = 400
    if log_lines > 4000:
        log_lines = 4000
    terminal_max_chars = int(args.get("terminal_max_chars") or 12000)
    if terminal_max_chars <= 0:
        terminal_max_chars = 12000
    if terminal_max_chars > 200_000:
        terminal_max_chars = 200_000

    try:
        home = ensure_home()
        snapshot = _build_debug_snapshot_result(
            home=home,
            group=group,
            get_observability=get_observability,
            effective_runner_kind=effective_runner_kind,
            throttle_debug_summary=throttle_debug_summary,
        )

        daemon_log_path, _ = _debug_component_path(component="daemon", group_id=group_id)
        web_log_path, _ = _debug_component_path(component="web", group_id=group_id)
        im_log_path, _ = _debug_component_path(component="im", group_id=group_id)

        zip_buffer = io.BytesIO()
        with zipfile.ZipFile(zip_buffer, "w", compression=zipfile.ZIP_DEFLATED) as zf:
            manifest = {
                "kind": "feedback_bundle",
                "generated_at": utc_now_iso(),
                "group_id": group_id,
                "group_title": str(group.doc.get("title") or ""),
                "by": by,
                "log_lines": log_lines,
                "terminal_max_chars": terminal_max_chars,
                "redaction": "best_effort",
            }
            zf.writestr("manifest.json", json.dumps(manifest, ensure_ascii=False, indent=2))
            zf.writestr("snapshot/debug_snapshot.json", json.dumps(snapshot, ensure_ascii=False, indent=2))

            daemon_log = _collect_log_tail(daemon_log_path, lines=log_lines)
            if daemon_log:
                zf.writestr("logs/daemon.log", daemon_log)
            web_log = _collect_log_tail(web_log_path, lines=log_lines)
            if web_log:
                zf.writestr("logs/web.log", web_log)
            im_log = _collect_log_tail(im_log_path, lines=log_lines)
            if im_log:
                zf.writestr("logs/im.log", im_log)

            terminal_capture_bytes = max(terminal_max_chars * 4, int(pty_backlog_bytes()))
            for actor in list_actors(group):
                if not isinstance(actor, dict):
                    continue
                actor_id = str(actor.get("id") or "").strip()
                if not actor_id or str(actor.get("runner") or "pty").strip() != "pty":
                    continue
                try:
                    running = bool(pty_runner.SUPERVISOR.actor_running(group_id, actor_id))
                except Exception:
                    running = False
                if not running:
                    continue
                transcript = _collect_terminal_snapshot(
                    group_id=group_id,
                    actor_id=actor_id,
                    max_chars=min(terminal_max_chars, terminal_capture_bytes),
                )
                if not transcript:
                    continue
                safe_actor_id = sanitize_filename(actor_id, fallback="actor")
                zf.writestr(f"terminals/{safe_actor_id}.txt", transcript)

        filename = sanitize_filename(
            f"cccc-feedback-bundle-{group_id}-{utc_now_iso().replace(':', '').replace('-', '')}.zip",
            fallback=f"cccc-feedback-bundle-{group_id}.zip",
        )
        attachment = store_blob_bytes(
            group,
            data=zip_buffer.getvalue(),
            filename=filename,
            mime_type="application/zip",
            kind="file",
        )
        return DaemonResponse(
            ok=True,
            result={
                "group_id": group_id,
                "attachment": attachment,
                "bundle": {
                    "filename": filename,
                    "redaction": "best_effort",
                },
            },
        )
    except Exception as e:
        return _error("feedback_bundle_export_failed", str(e))


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
    if op == "feedback_bundle_export":
        return handle_feedback_bundle_export(
            args,
            developer_mode_enabled=developer_mode_enabled,
            get_observability=get_observability,
            effective_runner_kind=effective_runner_kind,
            throttle_debug_summary=throttle_debug_summary,
            pty_backlog_bytes=pty_backlog_bytes,
        )
    if op == "debug_tail_logs":
        return handle_debug_tail_logs(args, developer_mode_enabled=developer_mode_enabled)
    if op == "debug_clear_logs":
        return handle_debug_clear_logs(args, developer_mode_enabled=developer_mode_enabled)
    return None
