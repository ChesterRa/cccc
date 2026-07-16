"""DingTalk message reaction helper."""

from __future__ import annotations

import json
import urllib.error
import urllib.request
from typing import Callable

from .dingtalk_api import DINGTALK_API_NEW


class DingTalkReactionService:
    """Send and recall DingTalk robot emoji reactions."""

    def __init__(
        self,
        *,
        get_token: Callable[[], str],
        robot_code_getter: Callable[[], str],
        log: Callable[[str], None],
    ) -> None:
        self._get_token = get_token
        self._robot_code_getter = robot_code_getter
        self._log = log

    async def send_reaction(
        self,
        *,
        message_id: str,
        conversation_id: str,
        reaction_type: str,
        recall: bool = False,
    ) -> bool:
        """Add or recall a reaction on a DingTalk message."""
        message_id = str(message_id or "").strip()
        conversation_id = str(conversation_id or "").strip()
        reaction_type = str(reaction_type or "").strip()
        robot_code = str(self._robot_code_getter() or "").strip()
        if not (message_id and conversation_id and reaction_type and robot_code):
            return False

        token = self._get_token()
        if not token:
            return False

        try:
            body = {
                "robotCode": robot_code,
                "openMsgId": message_id,
                "openConversationId": conversation_id,
                "emotionType": 2,
                "emotionName": reaction_type,
                "textEmotion": {
                    "emotionId": "2659900",
                    "emotionName": reaction_type,
                    "text": reaction_type,
                    "backgroundId": "im_bg_1",
                },
            }
            endpoint = "/v1.0/robot/emotion/recall" if recall else "/v1.0/robot/emotion/reply"
            _post_json(endpoint, body, token)
            return True
        except urllib.error.HTTPError as exc:
            action = "recall" if recall else "reply"
            try:
                detail = exc.read().decode("utf-8", "ignore")[:300]
            except Exception:
                detail = ""
            self._log(f"[reaction] DingTalk {action} reaction failed message={message_id}: HTTP {exc.code} {detail}")
            return False
        except Exception as exc:
            action = "recall" if recall else "reply"
            self._log(f"[reaction] DingTalk {action} reaction failed message={message_id}: {exc}")
            return False


def _post_json(endpoint: str, body: dict, token: str) -> None:
    data = json.dumps(body, ensure_ascii=False).encode("utf-8")
    req = urllib.request.Request(f"{DINGTALK_API_NEW}{endpoint}", data=data, method="POST")
    req.add_header("Content-Type", "application/json; charset=utf-8")
    req.add_header("Accept", "application/json")
    req.add_header("x-acs-dingtalk-access-token", token)
    req.add_header(
        "User-Agent",
        "DingTalkStream/1.0 SDK/0.1.0 Python (+https://github.com/open-dingtalk/dingtalk-stream-sdk-python)",
    )
    with urllib.request.urlopen(req, timeout=15) as resp:
        resp.read()
