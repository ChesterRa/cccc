import os
import tempfile
import unittest
from unittest.mock import patch


class TestRuntimeMcpFailFast(unittest.TestCase):
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

    def test_actor_start_fails_when_mcp_install_reports_false_for_pty(self) -> None:
        _, cleanup = self._with_home()
        try:
            create, _ = self._call("group_create", {"title": "actor-start-mcp-fail", "topic": "", "by": "user"})
            self.assertTrue(create.ok, getattr(create, "error", None))
            group_id = str((create.result or {}).get("group_id") or "").strip()
            self.assertTrue(group_id)

            attach, _ = self._call("attach", {"group_id": group_id, "path": ".", "by": "user"})
            self.assertTrue(attach.ok, getattr(attach, "error", None))

            add, _ = self._call(
                "actor_add",
                {
                    "group_id": group_id,
                    "actor_id": "peer1",
                    "title": "Peer 1",
                    "runtime": "codex",
                    "runner": "pty",
                    "by": "user",
                },
            )
            self.assertTrue(add.ok, getattr(add, "error", None))

            disable, _ = self._call(
                "actor_update",
                {"group_id": group_id, "actor_id": "peer1", "patch": {"enabled": False}, "by": "user"},
            )
            self.assertTrue(disable.ok, getattr(disable, "error", None))

            with patch("cccc.daemon.server._ensure_mcp_installed", return_value=False), patch(
                "cccc.daemon.server._REQUEST_DISPATCH_DEPS", None
            ):
                start, _ = self._call("actor_start", {"group_id": group_id, "actor_id": "peer1", "by": "user"})

            self.assertFalse(start.ok)
            self.assertEqual(getattr(start.error, "code", ""), "actor_start_failed")
            self.assertIn("failed to install MCP for runtime: codex", getattr(start.error, "message", ""))
        finally:
            cleanup()

    def test_actor_restart_fails_when_mcp_install_reports_false_for_pty(self) -> None:
        _, cleanup = self._with_home()
        try:
            from cccc.kernel.group import load_group

            create, _ = self._call("group_create", {"title": "actor-restart-mcp-fail", "topic": "", "by": "user"})
            self.assertTrue(create.ok, getattr(create, "error", None))
            group_id = str((create.result or {}).get("group_id") or "").strip()
            self.assertTrue(group_id)

            attach, _ = self._call("attach", {"group_id": group_id, "path": ".", "by": "user"})
            self.assertTrue(attach.ok, getattr(attach, "error", None))

            add, _ = self._call(
                "actor_add",
                {
                    "group_id": group_id,
                    "actor_id": "peer1",
                    "title": "Peer 1",
                    "runtime": "codex",
                    "runner": "pty",
                    "by": "user",
                },
            )
            self.assertTrue(add.ok, getattr(add, "error", None))

            group = load_group(group_id)
            self.assertIsNotNone(group)
            assert group is not None
            group.doc["running"] = True
            group.save()

            with patch("cccc.daemon.server._ensure_mcp_installed", return_value=False), patch(
                "cccc.daemon.server._REQUEST_DISPATCH_DEPS", None
            ):
                restart, _ = self._call("actor_restart", {"group_id": group_id, "actor_id": "peer1", "by": "user"})

            self.assertFalse(restart.ok)
            self.assertEqual(getattr(restart.error, "code", ""), "actor_restart_failed")
            self.assertIn("failed to install MCP for runtime: codex", getattr(restart.error, "message", ""))
        finally:
            cleanup()

    def test_actor_update_enable_fails_when_mcp_install_reports_false_for_pty(self) -> None:
        _, cleanup = self._with_home()
        try:
            from cccc.kernel.group import load_group

            create, _ = self._call("group_create", {"title": "actor-update-mcp-fail", "topic": "", "by": "user"})
            self.assertTrue(create.ok, getattr(create, "error", None))
            group_id = str((create.result or {}).get("group_id") or "").strip()
            self.assertTrue(group_id)

            attach, _ = self._call("attach", {"group_id": group_id, "path": ".", "by": "user"})
            self.assertTrue(attach.ok, getattr(attach, "error", None))

            add, _ = self._call(
                "actor_add",
                {
                    "group_id": group_id,
                    "actor_id": "peer1",
                    "title": "Peer 1",
                    "runtime": "codex",
                    "runner": "pty",
                    "by": "user",
                },
            )
            self.assertTrue(add.ok, getattr(add, "error", None))

            disable, _ = self._call(
                "actor_update",
                {"group_id": group_id, "actor_id": "peer1", "patch": {"enabled": False}, "by": "user"},
            )
            self.assertTrue(disable.ok, getattr(disable, "error", None))

            group = load_group(group_id)
            self.assertIsNotNone(group)
            assert group is not None
            group.doc["running"] = True
            group.save()

            with patch("cccc.daemon.server._ensure_mcp_installed", return_value=False), patch(
                "cccc.daemon.server._REQUEST_DISPATCH_DEPS", None
            ):
                enable, _ = self._call(
                    "actor_update",
                    {"group_id": group_id, "actor_id": "peer1", "patch": {"enabled": True}, "by": "user"},
                )

            self.assertFalse(enable.ok)
            self.assertEqual(getattr(enable.error, "code", ""), "actor_update_failed")
            self.assertIn("failed to install MCP for runtime: codex", getattr(enable.error, "message", ""))
        finally:
            cleanup()

    def test_group_start_fails_when_mcp_install_reports_false_for_pty(self) -> None:
        _, cleanup = self._with_home()
        try:
            create, _ = self._call("group_create", {"title": "group-start-mcp-fail", "topic": "", "by": "user"})
            self.assertTrue(create.ok, getattr(create, "error", None))
            group_id = str((create.result or {}).get("group_id") or "").strip()
            self.assertTrue(group_id)

            attach, _ = self._call("attach", {"group_id": group_id, "path": ".", "by": "user"})
            self.assertTrue(attach.ok, getattr(attach, "error", None))

            add, _ = self._call(
                "actor_add",
                {
                    "group_id": group_id,
                    "actor_id": "peer1",
                    "title": "Peer 1",
                    "runtime": "codex",
                    "runner": "pty",
                    "by": "user",
                },
            )
            self.assertTrue(add.ok, getattr(add, "error", None))

            with patch("cccc.daemon.server._ensure_mcp_installed", return_value=False), patch(
                "cccc.daemon.server._REQUEST_DISPATCH_DEPS", None
            ):
                start, _ = self._call("group_start", {"group_id": group_id, "by": "user"})

            self.assertFalse(start.ok)
            self.assertEqual(getattr(start.error, "code", ""), "group_start_failed")
            self.assertIn("failed to install MCP for actor peer1", getattr(start.error, "message", ""))
        finally:
            cleanup()


if __name__ == "__main__":
    unittest.main()
