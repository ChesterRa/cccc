from __future__ import annotations

import copy
import logging
import os
import socket
import signal
import threading
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Dict, Optional, Tuple

logger = logging.getLogger("cccc.daemon.server")

from .. import __version__
from ..contracts.v1 import DaemonError, DaemonRequest, DaemonResponse
from ..kernel.group import load_group
from ..kernel.actors import find_actor, find_foreman, update_actor, get_effective_role
from ..kernel.blobs import resolve_blob_attachment_path
from ..kernel.ledger_retention import compact as compact_ledger
from ..kernel.settings import get_observability_settings, update_observability_settings
from ..kernel.terminal_transcript import get_terminal_transcript_settings
from ..kernel.messaging import disabled_recipient_actor_ids
from ..paths import ensure_home
from ..runners import pty as pty_runner
from ..runners import headless as headless_runner
from ..util.conv import coerce_bool
from ..util.obslog import setup_root_json_logging
from ..util.fs import atomic_write_json, atomic_write_text, read_json
from ..util.file_lock import acquire_lockfile, release_lockfile, LockUnavailableError
from ..util.time import utc_now_iso
from .automation import AutomationManager
from .bootstrap_im_ops import autostart_enabled_im_bridges
from .bootstrap_actor_ops import autostart_running_groups
from .mcp_install import (
    is_mcp_installed as runtime_is_mcp_installed,
    ensure_mcp_installed as runtime_ensure_mcp_installed,
)
from .client_ops import send_daemon_request
from .im_bridge_ops import (
    stop_im_bridges_for_group as im_stop_group,
    stop_all_im_bridges as im_stop_all,
    cleanup_invalid_im_bridges as im_cleanup_invalid,
)
from .private_env_ops import (
    PRIVATE_ENV_MAX_KEYS as _PRIVATE_ENV_MAX_KEYS,
    validate_private_env_key as _validate_private_env_key,
    coerce_private_env_value as _coerce_private_env_value,
    load_actor_private_env as _load_actor_private_env,
    update_actor_private_env as _update_actor_private_env,
    delete_actor_private_env as _delete_actor_private_env,
    delete_group_private_env as _delete_group_private_env,
    merge_actor_env_with_private as _merge_actor_env_with_private,
)
from .runner_state_ops import (
    pty_state_path as _pty_state_path,
    write_pty_state as _write_pty_state,
    remove_pty_state_if_pid as _remove_pty_state_if_pid,
    headless_state_path as _headless_state_path,
    write_headless_state as _write_headless_state,
    remove_headless_state as _remove_headless_state,
    cleanup_stale_pty_state as _cleanup_stale_pty_state,
)
from .socket_protocol_ops import (
    recv_json_line as _recv_json_line,
    send_json as _send_json,
    dump_response as _dump_response,
    supported_stream_kinds as _supported_stream_kinds,
    start_events_stream as _start_events_stream,
    error as _error,
)
from .delivery import (
    inject_system_prompt as deliver_system_prompt,
    pty_submit_text,
    render_delivery_text,
    deliver_message_with_preamble,
    queue_system_notify,
    flush_pending_messages,
    tick_delivery,
    clear_preamble_sent,
    THROTTLE,
)
from .ops.chat_support_ops import auto_wake_recipients, normalize_attachments
from .ops.socket_special_ops import try_handle_socket_special_op
from .ops.socket_accept_ops import handle_incoming_connection
from .ops.actor_runtime_ops import start_actor_process as runtime_start_actor_process
from .ops.runner_ops import stop_actor as runner_stop_actor
from .request_dispatch_ops import RequestDispatchDeps, dispatch_request
from .serve_ops import (
    start_automation_thread,
    bind_server_socket,
    write_daemon_addr,
    start_bootstrap_thread,
    cleanup_after_stop,
)
from .ops.template_ops import (
    group_create_from_template,
    group_template_export,
    group_template_import_replace,
    group_template_preview,
)

_OBS_LOCK = threading.Lock()
_OBSERVABILITY: Dict[str, Any] = {}
_AUTO_WAKE_LOCK = threading.Lock()
_AUTO_WAKE_IN_PROGRESS: set[tuple[str, str]] = set()
_REQUEST_DISPATCH_DEPS: Optional[RequestDispatchDeps] = None


