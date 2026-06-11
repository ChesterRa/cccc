"""Transport registry assembly.

Importing this package registers the built-in MVP transports. Additional
transports (registry_hub, im_bridge, mcp_server) plug in here later by the same
``register_transport`` call without touching the dispatch layer.
"""

from __future__ import annotations

from .base import (
    RemoteMessageEnvelope,
    RemoteSendResult,
    RemoteSendTransport,
    RemoteTarget,
    UnknownTransportError,
    available_transports,
    get_transport,
    register_transport,
)
from .peer_cccc_http import PeerCcccHttpTransport

register_transport(PeerCcccHttpTransport())

__all__ = [
    "RemoteMessageEnvelope",
    "RemoteSendResult",
    "RemoteSendTransport",
    "RemoteTarget",
    "UnknownTransportError",
    "available_transports",
    "get_transport",
    "register_transport",
    "PeerCcccHttpTransport",
]
