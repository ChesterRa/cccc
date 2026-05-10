from __future__ import annotations

import asyncio
import json
from pathlib import Path
from typing import Any

from fastapi import WebSocket

from ....daemon.server import get_daemon_endpoint


async def open_daemon_stream(*, home: Path, limit: int):
    ep = get_daemon_endpoint()
    transport = str(ep.get("transport") or "").strip().lower()
    if transport == "tcp":
        host = str(ep.get("host") or "127.0.0.1").strip() or "127.0.0.1"
        port = int(ep.get("port") or 0)
        return await asyncio.open_connection(host, port, limit=limit)
    sock_path = home / "daemon" / "ccccd.sock"
    path = str(ep.get("path") or sock_path)
    return await asyncio.open_unix_connection(path, limit=limit)


async def send_daemon_attach_request(reader: asyncio.StreamReader, writer: asyncio.StreamWriter, *, op: str, args: dict[str, Any]) -> dict[str, Any]:
    req = {"op": str(op or "").strip(), "args": dict(args or {})}
    writer.write((json.dumps(req, ensure_ascii=False) + "\n").encode("utf-8"))
    await writer.drain()
    line = await reader.readline()
    try:
        resp = json.loads(line.decode("utf-8", errors="replace"))
    except Exception:
        resp = {}
    return resp if isinstance(resp, dict) else {}


async def proxy_daemon_raw_stream_to_websocket(
    websocket: WebSocket,
    reader: asyncio.StreamReader,
    writer: asyncio.StreamWriter,
) -> None:
    async def _pump_out() -> None:
        while True:
            chunk = await reader.read(65536)
            if not chunk:
                break
            await websocket.send_bytes(chunk)

    async def _pump_in() -> None:
        while True:
            msg = await websocket.receive()
            msg_type = str(msg.get("type") or "")
            if msg_type == "websocket.disconnect":
                break
            data = msg.get("bytes")
            if data is None:
                text = msg.get("text")
                data = str(text or "").encode("utf-8") if text is not None else b""
            if not data:
                continue
            writer.write(bytes(data))
            await writer.drain()

    out_task = asyncio.create_task(_pump_out())
    in_task = asyncio.create_task(_pump_in())
    try:
        done, pending = await asyncio.wait({out_task, in_task}, return_when=asyncio.FIRST_COMPLETED)
        for task in done:
            try:
                _ = task.result()
            except Exception:
                pass
        for task in pending:
            task.cancel()
        await asyncio.gather(*pending, return_exceptions=True)
    finally:
        try:
            writer.close()
            await writer.wait_closed()
        except Exception:
            pass
