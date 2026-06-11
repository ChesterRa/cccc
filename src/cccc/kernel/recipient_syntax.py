"""Recipient syntax guards shared by messaging entrypoints."""

from __future__ import annotations

from typing import Any

CROSS_GROUP_RESOLVE_HINT = (
    'Resolve natural #group/title tokens first with cccc_group(action="resolve", token="<#group or title>") '
    "and use the returned unique real group_id. Do not use #token/title as dst_group_id; never guess dst_group_id."
)

CROSS_GROUP_HASH_RECIPIENT_MESSAGE = (
    "#group tokens are routing hints, not recipients. Rewrite the call as "
    'cccc_group(action="resolve", token="<#group or title>"), use the returned unique real group_id, then '
    "cccc_message_send(dst_group_id=<g_...>, to='@foreman' or target actor id, "
    "text=<your own natural message to the target>). Do not put #group in to and do not forward a template."
)


def group_not_found_with_resolution_hint(group_id: str) -> str:
    raw = str(group_id or "").strip()
    token = raw or "<#group or title>"
    return (
        f"group not found: {raw}. dst_group_id must be the real group id. "
        f'Try cccc_group(action="resolve", token="{token}") first; use the returned unique real group_id. '
        "Do not use #token/title as dst_group_id."
    )


def normalize_recipient_tokens(raw: Any) -> list[str]:
    if isinstance(raw, list):
        return [str(item).strip() for item in raw if isinstance(item, str) and str(item).strip()]
    if isinstance(raw, str):
        token = raw.strip()
        return [token] if token else []
    return []


def has_hash_recipient_token(tokens: list[str]) -> bool:
    return any(str(token or "").strip().startswith("#") for token in tokens)


def cross_group_recipient_tokens_or_default(tokens: list[str]) -> list[str]:
    """Default cross-group contact to the target foreman, never broadcast."""
    cleaned = [str(token or "").strip() for token in tokens if str(token or "").strip()]
    return cleaned or ["@foreman"]
