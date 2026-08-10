from __future__ import annotations

import json
import unittest
from unittest.mock import patch


class TestMcpToolsListChanged(unittest.TestCase):
    def setUp(self) -> None:
        from cccc.ports.mcp.main import _reset_session_state_for_tests

        _reset_session_state_for_tests()

    def test_initialize_truthfully_disables_tools_list_changed(self) -> None:
        from cccc.ports.mcp.main import handle_request

        resp = handle_request(
            {
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": {"capabilities": {"tools": {"listChanged": True}}},
            }
        )
        result = resp.get("result") if isinstance(resp.get("result"), dict) else {}
        capabilities = result.get("capabilities") if isinstance(result.get("capabilities"), dict) else {}
        tools_caps = capabilities.get("tools") if isinstance(capabilities.get("tools"), dict) else {}
        self.assertIs(tools_caps.get("listChanged"), False)

    def test_capability_mutation_preserves_explicit_refresh_hint(self) -> None:
        from cccc.ports.mcp.main import handle_request

        with patch(
            "cccc.ports.mcp.main.handle_tool_call",
            return_value={"refresh_required": True, "state": "runnable"},
        ):
            response = handle_request(
                {
                    "jsonrpc": "2.0",
                    "id": 2,
                    "method": "tools/call",
                    "params": {"name": "cccc_capability_enable", "arguments": {}},
                }
            )

        result = response.get("result") if isinstance(response.get("result"), dict) else {}
        content = result.get("content") if isinstance(result.get("content"), list) else []
        payload = json.loads(str(content[0].get("text") or "{}"))
        self.assertTrue(bool(payload.get("refresh_required")))


if __name__ == "__main__":
    unittest.main()
