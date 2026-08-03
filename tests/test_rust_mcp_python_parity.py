from __future__ import annotations

import re
import unittest
from pathlib import Path

from cccc.ports.mcp.toolspecs import MCP_TOOLS


RUST_ONLY_TOOLS: set[str] = set()


class TestRustMcpPythonParity(unittest.TestCase):
    def test_static_tool_catalog_names_match(self) -> None:
        root = Path(__file__).resolve().parents[1]
        rust = (root / "crates/cccc-mcp/src/tools.rs").read_text(encoding="utf-8")
        rust_names = set(re.findall(r'\(\s*"(cccc_[a-z0-9_]+)"\s*,', rust))
        python_names = {
            str(tool.get("name") or "")
            for tool in MCP_TOOLS
            if isinstance(tool, dict) and str(tool.get("name") or "")
        }
        self.assertSetEqual(rust_names - RUST_ONLY_TOOLS, python_names)
        self.assertTrue(RUST_ONLY_TOOLS <= rust_names)


if __name__ == "__main__":
    unittest.main()
