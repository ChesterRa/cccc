from pathlib import Path
import unittest


class TestCliReferenceParity(unittest.TestCase):
    def test_cli_reference_avoids_removed_commands_and_options(self) -> None:
        repo_root = Path(__file__).resolve().parents[1]
        cli_doc = repo_root / "docs" / "reference" / "cli.md"
        text = cli_doc.read_text(encoding="utf-8")

        self.assertNotIn("cccc group edit", text)
        self.assertNotIn("cccc groups --json", text)

    def test_cli_reference_includes_group_set_state_with_stopped(self) -> None:
        repo_root = Path(__file__).resolve().parents[1]
        cli_doc = repo_root / "docs" / "reference" / "cli.md"
        text = cli_doc.read_text(encoding="utf-8")

        self.assertIn("cccc group set-state idle", text)
        self.assertIn("active/idle/paused/stopped", text)


if __name__ == "__main__":
    unittest.main()
