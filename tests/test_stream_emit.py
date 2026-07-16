"""Tests for handle_stream_emit daemon op (T192)."""

import json
import os
import tempfile
import unittest

from cccc.contracts.v1 import ChatStreamData


class TestStreamEmit(unittest.TestCase):
    def _with_home(self):
        old_home = os.environ.get("CCCC_HOME")
        td_ctx = tempfile.TemporaryDirectory()
        td = td_ctx.__enter__()
        os.environ["CCCC_HOME"] = td

        def cleanup() -> None:
            td_ctx.__exit__(None, None, None)
            if old_home is None:
                os.environ.pop("CCCC_HOME", None)
            else:
                os.environ["CCCC_HOME"] = old_home

        return td, cleanup

    def test_stream_start_generates_stream_id(self) -> None:
        from cccc.daemon.messaging.chat_ops import handle_stream_emit
        from cccc.kernel.group import create_group
        from cccc.kernel.registry import load_registry

        _, cleanup = self._with_home()
        try:
            reg = load_registry()
            group = create_group(reg, title="stream-test")
            resp = handle_stream_emit({
                "group_id": group.group_id,
                "by": "agent-1",
                "op": "start",
                "text": "hello",
            })
            self.assertTrue(resp.ok)
            result = resp.result
            self.assertIn("stream_id", result)
            self.assertTrue(len(result["stream_id"]) > 0)
            event = result["event"]
            self.assertEqual(event["kind"], "chat.stream")
            data = event["data"]
            self.assertEqual(data["op"], "start")
            self.assertEqual(data["stream_id"], result["stream_id"])
            self.assertEqual(data["text"], "hello")
        finally:
            cleanup()

    def test_stream_update_requires_stream_id(self) -> None:
        from cccc.daemon.messaging.chat_ops import handle_stream_emit
        from cccc.kernel.group import create_group
        from cccc.kernel.registry import load_registry

        _, cleanup = self._with_home()
        try:
            reg = load_registry()
            group = create_group(reg, title="stream-test")
            resp = handle_stream_emit({
                "group_id": group.group_id,
                "by": "agent-1",
                "op": "update",
                "text": "chunk",
            })
            self.assertFalse(resp.ok)
            self.assertEqual(resp.error.code, "missing_stream_id")
        finally:
            cleanup()

    def test_stream_update_and_end(self) -> None:
        from cccc.daemon.messaging.chat_ops import handle_stream_emit
        from cccc.kernel.group import create_group
        from cccc.kernel.registry import load_registry

        _, cleanup = self._with_home()
        try:
            reg = load_registry()
            group = create_group(reg, title="stream-test")

            # Start
            start_resp = handle_stream_emit({
                "group_id": group.group_id,
                "by": "agent-1",
                "op": "start",
                "text": "",
            })
            self.assertTrue(start_resp.ok)
            stream_id = start_resp.result["stream_id"]

            # Update
            update_resp = handle_stream_emit({
                "group_id": group.group_id,
                "by": "agent-1",
                "op": "update",
                "stream_id": stream_id,
                "text": "partial content",
                "seq": 1,
            })
            self.assertTrue(update_resp.ok)
            self.assertEqual(update_resp.result["stream_id"], stream_id)
            self.assertEqual(update_resp.result["event"]["data"]["seq"], 1)

            # End
            end_resp = handle_stream_emit({
                "group_id": group.group_id,
                "by": "agent-1",
                "op": "end",
                "stream_id": stream_id,
                "text": "final content",
            })
            self.assertTrue(end_resp.ok)
            self.assertEqual(end_resp.result["event"]["data"]["op"], "end")
            self.assertEqual(end_resp.result["event"]["data"]["text"], "final content")
        finally:
            cleanup()

    def test_stream_invalid_op(self) -> None:
        from cccc.daemon.messaging.chat_ops import handle_stream_emit
        from cccc.kernel.group import create_group
        from cccc.kernel.registry import load_registry

        _, cleanup = self._with_home()
        try:
            reg = load_registry()
            group = create_group(reg, title="stream-test")
            resp = handle_stream_emit({
                "group_id": group.group_id,
                "by": "agent-1",
                "op": "invalid",
            })
            self.assertFalse(resp.ok)
            self.assertEqual(resp.error.code, "invalid_op")
        finally:
            cleanup()

    def test_stream_events_written_to_ledger(self) -> None:
        from cccc.daemon.messaging.chat_ops import handle_stream_emit
        from cccc.kernel.group import create_group
        from cccc.kernel.inbox import iter_events
        from cccc.kernel.registry import load_registry

        _, cleanup = self._with_home()
        try:
            reg = load_registry()
            group = create_group(reg, title="stream-test")

            handle_stream_emit({
                "group_id": group.group_id,
                "by": "agent-1",
                "op": "start",
                "text": "begin",
            })

            events = list(iter_events(group.ledger_path))
            stream_events = [e for e in events if e.get("kind") == "chat.stream"]
            self.assertEqual(len(stream_events), 1)
            self.assertEqual(stream_events[0]["data"]["op"], "start")
        finally:
            cleanup()


if __name__ == "__main__":
    unittest.main()