def _get_observability() -> Dict[str, Any]:
    with _OBS_LOCK:
        return copy.deepcopy(_OBSERVABILITY) if _OBSERVABILITY else get_observability_settings()


def _developer_mode_enabled() -> bool:
    obs = _get_observability()
    return coerce_bool(obs.get("developer_mode"), default=False)


def _apply_observability_settings(home: Path, obs: Dict[str, Any]) -> None:
    """Apply observability settings in-process (best-effort)."""
    if not isinstance(obs, dict):
        return
    with _OBS_LOCK:
        _OBSERVABILITY.clear()
        _OBSERVABILITY.update(copy.deepcopy(obs))

    # Logging: keep simple; configure root JSONL logger to stderr.
    level = str(obs.get("log_level") or "INFO").strip().upper() or "INFO"
    if coerce_bool(obs.get("developer_mode"), default=False):
        # Developer mode typically wants more detail.
        if level == "INFO":
            level = "DEBUG"
    setup_root_json_logging(component="daemon", level=level, force=True)


def _pty_backlog_bytes() -> int:
    """Best-effort per-actor PTY backlog size (ring buffer)."""
    obs = _get_observability()
    tt = obs.get("terminal_transcript") if isinstance(obs, dict) else None
    n = 10 * 1024 * 1024
    if isinstance(tt, dict):
        try:
            n = int(tt.get("per_actor_bytes") or 0)
        except Exception:
            n = 0
    if n <= 0:
        n = 10 * 1024 * 1024
    if n > 50_000_000:
        n = 50_000_000
    return int(n)


def _pty_supported() -> bool:
    return bool(getattr(pty_runner, "PTY_SUPPORTED", True))


def _effective_runner_kind(runner_kind: str) -> str:
    """Return the effective runner kind for this platform.

    Windows (and some Python builds) cannot run PTY; treat PTY as headless.
    """
    rk = str(runner_kind or "").strip() or "pty"
    if rk == "headless":
        return "headless"
    return "pty" if _pty_supported() else "headless"


def _can_read_terminal_transcript(group: Any, *, by: str, target_actor_id: str) -> bool:
    who = str(by or "").strip()
    target = str(target_actor_id or "").strip()
    if not target:
        return False
    if not who or who == "user":
        return True
    if who == target:
        return True
    if find_actor(group, who) is None:
        return False
    tt = get_terminal_transcript_settings(group.doc)
    vis = str(tt.get("visibility") or "foreman")
    if vis == "all":
        return True
    if vis == "foreman" and get_effective_role(group, who) == "foreman":
        return True
    return False

SUPPORTED_RUNTIMES = (
    "amp",
    "auggie",
    "claude",
    "codex",
    "cursor",
    "droid",
    "gemini",
    "kilocode",
    "neovate",
    "opencode",
    "copilot",
    "custom",
)

AUTO_MCP_RUNTIMES = ("claude", "codex", "droid", "amp", "auggie", "neovate", "gemini")


def _normalize_runtime_command(runtime: str, command: list[str]) -> list[str]:
    """Return a runtime-safe command line used for process start.

    Important: This MUST NOT mutate the stored actor.command (ledger). It's runtime-only.

    Rationale:
    Some runtimes spawn MCP servers/tools as subprocesses and may not inherit the full actor env by
    default. CCCC injects critical context into actor env (CCCC_GROUP_ID/CCCC_ACTOR_ID); if a runtime
    drops these, MCP tools cannot resolve "self" and agent matches can stall.
    """
    rt = str(runtime or "").strip()
    cmd = [str(x) for x in (command or []) if str(x).strip()]
    if not cmd:
        return []

    if rt == "codex":
        try:
            exe = Path(str(cmd[0] or "")).name
        except Exception:
            exe = str(cmd[0] or "")
        if exe == "codex":
            # Ensure MCP servers inherit actor env (CCCC_* / ARENA_*).
            has_env_inherit = any("shell_environment_policy.inherit" in str(x) for x in cmd)
            if not has_env_inherit:
                cmd = [cmd[0], "-c", "shell_environment_policy.inherit=all", *cmd[1:]]

    return cmd


