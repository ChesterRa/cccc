import os
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

# Env vars that _resolve_group_id / _resolve_self_actor_id read at runtime.
# Tests must isolate from the host environment to avoid group_id_mismatch.
_CLEAN_ENV = {"CCCC_GROUP_ID": "", "CCCC_ACTOR_ID": ""}


def _isolated_runtime_context():
    from cccc.ports.mcp.common import runtime_context_override

    return runtime_context_override(home="/tmp/cccc-mcp-test", group_id="", actor_id="")


class TestMcpMessageSendReplyRequired(unittest.TestCase):
    def test_message_send_coerces_reply_required_string(self) -> None:
        from cccc.ports.mcp import server as mcp_server
        from cccc.ports.mcp import common as mcp_common

        captured = {}

        def _fake_call_daemon(req):
            captured["req"] = req
            return {"ok": True, "result": {"event_id": "ev_test"}}

        with _isolated_runtime_context(), \
             patch.dict(os.environ, _CLEAN_ENV, clear=False), \
             patch.object(mcp_common, "call_daemon", side_effect=_fake_call_daemon):
            out = mcp_server.handle_tool_call(
                "cccc_message_send",
                {
                    "group_id": "g_test",
                    "actor_id": "peer1",
                    "text": "hello",
                    "to": ["user"],
                    "reply_required": "true",
                },
            )

        self.assertEqual(out.get("event_id"), "ev_test")
        req = captured.get("req") or {}
        self.assertEqual(req.get("op"), "send")
        args = req.get("args") if isinstance(req.get("args"), dict) else {}
        self.assertTrue(args.get("reply_required") is True)

    def test_message_send_passes_refs(self) -> None:
        from cccc.ports.mcp import server as mcp_server
        from cccc.ports.mcp import common as mcp_common

        captured = {}

        def _fake_call_daemon(req):
            captured["req"] = req
            return {"ok": True, "result": {"event_id": "ev_test"}}

        refs = [{"kind": "presentation_ref", "slot_id": "slot-2", "label": "P2", "locator_label": "PDF p.12"}]

        with _isolated_runtime_context(), \
             patch.dict(os.environ, _CLEAN_ENV, clear=False), \
             patch.object(mcp_common, "call_daemon", side_effect=_fake_call_daemon):
            out = mcp_server.handle_tool_call(
                "cccc_message_send",
                {
                    "group_id": "g_test",
                    "actor_id": "peer1",
                    "text": "hello",
                    "to": ["user"],
                    "refs": refs,
                },
            )

        self.assertEqual(out.get("event_id"), "ev_test")
        req = captured.get("req") or {}
        args = req.get("args") if isinstance(req.get("args"), dict) else {}
        self.assertEqual(args.get("refs"), refs)

    def test_message_send_passes_suggested_user_message(self) -> None:
        from cccc.ports.mcp import server as mcp_server
        from cccc.ports.mcp import common as mcp_common

        captured = {}

        def _fake_call_daemon(req):
            captured["req"] = req
            return {"ok": True, "result": {"event_id": "ev_test"}}

        with patch.dict(os.environ, _CLEAN_ENV, clear=False), \
             patch.object(mcp_common, "call_daemon", side_effect=_fake_call_daemon):
            out = mcp_server.handle_tool_call(
                "cccc_message_send",
                {
                    "group_id": "g_test",
                    "actor_id": "peer1",
                    "text": "done",
                    "to": ["user"],
                    "suggested_user_message": "  Please continue with the next check.  ",
                },
            )

        self.assertEqual(out.get("event_id"), "ev_test")
        req = captured.get("req") or {}
        args = req.get("args") if isinstance(req.get("args"), dict) else {}
        self.assertEqual(args.get("suggested_user_message"), "Please continue with the next check.")

    def test_message_send_uses_cross_group_op_for_explicit_destination(self) -> None:
        from cccc.ports.mcp import server as mcp_server
        from cccc.ports.mcp import common as mcp_common

        captured = {}

        def _fake_call_daemon(req):
            captured["req"] = req
            return {"ok": True, "result": {"src_event": {"id": "src-1"}, "dst_event": {"id": "dst-1"}}}

        with _isolated_runtime_context(), \
             patch.dict(os.environ, _CLEAN_ENV, clear=False), \
             patch.object(mcp_common, "call_daemon", side_effect=_fake_call_daemon):
            out = mcp_server.handle_tool_call(
                "cccc_message_send",
                {
                    "group_id": "g_runtime",
                    "dst_group_id": "g_selected",
                    "actor_id": "peer1",
                    "text": "hello",
                    "to": ["@foreman"],
                },
            )

        self.assertEqual((out.get("dst_event") or {}).get("id"), "dst-1")
        req = captured.get("req") or {}
        self.assertEqual(req.get("op"), "send_cross_group")
        args = req.get("args") if isinstance(req.get("args"), dict) else {}
        self.assertEqual(args.get("group_id"), "g_runtime")
        self.assertEqual(args.get("dst_group_id"), "g_selected")
        self.assertEqual(args.get("to"), ["@foreman"])

    def test_message_send_routes_group_bridge_remote_destination_to_remote_send(self) -> None:
        from cccc.kernel.group_bridge import pairing as pairing_kernel
        from cccc.ports.mcp import server as mcp_server
        from cccc.ports.mcp import common as mcp_common

        captured = {}

        def _fake_call_daemon(req):
            captured["req"] = req
            return {"ok": True, "result": {"queued": False, "receipt": {"status": "sent", "remote_event_id": "ev_remote"}}}

        with tempfile.TemporaryDirectory() as td, \
             _isolated_runtime_context(), \
             patch.dict(os.environ, _CLEAN_ENV, clear=False), \
             patch("cccc.kernel.group_bridge.pairing.ensure_home", return_value=Path(td)), \
             patch.object(mcp_common, "call_daemon", side_effect=_fake_call_daemon):
            request = pairing_kernel.create_pairing_request(
                pairing_kernel.create_pairing_invite(group_id="g_runtime")["pairing_code"],
                requester_group_id="g_remote",
                requester_group_title="Remote Group",
                requester_peer_id="peer_remote",
                requester_endpoint="http://remote.example:8848",
            )
            approved = pairing_kernel.approve_pairing_request(request["request_id"], approver_user_id="user-a")

            out = mcp_server.handle_tool_call(
                "cccc_message_send",
                {
                    "group_id": "g_runtime",
                    "dst_group_id": "g_remote",
                    "actor_id": "peer1",
                    "text": "hello remote",
                    "to": ["@foreman"],
                    "priority": "attention",
                    "reply_required": "true",
                    "idempotency_key": "caller-key-1",
                    "refs": [{"kind": "note", "id": "ref-1"}],
                },
            )

        self.assertEqual((out.get("receipt") or {}).get("remote_event_id"), "ev_remote")
        req = captured.get("req") or {}
        self.assertEqual(req.get("op"), "remote_send")
        args = req.get("args") if isinstance(req.get("args"), dict) else {}
        self.assertEqual(args.get("group_id"), "g_runtime")
        self.assertEqual(args.get("registration_id"), approved["registration"]["registration_id"])
        self.assertEqual(args.get("idempotency_key"), "caller-key-1")
        payload = args.get("payload") if isinstance(args.get("payload"), dict) else {}
        self.assertEqual(payload.get("text"), "hello remote")
        self.assertEqual(payload.get("to"), ["@foreman"])
        self.assertEqual(payload.get("priority"), "attention")
        self.assertEqual(payload.get("reply_required"), True)
        self.assertEqual(payload.get("refs"), [{"kind": "note", "id": "ref-1"}])

    def test_message_send_requires_explicit_recipient_for_group_bridge_remote_destination(self) -> None:
        from cccc.kernel.group_bridge import pairing as pairing_kernel
        from cccc.ports.mcp import server as mcp_server
        from cccc.ports.mcp import common as mcp_common

        with tempfile.TemporaryDirectory() as td, \
             _isolated_runtime_context(), \
             patch.dict(os.environ, _CLEAN_ENV, clear=False), \
             patch("cccc.kernel.group_bridge.pairing.ensure_home", return_value=Path(td)), \
             patch.object(mcp_common, "call_daemon") as call_daemon:
            request = pairing_kernel.create_pairing_request(
                pairing_kernel.create_pairing_invite(group_id="g_runtime")["pairing_code"],
                requester_group_id="g_remote",
                requester_group_title="Remote Group",
                requester_peer_id="peer_remote",
                requester_endpoint="http://remote.example:8848",
            )
            pairing_kernel.approve_pairing_request(request["request_id"], approver_user_id="user-a")

            with self.assertRaises(mcp_server.MCPError) as raised:
                mcp_server.handle_tool_call(
                    "cccc_message_send",
                    {
                        "group_id": "g_runtime",
                        "dst_group_id": "g_remote",
                        "actor_id": "peer1",
                        "text": "hello remote",
                    },
                )

        self.assertEqual(raised.exception.code, "missing_remote_recipient")
        call_daemon.assert_not_called()

    def test_message_send_rejects_hash_recipient_for_cross_group_string_to(self) -> None:
        from cccc.ports.mcp import server as mcp_server
        from cccc.ports.mcp import common as mcp_common

        with _isolated_runtime_context(), \
             patch.dict(os.environ, _CLEAN_ENV, clear=False), \
             patch.object(mcp_common, "call_daemon") as call_daemon:
            with self.assertRaises(mcp_server.MCPError) as raised:
                mcp_server.handle_tool_call(
                    "cccc_message_send",
                    {
                        "group_id": "g_runtime",
                        "dst_group_id": "g_selected",
                        "actor_id": "peer1",
                        "text": "hello",
                        "to": "#self-agent",
                    },
                )

        self.assertEqual(raised.exception.code, "invalid_recipient_syntax")
        self.assertIn("#group tokens are routing hints, not recipients", str(raised.exception))
        self.assertIn('cccc_group(action="resolve"', str(raised.exception))
        self.assertIn("unique real group_id", str(raised.exception))
        self.assertIn("cccc_message_send(dst_group_id=<g_...>", str(raised.exception))
        self.assertIn("to='@foreman' or target actor id", str(raised.exception))
        self.assertIn("text=<your own natural message to the target>", str(raised.exception))
        call_daemon.assert_not_called()

    def test_message_send_rejects_hash_recipient_for_cross_group_list_to(self) -> None:
        from cccc.ports.mcp import server as mcp_server
        from cccc.ports.mcp import common as mcp_common

        with _isolated_runtime_context(), \
             patch.dict(os.environ, _CLEAN_ENV, clear=False), \
             patch.object(mcp_common, "call_daemon") as call_daemon:
            with self.assertRaises(mcp_server.MCPError) as raised:
                mcp_server.handle_tool_call(
                    "cccc_message_send",
                    {
                        "group_id": "g_runtime",
                        "dst_group_id": "g_selected",
                        "actor_id": "peer1",
                        "text": "hello",
                        "to": ["#self-agent"],
                    },
                )

        self.assertEqual(raised.exception.code, "invalid_recipient_syntax")
        self.assertIn("Do not put #group in to", str(raised.exception))
        self.assertIn("do not forward a template", str(raised.exception))
        call_daemon.assert_not_called()

    def test_message_send_rejects_suggested_user_message_for_cross_group(self) -> None:
        from cccc.ports.mcp import server as mcp_server
        from cccc.ports.mcp import common as mcp_common

        def _fake_call_daemon(req):
            raise AssertionError(f"daemon should not be called: {req}")

        with patch.dict(os.environ, _CLEAN_ENV, clear=False), \
             patch.object(mcp_common, "call_daemon", side_effect=_fake_call_daemon):
            with self.assertRaises(mcp_server.MCPError) as cm:
                mcp_server.handle_tool_call(
                    "cccc_message_send",
                    {
                        "group_id": "g_runtime",
                        "dst_group_id": "g_selected",
                        "actor_id": "peer1",
                        "text": "done",
                        "to": ["user"],
                        "suggested_user_message": "Please continue.",
                    },
                )

        self.assertEqual(cm.exception.code, "suggested_user_message_not_supported")

    def test_message_reply_passes_refs(self) -> None:
        from cccc.ports.mcp import server as mcp_server
        from cccc.ports.mcp import common as mcp_common

        captured = {}

        def _fake_call_daemon(req):
            captured["req"] = req
            return {"ok": True, "result": {"event_id": "ev_test"}}

        refs = [{"kind": "presentation_ref", "slot_id": "slot-4", "label": "P4", "locator_label": "Web"}]

        with _isolated_runtime_context(), \
             patch.dict(os.environ, _CLEAN_ENV, clear=False), \
             patch.object(mcp_common, "call_daemon", side_effect=_fake_call_daemon):
            out = mcp_server.handle_tool_call(
                "cccc_message_reply",
                {
                    "group_id": "g_test",
                    "actor_id": "peer1",
                    "event_id": "ev_1",
                    "text": "reply",
                    "refs": refs,
                },
            )

        self.assertEqual(out.get("event_id"), "ev_test")
        req = captured.get("req") or {}
        args = req.get("args") if isinstance(req.get("args"), dict) else {}
        self.assertEqual(args.get("refs"), refs)

    def test_message_reply_passes_suggested_user_message(self) -> None:
        from cccc.ports.mcp import server as mcp_server
        from cccc.ports.mcp import common as mcp_common

        captured = {}

        def _fake_call_daemon(req):
            captured["req"] = req
            return {"ok": True, "result": {"event_id": "ev_test"}}

        with patch.dict(os.environ, _CLEAN_ENV, clear=False), \
             patch.object(mcp_common, "call_daemon", side_effect=_fake_call_daemon):
            out = mcp_server.handle_tool_call(
                "cccc_message_reply",
                {
                    "group_id": "g_test",
                    "actor_id": "peer1",
                    "event_id": "ev_1",
                    "text": "reply",
                    "suggested_user_message": "  Please run the next implementation pass.  ",
                },
            )

        self.assertEqual(out.get("event_id"), "ev_test")
        req = captured.get("req") or {}
        args = req.get("args") if isinstance(req.get("args"), dict) else {}
        self.assertEqual(args.get("suggested_user_message"), "Please run the next implementation pass.")

    def test_tracked_send_passes_task_contract_args(self) -> None:
        from cccc.ports.mcp import server as mcp_server
        from cccc.ports.mcp import common as mcp_common

        captured = {}

        def _fake_call_daemon(req):
            captured["req"] = req
            return {"ok": True, "result": {"task_id": "T001", "message_sent": True}}

        checklist = [{"text": "Check"}, {"text": "Report", "status": "pending"}]
        with _isolated_runtime_context(), \
             patch.dict(os.environ, _CLEAN_ENV, clear=False), \
             patch.object(mcp_common, "call_daemon", side_effect=_fake_call_daemon):
            out = mcp_server.handle_tool_call(
                "cccc_tracked_send",
                {
                    "group_id": "g_test",
                    "actor_id": "foreman",
                    "title": "Review PR",
                    "text": "Please review this PR and report evidence.",
                    "to": "reviewer",
                    "outcome": "Review findings reported.",
                    "checklist": checklist,
                    "idempotency_key": "req-1",
                },
            )

        self.assertEqual(out.get("task_id"), "T001")
        req = captured.get("req") or {}
        self.assertEqual(req.get("op"), "tracked_send")
        args = req.get("args") if isinstance(req.get("args"), dict) else {}
        self.assertEqual(args.get("to"), ["reviewer"])
        self.assertEqual(args.get("title"), "Review PR")
        self.assertEqual(args.get("checklist"), checklist)
        self.assertTrue(args.get("reply_required"))

    def test_message_send_allows_codex_headless_actor(self) -> None:
        from cccc.contracts.v1 import DaemonRequest
        from cccc.daemon.server import handle_request
        from cccc.ports.mcp import common as mcp_common
        from cccc.ports.mcp import server as mcp_server

        captured = {}

        def _fake_call_daemon(req):
            captured["req"] = req
            return {"ok": True, "result": {"event_id": "ev_headless"}}

        with tempfile.TemporaryDirectory() as td, patch.dict(os.environ, {**_CLEAN_ENV, "CCCC_HOME": td}, clear=False):
            create_resp, _ = handle_request(
                DaemonRequest.model_validate({"op": "group_create", "args": {"title": "headless-send", "topic": "", "by": "user"}})
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
                            "actor_id": "peer1",
                            "runtime": "codex",
                            "runner": "headless",
                            "by": "user",
                        },
                    }
                )
            )
            self.assertTrue(add_resp.ok, getattr(add_resp, "error", None))

            with _isolated_runtime_context(), patch.object(mcp_common, "call_daemon", side_effect=_fake_call_daemon):
                out = mcp_server.handle_tool_call(
                    "cccc_message_send",
                    {
                        "group_id": group_id,
                        "actor_id": "peer1",
                        "text": "hello",
                        "to": ["user"],
                    },
                )

        self.assertEqual(out.get("event_id"), "ev_headless")
        req = captured.get("req") or {}
        self.assertEqual(req.get("op"), "send")
        args = req.get("args") if isinstance(req.get("args"), dict) else {}
        self.assertEqual(args.get("group_id"), group_id)
        self.assertEqual(args.get("by"), "peer1")


if __name__ == "__main__":
    unittest.main()
