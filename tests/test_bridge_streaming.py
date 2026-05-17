"""Tests for IMBridge streaming event handling (T193)."""

import unittest
from typing import Any, Dict, List, Optional
from unittest.mock import MagicMock, patch

from cccc.ports.im.adapters.base import IMAdapter, OutboundStreamHandle


class FakeStreamAdapter(IMAdapter):
    """Minimal adapter that supports streaming for testing."""

    platform = "test"

    def __init__(self):
        self.streams_started: List[Dict[str, Any]] = []
        self.streams_updated: List[Dict[str, Any]] = []
        self.streams_ended: List[Dict[str, Any]] = []

    def connect(self) -> bool:
        return True

    def disconnect(self) -> None:
        pass

    def poll(self) -> list:
        return []

    def send_message(self, chat_id: str, text: str, thread_id: Optional[int] = None) -> bool:
        return True

    def get_chat_title(self, chat_id: str) -> str:
        return "test"

    def begin_stream(self, chat_id: str, stream_id: str, *, text: str = "", thread_id: Optional[int] = None) -> Optional[OutboundStreamHandle]:
        handle: OutboundStreamHandle = {"stream_id": stream_id, "platform_handle": f"ph_{stream_id}"}
        self.streams_started.append({"chat_id": chat_id, "stream_id": stream_id, "text": text})
        return handle

    def update_stream(self, handle: OutboundStreamHandle, *, text: str = "", seq: int = 0) -> bool:
        self.streams_updated.append({"handle": handle, "text": text, "seq": seq})
        return True

    def end_stream(self, handle: OutboundStreamHandle, *, text: str = "") -> bool:
        self.streams_ended.append({"handle": handle, "text": text})
        return True


