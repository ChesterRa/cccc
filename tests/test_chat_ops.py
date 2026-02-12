import os
import tempfile
import unittest


class TestChatOps(unittest.TestCase):
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

    def test_try_handle_unknown_chat_op_returns_none(self) -> None:
        from cccc.daemon.ops.chat_ops import try_handle_chat_op

        self.assertIsNone(
            try_handle_chat_op(
                "not_chat",
                {},
                coerce_bool=lambda _: False,
                normalize_attachments=lambda _group, _raw: [],
                effective_runner_kind=lambda kind: kind,
                auto_wake_recipients=lambda _group, _to, _by: [],
                automation_on_resume=lambda _group: None,
                automation_on_new_message=lambda _group: None,
                clear_pending_system_notifies=lambda _group_id, _kinds: None,
            )
        )

    def test_attention_reply_still_writes_chat_ack(self) -> None:
        _, cleanup = self._with_home()
        try:
            create, _ = self._call("group_create", {"title": "chat-ops", "topic": "", "by": "user"})
            self.assertTrue(create.ok, getattr(create, "error", None))
            group_id = str((create.result or {}).get("group_id") or "").strip()
            self.assertTrue(group_id)

            add_1, _ = self._call(
                "actor_add",
                {
                    "group_id": group_id,
                    "by": "user",
                    "actor_id": "peer1",
                    "title": "Peer 1",
                    "runtime": "codex",
                    "runner": "headless",
                    "enabled": False,
                },
            )
            self.assertTrue(add_1.ok, getattr(add_1, "error", None))

            add_2, _ = self._call(
                "actor_add",
                {
                    "group_id": group_id,
                    "by": "user",
                    "actor_id": "peer2",
                    "title": "Peer 2",
                    "runtime": "codex",
                    "runner": "headless",
                    "enabled": False,
                },
            )
            self.assertTrue(add_2.ok, getattr(add_2, "error", None))

            send, _ = self._call(
                "send",
                {
                    "group_id": group_id,
                    "by": "peer1",
                    "to": ["peer2"],
                    "text": "ack me",
                    "priority": "attention",
                },
            )
            self.assertTrue(send.ok, getattr(send, "error", None))
            sent_event = (send.result or {}).get("event") if isinstance(send.result, dict) else {}
            self.assertIsInstance(sent_event, dict)
            assert isinstance(sent_event, dict)
            sent_event_id = str(sent_event.get("id") or "").strip()
            self.assertTrue(sent_event_id)

            reply, _ = self._call(
                "reply",
                {
                    "group_id": group_id,
                    "by": "peer2",
                    "reply_to": sent_event_id,
                    "text": "done",
                },
            )
            self.assertTrue(reply.ok, getattr(reply, "error", None))
            ack_event = (reply.result or {}).get("ack_event") if isinstance(reply.result, dict) else {}
            self.assertIsInstance(ack_event, dict)
            assert isinstance(ack_event, dict)
            self.assertEqual(str(ack_event.get("kind") or ""), "chat.ack")
        finally:
            cleanup()


if __name__ == "__main__":
    unittest.main()
