import os
import tempfile
import unittest
from pathlib import Path


class TestGroupBridgeRegistration(unittest.TestCase):
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

    def test_normalize_url(self) -> None:
        from cccc.kernel.group_bridge.registration import normalize_url

        self.assertEqual(normalize_url("HTTPS://Hub.Example.com:443/api/"), "https://hub.example.com/api")
        self.assertEqual(normalize_url("http://hub.example.com:80/"), "http://hub.example.com")
        self.assertEqual(normalize_url("https://hub.example.com"), "https://hub.example.com")
        self.assertEqual(normalize_url("  https://hub.example.com/x/  "), "https://hub.example.com/x")

    def test_upsert_dedup_same_target(self) -> None:
        from cccc.kernel.group_bridge.registration import list_registrations, upsert_registration

        _, cleanup = self._with_home()
        try:
            a = upsert_registration("g1", "https://hub.example/", credential_ref="sec_remote_1")
            # Same group + URL that normalizes identically => upsert, not a new record.
            b = upsert_registration("g1", "HTTPS://hub.example", credential_ref="sec_remote_2")
            self.assertEqual(a["registration_id"], b["registration_id"])
            self.assertEqual(len(list_registrations()), 1)
            self.assertEqual(b["credential_ref"], "sec_remote_2")
            self.assertEqual(b["url"], "https://hub.example")
        finally:
            cleanup()

    def test_distinct_group_or_url_distinct_record(self) -> None:
        from cccc.kernel.group_bridge.registration import list_registrations, upsert_registration

        _, cleanup = self._with_home()
        try:
            upsert_registration("g1", "https://hub.example/", credential_ref="fsec_remote_1")
            upsert_registration("g2", "https://hub.example/", credential_ref="fsec_remote_1")
            upsert_registration("g1", "https://other.example/", credential_ref="fsec_remote_1")
            self.assertEqual(len(list_registrations()), 3)
        finally:
            cleanup()

    def test_get_and_delete(self) -> None:
        from cccc.kernel.group_bridge.registration import (
            delete_registration,
            get_registration,
            upsert_registration,
        )

        _, cleanup = self._with_home()
        try:
            rec = upsert_registration("g1", "https://hub.example/", credential_ref="fsec_remote_1")
            rid = rec["registration_id"]
            self.assertIsNotNone(get_registration(rid))
            self.assertTrue(delete_registration(rid))
            self.assertIsNone(get_registration(rid))
            self.assertFalse(delete_registration(rid))
        finally:
            cleanup()

    def test_rejects_token_shaped_credential_ref(self) -> None:
        from cccc.kernel.group_bridge.registration import list_registrations, upsert_registration

        home, cleanup = self._with_home()
        try:
            # A raw access-token-shaped credential_ref must be rejected loudly,
            # never silently rewritten, and never persisted.
            with self.assertRaises(ValueError):
                upsert_registration("g1", "https://hub.example/", credential_ref="acc_deadbeefdeadbeef")
            self.assertEqual(len(list_registrations()), 0)
            path = home / "group_bridge_registrations.yaml"
            if path.exists():
                self.assertNotIn("acc_deadbeefdeadbeef", path.read_text(encoding="utf-8"))
        finally:
            cleanup()

    def test_rejects_raw_secret_shaped_credential_refs(self) -> None:
        from cccc.kernel.group_bridge.registration import list_registrations, upsert_registration

        home, cleanup = self._with_home()
        raw_values = [
            "ghp_1234567890abcdef1234567890abcdef123456",
            "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiJ1c2VyIn0.signature",
            "bearer-token-like-long-secret",
            "secret-token-like-long-secret",
            "secsecretlong",
            "credential-ref-1",
            "credsecretlong",
        ]
        try:
            for raw in raw_values:
                with self.subTest(raw=raw):
                    with self.assertRaises(ValueError) as ctx:
                        upsert_registration("g1", "https://hub.example/", credential_ref=raw)
                    self.assertNotIn(raw, str(ctx.exception))
            self.assertEqual(len(list_registrations()), 0)
            path = home / "group_bridge_registrations.yaml"
            if path.exists():
                text = path.read_text(encoding="utf-8")
                for raw in raw_values:
                    self.assertNotIn(raw, text)
        finally:
            cleanup()

    def test_allows_empty_and_reference_shaped_credential_refs(self) -> None:
        from cccc.kernel.group_bridge.registration import upsert_registration

        _, cleanup = self._with_home()
        try:
            empty = upsert_registration("g1", "https://hub.example/", credential_ref="")
            self.assertEqual(empty["credential_ref"], "")

            secure = upsert_registration("g1", "https://other.example/", credential_ref="sec_remote_peer")
            self.assertEqual(secure["credential_ref"], "sec_remote_peer")

            fsec = upsert_registration("g1", "https://fsec.example/", credential_ref="fsec_remote_peer")
            self.assertEqual(fsec["credential_ref"], "fsec_remote_peer")
        finally:
            cleanup()

    def test_peer_http_transport_is_rejected(self) -> None:
        from cccc.kernel.group_bridge.registration import list_registrations, upsert_registration

        _, cleanup = self._with_home()
        try:
            with self.assertRaises(ValueError) as ctx:
                upsert_registration("g1", "https://hub.example/", transport="peer_cccc_http", remote_group_id="g_remote")
            self.assertIn("unsupported group_bridge transport", str(ctx.exception))
            self.assertEqual(len(list_registrations()), 0)
        finally:
            cleanup()

    def test_group_bridge_session_requires_peer_and_remote_group(self) -> None:
        from cccc.kernel.group_bridge.pairing import _upsert_approved_session_registration
        from cccc.kernel.group_bridge.registration import list_registrations

        _, cleanup = self._with_home()
        try:
            with self.assertRaises(ValueError) as missing_peer:
                _upsert_approved_session_registration(
                    "g1",
                    "http://remote.example:8848",
                    remote_group_id="g_remote",
                    remote_peer_id="",
                )
            self.assertIn("remote_peer_id", str(missing_peer.exception))

            with self.assertRaises(ValueError) as missing_group:
                _upsert_approved_session_registration(
                    "g1",
                    "http://remote.example:8848",
                    remote_group_id="",
                    remote_peer_id="peer-remote",
                )
            self.assertIn("remote_group_id", str(missing_group.exception))

            with self.assertRaises(ValueError) as placeholder:
                _upsert_approved_session_registration(
                    "g1",
                    "session://peer-remote",
                    remote_group_id="g_remote",
                    remote_peer_id="peer-remote",
                )
            self.assertIn("concrete remote endpoint", str(placeholder.exception))
            self.assertEqual(len(list_registrations()), 0)
        finally:
            cleanup()

    def test_direct_group_bridge_session_registration_is_rejected(self) -> None:
        from cccc.kernel.group_bridge.registration import list_registrations, upsert_registration

        _, cleanup = self._with_home()
        try:
            with self.assertRaises(ValueError) as ctx:
                upsert_registration(
                    "g1",
                    "session://peer-remote",
                    transport="group_bridge_session",
                    remote_group_id="g_remote",
                    remote_peer_id="peer-remote",
                )
            self.assertIn("pairing", str(ctx.exception))
            self.assertEqual(len(list_registrations()), 0)
        finally:
            cleanup()

    def test_no_raw_token_persisted(self) -> None:
        from cccc.kernel.group_bridge.registration import upsert_registration

        home, cleanup = self._with_home()
        try:
            upsert_registration("g1", "https://hub.example/", credential_ref="fsec_remote_1")
            path = home / "group_bridge_registrations.yaml"
            self.assertTrue(path.exists())
            text = path.read_text(encoding="utf-8")
            # Only the opaque credential reference is stored, never a raw secret.
            self.assertIn("fsec_remote_1", text)
            self.assertNotIn("acc_", text)
            self.assertNotIn("token", text)
        finally:
            cleanup()


if __name__ == "__main__":
    unittest.main()
