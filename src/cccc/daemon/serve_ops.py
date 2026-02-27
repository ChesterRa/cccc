from __future__ import annotations

import errno
import logging
import socket
import threading
import time
from pathlib import Path
from typing import Any, Callable, Dict

_LOG = logging.getLogger("cccc.daemon.serve_ops")
_LOOP_ERROR_LAST_TS: Dict[str, float] = {}
_LOOP_ERROR_WINDOW_SECONDS = 30.0
_LOOP_ERROR_LOCK = threading.Lock()


def _log_loop_error(key: str, exc: Exception) -> None:
    now = time.time()
    with _LOOP_ERROR_LOCK:
        last = float(_LOOP_ERROR_LAST_TS.get(key) or 0.0)
        if now - last < _LOOP_ERROR_WINDOW_SECONDS:
            return
        _LOOP_ERROR_LAST_TS[key] = now
    _LOG.warning("%s: %s", key, exc)


def start_automation_thread(
    *,
    stop_event: threading.Event,
    home: Path,
    automation_tick: Callable[..., Any],
    load_group: Callable[[str], Any],
    group_running: Callable[[str], bool],
    tick_delivery: Callable[[Any], Any],
    compact_ledgers: Callable[[Path], Any],
) -> threading.Thread:
    def _automation_loop() -> None:
        next_compact = 0.0
        while not stop_event.is_set():
            try:
                automation_tick(home=home)
            except Exception as e:
                _log_loop_error("automation_tick failed", e)
            try:
                base = home / "groups"
                if base.exists():
                    for gp in base.glob("*/group.yaml"):
                        gid = gp.parent.name
                        group = load_group(gid)
                        if group is None:
                            continue
                        if not group_running(gid):
                            continue
                        try:
                            tick_delivery(group)
                        except Exception as e:
                            _log_loop_error(f"tick_delivery failed group={gid}", e)
            except Exception as e:
                _log_loop_error("automation scan failed", e)
            now = time.time()
            if now >= next_compact:
                next_compact = now + 60.0
                try:
                    compact_ledgers(home)
                except Exception as e:
                    _log_loop_error("compact_ledgers failed", e)
            stop_event.wait(1.0)

    t = threading.Thread(target=_automation_loop, name="cccc-automation", daemon=True)
    t.start()
    return t


def start_space_jobs_thread(
    *,
    stop_event: threading.Event,
    tick_space_jobs: Callable[[], Any],
    interval_seconds: float = 1.0,
) -> threading.Thread:
    def _space_jobs_loop() -> None:
        interval = max(0.2, float(interval_seconds or 1.0))
        while not stop_event.is_set():
            try:
                tick_space_jobs()
            except Exception as e:
                _log_loop_error("tick_space_jobs failed", e)
            stop_event.wait(interval)

    t = threading.Thread(target=_space_jobs_loop, name="cccc-space-jobs", daemon=True)
    t.start()
    return t


def start_space_sync_thread(
    *,
    stop_event: threading.Event,
    tick_space_sync: Callable[[], Any],
    interval_seconds: float = 30.0,
) -> threading.Thread:
    def _space_sync_loop() -> None:
        interval = max(5.0, float(interval_seconds or 30.0))
        while not stop_event.is_set():
            try:
                tick_space_sync()
            except Exception as e:
                _log_loop_error("tick_space_sync failed", e)
            stop_event.wait(interval)

    t = threading.Thread(target=_space_sync_loop, name="cccc-space-sync", daemon=True)
    t.start()
    return t


def start_capability_sync_thread(
    *,
    stop_event: threading.Event,
    tick_capability_sync: Callable[[], Any],
    interval_seconds: float = 900.0,
) -> threading.Thread:
    def _capability_sync_loop() -> None:
        interval = max(30.0, float(interval_seconds or 900.0))
        while not stop_event.is_set():
            try:
                tick_capability_sync()
            except Exception as e:
                _log_loop_error("tick_capability_sync failed", e)
            stop_event.wait(interval)

    t = threading.Thread(target=_capability_sync_loop, name="cccc-capability-sync", daemon=True)
    t.start()
    return t


