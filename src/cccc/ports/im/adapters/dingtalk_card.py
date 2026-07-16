"""
DingTalk AI Card streaming client for CCCC IM Bridge.

Provides a thin async wrapper around DingTalk's AI Card API for
streaming "typewriter effect" message updates.

API endpoints used:
- POST /v1.0/card/instances/createAndDeliver  (create + deliver card)
- PUT  /v1.0/card/streaming                   (streaming update)
- PUT  /v1.0/card/instances                   (update card data / status)
"""

from __future__ import annotations

import json
import logging
import time
import uuid
from dataclasses import dataclass
from typing import Any, Callable, Dict, Optional

import aiohttp

from .base import OutboundStreamHandle

logger = logging.getLogger(__name__)

# ── DingTalk AI Card constants ───────────────────────────────────────
DINGTALK_API = "https://api.dingtalk.com"
AI_CARD_TEMPLATE_ID = "382e4302-551d-4880-bf29-a30acfab2e71.schema"

# Throttle interval: DingTalk API has rate limits; 300ms is a safe floor.
THROTTLE_INTERVAL_S = 0.3


class DingTalkAICardClient:
    """Async client for DingTalk AI Card create / stream-update / finalize."""

    def __init__(
        self,
        get_access_token: Callable[[], str],
        *,
        robot_code: str = "",
        throttle_interval: float = THROTTLE_INTERVAL_S,
    ) -> None:
        """
        Args:
            get_access_token: callable that returns a valid access_token string.
            robot_code: DingTalk robot code (used in delivery model).
            throttle_interval: minimum seconds between streaming updates.
        """
        self._get_token = get_access_token
        self._robot_code = robot_code
        self._throttle_interval = throttle_interval

        # Per-card throttle state: card_instance_id → _ThrottleState
        self._throttle: Dict[str, _ThrottleState] = {}

    # ── public API ───────────────────────────────────────────────────

    async def create_card(
        self,
        conversation_id: str,
        content: str,
        *,
        title: str = "",
    ) -> Optional[str]:
        """Create and deliver an AI Card instance.

        Returns the ``card_instance_id`` (== outTrackId) on success, else None.
        """
        card_instance_id = uuid.uuid4().hex

        card_data = {
            "cardParamMap": {
                "msgContent": content,
                "msgTitle": title,
                "flowStatus": "1",  # PROCESSING
            },
        }

        body: Dict[str, Any] = {
            "cardTemplateId": AI_CARD_TEMPLATE_ID,
            "outTrackId": card_instance_id,
            "cardData": card_data,
            "callbackType": "STREAM",
            "imGroupOpenSpaceModel": {"supportForward": True},
            "imRobotOpenSpaceModel": {"supportForward": True},
        }

        # Determine space type
        if conversation_id.startswith("cid"):
            # Group chat
            body["openSpaceId"] = f"dtv1.card//IM_GROUP.{conversation_id}"
            body["imGroupOpenDeliverModel"] = {
                "robotCode": self._robot_code,
            }
        else:
            # Single chat
            body["openSpaceId"] = f"dtv1.card//IM_ROBOT.{conversation_id}"
            body["imRobotOpenDeliverModel"] = {
                "spaceType": "IM_ROBOT",
            }

        resp = await self._api(
            "POST",
            "/v1.0/card/instances/createAndDeliver",
            body,
        )

        if resp is None:
            logger.warning("[dingtalk_card] create_card failed")
            return None

        return card_instance_id

    async def update_card(
        self,
        card_instance_id: str,
        content: str,
        *,
        seq: int = 0,
    ) -> None:
        """Stream-update a card with full snapshot content.

        Sync-safe throttle (no async tasks): when called faster than the
        interval, content is buffered and the *previous* buffered content is
        flushed first.  This avoids depending on asyncio tasks surviving
        across separate ``asyncio.run()`` invocations.
        """
        state = self._throttle.get(card_instance_id)
        if state is None:
            state = _ThrottleState(interval=self._throttle_interval)
            self._throttle[card_instance_id] = state

        now = time.monotonic()
        elapsed = now - state.last_sent

        if elapsed >= state.interval:
            # Enough time passed — send immediately.
            state.last_sent = now
            state.pending_content = None
            await self._put_streaming(card_instance_id, content)
        else:
            # Too soon — buffer latest content.  It will be flushed on the
            # next call that passes the throttle window, or at finalize_card.
            state.pending_content = content

    async def finalize_card(
        self,
        card_instance_id: str,
        content: str,
    ) -> None:
        """Finalize the card (isFinalize=true). Not throttled.

        Flushes any pending buffered content before finalizing.
        """
        state = self._throttle.pop(card_instance_id, None)
        # Pending content is superseded by the final content — no need to
        # send the buffered update separately.

        await self._put_streaming(
            card_instance_id,
            content,
            is_finalize=True,
        )

    async def _put_streaming(
        self,
        card_instance_id: str,
        content: str,
        *,
        is_finalize: bool = False,
    ) -> None:
        """PUT /v1.0/card/streaming — low-level streaming call."""
        body = {
            "outTrackId": card_instance_id,
            "guid": uuid.uuid4().hex,
            "key": "msgContent",
            "content": content,
            "isFull": True,  # snapshot mode
            "isFinalize": is_finalize,
            "isError": False,
        }
        await self._api("PUT", "/v1.0/card/streaming", body)

    async def _api(
        self,
        method: str,
        endpoint: str,
        body: Dict[str, Any],
        *,
        timeout: int = 15,
    ) -> Optional[Dict[str, Any]]:
        """Generic async API call to api.dingtalk.com."""
        token = self._get_token()
        if not token:
            logger.warning("[dingtalk_card] No access token available")
            return None

        url = f"{DINGTALK_API}{endpoint}"
        headers = {
            "x-acs-dingtalk-access-token": token,
            "Content-Type": "application/json; charset=utf-8",
            "Accept": "application/json",
        }

        try:
            async with aiohttp.ClientSession() as session:
                async with session.request(
                    method,
                    url,
                    json=body,
                    headers=headers,
                    timeout=aiohttp.ClientTimeout(total=timeout),
                ) as resp:
                    text = await resp.text()
                    if resp.status >= 400:
                        logger.warning(
                            "[dingtalk_card] %s %s → HTTP %d: %s",
                            method, endpoint, resp.status, text[:300],
                        )
                        return None
                    return json.loads(text) if text else {}
        except Exception:
            logger.warning(
                "[dingtalk_card] %s %s failed", method, endpoint, exc_info=True,
            )
            return None


