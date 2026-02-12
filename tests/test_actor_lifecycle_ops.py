import os
import tempfile
import unittest


class TestActorLifecycleOps(unittest.TestCase):
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

    def test_actor_start_stop_transitions_group_running(self) -> None:
        _, cleanup = self._with_home()
        try:
            create, _ = self._call("group_create", {"title": "actor-lifecycle", "topic": "", "by": "user"})
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

            stop, _ = self._call("actor_stop", {"group_id": group_id, "actor_id": "peer1", "by": "user"})
            self.assertTrue(stop.ok, getattr(stop, "error", None))
            actor_after_stop = (stop.result or {}).get("actor") if isinstance(stop.result, dict) else {}
            self.assertIsInstance(actor_after_stop, dict)
            assert isinstance(actor_after_stop, dict)
            self.assertFalse(bool(actor_after_stop.get("enabled", True)))

            show_after_stop, _ = self._call("group_show", {"group_id": group_id})
            self.assertTrue(show_after_stop.ok, getattr(show_after_stop, "error", None))
            group_doc_after_stop = (show_after_stop.result or {}).get("group") if isinstance(show_after_stop.result, dict) else {}
            self.assertIsInstance(group_doc_after_stop, dict)
            assert isinstance(group_doc_after_stop, dict)
            self.assertFalse(bool(group_doc_after_stop.get("running")))

            start, _ = self._call("actor_start", {"group_id": group_id, "actor_id": "peer1", "by": "user"})
            self.assertTrue(start.ok, getattr(start, "error", None))
            actor_after_start = (start.result or {}).get("actor") if isinstance(start.result, dict) else {}
            self.assertIsInstance(actor_after_start, dict)
            assert isinstance(actor_after_start, dict)
            self.assertTrue(bool(actor_after_start.get("enabled", False)))

            show_after_start, _ = self._call("group_show", {"group_id": group_id})
            self.assertTrue(show_after_start.ok, getattr(show_after_start, "error", None))
            group_doc_after_start = (show_after_start.result or {}).get("group") if isinstance(show_after_start.result, dict) else {}
            self.assertIsInstance(group_doc_after_start, dict)
            assert isinstance(group_doc_after_start, dict)
            self.assertTrue(bool(group_doc_after_start.get("running")))
        finally:
            cleanup()

    def test_actor_restart_keeps_actor_enabled(self) -> None:
        _, cleanup = self._with_home()
        try:
            create, _ = self._call("group_create", {"title": "actor-restart", "topic": "", "by": "user"})
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

            restart, _ = self._call("actor_restart", {"group_id": group_id, "actor_id": "peer1", "by": "user"})
            self.assertTrue(restart.ok, getattr(restart, "error", None))
            actor = (restart.result or {}).get("actor") if isinstance(restart.result, dict) else {}
            self.assertIsInstance(actor, dict)
            assert isinstance(actor, dict)
            self.assertTrue(bool(actor.get("enabled", False)))

            event = (restart.result or {}).get("event") if isinstance(restart.result, dict) else {}
            self.assertIsInstance(event, dict)
            assert isinstance(event, dict)
            self.assertEqual(str(event.get("kind") or ""), "actor.restart")
        finally:
            cleanup()

    def test_actor_remove_stops_group_when_last_actor_removed(self) -> None:
        _, cleanup = self._with_home()
        try:
            create, _ = self._call("group_create", {"title": "actor-remove", "topic": "", "by": "user"})
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

            remove, _ = self._call("actor_remove", {"group_id": group_id, "actor_id": "peer1", "by": "user"})
            self.assertTrue(remove.ok, getattr(remove, "error", None))
            self.assertEqual(str((remove.result or {}).get("actor_id") or ""), "peer1")

            show, _ = self._call("group_show", {"group_id": group_id})
            self.assertTrue(show.ok, getattr(show, "error", None))
            group_doc = (show.result or {}).get("group") if isinstance(show.result, dict) else {}
            self.assertIsInstance(group_doc, dict)
            assert isinstance(group_doc, dict)
            self.assertFalse(bool(group_doc.get("running")))
        finally:
            cleanup()

    def test_actor_update_enabled_toggle_preserves_running_semantics(self) -> None:
        _, cleanup = self._with_home()
        try:
            create, _ = self._call("group_create", {"title": "actor-update", "topic": "", "by": "user"})
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

            disable, _ = self._call(
                "actor_update",
                {"group_id": group_id, "actor_id": "peer1", "by": "user", "patch": {"enabled": False}},
            )
            self.assertTrue(disable.ok, getattr(disable, "error", None))
            actor_after_disable = (disable.result or {}).get("actor") if isinstance(disable.result, dict) else {}
            self.assertIsInstance(actor_after_disable, dict)
            assert isinstance(actor_after_disable, dict)
            self.assertFalse(bool(actor_after_disable.get("enabled", True)))

            show_after_disable, _ = self._call("group_show", {"group_id": group_id})
            self.assertTrue(show_after_disable.ok, getattr(show_after_disable, "error", None))
            group_doc_after_disable = (show_after_disable.result or {}).get("group") if isinstance(show_after_disable.result, dict) else {}
            self.assertIsInstance(group_doc_after_disable, dict)
            assert isinstance(group_doc_after_disable, dict)
            self.assertFalse(bool(group_doc_after_disable.get("running")))

            enable, _ = self._call(
                "actor_update",
                {"group_id": group_id, "actor_id": "peer1", "by": "user", "patch": {"enabled": True}},
            )
            self.assertTrue(enable.ok, getattr(enable, "error", None))
            actor_after_enable = (enable.result or {}).get("actor") if isinstance(enable.result, dict) else {}
            self.assertIsInstance(actor_after_enable, dict)
            assert isinstance(actor_after_enable, dict)
            self.assertTrue(bool(actor_after_enable.get("enabled", False)))

            show_after_enable, _ = self._call("group_show", {"group_id": group_id})
            self.assertTrue(show_after_enable.ok, getattr(show_after_enable, "error", None))
            group_doc_after_enable = (show_after_enable.result or {}).get("group") if isinstance(show_after_enable.result, dict) else {}
            self.assertIsInstance(group_doc_after_enable, dict)
            assert isinstance(group_doc_after_enable, dict)
            self.assertFalse(bool(group_doc_after_enable.get("running")))
        finally:
            cleanup()


if __name__ == "__main__":
    unittest.main()
