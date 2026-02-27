from __future__ import annotations

import unittest
from unittest.mock import patch


class TestMcpCapabilityUse(unittest.TestCase):
    def test_capability_use_infers_pack_and_calls_tool(self) -> None:
        from cccc.ports.mcp.server import capability_use

        with patch(
            "cccc.ports.mcp.handlers.cccc_capability.capability_enable",
            return_value={"state": "ready", "refresh_required": True, "enabled": True},
        ) as enable_mock, patch(
            "cccc.ports.mcp.server.handle_tool_call",
            return_value={"ok": True},
        ) as call_mock:
            result = capability_use(
                group_id="g1",
                by="peer-1",
                actor_id="peer-1",
                capability_id="",
                tool_name="cccc_space_status",
                tool_arguments={},
            )

        self.assertTrue(bool(result.get("enabled")))
        self.assertTrue(bool(result.get("tool_called")))
        self.assertEqual(str(result.get("capability_id") or ""), "pack:space")
        enable_mock.assert_called_once()
        call_mock.assert_called_once()
        tool_args = call_mock.call_args.args[1] if len(call_mock.call_args.args) > 1 else {}
        self.assertEqual(str(tool_args.get("group_id") or ""), "g1")
        self.assertEqual(str(tool_args.get("by") or ""), "peer-1")
        self.assertEqual(str(tool_args.get("actor_id") or ""), "peer-1")

    def test_capability_use_returns_pending_when_enable_not_ready(self) -> None:
        from cccc.ports.mcp.server import capability_use

        with patch(
            "cccc.ports.mcp.handlers.cccc_capability.capability_enable",
            return_value={"state": "pending_approval", "refresh_required": False, "enabled": False},
        ) as enable_mock, patch(
            "cccc.ports.mcp.server.handle_tool_call",
        ) as call_mock:
            result = capability_use(
                group_id="g1",
                by="peer-1",
                actor_id="peer-1",
                capability_id="mcp:test",
                tool_name="cccc_ext_deadbeef_echo",
                tool_arguments={"message": "hello"},
            )

        self.assertFalse(bool(result.get("enabled")))
        self.assertFalse(bool(result.get("tool_called")))
        enable_mock.assert_called_once()
        call_mock.assert_not_called()

    def test_capability_use_passes_skill_payload_when_no_tool_call(self) -> None:
        from cccc.ports.mcp.server import capability_use

        with patch(
            "cccc.ports.mcp.handlers.cccc_capability.capability_enable",
            return_value={
                "state": "ready",
                "refresh_required": False,
                "enabled": True,
                "skill": {
                    "capability_id": "skill:anthropic:triage",
                    "capsule": "Use triage checklist",
                },
            },
        ) as enable_mock, patch(
            "cccc.ports.mcp.server.handle_tool_call",
        ) as call_mock:
            result = capability_use(
                group_id="g1",
                by="peer-1",
                actor_id="peer-1",
                capability_id="skill:anthropic:triage",
                tool_name="",
                tool_arguments={},
            )

        self.assertTrue(bool(result.get("enabled")))
        self.assertFalse(bool(result.get("tool_called")))
        skill_payload = result.get("skill") if isinstance(result.get("skill"), dict) else {}
        self.assertEqual(str(skill_payload.get("capability_id") or ""), "skill:anthropic:triage")
        enable_mock.assert_called_once()
        call_mock.assert_not_called()


    def test_memory_read_tools_skip_actor_id_injection(self) -> None:
        """capability_use should NOT inject actor_id for read-only memory tools.

        Bug: capability_use auto-injects actor_id=caller into all tool_args,
        causing memory_search to filter by caller's actor_id unintentionally.
        Read-only memory tools should search across all actors by default.
        """
        from cccc.ports.mcp.server import capability_use

        read_only_tools = [
            "cccc_memory_search",
            "cccc_memory_stats",
            "cccc_memory_decay",
            "cccc_memory_export",
        ]
        for tool_name in read_only_tools:
            with self.subTest(tool=tool_name):
                with patch(
                    "cccc.ports.mcp.handlers.cccc_capability.capability_enable",
                    return_value={"state": "ready", "refresh_required": True, "enabled": True},
                ), patch(
                    "cccc.ports.mcp.server.handle_tool_call",
                    return_value={"ok": True},
                ) as call_mock:
                    capability_use(
                        group_id="g1",
                        by="peer-1",
                        actor_id="peer-1",
                        capability_id="pack:context-advanced",
                        tool_name=tool_name,
                        tool_arguments={"query": "test"},
                    )

                call_mock.assert_called_once()
                tool_args = call_mock.call_args.args[1] if len(call_mock.call_args.args) > 1 else {}
                # actor_id should NOT be injected for read-only memory tools
                self.assertNotIn("actor_id", tool_args,
                    f"{tool_name}: actor_id should not be auto-injected for read-only memory tools")

    def test_memory_write_tools_still_inject_actor_id(self) -> None:
        """capability_use should still inject actor_id for write memory tools."""
        from cccc.ports.mcp.server import capability_use

        write_tools = [
            "cccc_memory_store",
            "cccc_memory_delete",
            "cccc_memory_ingest",
        ]
        for tool_name in write_tools:
            with self.subTest(tool=tool_name):
                with patch(
                    "cccc.ports.mcp.handlers.cccc_capability.capability_enable",
                    return_value={"state": "ready", "refresh_required": True, "enabled": True},
                ), patch(
                    "cccc.ports.mcp.server.handle_tool_call",
                    return_value={"ok": True},
                ) as call_mock:
                    capability_use(
                        group_id="g1",
                        by="peer-1",
                        actor_id="peer-1",
                        capability_id="pack:context-advanced",
                        tool_name=tool_name,
                        tool_arguments={"content": "test"},
                    )

                call_mock.assert_called_once()
                tool_args = call_mock.call_args.args[1] if len(call_mock.call_args.args) > 1 else {}
                # actor_id SHOULD be injected for write tools
                self.assertEqual(str(tool_args.get("actor_id") or ""), "peer-1",
                    f"{tool_name}: actor_id should be auto-injected for write memory tools")

    def test_memory_read_tool_with_explicit_actor_id_preserved(self) -> None:
        """If caller explicitly passes actor_id, it should be preserved even for read tools."""
        from cccc.ports.mcp.server import capability_use

        with patch(
            "cccc.ports.mcp.handlers.cccc_capability.capability_enable",
            return_value={"state": "ready", "refresh_required": True, "enabled": True},
        ), patch(
            "cccc.ports.mcp.server.handle_tool_call",
            return_value={"ok": True},
        ) as call_mock:
            capability_use(
                group_id="g1",
                by="peer-1",
                actor_id="peer-1",
                capability_id="pack:context-advanced",
                tool_name="cccc_memory_search",
                tool_arguments={"query": "test", "actor_id": "specific-actor"},
            )

        call_mock.assert_called_once()
        tool_args = call_mock.call_args.args[1] if len(call_mock.call_args.args) > 1 else {}
        # Explicit actor_id should be preserved
        self.assertEqual(str(tool_args.get("actor_id") or ""), "specific-actor")


if __name__ == "__main__":
    unittest.main()
