from __future__ import annotations

from dataclasses import dataclass
from typing import Any, Dict

from .mentions import FeishuBotIdentity


@dataclass(frozen=True)
class FeishuBotIdentityResult:
    ok: bool
    identity: FeishuBotIdentity
    error: str = ""


def parse_bot_identity_response(resp: Dict[str, Any], *, configured_name: str = "") -> FeishuBotIdentityResult:
    """Parse /bot/v3/info into the identity used for mention routing."""
    if resp.get("code") != 0:
        msg = str(resp.get("msg") or "unknown")
        return FeishuBotIdentityResult(False, FeishuBotIdentity(), msg)

    data = resp.get("data") if isinstance(resp.get("data"), dict) else {}
    bot = data.get("bot") if isinstance(data.get("bot"), dict) else data
    if not isinstance(bot, dict):
        return FeishuBotIdentityResult(False, FeishuBotIdentity(), "response did not include bot data")

    identity = FeishuBotIdentity.from_values(
        open_id=bot.get("open_id") or bot.get("bot_open_id") or "",
        user_id=bot.get("user_id") or bot.get("bot_user_id") or "",
        name=bot.get("app_name") or bot.get("name") or bot.get("bot_name") or configured_name,
    )
    if not identity.has_matchable_value:
        return FeishuBotIdentityResult(False, identity, "response did not include a usable id or name")

    return FeishuBotIdentityResult(True, identity)
