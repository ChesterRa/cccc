"""Tests for IM Bridge dynamic key-based authorization (KeyManager)."""

import json
import tempfile
import time
import unittest
from pathlib import Path

from cccc.ports.im.auth import KEY_TTL_SECONDS, KeyManager


class TestKeyManagerBasic(unittest.TestCase):
    """Core KeyManager functionality."""

    def setUp(self) -> None:
        self._td = tempfile.TemporaryDirectory()
        self.state_dir = Path(self._td.name)
        self.km = KeyManager(self.state_dir)

    def tearDown(self) -> None:
        self._td.cleanup()

    def test_generate_key_returns_string(self) -> None:
        key = self.km.generate_key("123", 0, "telegram")
        self.assertIsInstance(key, str)
        self.assertTrue(len(key) > 0)

    def test_get_pending_key_returns_metadata(self) -> None:
        key = self.km.generate_key("123", 0, "telegram")
        meta = self.km.get_pending_key(key)
        self.assertIsNotNone(meta)
        self.assertEqual(meta["chat_id"], "123")
        self.assertEqual(meta["thread_id"], 0)
        self.assertEqual(meta["platform"], "telegram")

    def test_get_pending_key_unknown_returns_none(self) -> None:
        self.assertIsNone(self.km.get_pending_key("nonexistent"))

    def test_is_authorized_initially_false(self) -> None:
        self.assertFalse(self.km.is_authorized("123", 0))

    def test_authorize_marks_chat(self) -> None:
        key = self.km.generate_key("123", 0, "telegram")
        self.km.authorize("123", 0, "telegram", key)
        self.assertTrue(self.km.is_authorized("123", 0))

    def test_authorize_consumes_key(self) -> None:
        key = self.km.generate_key("123", 0, "telegram")
        self.km.authorize("123", 0, "telegram", key)
        # Key should be consumed.
        self.assertIsNone(self.km.get_pending_key(key))

    def test_revoke_removes_authorization(self) -> None:
        key = self.km.generate_key("123", 0, "telegram")
        self.km.authorize("123", 0, "telegram", key)
        self.assertTrue(self.km.is_authorized("123", 0))
        revoked = self.km.revoke("123", 0)
        self.assertTrue(revoked)
        self.assertFalse(self.km.is_authorized("123", 0))

    def test_revoke_nonexistent_returns_false(self) -> None:
        self.assertFalse(self.km.revoke("999", 0))

    def test_list_authorized_empty(self) -> None:
        self.assertEqual(self.km.list_authorized(), [])

    def test_list_authorized_after_authorize(self) -> None:
        key = self.km.generate_key("123", 0, "telegram")
        self.km.authorize("123", 0, "telegram", key)
        result = self.km.list_authorized()
        self.assertEqual(len(result), 1)
        self.assertEqual(result[0]["chat_id"], "123")


class TestKeyManagerThreadId(unittest.TestCase):
    """Thread-id scoping: chat_id:thread_id are independent."""

    def setUp(self) -> None:
        self._td = tempfile.TemporaryDirectory()
        self.state_dir = Path(self._td.name)
        self.km = KeyManager(self.state_dir)

    def tearDown(self) -> None:
        self._td.cleanup()

    def test_different_thread_ids_are_independent(self) -> None:
        k1 = self.km.generate_key("100", 0, "telegram")
        k2 = self.km.generate_key("100", 42, "telegram")
        self.km.authorize("100", 0, "telegram", k1)
        self.assertTrue(self.km.is_authorized("100", 0))
        self.assertFalse(self.km.is_authorized("100", 42))
        self.km.authorize("100", 42, "telegram", k2)
        self.assertTrue(self.km.is_authorized("100", 42))


class TestKeyManagerPersistence(unittest.TestCase):
    """Data survives reload from disk."""

    def setUp(self) -> None:
        self._td = tempfile.TemporaryDirectory()
        self.state_dir = Path(self._td.name)

    def tearDown(self) -> None:
        self._td.cleanup()

    def test_authorized_survives_reload(self) -> None:
        km1 = KeyManager(self.state_dir)
        key = km1.generate_key("123", 0, "telegram")
        km1.authorize("123", 0, "telegram", key)

        km2 = KeyManager(self.state_dir)
        self.assertTrue(km2.is_authorized("123", 0))

    def test_pending_key_survives_reload(self) -> None:
        km1 = KeyManager(self.state_dir)
        key = km1.generate_key("123", 0, "telegram")

        km2 = KeyManager(self.state_dir)
        meta = km2.get_pending_key(key)
        self.assertIsNotNone(meta)
        self.assertEqual(meta["chat_id"], "123")


