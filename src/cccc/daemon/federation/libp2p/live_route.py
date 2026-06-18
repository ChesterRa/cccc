"""Live Stage C1 direct-route bootstrap helpers.

This module is intentionally operational glue: it starts local direct nodes,
then uses the existing pairing approval path to persist active libp2p
registration/trust records with real PeerIDs and listen multiaddrs.
"""

from __future__ import annotations

import os
from contextlib import contextmanager
from pathlib import Path
from typing import Any, Dict, Iterator

from ....kernel.federation.pairing import approve_pairing_request, create_pairing_invite, create_pairing_request
from .sidecar import Libp2pNode


@contextmanager
def _home_env(home: Path) -> Iterator[None]:
    old_home = os.environ.get("CCCC_HOME")
    os.environ["CCCC_HOME"] = str(home)
    try:
        yield
    finally:
        if old_home is None:
            os.environ.pop("CCCC_HOME", None)
        else:
            os.environ["CCCC_HOME"] = old_home


def ensure_direct_pairing_route(
    *,
    local_home: Path,
    local_group_id: str,
    remote_home: Path,
    remote_group_id: str,
    local_group_title: str = "",
    remote_group_title: str = "",
    approver_user_id: str = "",
) -> Dict[str, Any]:
    """Start two C1 nodes and persist bidirectional approved libp2p routes.

    The returned ``local_node`` and ``remote_node`` are live resources owned by
    the caller; stop them after the现场 send/verification completes.
    """
    local_node = Libp2pNode(home=Path(local_home), listen_multiaddr="/ip4/127.0.0.1/tcp/0")
    remote_node = Libp2pNode(home=Path(remote_home), listen_multiaddr="/ip4/127.0.0.1/tcp/0")
    local_node.start()
    remote_node.start()
    try:
        local_multiaddrs = local_node.multiaddrs()
        remote_multiaddrs = remote_node.multiaddrs()
        if not local_multiaddrs:
            raise RuntimeError("local libp2p node did not expose a listen multiaddr")
        if not remote_multiaddrs:
            raise RuntimeError("remote libp2p node did not expose a listen multiaddr")

        remote_registration = _approve_route(
            issuer_home=Path(remote_home),
            issuer_group_id=remote_group_id,
            issuer_group_title=remote_group_title,
            requester_group_id=local_group_id,
            requester_group_title=local_group_title,
            requester_peer_id=local_node.identity.peer_id,
            requester_multiaddrs=local_multiaddrs,
            approver_user_id=approver_user_id,
        )
        local_registration = _approve_route(
            issuer_home=Path(local_home),
            issuer_group_id=local_group_id,
            issuer_group_title=local_group_title,
            requester_group_id=remote_group_id,
            requester_group_title=remote_group_title,
            requester_peer_id=remote_node.identity.peer_id,
            requester_multiaddrs=remote_multiaddrs,
            approver_user_id=approver_user_id,
        )
        return {
            "local_node": local_node,
            "remote_node": remote_node,
            "local_identity": local_node.identity.public_dict(),
            "remote_identity": remote_node.identity.public_dict(),
            "local_multiaddrs": local_multiaddrs,
            "remote_multiaddrs": remote_multiaddrs,
            "local_registration": local_registration["registration"],
            "local_trust": local_registration["trust"],
            "remote_registration": remote_registration["registration"],
            "remote_trust": remote_registration["trust"],
        }
    except Exception:
        local_node.stop()
        remote_node.stop()
        raise


def _approve_route(
    *,
    issuer_home: Path,
    issuer_group_id: str,
    issuer_group_title: str,
    requester_group_id: str,
    requester_group_title: str,
    requester_peer_id: str,
    requester_multiaddrs: list[str],
    approver_user_id: str,
) -> Dict[str, Any]:
    with _home_env(issuer_home):
        invite = create_pairing_invite(
            group_id=issuer_group_id,
            remote_group_id=requester_group_id,
            remote_peer_id=requester_peer_id,
            multiaddrs=requester_multiaddrs,
        )
        request = create_pairing_request(
            invite["pairing_code"],
            requester_group_id=requester_group_id,
            requester_group_title=requester_group_title or requester_group_id,
            requester_peer_id=requester_peer_id,
            requester_multiaddrs=requester_multiaddrs,
        )
        approved = approve_pairing_request(request["request_id"], approver_user_id=approver_user_id)
    registration = approved.get("registration") if isinstance(approved, dict) else None
    if not isinstance(registration, dict) or str(registration.get("transport") or "") != "libp2p_cccc":
        raise RuntimeError(f"failed to create libp2p registration for {issuer_group_title or issuer_group_id}")
    if not registration.get("multiaddrs"):
        raise RuntimeError(f"created libp2p registration without multiaddrs for {issuer_group_title or issuer_group_id}")
    return approved
