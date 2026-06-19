"""WebSocket endpoint protocol handling for federation sessions."""

from __future__ import annotations

import asyncio
import logging
from typing import Any, Dict

from fastapi import WebSocket, WebSocketDisconnect

from .receiver import receive_remote_send
from .ws_auth import authenticated_session_peer_id
from .ws_peer import AsyncFederationWsPeer
from .ws_session import (
    FederationWsSession,
    authorize_session,
    register_session,
    unregister_session,
)

logger = logging.getLogger("cccc.daemon.federation.ws")


async def handle_federation_session_websocket(websocket: WebSocket) -> None:
    client = getattr(websocket, "client", None)
    client_host = getattr(client, "host", "") or ""
    client_port = getattr(client, "port", "") or ""
    await websocket.accept()
    try:
        hello = await websocket.receive_json()
    except Exception as exc:
        logger.warning(
            "federation session invalid hello client=%s:%s error=%s",
            client_host,
            client_port,
            exc,
        )
        await websocket.send_json({"ok": False, "error": {"code": "invalid_hello", "message": "invalid federation session hello"}})
        await websocket.close(code=1008)
        return

    target_group_id = str((hello or {}).get("target_group_id") or "").strip()
    src_group_id = str((hello or {}).get("src_group_id") or "").strip()
    remote_peer_id = str((hello or {}).get("remote_peer_id") or "").strip()
    authenticated_peer_id = authenticated_session_peer_id(hello if isinstance(hello, dict) else {})
    logger.info(
        "federation session hello client=%s:%s target_group=%s src_group=%s remote_peer=%s authenticated_peer=%s",
        client_host,
        client_port,
        target_group_id,
        src_group_id,
        remote_peer_id,
        authenticated_peer_id,
    )
    if not authenticated_peer_id or authenticated_peer_id != remote_peer_id:
        logger.warning(
            "federation session rejected reason=invalid_signature client=%s:%s target_group=%s src_group=%s remote_peer=%s authenticated_peer=%s",
            client_host,
            client_port,
            target_group_id,
            src_group_id,
            remote_peer_id,
            authenticated_peer_id,
        )
        await websocket.send_json({"ok": False, "error": {"code": "unauthorized_peer", "message": "remote peer signature is invalid"}})
        await websocket.close(code=1008)
        return
    if not authorize_session(target_group_id=target_group_id, src_group_id=src_group_id, remote_peer_id=remote_peer_id):
        logger.warning(
            "federation session rejected reason=untrusted_peer client=%s:%s target_group=%s src_group=%s remote_peer=%s",
            client_host,
            client_port,
            target_group_id,
            src_group_id,
            remote_peer_id,
        )
        await websocket.send_json({"ok": False, "error": {"code": "unauthorized_peer", "message": "remote peer is not trusted for this group"}})
        await websocket.close(code=1008)
        return

    peer = AsyncFederationWsPeer(websocket.send_json)
    session = FederationWsSession(
        target_group_id=target_group_id,
        src_group_id=src_group_id,
        remote_peer_id=remote_peer_id,
        send_request=peer.send_request,
        loop=asyncio.get_running_loop(),
    )
    await register_session(session)
    logger.info(
        "federation session registered client=%s:%s target_group=%s src_group=%s remote_peer=%s",
        client_host,
        client_port,
        target_group_id,
        src_group_id,
        remote_peer_id,
    )
    await websocket.send_json({"ok": True, "type": "ready"})
    try:
        while True:
            frame = await websocket.receive_json()
            frame_type = str((frame or {}).get("type") or "").strip()
            if frame_type == "response":
                peer.receive_response(frame)
                continue
            if frame_type == "ping":
                await websocket.send_json({"type": "pong"})
                continue
            if frame_type != "request":
                continue
            request_id = str((frame or {}).get("request_id") or "").strip()
            result = handle_federation_session_request(
                frame,
                target_group_id=target_group_id,
                src_group_id=src_group_id,
                remote_peer_id=remote_peer_id,
            )
            await websocket.send_json({"type": "response", "response_to": request_id, "result": result})
    except WebSocketDisconnect as exc:
        logger.info(
            "federation session disconnected client=%s:%s target_group=%s src_group=%s remote_peer=%s code=%s reason=%s",
            client_host,
            client_port,
            target_group_id,
            src_group_id,
            remote_peer_id,
            getattr(exc, "code", ""),
            getattr(exc, "reason", ""),
        )
    except Exception:
        logger.exception(
            "federation session failed client=%s:%s target_group=%s src_group=%s remote_peer=%s",
            client_host,
            client_port,
            target_group_id,
            src_group_id,
            remote_peer_id,
        )
        raise
    finally:
        await unregister_session(session)
        logger.info(
            "federation session unregistered client=%s:%s target_group=%s src_group=%s remote_peer=%s",
            client_host,
            client_port,
            target_group_id,
            src_group_id,
            remote_peer_id,
        )


def handle_federation_session_request(
    frame: Dict[str, Any],
    *,
    target_group_id: str,
    src_group_id: str,
    remote_peer_id: str,
) -> Dict[str, Any]:
    op = str((frame or {}).get("op") or "").strip()
    if op != "remote_send":
        return {"ok": False, "error": {"code": "unsupported_op", "message": f"unsupported federation session op: {op or '(empty)'}"}}
    return receive_remote_send(
        target_group_id=str((frame or {}).get("target_group_id") or target_group_id),
        src_group_id=str((frame or {}).get("src_group_id") or src_group_id),
        remote_peer_id=remote_peer_id,
        payload=dict((frame or {}).get("payload") or {}) if isinstance((frame or {}).get("payload"), dict) else {},
        idempotency_key=str((frame or {}).get("idempotency_key") or ""),
    )