class TestBridgeStreamForwarding(unittest.TestCase):
    """Test _forward_stream_event and stream_id skip logic in _forward_event."""

    def _make_bridge(self, adapter: IMAdapter):
        """Create a minimal IMBridge with mocked dependencies."""
        import os
        import tempfile

        from cccc.kernel.group import create_group
        from cccc.kernel.registry import load_registry
        from cccc.ports.im.bridge import IMBridge

        old_home = os.environ.get("CCCC_HOME")
        td_ctx = tempfile.TemporaryDirectory(ignore_cleanup_errors=True)
        td = td_ctx.__enter__()
        os.environ["CCCC_HOME"] = td

        reg = load_registry()
        group = create_group(reg, title="bridge-stream-test")

        bridge = IMBridge(group, adapter)
        # Mock subscriber and key_manager to authorize a test chat
        sub = MagicMock()
        sub.chat_id = "chat1"
        sub.thread_id = 0
        sub.verbose = True
        bridge.subscribers.get_subscribed_targets = MagicMock(return_value=[sub])
        bridge.key_manager.is_authorized = MagicMock(return_value=True)

        def cleanup():
            td_ctx.__exit__(None, None, None)
            if old_home is None:
                os.environ.pop("CCCC_HOME", None)
            else:
                os.environ["CCCC_HOME"] = old_home

        return bridge, cleanup

    def test_forward_stream_start_caches_handle(self) -> None:
        adapter = FakeStreamAdapter()
        bridge, cleanup = self._make_bridge(adapter)
        try:
            bridge._forward_stream_event({
                "kind": "chat.stream",
                "by": "agent-1",
                "data": {"op": "start", "stream_id": "s1", "text": "hello"},
            })
            self.assertEqual(len(adapter.streams_started), 1)
            self.assertEqual(adapter.streams_started[0]["stream_id"], "s1")
            self.assertIn("s1", bridge._active_streams)
        finally:
            cleanup()

    def test_forward_stream_update_uses_cached_handle(self) -> None:
        adapter = FakeStreamAdapter()
        bridge, cleanup = self._make_bridge(adapter)
        try:
            # Start first
            bridge._forward_stream_event({
                "kind": "chat.stream",
                "by": "agent-1",
                "data": {"op": "start", "stream_id": "s2", "text": ""},
            })
            # Update
            bridge._forward_stream_event({
                "kind": "chat.stream",
                "by": "agent-1",
                "data": {"op": "update", "stream_id": "s2", "text": "chunk", "seq": 1},
            })
            self.assertEqual(len(adapter.streams_updated), 1)
            self.assertEqual(adapter.streams_updated[0]["text"], "chunk")
            self.assertEqual(adapter.streams_updated[0]["seq"], 1)
        finally:
            cleanup()

    def test_forward_stream_end_calls_adapter(self) -> None:
        adapter = FakeStreamAdapter()
        bridge, cleanup = self._make_bridge(adapter)
        try:
            bridge._forward_stream_event({
                "kind": "chat.stream",
                "by": "agent-1",
                "data": {"op": "start", "stream_id": "s3", "text": ""},
            })
            self.assertIn("s3", bridge._active_streams)

            bridge._forward_stream_event({
                "kind": "chat.stream",
                "by": "agent-1",
                "data": {"op": "end", "stream_id": "s3", "text": "final"},
            })
            # end success moves the target into final-message dedup tracking
            self.assertNotIn("s3", bridge._active_streams)
            self.assertIn("s3", bridge._completed_stream_targets)
            self.assertEqual(len(adapter.streams_ended), 1)
            self.assertEqual(adapter.streams_ended[0]["text"], "final")
        finally:
            cleanup()

    def test_forward_stream_end_with_empty_text_closes_adapter_and_keeps_fallback(self) -> None:
        adapter = FakeStreamAdapter()
        bridge, cleanup = self._make_bridge(adapter)
        try:
            adapter.send_message = MagicMock(return_value=True)
            bridge._forward_stream_event({
                "kind": "chat.stream",
                "by": "agent-1",
                "data": {"op": "start", "stream_id": "s3-empty", "text": ""},
            })
            self.assertIn("s3-empty", bridge._active_streams)

            bridge._forward_stream_event({
                "kind": "chat.stream",
                "by": "agent-1",
                "data": {"op": "end", "stream_id": "s3-empty", "text": ""},
            })

            self.assertNotIn("s3-empty", bridge._active_streams)
            self.assertNotIn("s3-empty", bridge._completed_stream_targets)
            self.assertEqual(len(adapter.streams_ended), 1)
            self.assertEqual(adapter.streams_ended[0]["text"], "")

            bridge._forward_event({
                "kind": "chat.message",
                "by": "agent-1",
                "data": {
                    "text": "final fallback",
                    "to": ["user"],
                    "stream_id": "s3-empty",
                    "attachments": [],
                },
            })
            adapter.send_message.assert_called_once()
        finally:
            cleanup()

    def test_forward_event_routes_stream_to_handler(self) -> None:
        adapter = FakeStreamAdapter()
        bridge, cleanup = self._make_bridge(adapter)
        try:
            event = {
                "kind": "chat.stream",
                "by": "agent-1",
                "data": {"op": "start", "stream_id": "s4", "text": "routed"},
            }
            bridge._forward_event(event)
            self.assertEqual(len(adapter.streams_started), 1)
            self.assertEqual(adapter.streams_started[0]["stream_id"], "s4")
        finally:
            cleanup()

    def test_chat_message_with_stream_id_skipped_for_streamed_target(self) -> None:
        """chat.message with stream_id is skipped for targets that had a successful stream."""
        adapter = FakeStreamAdapter()
        bridge, cleanup = self._make_bridge(adapter)
        try:
            adapter.send_message = MagicMock(return_value=True)
            # First, start and successfully end a stream so the target is eligible
            # to skip the final plain-text fallback.
            bridge._forward_stream_event({
                "kind": "chat.stream",
                "by": "agent-1",
                "data": {"op": "start", "stream_id": "s5", "text": ""},
            })
            bridge._forward_stream_event({
                "kind": "chat.stream",
                "by": "agent-1",
                "data": {"op": "end", "stream_id": "s5", "text": "final streamed"},
            })
            self.assertIn("s5", bridge._completed_stream_targets)

            # Now forward the final chat.message with matching stream_id
            event = {
                "kind": "chat.message",
                "by": "agent-1",
                "data": {
                    "text": "final message",
                    "to": ["user"],
                    "stream_id": "s5",
                    "attachments": [],
                },
            }
            bridge._forward_event(event)
            # send_message should NOT be called — target already received via stream
            adapter.send_message.assert_not_called()
            # stream bookkeeping cleaned up by chat.message handler
            self.assertNotIn("s5", bridge._active_streams)
            self.assertNotIn("s5", bridge._completed_stream_targets)
        finally:
            cleanup()

    def test_chat_message_without_stream_id_forwarded(self) -> None:
        adapter = FakeStreamAdapter()
        bridge, cleanup = self._make_bridge(adapter)
        try:
            adapter.send_message = MagicMock(return_value=True)
            event = {
                "kind": "chat.message",
                "by": "agent-1",
                "data": {
                    "text": "normal message",
                    "to": ["user"],
                    "attachments": [],
                },
            }
            bridge._forward_event(event)
            adapter.send_message.assert_called_once()
        finally:
            cleanup()


if __name__ == "__main__":
    unittest.main()
