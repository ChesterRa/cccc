import os
import unittest
from unittest.mock import patch

# Env vars that _resolve_group_id / _resolve_caller_actor_id read at runtime.
# Tests must isolate from the host environment to avoid group_id_mismatch.
_CLEAN_ENV = {"CCCC_GROUP_ID": "", "CCCC_ACTOR_ID": ""}


class TestMcpGroupSetStateStopped(unittest.TestCase):
    def test_group_set_state_stopped_maps_to_group_stop(self) -> None:
        from cccc.ports.mcp import server as mcp_server
        from cccc.ports.mcp import common as mcp_common

        captured = {}

        def _fake_call_daemon(req):
            captured["req"] = req
            return {"ok": True, "result": {"group_id": "g_test"}}

        with patch.dict(os.environ, _CLEAN_ENV, clear=False), \
             patch.object(mcp_common, "call_daemon", side_effect=_fake_call_daemon):
            out = mcp_server.handle_tool_call(
                "cccc_group_set_state",
                {
                    "group_id": "g_test",
                    "actor_id": "foreman",
                    "state": "stopped",
                },
            )

        self.assertEqual(out.get("group_id"), "g_test")
        req = captured.get("req") or {}
        self.assertEqual(req.get("op"), "group_stop")
        args = req.get("args") if isinstance(req.get("args"), dict) else {}
        self.assertEqual(args.get("group_id"), "g_test")
        self.assertEqual(args.get("by"), "foreman")


if __name__ == "__main__":
    unittest.main()
