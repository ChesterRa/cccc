from __future__ import annotations

import errno
import logging
import socket
import threading
import time
import uuid
from pathlib import Path
from typing import Any, Callable, Dict, Optional

from .actor_runtime_cache import replace_group_runtime
from .runner_state_ops import headless_state_running, read_headless_state
from ..kernel.context import ContextStorage
from ..kernel.working_state import (
    derive_effective_working_state,
)

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
    automation_interval_seconds: float = 5.0,
    initial_automation_delay_seconds: float = 5.0,
) -> threading.Thread:
    def _automation_loop() -> None:
        next_compact = 0.0
        next_automation = time.time() + max(0.0, float(initial_automation_delay_seconds or 0.0))
        automation_interval = max(1.0, float(automation_interval_seconds or 5.0))
        while not stop_event.is_set():
            now = time.time()
            if now >= next_automation:
                try:
                    automation_tick(home=home)
                except Exception as e:
                    _log_loop_error("automation_tick failed", e)
                next_automation = now + automation_interval
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


def start_request_execution_thread(
    *,
    request_queue: Any,
    name: str = "cccc-request-worker",
) -> threading.Thread:
    t = threading.Thread(target=request_queue.run_forever, name=str(name or "cccc-request-worker"), daemon=True)
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
    drain_space_sync_runs: Optional[Callable[[int], int]] = None,
    wake_event: Optional[threading.Event] = None,
    interval_seconds: float = 30.0,
) -> threading.Thread:
    def _space_sync_loop() -> None:
        interval = max(5.0, float(interval_seconds or 30.0))
        next_periodic = 0.0
        while not stop_event.is_set():
            processed = 0
            if drain_space_sync_runs is not None:
                try:
                    processed = int(drain_space_sync_runs(4) or 0)
                except Exception as e:
                    _log_loop_error("drain_space_sync_runs failed", e)
            try:
                now = time.time()
                if now >= next_periodic:
                    tick_space_sync()
                    next_periodic = now + interval
            except Exception as e:
                _log_loop_error("tick_space_sync failed", e)
            if processed > 0:
                continue
            timeout = max(0.2, next_periodic - time.time())
            if wake_event is not None:
                wake_event.wait(timeout)
                wake_event.clear()
            else:
                stop_event.wait(timeout)

    t = threading.Thread(target=_space_sync_loop, name="cccc-space-sync", daemon=True)
    t.start()
    return t


def start_supervisor_watchdog_thread(
    *,
    stop_event: threading.Event,
    supervisor_pid: int,
    pid_alive: Callable[[int], bool],
    interval_seconds: float = 0.5,
) -> threading.Thread | None:
    target_pid = int(supervisor_pid or 0)
    if target_pid <= 0:
        return None

    def _supervisor_watchdog_loop() -> None:
        interval = max(0.2, float(interval_seconds or 0.5))
        while not stop_event.is_set():
            try:
                if not pid_alive(target_pid):
                    stop_event.set()
                    return
            except Exception as e:
                _log_loop_error("supervisor_watchdog failed", e)
                stop_event.set()
                return
            stop_event.wait(interval)

    t = threading.Thread(target=_supervisor_watchdog_loop, name="cccc-supervisor-watchdog", daemon=True)
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


