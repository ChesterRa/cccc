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
        self.payloads = []

    def deliver(self, envelope):
        self.calls += 1
        self.credentials.append(envelope.credential)
        self.payloads.append(envelope.payload)
        return self._result


class _SequenceTransport:
    transport = "registry_hub"
    capabilities = frozenset()

    def __init__(self, results):
        self._results = list(results)
        self.calls = 0

    def deliver(self, envelope):
        _ = envelope
        self.calls += 1
        if self._results:
            return self._results.pop(0)
        raise AssertionError("no more remote send results")


class TestGroupBridgeDaemonOps(unittest.TestCase):
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

    def _registration(self, *, group_id: str = "g_local", credential_ref: str = ""):
        from cccc.kernel.group_bridge.registration import upsert_registration

        return upsert_registration(
            group_id,
            "https://peer.example/",
            transport="registry_hub",
            remote_group_id="g_remote",
            credential_ref=credential_ref,
        )

    def test_unrelated_op_returns_none(self) -> None:
        from cccc.daemon.group_bridge.ops import try_handle_remote_send_op

        self.assertIsNone(try_handle_remote_send_op("some_other_op", {}))

    def test_receive_remote_send_op_delegates_to_receiver(self) -> None:
        from cccc.daemon.group_bridge.ops import try_handle_remote_send_op

        with patch(
            "cccc.daemon.group_bridge.ops.receive_remote_send",
            return_value={"ok": True, "event_id": "evt-local", "duplicate": False},
        ) as receive:
            resp = try_handle_remote_send_op(
                "group_bridge_receive_remote_send",
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
        from cccc.daemon.group_bridge.ops import handle_remote_send, handle_remote_delivery_status
        from cccc.daemon.group_bridge.transports.base import RemoteSendResult

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
            self.assertEqual(fake.payloads[0].source_by, "actor-a")

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
        from cccc.daemon.group_bridge.ops import handle_remote_send
        from cccc.daemon.group_bridge.transports.base import RemoteSendResult

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
        from cccc.daemon.group_bridge.ops import handle_remote_send
        from cccc.daemon.group_bridge.transports.base import RemoteSendResult

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
        from cccc.daemon.group_bridge.ops import try_handle_remote_send_op

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
        from cccc.daemon.group_bridge.ops import try_handle_remote_send_op

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
        from cccc.daemon.group_bridge.ops import try_handle_remote_send_op
        from cccc.kernel.group_bridge.receipts import get_receipt

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
        from cccc.daemon.group_bridge.ops import try_handle_remote_send_op
        from cccc.kernel.group_bridge.receipts import get_receipt

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
        from cccc.daemon.group_bridge.ops import try_handle_remote_send_op

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
        from cccc.daemon.group_bridge.ops import try_handle_remote_send_op

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

    def test_remote_outbox_retry_success_projects_cross_group_receipt(self) -> None:
        from cccc.daemon.group_bridge.ops import handle_remote_send
        from cccc.daemon.group_bridge.remote_outbox_worker import sweep_remote_outbox
        from cccc.daemon.group_bridge.transports.base import sent_result, transient_result
        from cccc.kernel.group import create_group
        from cccc.kernel.group_bridge.receipts import update_receipt
        from cccc.kernel.inbox import iter_events
        from cccc.kernel.ledger import append_event
        from cccc.kernel.registry import load_registry

        _, cleanup = self._with_home()
        try:
            group = create_group(load_registry(), title="remote-retry-source", topic="")
            reg = self._registration(group_id=group.group_id)
            source_event = append_event(
                group.ledger_path,
                kind="chat.message",
                group_id=group.group_id,
                scope_key="",
                by="user",
                data={"text": "remote ping", "dst_group_id": "g_remote"},
            )
            source_event_id = str(source_event.get("id") or "")
            fake = _SequenceTransport([
                transient_result("offline", "peer offline", transport="registry_hub"),
                sent_result("evt_remote_retry", transport="registry_hub"),
            ])

            first = handle_remote_send(
                {
                    "group_id": group.group_id,
                    "by": "user",
                    "registration_id": reg["registration_id"],
                    "idempotency_key": "k-retry",
                    "source_event_id": source_event_id,
                    "payload": {"text": "remote ping", "to": ["@foreman"]},
                },
                transport_factory=lambda _name: fake,
            )
            self.assertTrue(first.ok, getattr(first, "error", None))
            self.assertEqual(first.result["receipt"]["status"], "retrying")
            self.assertFalse([
                ev for ev in iter_events(group.ledger_path)
                if ev.get("kind") == "chat.cross_group_receipt"
            ])

            update_receipt(reg["registration_id"], "k-retry", next_attempt_at="")
            sweep = sweep_remote_outbox(transport_factory=lambda _name: fake)
            self.assertEqual(sweep["sent"], 1)

            receipts = [
                ev for ev in iter_events(group.ledger_path)
                if ev.get("kind") == "chat.cross_group_receipt"
            ]
            self.assertEqual(len(receipts), 1)
            data = receipts[0].get("data") or {}
            self.assertEqual(data.get("source_event_id"), source_event_id)
            self.assertEqual(data.get("dst_group_id"), "g_remote")
            self.assertEqual(data.get("remote_event_id"), "evt_remote_retry")
            self.assertEqual(data.get("registration_id"), reg["registration_id"])
            self.assertEqual(data.get("idempotency_key"), "k-retry")
            self.assertEqual(data.get("status"), "sent")

            replay = handle_remote_send(
                {
                    "group_id": group.group_id,
                    "by": "user",
                    "registration_id": reg["registration_id"],
                    "idempotency_key": "k-retry",
                    "source_event_id": source_event_id,
                    "payload": {"text": "remote ping changed", "to": ["@foreman"]},
                },
                transport_factory=lambda _name: fake,
            )
            self.assertTrue(replay.ok, getattr(replay, "error", None))
            receipts_after_replay = [
                ev for ev in iter_events(group.ledger_path)
                if ev.get("kind") == "chat.cross_group_receipt"
            ]
            self.assertEqual(len(receipts_after_replay), 1)
        finally:
            cleanup()


if __name__ == "__main__":
    unittest.main()
