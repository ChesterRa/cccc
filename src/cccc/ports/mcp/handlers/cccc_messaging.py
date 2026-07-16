"""MCP handler functions for messaging tools (message_send/reply, blob_path, file_send)."""

from __future__ import annotations

import mimetypes
import re
import uuid
from pathlib import Path
from typing import Any, Dict, List, Optional

from ....daemon.group_bridge.route_lookup import resolve_remote_group_route
from ....kernel.actors import find_actor
from ....kernel.blobs import resolve_blob_attachment_path, store_blob_bytes
from ....kernel.group import load_group
from ....kernel.peer_insight import (
    POST_MESSAGE_NUDGE,
    PeerRecipientError,
    normalized_insight_or_error,
    peer_insight_required_details,
    preflight_local_peer_audience,
    remote_recipients_include_peer,
)
from ....kernel.recipient_syntax import (
    CROSS_GROUP_HASH_RECIPIENT_MESSAGE,
    cross_group_recipient_tokens_or_default,
    group_not_found_with_resolution_hint,
    has_hash_recipient_token,
    normalize_recipient_tokens,
)
from ....util.conv import coerce_bool
from ..common import MCPError, _call_daemon_or_raise

_MAX_BLOB_READ_BYTES = 1_000_000
_DEFAULT_BLOB_READ_BYTES = 200_000
_FILENAME_MIME_FALLBACKS = {
    ".md": "text/markdown",
    ".markdown": "text/markdown",
}


def _guess_mime_type(filename: str) -> str:
    mt, _ = mimetypes.guess_type(filename)
    if mt:
        return str(mt)
    suffix = Path(str(filename or "")).suffix.lower()
    return _FILENAME_MIME_FALLBACKS.get(suffix, "")


def _mcp_error(code: str, message: str, recommended_action: str = "") -> MCPError:
    details = {"recommended_action": recommended_action} if str(recommended_action or "").strip() else None
    return MCPError(code=code, message=message, details=details)


def _find_target_actor(*, group_id: str, actor_id: str) -> Optional[Dict[str, Any]]:
    group = load_group(str(group_id or "").strip())
    if group is None:
        return None
    actor = find_actor(group, str(actor_id or "").strip())
    return dict(actor) if isinstance(actor, dict) else None


def _normalize_runtime_escaped_text(*, group_id: str, actor_id: str, text: str) -> str:
    """Normalize double-escaped control characters only for codex actor runtimes."""
    raw = str(text or "")
    actor = _find_target_actor(group_id=group_id, actor_id=actor_id)
    if actor is None:
        return raw
    runtime = str(actor.get("runtime") or "").strip().lower()
    if runtime != "codex":
        return raw
    normalized = raw
    for pattern, replacement in (
        (r"(?<!\\)\\n", "\n"),
        (r"(?<!\\)\\r", "\r"),
        (r"(?<!\\)\\t", "\t"),
    ):
        normalized = re.sub(pattern, replacement, normalized)
    return normalized


def _remote_message_idempotency_key(value: str = "") -> str:
    explicit = str(value or "").strip()
    return explicit or f"mcpmsg_{uuid.uuid4().hex}"


def _message_operation_succeeded(result: Dict[str, Any]) -> bool:
    payload = result if isinstance(result, dict) else {}
    if payload.get("partial_failure") is True or payload.get("message_sent") is False:
        return False
    bridge_reply = payload.get("group_bridge_reply")
    if isinstance(bridge_reply, dict):
        if isinstance(bridge_reply.get("error"), dict):
            return False
        bridge_receipt = bridge_reply.get("receipt")
        if isinstance(bridge_receipt, dict) and str(bridge_receipt.get("status") or "") == "failed":
            return False
    receipt = payload.get("receipt")
    if isinstance(receipt, dict):
        return str(receipt.get("status") or "").strip() in {"queued", "retrying", "sent"}
    remote_send = payload.get("remote_send")
    if isinstance(remote_send, dict):
        remote_receipt = remote_send.get("receipt")
        if isinstance(remote_receipt, dict):
            return str(remote_receipt.get("status") or "").strip() in {"queued", "retrying", "sent"}
    return bool(
        isinstance(payload.get("event"), dict)
        or isinstance(payload.get("src_event"), dict) and isinstance(payload.get("dst_event"), dict)
        or payload.get("message_sent") is True
        or str(payload.get("event_id") or "").strip()
    )