def _is_mcp_installed(runtime: str) -> bool:
    return runtime_is_mcp_installed(runtime)


def _ensure_mcp_installed(runtime: str, cwd: Path) -> bool:
    return runtime_ensure_mcp_installed(runtime, cwd, auto_mcp_runtimes=AUTO_MCP_RUNTIMES)


def _prepare_pty_env(env: Dict[str, Any]) -> Dict[str, str]:
    """Prepare environment variables for PTY session.
    
    Key modifications:
    - Disable bracketed paste mode via INPUTRC to prevent readline from
      interfering with programmatic text input
    - This is critical for reliable message delivery to CLI applications
    
    Returns a new dict with string values only.
    """
    result = {str(k): str(v) for k, v in env.items() if isinstance(k, str)}
    
    # Create a temporary inputrc file to disable bracketed paste
    # This is more reliable than sending escape sequences
    home = ensure_home()
    inputrc_path = home / "daemon" / "inputrc"
    try:
        inputrc_path.parent.mkdir(parents=True, exist_ok=True)
        inputrc_content = "set enable-bracketed-paste off\n"
        if not inputrc_path.exists() or inputrc_path.read_text() != inputrc_content:
            inputrc_path.write_text(inputrc_content)
        result["INPUTRC"] = str(inputrc_path)
    except Exception:
        pass
    
    return result


def _inject_actor_context_env(env: Dict[str, Any], *, group_id: str, actor_id: str) -> Dict[str, Any]:
    """Inject per-actor context for MCP servers/tools into the actor process env.

    This is runtime-only (not persisted to group docs).
    """
    out: Dict[str, Any] = dict(env or {})
    out["CCCC_GROUP_ID"] = str(group_id or "").strip()
    out["CCCC_ACTOR_ID"] = str(actor_id or "").strip()
    return out


AUTOMATION = AutomationManager()

_AUTOMATION_RESET_NOTIFY_KINDS = {"nudge", "keepalive", "help_nudge", "actor_idle", "silence_check", "automation"}


def _foreman_id(group: Any) -> str:
    try:
        foreman = find_foreman(group)
    except Exception:
        foreman = None
    if isinstance(foreman, dict):
        return str(foreman.get("id") or "").strip()
    return ""


def _reset_automation_timers_if_active(group: Any) -> None:
    """Reset automation timers without catch-up bursts.

    Used on resume/start and when foreman changes (ownership transfer).
    """
    try:
        from ..kernel.group import get_group_state

        if get_group_state(group) != "active":
            return
        AUTOMATION.on_resume(group)
        try:
            THROTTLE.clear_pending_system_notifies(group.group_id, notify_kinds=set(_AUTOMATION_RESET_NOTIFY_KINDS))
        except Exception:
            pass
    except Exception:
        pass


def _maybe_reset_automation_on_foreman_change(group: Any, *, before_foreman_id: str) -> None:
    after = _foreman_id(group)
    if str(before_foreman_id or "") == str(after or ""):
        return
    _reset_automation_timers_if_active(group)


@dataclass
class DaemonPaths:
    home: Path

    @property
    def daemon_dir(self) -> Path:
        return self.home / "daemon"

    @property
    def sock_path(self) -> Path:
        return self.daemon_dir / "ccccd.sock"

    @property
    def addr_path(self) -> Path:
        # Cross-platform daemon endpoint descriptor (TCP fallback on Windows).
        return self.daemon_dir / "ccccd.addr.json"

    @property
    def pid_path(self) -> Path:
        return self.daemon_dir / "ccccd.pid"

    @property
    def log_path(self) -> Path:
        return self.daemon_dir / "ccccd.log"


def default_paths() -> DaemonPaths:
    return DaemonPaths(home=ensure_home())


def _desired_daemon_transport() -> str:
    override = str(os.environ.get("CCCC_DAEMON_TRANSPORT") or "").strip().lower()
    if override in ("unix", "tcp"):
        return override
    # Default: AF_UNIX on POSIX, TCP on Windows.
    return "tcp" if os.name == "nt" else "unix"