class TestKeyManagerExpiry(unittest.TestCase):
    """Key TTL enforcement."""

    def setUp(self) -> None:
        self._td = tempfile.TemporaryDirectory()
        self.state_dir = Path(self._td.name)
        self.km = KeyManager(self.state_dir)

    def tearDown(self) -> None:
        self._td.cleanup()

    def test_expired_key_returns_none(self) -> None:
        key = self.km.generate_key("123", 0, "telegram")
        # Manually expire the key.
        self.km._pending[key]["created_at"] = time.time() - KEY_TTL_SECONDS - 1
        self.km._save_pending()
        self.assertIsNone(self.km.get_pending_key(key))


class TestKeyManagerAtomicWrite(unittest.TestCase):
    """Atomic writes produce valid JSON."""

    def setUp(self) -> None:
        self._td = tempfile.TemporaryDirectory()
        self.state_dir = Path(self._td.name)
        self.km = KeyManager(self.state_dir)

    def tearDown(self) -> None:
        self._td.cleanup()

    def test_pending_file_is_valid_json(self) -> None:
        self.km.generate_key("123", 0, "telegram")
        path = self.state_dir / "im_pending_keys.json"
        self.assertTrue(path.exists())
        data = json.loads(path.read_text(encoding="utf-8"))
        self.assertIsInstance(data, dict)
        self.assertEqual(len(data), 1)

    def test_authorized_file_is_valid_json(self) -> None:
        key = self.km.generate_key("123", 0, "telegram")
        self.km.authorize("123", 0, "telegram", key)
        path = self.state_dir / "im_authorized_chats.json"
        self.assertTrue(path.exists())
        data = json.loads(path.read_text(encoding="utf-8"))
        self.assertIsInstance(data, dict)
        self.assertEqual(len(data), 1)

    def test_no_tmp_files_left_behind(self) -> None:
        key = self.km.generate_key("123", 0, "telegram")
        self.km.authorize("123", 0, "telegram", key)
        tmp_files = list(self.state_dir.glob("*.tmp"))
        self.assertEqual(tmp_files, [])


class TestMCPImBind(unittest.TestCase):
    """MCP cccc_im_bind tool integration (unit-level, no daemon)."""

    def setUp(self) -> None:
        self._td = tempfile.TemporaryDirectory()
        self.state_dir = Path(self._td.name)
        self.km = KeyManager(self.state_dir)

    def tearDown(self) -> None:
        self._td.cleanup()

    def test_toolspecs_contains_cccc_im_bind(self) -> None:
        from cccc.ports.mcp.toolspecs import MCP_TOOLS
        names = [t["name"] for t in MCP_TOOLS]
        self.assertIn("cccc_im_bind", names)

    def test_bind_valid_key(self) -> None:
        """Normal bind flow: generate key → bind → authorized."""
        key = self.km.generate_key("500", 0, "telegram")
        pending = self.km.get_pending_key(key)
        self.assertIsNotNone(pending)
        self.km.authorize("500", 0, "telegram", key)
        self.assertTrue(self.km.is_authorized("500", 0))

    def test_bind_empty_key_rejected(self) -> None:
        """Empty key should not match any pending entry."""
        self.assertIsNone(self.km.get_pending_key(""))

    def test_bind_expired_key_rejected(self) -> None:
        """Expired keys must return None."""
        key = self.km.generate_key("600", 0, "telegram")
        self.km._pending[key]["created_at"] = time.time() - KEY_TTL_SECONDS - 1
        self.km._save_pending()
        self.assertIsNone(self.km.get_pending_key(key))


try:
    from cccc.daemon.ops.im_ops import _load_km
    _HAS_DAEMON_DEPS = True
except ImportError:
    _HAS_DAEMON_DEPS = False


@unittest.skipUnless(_HAS_DAEMON_DEPS, "daemon deps (pydantic) not available")
class TestImOpsLoadKm(unittest.TestCase):
    """Test the _load_km factory function in im_ops."""

    def test_missing_group_id_returns_error(self) -> None:
        err, km = _load_km({})
        self.assertIsNotNone(err)
        self.assertFalse(err.ok)
        self.assertIsNone(km)

    def test_nonexistent_group_returns_error(self) -> None:
        err, km = _load_km({"group_id": "g_nonexistent_xyz"})
        self.assertIsNotNone(err)
        self.assertFalse(err.ok)
        self.assertIsNone(km)


if __name__ == "__main__":
    unittest.main()
