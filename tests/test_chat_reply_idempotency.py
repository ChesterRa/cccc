import json
import os
import shutil
import tempfile
import time
import unittest
from unittest import mock


class TestChatReplyIdempotency(unittest.TestCase):
    def _with_home(self):
        old_home = os.environ.get("CCCC_HOME")
        td_ctx = tempfile.TemporaryDirectory()
        td = td_ctx.__enter__()
        os.environ["CCCC_HOME"] = td

        def cleanup() -> None:
            if old_home is None:
                os.environ.pop("CCCC_HOME", None)
            else:
                os.environ["CCCC_HOME"] = old_home
            for attempt in range(5):
                try:
                    shutil.rmtree(td)
                    break
                except FileNotFoundError:
                    break
                except OSError:
                    if attempt >= 4:
                        raise
                    time.sleep(0.05)

        return td, cleanup

    def _call(self, op: str, args: dict):
        from cccc.contracts.v1 import DaemonRequest
        from cccc.daemon.server import handle_request

        return handle_request(DaemonRequest.model_validate({"op": op, "args": args}))

    def test_reply_replay_with_same_client_id_returns_existing_event(self) -> None:
        from cccc.kernel.group import load_group
        from cccc.kernel.ledger import read_last_lines

        _, cleanup = self._with_home()
        try:
            created, _ = self._call("group_create", {"title": "reply-idempotency", "topic": "", "by": "user"})
            self.assertTrue(created.ok, getattr(created, "error", None))
            group_id = str((created.result or {}).get("group_id") or "").strip()
            self.assertTrue(group_id)

            original, _ = self._call(
                "send",
                {
                    "group_id": group_id,
                    "by": "user",
                    "to": ["user"],
                    "text": "original",
                },
            )
            self.assertTrue(original.ok, getattr(original, "error", None))
            original_event = (original.result or {}).get("event") if isinstance(original.result, dict) else {}
            self.assertIsInstance(original_event, dict)
            reply_to = str(original_event.get("id") or "").strip()
            self.assertTrue(reply_to)

            payload = {
                "group_id": group_id,
                "by": "user",
                "reply_to": reply_to,
                "to": ["user"],
                "text": "retry-safe reply",
                "client_id": "reply-retry-1",
            }
            first, _ = self._call("reply", dict(payload))
            second, _ = self._call("reply", dict(payload))

            self.assertTrue(first.ok, getattr(first, "error", None))
            self.assertTrue(second.ok, getattr(second, "error", None))
            first_event = (first.result or {}).get("event") if isinstance(first.result, dict) else {}
            second_event = (second.result or {}).get("event") if isinstance(second.result, dict) else {}
            self.assertEqual(first_event.get("id"), second_event.get("id"))
            self.assertTrue((second.result or {}).get("replayed"))

            group = load_group(group_id)
            self.assertIsNotNone(group)
            assert group is not None
            matching_replies = 0
            for raw_line in read_last_lines(group.ledger_path, 50):
                event = json.loads(raw_line)
                data = event.get("data") if isinstance(event.get("data"), dict) else {}
                if data.get("reply_to") == reply_to and data.get("text") == "retry-safe reply":
                    matching_replies += 1
            self.assertEqual(matching_replies, 1)
        finally:
            cleanup()

    def test_reply_replay_with_prefixed_reply_to_uses_existing_event(self) -> None:
        from cccc.kernel.group import load_group
        from cccc.kernel.ledger import read_last_lines

        _, cleanup = self._with_home()
        try:
            created, _ = self._call("group_create", {"title": "reply-prefix-idempotency", "topic": "", "by": "user"})
            self.assertTrue(created.ok, getattr(created, "error", None))
            group_id = str((created.result or {}).get("group_id") or "").strip()
            self.assertTrue(group_id)

            original, _ = self._call(
                "send",
                {
                    "group_id": group_id,
                    "by": "user",
                    "to": ["user"],
                    "text": "original",
                },
            )
            self.assertTrue(original.ok, getattr(original, "error", None))
            original_event = (original.result or {}).get("event") if isinstance(original.result, dict) else {}
            self.assertIsInstance(original_event, dict)
            reply_to = str(original_event.get("id") or "").strip()
            self.assertTrue(reply_to)
            reply_to_prefix = reply_to[:12]

            payload = {
                "group_id": group_id,
                "by": "user",
                "reply_to": reply_to_prefix,
                "to": ["user"],
                "text": "retry-safe prefix reply",
                "client_id": "reply-retry-prefix-1",
            }
            first, _ = self._call("reply", dict(payload))
            second, _ = self._call("reply", dict(payload))

            self.assertTrue(first.ok, getattr(first, "error", None))
            self.assertTrue(second.ok, getattr(second, "error", None))
            first_event = (first.result or {}).get("event") if isinstance(first.result, dict) else {}
            second_event = (second.result or {}).get("event") if isinstance(second.result, dict) else {}
            self.assertEqual(first_event.get("id"), second_event.get("id"))
            self.assertTrue((second.result or {}).get("replayed"))

            group = load_group(group_id)
            self.assertIsNotNone(group)
            assert group is not None
            matching_replies = 0
            for raw_line in read_last_lines(group.ledger_path, 50):
                event = json.loads(raw_line)
                data = event.get("data") if isinstance(event.get("data"), dict) else {}
                if data.get("reply_to") == reply_to and data.get("text") == "retry-safe prefix reply":
                    matching_replies += 1
            self.assertEqual(matching_replies, 1)
        finally:
            cleanup()

    def test_reply_replay_with_full_reply_to_short_circuits_before_event_resolution(self) -> None:
        _, cleanup = self._with_home()
        try:
            created, _ = self._call("group_create", {"title": "reply-full-id-short-circuit", "topic": "", "by": "user"})
            self.assertTrue(created.ok, getattr(created, "error", None))
            group_id = str((created.result or {}).get("group_id") or "").strip()
            self.assertTrue(group_id)

            original, _ = self._call(
                "send",
                {
                    "group_id": group_id,
                    "by": "user",
                    "to": ["user"],
                    "text": "original",
                },
            )
            self.assertTrue(original.ok, getattr(original, "error", None))
            original_event = (original.result or {}).get("event") if isinstance(original.result, dict) else {}
            self.assertIsInstance(original_event, dict)
            reply_to = str(original_event.get("id") or "").strip()
            self.assertTrue(reply_to)

            payload = {
                "group_id": group_id,
                "by": "user",
                "reply_to": reply_to,
                "to": ["user"],
                "text": "retry-safe reply",
                "client_id": "reply-retry-full-short-circuit",
            }
            first, _ = self._call("reply", dict(payload))
            self.assertTrue(first.ok, getattr(first, "error", None))

            with mock.patch(
                "cccc.daemon.messaging.chat_ops.find_event_with_chat_ack",
                side_effect=AssertionError("full-id replay should use raw idempotency lookup first"),
            ):
                second, _ = self._call("reply", dict(payload))

            self.assertTrue(second.ok, getattr(second, "error", None))
            self.assertTrue((second.result or {}).get("replayed"))
        finally:
            cleanup()

    def test_repeated_reply_without_client_id_writes_distinct_events(self) -> None:
        from cccc.kernel.group import load_group
        from cccc.kernel.ledger import read_last_lines

        _, cleanup = self._with_home()
        try:
            created, _ = self._call("group_create", {"title": "reply-no-client-id", "topic": "", "by": "user"})
            self.assertTrue(created.ok, getattr(created, "error", None))
            group_id = str((created.result or {}).get("group_id") or "").strip()
            self.assertTrue(group_id)

            original, _ = self._call(
                "send",
                {
                    "group_id": group_id,
                    "by": "user",
                    "to": ["user"],
                    "text": "original",
                },
            )
            self.assertTrue(original.ok, getattr(original, "error", None))
            original_event = (original.result or {}).get("event") if isinstance(original.result, dict) else {}
            self.assertIsInstance(original_event, dict)
            reply_to = str(original_event.get("id") or "").strip()
            self.assertTrue(reply_to)

            payload = {
                "group_id": group_id,
                "by": "user",
                "reply_to": reply_to,
                "to": ["user"],
                "text": "same visible reply",
            }
            first, _ = self._call("reply", dict(payload))
            second, _ = self._call("reply", dict(payload))

            self.assertTrue(first.ok, getattr(first, "error", None))
            self.assertTrue(second.ok, getattr(second, "error", None))
            first_event = (first.result or {}).get("event") if isinstance(first.result, dict) else {}
            second_event = (second.result or {}).get("event") if isinstance(second.result, dict) else {}
            self.assertNotEqual(first_event.get("id"), second_event.get("id"))
            self.assertFalse((second.result or {}).get("replayed"))

            group = load_group(group_id)
            self.assertIsNotNone(group)
            assert group is not None
            matching_replies = 0
            for raw_line in read_last_lines(group.ledger_path, 50):
                event = json.loads(raw_line)
                data = event.get("data") if isinstance(event.get("data"), dict) else {}
                if data.get("reply_to") == reply_to and data.get("text") == "same visible reply":
                    matching_replies += 1
            self.assertEqual(matching_replies, 2)
        finally:
            cleanup()


if __name__ == "__main__":
    unittest.main()
