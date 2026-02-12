import os
import tempfile
import unittest


class TestInboxReadOps(unittest.TestCase):
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

    def _call(self, op: str, args: dict):
        from cccc.contracts.v1 import DaemonRequest
        from cccc.daemon.server import handle_request

        return handle_request(DaemonRequest.model_validate({"op": op, "args": args}))

    def test_inbox_mark_read_emits_chat_ack_for_attention(self) -> None:
        _, cleanup = self._with_home()
        try:
            create, _ = self._call("group_create", {"title": "inbox-read", "topic": "", "by": "user"})
            self.assertTrue(create.ok, getattr(create, "error", None))
            group_id = str((create.result or {}).get("group_id") or "").strip()
            self.assertTrue(group_id)

            attach, _ = self._call("attach", {"group_id": group_id, "path": ".", "by": "user"})
            self.assertTrue(attach.ok, getattr(attach, "error", None))

            add, _ = self._call(
                "actor_add",
                {
                    "group_id": group_id,
                    "actor_id": "peer1",
                    "title": "Peer 1",
                    "runtime": "codex",
                    "runner": "headless",
                    "by": "user",
                },
            )
            self.assertTrue(add.ok, getattr(add, "error", None))

            sent, _ = self._call(
                "send",
                {
                    "group_id": group_id,
                    "by": "user",
                    "to": ["peer1"],
                    "text": "attention ping",
                    "priority": "attention",
                },
            )
            self.assertTrue(sent.ok, getattr(sent, "error", None))
            sent_event = (sent.result or {}).get("event") if isinstance(sent.result, dict) else {}
            self.assertIsInstance(sent_event, dict)
            assert isinstance(sent_event, dict)
            event_id = str(sent_event.get("id") or "").strip()
            self.assertTrue(event_id)

            inbox, _ = self._call("inbox_list", {"group_id": group_id, "actor_id": "peer1", "by": "peer1", "limit": 10})
            self.assertTrue(inbox.ok, getattr(inbox, "error", None))
            messages = (inbox.result or {}).get("messages") if isinstance(inbox.result, dict) else []
            self.assertIsInstance(messages, list)
            assert isinstance(messages, list)
            self.assertTrue(any(str(item.get("id") or "") == event_id for item in messages if isinstance(item, dict)))

            marked, _ = self._call(
                "inbox_mark_read",
                {"group_id": group_id, "actor_id": "peer1", "event_id": event_id, "by": "peer1"},
            )
            self.assertTrue(marked.ok, getattr(marked, "error", None))
            ack_event = (marked.result or {}).get("ack_event") if isinstance(marked.result, dict) else {}
            self.assertIsInstance(ack_event, dict)
            assert isinstance(ack_event, dict)
            self.assertEqual(str(ack_event.get("kind") or ""), "chat.ack")
        finally:
            cleanup()

    def test_chat_ack_idempotent_and_mark_all_read(self) -> None:
        _, cleanup = self._with_home()
        try:
            create, _ = self._call("group_create", {"title": "inbox-ack", "topic": "", "by": "user"})
            self.assertTrue(create.ok, getattr(create, "error", None))
            group_id = str((create.result or {}).get("group_id") or "").strip()
            self.assertTrue(group_id)

            attach, _ = self._call("attach", {"group_id": group_id, "path": ".", "by": "user"})
            self.assertTrue(attach.ok, getattr(attach, "error", None))

            add, _ = self._call(
                "actor_add",
                {
                    "group_id": group_id,
                    "actor_id": "peer1",
                    "title": "Peer 1",
                    "runtime": "codex",
                    "runner": "headless",
                    "by": "user",
                },
            )
            self.assertTrue(add.ok, getattr(add, "error", None))

            attention, _ = self._call(
                "send",
                {
                    "group_id": group_id,
                    "by": "user",
                    "to": ["peer1"],
                    "text": "attention task",
                    "priority": "attention",
                },
            )
            self.assertTrue(attention.ok, getattr(attention, "error", None))
            attention_event = (attention.result or {}).get("event") if isinstance(attention.result, dict) else {}
            self.assertIsInstance(attention_event, dict)
            assert isinstance(attention_event, dict)
            attention_event_id = str(attention_event.get("id") or "").strip()
            self.assertTrue(attention_event_id)

            ack1, _ = self._call(
                "chat_ack",
                {"group_id": group_id, "actor_id": "peer1", "event_id": attention_event_id, "by": "peer1"},
            )
            self.assertTrue(ack1.ok, getattr(ack1, "error", None))
            self.assertFalse(bool((ack1.result or {}).get("already")))

            ack2, _ = self._call(
                "chat_ack",
                {"group_id": group_id, "actor_id": "peer1", "event_id": attention_event_id, "by": "peer1"},
            )
            self.assertTrue(ack2.ok, getattr(ack2, "error", None))
            self.assertTrue(bool((ack2.result or {}).get("already")))

            normal, _ = self._call(
                "send",
                {"group_id": group_id, "by": "user", "to": ["peer1"], "text": "normal ping"},
            )
            self.assertTrue(normal.ok, getattr(normal, "error", None))

            mark_all, _ = self._call("inbox_mark_all_read", {"group_id": group_id, "actor_id": "peer1", "by": "peer1"})
            self.assertTrue(mark_all.ok, getattr(mark_all, "error", None))
            mark_event = (mark_all.result or {}).get("event") if isinstance(mark_all.result, dict) else {}
            self.assertIsInstance(mark_event, dict)
            assert isinstance(mark_event, dict)
            self.assertEqual(str(mark_event.get("kind") or ""), "chat.read")

            inbox, _ = self._call("inbox_list", {"group_id": group_id, "actor_id": "peer1", "by": "peer1", "limit": 10})
            self.assertTrue(inbox.ok, getattr(inbox, "error", None))
            messages = (inbox.result or {}).get("messages") if isinstance(inbox.result, dict) else []
            self.assertIsInstance(messages, list)
            assert isinstance(messages, list)
            self.assertEqual(messages, [])
        finally:
            cleanup()


if __name__ == "__main__":
    unittest.main()
