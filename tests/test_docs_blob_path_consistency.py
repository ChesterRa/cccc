from pathlib import Path
import unittest


class TestDocsBlobPathConsistency(unittest.TestCase):
    def test_architecture_doc_uses_state_blobs_path(self) -> None:
        repo_root = Path(__file__).resolve().parents[1]
        arch_doc = repo_root / "docs" / "reference" / "architecture.md"
        text = arch_doc.read_text(encoding="utf-8")

        self.assertIn("state/blobs", text)
        self.assertNotIn("state/ledger/blobs", text)


if __name__ == "__main__":
    unittest.main()
