"""DingTalk outbound text sender."""

from __future__ import annotations

import json
import urllib.request
from typing import Any, Callable, Dict, List, Optional

from .dingtalk_messages import (
    RateLimiter,
    build_markdown_payload,
    clean_at_user_ids,
    split_message_chunks,
)


class DingTalkSender:
    """Send DingTalk text messages through webhook/API fallback paths."""

    def __init__(
        self,
        *,
        api_old: Callable[[str, str, Optional[Dict[str, Any]], int], Dict[str, Any]],
        api_new: Callable[[str, str, Optional[Dict[str, Any]], int], Dict[str, Any]],
        send_webhook: Callable[[str, str, Optional[List[str]]], bool],
        rate_limiter: RateLimiter,
        robot_code_getter: Callable[[], str],
        is_group: Callable[[str], bool],
        user_id_for_chat: Callable[[str], str],
        webhook_entry: Callable[[str], Optional[tuple[str, float]]],
        last_sender_for_chat: Callable[[str], Optional[str]],
        log: Callable[[str], None],
        max_chars_getter: Callable[[], int],
        max_lines_getter: Callable[[], int],
    ) -> None:
        self._api_old = api_old
        self._api_new = api_new
        self._send_webhook = send_webhook
        self._rate_limiter = rate_limiter
        self._robot_code_getter = robot_code_getter
        self._is_group = is_group
        self._user_id_for_chat = user_id_for_chat
        self._webhook_entry = webhook_entry
        self._last_sender_for_chat = last_sender_for_chat
        self._log = log
        self._max_chars_getter = max_chars_getter
        self._max_lines_getter = max_lines_getter

    def send_text(
        self,
        chat_id: str,
        text: str,
        *,
        mention_user_ids: Optional[List[str]] = None,
    ) -> bool:
        """Send text to a DingTalk conversation."""
        if not text:
            return True

        chunks = split_message_chunks(
            text,
            max_chars=self._max_chars_getter(),
            max_lines=self._max_lines_getter(),
        )
        if not chunks:
            return True

        at_user_ids = self._resolve_at_user_ids(chat_id, mention_user_ids)
        for idx, chunk in enumerate(chunks):
            chunk_at_user_ids = at_user_ids if idx == 0 else None
            self._rate_limiter.wait_and_acquire(chat_id)

            webhook = self._webhook_entry(chat_id)
            if webhook:
                webhook_url, _expires_at = webhook
                if self._send_webhook(webhook_url, chunk, chunk_at_user_ids):
                    continue
                self._log("[send] Webhook failed, falling back to API...")
            else:
                self._log("[send] No cached sessionWebhook; falling back to API.")

            if not self._send_via_api_or_legacy(chat_id, chunk, chunk_at_user_ids):
                return False

        return True

    def send_via_webhook(
        self,
        webhook_url: str,
        text: str,
        at_user_ids: Optional[List[str]] = None,
    ) -> bool:
        """Send a markdown message via DingTalk sessionWebhook."""
        body: Dict[str, Any] = {
            "msgtype": "markdown",
            "markdown": build_markdown_payload(text),
        }
        cleaned_at_user_ids = clean_at_user_ids(at_user_ids)
        if cleaned_at_user_ids:
            body["at"] = {"atUserIds": cleaned_at_user_ids}

        req = urllib.request.Request(
            webhook_url,
            data=json.dumps(body, ensure_ascii=False).encode("utf-8"),
            method="POST",
        )
        req.add_header("Content-Type", "application/json; charset=utf-8")

        try:
            with urllib.request.urlopen(req, timeout=15) as resp:
                result = json.loads(resp.read().decode("utf-8", errors="replace"))
            if result.get("errcode") == 0:
                self._log("[webhook] Sent successfully")
                return True
            self._log(f"[webhook] Failed: {result}")
            return False
        except Exception as e:
            self._log(f"[webhook] Error: {e}")
            return False

    def send_legacy(
        self,
        chat_id: str,
        text: str,
        at_user_ids: Optional[List[str]] = None,
    ) -> bool:
        """Send text using DingTalk legacy /chat/send API."""
        body = {
            "chatid": chat_id,
            "msg": {
                "msgtype": "markdown",
                "markdown": build_markdown_payload(text),
            },
        }
        cleaned_at_user_ids = clean_at_user_ids(at_user_ids)
        if cleaned_at_user_ids:
            body["msg"]["at"] = {"atUserIds": cleaned_at_user_ids}

        resp = self._api_old("POST", "/chat/send", body, 15)
        if resp.get("errcode") == 0:
            return True

        self._log(f"[send_legacy] Failed: {resp.get('errmsg', 'unknown')}")
        return False

    def _resolve_at_user_ids(
        self,
        chat_id: str,
        mention_user_ids: Optional[List[str]],
    ) -> Optional[List[str]]:
        if mention_user_ids is not None:
            cleaned = clean_at_user_ids(mention_user_ids)
            return cleaned or None
        if self._is_group(chat_id):
            staff_id = self._last_sender_for_chat(chat_id)
            if staff_id:
                return [staff_id]
        return None

    def _send_via_api_or_legacy(
        self,
        chat_id: str,
        chunk: str,
        at_user_ids: Optional[List[str]],
    ) -> bool:
        robot_code = self._robot_code_getter()
        if not robot_code:
            if self._is_group(chat_id):
                self._log("[send] Missing robot_code; cannot use new API fallback. Trying legacy API.")
                return self.send_legacy(chat_id, chunk, at_user_ids=at_user_ids)
            self._log("[send] Missing robot_code; cannot send via API fallback. Configure DINGTALK_ROBOT_CODE.")
            return False

        body: Dict[str, Any] = {
            "robotCode": robot_code,
            "msgKey": "sampleMarkdown",
            "msgParam": json.dumps(build_markdown_payload(chunk, at_user_ids), ensure_ascii=False),
        }

        if self._is_group(chat_id):
            body["openConversationId"] = chat_id
            endpoint = "/v1.0/robot/groupMessages/send"
        else:
            user_id = self._user_id_for_chat(chat_id) or chat_id
            body["userIds"] = [user_id]
            endpoint = "/v1.0/robot/oToMessages/batchSend"

        resp = self._api_new("POST", endpoint, body, 15)
        if resp.get("processQueryKey") or resp.get("sendResults"):
            return True

        if "code" in resp or "errcode" in resp:
            return self.send_legacy(chat_id, chunk, at_user_ids=at_user_ids)

        self._log(f"[send] Failed to chat {chat_id}: {resp}")
        return False
