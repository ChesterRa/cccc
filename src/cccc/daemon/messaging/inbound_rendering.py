"""Actor-facing inbound message rendering.

This module is intentionally transport-neutral: it renders canonical chat
metadata into text an actor can read. PTY and headless transports still submit
that text through their own delivery mechanics.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any


@dataclass(frozen=True)
class ActorInboundEnvelope:
    event_id: str = ""
    kind: str = "chat.message"
    by: str = "user"
    to: list[str] = field(default_factory=list)
    text: str = ""
    reply_to: str = ""
    quote_text: str = ""
    source_platform: str = ""
    source_user_name: str = ""
    source_user_id: str = ""
    priority: str = "normal"
    reply_required: bool = False
    refs: list[dict[str, Any]] = field(default_factory=list)
    attachments: list[dict[str, Any]] = field(default_factory=list)


def _trim(value: Any) -> str:
    return str(value or "").strip()


def _quote_preview(value: str, *, limit: int = 80) -> str:
    quote = str(value or "").strip()
    if not quote:
        return ""
    preview = quote[:limit].replace("\n", " ")
    if len(quote) > limit:
        preview += "..."
    return preview


def render_actor_inbound_message(envelope: ActorInboundEnvelope) -> str:
    """Render one canonical inbound envelope for actor consumption."""
    who = _trim(envelope.by) or "user"
    targets = ", ".join([item for item in (_trim(x) for x in (envelope.to or [])) if item]) or "@all"

    source_bits = [
        bit
        for bit in (
            _trim(envelope.source_platform),
            _trim(envelope.source_user_name),
            _trim(envelope.source_user_id),
        )
        if bit
    ]
    if source_bits:
        who = f"{who}[{' / '.join(source_bits)}]"

    header = f"[cccc] {who} → {targets}"
    reply_to = _trim(envelope.reply_to)
    if reply_to:
        header += f" (reply:{reply_to[:8]})"

    quote = _quote_preview(envelope.quote_text)
    if quote:
        header += f'\n> "{quote}"'

    body = str(envelope.text or "").rstrip("\n")
    return f"{header}:\n{body}" if "\n" in body else f"{header}: {body}"
