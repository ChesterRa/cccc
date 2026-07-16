from __future__ import annotations

import argparse
import json
import os
import shutil
import signal
import socket
import subprocess
import sys
import time
import shlex
from pathlib import Path
from typing import Any, Optional

from .. import __version__
from ..contracts.v1 import ChatMessageData
from ..daemon.server import call_daemon
from ..kernel.active import load_active, set_active_group_id
from ..kernel.actors import add_actor, list_actors, remove_actor, resolve_recipient_tokens, update_actor
from ..kernel.group import (
    attach_scope_to_group,
    create_group,
    delete_group,
    detach_scope_from_group,
    ensure_group_for_scope,
    load_group,
    set_active_scope,
    update_group,
)
from ..kernel.inbox import find_event, get_cursor, get_quote_text, set_cursor, unread_messages
from ..kernel.ledger import append_event, follow, read_last_lines
from ..kernel.ledger_retention import compact as compact_ledger
from ..kernel.ledger_retention import snapshot as snapshot_ledger
from ..kernel.messaging import default_reply_recipients
from ..kernel.permissions import require_actor_permission, require_group_permission, require_inbox_permission
from ..kernel.registry import load_registry
from ..kernel.settings import resolve_remote_access_web_binding
from ..kernel.scope import detect_scope
from ..kernel.system_prompt import render_system_prompt
from ..paths import ensure_home
from ..ports.im.config_schema import canonicalize_im_config
from ..ports.web.runtime_control import (
    WEB_RUNTIME_RESTART_EXIT_CODE,
    clear_web_runtime_state,
    read_web_runtime_state,
    restart_supervised_web_child_with_fallback,
    start_supervised_web_child,
    stop_web_child,
    wait_for_child_exit_interruptibly,
    web_runtime_pid_candidates,
)
from ..util.conv import coerce_bool
from ..util.file_lock import LockUnavailableError, acquire_lockfile, release_lockfile
from ..util.process import (
    resolve_background_python_argv,
    SOFT_TERMINATE_SIGNAL,
    best_effort_signal_pid,
    pid_is_alive,
    resolve_subprocess_argv,
    supervised_process_popen_kwargs,
    terminate_pid,
)

_SPACE_QUERY_OPTION_KEYS = {"source_ids"}


def _display_local_host(host: str) -> str:
    h = str(host or "").strip()
    if h in {"0.0.0.0", "::", "[::]"}:
        return "localhost"
    return h or "localhost"


def _http_host_literal(host: str) -> str:
    h = _display_local_host(host)
    # Keep localhost as-is; bracket raw IPv6 literals for URL correctness.
    if h != "localhost" and ":" in h and not (h.startswith("[") and h.endswith("]")):
        return f"[{h}]"
    return h


def _resolve_web_server_binding() -> tuple[str, int]:
    binding = resolve_remote_access_web_binding()
    host = str(binding.get("web_host") or "").strip() or "127.0.0.1"
    port = int(binding.get("web_port") or 8848)
    return host, port


def _resolve_invocation_web_server_binding(
    *,
    web_host_override: str = "",
    web_port_override: Optional[int] = None,
) -> tuple[str, int]:
    host, port = _resolve_web_server_binding()
    override_host = str(web_host_override or "").strip()
    if override_host:
        host = override_host
    if web_port_override is not None:
        port = int(web_port_override)
    return host, int(port)


def _default_entry_lock_path(home: Path) -> Path:
    return home / "daemon" / "cccc-app.lock"


def _acquire_default_entry_lock(home: Path) -> tuple[Optional[Any], Optional[str]]:
    try:
        lock_handle = acquire_lockfile(_default_entry_lock_path(home), blocking=False)
    except LockUnavailableError:
        return None, "CCCC is already running for this CCCC_HOME. Stop the existing `cccc` session with Ctrl+C, then start it again."
    except Exception as e:
        return None, f"Failed to acquire CCCC app lock: {e}"
    return lock_handle, None


def _cleanup_daemon_state_files(home: Path) -> None:
    daemon_dir = home / "daemon"
    for path in (
        daemon_dir / "ccccd.sock",
        daemon_dir / "ccccd.addr.json",
        daemon_dir / "ccccd.pid",
    ):
        path.unlink(missing_ok=True)