@dataclass
class _ThrottleState:
    """Per-card throttle bookkeeping."""

    interval: float = THROTTLE_INTERVAL_S
    last_sent: float = 0.0
    pending_content: Optional[str] = None


# ── DingTalkCardHandle ───────────────────────────────────────────────


class DingTalkCardHandle:
    """Convenience wrapper implementing the OutboundStreamHandle contract.

    Holds a card_instance_id + client reference and exposes simple
    ``update`` / ``close`` methods.
    """

    def __init__(
        self,
        client: DingTalkAICardClient,
        card_instance_id: str,
        stream_id: str = "",
    ) -> None:
        self._client = client
        self._card_instance_id = card_instance_id
        self._stream_id = stream_id or card_instance_id
        self._seq = 0

    # ── OutboundStreamHandle-compatible dict ─────────────────────────

    def as_handle(self) -> OutboundStreamHandle:
        """Return a TypedDict compatible with IMAdapter stream methods."""
        return OutboundStreamHandle(
            stream_id=self._stream_id,
            platform_handle=self._card_instance_id,
        )

    # ── convenience methods ──────────────────────────────────────────

    async def update(self, text: str) -> None:
        """Push a snapshot update (throttled)."""
        self._seq += 1
        await self._client.update_card(
            self._card_instance_id, text, seq=self._seq,
        )

    async def close(self, text: str) -> None:
        """Finalize the card with final content."""
        self._seq += 1
        await self._client.finalize_card(self._card_instance_id, text)

    @property
    def card_instance_id(self) -> str:
        return self._card_instance_id

    @property
    def stream_id(self) -> str:
        return self._stream_id
