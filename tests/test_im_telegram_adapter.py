import unittest
from typing import Any, Dict, Optional


class TestTelegramAdapterInbound(unittest.TestCase):
    def test_private_document_without_caption_is_queued_as_attachment_message(self) -> None:
        from cccc.ports.im.adapters.telegram import TelegramAdapter

        adapter = TelegramAdapter(token="test-token")
        adapter._connected = True

        def _api(method: str, params: Optional[Dict[str, Any]] = None, timeout: int = 35) -> Dict[str, Any]:
            _ = (params, timeout)
            self.assertEqual(method, "getUpdates")
            return {
                "ok": True,
                "result": [
                    {
                        "update_id": 10,
                        "message": {
                            "message_id": 20,
                            "date": 123,
                            "chat": {"id": 100, "type": "private", "first_name": "Alice"},
                            "from": {"id": 200, "username": "alice"},
                            "document": {
                                "file_id": "file-1",
                                "file_unique_id": "unique-1",
                                "file_name": "report.pdf",
                                "mime_type": "application/pdf",
                                "file_size": 42,
                            },
                        },
                    }
                ],
            }

        adapter._api = _api  # type: ignore[method-assign]

        messages = adapter.poll()

        self.assertEqual(len(messages), 1)
        self.assertEqual(messages[0]["text"], "")
        self.assertTrue(bool(messages[0]["routed"]))
        self.assertEqual(messages[0]["attachments"][0]["file_name"], "report.pdf")


if __name__ == "__main__":
    unittest.main()
