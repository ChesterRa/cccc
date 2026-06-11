import os
import tempfile
import unittest
from pathlib import Path


class TestFederationReceipts(unittest.TestCase):
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

        return Path(td), cleanup

    def test_record_is_idempotent_replay(self) -> None:
        from cccc.kernel.federation.receipts import get_receipt, record_receipt

        _, cleanup = self._with_home()
        try:
            stored, created = record_receipt("r1", "k1", {"ok": True, "status": "sent", "remote_event_id": "e1"})
            self.assertTrue(created)
            self.assertEqual(stored["remote_event_id"], "e1")

            # Replaying the same (registration_id, idempotency_key) must NOT
            # regenerate or overwrite — the first effect wins.
            stored2, created2 = record_receipt("r1", "k1", {"ok": True, "status": "sent", "remote_event_id": "e2"})
            self.assertFalse(created2)
            self.assertEqual(stored2["remote_event_id"], "e1")
            self.assertEqual(get_receipt("r1", "k1")["remote_event_id"], "e1")
        finally:
            cleanup()

    def test_distinct_keys_are_independent(self) -> None:
        from cccc.kernel.federation.receipts import record_receipt

        _, cleanup = self._with_home()
        try:
            record_receipt("r1", "k1", {"status": "sent"})
            _, created = record_receipt("r1", "k2", {"status": "sent"})
            self.assertTrue(created)
            _, created_other = record_receipt("r2", "k1", {"status": "sent"})
            self.assertTrue(created_other)
        finally:
            cleanup()

    def test_update_transitions_status(self) -> None:
        from cccc.kernel.federation.receipts import record_receipt, update_receipt

        _, cleanup = self._with_home()
        try:
            record_receipt("r1", "k1", {"status": "queued"})
            upd = update_receipt("r1", "k1", status="sent", remote_event_id="e9")
            self.assertIsNotNone(upd)
            assert upd is not None
            self.assertEqual(upd["status"], "sent")
            self.assertEqual(upd["remote_event_id"], "e9")
            self.assertIsNone(update_receipt("missing", "nope", status="sent"))
        finally:
            cleanup()

    def test_safe_error_projection_masks_secret_and_drops_unknown(self) -> None:
        from cccc.kernel.federation.receipts import safe_error_projection

        proj = safe_error_projection(
            {
                "code": "transport_error",
                "message": "auth failed for token acc_deadbeefdeadbeef trailing",
                "retriable": False,
                "transport": "registry_hub",
                "http_status": 401,
                "raw_secret": "acc_deadbeefdeadbeef",
                "internal_state": {"x": 1},
            }
        )
        flat = str(proj)
        self.assertNotIn("acc_deadbeefdeadbeef", flat)
        # Non-whitelisted fields are dropped entirely.
        self.assertNotIn("raw_secret", proj)
        self.assertNotIn("internal_state", proj)
        # Whitelisted fields survive.
        self.assertEqual(proj["code"], "transport_error")
        self.assertEqual(proj["http_status"], 401)


if __name__ == "__main__":
    unittest.main()
