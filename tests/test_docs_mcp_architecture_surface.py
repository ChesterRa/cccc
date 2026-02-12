from pathlib import Path
import unittest


class TestDocsMcpArchitectureSurface(unittest.TestCase):
    def test_architecture_doc_avoids_hardcoded_mcp_namespace_count(self) -> None:
        repo_root = Path(__file__).resolve().parents[1]
        doc = repo_root / "docs" / "reference" / "architecture.md"
        text = doc.read_text(encoding="utf-8")

        self.assertNotIn("MCP tools across 4 namespaces", text)
        self.assertIn("capability groups", text)
        self.assertIn("cccc_automation_manage", text)
        self.assertIn("cccc_terminal_tail", text)
        self.assertIn("cccc_debug_*", text)


if __name__ == "__main__":
    unittest.main()
