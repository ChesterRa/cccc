import os
import tempfile
import unittest
from unittest.mock import patch


class TestMessageObligation(unittest.TestCase):
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

    def _create_group_with_peer(self):
        from cccc.contracts.v1 import DaemonRequest
        from cccc.daemon.server import handle_request
        from cccc.kernel.actors import add_actor
        from cccc.kernel.group import load_group

        resp, _ = handle_request(
            DaemonRequest.model_validate(
                {"op": "group_create", "args": {"title": "t", "topic": "", "by": "user"}}
            )
        )
        self.assertTrue(resp.ok, getattr(resp, "error", None))
        group_id = str((resp.result or {}).get("group_id") or "").strip()
        self.assertTrue(group_id)

        group = load_group(group_id)
        self.assertIsNotNone(group)
        assert group is not None
        add_actor(group, actor_id="peer1", runtime="codex", runner="pty", enabled=True)
        return group, group_id

    def test_send_persists_reply_required(self) -> None:
        from cccc.contracts.v1 import DaemonRequest
        from cccc.daemon.server import handle_request

        _, cleanup = self._with_home()
        try:
            _group, group_id = self._create_group_with_peer()

            resp, _ = handle_request(
                DaemonRequest.model_validate(
                    {
                        "op": "send",
                        "args": {
                            "group_id": group_id,
                            "text": "please report status",
                            "by": "user",
                            "to": ["peer1"],
                            "priority": "attention",
                            "reply_required": True,
                        },
                    }
                )
            )
            self.assertTrue(resp.ok, getattr(resp, "error", None))
            event = (resp.result or {}).get("event") or {}
            data = event.get("data") if isinstance(event.get("data"), dict) else {}
            self.assertEqual(str(data.get("priority") or ""), "attention")
            self.assertTrue(bool(data.get("reply_required") is True))
        finally:
            cleanup()

    def test_obligation_status_lifecycle(self) -> None:
        from cccc.contracts.v1 import ChatMessageData
        from cccc.kernel.inbox import get_obligation_status_batch, set_cursor
        from cccc.kernel.ledger import append_event

        group, _group_id = (None, "")
        _, cleanup = self._with_home()
        try:
            group, _group_id = self._create_group_with_peer()

            msg = append_event(
                group.ledger_path,
                kind="chat.message",
                group_id=group.group_id,
                scope_key="",
                by="user",
                data=ChatMessageData(
                    text="need action",
                    to=["peer1"],
                    priority="attention",
                    reply_required=True,
                ).model_dump(),
            )
            msg_id = str(msg.get("id") or "")
            msg_ts = str(msg.get("ts") or "")
            self.assertTrue(msg_id)

            st1 = get_obligation_status_batch(group, [msg])
            peer1 = st1.get(msg_id, {}).get("peer1", {})
            self.assertEqual(peer1.get("read"), False)
            self.assertEqual(peer1.get("acked"), False)
            self.assertEqual(peer1.get("replied"), False)
            self.assertEqual(peer1.get("reply_required"), True)

            set_cursor(group, "peer1", event_id=msg_id, ts=msg_ts)
            st2 = get_obligation_status_batch(group, [msg])
            peer2 = st2.get(msg_id, {}).get("peer1", {})
            self.assertEqual(peer2.get("read"), True)
            self.assertEqual(peer2.get("acked"), True)
            self.assertEqual(peer2.get("replied"), False)

            append_event(
                group.ledger_path,
                kind="chat.message",
                group_id=group.group_id,
                scope_key="",
                by="peer1",
                data=ChatMessageData(
                    text="done",
                    to=["user"],
                    reply_to=msg_id,
                ).model_dump(),
            )

            st3 = get_obligation_status_batch(group, [msg])
            peer3 = st3.get(msg_id, {}).get("peer1", {})
            self.assertEqual(peer3.get("replied"), True)
            self.assertEqual(peer3.get("acked"), True)
        finally:
            cleanup()

    def test_obligation_status_uses_indexed_ack_and_reply_lookup(self) -> None:
        from cccc.contracts.v1 import ChatMessageData
        from cccc.kernel.inbox import get_obligation_status_batch
        from cccc.kernel.ledger import append_event

        _, cleanup = self._with_home()
        try:
            group, _group_id = self._create_group_with_peer()

            msg = append_event(
                group.ledger_path,
                kind="chat.message",
                group_id=group.group_id,
                scope_key="",
                by="user",
                data=ChatMessageData(
                    text="need indexed status",
                    to=["peer1"],
                    priority="attention",
                    reply_required=True,
                ).model_dump(),
            )
            msg_id = str(msg.get("id") or "")
            self.assertTrue(msg_id)

            append_event(
                group.ledger_path,
                kind="chat.ack",
                group_id=group.group_id,
                scope_key="",
                by="peer1",
                data={"actor_id": "peer1", "event_id": msg_id},
            )
            append_event(
                group.ledger_path,
                kind="chat.message",
                group_id=group.group_id,
                scope_key="",
                by="peer1",
                data=ChatMessageData(text="done", to=["user"], reply_to=msg_id).model_dump(),
            )

            with patch("cccc.kernel.inbox.iter_events", side_effect=AssertionError("status lookup should use ledger index")):
                status = get_obligation_status_batch(group, [msg])

            peer = status.get(msg_id, {}).get("peer1", {})
            self.assertEqual(peer.get("acked"), True)
            self.assertEqual(peer.get("replied"), True)
            self.assertEqual(peer.get("reply_required"), True)
        finally:
            cleanup()

    def test_reply_auto_creates_ack_for_attention_message(self) -> None:
        from cccc.contracts.v1 import DaemonRequest
        from cccc.daemon.server import handle_request
        from cccc.kernel.inbox import has_chat_ack
        from cccc.kernel.ledger import append_event

        _, cleanup = self._with_home()
        try:
            group, group_id = self._create_group_with_peer()

            original = append_event(
                group.ledger_path,
                kind="chat.message",
                group_id=group.group_id,
                scope_key="",
                by="user",
                data={
                    "text": "confirm and execute",
                    "to": ["peer1"],
                    "priority": "attention",
                },
            )
            original_id = str(original.get("id") or "")
            self.assertTrue(original_id)

            resp, _ = handle_request(
                DaemonRequest.model_validate(
                    {
                        "op": "reply",
                        "args": {
                            "group_id": group_id,
                            "reply_to": original_id,
                            "text": "ack and done",
                            "by": "peer1",
                            "to": ["user"],
                        },
                    }
                )
            )
            self.assertTrue(resp.ok, getattr(resp, "error", None))
            result = resp.result or {}
            ack_event = result.get("ack_event") if isinstance(result, dict) else None
            self.assertIsInstance(ack_event, dict)
            assert isinstance(ack_event, dict)
            self.assertEqual(str(ack_event.get("kind") or ""), "chat.ack")
            ack_data = ack_event.get("data") if isinstance(ack_event.get("data"), dict) else {}
            self.assertEqual(str(ack_data.get("actor_id") or ""), "peer1")
            self.assertEqual(str(ack_data.get("event_id") or ""), original_id)
            self.assertTrue(has_chat_ack(group, event_id=original_id, actor_id="peer1"))
        finally:
            cleanup()


if __name__ == "__main__":
    unittest.main()
