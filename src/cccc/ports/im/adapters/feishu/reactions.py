"""Feishu reaction request helpers."""

from __future__ import annotations

from typing import Any, Dict, Optional, Tuple


DEFAULT_TYPING_EMOJI = "OnIt"


def build_add_reaction_request(
    message_id: str,
    emoji_type: str = "",
) -> Tuple[str, Dict[str, Any], str]:
    emoji = emoji_type or DEFAULT_TYPING_EMOJI
    return (
        f"/im/v1/messages/{message_id}/reactions",
        {"reaction_type": {"emoji_type": emoji}},
        emoji,
    )


def parse_add_reaction_response(resp: Dict[str, Any]) -> Optional[str]:
    if resp.get("code") != 0:
        return None
    reaction_id = (resp.get("data") or {}).get("reaction_id", "")
    return str(reaction_id or "") or None


def build_remove_reaction_request(message_id: str, reaction_id: str) -> str:
    return f"/im/v1/messages/{message_id}/reactions/{reaction_id}"


def reaction_succeeded(resp: Dict[str, Any]) -> bool:
    return resp.get("code") == 0
