"""In-memory WebSocket federation sessions.

This is the small transport primitive needed for asymmetric networks: if a
remote peer can open a long-lived WebSocket to this instance, this instance can
send federation requests back over that already-established connection without
dialing the remote peer's private address.
"""

from __future__ import annotations

import asyncio
import concurrent.futures
import secrets
import threading
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Awaitable, Callable, Dict, Optional

from ...kernel.federation.pairing import list_trusts

JsonRequestSender = Callable[[Dict[str, Any], float], Awaitable[Dict[str, Any]]]


@dataclass(frozen=True)
class FederationWsSession:
    target_group_id: str
    src_group_id: str
    remote_peer_id: str
    send_request: JsonRequestSender
    loop: Optional[asyncio.AbstractEventLoop] = None


_LOCK = asyncio.Lock()
_SESSIONS: dict[tuple[str, str, str], FederationWsSession] = {}


def session_key(*, target_group_id: str, src_group_id: str, remote_peer_id: str) -> tuple[str, str, str]:
    return (
        str(target_group_id or "").strip(),
        str(src_group_id or "").strip(),
        str(remote_peer_id or "").strip(),
    )


def active_session_count() -> int:
    return len(_SESSIONS)


async def register_session(session: FederationWsSession) -> None:
    key = session_key(
        target_group_id=session.target_group_id,
        src_group_id=session.src_group_id,
        remote_peer_id=session.remote_peer_id,
    )
    async with _LOCK:
        _SESSIONS[key] = session


async def unregister_session(session: FederationWsSession) -> None:
    key = session_key(
        target_group_id=session.target_group_id,
        src_group_id=session.src_group_id,
        remote_peer_id=session.remote_peer_id,
    )
    async with _LOCK:
        if _SESSIONS.get(key) is session:
            _SESSIONS.pop(key, None)


def clear_sessions() -> None:
    _SESSIONS.clear()


def get_session(*, target_group_id: str, src_group_id: str, remote_peer_id: str) -> Optional[FederationWsSession]:
    return _SESSIONS.get(
        session_key(
            target_group_id=target_group_id,
            src_group_id=src_group_id,
            remote_peer_id=remote_peer_id,
        )
    )


def authorize_session(
    *,
    target_group_id: str,
    src_group_id: str,
    remote_peer_id: str,
    home: Optional[Path] = None,
) -> bool:
    target_gid, src_gid, peer_id = session_key(
        target_group_id=target_group_id,
        src_group_id=src_group_id,
        remote_peer_id=remote_peer_id,
    )
    if not target_gid or not src_gid or not peer_id:
        return False
    for trust in list_trusts(group_id=target_gid, home=home):
        if str(trust.get("status") or "") != "active":
            continue
        if str(trust.get("remote_group_id") or "").strip() != src_gid:
            continue
        if str(trust.get("remote_peer_id") or "").strip() != peer_id:
            continue
        return True
    return False


async def send_via_session(
    *,
    target_group_id: str,
    src_group_id: str,
    remote_peer_id: str,
    request: Dict[str, Any],
    timeout: float = 5.0,
) -> Dict[str, Any]:
    session = get_session(target_group_id=target_group_id, src_group_id=src_group_id, remote_peer_id=remote_peer_id)
    if session is None:
        return {"ok": False, "error": {"code": "peer_session_unavailable", "message": "no active federation WebSocket session"}}
    return await session.send_request(dict(request or {}), timeout)


def next_request_id() -> str:
    return secrets.token_urlsafe(12)


def send_via_session_sync(
    *,
    target_group_id: str,
    src_group_id: str,
    remote_peer_id: str,
    request: Dict[str, Any],
    timeout: float = 5.0,
) -> Dict[str, Any]:
    session = get_session(target_group_id=target_group_id, src_group_id=src_group_id, remote_peer_id=remote_peer_id)
    if session is None:
        return {"ok": False, "error": {"code": "peer_session_unavailable", "message": "no active federation WebSocket session"}}
    if session.loop is not None:
        try:
            future = asyncio.run_coroutine_threadsafe(session.send_request(dict(request or {}), timeout), session.loop)
            return dict(future.result(timeout=max(0.1, float(timeout or 0.1)) + 0.5) or {})
        except concurrent.futures.TimeoutError:
            return {"ok": False, "error": {"code": "peer_session_timeout", "message": "federation WebSocket session timed out"}}
        except BaseException as exc:
            return {"ok": False, "error": {"code": "peer_session_failed", "message": str(exc)}}

    result: Dict[str, Any] = {}
    error: list[BaseException] = []

    def run() -> None:
        try:
            result.update(asyncio.run(session.send_request(dict(request or {}), timeout)))
        except BaseException as exc:
            error.append(exc)

    thread = threading.Thread(target=run, name="cccc-federation-ws-send", daemon=True)
    thread.start()
    thread.join(timeout + 0.5)
    if thread.is_alive():
        return {"ok": False, "error": {"code": "peer_session_timeout", "message": "federation WebSocket session timed out"}}
    if error:
        return {"ok": False, "error": {"code": "peer_session_failed", "message": str(error[0])}}
    return result or {"ok": False, "error": {"code": "peer_session_failed", "message": "federation WebSocket session returned no response"}}
