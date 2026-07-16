"""
Entry point for running IM bridge as a module.

Usage:
    python -m cccc.ports.im <group_id> [platform]
"""

from __future__ import annotations

import sys

from .bridge import start_bridge


def main() -> int:
    if len(sys.argv) < 2:
        print("Usage: python -m cccc.ports.im <group_id> [platform]")
        print("  platform: telegram (default), slack, discord, feishu (Feishu/Lark), dingtalk, wecom, weixin")
        print("")
        print("Environment variables:")
        print("  Telegram: TELEGRAM_BOT_TOKEN")
        print("  Slack:    SLACK_BOT_TOKEN, SLACK_APP_TOKEN (optional)")
        print("  Discord:  DISCORD_BOT_TOKEN")
        print("  Feishu/Lark: FEISHU_APP_ID, FEISHU_APP_SECRET, FEISHU_DOMAIN (optional: feishu|lark|https://...)")
        print("  DingTalk: DINGTALK_APP_KEY, DINGTALK_APP_SECRET, DINGTALK_ROBOT_CODE (optional)")
        print("  WeCom:    WECOM_BOT_ID, WECOM_SECRET")
        print("  Weixin:   optional CCCC_IM_WEIXIN_ACCOUNT_ID (legacy only; login uses per-group cached credentials)")
        return 1

    group_id = sys.argv[1]
    platform = sys.argv[2] if len(sys.argv) > 2 else "telegram"

    try:
        start_bridge(group_id, platform)
        return 0
    except SystemExit as e:
        return int(e.code) if e.code is not None else 1
    except Exception as e:
        print(f"[error] {e}")
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
