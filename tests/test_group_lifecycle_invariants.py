import os
import tempfile
import unittest


class TestGroupLifecycleInvariants(unittest.TestCase):
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

    def test_group_stop_and_start_preserve_core_invariants(self) -> None:
        _, cleanup = self._with_home()
        try:
            create_resp, _ = self._call("group_create", {"title": "lifecycle-test", "topic": "", "by": "user"})
            self.assertTrue(create_resp.ok, getattr(create_resp, "error", None))
            group_id = str((create_resp.result or {}).get("group_id") or "").strip()
            self.assertTrue(group_id)

            attach_resp, _ = self._call(
                "attach",
                {
                    "group_id": group_id,
                    "path": ".",
                    "by": "user",
                },
            )
            self.assertTrue(attach_resp.ok, getattr(attach_resp, "error", None))

            add_1_resp, _ = self._call(
                "actor_add",
                {
                    "group_id": group_id,
                    "by": "user",
                    "actor_id": "a1",
                    "title": "A1",
                    "runtime": "codex",
                    "runner": "headless",
                },
            )
            self.assertTrue(add_1_resp.ok, getattr(add_1_resp, "error", None))

            add_2_resp, _ = self._call(
                "actor_add",
                {
                    "group_id": group_id,
                    "by": "user",
                    "actor_id": "a2",
                    "title": "A2",
                    "runtime": "codex",
                    "runner": "headless",
                },
            )
            self.assertTrue(add_2_resp.ok, getattr(add_2_resp, "error", None))

            stop_resp, _ = self._call("group_stop", {"group_id": group_id, "by": "user"})
            self.assertTrue(stop_resp.ok, getattr(stop_resp, "error", None))

            show_after_stop_resp, _ = self._call("group_show", {"group_id": group_id})
            self.assertTrue(show_after_stop_resp.ok, getattr(show_after_stop_resp, "error", None))
            group_doc_after_stop = (show_after_stop_resp.result or {}).get("group") if isinstance(show_after_stop_resp.result, dict) else {}
            self.assertIsInstance(group_doc_after_stop, dict)
            assert isinstance(group_doc_after_stop, dict)
            self.assertFalse(bool(group_doc_after_stop.get("running")))

            actor_list_after_stop_resp, _ = self._call("actor_list", {"group_id": group_id})
            self.assertTrue(actor_list_after_stop_resp.ok, getattr(actor_list_after_stop_resp, "error", None))
            actors_after_stop = (actor_list_after_stop_resp.result or {}).get("actors") if isinstance(actor_list_after_stop_resp.result, dict) else []
            self.assertIsInstance(actors_after_stop, list)
            assert isinstance(actors_after_stop, list)
            for actor in actors_after_stop:
                if not isinstance(actor, dict):
                    continue
                self.assertFalse(bool(actor.get("enabled", True)))

            start_resp, _ = self._call("group_start", {"group_id": group_id, "by": "user"})
            self.assertTrue(start_resp.ok, getattr(start_resp, "error", None))

            show_after_start_resp, _ = self._call("group_show", {"group_id": group_id})
            self.assertTrue(show_after_start_resp.ok, getattr(show_after_start_resp, "error", None))
            group_doc_after_start = (show_after_start_resp.result or {}).get("group") if isinstance(show_after_start_resp.result, dict) else {}
            self.assertIsInstance(group_doc_after_start, dict)
            assert isinstance(group_doc_after_start, dict)
            self.assertTrue(bool(group_doc_after_start.get("running")))

            actor_list_after_start_resp, _ = self._call("actor_list", {"group_id": group_id})
            self.assertTrue(actor_list_after_start_resp.ok, getattr(actor_list_after_start_resp, "error", None))
            actors_after_start = (actor_list_after_start_resp.result or {}).get("actors") if isinstance(actor_list_after_start_resp.result, dict) else []
            self.assertIsInstance(actors_after_start, list)
            assert isinstance(actors_after_start, list)
            for actor in actors_after_start:
                if not isinstance(actor, dict):
                    continue
                self.assertTrue(bool(actor.get("enabled", False)))
        finally:
            cleanup()


if __name__ == "__main__":
    unittest.main()
