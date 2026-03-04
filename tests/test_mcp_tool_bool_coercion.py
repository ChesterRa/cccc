import os
import tempfile
import unittest
from unittest.mock import patch


class TestMcpToolBoolCoercion(unittest.TestCase):
    def test_group_info_running_string_false(self) -> None:
        from cccc.ports.mcp import server as mcp_server
        from cccc.ports.mcp.handlers import cccc_group_actor

        with patch.object(
            cccc_group_actor,
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
        from cccc.ports.mcp.handlers import cccc_group_actor

        with patch.object(
            cccc_group_actor,
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
        from cccc.ports.mcp.handlers import cccc_messaging

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

            with patch.object(cccc_messaging, "load_group", return_value=_FakeGroup(scope_root)):
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
        from cccc.ports.mcp.handlers import cccc_messaging

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

            with patch.object(cccc_messaging, "load_group", return_value=_FakeGroup(scope_root)), patch.object(
                cccc_messaging, "store_blob_bytes", return_value={"title": "note.txt", "path": "blobs/note.txt"}
            ), patch.object(cccc_messaging, "_call_daemon_or_raise", side_effect=_fake_call):
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
        from cccc.ports.mcp.handlers import cccc_messaging

        captured = {}

        def _fake_call(req):
            captured["req"] = req
            return {"ok": True, "event_id": "ev_test"}

        with patch.object(cccc_messaging, "_call_daemon_or_raise", side_effect=_fake_call):
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
        from cccc.ports.mcp.handlers import cccc_messaging

        captured = {}

        def _fake_call(req):
            captured["req"] = req
            return {"ok": True, "event_id": "ev_test"}

        with patch.object(cccc_messaging, "_call_daemon_or_raise", side_effect=_fake_call):
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
        from cccc.ports.mcp.handlers import cccc_group_actor

        with patch.object(
            cccc_group_actor,
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
            mcp_server, "_resolve_self_actor_id", return_value="peer1"
        ), patch.object(
            mcp_server, "context_sync", return_value={"ok": True}
        ) as mock_context_sync:
            mcp_server.handle_tool_call("cccc_context_sync", {"ops": [], "dry_run": "false"})
            self.assertTrue(mock_context_sync.called)
            kwargs = mock_context_sync.call_args.kwargs
            self.assertEqual(kwargs.get("group_id"), "g_test")
            self.assertFalse(bool(kwargs.get("dry_run")))
            self.assertEqual(str(kwargs.get("by") or ""), "peer1")

    def test_context_vision_update_forwards_caller_identity(self) -> None:
        from cccc.ports.mcp import server as mcp_server

        with patch.object(mcp_server, "_resolve_group_id", return_value="g_test"), patch.object(
            mcp_server, "_resolve_self_actor_id", return_value="peer1"
        ), patch.object(mcp_server, "vision_update", return_value={"ok": True}) as mock_vision_update:
            mcp_server.handle_tool_call("cccc_context_admin", {"action": "vision_update", "vision": "north-star"})
            self.assertTrue(mock_vision_update.called)
            kwargs = mock_vision_update.call_args.kwargs
            self.assertEqual(kwargs.get("group_id"), "g_test")
            self.assertEqual(str(kwargs.get("vision") or ""), "north-star")
            self.assertEqual(str(kwargs.get("by") or ""), "peer1")

    def test_notify_send_requires_ack_string_false(self) -> None:
        from cccc.ports.mcp import server as mcp_server

        with patch.object(mcp_server, "_resolve_group_id", return_value="g_test"), patch.object(
            mcp_server, "_resolve_self_actor_id", return_value="peer1"
        ), patch.object(mcp_server, "notify_send", return_value={"ok": True}) as mock_notify_send:
            mcp_server.handle_tool_call(
                "cccc_notify",
                {
                    "action": "send",
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
                "cccc_terminal",
                {
                    "action": "tail",
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

    def test_space_artifact_defaults_to_async_wait_false(self) -> None:
        from cccc.ports.mcp import server as mcp_server

        with patch.object(mcp_server, "_resolve_group_id", return_value="g_test"), patch.object(
            mcp_server, "_resolve_caller_from_by", return_value="peer1"
        ), patch.object(mcp_server, "space_artifact", return_value={"ok": True}) as mock_space_artifact:
            mcp_server.handle_tool_call(
                "cccc_space",
                {
                    "action": "artifact",
                    "sub_action": "generate",
                    "kind": "slide_deck",
                },
            )
            kwargs = mock_space_artifact.call_args.kwargs
            self.assertEqual(kwargs.get("group_id"), "g_test")
            self.assertEqual(kwargs.get("by"), "peer1")
            self.assertFalse(bool(kwargs.get("wait")))

    def test_space_artifact_infers_generate_when_action_missing(self) -> None:
        from cccc.ports.mcp import server as mcp_server

        with patch.object(mcp_server, "_resolve_group_id", return_value="g_test"), patch.object(
            mcp_server, "_resolve_caller_from_by", return_value="peer1"
        ), patch.object(mcp_server, "space_artifact", return_value={"ok": True}) as mock_space_artifact:
            mcp_server.handle_tool_call(
                "cccc_space",
                {
                    "action": "artifact",
                    "kind": "study_guide",
                    "save_to_space": "true",
                    "source": "/tmp/notes.md",
                },
            )
            kwargs = mock_space_artifact.call_args.kwargs
            self.assertEqual(kwargs.get("action"), "generate")
            options = kwargs.get("options") if isinstance(kwargs.get("options"), dict) else {}
            self.assertEqual(str(options.get("source") or ""), "/tmp/notes.md")

    def test_space_artifact_top_level_language_is_mapped_into_options(self) -> None:
        from cccc.ports.mcp import server as mcp_server

        with patch.object(mcp_server, "_resolve_group_id", return_value="g_test"), patch.object(
            mcp_server, "_resolve_caller_from_by", return_value="peer1"
        ), patch.object(mcp_server, "space_artifact", return_value={"ok": True}) as mock_space_artifact:
            mcp_server.handle_tool_call(
                "cccc_space",
                {
                    "action": "artifact",
                    "kind": "report",
                    "language": "zh-CN",
                    "source": "/tmp/notes.md",
                },
            )
            kwargs = mock_space_artifact.call_args.kwargs
            options = kwargs.get("options") if isinstance(kwargs.get("options"), dict) else {}
            self.assertEqual(str(options.get("language") or ""), "zh-CN")

    def test_space_artifact_language_infers_from_cjk_source_file(self) -> None:
        from cccc.ports.mcp import server as mcp_server

        with tempfile.TemporaryDirectory() as td:
            src = os.path.join(td, "zh_notes.md")
            with open(src, "w", encoding="utf-8") as f:
                f.write("这是中文内容\n用于测试语言推断。\n")
            with patch.object(mcp_server, "_resolve_group_id", return_value="g_test"), patch.object(
                mcp_server, "_resolve_caller_from_by", return_value="peer1"
            ), patch.object(mcp_server, "space_artifact", return_value={"ok": True}) as mock_space_artifact:
                mcp_server.handle_tool_call(
                    "cccc_space",
                    {
                        "action": "artifact",
                        "kind": "report",
                        "source": src,
                    },
                )
                kwargs = mock_space_artifact.call_args.kwargs
                options = kwargs.get("options") if isinstance(kwargs.get("options"), dict) else {}
                self.assertEqual(str(options.get("language") or ""), "zh-CN")

    def test_space_ingest_top_level_fields_auto_pack_payload(self) -> None:
        from cccc.ports.mcp import server as mcp_server

        with patch.object(mcp_server, "_resolve_group_id", return_value="g_test"), patch.object(
            mcp_server, "_resolve_caller_from_by", return_value="peer1"
        ), patch.object(mcp_server, "space_ingest", return_value={"ok": True}) as mock_space_ingest:
            mcp_server.handle_tool_call(
                "cccc_space",
                {
                    "action": "ingest",
                    "source_type": "file",
                    "url": "/tmp/spec.md",
                    "title": "Spec",
                },
            )
            kwargs = mock_space_ingest.call_args.kwargs
            self.assertEqual(str(kwargs.get("kind") or ""), "resource_ingest")
            payload = kwargs.get("payload") if isinstance(kwargs.get("payload"), dict) else {}
            self.assertEqual(str(payload.get("source_type") or ""), "file")
            self.assertEqual(str(payload.get("file_path") or ""), "/tmp/spec.md")
            self.assertEqual(str(payload.get("title") or ""), "Spec")

    def test_space_query_source_ids_option_is_normalized(self) -> None:
        from cccc.ports.mcp import server as mcp_server

        with patch.object(mcp_server, "_resolve_group_id", return_value="g_test"), patch.object(
            mcp_server, "space_query", return_value={"ok": True}
        ) as mock_space_query:
            mcp_server.handle_tool_call(
                "cccc_space",
                {
                    "action": "query",
                    "query": "summarize",
                    "options": {"source_ids": [" src_1 ", "src_2"]},
                },
            )
            kwargs = mock_space_query.call_args.kwargs
            options = kwargs.get("options") if isinstance(kwargs.get("options"), dict) else {}
            self.assertEqual(options.get("source_ids"), ["src_1", "src_2"])

    def test_space_query_rejects_top_level_language(self) -> None:
        from cccc.ports.mcp import server as mcp_server

        with patch.object(mcp_server, "_resolve_group_id", return_value="g_test"):
            with self.assertRaises(mcp_server.MCPError) as cm:
                mcp_server.handle_tool_call(
                    "cccc_space",
                    {
                        "action": "query",
                        "query": "summarize",
                        "language": "zh-CN",
                    },
                )
        self.assertEqual(cm.exception.code, "invalid_request")
        self.assertIn("language/lang", str(cm.exception.message))

    def test_space_query_rejects_unsupported_options(self) -> None:
        from cccc.ports.mcp import server as mcp_server

        with patch.object(mcp_server, "_resolve_group_id", return_value="g_test"):
            with self.assertRaises(mcp_server.MCPError) as cm:
                mcp_server.handle_tool_call(
                    "cccc_space",
                    {
                        "action": "query",
                        "query": "summarize",
                        "options": {"top_k": 5},
                    },
                )
        self.assertEqual(cm.exception.code, "invalid_request")
        self.assertIn("unsupported options", str(cm.exception.message))

    def test_space_artifact_wait_true_uses_extended_daemon_timeout(self) -> None:
        from cccc.ports.mcp import server as mcp_server
        from cccc.ports.mcp.handlers import cccc_space

        captured = {}

        def _fake_daemon(req, *, timeout_s=60.0):
            captured["req"] = req
            captured["timeout_s"] = float(timeout_s)
            return {"ok": True, "status": "completed"}

        with patch.object(cccc_space, "_call_daemon_or_raise", side_effect=_fake_daemon):
            mcp_server.space_artifact(
                group_id="g_test",
                by="peer1",
                action="generate",
                kind="slide_deck",
                wait=True,
                timeout_seconds=120.0,
            )
        self.assertGreaterEqual(float(captured.get("timeout_s") or 0.0), 150.0)

    def test_space_artifact_audio_forces_async_even_if_wait_true(self) -> None:
        from cccc.ports.mcp import server as mcp_server
        from cccc.ports.mcp.handlers import cccc_space

        captured = {}

        def _fake_daemon(req, *, timeout_s=60.0):
            captured["req"] = req
            captured["timeout_s"] = float(timeout_s)
            return {"ok": True, "status": "accepted"}

        with patch.object(cccc_space, "_call_daemon_or_raise", side_effect=_fake_daemon):
            mcp_server.space_artifact(
                group_id="g_test",
                by="peer1",
                action="generate",
                kind="audio",
                wait=True,
                timeout_seconds=120.0,
            )
        req = captured.get("req") if isinstance(captured.get("req"), dict) else {}
        args = req.get("args") if isinstance(req.get("args"), dict) else {}
        self.assertFalse(bool(args.get("wait")))
        self.assertGreaterEqual(float(captured.get("timeout_s") or 0.0), 120.0)

    def test_memory_write_routes_to_daemon(self) -> None:
        from cccc.ports.mcp import server as mcp_server

        captured = {}

        def _fake_call(req):
            captured["req"] = req
            return {"ok": True, "status": "written"}

        with patch.object(mcp_server, "_resolve_group_id", return_value="g_test"), patch.object(
            mcp_server, "_call_daemon_or_raise", side_effect=_fake_call
        ):
            mcp_server.handle_tool_call(
                "cccc_memory",
                {"action": "write", "target": "daily", "date": "2026-03-03", "content": "x"},
            )

        req = captured.get("req") if isinstance(captured.get("req"), dict) else {}
        self.assertEqual(req.get("op"), "memory_reme_write")
        args = req.get("args") if isinstance(req.get("args"), dict) else {}
        self.assertEqual(args.get("group_id"), "g_test")
        self.assertEqual(args.get("target"), "daily")
        self.assertEqual(args.get("date"), "2026-03-03")

    def test_memory_get_missing_path_raises_validation_error(self) -> None:
        from cccc.ports.mcp import server as mcp_server

        with patch.object(mcp_server, "_resolve_group_id", return_value="g_test"):
            with self.assertRaises(mcp_server.MCPError) as cm:
                mcp_server.handle_tool_call("cccc_memory", {"action": "get"})
        self.assertEqual(cm.exception.code, "validation_error")

    def test_memory_index_sync_routes_to_daemon(self) -> None:
        from cccc.ports.mcp import server as mcp_server

        captured = {}

        def _fake_call(req):
            captured["req"] = req
            return {"ok": True, "indexed_files": 2}

        with patch.object(mcp_server, "_resolve_group_id", return_value="g_test"), patch.object(
            mcp_server, "_call_daemon_or_raise", side_effect=_fake_call
        ):
            mcp_server.handle_tool_call("cccc_memory_admin", {"action": "index_sync", "mode": "rebuild"})
        req = captured.get("req") if isinstance(captured.get("req"), dict) else {}
        self.assertEqual(req.get("op"), "memory_reme_index_sync")
        args = req.get("args") if isinstance(req.get("args"), dict) else {}
        self.assertEqual(args.get("group_id"), "g_test")
        self.assertEqual(args.get("mode"), "rebuild")

    def test_memory_context_check_routes_to_daemon(self) -> None:
        from cccc.ports.mcp import server as mcp_server

        captured = {}

        def _fake_call(req):
            captured["req"] = req
            return {"ok": True, "needs_compaction": False}

        with patch.object(mcp_server, "_resolve_group_id", return_value="g_test"), patch.object(
            mcp_server, "_call_daemon_or_raise", side_effect=_fake_call
        ):
            mcp_server.handle_tool_call(
                "cccc_memory_admin",
                {
                    "action": "context_check",
                    "messages": [{"role": "user", "content": "hello"}],
                    "keep_recent_tokens": 2048,
                },
            )
        req = captured.get("req") if isinstance(captured.get("req"), dict) else {}
        self.assertEqual(req.get("op"), "memory_reme_context_check")
        args = req.get("args") if isinstance(req.get("args"), dict) else {}
        self.assertEqual(args.get("group_id"), "g_test")
        messages = args.get("messages") if isinstance(args.get("messages"), list) else []
        self.assertEqual(len(messages), 1)

    def test_memory_daily_flush_coerces_return_prompt_bool(self) -> None:
        from cccc.ports.mcp import server as mcp_server

        captured = {}

        def _fake_call(req):
            captured["req"] = req
            return {"ok": True, "status": "silent"}

        with patch.object(mcp_server, "_resolve_group_id", return_value="g_test"), patch.object(
            mcp_server, "_call_daemon_or_raise", side_effect=_fake_call
        ):
            mcp_server.handle_tool_call(
                "cccc_memory_admin",
                {"action": "daily_flush", "messages": [{"role": "user", "content": "h"}], "return_prompt": "false"},
            )
        req = captured.get("req") if isinstance(captured.get("req"), dict) else {}
        self.assertEqual(req.get("op"), "memory_reme_daily_flush")
        args = req.get("args") if isinstance(req.get("args"), dict) else {}
        self.assertFalse(bool(args.get("return_prompt")))

if __name__ == "__main__":
    unittest.main()
