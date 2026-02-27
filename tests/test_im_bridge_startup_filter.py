import tempfile
import unittest
from pathlib import Path
from typing import Any, Dict, List, Optional

from cccc.kernel.group import Group
from cccc.ports.im.adapters.base import IMAdapter
from cccc.ports.im.bridge import IMBridge
from cccc.ports.im.commands import ParsedCommand


class _FakeAdapter(IMAdapter):
    platform = "telegram"

    def __init__(self, messages: List[Dict[str, Any]]):
        self._messages = list(messages)
        self._connected = False

    def connect(self) -> bool:
        self._connected = True
        return True

    def disconnect(self) -> None:
        self._connected = False

    def poll(self) -> List[Dict[str, Any]]:
        if not self._connected:
            return []
        out = list(self._messages)
        self._messages = []
        return out

    def send_message(self, chat_id: str, text: str, thread_id: Optional[int] = None) -> bool:
        _ = chat_id
        _ = text
        _ = thread_id
        return True

    def get_chat_title(self, chat_id: str) -> str:
        return str(chat_id)


class TestImBridgeStartupFilter(unittest.TestCase):
    def _make_group(self, root: Path) -> Group:
        group_path = root / "g_test"
        (group_path / "state").mkdir(parents=True, exist_ok=True)
        (group_path / "ledger.jsonl").touch(exist_ok=True)
        return Group(group_id="g_test", path=group_path, doc={"group_id": "g_test"})

    def test_drop_historical_inbound_by_timestamp(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            group = self._make_group(Path(td))
            adapter = _FakeAdapter(
                [
                    {
                        "chat_id": "c1",
                        "chat_title": "t",
                        "chat_type": "private",
                        "routed": True,
                        "thread_id": 0,
                        "text": "old",
                        "from_user": "u",
                        "message_id": "m_old",
                        "timestamp": 80.0,
                    },
                    {
                        "chat_id": "c1",
                        "chat_title": "t",
                        "chat_type": "private",
                        "routed": True,
                        "thread_id": 0,
                        "text": "new",
                        "from_user": "u",
                        "message_id": "m_new",
                        "timestamp": 110.0,
                    },
                ]
            )
            bridge = IMBridge(group=group, adapter=adapter)
            self.assertTrue(bridge.start())
            bridge._connected_at = 100.0
            bridge.key_manager.is_authorized = lambda *_args, **_kwargs: True  # type: ignore[method-assign]

            processed: List[str] = []

            def _capture(
                chat_id: str,
                parsed: ParsedCommand,
                from_user: str,
                *,
                attachments: List[Dict[str, Any]],
                thread_id: int = 0,
                message_id: str = "",
                from_user_id: str = "",
            ) -> None:
                _ = chat_id
                _ = parsed
                _ = from_user
                _ = attachments
                _ = thread_id
                _ = message_id
                _ = from_user_id
                processed.append(parsed.text)

            bridge._handle_message = _capture  # type: ignore[method-assign]
            bridge._process_inbound()
            self.assertEqual(processed, ["new"])

    def test_keep_message_without_timestamp(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            group = self._make_group(Path(td))
            adapter = _FakeAdapter(
                [
                    {
                        "chat_id": "c1",
                        "chat_title": "t",
                        "chat_type": "private",
                        "routed": True,
                        "thread_id": 0,
                        "text": "no-ts",
                        "from_user": "u",
                        "message_id": "m1",
                    }
                ]
            )
            bridge = IMBridge(group=group, adapter=adapter)
            self.assertTrue(bridge.start())
            bridge._connected_at = 100.0
            bridge.key_manager.is_authorized = lambda *_args, **_kwargs: True  # type: ignore[method-assign]

            processed: List[str] = []

            def _capture(
                chat_id: str,
                parsed: ParsedCommand,
                from_user: str,
                *,
                attachments: List[Dict[str, Any]],
                thread_id: int = 0,
                message_id: str = "",
                from_user_id: str = "",
            ) -> None:
                _ = chat_id
                _ = parsed
                _ = from_user
                _ = attachments
                _ = thread_id
                _ = message_id
                _ = from_user_id
                processed.append(parsed.text)

            bridge._handle_message = _capture  # type: ignore[method-assign]
            bridge._process_inbound()
            self.assertEqual(processed, ["no-ts"])

    def test_millisecond_timestamp_is_supported(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            group = self._make_group(Path(td))
            adapter = _FakeAdapter([])
            bridge = IMBridge(group=group, adapter=adapter)
            self.assertTrue(bridge.start())
            bridge._connected_at = 1_700_000_000.0

            old_msg = {"timestamp": 1_699_999_000_000.0}
            new_msg = {"timestamp": 1_700_000_100_000.0}
            self.assertTrue(bridge._is_historical_inbound(old_msg))
            self.assertFalse(bridge._is_historical_inbound(new_msg))


if __name__ == "__main__":
    unittest.main()