def _same_home_daemon_lock_holder_pids(home: Path) -> list[int]:
    if os.name != "posix":
        return []
    if not shutil.which("lsof"):
        return []
    lock_path = home / "daemon" / "ccccd.lock"
    try:
        proc = subprocess.run(
            ["lsof", "-t", str(lock_path)],
            capture_output=True,
            text=True,
            check=False,
        )
    except Exception:
        return []
    if int(proc.returncode or 0) not in {0, 1}:
        return []

    pids: list[int] = []
    for line in str(proc.stdout or "").splitlines():
        text = str(line or "").strip()
        if not text.isdigit():
            continue
        pid = int(text)
        if pid <= 0 or pid == os.getpid():
            continue
        pids.append(pid)
    return sorted(set(pids))


def _same_home_daemon_pids(home: Path) -> list[int]:
    if os.name != "posix":
        return []

    target_home = str(home.resolve())
    default_home = str(ensure_home().resolve())
    proc_root = Path("/proc")
    if not proc_root.is_dir():
        return _same_home_daemon_lock_holder_pids(home)
    pids: list[int] = []
    try:
        proc_dirs = list(proc_root.iterdir())
    except Exception:
        return []
    for proc_dir in proc_dirs:
        name = proc_dir.name
        if not name.isdigit():
            continue
        pid = int(name)
        if pid <= 0 or pid == os.getpid():
            continue
        try:
            cmd_parts = proc_dir.joinpath("cmdline").read_bytes().split(b"\x00")
            cmd = [part.decode("utf-8", errors="ignore") for part in cmd_parts if part]
        except Exception:
            continue
        if "cccc.daemon_main" not in cmd or "run" not in cmd:
            continue

        try:
            env_parts = proc_dir.joinpath("environ").read_bytes().split(b"\x00")
            env_doc = {}
            for part in env_parts:
                if not part or b"=" not in part:
                    continue
                key_b, value_b = part.split(b"=", 1)
                key = key_b.decode("utf-8", errors="ignore")
                value = value_b.decode("utf-8", errors="ignore")
                env_doc[key] = value
        except Exception:
            env_doc = {}

        raw_home = str(env_doc.get("CCCC_HOME") or "").strip()
        proc_home = str(Path(raw_home).resolve()) if raw_home else default_home
        if proc_home != target_home:
            continue
        pids.append(pid)
    return sorted(set(pids))


def _terminate_same_home_daemons(home: Path, *, extra_pids: list[int] | None = None) -> bool:
    candidate_pids = set(int(pid) for pid in (extra_pids or []) if int(pid) > 0)
    for pid in _same_home_daemon_pids(home):
        if pid > 0:
            candidate_pids.add(int(pid))
    for pid in sorted(candidate_pids):
        if not terminate_pid(pid, timeout_s=2.0, include_group=True, force=True):
            return False
    return True


def _stop_existing_web_runtime(home: Path) -> bool:
    runtime = read_web_runtime_state(home)
    candidate_pids = web_runtime_pid_candidates(runtime)
    for runtime_pid in candidate_pids:
        if not pid_is_alive(runtime_pid):
            continue
        if not terminate_pid(runtime_pid, timeout_s=2.0, include_group=True, force=True):
            return False
    clear_pid = candidate_pids[0] if candidate_pids else None
    clear_web_runtime_state(home=home, pid=clear_pid)
    return True


def _stop_existing_daemon(home: Path) -> bool:
    resp = call_daemon({"op": "ping"}, timeout_s=1.0)
    daemon_pid = 0
    extra_pids: list[int] = []
    if resp.get("ok"):
        try:
            result = resp.get("result") if isinstance(resp.get("result"), dict) else {}
            daemon_pid = int(result.get("pid") or 0)
        except Exception:
            daemon_pid = 0
        try:
            call_daemon({"op": "shutdown"}, timeout_s=2.0)
        except Exception:
            pass

        deadline = time.time() + 2.0
        while time.time() < deadline:
            if not call_daemon({"op": "ping"}, timeout_s=0.5).get("ok"):
                break
            time.sleep(0.1)

        if call_daemon({"op": "ping"}, timeout_s=0.5).get("ok"):
            if daemon_pid > 0:
                extra_pids.append(daemon_pid)
            if not _terminate_same_home_daemons(home, extra_pids=extra_pids):
                return False
            if call_daemon({"op": "ping"}, timeout_s=0.5).get("ok"):
                return False
    else:
        pid_path = home / "daemon" / "ccccd.pid"
        try:
            txt = pid_path.read_text(encoding="utf-8").strip() if pid_path.exists() else ""
            daemon_pid = int(txt) if txt.isdigit() else 0
        except Exception:
            daemon_pid = 0
        if daemon_pid > 0 and pid_is_alive(daemon_pid):
            extra_pids.append(daemon_pid)

    if not _terminate_same_home_daemons(home, extra_pids=extra_pids):
        return False

    try:
        _cleanup_daemon_state_files(home)
    except Exception:
        pass
    return True


