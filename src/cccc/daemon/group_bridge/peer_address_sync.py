"""Helpers for refreshing runtime group_bridge peer addresses."""

from __future__ import annotations

from pathlib import Path
from typing import Any, Dict, Iterable, Optional

from ...kernel.group_bridge.peer_addresses import record_peer_addresses


def sync_group_bridge_peer_multiaddrs(
    *,
    group_id: str,
    remote_group_id: str,
    remote_peer_id: str,
    multiaddrs: Iterable[str],
    home: Optional[Path] = None,
) -> Dict[str, Any]:
    """Persist fresh runtime addresses for an already-trusted remote peer."""
    gid = str(group_id or "").strip()
    remote_gid = str(remote_group_id or "").strip()
    peer_id = str(remote_peer_id or "").strip()
    addrs = [str(addr or "").strip() for addr in (multiaddrs or []) if str(addr or "").strip()]
    if not gid or not remote_gid or not peer_id or not addrs:
        return {"updated": False, "trust_updates": 0, "registration_updates": 0}

    record_peer_addresses(peer_id, addrs, remote_group_id=remote_gid, home=home)
    return {
        "updated": True,
        "trust_updates": 0,
        "registration_updates": 0,
    }
