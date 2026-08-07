from __future__ import annotations

import json
import unittest
from pathlib import Path

from cccc.ports.mcp.toolspecs import MCP_TOOLS


class TestRustMcpPythonParity(unittest.TestCase):
    def test_python_and_rust_use_the_same_language_neutral_contract(self) -> None:
        root = Path(__file__).resolve().parents[1]
        contract_path = root / "src/cccc/resources/mcp_tools.json"
        contract = json.loads(contract_path.read_text(encoding="utf-8"))
        rust = (root / "crates/cccc-mcp/src/tools.rs").read_text(encoding="utf-8")

        self.assertEqual(MCP_TOOLS, contract)
        self.assertIn('include_str!("../../../src/cccc/resources/mcp_tools.json")', rust)
        self.assertFalse((root / "crates/cccc-mcp/src/schemas.rs").exists())

    def test_full_contract_has_unique_complete_entries(self) -> None:
        names = [str(tool.get("name") or "") for tool in MCP_TOOLS]
        self.assertEqual(len(names), 59)
        self.assertEqual(len(set(names)), len(names))
        for tool in MCP_TOOLS:
            self.assertEqual(set(tool) - {"annotations"}, {"name", "description", "inputSchema"})
            self.assertEqual(tool["inputSchema"].get("type"), "object")


if __name__ == "__main__":
    unittest.main()
