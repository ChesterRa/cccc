"""Minimal supervisor helpers for the Stage C1 sidecar."""

from __future__ import annotations

import os
import signal
import time
from pathlib import Path
from typing import Any, Dict, Optional

from ....paths import ensure_home
from ....kernel.federation.pairing import list_trusts
from ....util.fs import atomic_write_json, read_json
from ....util.time import utc_now_iso
from ....kernel.federation.peer_addresses import record_peer_addresses, resolve_peer_multiaddrs
from .sidecar import Libp2pNode


def sidecar_status_path(*, home: Optional[Path] = None) -> Path:
    base = Path(home) if home is not None else ensure_home()
    return base / "daemon" / "libp2p_sidecar.json"


def read_sidecar_status(*, home: Optional[Path] = None) -> Dict[str, Any]:
    status = read_json(sidecar_status_path(home=home))
    return status if isinstance(status, dict) else {}


def _default_listen_multiaddr() -> str:
    return str(os.environ.get("CCCC_LIBP2P_LISTEN_MULTIADDR") or "").strip() or "/ip4/127.0.0.1/tcp/0"


def _default_advertise_host() -> str:
    return str(os.environ.get("CCCC_LIBP2P_ADVERTISE_HOST") or "").strip()


def start_sidecar(
    *,
    home: Optional[Path] = None,
    listen_multiaddr: Optional[str] = None,
    advertise_host: Optional[str] = None,
) -> Libp2pNode:
    node = Libp2pNode(
        home=home,
        listen_multiaddr=str(listen_multiaddr or "").strip() or _default_listen_multiaddr(),
        advertise_host=_default_advertise_host() if advertise_host is None else str(advertise_host or "").strip(),
    )
    node.start()
    _write_status(node)
    announce_sidecar_addresses(node)
    return node


def run_forever(
    *,
    home: Optional[Path] = None,
    listen_multiaddr: Optional[str] = None,
    advertise_host: Optional[str] = None,
) -> None:
    node = start_sidecar(home=home, listen_multiaddr=listen_multiaddr, advertise_host=advertise_host)
    stopping = False

    def _stop(_signum, _frame):  # type: ignore[no-untyped-def]
        nonlocal stopping
        stopping = True
        node.stop()
        _write_status(node, status="stopped")

    old_int = signal.signal(signal.SIGINT, _stop)
    old_term = signal.signal(signal.SIGTERM, _stop)
    try:
        while not stopping:
            _write_status(node)
            time.sleep(5)
    finally:
        signal.signal(signal.SIGINT, old_int)
        signal.signal(signal.SIGTERM, old_term)
        node.stop()
        _write_status(node, status="stopped")


def _write_status(node: Libp2pNode, *, status: str = "running") -> None:
    addrs = node.multiaddrs()
    if status == "running":
        try:
            record_peer_addresses(node.identity.peer_id, addrs, home=node.home)
        except Exception:
            pass
    atomic_write_json(
        sidecar_status_path(home=node.home),
        {
            "status": status,
            "pid": os.getpid(),
            "peer_id": node.identity.peer_id,
            "multiaddrs": addrs,
            "updated_at": utc_now_iso(),
        },
    )


def announce_sidecar_addresses(node: Libp2pNode) -> Dict[str, Any]:
    addrs = tuple(node.multiaddrs())
    if not addrs:
        return {"attempted": 0, "sent": 0, "failed": 0}
    attempted = 0
    sent = 0
    failed = 0
    retried = 0
    trust_updates = 0
    registration_updates = 0
    for trust in list_trusts(home=node.home):
        if str(trust.get("status") or "") != "active":
            continue
        if str(trust.get("transport") or "") != "libp2p_cccc":
            continue
        src_group_id = str(trust.get("group_id") or "").strip()
        remote_group_id = str(trust.get("remote_group_id") or "").strip()
        remote_peer_id = str(trust.get("remote_peer_id") or "").strip()
        if not src_group_id or not remote_group_id or not remote_peer_id:
            continue
        target_addrs = tuple(str(addr or "").strip() for addr in (trust.get("multiaddrs") or []) if str(addr or "").strip())
        if not target_addrs:
            target_addrs = resolve_peer_multiaddrs(remote_peer_id, remote_group_id=remote_group_id, home=node.home)
        if not target_addrs:
            continue
        attempted += 1
        try:
            result = node.announce_addresses(
                multiaddr=target_addrs[0],
                request={
                    "src_group_id": src_group_id,
                    "remote_group_id": remote_group_id,
                    "remote_peer_id": remote_peer_id,
                    "multiaddrs": list(addrs),
                },
            )
        except Exception:
            result = {"ok": False}
        if result.get("ok"):
            sent += 1
            retried += int(result.get("retried") or 0)
            trust_updates += int(result.get("trust_updates") or 0)
            registration_updates += int(result.get("registration_updates") or 0)
        else:
            failed += 1
    return {
        "attempted": attempted,
        "sent": sent,
        "failed": failed,
        "retried": retried,
        "trust_updates": trust_updates,
        "registration_updates": registration_updates,
    }


def main() -> None:
    run_forever()


if __name__ == "__main__":
    main()
