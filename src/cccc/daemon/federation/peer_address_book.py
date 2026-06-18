"""Compatibility exports for the federation peer address book."""

from __future__ import annotations

from ...kernel.federation.peer_addresses import (
    address_book_path,
    load_address_book,
    record_peer_addresses,
    resolve_peer_multiaddrs,
)

__all__ = [
    "address_book_path",
    "load_address_book",
    "record_peer_addresses",
    "resolve_peer_multiaddrs",
]
