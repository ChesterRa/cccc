"""MCP handler functions for IM (instant messaging) tools."""

from __future__ import annotations

from typing import Any, Dict

from ..common import MCPError, _call_daemon_or_raise


def im_bind(*, group_id: str, key: str) -> Dict[str, Any]:
    """Bind an IM chat using a one-time key from /subscribe."""
    k = str(key or "").strip()
    if not k:
        raise MCPError(code="missing_key", message="key is required")
    return _call_daemon_or_raise({
        "op": "im_bind_chat",
        "args": {"group_id": group_id, "key": k},
    })
