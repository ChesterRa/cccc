import os
import tempfile
import unittest
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
            "https://peer.example/",
            transport="fake",
            remote_group_id="g_remote",
            credential_ref="cred-1",
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


if __name__ == "__main__":
    unittest.main()