def _allow_remote_daemon() -> bool:
    """Whether it's OK to bind the daemon to a non-loopback TCP host.

    Warning: the daemon IPC has no authentication.
    """
    v = str(os.environ.get("CCCC_DAEMON_ALLOW_REMOTE") or "").strip().lower()
    return v in ("1", "true", "yes", "y", "on")


def _daemon_tcp_connect_host(bind_host: str) -> str:
    """Return a TCP host that local clients can connect to."""
    h = str(bind_host or "").strip()
    if not h or h == "localhost":
        return "127.0.0.1"
    if h == "0.0.0.0":
        return "127.0.0.1"
    # This daemon currently uses AF_INET only; avoid writing an IPv6 host into addr.json.
    if ":" in h:
        return "127.0.0.1"
    return h


def _daemon_tcp_bind_host() -> str:
    host = str(os.environ.get("CCCC_DAEMON_HOST") or "").strip()
    if not host or host == "localhost":
        return "127.0.0.1"
    if ":" in host:
        logger.warning("CCCC_DAEMON_HOST=%s looks like IPv6; only IPv4 is supported. Using 127.0.0.1.", host)
        return "127.0.0.1"
    if host == "127.0.0.1":
        return host
    if not _allow_remote_daemon():
        logger.warning(
            "Refusing to bind daemon to non-loopback host %s (no auth). Using 127.0.0.1. "
            "Set CCCC_DAEMON_ALLOW_REMOTE=1 to override.",
            host,
        )
        return "127.0.0.1"
    return host


def _daemon_tcp_port() -> int:
    raw = str(os.environ.get("CCCC_DAEMON_PORT") or "").strip()
    if not raw:
        return 0
    try:
        port = int(raw)
    except Exception:
        return 0
    if port < 0 or port > 65535:
        return 0
    return port


def get_daemon_endpoint(paths: Optional[DaemonPaths] = None) -> Dict[str, Any]:
    """Best-effort: load the daemon endpoint descriptor (cross-platform)."""
    p = paths or default_paths()
    doc = read_json(p.addr_path)
    if isinstance(doc, dict):
        transport = str(doc.get("transport") or "").strip().lower()
        if transport == "tcp":
            try:
                host = str(doc.get("host") or "").strip() or "127.0.0.1"
                port = int(doc.get("port") or 0)
            except Exception:
                host = "127.0.0.1"
                port = 0
            if port > 0:
                return {"transport": "tcp", "host": _daemon_tcp_connect_host(host), "port": port}
        if transport == "unix":
            path = str(doc.get("path") or "").strip()
            if path:
                return {"transport": "unix", "path": path}

    # Back-compat: if no descriptor exists, fall back to AF_UNIX when available.
    if getattr(socket, "AF_UNIX", None) is not None:
        return {"transport": "unix", "path": str(p.sock_path)}
    return {}


def _is_daemon_alive(paths: DaemonPaths) -> bool:
    try:
        return bool(call_daemon({"op": "ping"}, paths=paths, timeout_s=0.2).get("ok"))
    except Exception:
        return False


def _cleanup_stale_daemon_endpoints(paths: DaemonPaths) -> None:
    if _is_daemon_alive(paths):
        return
    for stale in (paths.addr_path, paths.sock_path):
        try:
            stale.unlink(missing_ok=True)
        except Exception:
            pass


def _write_pid(pid_path: Path) -> None:
    pid_path.parent.mkdir(parents=True, exist_ok=True)
    atomic_write_text(pid_path, str(os.getpid()) + "\n")


def _pid_alive(pid: int) -> bool:
    if pid <= 0:
        return False
    try:
        os.kill(pid, 0)
        return True
    except Exception:
        return False


def _best_effort_killpg(pid: int, sig: signal.Signals) -> None:
    if pid <= 0:
        return
    try:
        os.killpg(pid, sig)
    except Exception:
        try:
            os.kill(pid, sig)
        except Exception:
            pass


def _maybe_autostart_enabled_im_bridges() -> None:
    """Autostart IM bridges that are marked enabled in group settings."""
    autostart_enabled_im_bridges(ensure_home())


