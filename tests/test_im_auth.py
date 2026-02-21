"""Tests for IM Bridge dynamic key-based authorization (KeyManager)."""

import json
import tempfile
import time
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch

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

    def test_list_pending_contains_generated_key(self) -> None:
        key = self.km.generate_key("123", 0, "telegram")
        pending = self.km.list_pending()
        self.assertEqual(len(pending), 1)
        self.assertEqual(pending[0]["key"], key)
        self.assertEqual(pending[0]["chat_id"], "123")

    def test_reject_pending_removes_key(self) -> None:
        key = self.km.generate_key("123", 0, "telegram")
        self.assertTrue(self.km.reject_pending(key))
        self.assertIsNone(self.km.get_pending_key(key))
        self.assertEqual(self.km.list_pending(), [])

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

    def test_list_pending_purges_expired_keys(self) -> None:
        key = self.km.generate_key("123", 0, "telegram")
        self.km._pending[key]["created_at"] = time.time() - KEY_TTL_SECONDS - 1
        self.km._save_pending()
        self.assertEqual(self.km.list_pending(), [])


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


class TestImRevokeSemantics(unittest.TestCase):
    """Revoke should also stop outbound subscription delivery."""

    def setUp(self) -> None:
        self._td = tempfile.TemporaryDirectory()
        self.group_path = Path(self._td.name)
        self.state_dir = self.group_path / "state"
        self.state_dir.mkdir(parents=True, exist_ok=True)

    def tearDown(self) -> None:
        self._td.cleanup()

    def test_revoke_also_unsubscribes_chat(self) -> None:
        from cccc.daemon.ops import im_ops
        from cccc.ports.im.subscribers import SubscriberManager

        km = KeyManager(self.state_dir)
        sm = SubscriberManager(self.state_dir)

        key = km.generate_key("chat1", 0, "telegram")
        km.authorize("chat1", 0, "telegram", key)
        sm.subscribe("chat1", chat_title="demo", thread_id=0, platform="telegram")

        self.assertTrue(km.is_authorized("chat1", 0))
        self.assertTrue(sm.is_subscribed("chat1", 0))

        fake_group = SimpleNamespace(path=self.group_path)
        with patch("cccc.daemon.ops.im_ops._load_km", return_value=(None, km, fake_group)):
            resp = im_ops.handle_im_revoke_chat({"group_id": "g_demo", "chat_id": "chat1", "thread_id": 0})

        self.assertTrue(resp.ok, getattr(resp, "error", None))
        result = resp.result if isinstance(resp.result, dict) else {}
        self.assertTrue(bool(result.get("revoked")))
        self.assertTrue(bool(result.get("unsubscribed")))

        # Reload managers from disk to assert persisted behavior.
        km2 = KeyManager(self.state_dir)
        sm2 = SubscriberManager(self.state_dir)
        self.assertFalse(km2.is_authorized("chat1", 0))
        self.assertFalse(sm2.is_subscribed("chat1", 0))

    def test_list_pending_returns_generated_key(self) -> None:
        from cccc.daemon.ops import im_ops

        km = KeyManager(self.state_dir)
        key = km.generate_key("chat2", 0, "telegram")
        fake_group = SimpleNamespace(path=self.group_path)
        with patch("cccc.daemon.ops.im_ops._load_km", return_value=(None, km, fake_group)):
            resp = im_ops.handle_im_list_pending({"group_id": "g_demo"})

        self.assertTrue(resp.ok, getattr(resp, "error", None))
        result = resp.result if isinstance(resp.result, dict) else {}
        pending = result.get("pending", [])
        self.assertIsInstance(pending, list)
        self.assertEqual(len(pending), 1)
        self.assertEqual(pending[0].get("key"), key)

    def test_reject_pending_is_idempotent(self) -> None:
        from cccc.daemon.ops import im_ops

        km = KeyManager(self.state_dir)
        key = km.generate_key("chat3", 0, "telegram")
        fake_group = SimpleNamespace(path=self.group_path)
        with patch("cccc.daemon.ops.im_ops._load_km", return_value=(None, km, fake_group)):
            first = im_ops.handle_im_reject_pending({"group_id": "g_demo", "key": key})
            second = im_ops.handle_im_reject_pending({"group_id": "g_demo", "key": key})

        self.assertTrue(first.ok, getattr(first, "error", None))
        self.assertTrue(second.ok, getattr(second, "error", None))
        first_result = first.result if isinstance(first.result, dict) else {}
        second_result = second.result if isinstance(second.result, dict) else {}
        self.assertTrue(bool(first_result.get("rejected")))
        self.assertFalse(bool(second_result.get("rejected")))

    def test_reject_pending_requires_key(self) -> None:
        from cccc.daemon.ops import im_ops

        km = KeyManager(self.state_dir)
        fake_group = SimpleNamespace(path=self.group_path)
        with patch("cccc.daemon.ops.im_ops._load_km", return_value=(None, km, fake_group)):
            resp = im_ops.handle_im_reject_pending({"group_id": "g_demo", "key": ""})

        self.assertFalse(resp.ok)
        err = resp.error
        self.assertIsNotNone(err)
        assert err is not None
        self.assertEqual(err.code, "missing_key")