def _with_post_message_nudge(result: Dict[str, Any]) -> Dict[str, Any]:
    if not _message_operation_succeeded(result):
        return result
    out = dict(result or {})
    out["post_message_nudge"] = {
        "kind": "whole_situation_reconstruction",
        "message": POST_MESSAGE_NUDGE,
    }
    return out


def message_send(
    *,
    group_id: str,
    dst_group_id: Optional[str] = None,
    actor_id: str,
    text: str,
    insight: Any = None,
    to: Optional[List[str]] = None,
    priority: str = "normal",
    reply_required: bool = False,
    idempotency_key: str = "",
    refs: Optional[List[Dict[str, Any]]] = None,
    suggested_user_message: str = "",
) -> Dict[str, Any]:
    """Send a message to the group (or cross-group)."""
    text = _normalize_runtime_escaped_text(group_id=group_id, actor_id=actor_id, text=text)
    insight = _normalize_runtime_escaped_text(
        group_id=group_id,
        actor_id=actor_id,
        text=insight,
    ) if isinstance(insight, str) else insight
    suggestion = _normalize_runtime_escaped_text(
        group_id=group_id,
        actor_id=actor_id,
        text=str(suggested_user_message or ""),
    ).strip()
    prio = str(priority or "normal").strip() or "normal"
    if prio not in ("normal", "attention"):
        raise MCPError(code="invalid_priority", message="priority must be 'normal' or 'attention'")
    reply_required_flag = coerce_bool(reply_required, default=False)

    dst_gid = str(dst_group_id or "").strip()
    if dst_gid and dst_gid != str(group_id or "").strip():
        if suggestion:
            raise MCPError(
                code="suggested_user_message_not_supported",
                message="suggested_user_message is only supported for messages in the current group",
            )
        if has_hash_recipient_token(normalize_recipient_tokens(to)):
            raise MCPError(code="invalid_recipient_syntax", message=CROSS_GROUP_HASH_RECIPIENT_MESSAGE)
        group_bridge_route = resolve_remote_group_route(group_id=group_id, remote_group_id=dst_gid)
        if group_bridge_route is not None:
            remote_to = normalize_recipient_tokens(to)
            if not remote_to:
                raise MCPError(
                    code="missing_remote_recipient",
                    message="remote messages require explicit to across Group Bridge; use '@foreman', '@all', or a target actor",
                )
            return _with_post_message_nudge(
                _call_daemon_or_raise(
                    {
                        "op": "remote_send",
                        "args": {
                            "group_id": group_id,
                            "by": actor_id,
                            "registration_id": group_bridge_route.registration_id,
                            "idempotency_key": _remote_message_idempotency_key(idempotency_key),
                            "insight": insight,
                            "require_peer_insight": True,
                            "payload": {
                                "text": text,
                                "to": remote_to,
                                "priority": prio,
                                "reply_required": reply_required_flag,
                                "refs": refs if refs is not None else [],
                            },
                        },
                    }
                )
            )
        return _with_post_message_nudge(
            _call_daemon_or_raise(
                {
                    "op": "send_cross_group",
                    "args": {
                        "group_id": group_id,
                        "dst_group_id": dst_gid,
                        "text": text,
                        "by": actor_id,
                        "to": to if to is not None else [],
                        "priority": prio,
                        "reply_required": reply_required_flag,
                        "insight": insight,
                        "require_peer_insight": True,
                        "refs": refs if refs is not None else [],
                    },
                }
            )
        )

    return _with_post_message_nudge(
        _call_daemon_or_raise(
            {
                "op": "send",
                "args": {
                    "group_id": group_id,
                    "text": text,
                    "by": actor_id,
                    "to": to if to is not None else [],
                    "path": "",
                    "priority": prio,
                    "reply_required": reply_required_flag,
                    "insight": insight,
                    "require_peer_insight": True,
                    "refs": refs if refs is not None else [],
                    "suggested_user_message": suggestion,
                },
            }
        )
    )


