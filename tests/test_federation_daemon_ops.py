import os
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch


class _CountingTransport:
    transport = "registry_hub"
    capabilities = frozenset()

    def __init__(self, result):
        self._result = result
        self.calls = 0
        self.credentials = []

    def deliver(self, envelope):
        self.calls += 1
        self.credentials.append(envelope.credential)
        return self._result


class TestFederationDaemonOps(unittest.TestCase):
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

    def _registration(self, *, credential_ref: str = ""):
        from cccc.kernel.federation.registration import upsert_registration

        return upsert_registration(
            "g_local",
            "https://peer.example/",
            transport="registry_hub",
            remote_group_id="g_remote",
            credential_ref=credential_ref,
        )

    def test_unrelated_op_returns_none(self) -> None:
        from cccc.daemon.federation.ops import try_handle_remote_send_op

        self.assertIsNone(try_handle_remote_send_op("some_other_op", {}))

    def test_receive_remote_send_op_delegates_to_receiver(self) -> None:
        from cccc.daemon.federation.ops import try_handle_remote_send_op

        with patch(
            "cccc.daemon.federation.ops.receive_remote_send",
            return_value={"ok": True, "event_id": "evt-local", "duplicate": False},
        ) as receive:
            resp = try_handle_remote_send_op(
                "federation_receive_remote_send",
                {
                    "target_group_id": "g_local",
                    "src_group_id": "g_remote",
                    "remote_peer_id": "peer_remote",
                    "payload": {"text": "hi", "to": ["@foreman"]},
                    "idempotency_key": "remote-1",
                },
            )

        self.assertIsNotNone(resp)
        assert resp is not None
        self.assertTrue(resp.ok)
        self.assertEqual(resp.result["event_id"], "evt-local")
        receive.assert_called_once_with(
            target_group_id="g_local",
            src_group_id="g_remote",
            remote_peer_id="peer_remote",
            payload={"text": "hi", "to": ["@foreman"]},
            idempotency_key="remote-1",
        )

    def test_remote_send_delivers_once_and_replays_terminal_receipt(self) -> None:
        from cccc.daemon.federation.ops import handle_remote_send, handle_remote_delivery_status
        from cccc.daemon.federation.transports.base import RemoteSendResult

        _, cleanup = self._with_home()
        try:
            reg = self._registration()
            fake = _CountingTransport(
                RemoteSendResult(ok=True, status="sent", remote_event_id="remote-1", transport="registry_hub")
            )
            resp = handle_remote_send(
                {
                    "group_id": "g_local",
                    "by": "actor-a",
                    "registration_id": reg["registration_id"],
                    "idempotency_key": "k1",
                    "payload": {"text": "hi", "to": ["@foreman"]},
                },
                transport_factory=lambda _name: fake,
            )
            self.assertIsNotNone(resp)
            self.assertTrue(resp.ok)
            self.assertFalse(resp.result["queued"])
            self.assertEqual(resp.result["receipt"]["status"], "sent")
            self.assertEqual(resp.result["receipt"]["remote_event_id"], "remote-1")
            self.assertEqual(fake.calls, 1)

            replay = handle_remote_send(
                {
                    "group_id": "g_local",
                    "by": "actor-a",
                    "registration_id": reg["registration_id"],
                    "idempotency_key": "k1",
                    "payload": {"text": "changed", "to": ["@foreman"]},
                },
                transport_factory=lambda _name: fake,
            )
            self.assertTrue(replay.ok)
            self.assertEqual(replay.result["receipt"]["remote_event_id"], "remote-1")
            self.assertEqual(fake.calls, 1)

            status = handle_remote_delivery_status(
                {"group_id": "g_local", "registration_id": reg["registration_id"], "idempotency_key": "k1"}
            )
            self.assertTrue(status.ok)
            self.assertEqual(status.result["receipt"]["status"], "sent")
        finally:
            cleanup()

    def test_remote_send_records_failed_when_credential_ref_cannot_resolve(self) -> None:
        from cccc.daemon.federation.ops import handle_remote_send
        from cccc.daemon.federation.transports.base import RemoteSendResult

        _, cleanup = self._with_home()
        try:
            reg = self._registration(credential_ref="sec_remote_peer")
            fake = _CountingTransport(
                RemoteSendResult(ok=True, status="sent", remote_event_id="remote-1", transport="registry_hub")
            )
            resp = handle_remote_send(
                {
                    "group_id": "g_local",
                    "by": "actor-a",
                    "registration_id": reg["registration_id"],
                    "idempotency_key": "k1",
                    "payload": {"text": "hi", "to": ["@foreman"]},
                },
                transport_factory=lambda _name: fake,
            )
            self.assertTrue(resp.ok)
            receipt = resp.result["receipt"]
            self.assertEqual(receipt["status"], "failed")
            self.assertEqual(receipt["error"]["code"], "credential_unresolved")
            self.assertEqual(fake.calls, 0)
            self.assertNotIn("sec_remote_peer", str(receipt["error"]))
        finally:
            cleanup()

    def test_remote_send_passes_resolved_credential_not_reference(self) -> None:
        from cccc.daemon.federation.ops import handle_remote_send
        from cccc.daemon.federation.transports.base import RemoteSendResult

        _, cleanup = self._with_home()
        try:
            reg = self._registration(credential_ref="sec_remote_peer")
            fake = _CountingTransport(
                RemoteSendResult(ok=True, status="sent", remote_event_id="remote-1", transport="registry_hub")
            )
            resp = handle_remote_send(
                {
                    "group_id": "g_local",
                    "by": "actor-a",
                    "registration_id": reg["registration_id"],
                    "idempotency_key": "k1",
                    "payload": {"text": "hi", "to": ["@foreman"]},
                },
                transport_factory=lambda _name: fake,
                credential_resolver=lambda _ref: "raw-token-from-store",
            )
            self.assertTrue(resp.ok)
            self.assertEqual(resp.result["receipt"]["status"], "sent")
            self.assertEqual(fake.credentials, ["raw-token-from-store"])
            self.assertNotIn("sec_remote_peer", fake.credentials)
        finally:
            cleanup()

    def test_remote_send_unknown_registration_errors(self) -> None:
        from cccc.daemon.federation.ops import try_handle_remote_send_op

        _, cleanup = self._with_home()
        try:
            resp = try_handle_remote_send_op(
                "remote_send",
                {
                    "group_id": "g_local",
                    "registration_id": "reg_missing",
                    "idempotency_key": "k1",
                    "payload": {"text": "x", "to": ["@foreman"]},
                },
            )
            self.assertIsNotNone(resp)
            assert resp is not None
            self.assertFalse(resp.ok)
            self.assertEqual(resp.error.code, "registration_not_found")
        finally:
            cleanup()

    def test_remote_send_requires_idempotency_key(self) -> None:
        from cccc.daemon.federation.ops import try_handle_remote_send_op

        _, cleanup = self._with_home()
        try:
            reg = self._registration(credential_ref="sec_remote_peer")
            resp = try_handle_remote_send_op(
                "remote_send",
                {"group_id": "g_local", "registration_id": reg["registration_id"], "payload": {"text": "x"}},
            )
            self.assertIsNotNone(resp)
            assert resp is not None
            self.assertFalse(resp.ok)
            self.assertEqual(resp.error.code, "missing_idempotency_key")
        finally:
            cleanup()

    def test_remote_send_requires_explicit_recipient(self) -> None:
        from cccc.daemon.federation.ops import try_handle_remote_send_op
        from cccc.kernel.federation.receipts import get_receipt

        _, cleanup = self._with_home()
        try:
            reg = self._registration()
            rid = reg["registration_id"]
            resp = try_handle_remote_send_op(
                "remote_send",
                {
                    "group_id": "g_local",
                    "registration_id": rid,
                    "idempotency_key": "k1",
                    "payload": {"text": "x", "to": [" "]},
                },
            )
            self.assertIsNotNone(resp)
            assert resp is not None
            self.assertFalse(resp.ok)
            self.assertEqual(resp.error.code, "missing_remote_recipient")
            self.assertIsNone(get_receipt(rid, "k1"))
        finally:
            cleanup()

    def test_remote_send_rejects_group_mismatch(self) -> None:
        from cccc.daemon.federation.ops import try_handle_remote_send_op
        from cccc.kernel.federation.receipts import get_receipt

        _, cleanup = self._with_home()
        try:
            reg = self._registration()  # registered for g_local
            rid = reg["registration_id"]
            resp = try_handle_remote_send_op(
                "remote_send",
                {
                    "group_id": "g_other",
                    "registration_id": rid,
                    "idempotency_key": "k1",
                    "payload": {"text": "x", "to": ["@foreman"]},
                },
            )
            assert resp is not None
            self.assertFalse(resp.ok)
            self.assertEqual(resp.error.code, "group_mismatch")
            # Rejected => must not have enqueued anything.
            self.assertIsNone(get_receipt(rid, "k1"))
        finally:
            cleanup()

    def test_remote_delivery_status_rejects_group_mismatch(self) -> None:
        from cccc.daemon.federation.ops import try_handle_remote_send_op

        _, cleanup = self._with_home()
        try:
            reg = self._registration(credential_ref="sec_remote_peer")
            rid = reg["registration_id"]
            try_handle_remote_send_op(
                "remote_send",
                {
                    "group_id": "g_local",
                    "registration_id": rid,
                    "idempotency_key": "k1",
                    "payload": {"text": "hi", "to": ["@foreman"]},
                },
            )
            resp = try_handle_remote_send_op(
                "remote_delivery_status",
                {"group_id": "g_other", "registration_id": rid, "idempotency_key": "k1"},
            )
            assert resp is not None
            self.assertFalse(resp.ok)
            self.assertEqual(resp.error.code, "group_mismatch")
        finally:
            cleanup()

    def test_remote_delivery_status_returns_receipt(self) -> None:
        from cccc.daemon.federation.ops import try_handle_remote_send_op

        _, cleanup = self._with_home()
        try:
            reg = self._registration(credential_ref="sec_remote_peer")
            rid = reg["registration_id"]
            try_handle_remote_send_op(
                "remote_send",
                {
                    "group_id": "g_local",
                    "registration_id": rid,
                    "idempotency_key": "k1",
                    "payload": {"text": "hi", "to": ["@foreman"]},
                },
            )
            resp = try_handle_remote_send_op(
                "remote_delivery_status",
                {"group_id": "g_local", "registration_id": rid, "idempotency_key": "k1"},
            )
            self.assertIsNotNone(resp)
            assert resp is not None
            self.assertTrue(resp.ok)
            self.assertEqual(resp.result["receipt"]["status"], "failed")
            self.assertEqual(resp.result["receipt"]["error"]["code"], "credential_unresolved")

            missing = try_handle_remote_send_op(
                "remote_delivery_status",
                {"group_id": "g_local", "registration_id": rid, "idempotency_key": "nope"},
            )
            assert missing is not None
            self.assertTrue(missing.ok)
            self.assertIsNone(missing.result["receipt"])
        finally:
            cleanup()


if __name__ == "__main__":
    unittest.main()