def _print_json(obj: Any) -> None:
    print(json.dumps(obj, ensure_ascii=False, indent=2))

def _parse_json_object_arg(raw: Any, *, field: str) -> dict[str, Any]:
    text = str(raw or "").strip()
    if not text:
        return {}
    try:
        obj = json.loads(text)
    except Exception as e:
        raise ValueError(f"{field} must be valid JSON object: {e}") from e
    if not isinstance(obj, dict):
        raise ValueError(f"{field} must be a JSON object")
    return dict(obj)

def _normalize_space_query_options_cli(options: dict[str, Any]) -> dict[str, Any]:
    normalized = dict(options or {})
    unsupported = sorted(k for k in normalized.keys() if str(k or "").strip() not in _SPACE_QUERY_OPTION_KEYS)
    if unsupported:
        if any(str(k or "").strip() in {"language", "lang"} for k in unsupported):
            raise ValueError(
                "query options do not support language/lang; NotebookLM query API has no language parameter. "
                "Put language requirements in query text."
            )
        supported = ", ".join(sorted(_SPACE_QUERY_OPTION_KEYS))
        raise ValueError(
            f"unsupported query options: {', '.join(str(k or '').strip() for k in unsupported)} (supported: {supported})"
        )

    if "source_ids" in normalized:
        raw_source_ids = normalized.get("source_ids")
        if raw_source_ids is None:
            normalized["source_ids"] = []
        elif not isinstance(raw_source_ids, list):
            raise ValueError("options.source_ids must be an array of non-empty strings")
        else:
            source_ids: list[str] = []
            for idx, item in enumerate(raw_source_ids):
                sid = str(item or "").strip()
                if not sid:
                    raise ValueError(f"options.source_ids[{idx}] must be a non-empty string")
                source_ids.append(sid)
            normalized["source_ids"] = source_ids
    return normalized

def _default_runner_kind() -> str:
    """Use the current product-standard runner."""
    return "pty"

