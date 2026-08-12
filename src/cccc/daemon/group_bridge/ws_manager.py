"""Daemon-owned Group Bridge WebSocket session client manager."""

from __future__ import annotations

import threading
from pathlib import Path
from typing import Any, Callable, Dict

from ...kernel.group_bridge.pairing import list_trusts
from .ws_client import start_group_bridge_session_client

TrustLister = Callable[..., list[Dict[str, Any]]]
ClientStarter = Callable[..., threading.Thread]
ClientState = Dict[str, tuple[threading.Thread, threading.Event]]
SESSION_TRANSPORTS = frozenset({"group_bridge_session"})


def tick_group_bridge_session_clients(
    *,
    home: Path,
    stop_event: threading.Event,
    state: ClientState,
    list_trusts_fn: TrustLister = list_trusts,
    start_client: ClientStarter = start_group_bridge_session_client,
) -> Dict[str, int]:
    started = 0
    active = 0
    desired: Dict[str, Dict[str, str]] = {}
    for trust in list_trusts_fn(home=home):
        endpoint = str(trust.get("remote_endpoint") or "").strip()
        local_group_id = str(trust.get("group_id") or "").strip()
        remote_group_id = str(trust.get("remote_group_id") or "").strip()
        remote_peer_id = str(trust.get("remote_peer_id") or "").strip()
        if str(trust.get("status") or "") != "active":
            continue
        if str(trust.get("transport") or "") not in SESSION_TRANSPORTS:
            continue
        if not endpoint or not local_group_id or not remote_group_id or not remote_peer_id:
            continue
        key = "|".join((local_group_id, remote_group_id, remote_peer_id, endpoint))
        desired[key] = {
            "remote_base_url": endpoint,
            "local_group_id": local_group_id,
            "remote_group_id": remote_group_id,
            "remote_peer_id": remote_peer_id,
        }

    for key, (thread, client_stop) in list(state.items()):
        if stop_event.is_set() or key not in desired or not thread.is_alive():
            client_stop.set()
            state.pop(key, None)

    if stop_event.is_set():
        return {"started": 0, "active": 0}

    for key, client_args in desired.items():
        existing = state.get(key)
        if existing is not None and existing[0].is_alive():
            active += 1
            continue
        client_stop = threading.Event()
        state[key] = (start_client(**client_args, stop=client_stop), client_stop)
        started += 1
        active += 1
    return {"started": started, "active": active}


def start_group_bridge_session_manager_thread(
    *,
    home: Path,
    stop_event: threading.Event,
    interval_seconds: float = 10.0,
) -> threading.Thread:
    state: ClientState = {}

    def run() -> None:
        interval = max(1.0, float(interval_seconds or 10.0))
        try:
            while not stop_event.is_set():
                try:
                    tick_group_bridge_session_clients(home=home, stop_event=stop_event, state=state)
                except Exception:
                    pass
                stop_event.wait(interval)
        finally:
            for _thread, client_stop in state.values():
                client_stop.set()
            state.clear()

    thread = threading.Thread(target=run, name="cccc-group-bridge-ws-manager", daemon=True)
    thread.start()
    return thread
