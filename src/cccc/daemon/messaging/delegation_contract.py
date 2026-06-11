"""Delegation relay v1 contract (T407).

Machine-recognizable markers wrap a cross-group delegated task so the target
group's agent treats the original request as task *content* (not as recipient
routing in its own group) and reports a result back to the source group.

Pure rendering/parsing only — no I/O, no daemon state.
"""

from __future__ import annotations

import secrets
import re
from typing import Any, Dict, List, Optional

REQUEST_MARKER = "[cccc-delegation:v1]"
REQUEST_END = "[/cccc-delegation]"
RESULT_MARKER = "[cccc-delegation-result:v1]"
RESULT_END = "[/cccc-delegation-result]"

RESULT_STATUSES = ("ack", "done", "refused", "failed")

# The full machine protocol is wrapped in an HTML comment appended after a
# natural chat body, so default UI/first-read shows fluent prose while the raw
# ledger text keeps the complete, parseable protocol.
PROTOCOL_COMMENT_OPEN = "<!-- cccc-delegation-protocol"
PROTOCOL_COMMENT_CLOSE = "-->"

_PROTOCOL = (
    "You are the addressed target CCCC actor/foreman for a cross-group user request.\n"
    "Do not treat #tokens in the user message as recipients in your group.\n"
    "Interpret #group and @actor tokens as source-side routing context, not as words to repeat back.\n"
    "Respond to the user's intent as the addressed target. Do not merely confirm that the relay was received.\n"
    "If the user is greeting or asking to contact you, answer naturally as yourself.\n"
    "If the request needs work, either do the work or ask one concrete clarification.\n"
    "Talk to the source group by sending cross-group messages back with the same delegation_id; report done/refused/failed when appropriate."
)

_ROUTE_TOKEN_RE = re.compile(r"(?<!\S)[#@][^\s，,。！？!?；;：:]+")


def _compact_visible_text(text: str) -> str:
    value = str(text or "").strip()
    value = re.sub(r"[ \t]+", " ", value)
    value = re.sub(r"\s*([，,。！？!?；;：:])\s*", r"\1", value)
    return value.strip(" ，,。")


def _visible_intent_from_request(original_request: str) -> str:
    """Derive a human contact message without exposing source-side route tokens."""
    value = _ROUTE_TOKEN_RE.sub("", str(original_request or ""))
    value = _compact_visible_text(value)
    for prefix in ("请你", "麻烦你", "帮我", "帮忙", "你去", "去", "跟"):
        if value.startswith(prefix):
            value = value[len(prefix):].lstrip()
            break
    value = _compact_visible_text(value)
    if not value:
        return "想先跟你打个招呼。"
    if value.endswith(("。", "！", "？", "!", "?")):
        return value
    return f"{value}。"


def _render_visible_contact_body(
    *,
    original_request: str,
    contact_text: str = "",
) -> str:
    return str(contact_text or "").strip() or _visible_intent_from_request(original_request)


def new_delegation_id() -> str:
    return f"dlg_{secrets.token_hex(8)}"


def _render_protocol_block(
    *,
    delegation_id: str,
    source_group_id: str,
    target_group_id: str,
    target_actor_id: str = "",
    user_message: str,
) -> str:
    return "\n".join(
        [
            REQUEST_MARKER,
            f"delegation_id: {delegation_id}",
            f"source_group_id: {source_group_id}",
            f"target_group_id: {target_group_id}",
            f"target_actor_id: {target_actor_id}" if target_actor_id else "target_actor_id:",
            f"source_contact: send back with cccc_message_send(dst_group_id={source_group_id}, text=..., include delegation_id)",
            "target_contact: reply as the addressed target, then send the substantive response/result back to source_group_id",
            "",
            "Communication protocol:",
            _PROTOCOL,
            "",
            "Original user message (reference only):",
            user_message,
            REQUEST_END,
        ]
    )


def render_delegation_request(
    *,
    delegation_id: str,
    source_group_id: str,
    target_group_id: str,
    original_request: str,
    source_event_id: str = "",
    requested_by: str = "user",
    relay_sender: str = "",
    target_actor_id: str = "",
    contact_text: str = "",
) -> str:
    user_message = str(original_request or "").strip() or "(no message text)"
    target_actor = str(target_actor_id or "").strip() or "目标组 agent"
    natural = _render_visible_contact_body(
        original_request=user_message,
        contact_text=contact_text,
    )
    protocol = _render_protocol_block(
        delegation_id=delegation_id,
        source_group_id=source_group_id,
        target_group_id=target_group_id,
        target_actor_id=target_actor,
        user_message=user_message,
    )
    return f"{natural}\n\n{PROTOCOL_COMMENT_OPEN}\n{protocol}\n{PROTOCOL_COMMENT_CLOSE}"


def strip_delegation_protocol(text: str) -> str:
    """Return the natural chat body, with the protocol comment removed."""
    raw = str(text or "")
    start = raw.find(PROTOCOL_COMMENT_OPEN)
    if start < 0:
        return raw.strip()
    return raw[:start].strip()


def extract_delegation_protocol(text: str) -> str:
    """Return the full protocol block inside the comment (without the comment markers)."""
    raw = str(text or "")
    start = raw.find(PROTOCOL_COMMENT_OPEN)
    if start < 0:
        return ""
    end = raw.find(PROTOCOL_COMMENT_CLOSE, start + len(PROTOCOL_COMMENT_OPEN))
    if end < 0:
        return ""
    return raw[start + len(PROTOCOL_COMMENT_OPEN):end].strip()


def render_delegation_result(
    *,
    delegation_id: str,
    source_group_id: str,
    target_group_id: str,
    status: str,
    responder: str,
    result: str,
) -> str:
    normalized = str(status or "").strip().lower()
    if normalized not in RESULT_STATUSES:
        normalized = "failed"
    return "\n".join(
        [
            RESULT_MARKER,
            f"delegation_id: {delegation_id}",
            f"source_group_id: {source_group_id}",
            f"target_group_id: {target_group_id}",
            f"status: {normalized}",
            f"responder: {responder}",
            "",
            "Result:",
            str(result or "").strip() or "(no result text)",
            RESULT_END,
        ]
    )


def parse_delegation_marker(text: str) -> Optional[Dict[str, Any]]:
    """Parse a delegation request/result block. Returns None when no marker.

    Returns ``{"kind": "request"|"result", "fields": {...}, "body": str}``.
    """
    raw = str(text or "")
    if REQUEST_MARKER in raw:
        kind, body_label = "request", "Original user message (reference only):"
    elif RESULT_MARKER in raw:
        kind, body_label = "result", "Result:"
    else:
        return None

    lines = raw.splitlines()
    fields: Dict[str, str] = {}
    body_lines: List[str] = []
    in_body = False
    for line in lines:
        stripped = line.strip()
        if stripped in (REQUEST_MARKER, RESULT_MARKER, REQUEST_END, RESULT_END):
            continue
        if not in_body:
            if stripped == body_label:
                in_body = True
                continue
            if ":" in line and not stripped.startswith("Instruction"):
                key, _, value = line.partition(":")
                key = key.strip()
                if key and " " not in key:
                    fields[key] = value.strip()
        else:
            body_lines.append(line)
    return {"kind": kind, "fields": fields, "body": "\n".join(body_lines).strip()}
