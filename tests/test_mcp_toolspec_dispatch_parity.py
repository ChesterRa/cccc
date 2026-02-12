import re
import unittest
from pathlib import Path

from cccc.ports.mcp.toolspecs import MCP_TOOLS


class TestMcpToolspecDispatchParity(unittest.TestCase):
    def test_toolspec_names_are_unique(self) -> None:
        names = [str(t.get("name") or "").strip() for t in MCP_TOOLS if isinstance(t, dict)]
        self.assertEqual(
            len(names),
            len(set(names)),
            msg="Duplicate MCP tool names in toolspecs.py",
        )

    def test_toolspec_and_dispatch_names_match_exactly(self) -> None:
        spec_names = {
            str(t.get("name") or "").strip()
            for t in MCP_TOOLS
            if isinstance(t, dict) and str(t.get("name") or "").strip()
        }

        repo_root = Path(__file__).resolve().parents[1]
        server_path = repo_root / "src" / "cccc" / "ports" / "mcp" / "server.py"
        text = server_path.read_text(encoding="utf-8")
        impl_names = set(re.findall(r'if name == "([a-z0-9_]+)"', text))

        self.assertEqual(
            sorted(spec_names - impl_names),
            [],
            msg=f"Tools declared in MCP_TOOLS but not dispatched in server.py: {sorted(spec_names - impl_names)}",
        )
        self.assertEqual(
            sorted(impl_names - spec_names),
            [],
            msg=f"Tools dispatched in server.py but missing from MCP_TOOLS: {sorted(impl_names - spec_names)}",
        )


if __name__ == "__main__":
    unittest.main()
