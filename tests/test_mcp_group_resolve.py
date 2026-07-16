import os
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

_CLEAN_ENV = {"CCCC_GROUP_ID": "", "CCCC_ACTOR_ID": ""}


class TestMcpGroupResolve(unittest.TestCase):
    def test_group_resolve_returns_unique_real_group_id_for_hash_token(self) -> None:
        from cccc.ports.mcp import common as mcp_common
        from cccc.ports.mcp import server as mcp_server

        def _fake_call_daemon(req):
            self.assertEqual(req.get("op"), "groups")
            return {
                "ok": True,
                "result": {
                    "groups": [
                        {"group_id": "g_other", "title": "other", "topic": "", "running": True},
                        {"group_id": "g_cccc", "title": "cccc", "topic": "core group", "running": True},
                    ]
                },
            }

        with patch.dict(os.environ, _CLEAN_ENV, clear=False), patch.object(
            mcp_common, "call_daemon", side_effect=_fake_call_daemon
        ):
            out = mcp_server.handle_tool_call("cccc_group", {"action": "resolve", "token": "#cccc"})

        self.assertEqual(out.get("group_id"), "g_cccc")
        self.assertEqual(out.get("title"), "cccc")
        self.assertEqual(out.get("matched_by"), "title")

    def test_group_resolve_reports_not_found_without_guessing(self) -> None:
        from cccc.ports.mcp import common as mcp_common
        from cccc.ports.mcp import server as mcp_server

        with patch.dict(os.environ, _CLEAN_ENV, clear=False), patch.object(
            mcp_common,
            "call_daemon",
            return_value={"ok": True, "result": {"groups": [{"group_id": "g_other", "title": "other"}]}},
        ):
            with self.assertRaises(mcp_server.MCPError) as raised:
                mcp_server.handle_tool_call("cccc_group", {"action": "resolve", "token": "cccc"})

        self.assertEqual(raised.exception.code, "not_found")
        self.assertIn("no group matches token: cccc", str(raised.exception))
        self.assertIn("Do not guess dst_group_id", str(raised.exception))

    def test_group_resolve_reports_ambiguous_candidates(self) -> None:
        from cccc.ports.mcp import common as mcp_common
        from cccc.ports.mcp import server as mcp_server

        with patch.dict(os.environ, _CLEAN_ENV, clear=False), patch.object(
            mcp_common,
            "call_daemon",
            return_value={
                "ok": True,
                "result": {
                    "groups": [
                        {"group_id": "g_one", "title": "cccc", "topic": "alpha"},
                        {"group_id": "g_two", "title": "cccc", "topic": "beta"},
                    ]
                },
            },
        ):
            with self.assertRaises(mcp_server.MCPError) as raised:
                mcp_server.handle_tool_call("cccc_group", {"action": "resolve", "token": "#cccc"})

        self.assertEqual(raised.exception.code, "ambiguous")
        self.assertIn("multiple groups match token: #cccc", str(raised.exception))
        self.assertEqual(
            [item.get("group_id") for item in raised.exception.details.get("candidates", [])],
            ["g_one", "g_two"],
        )

    def test_group_resolve_returns_current_group_group_bridge_remote_title(self) -> None:
        from cccc.kernel.group_bridge import pairing as pairing_kernel
        from cccc.ports.mcp.common import runtime_context_override
        from cccc.ports.mcp import common as mcp_common
        from cccc.ports.mcp import server as mcp_server

        with tempfile.TemporaryDirectory() as td, \
             patch.dict(os.environ, _CLEAN_ENV, clear=False), \
             patch("cccc.kernel.group_bridge.pairing.ensure_home", return_value=Path(td)), \
             patch.object(mcp_common, "call_daemon", return_value={"ok": True, "result": {"groups": []}}), \
             runtime_context_override(home=td, group_id="g_local", actor_id="test-peer"):
            request = pairing_kernel.create_pairing_request(
                pairing_kernel.create_pairing_invite(group_id="g_local")["pairing_code"],
                requester_group_id="g_remote",
                requester_group_title="CCCC Cross Test",
                requester_peer_id="peer_remote",
                requester_endpoint="http://remote.example:8848",
            )
            approved = pairing_kernel.approve_pairing_request(request["request_id"], approver_user_id="user-a")

            out = mcp_server.handle_tool_call(
                "cccc_group",
                {"action": "resolve", "group_id": "g_local", "token": "#CCCC Cross Test"},
            )

        self.assertEqual(out.get("group_id"), "g_remote")
        self.assertEqual(out.get("title"), "CCCC Cross Test")
        self.assertEqual(out.get("matched_by"), "group_bridge_remote_group_title")
        self.assertEqual(out.get("registration_id"), approved["registration"]["registration_id"])
        self.assertEqual(out.get("group_bridge"), True)

    def test_group_resolve_ignores_revoked_group_bridge_remote_title(self) -> None:
        from cccc.kernel.group_bridge import pairing as pairing_kernel
        from cccc.ports.mcp.common import runtime_context_override
        from cccc.ports.mcp import common as mcp_common
        from cccc.ports.mcp import server as mcp_server

        with tempfile.TemporaryDirectory() as td, \
             patch.dict(os.environ, _CLEAN_ENV, clear=False), \
             patch("cccc.kernel.group_bridge.pairing.ensure_home", return_value=Path(td)), \
             patch.object(mcp_common, "call_daemon", return_value={"ok": True, "result": {"groups": []}}), \
             runtime_context_override(home=td, group_id="g_local", actor_id="test-peer"):
            request = pairing_kernel.create_pairing_request(
                pairing_kernel.create_pairing_invite(group_id="g_local")["pairing_code"],
                requester_group_id="g_0fb5f39478cc",
                requester_group_title="CCCC Cross Test",
                requester_peer_id="peer_00e780d5eb7bad9dea41bba479a9c292",
                requester_endpoint="http://remote.example:8848",
            )
            approved = pairing_kernel.approve_pairing_request(request["request_id"], approver_user_id="user-a")
            pairing_kernel.revoke_trust(approved["trust"]["trust_id"], revoked_by="user-a")

            with self.assertRaises(mcp_server.MCPError) as raised:
                mcp_server.handle_tool_call(
                    "cccc_group",
                    {"action": "resolve", "group_id": "g_local", "token": "#CCCC Cross Test"},
                )

        self.assertEqual(raised.exception.code, "not_found")
        self.assertIn("no group matches token: #CCCC Cross Test", str(raised.exception))


if __name__ == "__main__":
    unittest.main()