class TestImBridgeOutboundAuthGuard(unittest.TestCase):
    """Bridge should not forward outbound events to unauthorized chats."""

    class _FakeAdapter:
        platform = "telegram"

        def __init__(self) -> None:
            self.sent_messages: list[tuple[str, str, int]] = []
            self.formatted_calls: list[tuple[str, list[str], str, bool]] = []

        def format_outbound(self, by: str, to: object, text: str, is_system: bool) -> str:
            to_list = [str(item) for item in to] if isinstance(to, list) else []
            self.formatted_calls.append((str(by), to_list, str(text or ""), bool(is_system)))
            return str(text or "")

        def send_file(self, chat_id: str, file_path: Path, filename: str, caption: str = "", thread_id: int = 0) -> bool:
            _ = (chat_id, file_path, filename, caption, thread_id)
            return False

        def send_message(self, chat_id: str, text: str, thread_id: int = 0) -> bool:
            self.sent_messages.append((str(chat_id), str(text), int(thread_id or 0)))
            return True

    def setUp(self) -> None:
        self._td = tempfile.TemporaryDirectory()
        self.group_path = Path(self._td.name)
        self.state_dir = self.group_path / "state"
        self.state_dir.mkdir(parents=True, exist_ok=True)
        (self.group_path / "ledger.jsonl").write_text("", encoding="utf-8")

    def tearDown(self) -> None:
        self._td.cleanup()

    def test_process_outbound_reloads_auth_and_blocks_revoked_chat(self) -> None:
        from cccc.ports.im.bridge import IMBridge
        from cccc.ports.im.subscribers import SubscriberManager

        km = KeyManager(self.state_dir)
        sm = SubscriberManager(self.state_dir)
        key = km.generate_key("chat_auth", 0, "telegram")
        km.authorize("chat_auth", 0, "telegram", key)
        sm.subscribe("chat_auth", chat_title="auth", thread_id=0, platform="telegram")

        # External revoke from daemon process (bridge still has stale in-memory auth).
        km_external = KeyManager(self.state_dir)
        km_external.revoke("chat_auth", 0)

        fake_group = SimpleNamespace(
            group_id="g_demo",
            path=self.group_path,
            ledger_path=self.group_path / "ledger.jsonl",
            doc={"title": "demo", "im": {}},
        )
        adapter = self._FakeAdapter()
        bridge = IMBridge(group=fake_group, adapter=adapter)

        bridge.watcher.poll = lambda: [  # type: ignore[method-assign]
            {
                "kind": "chat.message",
                "by": "foreman",
                "data": {"text": "hello", "to": ["user"], "attachments": []},
            }
        ]
        bridge._process_outbound()

        self.assertEqual(adapter.sent_messages, [])

    def test_outbound_header_uses_actor_title_first(self) -> None:
        from cccc.ports.im.bridge import IMBridge
        from cccc.ports.im.subscribers import SubscriberManager

        km = KeyManager(self.state_dir)
        sm = SubscriberManager(self.state_dir)
        key = km.generate_key("chat_auth", 0, "telegram")
        km.authorize("chat_auth", 0, "telegram", key)
        sm.subscribe("chat_auth", chat_title="auth", thread_id=0, platform="telegram")

        fake_group = SimpleNamespace(
            group_id="g_demo",
            path=self.group_path,
            ledger_path=self.group_path / "ledger.jsonl",
            doc={
                "title": "demo",
                "im": {},
                "actors": [
                    {"id": "foreman", "title": "Captain"},
                    {"id": "peer_a", "title": "Reviewer"},
                ],
            },
        )
        adapter = self._FakeAdapter()
        bridge = IMBridge(group=fake_group, adapter=adapter)

        bridge.watcher.poll = lambda: [  # type: ignore[method-assign]
            {
                "kind": "chat.message",
                "by": "foreman",
                "data": {"text": "review this", "to": ["@all", "peer_a"], "attachments": []},
            }
        ]
        bridge._process_outbound()

        self.assertEqual(len(adapter.formatted_calls), 1)
        by, to, _text, _is_system = adapter.formatted_calls[0]
        self.assertEqual(by, "Captain")
        self.assertEqual(to, ["@all", "Reviewer"])

    def test_subscribe_reloads_auth_state_and_avoids_stale_authorized_decision(self) -> None:
        from cccc.ports.im.bridge import IMBridge

        km = KeyManager(self.state_dir)
        key = km.generate_key("chat_auth", 0, "telegram")
        km.authorize("chat_auth", 0, "telegram", key)

        fake_group = SimpleNamespace(
            group_id="g_demo",
            path=self.group_path,
            ledger_path=self.group_path / "ledger.jsonl",
            doc={"title": "demo", "im": {}},
        )
        adapter = self._FakeAdapter()
        bridge = IMBridge(group=fake_group, adapter=adapter)

        # External revoke after bridge initialization (bridge in-memory auth is stale).
        km_external = KeyManager(self.state_dir)
        km_external.revoke("chat_auth", 0)

        bridge._handle_subscribe("chat_auth", "auth", thread_id=0)

        self.assertEqual(len(adapter.sent_messages), 1)
        _chat_id, text, _thread_id = adapter.sent_messages[0]
        self.assertIn("Authorization required", text)


try:
    from cccc.daemon.ops.im_ops import _load_km
    _HAS_DAEMON_DEPS = True
except ImportError:
    _HAS_DAEMON_DEPS = False


@unittest.skipUnless(_HAS_DAEMON_DEPS, "daemon deps (pydantic) not available")
class TestImOpsLoadKm(unittest.TestCase):
    """Test the _load_km factory function in im_ops."""

    def test_missing_group_id_returns_error(self) -> None:
        err, km, group = _load_km({})
        self.assertIsNotNone(err)
        self.assertFalse(err.ok)
        self.assertIsNone(km)
        self.assertIsNone(group)

    def test_nonexistent_group_returns_error(self) -> None:
        err, km, group = _load_km({"group_id": "g_nonexistent_xyz"})
        self.assertIsNotNone(err)
        self.assertFalse(err.ok)
        self.assertIsNone(km)
        self.assertIsNone(group)


if __name__ == "__main__":
    unittest.main()
