"""Client/supervisor seam used by the ``libp2p_cccc`` transport."""

from __future__ import annotations

import threading
import os
from pathlib import Path
from typing import Any, Dict, Optional

from ....paths import ensure_home
from .sidecar import Libp2pNode

_LOCK = threading.RLock()
_NODES: dict[Path, Libp2pNode] = {}


def _default_listen_multiaddr() -> str:
    return str(os.environ.get("CCCC_LIBP2P_LISTEN_MULTIADDR") or "").strip() or "/ip4/127.0.0.1/tcp/0"


def _default_advertise_host() -> str:
    return str(os.environ.get("CCCC_LIBP2P_ADVERTISE_HOST") or "").strip()


def get_default_node(*, home: Optional[Path] = None) -> Libp2pNode:
    base = Path(home) if home is not None else ensure_home()
    with _LOCK:
        node = _NODES.get(base)
        if node is None:
            node = Libp2pNode(
                home=base,
                listen_multiaddr=_default_listen_multiaddr(),
                advertise_host=_default_advertise_host(),
            )
            node.start()
            _NODES[base] = node
        return node


def register_node(node: Libp2pNode) -> None:
    base = Path(node.home) if node.home is not None else ensure_home()
    with _LOCK:
        _NODES[base] = node


def unregister_node(node: Libp2pNode) -> None:
    base = Path(node.home) if node.home is not None else ensure_home()
    with _LOCK:
        if _NODES.get(base) is node:
            _NODES.pop(base, None)


def stop_default_node(*, home: Optional[Path] = None) -> None:
    base = Path(home) if home is not None else ensure_home()
    with _LOCK:
        node = _NODES.pop(base, None)
    if node is not None:
        node.stop()


def default_libp2p_send(request: Any, *, home: Optional[Path] = None) -> Dict[str, Any]:
    node = get_default_node(home=home)
    multiaddrs = tuple(getattr(request, "multiaddrs", ()) or ())
    if not multiaddrs:
        return {"ok": False, "error": {"code": "missing_multiaddr", "message": "remote multiaddr is required"}}
    payload = getattr(request, "payload", None)
    if hasattr(payload, "model_dump"):
        payload_doc = payload.model_dump()
    elif isinstance(payload, dict):
        payload_doc = dict(payload)
    else:
        payload_doc = {}
    return node.send_remote(
        multiaddr=str(multiaddrs[0]),
        request={
            "src_group_id": str(getattr(request, "src_group_id", "") or ""),
            "remote_group_id": str(getattr(request, "remote_group_id", "") or ""),
            "remote_peer_id": str(getattr(request, "remote_peer_id", "") or ""),
            "idempotency_key": str(getattr(request, "idempotency_key", "") or ""),
            "payload": payload_doc,
        },
    )
