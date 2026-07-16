import json
import os
import tempfile
import time
import unittest


class TestDeliveryAutoMarkReadEvent(unittest.TestCase):
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
            for attempt in range(4):
                try:
                    td_ctx.__exit__(None, None, None)
                    return
                except OSError as exc:
                    if getattr(exc, "errno", None) != 66 or attempt == 3:
                        raise
                    time.sleep(0.05 * (attempt + 1))

        return td, cleanup

    def _call(self, op: str, args: dict):
        from cccc.contracts.v1 import DaemonRequest
        from cccc.daemon.server import handle_request

        return handle_request(DaemonRequest.model_validate({"op": op, "args": args}))

    def _create_group_with_actor(self) -> str:
        create, _ = self._call("group_create", {"title": "delivery-auto-mark", "topic": "", "by": "user"})
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
        return group_id

    def _ledger_events(self, group) -> list[dict]:
        events: list[dict] = []
        with group.ledger_path.open("r", encoding="utf-8") as fh:
            for line in fh:
                line = line.strip()
                if not line:
                    continue
                obj = json.loads(line)
                if isinstance(obj, dict):
                    events.append(obj)
        return events

    def test_finalize_delivery_success_emits_chat_read_when_auto_mark_advances_cursor(self) -> None:
        from cccc.daemon.messaging.delivery import PendingMessage, _finalize_delivery_success
        from cccc.kernel.group import load_group
        from cccc.kernel.inbox import get_cursor

        _, cleanup = self._with_home()
        try:
            group_id = self._create_group_with_actor()
            sent, _ = self._call(
                "send",
                {
                    "group_id": group_id,
                    "by": "user",
                    "to": ["peer1"],
                    "text": "hello peer1",
                },
            )
            self.assertTrue(sent.ok, getattr(sent, "error", None))
            event = (sent.result or {}).get("event") if isinstance(sent.result, dict) else {}
            self.assertIsInstance(event, dict)
            assert isinstance(event, dict)

            group = load_group(group_id)
            self.assertIsNotNone(group)
            assert group is not None
            group.doc["delivery"] = {"auto_mark_on_delivery": True}

            _finalize_delivery_success(
                group,
                actor_id="peer1",
                chat_total=1,
                deliverable=[
                    PendingMessage(
                        event_id=str(event.get("id") or ""),
                        by="user",
                        to=["peer1"],
                        text="hello peer1",
                        ts=str(event.get("ts") or ""),
                        kind="chat.message",
                    )
                ],
                requeue=[],
            )

            cursor_event_id, cursor_ts = get_cursor(group, "peer1")
            self.assertEqual(cursor_event_id, str(event.get("id") or ""))
            self.assertEqual(cursor_ts, str(event.get("ts") or ""))

            ledger_events = self._ledger_events(group)
            self.assertEqual(str(ledger_events[-1].get("kind") or ""), "chat.read")
            self.assertEqual(str(ledger_events[-1].get("by") or ""), "peer1")
            self.assertEqual(str((ledger_events[-1].get("data") or {}).get("actor_id") or ""), "peer1")
            self.assertEqual(str((ledger_events[-1].get("data") or {}).get("event_id") or ""), str(event.get("id") or ""))
        finally:
            cleanup()

    def test_finalize_delivery_success_skips_chat_read_when_cursor_is_already_newer(self) -> None:
        from cccc.daemon.messaging.delivery import PendingMessage, _finalize_delivery_success
        from cccc.kernel.group import load_group
        from cccc.kernel.inbox import set_cursor

        _, cleanup = self._with_home()
        try:
            group_id = self._create_group_with_actor()
            first, _ = self._call(
                "send",
                {
                    "group_id": group_id,
                    "by": "user",
                    "to": ["peer1"],
                    "text": "first",
                },
            )
            second, _ = self._call(
                "send",
                {
                    "group_id": group_id,
                    "by": "user",
                    "to": ["peer1"],
                    "text": "second",
                },
            )
            self.assertTrue(first.ok, getattr(first, "error", None))
            self.assertTrue(second.ok, getattr(second, "error", None))
            first_event = (first.result or {}).get("event") if isinstance(first.result, dict) else {}
            second_event = (second.result or {}).get("event") if isinstance(second.result, dict) else {}
            self.assertIsInstance(first_event, dict)
            self.assertIsInstance(second_event, dict)
            assert isinstance(first_event, dict)
            assert isinstance(second_event, dict)

            group = load_group(group_id)
            self.assertIsNotNone(group)
            assert group is not None
            group.doc["delivery"] = {"auto_mark_on_delivery": True}
            set_cursor(
                group,
                "peer1",
                event_id=str(second_event.get("id") or ""),
                ts=str(second_event.get("ts") or ""),
            )
            before_count = len(self._ledger_events(group))

            _finalize_delivery_success(
                group,
                actor_id="peer1",
                chat_total=1,
                deliverable=[
                    PendingMessage(
                        event_id=str(first_event.get("id") or ""),
                        by="user",
                        to=["peer1"],
                        text="first",
                        ts=str(first_event.get("ts") or ""),
                        kind="chat.message",
                    )
                ],
                requeue=[],
            )

            after_events = self._ledger_events(group)
            self.assertEqual(len(after_events), before_count)
            self.assertNotEqual(str(after_events[-1].get("kind") or ""), "chat.read")
        finally:
            cleanup()


if __name__ == "__main__":
    unittest.main()