def _maybe_autostart_running_groups() -> None:
    from ..kernel.group import get_group_state

    autostart_running_groups(
        ensure_home(),
        effective_runner_kind=_effective_runner_kind,
        find_scope_url=_find_scope_url,
        supported_runtimes=SUPPORTED_RUNTIMES,
        ensure_mcp_installed=_ensure_mcp_installed,
        auto_mcp_runtimes=AUTO_MCP_RUNTIMES,
        pty_supported=_pty_supported,
        merge_actor_env_with_private=_merge_actor_env_with_private,
        inject_actor_context_env=lambda env, gid, aid: _inject_actor_context_env(env, group_id=gid, actor_id=aid),
        prepare_pty_env=_prepare_pty_env,
        normalize_runtime_command=_normalize_runtime_command,
        pty_backlog_bytes=_pty_backlog_bytes,
        write_headless_state=_write_headless_state,
        write_pty_state=lambda gid, aid, pid: _write_pty_state(gid, aid, pid=pid),
        clear_preamble_sent=clear_preamble_sent,
        throttle_reset_actor=lambda gid, aid: THROTTLE.reset_actor(gid, aid, keep_pending=True),
        automation_on_resume=AUTOMATION.on_resume,
        get_group_state=get_group_state,
    )


def _maybe_compact_ledgers(home: Path) -> None:
    base = home / "groups"
    if not base.exists():
        return
    for p in base.glob("*/group.yaml"):
        gid = p.parent.name
        group = load_group(gid)
        if group is None:
            continue
        if not coerce_bool(group.doc.get("running"), default=False):
            continue
        try:
            _ = compact_ledger(group, reason="auto", force=False)
        except Exception:
            continue


def _inject_system_prompt(group: Any, actor: Dict[str, Any]) -> None:
    try:
        deliver_system_prompt(group, actor=actor)
    except Exception:
        pass


def _remove_stale_socket(sock_path: Path) -> None:
    # Deprecated: daemon IPC is now cross-platform and uses `ccccd.addr.json`.
    # Keep this as a best-effort cleanup helper for older call sites.
    try:
        sock_path.unlink(missing_ok=True)
    except Exception:
        pass


def _find_scope_url(group: Any, scope_key: str) -> str:
    wanted = str(scope_key or "").strip()
    if not wanted:
        return ""
    scopes = group.doc.get("scopes")
    if not isinstance(scopes, list):
        return ""
    for sc in scopes:
        if not isinstance(sc, dict):
            continue
        if str(sc.get("scope_key") or "").strip() != wanted:
            continue
        return str(sc.get("url") or "").strip()
    return ""


def _start_actor_process(
    group: Any,
    actor_id: str,
    *,
    command: List[str],
    env: Dict[str, str],
    runner: str,
    runtime: str,
    by: str,
) -> Dict[str, Any]:
    return runtime_start_actor_process(
        group,
        actor_id,
        command=command,
        env=env,
        runner=runner,
        runtime=runtime,
        by=by,
        find_scope_url=_find_scope_url,
        effective_runner_kind=_effective_runner_kind,
        merge_actor_env_with_private=_merge_actor_env_with_private,
        normalize_runtime_command=_normalize_runtime_command,
        ensure_mcp_installed=_ensure_mcp_installed,
        inject_actor_context_env=lambda e, gid, aid: _inject_actor_context_env(e, group_id=gid, actor_id=aid),
        prepare_pty_env=_prepare_pty_env,
        pty_backlog_bytes=_pty_backlog_bytes,
        write_headless_state=_write_headless_state,
        write_pty_state=lambda gid, aid, pid: _write_pty_state(gid, aid, pid=pid),
        clear_preamble_sent=clear_preamble_sent,
        throttle_reset_actor=lambda gid, aid: THROTTLE.reset_actor(gid, aid, keep_pending=True),
        supported_runtimes=SUPPORTED_RUNTIMES,
    )


