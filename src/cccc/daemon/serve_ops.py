from __future__ import annotations

import socket
import threading
import time
from pathlib import Path
from typing import Any, Callable, Dict


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
            except Exception:
                pass
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
                        except Exception:
                            pass
            except Exception:
                pass
            now = time.time()
            if now >= next_compact:
                next_compact = now + 60.0
                try:
                    compact_ledgers(home)
                except Exception:
                    pass
            stop_event.wait(1.0)

    t = threading.Thread(target=_automation_loop, name="cccc-automation", daemon=True)
    t.start()
    return t


def bind_server_socket(
    *,
    transport: str,
    sock_path: Path,
    daemon_tcp_bind_host: Callable[[], str],
    daemon_tcp_port: Callable[[], int],
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
