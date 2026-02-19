import os
import tempfile
import unittest


class TestMaintenanceOps(unittest.TestCase):
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

    def test_send_cross_group_relay(self) -> None:
        _, cleanup = self._with_home()
        try:
            src_create, _ = self._call("group_create", {"title": "src", "topic": "", "by": "user"})
            self.assertTrue(src_create.ok, getattr(src_create, "error", None))
            src_group_id = str((src_create.result or {}).get("group_id") or "").strip()
            self.assertTrue(src_group_id)

            dst_create, _ = self._call("group_create", {"title": "dst", "topic": "", "by": "user"})
            self.assertTrue(dst_create.ok, getattr(dst_create, "error", None))
            dst_group_id = str((dst_create.result or {}).get("group_id") or "").strip()
            self.assertTrue(dst_group_id)

            relay, _ = self._call(
                "send_cross_group",
                {
                    "group_id": src_group_id,
                    "dst_group_id": dst_group_id,
                    "by": "user",
                    "text": "relay ping",
                    "to": ["user"],
                },
            )
            self.assertTrue(relay.ok, getattr(relay, "error", None))
            src_event = (relay.result or {}).get("src_event") if isinstance(relay.result, dict) else {}
            dst_event = (relay.result or {}).get("dst_event") if isinstance(relay.result, dict) else {}
            self.assertIsInstance(src_event, dict)
            self.assertIsInstance(dst_event, dict)
            assert isinstance(src_event, dict)
            assert isinstance(dst_event, dict)
            self.assertEqual(str(src_event.get("kind") or ""), "chat.message")
            self.assertEqual(str(dst_event.get("kind") or ""), "chat.message")
        finally:
            cleanup()

    def test_ledger_snapshot_and_compact(self) -> None:
        _, cleanup = self._with_home()
        try:
            create, _ = self._call("group_create", {"title": "ledger", "topic": "", "by": "user"})
            self.assertTrue(create.ok, getattr(create, "error", None))
            group_id = str((create.result or {}).get("group_id") or "").strip()
            self.assertTrue(group_id)

            sent, _ = self._call("send", {"group_id": group_id, "text": "hello", "by": "user", "to": ["user"]})
            self.assertTrue(sent.ok, getattr(sent, "error", None))

            snap, _ = self._call("ledger_snapshot", {"group_id": group_id, "by": "user", "reason": "test"})
            self.assertTrue(snap.ok, getattr(snap, "error", None))
            snapshot = (snap.result or {}).get("snapshot") if isinstance(snap.result, dict) else {}
            self.assertIsInstance(snapshot, dict)

            compact, _ = self._call("ledger_compact", {"group_id": group_id, "by": "user", "reason": "test", "force": True})
            self.assertTrue(compact.ok, getattr(compact, "error", None))
            self.assertIsInstance(compact.result, dict)
        finally:
            cleanup()

    def test_term_resize_rejects_tiny_size(self) -> None:
        _, cleanup = self._with_home()
        try:
            create, _ = self._call("group_create", {"title": "resize", "topic": "", "by": "user"})
            self.assertTrue(create.ok, getattr(create, "error", None))
            group_id = str((create.result or {}).get("group_id") or "").strip()
            self.assertTrue(group_id)

            tiny, _ = self._call(
                "term_resize",
                {"group_id": group_id, "actor_id": "peer1", "cols": 9, "rows": 1},
            )
            self.assertFalse(tiny.ok)
            self.assertEqual(str(getattr(tiny.error, "code", "") or ""), "invalid_size")
        finally:
            cleanup()

    def test_term_resize_accepts_minimum_supported_size(self) -> None:
        _, cleanup = self._with_home()
        try:
            create, _ = self._call("group_create", {"title": "resize-min", "topic": "", "by": "user"})
            self.assertTrue(create.ok, getattr(create, "error", None))
            group_id = str((create.result or {}).get("group_id") or "").strip()
            self.assertTrue(group_id)

            ok, _ = self._call(
                "term_resize",
                {"group_id": group_id, "actor_id": "peer1", "cols": 10, "rows": 2},
            )
            self.assertTrue(ok.ok, getattr(ok, "error", None))
            result = ok.result if isinstance(ok.result, dict) else {}
            self.assertIsInstance(result, dict)
            assert isinstance(result, dict)
            self.assertEqual(int(result.get("cols") or 0), 10)
            self.assertEqual(int(result.get("rows") or 0), 2)
        finally:
            cleanup()


if __name__ == "__main__":
    unittest.main()
