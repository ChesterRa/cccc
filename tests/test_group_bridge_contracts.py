import unittest


class TestGroupBridgeContracts(unittest.TestCase):
    def test_payload_defaults_and_forbid_extra(self) -> None:
        from cccc.contracts.v1.group_bridge import RemoteSendPayload

        p = RemoteSendPayload(text="hi")
        self.assertEqual(p.priority, "normal")
        self.assertFalse(p.reply_required)
        self.assertEqual(p.to, [])
        self.assertEqual(p.refs, [])
        self.assertEqual(p.attachments, [])
        self.assertEqual(p.source_by, "")
        with self.assertRaises(Exception):
            RemoteSendPayload(text="hi", bogus=1)

    def test_registration_record_has_no_raw_token_field(self) -> None:
        from cccc.contracts.v1.group_bridge import RegistrationRecord

        # Raw secret must never be part of the schema; only an opaque reference.
        self.assertNotIn("token", RegistrationRecord.model_fields)
        self.assertNotIn("credential", RegistrationRecord.model_fields)
        self.assertIn("credential_ref", RegistrationRecord.model_fields)

        # extra="forbid" rejects an attempt to smuggle a raw token in.
        with self.assertRaises(Exception):
            RegistrationRecord(
                registration_id="r1",
                group_id="g1",
                url="https://hub.example/",
                created_at="t",
                updated_at="t",
                token="acc_deadbeefdeadbeef",
            )

    def test_receipt_and_error_roundtrip(self) -> None:
        from cccc.contracts.v1.group_bridge import RemoteSendError, RemoteSendReceipt

        err = RemoteSendError(code="transport_error", message="boom", retriable=True)
        r = RemoteSendReceipt(
            ok=False,
            status="failed",
            idempotency_key="k1",
            registration_id="r1",
            error=err,
        )
        d = r.model_dump()
        self.assertEqual(d["status"], "failed")
        self.assertFalse(d["ok"])
        self.assertEqual(d["error"]["code"], "transport_error")
        self.assertTrue(d["error"]["retriable"])

    def test_envelope_wraps_payload(self) -> None:
        from cccc.contracts.v1.group_bridge import RemoteSendEnvelope, RemoteSendPayload

        env = RemoteSendEnvelope(
            src_group_id="g1",
            registration_id="r1",
            idempotency_key="k1",
            payload=RemoteSendPayload(text="hi"),
        )
        self.assertEqual(env.payload.text, "hi")
        self.assertEqual(env.idempotency_key, "k1")


if __name__ == "__main__":
    unittest.main()
