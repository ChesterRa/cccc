from __future__ import annotations

import unittest
from unittest.mock import patch


class TestMcpToolsListPagination(unittest.TestCase):
    def test_tools_list_supports_limit_and_cursor(self) -> None:
        from cccc.ports.mcp.main import handle_request

        fake_tools = [
            {"name": "cccc_a", "description": "a", "inputSchema": {"type": "object", "properties": {}, "required": []}},
            {"name": "cccc_b", "description": "b", "inputSchema": {"type": "object", "properties": {}, "required": []}},
            {"name": "cccc_c", "description": "c", "inputSchema": {"type": "object", "properties": {}, "required": []}},
        ]

        with patch("cccc.ports.mcp.main.list_tools_for_caller", return_value=fake_tools):
            r1 = handle_request(
                {
                    "jsonrpc": "2.0",
                    "id": 1,
                    "method": "tools/list",
                    "params": {"limit": 2},
                }
            )
            self.assertEqual(r1.get("id"), 1)
            result1 = r1.get("result") if isinstance(r1.get("result"), dict) else {}
            tools1 = result1.get("tools") if isinstance(result1.get("tools"), list) else []
            self.assertEqual(len(tools1), 2)
            self.assertEqual(result1.get("nextCursor"), "2")

            r2 = handle_request(
                {
                    "jsonrpc": "2.0",
                    "id": 2,
                    "method": "tools/list",
                    "params": {"limit": 2, "cursor": result1.get("nextCursor")},
                }
            )
            self.assertEqual(r2.get("id"), 2)
            result2 = r2.get("result") if isinstance(r2.get("result"), dict) else {}
            tools2 = result2.get("tools") if isinstance(result2.get("tools"), list) else []
            self.assertEqual(len(tools2), 1)
            self.assertEqual(result2.get("nextCursor"), "")


if __name__ == "__main__":
    unittest.main()

