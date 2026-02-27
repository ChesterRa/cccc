from __future__ import annotations

import os
import unittest
from unittest.mock import patch

from cccc.ports.mcp.common import MCPError


class TestMcpDynamicCapabilityTools(unittest.TestCase):
    def test_list_tools_for_caller_appends_dynamic_specs(self) -> None:
        from cccc.ports.mcp.server import list_tools_for_caller

        with patch.dict(os.environ, {"CCCC_GROUP_ID": "g1", "CCCC_ACTOR_ID": "peer-1"}, clear=False), patch(
            "cccc.ports.mcp.server._call_daemon_or_raise",
            return_value={
                "visible_tools": ["cccc_help", "cccc_ext_deadbeef_echo"],
                "dynamic_tools": [
                    {
                        "name": "cccc_ext_deadbeef_echo",
                        "description": "echo",
                        "inputSchema": {"type": "object", "properties": {}, "required": []},
                        "capability_id": "mcp:test-server",
                        "real_tool_name": "echo",
                    }
                ],
            },
        ):
            tools = list_tools_for_caller()
        names = {str(item.get("name") or "") for item in tools if isinstance(item, dict)}
        self.assertIn("cccc_help", names)
        self.assertIn("cccc_ext_deadbeef_echo", names)

    def test_handle_tool_call_falls_back_to_dynamic_capability_tool_call(self) -> None:
        from cccc.ports.mcp.server import handle_tool_call

        with patch.dict(os.environ, {"CCCC_GROUP_ID": "g1", "CCCC_ACTOR_ID": "peer-1"}, clear=False), patch(
            "cccc.ports.mcp.server._call_daemon_or_raise",
            return_value={"tool_name": "cccc_ext_deadbeef_echo", "result": {"ok": True}},
        ) as daemon_call:
            result = handle_tool_call("cccc_ext_deadbeef_echo", {"message": "hello"})

        self.assertEqual(str(result.get("tool_name") or ""), "cccc_ext_deadbeef_echo")
        daemon_call.assert_called_once()

    def test_handle_tool_call_keeps_unknown_when_dynamic_not_found(self) -> None:
        from cccc.ports.mcp.server import handle_tool_call

        with patch.dict(os.environ, {"CCCC_GROUP_ID": "g1", "CCCC_ACTOR_ID": "peer-1"}, clear=False), patch(
            "cccc.ports.mcp.server._call_daemon_or_raise",
            side_effect=MCPError("capability_tool_not_found", "not found"),
        ):
            with self.assertRaises(MCPError):
                handle_tool_call("cccc_ext_deadbeef_missing", {})


if __name__ == "__main__":
    unittest.main()

