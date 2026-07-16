import unittest
from pathlib import Path

from cccc.ports.im.commands import format_status


class TestImFormatStatus(unittest.TestCase):
    def test_format_status_uses_title_then_id(self) -> None:
        text = format_status(
            group_title="demo",
            group_state="active",
            running=True,
            actors=[
                {"id": "foreman", "title": "Planner", "role": "foreman", "running": True, "runtime": "claude"},
                {"id": "peer_a", "title": "", "role": "peer", "running": False, "runtime": "codex"},
            ],
        )
        self.assertIn("Planner (claude)", text)
        self.assertIn("peer_a (codex)", text)

    def test_format_status_avoids_duplicate_when_title_equals_id(self) -> None:
        text = format_status(
            group_title="demo",
            group_state="active",
            running=True,
            actors=[
                {"id": "peer_a", "title": "peer_a", "role": "peer", "running": True, "runtime": "codex"},
            ],
        )
        self.assertIn("peer_a (codex)", text)
        self.assertNotIn("(@peer_a)", text)

    def test_format_status_includes_im_capabilities_when_available(self) -> None:
        text = format_status(
            group_title="demo",
            group_state="active",
            running=True,
            actors=[],
            im_status={
                "platform": "telegram",
                "authorized": True,
                "subscribed": True,
                "verbose": False,
                "thread_id": 42,
                "capabilities": {
                    "features": {
                        "text_in": "yes",
                        "text_out": "yes",
                        "files_in": "partial",
                        "files_out": "yes",
                        "threads": "yes",
                        "reactions": "yes",
                        "typing": "yes",
                        "streaming": "no",
                        "voice_in": "no",
                        "markdown": "partial",
                    }
                },
            },
        )
        self.assertIn("IM:", text)
        self.assertIn("Platform: telegram", text)
        self.assertIn("authorized yes | subscribed yes | verbose no | thread 42", text)
        self.assertIn("Text: in yes / out yes", text)
        self.assertIn("Files: in partial / out yes", text)
        self.assertIn("Voice/audio no | Markdown partial", text)

    def test_bridge_keeps_leading_targets_but_not_body_mentions(self) -> None:
        source = Path("src/cccc/ports/im/bridge.py").read_text(encoding="utf-8")
        self.assertIn("Parse recipients from leading args", source)
        self.assertIn("head.startswith(\"@\")", source)
        self.assertNotIn("try @mentions in the message text", source)
        self.assertNotIn("mention_tokens", source)


if __name__ == "__main__":
    unittest.main()