def bind_server_socket(
    *,
    transport: str,
    sock_path: Path,
    daemon_tcp_bind_host: Callable[[], str],
    daemon_tcp_port: Callable[[], int],
    daemon_tcp_port_is_explicit: Callable[[], bool] = lambda: False,
) -> tuple[socket.socket, Dict[str, Any]]:
    tr = str(transport or "").strip().lower()
    if tr == "unix":
        af_unix = getattr(socket, "AF_UNIX", None)
        assert af_unix is not None
        s = socket.socket(af_unix, socket.SOCK_STREAM)
        endpoint = {"transport": "unix", "path": str(sock_path)}
        s.bind(str(sock_path))
        return s, endpoint

    host = daemon_tcp_bind_host()
    port = daemon_tcp_port()
    reserved_ports = {8848} if port == 0 else set()
    s = None
    endpoint = {"transport": "tcp", "host": host, "port": port}
    for _ in range(25):
        cand = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        try:
            cand.bind((host, port))
            bound = cand.getsockname()
            try:
                bound_port = int(bound[1])
            except Exception:
                bound_port = 0
            if bound_port and bound_port in reserved_ports:
                cand.close()
                continue
            s = cand
            break
        except OSError as e:
            try:
                cand.close()
            except Exception:
                pass
            if e.errno == errno.EADDRINUSE and not daemon_tcp_port_is_explicit():
                _LOG.warning("Port %d in use, falling back to dynamic port", port)
                port = 0
                endpoint["port"] = 0
                reserved_ports = {8848}
                continue
            raise
        except Exception:
            try:
                cand.close()
            except Exception:
                pass
            raise
    if s is None:
        s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        s.bind((host, port))
    try:
        bound_host, bound_port = s.getsockname()[:2]
        endpoint["host"] = str(bound_host)
        endpoint["port"] = int(bound_port)
    except Exception:
        pass
    return s, endpoint


def write_daemon_addr(
    *,
    atomic_write_json: Callable[..., Any],
    addr_path: Path,
    endpoint: Dict[str, Any],
    pid: int,
    version: str,
    now_iso: str,
) -> None:
    try:
        atomic_write_json(
            addr_path,
            {
                "v": 1,
                "transport": str(endpoint.get("transport") or ""),
                "path": str(endpoint.get("path") or ""),
                "host": str(endpoint.get("host") or ""),
                "port": int(endpoint.get("port") or 0),
                "pid": int(pid),
                "version": str(version),
                "ts": str(now_iso or ""),
            },
        )
    except Exception:
        pass


def start_bootstrap_thread(
    *,
    maybe_autostart_running_groups: Callable[[], Any],
    maybe_autostart_enabled_im_bridges: Callable[[], Any],
) -> threading.Thread:
    def _bootstrap_after_listen() -> None:
        try:
            maybe_autostart_running_groups()
        except Exception:
            pass
        try:
            maybe_autostart_enabled_im_bridges()
        except Exception:
            pass

    t = threading.Thread(target=_bootstrap_after_listen, name="cccc-bootstrap", daemon=True)
    t.start()
    return t


def cleanup_after_stop(
    *,
    stop_event: threading.Event,
    home: Path,
    best_effort_killpg: Callable[[int, Any], Any],
    im_stop_all: Callable[..., Any],
    pty_stop_all: Callable[[], Any],
    headless_stop_all: Callable[[], Any],
    sock_path: Path,
    addr_path: Path,
    pid_path: Path,
    release_lockfile: Callable[[Any], Any],
    lock_handle: Any,
) -> None:
    stop_event.set()
    try:
        im_stop_all(home, best_effort_killpg=best_effort_killpg)
    except Exception:
        pass
    try:
        pty_stop_all()
    except Exception:
        pass
    try:
        headless_stop_all()
    except Exception:
        pass
    try:
        if sock_path.exists():
            sock_path.unlink()
    except Exception:
        pass
    try:
        if addr_path.exists():
            addr_path.unlink()
    except Exception:
        pass
    try:
        if pid_path.exists():
            pid_path.unlink()
    except Exception:
        pass
    try:
        release_lockfile(lock_handle)
    except Exception:
        pass
