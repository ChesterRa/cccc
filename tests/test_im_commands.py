import unittest

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


if __name__ == "__main__":
    unittest.main()
