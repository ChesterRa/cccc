from __future__ import annotations

import os
import tempfile
import unittest
from unittest.mock import patch


class TestMcpCapabilityUse(unittest.TestCase):
    def _with_home(self):
        old_home = os.environ.get("CCCC_HOME")
        td_ctx = tempfile.TemporaryDirectory()
        td = td_ctx.__enter__()
        os.environ["CCCC_HOME"] = td

        def cleanup() -> None:
            td_ctx.__exit__(None, None, None)
            if old_home is None:
                os.environ.pop("CCCC_HOME", None)
            else:
                os.environ["CCCC_HOME"] = old_home

        return td, cleanup

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
                tool_name="cccc_agent_state",
                tool_arguments={"action": "update", "actor_id": "peer-1", "focus": "work"},
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
            return_value={"state": "runnable", "refresh_required": True, "enabled": True},
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

    def test_capability_use_does_not_inject_actor_id_into_space_query(self) -> None:
        from cccc.ports.mcp.server import capability_use

        with patch(
            "cccc.ports.mcp.handlers.cccc_capability.capability_enable",
            return_value={"state": "runnable", "refresh_required": False, "enabled": True},
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
                tool_arguments={"action": "query", "lane": "work", "query": "status?"},
            )

        self.assertTrue(bool(result.get("enabled")))
        self.assertTrue(bool(result.get("tool_called")))
        self.assertEqual(str(result.get("capability_id") or ""), "pack:space")
        enable_mock.assert_called_once()
        call_mock.assert_called_once()
        tool_args = call_mock.call_args.args[1] if len(call_mock.call_args.args) > 1 else {}
        self.assertEqual(str(tool_args.get("group_id") or ""), "g1")
        self.assertEqual(str(tool_args.get("by") or ""), "peer-1")
        self.assertNotIn("actor_id", tool_args)

    def test_capability_use_returns_not_ready_when_enable_not_ready(self) -> None:
        from cccc.ports.mcp.server import capability_use

        with patch(
            "cccc.ports.mcp.handlers.cccc_capability.capability_enable",
            return_value={"state": "blocked", "refresh_required": False, "enabled": False, "reason": "blocked_by_group_policy"},
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
                    "state": "blocked",
                    "enabled": False,
                    "reason": "install_failed:probe_timeout",
                    "diagnostics": [
                        {"code": "probe_timeout", "message": "timed out", "retryable": True}
                    ],
                },
                {
                    "state": "runnable",
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
                "state": "blocked",
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
                "state": "runnable",
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
                scope="actor",
                tool_name="",
                tool_arguments={},
            )

        self.assertTrue(bool(result.get("enabled")))
        self.assertFalse(bool(result.get("tool_called")))
        self.assertEqual(str(result.get("scope") or ""), "actor")
        self.assertEqual(str(result.get("requested_scope") or ""), "actor")
        skill_payload = result.get("skill") if isinstance(result.get("skill"), dict) else {}
        self.assertEqual(str(skill_payload.get("capability_id") or ""), "skill:anthropic:triage")
        self.assertEqual(str(result.get("skill_mode") or ""), "capsule_runtime")
        self.assertFalse(bool(result.get("full_local_skill_equivalent")))
        self.assertFalse(bool(result.get("dynamic_tools_expected")))
        runtime_visible_in = result.get("runtime_visible_in") if isinstance(result.get("runtime_visible_in"), list) else []
        self.assertIn("active_capsule_skills", runtime_visible_in)
        self.assertIn("active_capsule_skills", str(result.get("runtime_activation_evidence") or ""))
        self.assertIn("dynamic_tools", str(result.get("next_step_hint") or ""))
        self.assertIn("CCCC capability runtime", str(result.get("next_step_hint") or ""))
        self.assertNotIn("Codex's skills directory", str(result.get("next_step_hint") or ""))
        self.assertNotIn("CODEX_HOME", str(result.get("next_step_hint") or ""))
        enable_mock.assert_called_once()
        self.assertEqual(str(enable_mock.call_args.kwargs.get("scope") or ""), "actor")
        call_mock.assert_not_called()

    def test_capability_use_skill_failure_contains_runtime_contract_fields(self) -> None:
        from cccc.ports.mcp.server import capability_use

        with patch(
            "cccc.ports.mcp.handlers.cccc_capability.capability_enable",
            return_value={
                "state": "blocked",
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
        self.assertEqual(str(result.get("scope") or ""), "session")
        self.assertEqual(str(result.get("requested_scope") or ""), "session")
        self.assertEqual(str(result.get("skill_mode") or ""), "capsule_runtime")
        self.assertFalse(bool(result.get("full_local_skill_equivalent")))
        self.assertFalse(bool(result.get("dynamic_tools_expected")))
        runtime_visible_in = result.get("runtime_visible_in") if isinstance(result.get("runtime_visible_in"), list) else []
        self.assertIn("active_capsule_skills", runtime_visible_in)
        self.assertIn("active_capsule_skills", str(result.get("runtime_activation_evidence") or ""))
        self.assertIn("CCCC capability runtime", str(result.get("next_step_hint") or ""))
        self.assertNotIn("Codex's skills directory", str(result.get("next_step_hint") or ""))
        self.assertNotIn("CODEX_HOME", str(result.get("next_step_hint") or ""))

    def test_capability_use_legacy_self_proposed_skill_points_to_migration_path(self) -> None:
        from cccc.ports.mcp.server import capability_use

        with patch(
            "cccc.ports.mcp.handlers.cccc_capability.capability_enable",
            return_value={
                "state": "blocked",
                "enabled": False,
                "reason": "legacy_agent_self_proposed_namespace",
                "diagnostics": [
                    {
                        "code": "legacy_agent_self_proposed_namespace",
                        "message": "legacy self-proposed skill id",
                        "retryable": False,
                        "action_hints": [
                            "reimport_the_capsule_under_skill_agent_self_proposed_stable_slug",
                            "call_cccc_capability_uninstall_on_the_legacy_capability_id_after_migration",
                        ],
                    }
                ],
            },
        ):
            result = capability_use(
                group_id="g1",
                by="peer-1",
                actor_id="peer-1",
                capability_id="skill:agent:legacy-self-proposed",
                tool_name="",
                tool_arguments={},
            )

        self.assertFalse(bool(result.get("enabled")))
        diagnostics = result.get("diagnostics") if isinstance(result.get("diagnostics"), list) else []
        self.assertTrue(diagnostics)
        plan = result.get("resolution_plan") if isinstance(result.get("resolution_plan"), dict) else {}
        self.assertEqual(str(plan.get("status") or ""), "needs_agent_action")
        actions = plan.get("agent_actions") if isinstance(plan.get("agent_actions"), list) else []
        self.assertIn("reimport_the_capsule_under_skill_agent_self_proposed_stable_slug", actions)
        self.assertIn("call_cccc_capability_uninstall_on_the_legacy_capability_id_after_migration", actions)

    def test_capability_use_builtin_runtime_bootstrap_inproc_enables_skill_and_dependencies(self) -> None:
        from cccc.contracts.v1 import DaemonRequest
        from cccc.daemon.server import handle_request
        from cccc.ports.mcp import common as mcp_common
        from cccc.ports.mcp import server as mcp_server

        def _fake_call_daemon(req, timeout_s=None, **kwargs):
            _ = timeout_s
            _ = kwargs
            resp, _meta = handle_request(DaemonRequest.model_validate(req))
            if bool(resp.ok):
                return {"ok": True, "result": resp.result}
            err = resp.error
            return {
                "ok": False,
                "error": {
                    "code": str(getattr(err, "code", "") or "daemon_error"),
                    "message": str(getattr(err, "message", "") or "daemon error"),
                    "details": dict(getattr(err, "details", {}) or {}),
                },
            }

        _, cleanup = self._with_home()
        try:
            create_req = DaemonRequest.model_validate({"op": "group_create", "args": {"title": "rb", "topic": "", "by": "user"}})
            create_resp, _ = handle_request(create_req)
            self.assertTrue(create_resp.ok, getattr(create_resp, "error", None))
            group_id = str((create_resp.result or {}).get("group_id") or "").strip()
            self.assertTrue(group_id)

            add_req = DaemonRequest.model_validate(
                {
                    "op": "actor_add",
                    "args": {
                        "group_id": group_id,
                        "actor_id": "peer-1",
                        "runtime": "codex",
                        "runner": "headless",
                        "by": "user",
                    },
                }
            )
            add_resp, _ = handle_request(add_req)
            self.assertTrue(add_resp.ok, getattr(add_resp, "error", None))

            with patch.object(mcp_common, "call_daemon", side_effect=_fake_call_daemon), patch.dict(
                os.environ,
                {"CCCC_GROUP_ID": group_id, "CCCC_ACTOR_ID": "peer-1"},
                clear=False,
            ):
                search_result = mcp_server.handle_tool_call(
                    "cccc_capability_search",
                    {"query": "mcp injection", "kind": "skill", "include_external": False, "limit": 10},
                )
                items = search_result.get("items") if isinstance(search_result.get("items"), list) else []
                ids = {str(item.get("capability_id") or "") for item in items if isinstance(item, dict)}
                self.assertIn("skill:cccc:runtime-bootstrap", ids)

                use_result = mcp_server.handle_tool_call(
                    "cccc_capability_use",
                    {"capability_id": "skill:cccc:runtime-bootstrap", "scope": "session"},
                )
                self.assertTrue(bool(use_result.get("enabled")))
                self.assertFalse(bool(use_result.get("tool_called")))
                self.assertEqual(str(use_result.get("state") or ""), "activation_pending")
                self.assertEqual(str(use_result.get("skill_mode") or ""), "capsule_runtime")
                skill_payload = use_result.get("skill") if isinstance(use_result.get("skill"), dict) else {}
                self.assertEqual(str(skill_payload.get("capability_id") or ""), "skill:cccc:runtime-bootstrap")
                applied = skill_payload.get("applied_dependencies") if isinstance(skill_payload.get("applied_dependencies"), list) else []
                self.assertEqual(applied, ["pack:diagnostics", "pack:group-runtime"])

                state_result = mcp_server.handle_tool_call("cccc_capability_state", {})
                enabled = set(state_result.get("enabled_capabilities") or [])
                self.assertIn("skill:cccc:runtime-bootstrap", enabled)
                self.assertIn("pack:diagnostics", enabled)
                self.assertIn("pack:group-runtime", enabled)
                active_skills = state_result.get("active_capsule_skills") if isinstance(state_result.get("active_capsule_skills"), list) else []
                active_ids = {str(item.get("capability_id") or "") for item in active_skills if isinstance(item, dict)}
                self.assertIn("skill:cccc:runtime-bootstrap", active_ids)
                visible = set(state_result.get("visible_tools") or [])
                self.assertIn("cccc_terminal", visible)
                self.assertIn("cccc_actor", visible)
        finally:
            cleanup()


    def test_memory_read_tools_skip_actor_id_injection(self) -> None:
        """capability_use should NOT inject actor_id for read-only memory tools.

        Bug: capability_use auto-injects actor_id=caller into all tool_args,
        causing memory_search to filter by caller's actor_id unintentionally.
        Read-only memory tools should search across all actors by default.
        """
        from cccc.ports.mcp.server import capability_use

        read_only_tools = [
            ("cccc_memory", {"action": "search", "query": "test"}),
            ("cccc_memory", {"action": "layout_get"}),
            ("cccc_memory", {"action": "get", "path": "/tmp/memory.md"}),
            ("cccc_memory_admin", {"action": "index_sync"}),
            ("cccc_memory_admin", {"action": "context_check", "messages": [{"role": "user", "content": "x"}]}),
        ]
        for tool_name, tool_arguments in read_only_tools:
            with self.subTest(tool=tool_name):
                with patch(
                    "cccc.ports.mcp.handlers.cccc_capability.capability_enable",
                    return_value={"state": "runnable", "refresh_required": True, "enabled": True},
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
            ("cccc_memory", {"action": "write", "target": "memory", "content": "test"}),
            ("cccc_memory", {"action": "write", "target": "daily", "date": "2026-03-03", "content": "test"}),
        ]
        for tool_name, tool_arguments in write_tools:
            with self.subTest(tool=tool_name):
                with patch(
                    "cccc.ports.mcp.handlers.cccc_capability.capability_enable",
                    return_value={"state": "runnable", "refresh_required": True, "enabled": True},
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
            return_value={"state": "runnable", "refresh_required": True, "enabled": True},
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
            return_value={"state": "runnable", "refresh_required": True, "enabled": True},
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
            return_value={"state": "runnable", "refresh_required": True, "enabled": True},
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
            return_value={"state": "runnable", "refresh_required": True, "enabled": True},
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
        self.assertEqual(str(result.get("scope") or ""), "session")
        self.assertEqual(str(result.get("requested_scope") or ""), "session")
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
