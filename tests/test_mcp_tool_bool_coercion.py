import os
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch


class TestMcpToolBoolCoercion(unittest.TestCase):
    def test_headless_codex_message_send_is_allowed(self) -> None:
        from cccc.ports.mcp.handlers import cccc_messaging

        class _FakeGroup:
            pass

        with patch.object(cccc_messaging, "load_group", return_value=_FakeGroup()), patch.object(
            cccc_messaging, "find_actor", return_value={"id": "peer1", "runtime": "codex", "runner": "headless"}
        ), patch.object(cccc_messaging, "_call_daemon_or_raise", return_value={"ok": True, "kind": "chat.message"}) as call_daemon:
            result = cccc_messaging.message_send(
                group_id="g_test",
                actor_id="peer1",
                text="hello",
                to=["user"],
            )
        self.assertEqual(result.get("kind"), "chat.message")
        self.assertEqual(call_daemon.call_args.args[0]["op"], "send")

    def test_headless_codex_message_reply_is_allowed(self) -> None:
        from cccc.ports.mcp.handlers import cccc_messaging

        class _FakeGroup:
            pass

        with patch.object(cccc_messaging, "load_group", return_value=_FakeGroup()), patch.object(
            cccc_messaging, "find_actor", return_value={"id": "peer1", "runtime": "codex", "runner": "headless"}
        ), patch.object(cccc_messaging, "_call_daemon_or_raise", return_value={"ok": True, "kind": "chat.message"}) as call_daemon:
            result = cccc_messaging.message_reply(
                group_id="g_test",
                actor_id="peer1",
                reply_to="ev_1",
                text="hello",
                to=["user"],
            )
        self.assertEqual(result.get("kind"), "chat.message")
        self.assertEqual(call_daemon.call_args.args[0]["op"], "reply")

    def test_headless_claude_message_send_is_allowed(self) -> None:
        from cccc.ports.mcp.handlers import cccc_messaging

        class _FakeGroup:
            pass

        with patch.object(cccc_messaging, "load_group", return_value=_FakeGroup()), patch.object(
            cccc_messaging, "find_actor", return_value={"id": "peer1", "runtime": "claude", "runner": "headless"}
        ), patch.object(cccc_messaging, "_call_daemon_or_raise", return_value={"ok": True, "kind": "chat.message"}) as call_daemon:
            result = cccc_messaging.message_send(
                group_id="g_test",
                actor_id="peer1",
                text="hello",
                to=["user"],
            )
        self.assertEqual(result.get("kind"), "chat.message")
        self.assertEqual(call_daemon.call_args.args[0]["op"], "send")

    def test_headless_claude_message_reply_is_allowed(self) -> None:
        from cccc.ports.mcp.handlers import cccc_messaging

        class _FakeGroup:
            pass

        with patch.object(cccc_messaging, "load_group", return_value=_FakeGroup()), patch.object(
            cccc_messaging, "find_actor", return_value={"id": "peer1", "runtime": "claude", "runner": "headless"}
        ), patch.object(cccc_messaging, "_call_daemon_or_raise", return_value={"ok": True, "kind": "chat.message"}) as call_daemon:
            result = cccc_messaging.message_reply(
                group_id="g_test",
                actor_id="peer1",
                reply_to="ev_1",
                text="hello",
                to=["user"],
            )
        self.assertEqual(result.get("kind"), "chat.message")
        self.assertEqual(call_daemon.call_args.args[0]["op"], "reply")

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

    def test_blob_read_reads_blob_attachment_with_limit(self) -> None:
        from cccc.kernel.blobs import store_blob_bytes
        from cccc.ports.mcp.handlers import cccc_messaging

        class _FakeGroup:
            def __init__(self, root: str) -> None:
                self.group_id = "g_test"
                self.path = Path(root)
                self.doc = {}

        with tempfile.TemporaryDirectory() as td:
            group = _FakeGroup(td)
            att = store_blob_bytes(group, data="hello world".encode("utf-8"), filename="note.txt", mime_type="text/plain")

            with patch.object(cccc_messaging, "load_group", return_value=group):
                out = cccc_messaging.blob_read(group_id="g_test", rel_path=str(att.get("path")), max_bytes=5)

            self.assertEqual(out.get("text"), "hello")
            self.assertTrue(out.get("truncated"))
            self.assertEqual(out.get("bytes"), 11)
            self.assertTrue(str(out.get("path") or "").endswith("note.txt"))

    def test_repo_edit_requires_web_model_actor_even_when_called_directly(self) -> None:
        from cccc.ports.mcp import server as mcp_server

        class _FakeGroup:
            pass

        with patch.object(mcp_server, "_resolve_group_id", return_value="g_test"), patch.object(
            mcp_server, "_resolve_self_actor_id", return_value="peer1"
        ), patch.object(mcp_server, "load_group", return_value=_FakeGroup()), patch.object(
            mcp_server, "find_actor", return_value={"id": "peer1", "runtime": "codex", "runner": "headless"}
        ), patch.object(
            mcp_server, "repo_tool", return_value={"ok": True}
        ) as mock_repo_tool:
            with self.assertRaises(mcp_server.MCPError) as cm:
                mcp_server.handle_tool_call(
                    "cccc_repo_edit",
                    {"action": "write", "path": "notes.txt", "content": "blocked"},
                )

        self.assertEqual(cm.exception.code, "invalid_actor_runtime")
        mock_repo_tool.assert_not_called()

    def test_repo_edit_allows_web_model_actor(self) -> None:
        from cccc.ports.mcp import server as mcp_server

        class _FakeGroup:
            pass

        with patch.object(mcp_server, "_resolve_group_id", return_value="g_test"), patch.object(
            mcp_server, "_resolve_self_actor_id", return_value="peer1"
        ), patch.object(mcp_server, "load_group", return_value=_FakeGroup()), patch.object(
            mcp_server, "find_actor", return_value={"id": "peer1", "runtime": "web_model", "runner": "headless"}
        ), patch.object(
            mcp_server, "repo_tool", return_value={"ok": True}
        ) as mock_repo_tool:
            result = mcp_server.handle_tool_call(
                "cccc_repo_edit",
                {"action": "write", "path": "notes.txt", "content": "ok"},
            )

        self.assertEqual(result.get("ok"), True)
        mock_repo_tool.assert_called_once()

    def test_message_send_normalizes_double_escaped_newlines(self) -> None:
        from cccc.ports.mcp import server as mcp_server
        from cccc.ports.mcp.handlers import cccc_messaging

        captured = {}

        def _fake_call(req):
            captured["req"] = req
            return {"ok": True, "event_id": "ev_test"}

        class _FakeGroup:
            pass

        with patch.object(cccc_messaging, "_call_daemon_or_raise", side_effect=_fake_call), patch.object(
            cccc_messaging, "load_group", return_value=_FakeGroup()
        ), patch.object(
            cccc_messaging, "find_actor", return_value={"id": "peer1", "runtime": "codex"}
        ):
            mcp_server.message_send(
                group_id="g_test",
                actor_id="peer1",
                text="line1\\nline2\\tindent",
                to=["user"],
            )

        req = captured.get("req") if isinstance(captured.get("req"), dict) else {}
        args = req.get("args") if isinstance(req.get("args"), dict) else {}
        self.assertEqual(args.get("text"), "line1\nline2\tindent")

    def test_message_reply_keeps_normal_newlines_idempotent(self) -> None:
        from cccc.ports.mcp import server as mcp_server
        from cccc.ports.mcp.handlers import cccc_messaging

        captured = {}

        def _fake_call(req):
            captured["req"] = req
            return {"ok": True, "event_id": "ev_test"}

        class _FakeGroup:
            pass

        with patch.object(cccc_messaging, "_call_daemon_or_raise", side_effect=_fake_call), patch.object(
            cccc_messaging, "load_group", return_value=_FakeGroup()
        ), patch.object(
            cccc_messaging, "find_actor", return_value={"id": "peer1", "runtime": "claude"}
        ):
            mcp_server.message_reply(
                group_id="g_test",
                actor_id="peer1",
                reply_to="ev_1",
                text="line1\nline2",
                to=["user"],
            )

        req = captured.get("req") if isinstance(captured.get("req"), dict) else {}
        args = req.get("args") if isinstance(req.get("args"), dict) else {}
        self.assertEqual(args.get("text"), "line1\nline2")

    def test_message_send_keeps_windows_path_for_non_codex_runtime(self) -> None:
        from cccc.ports.mcp import server as mcp_server
        from cccc.ports.mcp.handlers import cccc_messaging

        captured = {}

        def _fake_call(req):
            captured["req"] = req
            return {"ok": True, "event_id": "ev_test"}

        class _FakeGroup:
            pass

        with patch.object(cccc_messaging, "_call_daemon_or_raise", side_effect=_fake_call), patch.object(
            cccc_messaging, "load_group", return_value=_FakeGroup()
        ), patch.object(
            cccc_messaging, "find_actor", return_value={"id": "peer1", "runtime": "claude"}
        ):
            mcp_server.message_send(
                group_id="g_test",
                actor_id="peer1",
                text=r"C:\\temp\\new",
                to=["user"],
            )

        req = captured.get("req") if isinstance(captured.get("req"), dict) else {}
        args = req.get("args") if isinstance(req.get("args"), dict) else {}
        self.assertEqual(args.get("text"), r"C:\\temp\\new")

    def test_message_send_keeps_literal_backslash_n_for_codex_runtime(self) -> None:
        from cccc.ports.mcp import server as mcp_server
        from cccc.ports.mcp.handlers import cccc_messaging

        captured = {}

        def _fake_call(req):
            captured["req"] = req
            return {"ok": True, "event_id": "ev_test"}

        class _FakeGroup:
            pass

        with patch.object(cccc_messaging, "_call_daemon_or_raise", side_effect=_fake_call), patch.object(
            cccc_messaging, "load_group", return_value=_FakeGroup()
        ), patch.object(
            cccc_messaging, "find_actor", return_value={"id": "peer1", "runtime": "codex"}
        ):
            mcp_server.message_send(
                group_id="g_test",
                actor_id="peer1",
                text=r"literal \\n path C:\\temp\\new",
                to=["user"],
            )

        req = captured.get("req") if isinstance(captured.get("req"), dict) else {}
        args = req.get("args") if isinstance(req.get("args"), dict) else {}
        self.assertEqual(args.get("text"), r"literal \\n path C:\\temp\\new")

    def test_message_reply_keeps_literal_backslash_t_for_codex_runtime(self) -> None:
        from cccc.ports.mcp import server as mcp_server
        from cccc.ports.mcp.handlers import cccc_messaging

        captured = {}

        def _fake_call(req):
            captured["req"] = req
            return {"ok": True, "event_id": "ev_test"}

        class _FakeGroup:
            pass

        with patch.object(cccc_messaging, "_call_daemon_or_raise", side_effect=_fake_call), patch.object(
            cccc_messaging, "load_group", return_value=_FakeGroup()
        ), patch.object(
            cccc_messaging, "find_actor", return_value={"id": "peer1", "runtime": "codex"}
        ):
            mcp_server.message_reply(
                group_id="g_test",
                actor_id="peer1",
                reply_to="ev_1",
                text=r"regex \\t token",
                to=["user"],
            )

        req = captured.get("req") if isinstance(captured.get("req"), dict) else {}
        args = req.get("args") if isinstance(req.get("args"), dict) else {}
        self.assertEqual(args.get("text"), r"regex \\t token")


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
                    "lane": "work",
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
                    "lane": "work",
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
                    "lane": "work",
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
                        "lane": "work",
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
                    "lane": "work",
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
                    "lane": "work",
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
                        "lane": "work",
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
                        "lane": "work",
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
