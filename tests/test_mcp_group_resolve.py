import os
import unittest
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


if __name__ == "__main__":
    unittest.main()