def _ensure_daemon_running() -> bool:
    resp = call_daemon({"op": "ping"}, timeout_s=1.0)
    if resp.get("ok"):
        # If the daemon is from a different version, restart it. This commonly happens
        # after a package upgrade while an old background daemon is still running,
        # causing "unknown op" errors in newer Web/UI flows.
        try:
            res = resp.get("result") if isinstance(resp.get("result"), dict) else {}
            daemon_version = str(res.get("version") or "").strip()
            daemon_pid = int(res.get("pid") or 0)
        except Exception:
            daemon_version = ""
            daemon_pid = 0

        def _daemon_supports_required_ops() -> bool:
            try:
                # Probe a couple of newer ops so we don't get stuck with a stale
                # background daemon that lacks features (even if version string matches).
                for probe in (
                    {"op": "observability_get"},
                    {"op": "debug_snapshot", "args": {}},
                ):
                    r = call_daemon(probe, timeout_s=1.0)
                    if r.get("ok"):
                        continue
                    err = r.get("error") if isinstance(r.get("error"), dict) else {}
                    if str(err.get("code") or "") == "unknown_op":
                        return False
                return True
            except Exception:
                return False

        needs_restart = False
        if daemon_version and daemon_version != __version__:
            needs_restart = True
        elif not _daemon_supports_required_ops():
            needs_restart = True

        if needs_restart:
            try:
                shutdown_resp = call_daemon({"op": "shutdown"}, timeout_s=2.0)
                if not bool(shutdown_resp.get("ok")):
                    print("warn: daemon restart requested but shutdown RPC failed; trying fallback termination", file=sys.stderr)
            except Exception as e:
                print(f"warn: daemon restart requested but shutdown RPC errored: {e}", file=sys.stderr)

            deadline = time.time() + 2.0
            while time.time() < deadline:
                if not call_daemon({"op": "ping"}, timeout_s=0.5).get("ok"):
                    break
                time.sleep(0.1)

            # Last resort: terminate the stale daemon by pid (best-effort).
            if call_daemon({"op": "ping"}, timeout_s=0.5).get("ok") and daemon_pid > 0:
                try:
                    killed = best_effort_signal_pid(daemon_pid, SOFT_TERMINATE_SIGNAL, include_group=True)
                    if not killed:
                        print(f"warn: failed to terminate stale daemon pid={daemon_pid}: signal not delivered", file=sys.stderr)
                        return True
                except Exception as e:
                    print(f"warn: failed to terminate stale daemon pid={daemon_pid}: {e}", file=sys.stderr)
                    return True

                deadline = time.time() + 2.0
                while time.time() < deadline:
                    if not call_daemon({"op": "ping"}, timeout_s=0.5).get("ok"):
                        break
                    time.sleep(0.1)

            # If it's still running, don't stomp its socket/pid files.
            if call_daemon({"op": "ping"}, timeout_s=0.5).get("ok"):
                return True

            # Cleanup stale socket/pid files so a new daemon can bind.
            try:
                home = ensure_home()
                sock_path = home / "daemon" / "ccccd.sock"
                addr_path = home / "daemon" / "ccccd.addr.json"
                pid_path = home / "daemon" / "ccccd.pid"
                sock_path.unlink(missing_ok=True)
                addr_path.unlink(missing_ok=True)
                pid_path.unlink(missing_ok=True)
            except Exception as e:
                print(f"warn: failed to cleanup stale daemon state files: {e}", file=sys.stderr)
        else:
            return True

    try:
        subprocess.run(
            [sys.executable, "-m", "cccc.daemon_main", "start"],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            check=False,
        )
    except Exception:
        return False

    # Windows TCP startup can take longer than POSIX AF_UNIX; be patient but bounded.
    attempts = 200 if os.name == "nt" else 60
    for _ in range(attempts):
        time.sleep(0.05)
        resp = call_daemon({"op": "ping"}, timeout_s=0.5)
        if resp.get("ok"):
            return True
    return False

def _daemon_response_allows_local_fallback(resp: dict[str, Any]) -> bool:
    error = resp.get("error") if isinstance(resp.get("error"), dict) else {}
    return str(error.get("code") or "").strip() == "daemon_unavailable"

def _return_daemon_rejection(resp: dict[str, Any]) -> int:
    _print_json(resp)
    return 2

def _resolve_group_id(explicit: str) -> str:
    gid = (explicit or "").strip()
    if gid:
        return gid
    active = load_active()
    return str(active.get("active_group_id") or "").strip()

def _env_flag(name: str, *, default: bool = False) -> bool:
    return coerce_bool(os.environ.get(name), default=default)

def _is_first_run() -> bool:
    """Check if this is the first time running CCCC."""
    home = ensure_home()
    marker = home / ".initialized"
    if marker.exists():
        return False
    # Create marker file
    try:
        marker.write_text(__version__)
    except Exception:
        pass
    return True

def _show_welcome() -> None:
    """Show welcome message for first-time users."""
    print()
    print("=" * 60)
    print("  Welcome to CCCC - Collaborative Code Coordination Center")
    print("=" * 60)
    print()
    print("Quick Start:")
    print("  1. Create a working group:  cccc attach .")
    print("  2. Add an agent:            cccc actor add my-agent --runtime claude")
    print("  3. Start the group:         cccc group start")
    print("  4. Open Web UI:             http://127.0.0.1:8848")
    print()
    print("Useful Commands:")
    print("  cccc status      - Show daemon, groups, and actors")
    print("  cccc doctor      - Check environment and available runtimes")
    print("  cccc setup       - Configure MCP for agent runtimes")
    print()
    print("Documentation: https://github.com/ChesterRa/cccc")
    print("=" * 60)
    print()

