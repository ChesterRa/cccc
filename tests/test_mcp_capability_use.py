from __future__ import annotations

import os
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

    def test_capability_use_retries_on_retryable_diagnostics(self) -> None:
        from cccc.ports.mcp.server import capability_use

        with patch(
            "cccc.ports.mcp.handlers.cccc_capability.capability_enable",
            side_effect=[
                {
                    "state": "failed",
                    "enabled": False,
                    "reason": "install_failed:probe_timeout",
                    "diagnostics": [
                        {"code": "probe_timeout", "message": "timed out", "retryable": True}
                    ],
                },
                {
                    "state": "ready",
                    "refresh_required": False,
                    "enabled": True,
                },
            ],
        ) as enable_mock, patch(
            "cccc.ports.mcp.server.handle_tool_call",
        ) as call_mock:
            result = capability_use(
                group_id="g1",
                by="peer-1",
                actor_id="peer-1",
                capability_id="mcp:test",
                tool_name="",
                tool_arguments={},
            )

        self.assertTrue(bool(result.get("enabled")))
        self.assertFalse(bool(result.get("tool_called")))
        self.assertEqual(enable_mock.call_count, 2)
        call_mock.assert_not_called()

    def test_capability_use_failed_result_contains_resolution_plan(self) -> None:
        from cccc.ports.mcp.server import capability_use

        with patch(
            "cccc.ports.mcp.handlers.cccc_capability.capability_enable",
            return_value={
                "state": "failed",
                "enabled": False,
                "reason": "install_failed:missing_required_env",
                "diagnostics": [
                    {
                        "code": "missing_required_env",
                        "message": "missing_required_env:OPENAI_API_KEY",
                        "required_env": ["OPENAI_API_KEY"],
                        "retryable": False,
                    }
                ],
            },
        ) as enable_mock, patch(
            "cccc.ports.mcp.server.handle_tool_call",
        ) as call_mock:
            result = capability_use(
                group_id="g1",
                by="peer-1",
                actor_id="peer-1",
                capability_id="mcp:test",
                tool_name="cccc_ext_test_tool",
                tool_arguments={},
            )

        self.assertFalse(bool(result.get("enabled")))
        self.assertFalse(bool(result.get("tool_called")))
        diagnostics = result.get("diagnostics") if isinstance(result.get("diagnostics"), list) else []
        self.assertTrue(diagnostics)
        plan = result.get("resolution_plan") if isinstance(result.get("resolution_plan"), dict) else {}
        self.assertEqual(str(plan.get("status") or ""), "needs_user_input")
        user_requests = plan.get("user_requests") if isinstance(plan.get("user_requests"), list) else []
        self.assertTrue(user_requests)
        first = user_requests[0] if isinstance(user_requests[0], dict) else {}
        required_env = first.get("required_env") if isinstance(first.get("required_env"), list) else []
        self.assertIn("OPENAI_API_KEY", required_env)
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
        self.assertEqual(str(result.get("skill_mode") or ""), "capsule_runtime")
        self.assertFalse(bool(result.get("full_local_skill_equivalent")))
        self.assertIn("$CODEX_HOME/skills", str(result.get("next_step_hint") or ""))
        enable_mock.assert_called_once()
        call_mock.assert_not_called()

    def test_capability_use_skill_failure_contains_runtime_contract_fields(self) -> None:
        from cccc.ports.mcp.server import capability_use

        with patch(
            "cccc.ports.mcp.handlers.cccc_capability.capability_enable",
            return_value={
                "state": "failed",
                "enabled": False,
                "reason": "capability_unavailable",
            },
        ):
            result = capability_use(
                group_id="g1",
                by="peer-1",
                actor_id="peer-1",
                capability_id="skill:anthropic:triage",
                tool_name="",
                tool_arguments={},
            )

        self.assertFalse(bool(result.get("enabled")))
        self.assertEqual(str(result.get("skill_mode") or ""), "capsule_runtime")
        self.assertFalse(bool(result.get("full_local_skill_equivalent")))
        self.assertIn("$CODEX_HOME/skills", str(result.get("next_step_hint") or ""))


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
        self.assertGreaterEqual(daemon_call.call_count, 1)
        last = daemon_call.call_args_list[-1] if daemon_call.call_args_list else None
        req = last.args[0] if (last and last.args) else {}
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

    def test_capability_use_infers_external_capability_from_hyphen_underscore_alias(self) -> None:
        from cccc.ports.mcp.server import capability_use

        with patch(
            "cccc.ports.mcp.handlers.cccc_capability.capability_state",
            return_value={
                "dynamic_tools": [
                    {
                        "name": "cccc_ext_deadbeef_resolve_library_id",
                        "real_tool_name": "resolve_library_id",
                        "capability_id": "mcp:io.github.upstash/context7",
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
                tool_name="resolve-library-id",
                tool_arguments={"libraryName": "claude code sdk"},
            )

        self.assertEqual(str(result.get("capability_id") or ""), "mcp:io.github.upstash/context7")
        self.assertTrue(bool(result.get("tool_called")))

    def test_capability_use_skips_reenable_when_capability_already_enabled_for_tool_call(self) -> None:
        from cccc.ports.mcp.server import capability_use

        with patch(
            "cccc.ports.mcp.handlers.cccc_capability.capability_state",
            return_value={"enabled_capabilities": ["mcp:test-server"]},
        ), patch(
            "cccc.ports.mcp.handlers.cccc_capability.capability_enable",
        ) as enable_mock, patch(
            "cccc.ports.mcp.handlers.cccc_capability._call_daemon_or_raise",
            return_value={"result": {"ok": True}},
        ):
            result = capability_use(
                group_id="g1",
                by="peer-1",
                actor_id="peer-1",
                capability_id="mcp:test-server",
                tool_name="echo",
                tool_arguments={"message": "hello"},
            )

        enable_mock.assert_not_called()
        self.assertTrue(bool(result.get("tool_called")))
        self.assertTrue(bool(result.get("reused_existing_binding")))
        self.assertFalse(bool(result.get("refresh_required")))

    def test_mcp_router_capability_use_accepts_actor_id_without_by(self) -> None:
        from cccc.ports.mcp import server as mcp_server

        with patch.dict(os.environ, {"CCCC_GROUP_ID": "", "CCCC_ACTOR_ID": ""}, clear=False), patch(
            "cccc.ports.mcp.server.capability_use",
            return_value={"ok": True},
        ) as use_mock:
            out = mcp_server.handle_tool_call(
                "cccc_capability_use",
                {
                    "group_id": "g_test",
                    "actor_id": "peer-1",
                    "capability_id": "mcp:test-server",
                    "tool_name": "echo",
                    "tool_arguments": {"message": "hello"},
                },
            )

        self.assertEqual(out.get("ok"), True)
        self.assertTrue(use_mock.called)
        kwargs = use_mock.call_args.kwargs if use_mock.call_args else {}
        self.assertEqual(str(kwargs.get("by") or ""), "peer-1")
        self.assertEqual(str(kwargs.get("actor_id") or ""), "peer-1")


if __name__ == "__main__":
    unittest.main()
