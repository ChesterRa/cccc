"""Stage C1 direct libp2p runtime boundary.

This package keeps the network runtime isolated from daemon entrypoints. C1 is
limited to localhost/direct multiaddr streams and the CCCC remote-send protocol.
It does not implement DHT, relay, hole punching, pubsub, or HTTP fallback.
"""

from .identity import Libp2pIdentity, get_libp2p_identity
from .sidecar import Libp2pNode

__all__ = ["Libp2pIdentity", "Libp2pNode", "get_libp2p_identity"]
