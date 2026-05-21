from __future__ import annotations

import os
import tempfile
import unittest
from unittest.mock import patch

from cccc.ports.mcp.common import MCPError


class TestMcpDynamicCapabilityTools(unittest.TestCase):
    def test_capability_use_accepts_voice_secretary_builtin_tool_without_capability_id(self) -> None:
        from cccc.ports.mcp.handlers.cccc_capability import capability_use

        with patch.dict(os.environ, {"CCCC_GROUP_ID": "g1", "CCCC_ACTOR_ID": "voice-secretary"}, clear=False), patch(
            "cccc.ports.mcp.server.handle_tool_call",
            return_value={"ok": True, "item_count": 1},
        ) as handle_tool_call:
            result = capability_use(
                group_id="g1",
                by="voice-secretary",
                actor_id="voice-secretary",
                capability_id="",
                tool_name="cccc_voice_secretary_document",
                tool_arguments={"action": "read_new_input"},
                scope="session",
                ttl_seconds=3600,
                reason="Mandatory voice secretary input consumption",
            )

        self.assertEqual(result.get("capability_id"), "core")
        self.assertTrue(bool(result.get("tool_called")))
        handle_tool_call.assert_called_once_with(
            "cccc_voice_secretary_document",
            {"action": "read_new_input", "group_id": "g1", "by": "voice-secretary", "actor_id": "voice-secretary"},
        )

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

    def test_voice_secretary_document_rejects_mcp_save(self) -> None:
        from cccc.ports.mcp.server import handle_tool_call

        with patch.dict(os.environ, {"CCCC_GROUP_ID": "g1", "CCCC_ACTOR_ID": "voice-secretary"}, clear=False), patch(
            "cccc.ports.mcp.server._call_daemon_or_raise",
        ) as daemon_call:
            with self.assertRaises(MCPError) as caught:
                handle_tool_call(
                    "cccc_voice_secretary_document",
                    {
                        "action": "save",
                        "document_path": "docs/voice-secretary/notes.md",
                        "new_source": "# Updated\n",
                    },
                )

        self.assertEqual(caught.exception.code, "invalid_request")
        self.assertIn("list|create|read_new_input|archive", caught.exception.message)
        daemon_call.assert_not_called()

    def test_voice_secretary_document_read_new_input_routes_to_daemon(self) -> None:
        from cccc.ports.mcp.server import handle_tool_call

        with patch.dict(os.environ, {"CCCC_GROUP_ID": "g1", "CCCC_ACTOR_ID": "voice-secretary"}, clear=False), patch(
            "cccc.ports.mcp.server._call_daemon_or_raise",
            return_value={"ok": True},
        ) as daemon_call:
            handle_tool_call(
                "cccc_voice_secretary_document",
                {
                    "action": "read_new_input",
                },
            )

        req = daemon_call.call_args.args[0]
        args = req.get("args") if isinstance(req.get("args"), dict) else {}
        self.assertEqual(req.get("op"), "assistant_voice_document_input_read")
        self.assertEqual(args.get("by"), "assistant:voice_secretary")

    def test_voice_secretary_composer_submit_prompt_draft_routes_to_daemon(self) -> None:
        from cccc.ports.mcp.server import handle_tool_call

        with patch.dict(os.environ, {"CCCC_GROUP_ID": "g1", "CCCC_ACTOR_ID": "voice-secretary"}, clear=False), patch(
            "cccc.ports.mcp.server._call_daemon_or_raise",
            return_value={"ok": True},
        ) as daemon_call:
            handle_tool_call(
                "cccc_voice_secretary_composer",
                {
                    "action": "submit_prompt_draft",
                    "request_id": "voice-prompt-1",
                    "draft_text": "Please review the plan and list concrete risks.",
                },
            )

        req = daemon_call.call_args.args[0]
        args = req.get("args") if isinstance(req.get("args"), dict) else {}
        self.assertEqual(req.get("op"), "assistant_voice_prompt_draft_submit")
        self.assertEqual(args.get("by"), "voice-secretary")
        self.assertEqual(args.get("request_id"), "voice-prompt-1")
        self.assertEqual(args.get("operation"), "")
        self.assertFalse(args.get("no_op"))

    def test_voice_secretary_composer_no_op_routes_to_daemon(self) -> None:
        from cccc.ports.mcp.server import handle_tool_call

        with patch.dict(os.environ, {"CCCC_GROUP_ID": "g1", "CCCC_ACTOR_ID": "voice-secretary"}, clear=False), patch(
            "cccc.ports.mcp.server._call_daemon_or_raise",
            return_value={"ok": True},
        ) as daemon_call:
            handle_tool_call(
                "cccc_voice_secretary_composer",
                {
                    "action": "submit_prompt_draft",
                    "request_id": "voice-prompt-1",
                    "no_op": True,
                },
            )

        req = daemon_call.call_args.args[0]
        args = req.get("args") if isinstance(req.get("args"), dict) else {}
        self.assertEqual(req.get("op"), "assistant_voice_prompt_draft_submit")
        self.assertEqual(args.get("request_id"), "voice-prompt-1")
        self.assertTrue(args.get("no_op"))
        self.assertEqual(args.get("draft_text"), "")

    def test_voice_secretary_document_list_defaults_to_compact_content(self) -> None:
        from cccc.ports.mcp.server import handle_tool_call

        with patch.dict(os.environ, {"CCCC_GROUP_ID": "g1", "CCCC_ACTOR_ID": "voice-secretary"}, clear=False), patch(
            "cccc.ports.mcp.server._call_daemon_or_raise",
            return_value={"ok": True},
        ) as daemon_call:
            handle_tool_call(
                "cccc_voice_secretary_document",
                {
                    "action": "list",
                    "document_path": "docs/voice-secretary/notes.md",
                },
            )

        req = daemon_call.call_args.args[0]
        args = req.get("args") if isinstance(req.get("args"), dict) else {}
        self.assertEqual(req.get("op"), "assistant_voice_document_list")
        self.assertEqual(args.get("document_path"), "docs/voice-secretary/notes.md")
        self.assertFalse(bool(args.get("include_content")))
        self.assertFalse(bool(args.get("include_documents_by_id")))
        self.assertFalse(bool(args.get("include_documents_by_path")))

    def test_voice_secretary_document_list_rejects_content_payload(self) -> None:
        from cccc.ports.mcp.server import handle_tool_call

        with patch.dict(os.environ, {"CCCC_GROUP_ID": "g1", "CCCC_ACTOR_ID": "voice-secretary"}, clear=False), patch(
            "cccc.ports.mcp.server._call_daemon_or_raise",
        ) as daemon_call:
            with self.assertRaises(MCPError) as caught:
                handle_tool_call(
                    "cccc_voice_secretary_document",
                    {
                        "action": "list",
                        "include_content": True,
                    },
                )

        self.assertEqual(caught.exception.code, "invalid_request")
        self.assertIn("read repository markdown directly", caught.exception.message)
        daemon_call.assert_not_called()

    def test_voice_secretary_document_schema_has_no_save_action(self) -> None:
        from cccc.ports.mcp.toolspecs import MCP_TOOLS

        tool = next(item for item in MCP_TOOLS if item.get("name") == "cccc_voice_secretary_document")
        properties = ((tool.get("inputSchema") or {}).get("properties") or {})
        actions = set(properties.get("action", {}).get("enum") or [])
        self.assertEqual(actions, {"list", "create", "read_new_input", "archive"})
        self.assertNotIn("content", properties)
        self.assertNotIn("include_content", properties)
        self.assertNotIn("status", properties)

    def test_voice_secretary_document_create_rejects_content_payload(self) -> None:
        from cccc.ports.mcp.server import handle_tool_call

        with patch.dict(os.environ, {"CCCC_GROUP_ID": "g1", "CCCC_ACTOR_ID": "voice-secretary"}, clear=False), patch(
            "cccc.ports.mcp.server._call_daemon_or_raise",
        ) as daemon_call:
            with self.assertRaises(MCPError) as caught:
                handle_tool_call(
                    "cccc_voice_secretary_document",
                    {
                        "action": "create",
                        "title": "Notes",
                        "content": "# Should not go through MCP\n",
                    },
                )

        self.assertEqual(caught.exception.code, "invalid_request")
        self.assertIn("edit document content directly", caught.exception.message)
        daemon_call.assert_not_called()

    def test_voice_secretary_request_routes_to_daemon(self) -> None:
        from cccc.ports.mcp.server import handle_tool_call

        with patch.dict(os.environ, {"CCCC_GROUP_ID": "g1", "CCCC_ACTOR_ID": "voice-secretary"}, clear=False), patch(
            "cccc.ports.mcp.server._call_daemon_or_raise",
            return_value={"ok": True},
        ) as daemon_call:
            handle_tool_call(
                "cccc_voice_secretary_request",
                {
                    "target": "@foreman",
                    "request_text": "Please review this action request.",
                    "summary": "Spoken task detected.",
                    "document_path": "docs/voice-secretary/notes.md",
                    "requires_ack": True,
                },
            )

        req = daemon_call.call_args.args[0]
        args = req.get("args") if isinstance(req.get("args"), dict) else {}
        self.assertEqual(req.get("op"), "assistant_voice_request")
        self.assertEqual(args.get("target"), "@foreman")
        self.assertEqual(args.get("request_text"), "Please review this action request.")
        self.assertEqual(args.get("document_path"), "docs/voice-secretary/notes.md")
        self.assertEqual(args.get("by"), "voice-secretary")

    def test_voice_secretary_request_report_routes_to_feedback_op(self) -> None:
        from cccc.ports.mcp.server import handle_tool_call

        with patch.dict(os.environ, {"CCCC_GROUP_ID": "g1", "CCCC_ACTOR_ID": "voice-secretary"}, clear=False), patch(
            "cccc.ports.mcp.server._call_daemon_or_raise",
            return_value={"ok": True},
        ) as daemon_call:
            handle_tool_call(
                "cccc_voice_secretary_request",
                {
                    "action": "report",
                    "request_id": "voice-ask-123",
                    "status": "done",
                    "reply_text": "Handled directly.",
                    "document_path": "docs/voice-secretary/notes.md",
                    "artifact_paths": ["docs/voice-secretary/notes.md", "docs/voice-secretary/report.md"],
                    "source_summary": "Checked current weather provider snapshot.",
                    "checked_at": "2026-04-22T12:00:00Z",
                    "source_urls": ["https://example.com/weather"],
                },
            )

        req = daemon_call.call_args.args[0]
        args = req.get("args") if isinstance(req.get("args"), dict) else {}
        self.assertEqual(req.get("op"), "assistant_voice_instruction_feedback")
        self.assertEqual(args.get("request_id"), "voice-ask-123")
        self.assertEqual(args.get("status"), "done")
        self.assertEqual(args.get("reply_text"), "Handled directly.")
        self.assertEqual(args.get("artifact_paths"), ["docs/voice-secretary/notes.md", "docs/voice-secretary/report.md"])
        self.assertEqual(args.get("source_summary"), "Checked current weather provider snapshot.")
        self.assertEqual(args.get("checked_at"), "2026-04-22T12:00:00Z")
        self.assertEqual(args.get("source_urls"), ["https://example.com/weather"])
        self.assertNotIn("result_summary", args)
        self.assertEqual(args.get("by"), "voice-secretary")

    def test_voice_secretary_request_rejects_non_voice_actor(self) -> None:
        from cccc.ports.mcp.server import handle_tool_call

        with patch.dict(os.environ, {"CCCC_GROUP_ID": "g1", "CCCC_ACTOR_ID": "peer-1"}, clear=False):
            with self.assertRaises(MCPError) as caught:
                handle_tool_call(
                    "cccc_voice_secretary_request",
                    {"request_text": "Please review this action request."},
                )

        self.assertEqual(caught.exception.code, "permission_denied")

    def test_voice_secretary_request_requires_explicit_target(self) -> None:
        from cccc.ports.mcp.server import handle_tool_call

        with patch.dict(os.environ, {"CCCC_GROUP_ID": "g1", "CCCC_ACTOR_ID": "voice-secretary"}, clear=False), patch(
            "cccc.ports.mcp.server._call_daemon_or_raise",
            return_value={"ok": True},
        ) as daemon_call:
            with self.assertRaises(MCPError) as caught:
                handle_tool_call(
                    "cccc_voice_secretary_request",
                    {"request_text": "Please review this action request."},
                )

        self.assertEqual(caught.exception.code, "invalid_request")
        daemon_call.assert_not_called()

    def test_list_tools_for_caller_fallback_uses_pet_minimal_surface(self) -> None:
        from cccc.contracts.v1 import DaemonRequest
        from cccc.daemon.server import handle_request
        from cccc.kernel.group import load_group
        from cccc.kernel.pet_actor import ensure_pet_actor
        from cccc.ports.mcp.server import list_tools_for_caller

        with tempfile.TemporaryDirectory() as td, patch.dict(os.environ, {"CCCC_HOME": td}, clear=False):
            create_resp, _ = handle_request(
                DaemonRequest.model_validate({"op": "group_create", "args": {"title": "mcp-pet", "topic": "", "by": "user"}})
            )
            self.assertTrue(create_resp.ok, getattr(create_resp, "error", None))
            group_id = str((create_resp.result or {}).get("group_id") or "").strip()
            self.assertTrue(group_id)

            add_resp, _ = handle_request(
                DaemonRequest.model_validate(
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
            )
            self.assertTrue(add_resp.ok, getattr(add_resp, "error", None))

            group = load_group(group_id)
            self.assertIsNotNone(group)
            ensure_pet_actor(group)

            with patch.dict(os.environ, {"CCCC_GROUP_ID": group_id, "CCCC_ACTOR_ID": "pet-peer"}, clear=False), patch(
                "cccc.ports.mcp.server._call_daemon_or_raise",
                side_effect=RuntimeError("daemon unavailable"),
            ):
                tools = list_tools_for_caller()

        names = {str(item.get("name") or "") for item in tools if isinstance(item, dict)}
        self.assertIn("cccc_help", names)
        self.assertIn("cccc_context_get", names)
        self.assertIn("cccc_agent_state", names)
        self.assertIn("cccc_pet_decisions", names)
        self.assertNotIn("cccc_message_send", names)
        self.assertNotIn("cccc_message_reply", names)
        self.assertNotIn("cccc_file", names)

    def test_list_tools_for_caller_fallback_uses_voice_secretary_surface(self) -> None:
        from cccc.contracts.v1 import DaemonRequest
        from cccc.daemon.server import handle_request
        from cccc.kernel.group import load_group
        from cccc.kernel.voice_secretary_actor import ensure_voice_secretary_actor
        from cccc.ports.mcp.server import list_tools_for_caller

        with tempfile.TemporaryDirectory() as td, patch.dict(os.environ, {"CCCC_HOME": td}, clear=False):
            create_resp, _ = handle_request(
                DaemonRequest.model_validate({"op": "group_create", "args": {"title": "mcp-voice", "topic": "", "by": "user"}})
            )
            self.assertTrue(create_resp.ok, getattr(create_resp, "error", None))
            group_id = str((create_resp.result or {}).get("group_id") or "").strip()
            self.assertTrue(group_id)

            add_resp, _ = handle_request(
                DaemonRequest.model_validate(
                    {
                        "op": "actor_add",
                        "args": {
                            "group_id": group_id,
                            "actor_id": "lead",
                            "runtime": "codex",
                            "runner": "headless",
                            "by": "user",
                        },
                    }
                )
            )
            self.assertTrue(add_resp.ok, getattr(add_resp, "error", None))

            group = load_group(group_id)
            self.assertIsNotNone(group)
            assert group is not None
            ensure_voice_secretary_actor(group)

            with patch.dict(os.environ, {"CCCC_GROUP_ID": group_id, "CCCC_ACTOR_ID": "voice-secretary"}, clear=False), patch(
                "cccc.ports.mcp.server._call_daemon_or_raise",
                side_effect=RuntimeError("daemon unavailable"),
            ):
                tools = list_tools_for_caller()

        names = {str(item.get("name") or "") for item in tools if isinstance(item, dict)}
        self.assertIn("cccc_help", names)
        self.assertIn("cccc_voice_secretary_document", names)
        self.assertIn("cccc_voice_secretary_composer", names)
        self.assertIn("cccc_voice_secretary_request", names)
        self.assertNotIn("cccc_pet_decisions", names)
        self.assertNotIn("cccc_message_send", names)
        self.assertNotIn("cccc_message_reply", names)

    def test_list_tools_for_caller_fallback_uses_voice_secretary_env_without_actor_doc(self) -> None:
        from cccc.contracts.v1 import DaemonRequest
        from cccc.daemon.server import handle_request
        from cccc.ports.mcp.server import list_tools_for_caller

        with tempfile.TemporaryDirectory() as td, patch.dict(os.environ, {"CCCC_HOME": td}, clear=False):
            create_resp, _ = handle_request(
                DaemonRequest.model_validate({"op": "group_create", "args": {"title": "mcp-voice-env", "topic": "", "by": "user"}})
            )
            self.assertTrue(create_resp.ok, getattr(create_resp, "error", None))
            group_id = str((create_resp.result or {}).get("group_id") or "").strip()
            self.assertTrue(group_id)

            with patch.dict(os.environ, {"CCCC_GROUP_ID": group_id, "CCCC_ACTOR_ID": "voice-secretary"}, clear=False), patch(
                "cccc.ports.mcp.server._call_daemon_or_raise",
                side_effect=RuntimeError("daemon unavailable"),
            ):
                tools = list_tools_for_caller()

        names = {str(item.get("name") or "") for item in tools if isinstance(item, dict)}
        self.assertIn("cccc_help", names)
        self.assertIn("cccc_voice_secretary_document", names)
        self.assertIn("cccc_voice_secretary_composer", names)
        self.assertIn("cccc_voice_secretary_request", names)
        self.assertNotIn("cccc_message_send", names)
        self.assertNotIn("cccc_message_reply", names)


if __name__ == "__main__":
    unittest.main()
