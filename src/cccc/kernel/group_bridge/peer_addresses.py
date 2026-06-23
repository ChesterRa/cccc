"""Runtime peer address book for Group Bridge transports.

Registrations and trusts store stable peer identity. This module stores
runtime-discovered dial addresses, which may change whenever a sidecar restarts.
"""

from __future__ import annotations

from pathlib import Path
from typing import Any, Dict, Iterable, Optional

from ...paths import ensure_home
from ...util.fs import atomic_write_json, read_json
from ...util.time import utc_now_iso


def address_book_path(*, home: Optional[Path] = None) -> Path:
    base = Path(home) if home is not None else ensure_home()
    return base / "state" / "group_bridge" / "peer_address_book.json"


def load_address_book(*, home: Optional[Path] = None) -> Dict[str, Dict[str, Any]]:
    doc = read_json(address_book_path(home=home))
    peers = doc.get("peers") if isinstance(doc, dict) else None
    if not isinstance(peers, dict):
        return {}
    return {str(peer_id): dict(entry) for peer_id, entry in peers.items() if isinstance(entry, dict)}


def record_peer_addresses(
    peer_id: str,
    multiaddrs: Iterable[str],
    *,
    remote_group_id: str = "",
    home: Optional[Path] = None,
) -> Dict[str, Any]:
    pid = str(peer_id or "").strip()
    if not pid:
        raise ValueError("peer_id is required")
    addrs = [str(addr or "").strip() for addr in multiaddrs if str(addr or "").strip()]
    book = load_address_book(home=home)
    entry = {
        "peer_id": pid,
        "remote_group_id": str(remote_group_id or "").strip(),
        "multiaddrs": addrs,
        "updated_at": utc_now_iso(),
    }
    book[pid] = entry
    atomic_write_json(address_book_path(home=home), {"peers": book}, indent=2)
    return dict(entry)


def resolve_peer_multiaddrs(
    peer_id: str,
    *,
    remote_group_id: str = "",
    home: Optional[Path] = None,
) -> tuple[str, ...]:
    pid = str(peer_id or "").strip()
    if not pid:
        return ()
    entry = load_address_book(home=home).get(pid)
    if not isinstance(entry, dict):
        return ()
    expected_gid = str(remote_group_id or "").strip()
    stored_gid = str(entry.get("remote_group_id") or "").strip()
    if expected_gid and stored_gid and stored_gid != expected_gid:
        return ()
    return tuple(str(addr or "").strip() for addr in (entry.get("multiaddrs") or []) if str(addr or "").strip())