def tracked_send(
    *,
    group_id: str,
    actor_id: str,
    title: str,
    text: str,
    insight: Any = None,
    to: Optional[List[str]] = None,
    outcome: str = "",
    checklist: Optional[List[Dict[str, Any]]] = None,
    assignee: str = "",
    waiting_on: str = "",
    handoff_to: str = "",
    notes: str = "",
    priority: str = "normal",
    reply_required: bool = True,
    idempotency_key: str = "",
    refs: Optional[List[Dict[str, Any]]] = None,
) -> Dict[str, Any]:
    """Create a task and send one visible task-linked delegation message."""
    text = _normalize_runtime_escaped_text(group_id=group_id, actor_id=actor_id, text=text)
    insight = _normalize_runtime_escaped_text(
        group_id=group_id,
        actor_id=actor_id,
        text=insight,
    ) if isinstance(insight, str) else insight
    title = str(title or "").strip()
    if not title:
        raise MCPError(code="missing_title", message="cccc_tracked_send requires title")
    if not text.strip():
        raise MCPError(code="empty_message", message="cccc_tracked_send message text cannot be empty")
    prio = str(priority or "normal").strip() or "normal"
    if prio not in ("normal", "attention"):
        raise MCPError(code="invalid_priority", message="priority must be 'normal' or 'attention'")
    return _with_post_message_nudge(
        _call_daemon_or_raise({
            "op": "tracked_send",
            "args": {
                "group_id": group_id,
                "by": actor_id,
                "title": title,
                "text": text,
                "to": to if to is not None else [],
                "outcome": str(outcome or "").strip(),
                "checklist": checklist if checklist is not None else [],
                "assignee": str(assignee or "").strip(),
                "waiting_on": str(waiting_on or "").strip(),
                "handoff_to": str(handoff_to or "").strip(),
                "notes": str(notes or "").strip(),
                "priority": prio,
                "reply_required": coerce_bool(reply_required, default=True),
                "insight": insight,
                "require_peer_insight": True,
                "idempotency_key": str(idempotency_key or "").strip(),
                "refs": refs if refs is not None else [],
            },
        })
    )


def message_reply(
    *,
    group_id: str,
    actor_id: str,
    reply_to: str,
    text: str,
    insight: Any = None,
    to: Optional[List[str]] = None,
    priority: str = "normal",
    reply_required: bool = False,
    refs: Optional[List[Dict[str, Any]]] = None,
    suggested_user_message: str = "",
) -> Dict[str, Any]:
    """Reply to a message."""
    if not str(reply_to or "").strip():
        raise _mcp_error(
            code="missing_event_id",
            message="missing event_id (reply target)",
            recommended_action="Use the event_id/reply_to from the message or delivered turn envelope; if unsure, inspect cccc_inbox_list or the current turn events.",
        )
    text = _normalize_runtime_escaped_text(group_id=group_id, actor_id=actor_id, text=text)
    insight = _normalize_runtime_escaped_text(
        group_id=group_id,
        actor_id=actor_id,
        text=insight,
    ) if isinstance(insight, str) else insight
    suggestion = _normalize_runtime_escaped_text(
        group_id=group_id,
        actor_id=actor_id,
        text=str(suggested_user_message or ""),
    ).strip()
    prio = str(priority or "normal").strip() or "normal"
    if prio not in ("normal", "attention"):
        raise MCPError(code="invalid_priority", message="priority must be 'normal' or 'attention'")
    reply_required_flag = coerce_bool(reply_required, default=False)
    return _with_post_message_nudge(
        _call_daemon_or_raise(
            {
                "op": "reply",
                "args": {
                    "group_id": group_id,
                    "text": text,
                    "by": actor_id,
                    "reply_to": reply_to,
                    "to": to if to is not None else [],
                    "priority": prio,
                    "reply_required": reply_required_flag,
                    "insight": insight,
                    "require_peer_insight": True,
                    "refs": refs if refs is not None else [],
                    "suggested_user_message": suggestion,
                },
            }
        )
    )


