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

    @staticmethod
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

    def test_peer_cannot_update_coordination_brief_via_mcp(self) -> None:
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

            with patch.object(mcp_common, "call_daemon", side_effect=self._fake_call_daemon), patch.dict(
                os.environ,
                {"CCCC_GROUP_ID": group_id, "CCCC_ACTOR_ID": "peer-impl"},
                clear=False,
            ):
                with self.assertRaises(MCPError) as err:
                    mcp_server.handle_tool_call(
                        "cccc_coordination",
                        {"action": "update_brief", "objective": "hijacked"},
                    )
                self.assertEqual(err.exception.code, "context_sync_error")
                self.assertIn("Permission denied", str(err.exception.message))

            read_resp, _ = self._call("context_get", {"group_id": group_id})
            self.assertTrue(read_resp.ok, getattr(read_resp, "error", None))
            result = read_resp.result if isinstance(read_resp.result, dict) else {}
            self.assertEqual(str((((result.get("coordination") or {}).get("brief") or {}).get("objective") or "")), "")
        finally:
            cleanup()

    def test_actor_notes_read_is_self_only_for_peers_and_full_for_foreman(self) -> None:
        from cccc.ports.mcp import common as mcp_common
        from cccc.ports.mcp import server as mcp_server
        from cccc.kernel.group import load_group
        from cccc.kernel.prompt_files import HELP_FILENAME, write_group_prompt_file

        _, cleanup = self._with_home()
        try:
            create_resp, _ = self._call("group_create", {"title": "mcp-actor-notes", "topic": "", "by": "user"})
            self.assertTrue(create_resp.ok, getattr(create_resp, "error", None))
            group_id = str((create_resp.result or {}).get("group_id") or "").strip()
            self.assertTrue(group_id)

            for actor_id in ("foreman-impl", "peer-impl", "peer-2"):
                add_resp, _ = self._call(
                    "actor_add",
                    {
                        "group_id": group_id,
                        "actor_id": actor_id,
                        "runtime": "codex",
                        "runner": "headless",
                        "by": "user",
                    },
                )
                self.assertTrue(add_resp.ok, getattr(add_resp, "error", None))

            group = load_group(group_id)
            self.assertIsNotNone(group)
            write_group_prompt_file(
                group,
                HELP_FILENAME,
                (
                    "## @actor: peer-impl\n\nself notes\n\n"
                    "## @actor: peer-2\n\npeer-2 secret\n\n"
                    "## @actor: foreman-impl\n\nforeman secret\n"
                ),
            )

            with patch.object(mcp_common, "call_daemon", side_effect=self._fake_call_daemon), patch.dict(
                os.environ,
                {"CCCC_GROUP_ID": group_id, "CCCC_ACTOR_ID": "peer-impl"},
                clear=False,
            ):
                own = mcp_server.handle_tool_call(
                    "cccc_actor_notes",
                    {"action": "get", "target_actor_id": "peer-impl"},
                )
                self.assertEqual(str(own.get("content") or ""), "self notes")
                with self.assertRaises(MCPError) as other_err:
                    mcp_server.handle_tool_call(
                        "cccc_actor_notes",
                        {"action": "get", "target_actor_id": "peer-2"},
                    )
                self.assertEqual(other_err.exception.code, "permission_denied")
                with self.assertRaises(MCPError) as list_err:
                    mcp_server.handle_tool_call("cccc_actor_notes", {"action": "get"})
                self.assertEqual(list_err.exception.code, "permission_denied")

            with patch.object(mcp_common, "call_daemon", side_effect=self._fake_call_daemon), patch.dict(
                os.environ,
                {"CCCC_GROUP_ID": group_id, "CCCC_ACTOR_ID": "foreman-impl"},
                clear=False,
            ):
                all_notes = mcp_server.handle_tool_call("cccc_actor_notes", {"action": "get"})
                actor_notes = all_notes.get("actor_notes") if isinstance(all_notes.get("actor_notes"), list) else []
                self.assertEqual(
                    sorted((str(item.get("actor_id") or ""), str(item.get("content") or "")) for item in actor_notes if isinstance(item, dict)),
                    [
                        ("foreman-impl", "foreman secret"),
                        ("peer-2", "peer-2 secret"),
                        ("peer-impl", "self notes"),
                    ],
                )
        finally:
            cleanup()

    def test_foreman_actor_notes_set_updates_help_actor_block_without_touching_persona_notes(self) -> None:
        from cccc.kernel.group import load_group
        from cccc.kernel.prompt_files import HELP_FILENAME, read_group_prompt_file
        from cccc.ports.mcp import common as mcp_common
        from cccc.ports.mcp import server as mcp_server

        _, cleanup = self._with_home()
        try:
            create_resp, _ = self._call("group_create", {"title": "mcp-actor-notes-set", "topic": "", "by": "user"})
            self.assertTrue(create_resp.ok, getattr(create_resp, "error", None))
            group_id = str((create_resp.result or {}).get("group_id") or "").strip()
            self.assertTrue(group_id)

            for actor_id in ("foreman-impl", "peer-impl"):
                add_resp, _ = self._call(
                    "actor_add",
                    {
                        "group_id": group_id,
                        "actor_id": actor_id,
                        "runtime": "codex",
                        "runner": "headless",
                        "by": "user",
                    },
                )
                self.assertTrue(add_resp.ok, getattr(add_resp, "error", None))

            notify_calls: list[dict] = []

            def fake_daemon(req, timeout_s=None):
                op = str(req.get("op") or "")
                if op == "actor_list":
                    return {
                        "ok": True,
                        "result": {
                            "actors": [
                                {"id": "foreman-impl", "role": "foreman", "running": True},
                                {"id": "peer-impl", "role": "peer", "running": True},
                            ]
                        },
                    }
                if op == "system_notify":
                    notify_calls.append(req)
                    return {"ok": True, "result": {"accepted": True}}
                return self._fake_call_daemon(req, timeout_s=timeout_s)

            with patch.object(mcp_common, "call_daemon", side_effect=fake_daemon), patch.dict(
                os.environ,
                {"CCCC_GROUP_ID": group_id, "CCCC_ACTOR_ID": "foreman-impl"},
                clear=False,
            ):
                updated = mcp_server.handle_tool_call(
                    "cccc_actor_notes",
                    {
                        "action": "set",
                        "target_actor_id": "peer-impl",
                        "content": "Stay skeptical.\nUse receipts.",
                    },
                )
                self.assertEqual(str(updated.get("target_actor_id") or ""), "peer-impl")
                self.assertEqual(str(updated.get("content") or ""), "Stay skeptical.\nUse receipts.")
                self.assertEqual(updated.get("notified_actor_ids"), ["peer-impl"])

            group = load_group(group_id)
            self.assertIsNotNone(group)
            prompt_file = read_group_prompt_file(group, HELP_FILENAME)
            self.assertTrue(prompt_file.found)
            help_content = str(prompt_file.content or "")
            self.assertIn("## @actor: peer-impl", help_content)
            self.assertIn("Stay skeptical.\nUse receipts.", help_content)

            with patch.object(mcp_common, "call_daemon", side_effect=fake_daemon), patch.dict(
                os.environ,
                {"CCCC_GROUP_ID": group_id, "CCCC_ACTOR_ID": "peer-impl"},
                clear=False,
            ), patch(
                "cccc.ports.mcp.server._append_runtime_help_addenda",
                side_effect=lambda markdown, group_id, actor_id: markdown,
            ):
                help_resp = mcp_server.handle_tool_call("cccc_help", {})
                markdown = str(help_resp.get("markdown") or "")
                self.assertIn("## Notes for you", markdown)
                self.assertIn("Stay skeptical.\nUse receipts.", markdown)

            context_resp, _ = self._call("context_get", {"group_id": group_id})
            self.assertTrue(context_resp.ok, getattr(context_resp, "error", None))
            states = (context_resp.result or {}).get("agent_states") if isinstance(context_resp.result, dict) else []
            peer_state = next(
                (
                    item
                    for item in (states or [])
                    if isinstance(item, dict) and str(item.get("id") or "").strip() == "peer-impl"
                ),
                None,
            )
            warm = peer_state.get("warm") if isinstance(peer_state, dict) and isinstance(peer_state.get("warm"), dict) else {}
            self.assertEqual(str(warm.get("persona_notes") or ""), "")
            self.assertEqual(len(notify_calls), 1)
            notify_args = notify_calls[0].get("args") if isinstance(notify_calls[0].get("args"), dict) else {}
            self.assertEqual(str(notify_args.get("target_actor_id") or ""), "peer-impl")
        finally:
            cleanup()

    def test_peer_cannot_set_actor_notes_via_mcp(self) -> None:
        from cccc.ports.mcp import common as mcp_common
        from cccc.ports.mcp import server as mcp_server

        _, cleanup = self._with_home()
        try:
            create_resp, _ = self._call("group_create", {"title": "mcp-actor-notes-peer-write", "topic": "", "by": "user"})
            self.assertTrue(create_resp.ok, getattr(create_resp, "error", None))
            group_id = str((create_resp.result or {}).get("group_id") or "").strip()
            self.assertTrue(group_id)

            for actor_id in ("foreman-impl", "peer-impl"):
                add_resp, _ = self._call(
                    "actor_add",
                    {
                        "group_id": group_id,
                        "actor_id": actor_id,
                        "runtime": "codex",
                        "runner": "headless",
                        "by": "user",
                    },
                )
                self.assertTrue(add_resp.ok, getattr(add_resp, "error", None))

            with patch.object(mcp_common, "call_daemon", side_effect=self._fake_call_daemon), patch.dict(
                os.environ,
                {"CCCC_GROUP_ID": group_id, "CCCC_ACTOR_ID": "peer-impl"},
                clear=False,
            ):
                with self.assertRaises(MCPError) as err:
                    mcp_server.handle_tool_call(
                        "cccc_actor_notes",
                        {"action": "set", "target_actor_id": "peer-impl", "content": "I should not self-author this"},
                    )
                self.assertEqual(err.exception.code, "permission_denied")
        finally:
            cleanup()


if __name__ == "__main__":
    unittest.main()
