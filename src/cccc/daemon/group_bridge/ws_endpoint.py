"""Shared Group Bridge session request handling for the retiring Python client."""

from __future__ import annotations

import logging
import os
from typing import Any, Dict

from ...contracts.v1.group_bridge import GROUP_BRIDGE_MESSAGE_CONTRACT_VERSION
from .cancellation import receive_remote_reply_request_cancel
from .receiver import receive_remote_send

logger = logging.getLogger("cccc.daemon.group_bridge.ws")


def handle_group_bridge_session_request(
    frame: Dict[str, Any],
    *,
    target_group_id: str,
    src_group_id: str,
    remote_peer_id: str,
) -> Dict[str, Any]:
    op = str((frame or {}).get("op") or "").strip()
    if (frame or {}).get("message_contract_version") != GROUP_BRIDGE_MESSAGE_CONTRACT_VERSION:
        return {
            "ok": False,
            "error": {
                "code": "contract_version_mismatch",
                "message": "Group Bridge message contract version does not match",
            },
        }
    if op not in {"remote_send", "reply_request_cancel"}:
        return {"ok": False, "error": {"code": "unsupported_op", "message": f"unsupported Group Bridge session op: {op or '(empty)'}"}}
    payload = dict((frame or {}).get("payload") or {}) if isinstance((frame or {}).get("payload"), dict) else {}
    if op == "remote_send":
        payload["message_contract_version"] = GROUP_BRIDGE_MESSAGE_CONTRACT_VERSION
    args = {
        "target_group_id": str((frame or {}).get("target_group_id") or target_group_id),
        "src_group_id": str((frame or {}).get("src_group_id") or src_group_id),
        "remote_peer_id": remote_peer_id,
        "payload": payload,
        "idempotency_key": str((frame or {}).get("idempotency_key") or ""),
    }
    daemon_op = (
        "group_bridge_receive_reply_request_cancel"
        if op == "reply_request_cancel"
        else "group_bridge_receive_remote_send"
    )
    if os.environ.get("CCCC_WEB_SUPERVISED"):
        try:
            from ..server import call_daemon

            resp = call_daemon({"op": daemon_op, "args": args})
        except Exception as exc:
            logger.exception("Group Bridge session daemon receive failed")
            return {"ok": False, "error": {"code": "daemon_receive_failed", "message": str(exc)}}
        if resp.get("ok"):
            result = resp.get("result") if isinstance(resp.get("result"), dict) else {}
            return result if result else {"ok": True}
        error = resp.get("error") if isinstance(resp.get("error"), dict) else {}
        return {
            "ok": False,
            "error": {
                "code": str(error.get("code") or "daemon_receive_failed"),
                "message": str(error.get("message") or "daemon receive failed"),
            },
        }
    if op == "reply_request_cancel":
        return receive_remote_reply_request_cancel(
            target_group_id=str(args.get("target_group_id") or ""),
            src_group_id=str(args.get("src_group_id") or ""),
            remote_peer_id=str(args.get("remote_peer_id") or ""),
            payload=dict(args.get("payload") or {}) if isinstance(args.get("payload"), dict) else {},
        )
    return receive_remote_send(
        target_group_id=str(args.get("target_group_id") or ""),
        src_group_id=str(args.get("src_group_id") or ""),
        remote_peer_id=str(args.get("remote_peer_id") or ""),
        payload=dict(args.get("payload") or {}) if isinstance(args.get("payload"), dict) else {},
        idempotency_key=str(args.get("idempotency_key") or ""),
    )
