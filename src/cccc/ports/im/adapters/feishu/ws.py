"""Feishu WebSocket listener lifecycle."""

from __future__ import annotations

import threading
import traceback
from typing import Any, Callable, Dict, Optional

from .events import normalize_message_event


class FeishuWsListener:
    """Own the Feishu SDK WebSocket client and event dispatch wiring."""

    def __init__(
        self,
        *,
        app_id: str,
        app_secret: str,
        domain: str,
        lark: Any,
        ws_client_cls: Any,
        log_fn: Callable[[str], None],
        enqueue_fn: Callable[[Dict[str, Any]], None],
    ):
        self.app_id = app_id
        self.app_secret = app_secret
        self.domain = domain
        self.lark = lark
        self.ws_client_cls = ws_client_cls
        self.log_fn = log_fn
        self.enqueue_fn = enqueue_fn

        self.started = threading.Event()
        self.connect_error: Optional[str] = None
        self.thread: Optional[threading.Thread] = None
        self.client: Any = None

    def start(self) -> None:
        if self.thread and self.thread.is_alive():
            return

        self.connect_error = None
        self.started.clear()
        self.thread = threading.Thread(target=self._run, daemon=True)
        self.thread.start()

    def stop(self, *, join_timeout: float = 2.0) -> None:
        if self.client:
            try:
                self.client.stop()
            except Exception:
                pass
        if self.thread:
            self.thread.join(timeout=join_timeout)

    def is_alive(self) -> bool:
        return bool(self.thread and self.thread.is_alive())

    def _run(self) -> None:
        self.log_fn("[ws] Event listener starting...")

        def on_p2_im_message_receive_v1(data: Any) -> None:
            try:
                self.log_fn(f"[ws] Received event: {type(data).__name__}")
                event_data = normalize_message_event(data)
                message_data = event_data.get("message") if isinstance(event_data.get("message"), dict) else {}
                if message_data:
                    self.log_fn(
                        f"[ws] Chat: {message_data.get('chat_id', '')} "
                        f"type={message_data.get('message_type', '')}"
                    )

                self.enqueue_fn(
                    {
                        "header": {"event_type": "im.message.receive_v1"},
                        "event": event_data,
                    }
                )
                self.log_fn("[ws] Message enqueued")
            except Exception as e:
                self.log_fn(f"[ws] Event handler error: {e}")
                self.log_fn(f"[ws] Traceback: {traceback.format_exc()}")

        try:
            event_handler = (
                self.lark.EventDispatcherHandler.builder("", "")
                .register_p2_im_message_receive_v1(on_p2_im_message_receive_v1)
                .build()
            )

            self.client = self.ws_client_cls(
                app_id=self.app_id,
                app_secret=self.app_secret,
                event_handler=event_handler,
                log_level=self.lark.LogLevel.INFO,
                domain=self.domain,
            )

            self.log_fn("[ws] Starting Feishu SDK WebSocket client...")
            self.connect_error = None
            self.started.set()
            self.client.start()
        except Exception as e:
            self.connect_error = str(e)
            self.log_fn(f"[ws] SDK error: {self.connect_error}")
            self.log_fn(f"[ws] Traceback: {traceback.format_exc()}")
            self.started.set()

        self.log_fn("[ws] Event listener stopped")
