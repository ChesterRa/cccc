from __future__ import annotations

import os
import tempfile
import unittest
from unittest.mock import patch

from cccc.ports.mcp.common import MCPError


class TestMcpContextPermissionEnforcement(unittest.TestCase):
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

    def _call(self, op: str, args: dict):
        from cccc.contracts.v1 import DaemonRequest
        from cccc.daemon.server import handle_request

        return handle_request(DaemonRequest.model_validate({"op": op, "args": args}))

    def test_peer_cannot_update_vision_via_mcp(self) -> None:
        from cccc.ports.mcp import common as mcp_common
        from cccc.ports.mcp import server as mcp_server

        _, cleanup = self._with_home()
        try:
            create_resp, _ = self._call("group_create", {"title": "mcp-context-perm", "topic": "", "by": "user"})
            self.assertTrue(create_resp.ok, getattr(create_resp, "error", None))
            group_id = str((create_resp.result or {}).get("group_id") or "").strip()
            self.assertTrue(group_id)

            foreman_add_resp, _ = self._call(
                "actor_add",
                {
                    "group_id": group_id,
                    "actor_id": "foreman-impl",
                    "runtime": "codex",
                    "runner": "headless",
                    "by": "user",
                },
            )
            self.assertTrue(foreman_add_resp.ok, getattr(foreman_add_resp, "error", None))

            add_resp, _ = self._call(
                "actor_add",
                {
                    "group_id": group_id,
                    "actor_id": "peer-impl",
                    "runtime": "codex",
                    "runner": "headless",
                    "by": "user",
                },
            )
            self.assertTrue(add_resp.ok, getattr(add_resp, "error", None))

            def _fake_call_daemon(req, timeout_s=None):
                from cccc.contracts.v1 import DaemonRequest
                from cccc.daemon.server import handle_request

                _ = timeout_s
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

            with patch.object(mcp_common, "call_daemon", side_effect=_fake_call_daemon), patch.dict(
                os.environ,
                {"CCCC_GROUP_ID": group_id, "CCCC_ACTOR_ID": "peer-impl"},
                clear=False,
            ):
                with self.assertRaises(MCPError) as err:
                    mcp_server.handle_tool_call(
                        "cccc_context_admin",
                        {"action": "vision_update", "vision": "hijacked"},
                    )
                self.assertEqual(err.exception.code, "context_sync_error")
                self.assertIn("Permission denied", str(err.exception.message))

            read_resp, _ = self._call("context_get", {"group_id": group_id})
            self.assertTrue(read_resp.ok, getattr(read_resp, "error", None))
            result = read_resp.result if isinstance(read_resp.result, dict) else {}
            self.assertEqual(str(result.get("vision") or ""), "")
        finally:
            cleanup()


if __name__ == "__main__":
    unittest.main()
