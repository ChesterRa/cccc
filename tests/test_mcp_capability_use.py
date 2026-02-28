from __future__ import annotations

import unittest
from unittest.mock import patch


class TestMcpCapabilityUse(unittest.TestCase):
    def test_capability_use_calls_core_tool_without_capability_enable(self) -> None:
        from cccc.ports.mcp.server import capability_use

        with patch(
            "cccc.ports.mcp.handlers.cccc_capability.capability_enable",
        ) as enable_mock, patch(
            "cccc.ports.mcp.server.handle_tool_call",
            return_value={"ok": True},
        ) as call_mock:
            result = capability_use(
                group_id="g1",
                by="peer-1",
                actor_id="peer-1",
                capability_id="",
                tool_name="cccc_context_agent",
                tool_arguments={"action": "update", "agent_id": "peer-1", "focus": "work"},
            )

        self.assertTrue(bool(result.get("enabled")))
        self.assertTrue(bool(result.get("tool_called")))
        self.assertEqual(str(result.get("capability_id") or ""), "core")
        enable_mock.assert_not_called()
        call_mock.assert_called_once()

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
                tool_name="cccc_space",
                tool_arguments={"action": "status"},
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

    def test_capability_use_returns_not_ready_when_enable_not_ready(self) -> None:
        from cccc.ports.mcp.server import capability_use

        with patch(
            "cccc.ports.mcp.handlers.cccc_capability.capability_enable",
            return_value={"state": "failed", "refresh_required": False, "enabled": False, "reason": "blocked_by_group_policy"},
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
            ("cccc_memory", {"action": "search", "query": "test"}),
            ("cccc_memory", {"action": "stats"}),
            ("cccc_memory_admin", {"action": "decay"}),
            ("cccc_memory_admin", {"action": "export"}),
        ]
        for tool_name, tool_arguments in read_only_tools:
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
                        tool_arguments=tool_arguments,
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
            ("cccc_memory", {"action": "store", "content": "test"}),
            ("cccc_memory_admin", {"action": "delete", "id": "m1"}),
            ("cccc_memory_admin", {"action": "ingest"}),
        ]
        for tool_name, tool_arguments in write_tools:
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
                        tool_arguments=tool_arguments,
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
                tool_name="cccc_memory",
                tool_arguments={"action": "search", "query": "test", "actor_id": "specific-actor"},
            )

        call_mock.assert_called_once()
        tool_args = call_mock.call_args.args[1] if len(call_mock.call_args.args) > 1 else {}
        # Explicit actor_id should be preserved
        self.assertEqual(str(tool_args.get("actor_id") or ""), "specific-actor")

    def test_capability_use_external_mcp_calls_daemon_capability_tool_call(self) -> None:
        from cccc.ports.mcp.server import capability_use

        with patch(
            "cccc.ports.mcp.handlers.cccc_capability.capability_enable",
            return_value={"state": "ready", "refresh_required": True, "enabled": True},
        ) as enable_mock, patch(
            "cccc.ports.mcp.handlers.cccc_capability._call_daemon_or_raise",
            return_value={"result": {"ok": True}},
        ) as daemon_call:
            result = capability_use(
                group_id="g1",
                by="peer-1",
                actor_id="peer-1",
                capability_id="mcp:test-server",
                tool_name="echo",
                tool_arguments={"message": "hello"},
            )

        self.assertTrue(bool(result.get("enabled")))
        self.assertTrue(bool(result.get("tool_called")))
        enable_mock.assert_called_once()
        daemon_call.assert_called_once()
        req = daemon_call.call_args.args[0] if daemon_call.call_args.args else {}
        req_args = req.get("args") if isinstance(req.get("args"), dict) else {}
        self.assertEqual(str(req.get("op") or ""), "capability_tool_call")
        self.assertEqual(str(req_args.get("capability_id") or ""), "mcp:test-server")
        self.assertEqual(str(req_args.get("tool_name") or ""), "echo")
        called_args = req_args.get("arguments") if isinstance(req_args.get("arguments"), dict) else {}
        self.assertEqual(str(called_args.get("group_id") or ""), "g1")
        self.assertEqual(str(called_args.get("actor_id") or ""), "peer-1")

    def test_capability_use_infers_external_capability_from_dynamic_tool_name(self) -> None:
        from cccc.ports.mcp.server import capability_use

        with patch(
            "cccc.ports.mcp.handlers.cccc_capability.capability_state",
            return_value={
                "dynamic_tools": [
                    {
                        "name": "cccc_ext_deadbeef_echo",
                        "real_tool_name": "echo",
                        "capability_id": "mcp:test-server",
                    }
                ]
            },
        ), patch(
            "cccc.ports.mcp.handlers.cccc_capability.capability_enable",
            return_value={"state": "ready", "refresh_required": True, "enabled": True},
        ), patch(
            "cccc.ports.mcp.handlers.cccc_capability._call_daemon_or_raise",
            return_value={"result": {"ok": True}},
        ):
            result = capability_use(
                group_id="g1",
                by="peer-1",
                actor_id="peer-1",
                capability_id="",
                tool_name="cccc_ext_deadbeef_echo",
                tool_arguments={"message": "hello"},
            )

        self.assertEqual(str(result.get("capability_id") or ""), "mcp:test-server")
        self.assertTrue(bool(result.get("tool_called")))


if __name__ == "__main__":
    unittest.main()
