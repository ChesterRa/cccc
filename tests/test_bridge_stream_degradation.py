"""Tests for bridge streaming graceful degradation (T196)."""

import unittest
from typing import Any, Dict, List, Optional
from unittest.mock import MagicMock

from cccc.ports.im.adapters.base import IMAdapter, OutboundStreamHandle


class FailBeginAdapter(IMAdapter):
    """Adapter where begin_stream raises an exception."""

    platform = "test"

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
        raise ConnectionError("DingTalk API unavailable")


class FailUpdateAdapter(IMAdapter):
    """Adapter where update_stream raises, but begin/end succeed."""

    platform = "test"

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
        return {"stream_id": stream_id, "platform_handle": f"ph_{stream_id}"}

    def update_stream(self, handle: OutboundStreamHandle, *, text: str = "", seq: int = 0) -> bool:
        raise RuntimeError("update failed")

    def end_stream(self, handle: OutboundStreamHandle, *, text: str = "") -> bool:
        return True


class FailEndAdapter(IMAdapter):
    """Adapter where end_stream raises."""

    platform = "test"

    def __init__(self) -> None:
        self.sent_messages: List[Dict[str, Any]] = []

    def connect(self) -> bool:
        return True

    def disconnect(self) -> None:
        pass

    def poll(self) -> list:
        return []

    def send_message(self, chat_id: str, text: str, thread_id: Optional[int] = None) -> bool:
        self.sent_messages.append({"chat_id": chat_id, "text": text, "thread_id": thread_id})
        return True

    def get_chat_title(self, chat_id: str) -> str:
        return "test"

    def begin_stream(self, chat_id: str, stream_id: str, *, text: str = "", thread_id: Optional[int] = None) -> Optional[OutboundStreamHandle]:
        return {"stream_id": stream_id, "platform_handle": f"ph_{stream_id}"}

    def update_stream(self, handle: OutboundStreamHandle, *, text: str = "", seq: int = 0) -> bool:
        return True

    def end_stream(self, handle: OutboundStreamHandle, *, text: str = "") -> bool:
        raise RuntimeError("end failed")


class TestBridgeStreamDegradation(unittest.TestCase):
    def _make_bridge(self, adapter: IMAdapter):
        import os
        import tempfile

        from cccc.kernel.group import create_group
        from cccc.kernel.registry import load_registry
        from cccc.ports.im.bridge import IMBridge

        old_home = os.environ.get("CCCC_HOME")
        td_ctx = tempfile.TemporaryDirectory()
        td = td_ctx.__enter__()
        os.environ["CCCC_HOME"] = td

        reg = load_registry()
        group = create_group(reg, title="degrade-test")

        bridge = IMBridge(group, adapter)
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

    def test_begin_stream_exception_degrades_gracefully(self) -> None:
        """begin_stream exception → no handle cached, update/end silently ignored."""
        adapter = FailBeginAdapter()
        bridge, cleanup = self._make_bridge(adapter)
        try:
            # begin_stream raises but should not propagate
            bridge._forward_stream_event({
                "kind": "chat.stream",
                "by": "agent-1",
                "data": {"op": "start", "stream_id": "s1", "text": "hello"},
            })
            self.assertNotIn("s1", bridge._active_streams)

            # update/end should silently skip (no handle cached)
            bridge._forward_stream_event({
                "kind": "chat.stream",
                "by": "agent-1",
                "data": {"op": "update", "stream_id": "s1", "text": "chunk", "seq": 1},
            })
            bridge._forward_stream_event({
                "kind": "chat.stream",
                "by": "agent-1",
                "data": {"op": "end", "stream_id": "s1", "text": "final"},
            })
            # No exception raised — degraded successfully
        finally:
            cleanup()

    def test_begin_stream_returns_none_degrades(self) -> None:
        """Default no-op adapter returns None → update/end silently ignored."""
        adapter = MagicMock(spec=IMAdapter)
        adapter.platform = "noop"
        adapter.begin_stream = MagicMock(return_value=None)
        adapter.update_stream = MagicMock()
        adapter.end_stream = MagicMock()

        bridge, cleanup = self._make_bridge(adapter)
        try:
            bridge._forward_stream_event({
                "kind": "chat.stream",
                "by": "agent-1",
                "data": {"op": "start", "stream_id": "s2", "text": ""},
            })
            self.assertNotIn("s2", bridge._active_streams)

            bridge._forward_stream_event({
                "kind": "chat.stream",
                "by": "agent-1",
                "data": {"op": "update", "stream_id": "s2", "text": "x", "seq": 1},
            })
            bridge._forward_stream_event({
                "kind": "chat.stream",
                "by": "agent-1",
                "data": {"op": "end", "stream_id": "s2", "text": "done"},
            })
            # update/end should NOT be called since no handle was cached
            adapter.update_stream.assert_not_called()
            adapter.end_stream.assert_not_called()
        finally:
            cleanup()

    def test_update_stream_exception_drops_frame(self) -> None:
        """update_stream exception → frame dropped, stream continues."""
        adapter = FailUpdateAdapter()
        bridge, cleanup = self._make_bridge(adapter)
        try:
            bridge._forward_stream_event({
                "kind": "chat.stream",
                "by": "agent-1",
                "data": {"op": "start", "stream_id": "s3", "text": ""},
            })
            self.assertIn("s3", bridge._active_streams)

            # update raises but should not propagate
            bridge._forward_stream_event({
                "kind": "chat.stream",
                "by": "agent-1",
                "data": {"op": "update", "stream_id": "s3", "text": "chunk", "seq": 1},
            })
            # Handle still cached — stream continues
            self.assertIn("s3", bridge._active_streams)

            # end should still work (calls adapter, keeps entry for dedup)
            bridge._forward_stream_event({
                "kind": "chat.stream",
                "by": "agent-1",
                "data": {"op": "end", "stream_id": "s3", "text": "final"},
            })
            self.assertNotIn("s3", bridge._active_streams)
            self.assertIn("s3", bridge._completed_stream_targets)
        finally:
            cleanup()

    def test_end_stream_exception_does_not_block(self) -> None:
        """end_stream exception should degrade back to final chat.message delivery."""
        adapter = FailEndAdapter()
        bridge, cleanup = self._make_bridge(adapter)
        try:
            bridge._forward_stream_event({
                "kind": "chat.stream",
                "by": "agent-1",
                "data": {"op": "start", "stream_id": "s4", "text": ""},
            })
            self.assertIn("s4", bridge._active_streams)

            # end raises but should not propagate
            bridge._forward_stream_event({
                "kind": "chat.stream",
                "by": "agent-1",
                "data": {"op": "end", "stream_id": "s4", "text": "final"},
            })
            self.assertNotIn("s4", bridge._active_streams)
            self.assertNotIn("s4", bridge._completed_stream_targets)

            bridge._forward_event({
                "kind": "chat.message",
                "by": "agent-1",
                "data": {"text": "final fallback", "to": ["user"], "stream_id": "s4", "attachments": []},
            })
            self.assertEqual(len(adapter.sent_messages), 1)
            self.assertEqual(adapter.sent_messages[0]["text"], "[agent-1] final fallback")
        finally:
            cleanup()


if __name__ == "__main__":
    unittest.main()
