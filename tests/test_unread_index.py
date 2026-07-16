import os
import tempfile
import unittest
from unittest.mock import patch


class TestUnreadIndex(unittest.TestCase):
    def _with_home(self):
        old_home = os.environ.get("CCCC_HOME")
        td_ctx = tempfile.TemporaryDirectory(ignore_cleanup_errors=True)
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

    def _create_group(self) -> str:
        create, _ = self._call("group_create", {"title": "unread-index", "topic": "", "by": "user"})
        self.assertTrue(create.ok, getattr(create, "error", None))
        group_id = str((create.result or {}).get("group_id") or "").strip()
        self.assertTrue(group_id)

        attach, _ = self._call("attach", {"group_id": group_id, "path": ".", "by": "user"})
        self.assertTrue(attach.ok, getattr(attach, "error", None))
        return group_id

    def _add_actor(self, group_id: str, actor_id: str, title: str) -> None:
        add, _ = self._call(
            "actor_add",
            {
                "group_id": group_id,
                "actor_id": actor_id,
                "title": title,
                "runtime": "codex",
                "runner": "headless",
                "by": "user",
            },
        )
        self.assertTrue(add.ok, getattr(add, "error", None))

    def _actor_list(self, group_id: str, *, include_unread: bool) -> list[dict]:
        resp, _ = self._call("actor_list", {"group_id": group_id, "include_unread": include_unread})
        self.assertTrue(resp.ok, getattr(resp, "error", None))
        actors = (resp.result or {}).get("actors") if isinstance(resp.result, dict) else []
        self.assertIsInstance(actors, list)
        assert isinstance(actors, list)
        return [item for item in actors if isinstance(item, dict)]

    def test_actor_list_include_unread_reuses_snapshot_without_ledger_scan(self) -> None:
        _, cleanup = self._with_home()
        try:
            group_id = self._create_group()
            self._add_actor(group_id, "peer1", "Peer 1")

            first = self._actor_list(group_id, include_unread=True)
            self.assertEqual(len(first), 1)
            self.assertEqual(int(first[0].get("unread_count") or 0), 0)

            with patch("cccc.kernel.inbox.iter_events", side_effect=AssertionError("snapshot path should not rescan ledger")):
                second = self._actor_list(group_id, include_unread=True)
            self.assertEqual(int(second[0].get("unread_count") or 0), 0)
        finally:
            cleanup()

    def test_actor_list_include_unread_applies_delta_without_full_rebuild(self) -> None:
        _, cleanup = self._with_home()
        try:
            group_id = self._create_group()
            self._add_actor(group_id, "peer1", "Peer 1")
            self._add_actor(group_id, "peer2", "Peer 2")

            initial = self._actor_list(group_id, include_unread=True)
            initial_by_id = {str(item.get("id") or ""): item for item in initial}
            peer1_before = int(initial_by_id["peer1"].get("unread_count") or 0)
            peer2_before = int(initial_by_id["peer2"].get("unread_count") or 0)

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

            with patch("cccc.kernel.inbox.batch_unread_counts", side_effect=AssertionError("delta path should avoid full rebuild")):
                actors = self._actor_list(group_id, include_unread=True)
            by_id = {str(item.get("id") or ""): item for item in actors}
            self.assertEqual(int(by_id["peer1"].get("unread_count") or 0), peer1_before + 1)
            self.assertEqual(int(by_id["peer2"].get("unread_count") or 0), peer2_before)
        finally:
            cleanup()

    def test_actor_list_include_unread_rebuilds_when_actors_rev_changes(self) -> None:
        _, cleanup = self._with_home()
        try:
            group_id = self._create_group()
            self._add_actor(group_id, "lead", "Lead")
            self._add_actor(group_id, "peer1", "Peer 1")

            sent, _ = self._call(
                "send",
                {
                    "group_id": group_id,
                    "by": "user",
                    "to": ["@foreman"],
                    "text": "foreman only",
                },
            )
            self.assertTrue(sent.ok, getattr(sent, "error", None))

            before = self._actor_list(group_id, include_unread=True)
            before_by_id = {str(item.get("id") or ""): item for item in before}
            lead_before = int(before_by_id["lead"].get("unread_count") or 0)
            peer_before = int(before_by_id["peer1"].get("unread_count") or 0)
            self.assertGreater(lead_before, peer_before)

            removed, _ = self._call("actor_remove", {"group_id": group_id, "actor_id": "lead", "by": "user"})
            self.assertTrue(removed.ok, getattr(removed, "error", None))

            after = self._actor_list(group_id, include_unread=True)
            self.assertEqual(len(after), 1)
            self.assertEqual(str(after[0].get("id") or ""), "peer1")
            self.assertGreater(int(after[0].get("unread_count") or 0), peer_before)
        finally:
            cleanup()

    def test_actor_list_include_unread_can_restore_from_ledger_snapshot_after_compact(self) -> None:
        _, cleanup = self._with_home()
        try:
            from cccc.kernel.group import load_group

            group_id = self._create_group()
            self._add_actor(group_id, "peer1", "Peer 1")

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

            baseline = self._actor_list(group_id, include_unread=True)
            self.assertEqual(int(baseline[0].get("unread_count") or 0), 1)

            compact, _ = self._call("ledger_compact", {"group_id": group_id, "by": "user", "reason": "snapshot-seed", "force": True})
            self.assertTrue(compact.ok, getattr(compact, "error", None))

            group = load_group(group_id)
            self.assertIsNotNone(group)
            assert group is not None
            unread_index_path = group.path / "state" / "unread_index.json"
            unread_index_path.unlink(missing_ok=True)

            with patch("cccc.kernel.inbox.batch_unread_counts", side_effect=AssertionError("ledger snapshot should seed unread state")):
                restored = self._actor_list(group_id, include_unread=True)
            self.assertEqual(int(restored[0].get("unread_count") or 0), 1)
        finally:
            cleanup()


if __name__ == "__main__":
    unittest.main()