def _default_entry(*, web_host_override: str = "", web_port_override: Optional[int] = None) -> int:
    """Default entry: start daemon + web together, stop both on Ctrl+C."""
    import threading
    
    from ..paths import ensure_home
    
    # Show welcome message on first run
    if _is_first_run():
        _show_welcome()
    
    home = ensure_home()
    app_lock, app_lock_error = _acquire_default_entry_lock(home)
    if app_lock is None:
        print(f"[cccc] Error: {app_lock_error or 'Could not acquire CCCC app lock'}", file=sys.stderr)
        return 1

    daemon_process = None
    shutdown_requested = False
    log_path = home / "daemon" / "ccccd.log"
    previous_signal_handlers: dict[int, Any] = {}

    def _handle_shutdown_signal(signum: int, _frame: Any) -> None:
        raise KeyboardInterrupt()

    for sig in (
        getattr(signal, "SIGINT", None),
        getattr(signal, "SIGBREAK", None),
        getattr(signal, "SIGTERM", None),
        getattr(signal, "SIGHUP", None),
    ):
        if sig is None:
            continue
        try:
            previous_signal_handlers[int(sig)] = signal.getsignal(sig)
            signal.signal(sig, _handle_shutdown_signal)
        except Exception:
            continue
    
    def _start_daemon() -> bool:
        nonlocal daemon_process
        max_attempts = 2
        for attempt in range(max_attempts):
            if not _stop_existing_daemon(home):
                print("[cccc] Failed to stop existing daemon before restart", file=sys.stderr)
                return False

            # Start daemon as subprocess, capture output to log file
            (home / "daemon").mkdir(parents=True, exist_ok=True)
            log_file = log_path.open("a", encoding="utf-8")
            try:
                daemon_env = os.environ.copy()
                daemon_env["CCCC_HOME"] = str(home)
                daemon_env["CCCC_DAEMON_SUPERVISOR_PID"] = str(os.getpid())
                daemon_process = subprocess.Popen(
                    resolve_background_python_argv([sys.executable, "-m", "cccc.daemon_main", "run"]),
                    stdout=log_file,
                    stderr=log_file,
                    stdin=subprocess.DEVNULL,
                    env=daemon_env,
                    cwd=str(home),
                    **supervised_process_popen_kwargs(),
                )
                try:
                    log_file.close()
                except Exception:
                    pass
            except Exception as e:
                try:
                    log_file.close()
                except Exception:
                    pass
                print(f"[cccc] Failed to start daemon: {e}", file=sys.stderr)
                return False

            # Wait for daemon to be ready
            for _ in range(50):
                time.sleep(0.1)
                ret = daemon_process.poll()
                if ret is not None:
                    still_running = bool(call_daemon({"op": "ping"}, timeout_s=0.5).get("ok"))
                    if attempt + 1 < max_attempts:
                        if still_running and int(ret) == 0:
                            print("[cccc] Another daemon remained active during startup; retrying clean restart...", file=sys.stderr)
                        else:
                            print("[cccc] Daemon exited during startup; retrying clean restart...", file=sys.stderr)
                        break
                    if still_running and int(ret) == 0:
                        print("[cccc] Another daemon is still active after clean restart attempts.", file=sys.stderr)
                        return False
                    print(f"[cccc] Daemon exited during startup (exit code {ret}). Check log: {log_path}", file=sys.stderr)
                    try:
                        lines = log_path.read_text(encoding="utf-8").strip().split("\n")[-20:]
                        for line in lines:
                            print(f"  {line}", file=sys.stderr)
                    except Exception:
                        pass
                    return False
                resp = call_daemon({"op": "ping"})
                if resp.get("ok"):
                    return True
            else:
                if attempt + 1 < max_attempts:
                    print("[cccc] Daemon did not become ready in time; retrying clean restart...", file=sys.stderr)
                    continue
                print("[cccc] Daemon failed to become ready in time", file=sys.stderr)
                return False

        return False
    
    # Lifecycle helper — real logic lives in daemon_lifecycle.py so tests
    # can import and drive it directly with injectable deps.
    from .daemon_lifecycle import DaemonLifecycle

    def _read_log_tail(n: int) -> list[str]:
        try:
            return log_path.read_text().strip().split("\n")[-n:]
        except Exception:
            return []

    def _start_daemon_for_lifecycle() -> bool:
        """Wraps _start_daemon and syncs process ref to lifecycle."""
        ok = _start_daemon()
        # _start_daemon sets daemon_process via nonlocal; sync to lifecycle.
        _lifecycle.process = daemon_process
        return ok

    _lifecycle = DaemonLifecycle(
        call_daemon=lambda req, timeout: call_daemon(req, timeout_s=timeout),
        start_daemon=_start_daemon_for_lifecycle,
        is_shutdown_requested=lambda: shutdown_requested,
        log=lambda msg: print(f"[cccc] {msg}", file=sys.stderr),
        read_log_tail=_read_log_tail,
    )

    def _stop_daemon() -> None:
        nonlocal daemon_process
        _lifecycle.stop_daemon()
        daemon_process = _lifecycle.process
    
    # Saved binding is the baseline, but top-level `cccc --host/--port`
    # must win for this invocation, including supervised child restarts.
    def _resolve_invocation_binding() -> tuple[str, int]:
        return _resolve_invocation_web_server_binding(
            web_host_override=web_host_override,
            web_port_override=web_port_override,
        )

    host, port = _resolve_invocation_binding()
    log_level = str(os.environ.get("CCCC_WEB_LOG_LEVEL") or "").strip() or "info"
    reload_mode = _env_flag("CCCC_WEB_RELOAD", default=False)
    web_process = None

    # Start daemon
    try:
        if not _stop_existing_web_runtime(home):
            print("[cccc] Failed to stop existing web runtime before restart", file=sys.stderr)
            return 1

        print("[cccc] Starting daemon...", file=sys.stderr)
        if not _start_daemon():
            print("[cccc] Error: Could not start daemon", file=sys.stderr)
            return 1
        # Sync initial process reference to lifecycle helper.
        _lifecycle.process = daemon_process
        print("[cccc] Daemon started", file=sys.stderr)

        # Start daemon monitor thread
        monitor_thread = threading.Thread(target=_lifecycle.monitor_daemon, daemon=True)
        monitor_thread.start()

        def _get_lan_ip() -> str:
            try:
                s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
                s.settimeout(0.1)
                s.connect(("8.8.8.8", 80))
                ip = s.getsockname()[0]
                s.close()
                return ip
            except Exception:
                return ""

        def _print_web_banner(cur_host: str, cur_port: int) -> None:
            print("[cccc] Starting web server...", file=sys.stderr)
            print(f"[cccc]   Local:   http://{_http_host_literal(cur_host)}:{cur_port}", file=sys.stderr)
            lan_ip = _get_lan_ip()
            if lan_ip and lan_ip != cur_host and lan_ip != "127.0.0.1":
                print(f"[cccc]   Network: http://{lan_ip}:{cur_port}", file=sys.stderr)

        web_process, web_error = start_supervised_web_child(
            home=home,
            host=host,
            port=port,
            mode=str(os.environ.get("CCCC_WEB_MODE") or "normal"),
            reload=reload_mode,
            log_level=log_level,
            launch_source="default_entry",
        )
        if web_process is None:
            print(f"[cccc] Error: {web_error or 'Could not start web server'}", file=sys.stderr)
            return 1
        _print_web_banner(host, port)

        current_host, current_port = host, port
        while True:
            ret = wait_for_child_exit_interruptibly(web_process)
            if int(ret or 0) == WEB_RUNTIME_RESTART_EXIT_CODE:
                print("[cccc] Applying saved Web binding changes...", file=sys.stderr)
                restarted, current_host, current_port = restart_supervised_web_child_with_fallback(
                    home=home,
                    previous_host=current_host,
                    previous_port=current_port,
                    mode=str(os.environ.get("CCCC_WEB_MODE") or "normal"),
                    reload=reload_mode,
                    log_level=log_level,
                    launch_source="default_entry",
                    resolve_binding=_resolve_invocation_binding,
                    log=lambda msg: print(f"[cccc] {msg}", file=sys.stderr),
                )
                if restarted is None:
                    return 1
                web_process = restarted
                _print_web_banner(current_host, current_port)
                continue
            return int(ret or 0)
    except (SystemExit, KeyboardInterrupt):
        pass
    finally:
        shutdown_requested = True
        if web_process is not None:
            web_pid = int(getattr(web_process, "pid", 0) or 0)
            try:
                stop_web_child(web_process, timeout_s=2.0)
            except Exception:
                pass
            try:
                clear_web_runtime_state(home=home, pid=web_pid if web_pid > 0 else None)
            except Exception:
                pass
        _stop_daemon()
        for sig_num, previous_handler in previous_signal_handlers.items():
            try:
                signal.signal(sig_num, previous_handler)
            except Exception:
                pass
        release_lockfile(app_lock)
    
    return 0


# Export helper symbols (including leading underscore names) for CLI submodules.
__all__ = [name for name in globals().keys() if not name.startswith("__") or name == "__version__"]