def _request_dispatch_deps() -> RequestDispatchDeps:
    global _REQUEST_DISPATCH_DEPS
    if _REQUEST_DISPATCH_DEPS is not None:
        return _REQUEST_DISPATCH_DEPS
    _REQUEST_DISPATCH_DEPS = RequestDispatchDeps(
        version=__version__,
        pid_provider=os.getpid,
        now_iso=utc_now_iso,
        get_observability=_get_observability,
        update_observability_settings=update_observability_settings,
        apply_observability_settings=lambda obs: _apply_observability_settings(ensure_home(), obs),
        developer_mode_enabled=_developer_mode_enabled,
        effective_runner_kind=_effective_runner_kind,
        throttle_debug_summary=THROTTLE.debug_summary,
        can_read_terminal_transcript=lambda group, by, actor_id: _can_read_terminal_transcript(
            group,
            by=by,
            target_actor_id=actor_id,
        ),
        pty_backlog_bytes=_pty_backlog_bytes,
        group_create_from_template=group_create_from_template,
        group_template_export=group_template_export,
        group_template_preview=group_template_preview,
        group_template_import_replace=group_template_import_replace,
        foreman_id=_foreman_id,
        maybe_reset_automation_on_foreman_change=lambda group, before_foreman_id: _maybe_reset_automation_on_foreman_change(
            group,
            before_foreman_id=before_foreman_id,
        ),
        stop_im_bridges_for_group=lambda gid: im_stop_group(
            ensure_home(),
            group_id=gid,
            best_effort_killpg=_best_effort_killpg,
        ),
        delete_group_private_env=_delete_group_private_env,
        pty_supported=_pty_supported,
        find_scope_url=_find_scope_url,
        ensure_mcp_installed=_ensure_mcp_installed,
        merge_actor_env_with_private=_merge_actor_env_with_private,
        inject_actor_context_env=_inject_actor_context_env,
        normalize_runtime_command=_normalize_runtime_command,
        prepare_pty_env=_prepare_pty_env,
        write_headless_state=_write_headless_state,
        write_pty_state=_write_pty_state,
        clear_preamble_sent=clear_preamble_sent,
        throttle_reset_actor=THROTTLE.reset_actor,
        reset_automation_timers_if_active=_reset_automation_timers_if_active,
        supported_runtimes=SUPPORTED_RUNTIMES,
        pty_state_dir_for_group=lambda group_id: _pty_state_path(group_id, "_").parent,
        headless_state_dir_for_group=lambda group_id: _headless_state_path(group_id, "_").parent,
        automation_on_resume=AUTOMATION.on_resume,
        clear_pending_system_notifies=lambda group_id: THROTTLE.clear_pending_system_notifies(
            group_id,
            notify_kinds={"nudge", "keepalive", "help_nudge", "actor_idle", "silence_check", "automation"},
        ),
        load_actor_private_env=_load_actor_private_env,
        validate_private_env_key=_validate_private_env_key,
        coerce_private_env_value=_coerce_private_env_value,
        update_actor_private_env=_update_actor_private_env,
        private_env_max_keys=_PRIVATE_ENV_MAX_KEYS,
        start_actor_process=_start_actor_process,
        delete_actor_private_env=_delete_actor_private_env,
        warn_forced_headless=lambda group_id, actor_id: logger.warning(
            "pty runner is not supported on this platform; forcing runner=headless for %s/%s",
            group_id,
            actor_id,
        ),
        remove_headless_state=_remove_headless_state,
        remove_pty_state_if_pid=_remove_pty_state_if_pid,
        throttle_clear_actor=THROTTLE.clear_actor,
        daemon_request_factory=DaemonRequest,
        coerce_bool_default_false=lambda value: coerce_bool(value, default=False),
        normalize_attachments=lambda group, raw: normalize_attachments(
            group,
            raw,
            resolve_blob_attachment_path=resolve_blob_attachment_path,
        ),
        auto_wake_recipients=lambda group, to, by: auto_wake_recipients(
            group,
            to,
            by=by,
            disabled_recipient_actor_ids=disabled_recipient_actor_ids,
            find_actor=find_actor,
            coerce_bool=coerce_bool,
            start_actor_process=_start_actor_process,
            update_actor=update_actor,
            runner_stop_actor=runner_stop_actor,
            logger=logger,
            auto_wake_lock=_AUTO_WAKE_LOCK,
            auto_wake_in_progress=_AUTO_WAKE_IN_PROGRESS,
        ),
        automation_on_new_message=AUTOMATION.on_new_message,
        clear_pending_system_notifies_chat=lambda group_id, notify_kinds: THROTTLE.clear_pending_system_notifies(
            group_id,
            notify_kinds=notify_kinds,
        ),
        queue_system_notify=queue_system_notify,
        error_factory=_error,
    )
    return _REQUEST_DISPATCH_DEPS


