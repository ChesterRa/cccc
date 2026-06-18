"""Federation route lookup helpers.

This module resolves local federation trust state into sendable routes. It
does not perform delivery and does not know about MCP or web transport shapes.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import Optional

from ...kernel.federation.pairing import list_trusts


@dataclass(frozen=True)
class FederationRemoteGroupRoute:
    group_id: str
    remote_group_id: str
    registration_id: str
    trust_id: str = ""
    remote_group_title: str = ""


def resolve_remote_group_route(*, group_id: str, remote_group_id: str) -> Optional[FederationRemoteGroupRoute]:
    gid = str(group_id or "").strip()
    remote_gid = str(remote_group_id or "").strip()
    if not gid or not remote_gid:
        return None
    for trust in list_trusts(group_id=gid):
        if str(trust.get("status") or "") != "active":
            continue
        if str(trust.get("remote_group_id") or "").strip() != remote_gid:
            continue
        registration_id = str(trust.get("registration_id") or "").strip()
        if not registration_id:
            continue
        return FederationRemoteGroupRoute(
            group_id=gid,
            remote_group_id=remote_gid,
            registration_id=registration_id,
            trust_id=str(trust.get("trust_id") or "").strip(),
            remote_group_title=str(trust.get("remote_group_title") or "").strip(),
        )
    return None


def resolve_remote_group_route_token(*, group_id: str, token: str) -> Optional[FederationRemoteGroupRoute]:
    gid = str(group_id or "").strip()
    normalized = _normalize_route_token(token)
    if not gid or not normalized:
        return None
    for trust in list_trusts(group_id=gid):
        if str(trust.get("status") or "") != "active":
            continue
        registration_id = str(trust.get("registration_id") or "").strip()
        remote_group_id = str(trust.get("remote_group_id") or "").strip()
        if not registration_id or not remote_group_id:
            continue
        title = str(trust.get("remote_group_title") or "").strip()
        if normalized not in {_normalize_route_token(remote_group_id), _normalize_route_token(title)}:
            continue
        return FederationRemoteGroupRoute(
            group_id=gid,
            remote_group_id=remote_group_id,
            registration_id=registration_id,
            trust_id=str(trust.get("trust_id") or "").strip(),
            remote_group_title=title,
        )
    return None


def _normalize_route_token(value: object) -> str:
    token = str(value or "").strip()
    while token.startswith("#"):
        token = token[1:].strip()
    return token.lower()
