import os
import tempfile
import unittest
from unittest.mock import patch


class TestGroupStateOps(unittest.TestCase):
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

    def test_group_set_state_roundtrip(self) -> None:
        _, cleanup = self._with_home()
        try:
            create, _ = self._call("group_create", {"title": "group-state", "topic": "", "by": "user"})
            self.assertTrue(create.ok, getattr(create, "error", None))
            group_id = str((create.result or {}).get("group_id") or "").strip()
            self.assertTrue(group_id)

            idle, _ = self._call("group_set_state", {"group_id": group_id, "state": "idle", "by": "user"})
            self.assertTrue(idle.ok, getattr(idle, "error", None))
            self.assertEqual(str((idle.result or {}).get("state") or ""), "idle")

            active, _ = self._call("group_set_state", {"group_id": group_id, "state": "active", "by": "user"})
            self.assertTrue(active.ok, getattr(active, "error", None))
            self.assertEqual(str((active.result or {}).get("state") or ""), "active")
        finally:
            cleanup()

    def test_resume_from_idle_clears_pending_auto_idle_notifications(self) -> None:
        _, cleanup = self._with_home()
        try:
            create, _ = self._call("group_create", {"title": "group-state-resume", "topic": "", "by": "user"})
            self.assertTrue(create.ok, getattr(create, "error", None))
            group_id = str((create.result or {}).get("group_id") or "").strip()
            self.assertTrue(group_id)

            idle, _ = self._call("group_set_state", {"group_id": group_id, "state": "idle", "by": "user"})
            self.assertTrue(idle.ok, getattr(idle, "error", None))

            with patch("cccc.daemon.server.THROTTLE.clear_pending_system_notifies", return_value=1) as clear_mock:
                active, _ = self._call("group_set_state", {"group_id": group_id, "state": "active", "by": "user"})

            self.assertTrue(active.ok, getattr(active, "error", None))
            clear_mock.assert_called_once()
            notify_kinds = clear_mock.call_args.kwargs.get("notify_kinds")
            self.assertIsInstance(notify_kinds, set)
            self.assertIn("auto_idle", notify_kinds)
        finally:
            cleanup()

    def test_resume_from_paused_recovers_headless_unread_work(self) -> None:
        from cccc.daemon.messaging.runtime_delivery import latest_delivery_state
        from cccc.kernel.actors import add_actor
        from cccc.kernel.group import load_group, set_group_state
        from cccc.kernel.inbox import get_cursor
        from cccc.kernel.ledger import append_event

        _, cleanup = self._with_home()
        try:
            create, _ = self._call("group_create", {"title": "paused-headless-resume", "topic": "", "by": "user"})
            self.assertTrue(create.ok, getattr(create, "error", None))
            group_id = str((create.result or {}).get("group_id") or "").strip()
            self.assertTrue(group_id)

            group = load_group(group_id)
            self.assertIsNotNone(group)
            assert group is not None
            add_actor(group, actor_id="peer1", runtime="codex", runner="headless")
            unread = append_event(
                group.ledger_path,
                kind="chat.message",
                group_id=group.group_id,
                scope_key="",
                by="user",
                data={"to": ["peer1"], "text": "resume this work", "message_mode": "send"},
            )
            group = set_group_state(group, state="paused")

            with (
                patch("cccc.daemon.codex_app_sessions.SUPERVISOR.actor_running", return_value=True),
                patch(
                    "cccc.daemon.codex_app_sessions.SUPERVISOR.submit_user_message",
                    return_value=True,
                ) as submit,
            ):
                active, _ = self._call(
                    "group_set_state",
                    {"group_id": group_id, "state": "active", "by": "user"},
                )

            self.assertTrue(active.ok, getattr(active, "error", None))
            submit.assert_called_once()
            self.assertEqual(submit.call_args.kwargs.get("event_id"), unread["id"])
            delivery = latest_delivery_state(
                group,
                actor_id="peer1",
                source_event_id=unread["id"],
            )
            self.assertEqual((delivery or {}).get("data", {}).get("state"), "accepted")
            self.assertEqual(get_cursor(group, "peer1"), ("", ""))
        finally:
            cleanup()


if __name__ == "__main__":
    unittest.main()
