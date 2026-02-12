import os
import tempfile
import unittest


class TestSystemNotifyOps(unittest.TestCase):
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

    def test_system_notify_and_notify_ack(self) -> None:
        _, cleanup = self._with_home()
        try:
            create, _ = self._call("group_create", {"title": "sys-notify", "topic": "", "by": "user"})
            self.assertTrue(create.ok, getattr(create, "error", None))
            group_id = str((create.result or {}).get("group_id") or "").strip()
            self.assertTrue(group_id)

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

            notify, _ = self._call(
                "system_notify",
                {
                    "group_id": group_id,
                    "by": "system",
                    "kind": "unknown_kind",
                    "priority": "unknown_priority",
                    "title": "notify",
                    "message": "hello",
                    "target_actor_id": "peer1",
                    "requires_ack": True,
                },
            )
            self.assertTrue(notify.ok, getattr(notify, "error", None))
            notify_event = (notify.result or {}).get("event") if isinstance(notify.result, dict) else {}
            self.assertIsInstance(notify_event, dict)
            assert isinstance(notify_event, dict)
            self.assertEqual(str(notify_event.get("kind") or ""), "system.notify")
            notify_data = notify_event.get("data") if isinstance(notify_event.get("data"), dict) else {}
            self.assertIsInstance(notify_data, dict)
            assert isinstance(notify_data, dict)
            self.assertEqual(str(notify_data.get("kind") or ""), "info")
            self.assertEqual(str(notify_data.get("priority") or ""), "normal")

            notify_event_id = str(notify_event.get("id") or "").strip()
            self.assertTrue(notify_event_id)

            ack, _ = self._call(
                "notify_ack",
                {"group_id": group_id, "actor_id": "peer1", "notify_event_id": notify_event_id, "by": "peer1"},
            )
            self.assertTrue(ack.ok, getattr(ack, "error", None))
            ack_event = (ack.result or {}).get("event") if isinstance(ack.result, dict) else {}
            self.assertIsInstance(ack_event, dict)
            assert isinstance(ack_event, dict)
            self.assertEqual(str(ack_event.get("kind") or ""), "system.notify_ack")
        finally:
            cleanup()


if __name__ == "__main__":
    unittest.main()
