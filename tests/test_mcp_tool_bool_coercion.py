import os
import tempfile
import unittest
from unittest.mock import patch


class TestMcpToolBoolCoercion(unittest.TestCase):
    def test_group_info_running_string_false(self) -> None:
        from cccc.ports.mcp import server as mcp_server

        with patch.object(
            mcp_server,
            "_call_daemon_or_raise",
            return_value={"group": {"group_id": "g_test", "running": "false", "title": "t"}},
        ):
            result = mcp_server.group_info(group_id="g_test")
            group = result.get("group") if isinstance(result, dict) else {}
            self.assertIsInstance(group, dict)
            assert isinstance(group, dict)
            self.assertFalse(bool(group.get("running")))

    def test_actor_list_enabled_running_string_coercion(self) -> None:
        from cccc.ports.mcp import server as mcp_server

        with patch.object(
            mcp_server,
            "_call_daemon_or_raise",
            return_value={"actors": [{"id": "peer1", "enabled": "false", "running": "true"}]},
        ):
            result = mcp_server.actor_list(group_id="g_test")
            actors = result.get("actors") if isinstance(result, dict) else []
            self.assertIsInstance(actors, list)
            assert isinstance(actors, list)
            self.assertEqual(len(actors), 1)
            self.assertFalse(bool(actors[0].get("enabled")))
            self.assertTrue(bool(actors[0].get("running")))

    def test_file_send_blocks_path_outside_scope_root(self) -> None:
        from cccc.ports.mcp import server as mcp_server

        class _FakeGroup:
            def __init__(self, root: str) -> None:
                self.group_id = "g_test"
                self.doc = {
                    "active_scope_key": "s1",
                    "scopes": [{"scope_key": "s1", "url": root}],
                }

        with tempfile.TemporaryDirectory() as td:
            scope_root = os.path.join(td, "scope")
            outside_root = os.path.join(td, "outside")
            os.makedirs(scope_root, exist_ok=True)
            os.makedirs(outside_root, exist_ok=True)
            outside_file = os.path.join(outside_root, "note.txt")
            with open(outside_file, "w", encoding="utf-8") as f:
                f.write("x")

            with patch.object(mcp_server, "load_group", return_value=_FakeGroup(scope_root)):
                with self.assertRaises(mcp_server.MCPError) as cm:
                    mcp_server.file_send(
                        group_id="g_test",
                        actor_id="peer1",
                        path=outside_file,
                        text="hello",
                    )
            self.assertEqual(cm.exception.code, "invalid_path")

    def test_file_send_coerces_reply_required_string_false(self) -> None:
        from cccc.ports.mcp import server as mcp_server

        class _FakeGroup:
            def __init__(self, root: str) -> None:
                self.group_id = "g_test"
                self.doc = {
                    "active_scope_key": "s1",
                    "scopes": [{"scope_key": "s1", "url": root}],
                }

        captured = {}
        with tempfile.TemporaryDirectory() as td:
            scope_root = os.path.join(td, "scope")
            os.makedirs(scope_root, exist_ok=True)
            in_file = os.path.join(scope_root, "note.txt")
            with open(in_file, "w", encoding="utf-8") as f:
                f.write("hello")

            def _fake_call(req):
                captured["req"] = req
                return {"ok": True, "event_id": "ev_test"}

            with patch.object(mcp_server, "load_group", return_value=_FakeGroup(scope_root)), patch.object(
                mcp_server, "store_blob_bytes", return_value={"title": "note.txt", "path": "blobs/note.txt"}
            ), patch.object(mcp_server, "_call_daemon_or_raise", side_effect=_fake_call):
                mcp_server.file_send(
                    group_id="g_test",
                    actor_id="peer1",
                    path=in_file,
                    text="hello",
                    reply_required="false",  # type: ignore[arg-type]
                )

        req = captured.get("req") if isinstance(captured.get("req"), dict) else {}
        args = req.get("args") if isinstance(req.get("args"), dict) else {}
        self.assertFalse(bool(args.get("reply_required")))

    def test_message_send_coerces_reply_required_string_false(self) -> None:
        from cccc.ports.mcp import server as mcp_server

        captured = {}

        def _fake_call(req):
            captured["req"] = req
            return {"ok": True, "event_id": "ev_test"}

        with patch.object(mcp_server, "_call_daemon_or_raise", side_effect=_fake_call):
            mcp_server.message_send(
                group_id="g_test",
                actor_id="peer1",
                text="hello",
                to=["user"],
                reply_required="false",  # type: ignore[arg-type]
            )

        req = captured.get("req") if isinstance(captured.get("req"), dict) else {}
        args = req.get("args") if isinstance(req.get("args"), dict) else {}
        self.assertFalse(bool(args.get("reply_required")))

    def test_message_reply_coerces_reply_required_string_false(self) -> None:
        from cccc.ports.mcp import server as mcp_server

        captured = {}

        def _fake_call(req):
            captured["req"] = req
            return {"ok": True, "event_id": "ev_test"}

        with patch.object(mcp_server, "_call_daemon_or_raise", side_effect=_fake_call):
            mcp_server.message_reply(
                group_id="g_test",
                actor_id="peer1",
                reply_to="ev_1",
                text="hello",
                to=["user"],
                reply_required="false",  # type: ignore[arg-type]
            )

        req = captured.get("req") if isinstance(captured.get("req"), dict) else {}
        args = req.get("args") if isinstance(req.get("args"), dict) else {}
        self.assertFalse(bool(args.get("reply_required")))

    def test_group_list_running_string_false(self) -> None:
        from cccc.ports.mcp import server as mcp_server

        with patch.object(
            mcp_server,
            "_call_daemon_or_raise",
            return_value={
                "groups": [
                    {
                        "group_id": "g_test",
                        "title": "t",
                        "topic": "",
                        "running": "false",
                    }
                ]
            },
        ):
            result = mcp_server.group_list()
            groups = result.get("groups") if isinstance(result, dict) else []
            self.assertIsInstance(groups, list)
            assert isinstance(groups, list)
            self.assertEqual(len(groups), 1)
            self.assertFalse(bool(groups[0].get("running")))

    def test_context_get_include_archived_string_false(self) -> None:
        from cccc.ports.mcp import server as mcp_server

        with patch.object(mcp_server, "_resolve_group_id", return_value="g_test"), patch.object(
            mcp_server, "context_get", return_value={"ok": True}
        ) as mock_context_get:
            mcp_server.handle_tool_call("cccc_context_get", {"include_archived": "false"})
            self.assertTrue(mock_context_get.called)
            kwargs = mock_context_get.call_args.kwargs
            self.assertEqual(kwargs.get("group_id"), "g_test")
            self.assertFalse(bool(kwargs.get("include_archived")))

    def test_context_sync_dry_run_string_false(self) -> None:
        from cccc.ports.mcp import server as mcp_server

        with patch.object(mcp_server, "_resolve_group_id", return_value="g_test"), patch.object(
            mcp_server, "context_sync", return_value={"ok": True}
        ) as mock_context_sync:
            mcp_server.handle_tool_call("cccc_context_sync", {"ops": [], "dry_run": "false"})
            self.assertTrue(mock_context_sync.called)
            kwargs = mock_context_sync.call_args.kwargs
            self.assertEqual(kwargs.get("group_id"), "g_test")
            self.assertFalse(bool(kwargs.get("dry_run")))

    def test_notify_send_requires_ack_string_false(self) -> None:
        from cccc.ports.mcp import server as mcp_server

        with patch.object(mcp_server, "_resolve_group_id", return_value="g_test"), patch.object(
            mcp_server, "_resolve_self_actor_id", return_value="peer1"
        ), patch.object(mcp_server, "notify_send", return_value={"ok": True}) as mock_notify_send:
            mcp_server.handle_tool_call(
                "cccc_notify_send",
                {
                    "kind": "info",
                    "title": "t",
                    "message": "m",
                    "requires_ack": "false",
                },
            )
            self.assertTrue(mock_notify_send.called)
            kwargs = mock_notify_send.call_args.kwargs
            self.assertEqual(kwargs.get("group_id"), "g_test")
            self.assertEqual(kwargs.get("actor_id"), "peer1")
            self.assertFalse(bool(kwargs.get("requires_ack")))

    def test_terminal_tail_strip_ansi_string_false(self) -> None:
        from cccc.ports.mcp import server as mcp_server

        with patch.object(mcp_server, "_resolve_group_id", return_value="g_test"), patch.object(
            mcp_server, "_resolve_self_actor_id", return_value="peer1"
        ), patch.object(mcp_server, "terminal_tail", return_value={"ok": True}) as mock_terminal_tail:
            mcp_server.handle_tool_call(
                "cccc_terminal_tail",
                {
                    "target_actor_id": "peer2",
                    "strip_ansi": "false",
                },
            )
            self.assertTrue(mock_terminal_tail.called)
            kwargs = mock_terminal_tail.call_args.kwargs
            self.assertEqual(kwargs.get("group_id"), "g_test")
            self.assertEqual(kwargs.get("actor_id"), "peer1")
            self.assertEqual(kwargs.get("target_actor_id"), "peer2")
            self.assertFalse(bool(kwargs.get("strip_ansi")))


if __name__ == "__main__":
    unittest.main()