def start_actor_activity_thread(
    *,
    stop_event: threading.Event,
    home: Path,
    pty_supervisor: Any,
    headless_supervisor: Any,
    codex_supervisor: Any,
    claude_supervisor: Any = None,
    event_broadcaster: Any,
    load_group: Callable[[str], Any],
    interval_seconds: float = 1.0,
) -> threading.Thread:
    """Periodically publish actor.activity SSE events with effective runtime status."""
    import uuid

    def _actor_activity_loop() -> None:
        interval = max(1.0, float(interval_seconds or 1.0))
        prev_runtime_by_group: Dict[str, Dict[str, Dict[str, Any]]] = {}
        while not stop_event.is_set():
            try:
                groups_base = home / "groups"
                if groups_base.exists():
                    for gp in groups_base.glob("*/group.yaml"):
                        gid = gp.parent.name
                        group = load_group(gid)
                        if group is None:
                            continue
                        storage = ContextStorage(group)
                        agent_rows = [
                            {
                                "id": agent.id,
                                "hot": {
                                    "focus": agent.hot.focus,
                                    "active_task_id": agent.hot.active_task_id,
                                },
                                "updated_at": agent.updated_at,
                            }
                            for agent in storage.load_agents().agents
                        ]
                        agent_state_by_id = {
                            str(item.get("id") or "").strip(): item
                            for item in agent_rows
                            if isinstance(item, dict) and str(item.get("id") or "").strip()
                        } if isinstance(agent_rows, list) else {}
                        actors_data = []
                        actors_snapshot: Dict[str, Dict[str, Any]] = {}
                        actor_list = group.doc.get("actors")
                        if not isinstance(actor_list, list):
                            continue
                        for actor in actor_list:
                            if not isinstance(actor, dict):
                                continue
                            aid = str(actor.get("id") or "").strip()
                            if not aid:
                                continue
                            runtime = str(actor.get("runtime") or "").strip().lower()
                            runner_kind = str(actor.get("runner") or "pty").strip().lower() or "pty"
                            effective_runner = "headless" if runner_kind == "headless" else "pty"
                            running = False
                            idle = None
                            headless_state = None
                            if runtime == "web_model" and effective_runner == "headless":
                                headless_state = read_headless_state(gid, aid)
                                running = bool(headless_state_running(gid, aid))
                            elif runtime == "codex" and effective_runner == "headless":
                                headless_state = codex_supervisor.get_state(group_id=gid, actor_id=aid)
                                running = bool(headless_state is not None and codex_supervisor.actor_running(gid, aid))
                            elif runtime == "claude" and effective_runner == "headless" and claude_supervisor is not None:
                                headless_state = claude_supervisor.get_state(group_id=gid, actor_id=aid)
                                running = bool(headless_state is not None and claude_supervisor.actor_running(gid, aid))
                            elif effective_runner == "headless":
                                state = headless_supervisor.get_state(group_id=gid, actor_id=aid)
                                headless_state = state.model_dump() if state is not None else None
                                running = bool(state is not None and headless_supervisor.actor_running(gid, aid))
                            else:
                                running = bool(pty_supervisor.actor_running(gid, aid))
                                idle = pty_supervisor.idle_seconds(group_id=gid, actor_id=aid) if running else None
                            pty_terminal_override = None
                            if effective_runner == "pty" and running:
                                try:
                                    pty_terminal_override = pty_supervisor.terminal_override(group_id=gid, actor_id=aid)
                                except Exception:
                                    pty_terminal_override = None
                            if not running:
                                continue
                            payload = {
                                "id": aid,
                                "running": True,
                                "runner_effective": effective_runner,
                                "idle_seconds": round(float(idle), 1) if idle is not None else None,
                            }
                            payload.update(
                                derive_effective_working_state(
                                    running=running,
                                    effective_runner=effective_runner,
                                    runtime=str(actor.get("runtime") or ""),
                                    idle_seconds=idle,
                                    pty_terminal_override=pty_terminal_override,
                                    agent_state=agent_state_by_id.get(aid),
                                    headless_state=headless_state,
                                )
                            )
                            actors_data.append(payload)
                            actors_snapshot[aid] = payload
                        replace_group_runtime(gid, actors_snapshot)

                        # Broadcast running actors in-memory (daemon IPC).
                        if actors_data:
                            activity_event = {
                                "id": uuid.uuid4().hex,
                                "kind": "actor.activity",
                                "ts": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
                                "group_id": gid,
                                "by": "system",
                                "data": {"actors": actors_data},
                            }
                            event_broadcaster.publish(activity_event)

                        # Detect working-state transitions so the web port
                        # (which tails the ledger file) also receives the
                        # update.  Without this, actor.activity only flows
                        # through the in-memory EventBroadcaster and never
                        # reaches the web frontend.
                        prev_snapshot = prev_runtime_by_group.get(gid, {})
                        state_changed = len(actors_snapshot) != len(prev_snapshot)
                        if not state_changed:
                            for aid, payload in actors_snapshot.items():
                                prev_actor = prev_snapshot.get(aid)
                                if prev_actor is None or payload.get("effective_working_state") != prev_actor.get("effective_working_state"):
                                    state_changed = True
                                    break
                        # Emit "stopped" entries for actors that disappeared
                        # (crashed/stopped since last tick) so the web
                        # frontend can clear stale "working" halos.
                        stopped_entries: list[Dict[str, Any]] = []
                        for prev_aid, prev_actor in prev_snapshot.items():
                            if prev_aid not in actors_snapshot:
                                prev_runner = str(prev_actor.get("runner_effective") or "pty").strip() or "pty"
                                state_changed = True
                                stopped_entries.append({
                                    "id": prev_aid,
                                    "running": False,
                                    "runner_effective": prev_runner,
                                    "idle_seconds": None,
                                    "effective_working_state": "stopped",
                                    "effective_working_reason": "runner_not_running",
                                    "effective_working_updated_at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
                                    "effective_active_task_id": None,
                                })
                        prev_runtime_by_group[gid] = {
                            aid: {
                                "effective_working_state": p.get("effective_working_state"),
                                "runner_effective": p.get("runner_effective"),
                            }
                            for aid, p in actors_snapshot.items()
                        }
                        if state_changed:
                            ledger_actors = actors_data + stopped_entries
                            if ledger_actors:
                                try:
                                    from ..kernel.ledger import append_event
                                    append_event(
                                        group.ledger_path,
                                        kind="actor.activity",
                                        group_id=gid,
                                        scope_key="",
                                        by="system",
                                        data={"actors": ledger_actors},
                                    )
                                except Exception as e:
                                    _log_loop_error(
                                        f"actor_activity ledger append failed group={gid} actor_count={len(ledger_actors)}",
                                        e,
                                    )
            except Exception as e:
                _log_loop_error("actor_activity_tick failed", e)
            stop_event.wait(interval)

    t = threading.Thread(target=_actor_activity_loop, name="cccc-actor-activity", daemon=True)
    t.start()
    return t


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
    codex_stop_all: Callable[[], Any],
    claude_stop_all: Callable[[], Any] = lambda: None,
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
        codex_stop_all()
    except Exception:
        pass
    try:
        claude_stop_all()
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