def blob_path(*, group_id: str, rel_path: str) -> Dict[str, Any]:
    """Resolve a blob attachment path."""
    gid = str(group_id or "").strip()
    group = load_group(gid)
    if group is None:
        raise MCPError(code="group_not_found", message=f"group not found: {group_id}")
    rp = str(rel_path or "").strip()
    if not rp:
        raise _mcp_error(
            code="missing_rel_path",
            message="missing rel_path",
            recommended_action="Use the rel_path from the CCCC attachment, usually state/blobs/<sha256>_<name>.",
        )
    # Auto-prefix state/blobs/ when caller provides just the blob filename.
    if not rp.startswith("state/blobs/"):
        rp = f"state/blobs/{rp}"
    full = resolve_blob_attachment_path(group, rel_path=rp)
    return {"path": str(full)}


def blob_info(*, group_id: str, rel_path: str) -> Dict[str, Any]:
    """Return metadata for a blob attachment."""

    resolved = blob_path(group_id=group_id, rel_path=rel_path)
    path = Path(str(resolved.get("path") or ""))
    if not path.exists() or not path.is_file():
        raise _mcp_error(
            code="not_found",
            message=f"blob not found: {rel_path}",
            recommended_action="Check the exact attachment rel_path from the inbox/turn payload; pass the full state/blobs/... path or the blob filename.",
        )
    try:
        size = int(path.stat().st_size)
    except Exception:
        size = 0
    return {
        "path": str(path),
        "rel_path": str(rel_path or "").strip(),
        "title": path.name,
        "mime_type": _guess_mime_type(path.name),
        "bytes": size,
    }


def blob_read(*, group_id: str, rel_path: str, max_bytes: Any = _DEFAULT_BLOB_READ_BYTES) -> Dict[str, Any]:
    """Read a text blob attachment by relative path."""

    info = blob_info(group_id=group_id, rel_path=rel_path)
    path = Path(str(info.get("path") or ""))
    try:
        limit = int(max_bytes if max_bytes is not None else _DEFAULT_BLOB_READ_BYTES)
    except Exception:
        limit = _DEFAULT_BLOB_READ_BYTES
    limit = max(1, min(limit, _MAX_BLOB_READ_BYTES))
    try:
        with path.open("rb") as fh:
            data = fh.read(limit + 1)
    except Exception as exc:
        raise MCPError(code="read_failed", message=str(exc))
    truncated = len(data) > limit
    raw = data[:limit]
    text = raw.decode("utf-8", errors="replace")
    return {
        **info,
        "text": text,
        "truncated": truncated,
        "max_bytes": limit,
    }


