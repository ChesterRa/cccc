"""Feishu webhook event normalization helpers."""

from __future__ import annotations

from dataclasses import dataclass
from typing import Any, Dict, Optional


@dataclass
class FeishuWebhookResult:
    challenge_response: Optional[Dict[str, Any]] = None
    message_event: Optional[Dict[str, Any]] = None


def normalize_webhook_event(event: Dict[str, Any]) -> FeishuWebhookResult:
    if "challenge" in event:
        return FeishuWebhookResult(challenge_response={"challenge": event["challenge"]})

    if event.get("schema", "") == "2.0":
        return FeishuWebhookResult(message_event=event)

    legacy_event = event.get("event", {})
    return FeishuWebhookResult(
        message_event={
            "header": {
                "event_type": legacy_event.get("type", ""),
            },
            "event": legacy_event,
        }
    )
