"""Utilities for closing proxied stream writers from WebSocket routes."""

from __future__ import annotations

from typing import Any


async def close_stream_writer(writer: Any) -> None:
    try:
        writer.close()
    except Exception:
        return
    try:
        await writer.wait_closed()
    except (BrokenPipeError, ConnectionResetError):
        return
    except OSError:
        return
    except Exception:
        return
