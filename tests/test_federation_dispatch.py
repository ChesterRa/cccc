import os
import tempfile
import unittest
from datetime import datetime, timedelta, timezone
from pathlib import Path


class _CountingTransport:
    """Fake transport recording how many times deliver() is called."""

    transport = "fake"
    capabilities = frozenset()

    def __init__(self, result):
        self._result = result
        self.calls = 0

    def deliver(self, envelope):
        self.calls += 1
        return self._result


class TestFederationDispatch(unittest.TestCase):
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

    def _make_registration(self):
        from cccc.kernel.federation.registration import upsert_registration

        return upsert_registration(
            "g_local",
            "federation-session://peer-remote",
            transport="federation_session",
            remote_group_id="g_remote",
            remote_peer_id="peer-remote",
            credential_ref="cred-1",
            _approved_by_pairing=True,
        )

    def test_enqueue_records_queued_idempotently(self) -> None:
        from cccc.daemon.federation.remote_dispatch import enqueue_remote_send
        from cccc.kernel.federation.receipts import get_receipt

        _, cleanup = self._with_home()
        try:
            reg = self._make_registration()
            rid = reg["registration_id"]
            r1 = enqueue_remote_send(
                src_group_id="g_local",
                registration_id=rid,
                idempotency_key="k1",
                payload={"text": "hi"},
            )
            self.assertEqual(r1["status"], "queued")
            # Re-enqueue same key must not create a second receipt.
            enqueue_remote_send(
                src_group_id="g_local",
                registration_id=rid,
                idempotency_key="k1",
                payload={"text": "changed"},
            )
            stored = get_receipt(rid, "k1")
            self.assertEqual(stored["status"], "queued")
        finally:
            cleanup()

    def test_deliver_calls_adapter_once_then_replays(self) -> None:
        from cccc.daemon.federation.remote_dispatch import deliver_enqueued, enqueue_remote_send
        from cccc.daemon.federation.transports.base import RemoteSendResult

        _, cleanup = self._with_home()
        try:
            reg = self._make_registration()
            rid = reg["registration_id"]
            enqueue_remote_send(
                src_group_id="g_local", registration_id=rid, idempotency_key="k1", payload={"text": "hi"}
            )

            fake = _CountingTransport(
                RemoteSendResult(ok=True, status="sent", remote_event_id="e1", transport="fake")
            )
            out1 = deliver_enqueued(
                registration_id=rid, idempotency_key="k1", transport_factory=lambda name: fake, credential="s"
            )
            self.assertEqual(out1["status"], "sent")
            self.assertEqual(fake.calls, 1)

            # Replay: receipt already terminal => adapter must NOT be called again.
            out2 = deliver_enqueued(
                registration_id=rid, idempotency_key="k1", transport_factory=lambda name: fake, credential="s"
            )
            self.assertEqual(out2["status"], "sent")
            self.assertEqual(out2["remote_event_id"], "e1")
            self.assertEqual(fake.calls, 1)
        finally:
            cleanup()

    def test_deliver_preserves_full_payload(self) -> None:
        from cccc.daemon.federation.remote_dispatch import deliver_enqueued, enqueue_remote_send
        from cccc.daemon.federation.transports.base import RemoteSendResult

        _, cleanup = self._with_home()
        try:
            reg = self._make_registration()
            rid = reg["registration_id"]
            payload = {
                "text": "hello world",
                "to": ["@all", "peer-x"],
                "priority": "attention",
                "reply_required": True,
                "refs": [{"kind": "url", "url": "https://x"}],
            }
            enqueue_remote_send(src_group_id="g_local", registration_id=rid, idempotency_key="k1", payload=payload)

            captured = {}

            class Capturing:
                transport = "fake"
                capabilities = frozenset({"refs"})

                def deliver(self, envelope):
                    captured["env"] = envelope
                    return RemoteSendResult(ok=True, status="sent", remote_event_id="e1", transport="fake")

            deliver_enqueued(
                registration_id=rid, idempotency_key="k1", transport_factory=lambda name: Capturing(), credential="s"
            )
            p = captured["env"].payload
            self.assertEqual(p.text, "hello world")
            self.assertEqual(p.to, ["@all", "peer-x"])
            self.assertEqual(p.priority, "attention")
            self.assertTrue(p.reply_required)
            self.assertEqual(p.refs, [{"kind": "url", "url": "https://x"}])
        finally:
            cleanup()

    def test_deliver_uses_source_event_id_separate_from_idempotency_key(self) -> None:
        from cccc.daemon.federation.remote_dispatch import deliver_enqueued, enqueue_remote_send
        from cccc.daemon.federation.transports.base import RemoteSendResult

        _, cleanup = self._with_home()
        try:
            reg = self._make_registration()
            rid = reg["registration_id"]
            enqueue_remote_send(
                src_group_id="g_local",
                registration_id=rid,
                idempotency_key="delivery-key",
                payload={"text": "hi"},
                source_event_id="local-event-1",
            )

            captured = {}

            class Capturing:
                transport = "fake"
                capabilities = frozenset()

                def deliver(self, envelope):
                    captured["env"] = envelope
                    return RemoteSendResult(ok=True, status="sent", remote_event_id="e1", transport="fake")

            deliver_enqueued(
                registration_id=rid,
                idempotency_key="delivery-key",
                transport_factory=lambda name: Capturing(),
                credential="s",
            )

            self.assertEqual(captured["env"].idempotency_key, "delivery-key")
            self.assertEqual(captured["env"].source_event_id, "local-event-1")
        finally:
            cleanup()

    def test_deliver_passes_session_registration_metadata_to_target(self) -> None:
        from cccc.daemon.federation.remote_dispatch import deliver_enqueued, enqueue_remote_send
        from cccc.daemon.federation.transports.base import RemoteSendResult
        from cccc.kernel.federation.pairing import _upsert_approved_session_registration

        _, cleanup = self._with_home()
        try:
            reg = _upsert_approved_session_registration(
                "g_local",
                "http://remote.example:8848",
                remote_group_id="g_remote",
                remote_peer_id="peer-remote",
            )
            rid = reg["registration_id"]
            enqueue_remote_send(src_group_id="g_local", registration_id=rid, idempotency_key="k1", payload={"text": "hi"})

            captured = {}

            class Capturing:
                transport = "federation_session"
                capabilities = frozenset()

                def deliver(self, envelope):
                    captured["target"] = envelope.target
                    return RemoteSendResult(ok=True, status="sent", remote_event_id="e1", transport="federation_session")

            deliver_enqueued(
                registration_id=rid, idempotency_key="k1", transport_factory=lambda name: Capturing(), credential="s"
            )
            target = captured["target"]
            self.assertEqual(target.url, "http://remote.example:8848")
            self.assertEqual(target.remote_group_id, "g_remote")
            self.assertEqual(target.remote_peer_id, "peer-remote")
            self.assertEqual(target.multiaddrs, ())
        finally:
            cleanup()

    def test_deliver_records_failed_terminal_without_recall(self) -> None:
        from cccc.daemon.federation.remote_dispatch import deliver_enqueued, enqueue_remote_send
        from cccc.daemon.federation.transports.base import RemoteSendResult

        _, cleanup = self._with_home()
        try:
            reg = self._make_registration()
            rid = reg["registration_id"]
            enqueue_remote_send(
                src_group_id="g_local", registration_id=rid, idempotency_key="k1", payload={"text": "hi"}
            )
            fake = _CountingTransport(
                RemoteSendResult(ok=False, status="failed", error_code="unauthorized", retriable=False, transport="fake")
            )
            out1 = deliver_enqueued(
                registration_id=rid, idempotency_key="k1", transport_factory=lambda name: fake, credential="s"
            )
            self.assertEqual(out1["status"], "failed")
            self.assertEqual(fake.calls, 1)
            # Permanent failure is terminal — no re-call on replay.
            deliver_enqueued(
                registration_id=rid, idempotency_key="k1", transport_factory=lambda name: fake, credential="s"
            )
            self.assertEqual(fake.calls, 1)
        finally:
            cleanup()

    def test_retriable_failure_is_retrying_and_recalled_with_same_key(self) -> None:
        from cccc.daemon.federation.remote_dispatch import deliver_enqueued, enqueue_remote_send
        from cccc.daemon.federation.transports.base import RemoteSendResult

        _, cleanup = self._with_home()
        try:
            reg = self._make_registration()
            rid = reg["registration_id"]
            enqueue_remote_send(
                src_group_id="g_local", registration_id=rid, idempotency_key="k1", payload={"text": "hi"}
            )

            class FlakyTransport:
                transport = "fake"
                capabilities = frozenset()

                def __init__(self):
                    self.calls = 0

                def deliver(self, envelope):
                    self.calls += 1
                    if self.calls == 1:
                        return RemoteSendResult(
                            ok=False,
                            status="failed",
                            error_code="peer_session_unavailable",
                            error_message="offline",
                            retriable=True,
                            transport="fake",
                        )
                    return RemoteSendResult(ok=True, status="sent", remote_event_id="e1", transport="fake")

            fake = FlakyTransport()
            out1 = deliver_enqueued(
                registration_id=rid, idempotency_key="k1", transport_factory=lambda name: fake, credential="s"
            )
            self.assertEqual(out1["status"], "retrying")
            self.assertEqual(out1["error"]["code"], "peer_session_unavailable")
            self.assertEqual(out1["attempt"], 1)
            self.assertTrue(out1["first_queued_at"])
            self.assertTrue(out1["last_attempt_at"])
            self.assertTrue(out1["next_attempt_at"])

            out2 = deliver_enqueued(
                registration_id=rid, idempotency_key="k1", transport_factory=lambda name: fake, credential="s"
            )
            self.assertEqual(out2["status"], "sent")
            self.assertEqual(out2["remote_event_id"], "e1")
            self.assertEqual(out2["attempt"], 2)
            self.assertEqual(fake.calls, 2)
        finally:
            cleanup()

    def test_due_receipts_wait_for_next_attempt_at(self) -> None:
        from cccc.daemon.federation.remote_dispatch import enqueue_remote_send, iter_due_receipts
        from cccc.kernel.federation.receipts import update_receipt

        _, cleanup = self._with_home()
        try:
            reg = self._make_registration()
            rid = reg["registration_id"]
            enqueue_remote_send(
                src_group_id="g_local", registration_id=rid, idempotency_key="k1", payload={"text": "hi"}
            )
            future = (datetime.now(timezone.utc) + timedelta(minutes=5)).isoformat().replace("+00:00", "Z")
            update_receipt(rid, "k1", status="retrying", next_attempt_at=future)

            due = iter_due_receipts()
            self.assertEqual(due, [])

            due_later = iter_due_receipts(now=datetime.now(timezone.utc) + timedelta(minutes=6))
            self.assertEqual(len(due_later), 1)
            self.assertEqual(due_later[0]["idempotency_key"], "k1")
        finally:
            cleanup()

    def test_retry_exhaustion_becomes_terminal_failed(self) -> None:
        from cccc.daemon.federation.remote_dispatch import deliver_enqueued, enqueue_remote_send
        from cccc.daemon.federation.transports.base import RemoteSendResult
        from cccc.kernel.federation.receipts import update_receipt

        _, cleanup = self._with_home()
        try:
            reg = self._make_registration()
            rid = reg["registration_id"]
            enqueue_remote_send(
                src_group_id="g_local", registration_id=rid, idempotency_key="k1", payload={"text": "hi"}
            )
            update_receipt(rid, "k1", attempt=4, max_attempts=5)
            fake = _CountingTransport(
                RemoteSendResult(
                    ok=False,
                    status="failed",
                    error_code="offline",
                    error_message="still offline",
                    retriable=True,
                    transport="fake",
                )
            )

            out = deliver_enqueued(
                registration_id=rid, idempotency_key="k1", transport_factory=lambda name: fake, credential="s"
            )
            self.assertEqual(out["status"], "failed")
            self.assertEqual(out["attempt"], 5)
            self.assertFalse(out["error"]["retriable"])
            self.assertEqual(out["next_attempt_at"], "")
        finally:
            cleanup()

    def test_stale_sending_receipt_is_due_after_restart(self) -> None:
        from cccc.daemon.federation.remote_dispatch import enqueue_remote_send, iter_due_receipts
        from cccc.kernel.federation.receipts import update_receipt

        _, cleanup = self._with_home()
        try:
            reg = self._make_registration()
            rid = reg["registration_id"]
            enqueue_remote_send(
                src_group_id="g_local", registration_id=rid, idempotency_key="k1", payload={"text": "hi"}
            )
            old = (datetime.now(timezone.utc) - timedelta(minutes=5)).isoformat().replace("+00:00", "Z")
            update_receipt(rid, "k1", status="sending", last_attempt_at=old)

            due = iter_due_receipts()
            self.assertEqual(len(due), 1)
            self.assertEqual(due[0]["status"], "sending")
        finally:
            cleanup()

    def test_outbox_worker_sweeps_due_receipts_once(self) -> None:
        from cccc.daemon.federation.remote_dispatch import enqueue_remote_send
        from cccc.daemon.federation.remote_outbox_worker import sweep_remote_outbox
        from cccc.daemon.federation.transports.base import RemoteSendResult

        _, cleanup = self._with_home()
        try:
            reg = self._make_registration()
            rid = reg["registration_id"]
            enqueue_remote_send(
                src_group_id="g_local", registration_id=rid, idempotency_key="k1", payload={"text": "hi"}
            )
            fake = _CountingTransport(
                RemoteSendResult(ok=True, status="sent", remote_event_id="remote-1", transport="fake")
            )

            result = sweep_remote_outbox(transport_factory=lambda name: fake, credential_resolver=lambda ref: "secret")

            self.assertEqual(result["attempted"], 1)
            self.assertEqual(result["sent"], 1)
            self.assertEqual(fake.calls, 1)
        finally:
            cleanup()

    def test_outbox_worker_does_not_send_when_credential_is_unresolved(self) -> None:
        from cccc.daemon.federation.remote_dispatch import enqueue_remote_send
        from cccc.daemon.federation.remote_outbox_worker import sweep_remote_outbox
        from cccc.daemon.federation.transports.base import RemoteSendResult
        from cccc.kernel.federation.receipts import get_receipt

        _, cleanup = self._with_home()
        try:
            reg = self._make_registration()
            rid = reg["registration_id"]
            enqueue_remote_send(
                src_group_id="g_local", registration_id=rid, idempotency_key="k1", payload={"text": "hi"}
            )
            fake = _CountingTransport(
                RemoteSendResult(ok=True, status="sent", remote_event_id="remote-1", transport="fake")
            )

            result = sweep_remote_outbox(transport_factory=lambda name: fake, credential_resolver=lambda ref: None)

            self.assertEqual(result["attempted"], 1)
            self.assertEqual(result["failed"], 1)
            self.assertEqual(fake.calls, 0)
            receipt = get_receipt(rid, "k1")
            self.assertEqual(receipt["status"], "failed")
            self.assertEqual(receipt["error"]["code"], "credential_unresolved")
        finally:
            cleanup()


if __name__ == "__main__":
    unittest.main()
