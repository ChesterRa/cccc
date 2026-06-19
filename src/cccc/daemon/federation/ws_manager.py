"""Daemon-owned federation WebSocket session client manager."""

from __future__ import annotations

import threading
from pathlib import Path
from typing import Any, Callable, Dict

from ...kernel.federation.pairing import list_trusts
from .ws_client import start_federation_session_client

TrustLister = Callable[..., list[Dict[str, Any]]]
ClientStarter = Callable[..., threading.Thread]
SESSION_TRANSPORTS = frozenset({"federation_session"})


def tick_federation_session_clients(
    *,
    home: Path,
    stop_event: threading.Event,
    state: Dict[str, threading.Thread],
    list_trusts_fn: TrustLister = list_trusts,
    start_client: ClientStarter = start_federation_session_client,
) -> Dict[str, int]:
    started = 0
    active = 0
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
        existing = state.get(key)
        if existing is not None and existing.is_alive():
            active += 1
            continue
        state[key] = start_client(
            remote_base_url=endpoint,
            local_group_id=local_group_id,
            remote_group_id=remote_group_id,
            remote_peer_id=remote_peer_id,
            stop=stop_event,
        )
        started += 1
        active += 1
    return {"started": started, "active": active}


def start_federation_session_manager_thread(
    *,
    home: Path,
    stop_event: threading.Event,
    interval_seconds: float = 10.0,
) -> threading.Thread:
    state: Dict[str, threading.Thread] = {}

    def run() -> None:
        interval = max(1.0, float(interval_seconds or 10.0))
        while not stop_event.is_set():
            try:
                tick_federation_session_clients(home=home, stop_event=stop_event, state=state)
            except Exception:
                pass
            stop_event.wait(interval)

    thread = threading.Thread(target=run, name="cccc-federation-ws-manager", daemon=True)
    thread.start()
    return thread
