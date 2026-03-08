from __future__ import annotations

import json
import unittest
from unittest.mock import patch


class TestMcpToolsListChanged(unittest.TestCase):
    def setUp(self) -> None:
        from cccc.ports.mcp.main import _reset_session_state_for_tests

        _reset_session_state_for_tests()

    def test_initialize_advertises_tools_list_changed(self) -> None:
        from cccc.ports.mcp.main import handle_request

        resp = handle_request(
            {
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": {},
            }
        )
        result = resp.get("result") if isinstance(resp.get("result"), dict) else {}
        capabilities = result.get("capabilities") if isinstance(result.get("capabilities"), dict) else {}
        tools_caps = capabilities.get("tools") if isinstance(capabilities.get("tools"), dict) else {}
        self.assertTrue(bool(tools_caps.get("listChanged")))

    def test_capability_enable_enqueues_list_changed_notification_when_supported(self) -> None:
        from cccc.ports.mcp.main import _drain_pending_notifications, handle_request

        # Client negotiates support first.
        handle_request(
            {
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": {"capabilities": {"tools": {"listChanged": True}}},
            }
        )

        with patch(
            "cccc.ports.mcp.main.handle_tool_call",
            return_value={"refresh_required": True, "state": "runnable"},
        ):
            call_resp = handle_request(
                {
                    "jsonrpc": "2.0",
                    "id": 2,
                    "method": "tools/call",
                    "params": {"name": "cccc_capability_enable", "arguments": {}},
                }
            )
        self.assertEqual(call_resp.get("id"), 2)
        result = call_resp.get("result") if isinstance(call_resp.get("result"), dict) else {}
        content = result.get("content") if isinstance(result.get("content"), list) else []
        self.assertTrue(content)
        payload = json.loads(str(content[0].get("text") or "{}"))
        self.assertTrue(bool(payload.get("refresh_required")))

        notes = _drain_pending_notifications()
        self.assertEqual(len(notes), 1)
        self.assertEqual(notes[0].get("method"), "notifications/tools/list_changed")

    def test_capability_enable_does_not_enqueue_notification_without_support(self) -> None:
        from cccc.ports.mcp.main import _drain_pending_notifications, handle_request

        # No initialize => no negotiated support.
        with patch(
            "cccc.ports.mcp.main.handle_tool_call",
            return_value={"refresh_required": True, "state": "runnable"},
        ):
            handle_request(
                {
                    "jsonrpc": "2.0",
                    "id": 3,
                    "method": "tools/call",
                    "params": {"name": "cccc_capability_enable", "arguments": {}},
                }
            )
        notes = _drain_pending_notifications()
        self.assertEqual(notes, [])

    def test_capability_uninstall_enqueues_list_changed_notification_when_supported(self) -> None:
        from cccc.ports.mcp.main import _drain_pending_notifications, handle_request

        handle_request(
            {
                "jsonrpc": "2.0",
                "id": 10,
                "method": "initialize",
                "params": {"capabilities": {"tools": {"listChanged": True}}},
            }
        )

        with patch(
            "cccc.ports.mcp.main.handle_tool_call",
            return_value={"refresh_required": True, "state": "runnable"},
        ):
            handle_request(
                {
                    "jsonrpc": "2.0",
                    "id": 11,
                    "method": "tools/call",
                    "params": {"name": "cccc_capability_uninstall", "arguments": {}},
                }
            )
        notes = _drain_pending_notifications()
        self.assertEqual(len(notes), 1)
        self.assertEqual(notes[0].get("method"), "notifications/tools/list_changed")

    def test_capability_import_enqueues_list_changed_notification_when_supported(self) -> None:
        from cccc.ports.mcp.main import _drain_pending_notifications, handle_request

        handle_request(
            {
                "jsonrpc": "2.0",
                "id": 15,
                "method": "initialize",
                "params": {"capabilities": {"tools": {"listChanged": True}}},
            }
        )

        with patch(
            "cccc.ports.mcp.main.handle_tool_call",
            return_value={"refresh_required": True, "state": "runnable"},
        ):
            handle_request(
                {
                    "jsonrpc": "2.0",
                    "id": 16,
                    "method": "tools/call",
                    "params": {"name": "cccc_capability_import", "arguments": {}},
                }
            )
        notes = _drain_pending_notifications()
        self.assertEqual(len(notes), 1)
        self.assertEqual(notes[0].get("method"), "notifications/tools/list_changed")

    def test_capability_use_enqueues_list_changed_notification_when_supported(self) -> None:
        from cccc.ports.mcp.main import _drain_pending_notifications, handle_request

        handle_request(
            {
                "jsonrpc": "2.0",
                "id": 20,
                "method": "initialize",
                "params": {"capabilities": {"tools": {"listChanged": True}}},
            }
        )

        with patch(
            "cccc.ports.mcp.main.handle_tool_call",
            return_value={"enable_result": {"refresh_required": True}, "enabled": True},
        ):
            handle_request(
                {
                    "jsonrpc": "2.0",
                    "id": 21,
                    "method": "tools/call",
                    "params": {"name": "cccc_capability_use", "arguments": {}},
                }
            )
        notes = _drain_pending_notifications()
        self.assertEqual(len(notes), 1)
        self.assertEqual(notes[0].get("method"), "notifications/tools/list_changed")


if __name__ == "__main__":
    unittest.main()