def handle_request(req: DaemonRequest) -> Tuple[DaemonResponse, bool]:
    return dispatch_request(req, deps=_request_dispatch_deps(), recurse=handle_request)


def serve_forever(paths: Optional[DaemonPaths] = None) -> int:
    p = paths or default_paths()
    p.daemon_dir.mkdir(parents=True, exist_ok=True)

    # Acquire exclusive lock to prevent multiple daemon instances (race condition fix).
    # The lock is held for the lifetime of the daemon process.
    lock_path = p.daemon_dir / "ccccd.lock"
    try:
        lock_handle = acquire_lockfile(lock_path, blocking=False)
    except LockUnavailableError:
        # Another daemon already holds the lock
        return 0

    # Apply global observability settings early (logging + developer mode gating).
    try:
        _apply_observability_settings(p.home, get_observability_settings())
    except Exception:
        pass

    _cleanup_stale_daemon_endpoints(p)
    if _is_daemon_alive(p):
        release_lockfile(lock_handle)
        return 0

    # Cleanup stale IM bridge state from previous runs/crashes.
    try:
        res = im_cleanup_invalid(
            p.home,
            pid_alive=_pid_alive,
            best_effort_killpg=_best_effort_killpg,
        )
        if res.get("killed") or res.get("stale_pidfiles"):
            logger.info(
                "im_bridge_cleanup killed=%s stale_pidfiles=%s",
                res.get("killed"),
                res.get("stale_pidfiles"),
                extra={"op": "im_bridge_cleanup"},
            )
    except Exception:
        pass

    # Best-effort cleanup of orphaned PTY actor processes from a previous daemon crash.
    try:
        _cleanup_stale_pty_state(
            p.home,
            pid_alive=_pid_alive,
            best_effort_killpg=_best_effort_killpg,
        )
    except Exception:
        pass

    def _on_session_exit(session: pty_runner.PtySession) -> None:
        _remove_pty_state_if_pid(session.group_id, session.actor_id, pid=session.pid)

    try:
        pty_runner.SUPERVISOR.set_exit_hook(_on_session_exit)
    except Exception:
        pass

    try:
        p.sock_path.unlink(missing_ok=True)
    except Exception:
        pass
    try:
        p.addr_path.unlink(missing_ok=True)
    except Exception:
        pass

    stop_event = threading.Event()

    # Best-effort: enable in-process event streaming for SDKs (daemon-owned only).
    try:
        from ..kernel.ledger import set_append_hook
        from .streaming import EVENT_BROADCASTER

        set_append_hook(EVENT_BROADCASTER.on_append)
    except Exception:
        pass

    # Graceful shutdown on SIGTERM/SIGINT
    def _signal_handler(signum: int, frame: Any) -> None:
        stop_event.set()

    signal.signal(signal.SIGTERM, _signal_handler)
    signal.signal(signal.SIGINT, _signal_handler)

    start_automation_thread(
        stop_event=stop_event,
        home=p.home,
        automation_tick=AUTOMATION.tick,
        load_group=load_group,
        group_running=lambda gid: (
            pty_runner.SUPERVISOR.group_running(gid)
            or headless_runner.SUPERVISOR.group_running(gid)
        ),
        tick_delivery=tick_delivery,
        compact_ledgers=_maybe_compact_ledgers,
    )

    transport = _desired_daemon_transport()
    if transport == "unix" and getattr(socket, "AF_UNIX", None) is None:
        transport = "tcp"
    s, endpoint = bind_server_socket(
        transport=transport,
        sock_path=p.sock_path,
        daemon_tcp_bind_host=_daemon_tcp_bind_host,
        daemon_tcp_port=_daemon_tcp_port,
    )

    with s:
        s.listen(50)
        s.settimeout(1.0)  # Allow periodic check of stop_event
        _write_pid(p.pid_path)
        write_daemon_addr(
            atomic_write_json=atomic_write_json,
            addr_path=p.addr_path,
            endpoint=endpoint,
            pid=os.getpid(),
            version=__version__,
            now_iso=utc_now_iso(),
        )

        # Bootstrap background work only after the daemon socket is ready, but
        # don't block the accept loop (clients should see the daemon as responsive).
        start_bootstrap_thread(
            maybe_autostart_running_groups=_maybe_autostart_running_groups,
            maybe_autostart_enabled_im_bridges=_maybe_autostart_enabled_im_bridges,
        )

        should_exit = False
        while not should_exit and not stop_event.is_set():
            try:
                conn, _ = s.accept()
            except socket.timeout:
                continue
            except KeyboardInterrupt:
                break
            except Exception:
                continue
            should_exit = handle_incoming_connection(
                conn,
                recv_json_line=_recv_json_line,
                parse_request=DaemonRequest.model_validate,
                make_invalid_request_error=lambda err: _error(
                    "invalid_request",
                    "invalid request",
                    details={"error": err},
                ),
                send_json=_send_json,
                dump_response=_dump_response,
                try_handle_special=lambda req, sock: try_handle_socket_special_op(
                    req,
                    sock,
                    send_json=_send_json,
                    dump_response=_dump_response,
                    error=lambda code, message, details=None: _error(code, message, details=details),
                    actor_running=pty_runner.SUPERVISOR.actor_running,
                    attach_actor_socket=lambda group_id, actor_id, sock2: pty_runner.SUPERVISOR.attach(
                        group_id=group_id,
                        actor_id=actor_id,
                        sock=sock2,
                    ),
                    load_group=load_group,
                    find_actor=find_actor,
                    supported_stream_kinds=_supported_stream_kinds,
                    start_events_stream=lambda sock2, group_id, by, kinds, since_event_id, since_ts: _start_events_stream(
                        sock=sock2,
                        group_id=group_id,
                        by=by,
                        kinds=kinds,
                        since_event_id=since_event_id,
                        since_ts=since_ts,
                    ),
                ),
                handle_request=handle_request,
                logger=logger,
            )
            if should_exit:
                stop_event.set()

    cleanup_after_stop(
        stop_event=stop_event,
        home=p.home,
        best_effort_killpg=_best_effort_killpg,
        im_stop_all=im_stop_all,
        pty_stop_all=pty_runner.SUPERVISOR.stop_all,
        headless_stop_all=headless_runner.SUPERVISOR.stop_all,
        sock_path=p.sock_path,
        addr_path=p.addr_path,
        pid_path=p.pid_path,
        release_lockfile=release_lockfile,
        lock_handle=lock_handle,
    )

    return 0


def call_daemon(req: Dict[str, Any], *, paths: Optional[DaemonPaths] = None, timeout_s: float = 60.0) -> Dict[str, Any]:
    p = paths or default_paths()
    try:
        request = DaemonRequest.model_validate(req)
    except Exception as e:
        return DaemonResponse(
            ok=False,
            error=DaemonError(code="invalid_request", message="invalid request", details={"error": str(e)}),
        ).model_dump()
    try:
        ep = get_daemon_endpoint(p)
        obj = send_daemon_request(
            ep,
            request.model_dump(),
            timeout_s=timeout_s,
            sock_path_default=p.sock_path,
        )
        resp = DaemonResponse.model_validate(obj)
        return resp.model_dump()
    except Exception:
        return DaemonResponse(ok=False, error=DaemonError(code="daemon_unavailable", message="daemon unavailable")).model_dump()


def read_pid(paths: Optional[DaemonPaths] = None) -> int:
    p = paths or default_paths()
    try:
        txt = p.pid_path.read_text(encoding="utf-8").strip()
        return int(txt) if txt.isdigit() else 0
    except Exception:
        return 0