def file_send(
    *,
    group_id: str,
    actor_id: str,
    path: str,
    text: str = "",
    insight: Any = None,
    dst_group_id: str = "",
    to: Optional[List[str]] = None,
    priority: str = "normal",
    reply_required: bool = False,
) -> Dict[str, Any]:
    """Send a local file as a chat.message attachment.

    Security: only files under the group's active scope root are allowed.
    """
    gid = str(group_id or "").strip()
    group = load_group(gid)
    if group is None:
        raise MCPError(code="group_not_found", message=f"group not found: {group_id}")

    scope_key = str(group.doc.get("active_scope_key") or "").strip()
    if not scope_key:
        raise MCPError(code="missing_scope", message="group has no active scope")

    scopes = group.doc.get("scopes")
    scope_url = ""
    if isinstance(scopes, list):
        for sc in scopes:
            if isinstance(sc, dict) and str(sc.get("scope_key") or "").strip() == scope_key:
                scope_url = str(sc.get("url") or "").strip()
                break
    if not scope_url:
        raise MCPError(code="missing_scope", message="active scope url not found")

    root = Path(scope_url).expanduser().resolve()
    src = Path(str(path or "").strip())
    if not src.is_absolute():
        src = (root / src).resolve()
    else:
        src = src.expanduser().resolve()

    try:
        src.relative_to(root)
    except ValueError:
        raise _mcp_error(
            code="invalid_path",
            message="path must be under the group's active scope root",
            recommended_action="Write or move the file under the active workspace scope first, then call cccc_file(action='send', path=...).",
        )
    if not src.exists() or not src.is_file():
        raise _mcp_error(
            code="not_found",
            message=f"file not found: {src}",
            recommended_action="Verify the active scope and file path with cccc_repo(action='list_dir') or cccc_shell('ls ...'), then retry cccc_file(action='send').",
        )

    prio = str(priority or "normal").strip() or "normal"
    if prio not in ("normal", "attention"):
        raise MCPError(code="invalid_priority", message="priority must be 'normal' or 'attention'")
    try:
        normalized_insight = normalized_insight_or_error(insight)
    except ValueError as exc:
        raise MCPError(code="invalid_insight", message=str(exc)) from exc
    dst_gid = str(dst_group_id or "").strip()
    recipient_tokens = normalize_recipient_tokens(to)
    if dst_gid and dst_gid != gid:
        if has_hash_recipient_token(recipient_tokens):
            raise MCPError(code="invalid_recipient_syntax", message=CROSS_GROUP_HASH_RECIPIENT_MESSAGE)
        if resolve_remote_group_route(group_id=gid, remote_group_id=dst_gid) is None:
            if load_group(dst_gid) is None:
                raise MCPError(
                    code="group_not_found",
                    message=group_not_found_with_resolution_hint(dst_gid),
                )
            raise MCPError(
                code="attachments_not_supported",
                message="attachments are only supported for remote Group Bridge messages",
            )
        remote_to = cross_group_recipient_tokens_or_default(recipient_tokens)
        if remote_recipients_include_peer(remote_to) and normalized_insight is None:
            raise MCPError(
                code="peer_insight_required",
                message="Not sent: this peer-facing message is missing `insight`.",
                details=peer_insight_required_details(),
            )
    else:
        try:
            audience = preflight_local_peer_audience(
                group,
                to_tokens=recipient_tokens,
                by=actor_id,
                apply_default_send=True,
            )
        except PeerRecipientError as exc:
            raise MCPError(code=exc.code, message=exc.message, details=exc.details) from exc
        if audience.peer_actor_ids and normalized_insight is None:
            raise MCPError(
                code="peer_insight_required",
                message="Not sent: this peer-facing message is missing `insight`.",
                details=peer_insight_required_details(),
            )

    try:
        raw = src.read_bytes()
    except Exception as e:
        raise MCPError(code="read_failed", message=str(e))

    att = store_blob_bytes(group, data=raw, filename=src.name, mime_type=_guess_mime_type(src.name))
    msg = str(text or "").strip() or f"[file] {att.get('title') or src.name}"
    reply_required_flag = coerce_bool(reply_required, default=False)
    if dst_gid and dst_gid != gid:
        return _with_post_message_nudge(_call_daemon_or_raise(
            {
                "op": "send_cross_group",
                "args": {
                    "group_id": gid,
                    "dst_group_id": dst_gid,
                    "text": msg,
                    "by": actor_id,
                    "to": to if to is not None else [],
                    "attachments": [att],
                    "priority": prio,
                    "reply_required": reply_required_flag,
                    "insight": normalized_insight,
                    "require_peer_insight": True,
                },
            }
        ))
    return _with_post_message_nudge(_call_daemon_or_raise(
        {
            "op": "send",
            "args": {
                "group_id": gid,
                "text": msg,
                "by": actor_id,
                "to": to if to is not None else [],
                "path": "",
                "attachments": [att],
                "priority": prio,
                "reply_required": reply_required_flag,
                "insight": normalized_insight,
                "require_peer_insight": True,
            },
        }
    ))
