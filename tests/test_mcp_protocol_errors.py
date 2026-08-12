from __future__ import annotations

from unittest.mock import patch


def test_protocol_and_tool_execution_errors_use_distinct_envelopes() -> None:
    from cccc.ports.mcp.main import handle_request
    from cccc.ports.mcp.server import MCPError

    unknown_method = handle_request(
        {"jsonrpc": "2.0", "id": 1, "method": "unknown/method", "params": {}}
    )
    assert unknown_method["error"]["code"] == -32601

    invalid_request = handle_request([])
    assert invalid_request["error"]["code"] == -32600

    notification = handle_request(
        {"jsonrpc": "2.0", "method": "notifications/initialized", "params": {}}
    )
    assert notification == {}

    malformed = handle_request(
        {"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": []}
    )
    assert malformed["error"]["code"] == -32602

    unknown_tool = handle_request(
        {
            "jsonrpc": "2.0",
            "id": 3,
            "method": "tools/call",
            "params": {"name": "not_a_tool", "arguments": {}},
        }
    )
    assert unknown_tool["error"]["code"] == -32602
    assert "Unknown tool" in unknown_tool["error"]["message"]

    with patch("cccc.ports.mcp.main.handle_tool_call", return_value={"ok": True}):
        omitted_arguments = handle_request(
            {
                "jsonrpc": "2.0",
                "id": 5,
                "method": "tools/call",
                "params": {"name": "cccc_help"},
            }
        )
    assert omitted_arguments["result"]["content"]
    assert omitted_arguments["result"]["structuredContent"] == {"ok": True}

    with patch(
        "cccc.ports.mcp.main.handle_tool_call",
        side_effect=MCPError(code="missing_group_id", message="group_id is required"),
    ):
        execution_error = handle_request(
            {
                "jsonrpc": "2.0",
                "id": 6,
                "method": "tools/call",
                "params": {"name": "cccc_repo", "arguments": {"action": "info"}},
            }
        )
    assert execution_error["result"]["isError"] is True
    assert execution_error["result"]["structuredContent"]["error"]["code"] == "missing_group_id"
    assert "error" not in execution_error
