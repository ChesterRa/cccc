"""
IM Platform Adapters

Each adapter handles platform-specific communication:
- Telegram: Long-poll getUpdates
- Slack: Socket Mode + Web API
- Discord: Gateway
- Feishu/Lark: WebSocket + REST API
- DingTalk: Stream mode + REST API
"""

from .base import IMAdapter
from .telegram import TelegramAdapter
from .slack import SlackAdapter
from .discord import DiscordAdapter
from .feishu import FeishuAdapter
from .dingtalk import DingTalkAdapter
from .weixin import WeixinAdapter

__all__ = [
    "IMAdapter",
    "TelegramAdapter",
    "SlackAdapter",
    "DiscordAdapter",
    "FeishuAdapter",
    "DingTalkAdapter",
    "WeixinAdapter",
]
