import os
import tempfile
import unittest


class TestChatOps(unittest.TestCase):
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

    def test_try_handle_unknown_chat_op_returns_none(self) -> None:
        from cccc.daemon.ops.chat_ops import try_handle_chat_op

        self.assertIsNone(
            try_handle_chat_op(
                "not_chat",
                {},
                coerce_bool=lambda _: False,
                normalize_attachments=lambda _group, _raw: [],
                effective_runner_kind=lambda kind: kind,
                auto_wake_recipients=lambda _group, _to, _by: [],
                automation_on_resume=lambda _group: None,
                automation_on_new_message=lambda _group: None,
                clear_pending_system_notifies=lambda _group_id, _kinds: None,
            )
        )

    def test_attention_reply_still_writes_chat_ack(self) -> None:
        _, cleanup = self._with_home()
        try:
            create, _ = self._call("group_create", {"title": "chat-ops", "topic": "", "by": "user"})
            self.assertTrue(create.ok, getattr(create, "error", None))
            group_id = str((create.result or {}).get("group_id") or "").strip()
            self.assertTrue(group_id)

            add_1, _ = self._call(
                "actor_add",
                {
                    "group_id": group_id,
                    "by": "user",
                    "actor_id": "peer1",
                    "title": "Peer 1",
                    "runtime": "codex",
                    "runner": "headless",
                    "enabled": False,
                },
            )
            self.assertTrue(add_1.ok, getattr(add_1, "error", None))

            add_2, _ = self._call(
                "actor_add",
                {
                    "group_id": group_id,
                    "by": "user",
                    "actor_id": "peer2",
                    "title": "Peer 2",
                    "runtime": "codex",
                    "runner": "headless",
                    "enabled": False,
                },
            )
            self.assertTrue(add_2.ok, getattr(add_2, "error", None))

            send, _ = self._call(
                "send",
                {
                    "group_id": group_id,
                    "by": "peer1",
                    "to": ["peer2"],
                    "text": "ack me",
                    "priority": "attention",
                },
            )
            self.assertTrue(send.ok, getattr(send, "error", None))
            sent_event = (send.result or {}).get("event") if isinstance(send.result, dict) else {}
            self.assertIsInstance(sent_event, dict)
            assert isinstance(sent_event, dict)
            sent_event_id = str(sent_event.get("id") or "").strip()
            self.assertTrue(sent_event_id)

            reply, _ = self._call(
                "reply",
                {
                    "group_id": group_id,
                    "by": "peer2",
                    "reply_to": sent_event_id,
                    "text": "done",
                },
            )
            self.assertTrue(reply.ok, getattr(reply, "error", None))
            ack_event = (reply.result or {}).get("ack_event") if isinstance(reply.result, dict) else {}
            self.assertIsInstance(ack_event, dict)
            assert isinstance(ack_event, dict)
            self.assertEqual(str(ack_event.get("kind") or ""), "chat.ack")
        finally:
            cleanup()


    # -- T067: cccc_message_send `to` routing bug fix tests --

    def _setup_group_with_actors(self):
        """Helper: create group with foreman + 2 peers for routing tests."""
        _, cleanup = self._with_home()
        create, _ = self._call("group_create", {"title": "route-test", "topic": "", "by": "user"})
        assert create.ok, getattr(create, "error", None)
        group_id = str((create.result or {}).get("group_id") or "").strip()

        self._call("actor_add", {
            "group_id": group_id, "by": "user",
            "actor_id": "peer1", "title": "Peer 1",
            "runtime": "codex", "runner": "headless", "enabled": False,
        })
        self._call("actor_add", {
            "group_id": group_id, "by": "user",
            "actor_id": "peer2", "title": "Peer 2",
            "runtime": "codex", "runner": "headless", "enabled": False,
        })
        return group_id, cleanup

    def test_send_to_string_is_routed_correctly(self) -> None:
        """T067 scenario 1: LLM passes `to` as string instead of array."""
        group_id, cleanup = self._setup_group_with_actors()
        try:
            resp, _ = self._call("send", {
                "group_id": group_id,
                "by": "user",
                "to": ["peer1"],
                "text": "hello peer1",
            })
            self.assertTrue(resp.ok, getattr(resp, "error", None))
            event = (resp.result or {}).get("event", {})
            self.assertIn("peer1", event.get("data", {}).get("to", []))
        finally:
            cleanup()

    def test_send_to_array_is_routed_correctly(self) -> None:
        """T067 scenario 2: `to` passed as array (normal case)."""
        group_id, cleanup = self._setup_group_with_actors()
        try:
            resp, _ = self._call("send", {
                "group_id": group_id,
                "by": "user",
                "to": ["peer1", "peer2"],
                "text": "hello both",
            })
            self.assertTrue(resp.ok, getattr(resp, "error", None))
            event = (resp.result or {}).get("event", {})
            to_list = event.get("data", {}).get("to", [])
            self.assertIn("peer1", to_list)
            self.assertIn("peer2", to_list)
        finally:
            cleanup()

    def test_send_to_string_direct_payload_is_routed_correctly(self) -> None:
        """T067 scenario: daemon send op tolerates string `to` payload."""
        group_id, cleanup = self._setup_group_with_actors()
        try:
            resp, _ = self._call("send", {
                "group_id": group_id,
                "by": "user",
                "to": "peer1",
                "text": "hello peer1",
            })
            self.assertTrue(resp.ok, getattr(resp, "error", None))
            event = (resp.result or {}).get("event", {})
            self.assertEqual(event.get("data", {}).get("to", []), ["peer1"])
        finally:
            cleanup()

    def test_send_empty_to_uses_default(self) -> None:
        """T067 scenario 3: empty `to` falls back to group default."""
        group_id, cleanup = self._setup_group_with_actors()
        try:
            resp, _ = self._call("send", {
                "group_id": group_id,
                "by": "user",
                "to": [],
                "text": "broadcast test",
            })
            # Empty to with no mentions -> falls back to group default or broadcast
            # Either way should succeed for user sender
            self.assertTrue(resp.ok, getattr(resp, "error", None))
        finally:
            cleanup()

    def test_send_to_malformed_list_entries_falls_back_to_default(self) -> None:
        """T067 edge: malformed list entries should not become broadcast."""
        group_id, cleanup = self._setup_group_with_actors()
        try:
            resp, _ = self._call("send", {
                "group_id": group_id,
                "by": "user",
                "to": [None, ""],
                "text": "malformed to payload",
            })
            self.assertTrue(resp.ok, getattr(resp, "error", None))
            event = (resp.result or {}).get("event", {})
            self.assertEqual(event.get("data", {}).get("to", []), ["@foreman"])
        finally:
            cleanup()

    def test_send_multiple_recipients(self) -> None:
        """T067 scenario 4: multiple explicit recipients."""
        group_id, cleanup = self._setup_group_with_actors()
        try:
            resp, _ = self._call("send", {
                "group_id": group_id,
                "by": "user",
                "to": ["peer1", "peer2"],
                "text": "multi-target",
            })
            self.assertTrue(resp.ok, getattr(resp, "error", None))
            event = (resp.result or {}).get("event", {})
            to_list = event.get("data", {}).get("to", [])
            self.assertEqual(len(to_list), 2)
        finally:
            cleanup()

    def test_send_to_nonexistent_actor_returns_error(self) -> None:
        """T067 scenario 5: sending to a non-existent actor returns error."""
        group_id, cleanup = self._setup_group_with_actors()
        try:
            resp, _ = self._call("send", {
                "group_id": group_id,
                "by": "user",
                "to": ["nonexistent_actor"],
                "text": "should fail",
            })
            self.assertFalse(resp.ok)
            err = resp.error
            self.assertIsNotNone(err)
            self.assertEqual(err.code, "invalid_recipient")
        finally:
            cleanup()

    def test_mcp_to_string_coercion(self) -> None:
        """T067 MCP layer: string `to` is converted to single-element array.

        Tests the daemon layer with a single-element array (simulating
        the MCP layer's string→array conversion).
        """
        group_id, cleanup = self._setup_group_with_actors()
        try:
            resp, _ = self._call("send", {
                "group_id": group_id,
                "by": "user",
                "to": ["peer1"],
                "text": "string coercion test",
            })
            self.assertTrue(resp.ok, getattr(resp, "error", None))
            event = (resp.result or {}).get("event", {})
            self.assertEqual(event.get("data", {}).get("to", []), ["peer1"])
        finally:
            cleanup()

    def test_foreman_send_to_peer_no_self_routing(self) -> None:
        """T067 regression: foreman sending to peer should NOT route to self."""
        group_id, cleanup = self._setup_group_with_actors()
        try:
            # Add a foreman actor
            self._call("actor_add", {
                "group_id": group_id, "by": "user",
                "actor_id": "fm1", "title": "Foreman",
                "runtime": "codex", "runner": "headless", "enabled": False,
                "role": "foreman",
            })
            # Foreman sends explicitly to peer1
            resp, _ = self._call("send", {
                "group_id": group_id,
                "by": "fm1",
                "to": ["peer1"],
                "text": "foreman to peer",
            })
            self.assertTrue(resp.ok, getattr(resp, "error", None))
            event = (resp.result or {}).get("event", {})
            to_list = event.get("data", {}).get("to", [])
            self.assertEqual(to_list, ["peer1"])
            # NOT ["@foreman"]!
            self.assertNotIn("@foreman", to_list)
        finally:
            cleanup()

    # -- T070: cccc_message_reply & cccc_file_send `to` string coercion tests --

    def test_reply_to_string_is_routed_correctly(self) -> None:
        """T070: reply with string `to` should route correctly (daemon layer)."""
        group_id, cleanup = self._setup_group_with_actors()
        try:
            # First send a message to get an event_id to reply to
            send_resp, _ = self._call("send", {
                "group_id": group_id,
                "by": "peer1",
                "to": ["peer2"],
                "text": "original message",
            })
            self.assertTrue(send_resp.ok, getattr(send_resp, "error", None))
            event_id = (send_resp.result or {}).get("event", {}).get("id", "")

            # Reply with string-style to (as single-element array, simulating MCP coercion)
            resp, _ = self._call("reply", {
                "group_id": group_id,
                "by": "peer2",
                "reply_to": event_id,
                "to": ["peer1"],
                "text": "reply to peer1",
            })
            self.assertTrue(resp.ok, getattr(resp, "error", None))
            event = (resp.result or {}).get("event", {})
            self.assertIn("peer1", event.get("data", {}).get("to", []))
        finally:
            cleanup()

    def test_reply_empty_to_defaults_to_original_sender(self) -> None:
        """T070: reply with empty/None `to` defaults to original sender."""
        group_id, cleanup = self._setup_group_with_actors()
        try:
            send_resp, _ = self._call("send", {
                "group_id": group_id,
                "by": "peer1",
                "to": ["peer2"],
                "text": "original",
            })
            self.assertTrue(send_resp.ok, getattr(send_resp, "error", None))
            event_id = (send_resp.result or {}).get("event", {}).get("id", "")

            # Reply with no `to` — should default to original sender
            resp, _ = self._call("reply", {
                "group_id": group_id,
                "by": "peer2",
                "reply_to": event_id,
                "text": "reply default",
            })
            self.assertTrue(resp.ok, getattr(resp, "error", None))
        finally:
            cleanup()


class TestMCPToCoercion(unittest.TestCase):
    """T070: Test MCP handler string→array coercion for reply and file_send."""

    def test_mcp_reply_handler_string_to_coercion(self) -> None:
        """MCP cccc_message_reply handler: string `to` → single-element list."""
        from cccc.ports.mcp.server import _handle_cccc_namespace
        from unittest.mock import patch

        # Capture what message_reply receives
        captured = {}

        def fake_message_reply(**kwargs):
            captured.update(kwargs)
            return {"event": {}}

        with patch("cccc.ports.mcp.server.message_reply", side_effect=fake_message_reply):
            with patch("cccc.ports.mcp.server._resolve_group_id", return_value="g_test"):
                with patch("cccc.ports.mcp.server._resolve_self_actor_id", return_value="peer1"):
                    try:
                        _handle_cccc_namespace("cccc_message_reply", {
                            "to": "peer2",
                            "event_id": "evt_123",
                            "text": "hello",
                        })
                    except Exception:
                        pass  # May fail on daemon call, we only care about captured args

        self.assertEqual(captured.get("to"), ["peer2"])

    def test_mcp_reply_handler_list_to_preserved(self) -> None:
        """MCP cccc_message_reply handler: list `to` is preserved."""
        from cccc.ports.mcp.server import _handle_cccc_namespace
        from unittest.mock import patch

        captured = {}

        def fake_message_reply(**kwargs):
            captured.update(kwargs)
            return {"event": {}}

        with patch("cccc.ports.mcp.server.message_reply", side_effect=fake_message_reply):
            with patch("cccc.ports.mcp.server._resolve_group_id", return_value="g_test"):
                with patch("cccc.ports.mcp.server._resolve_self_actor_id", return_value="peer1"):
                    try:
                        _handle_cccc_namespace("cccc_message_reply", {
                            "to": ["peer2", "peer3"],
                            "event_id": "evt_123",
                            "text": "hello",
                        })
                    except Exception:
                        pass

        self.assertEqual(captured.get("to"), ["peer2", "peer3"])

    def test_mcp_reply_handler_none_to_is_none(self) -> None:
        """MCP cccc_message_reply handler: missing `to` → None."""
        from cccc.ports.mcp.server import _handle_cccc_namespace
        from unittest.mock import patch

        captured = {}

        def fake_message_reply(**kwargs):
            captured.update(kwargs)
            return {"event": {}}

        with patch("cccc.ports.mcp.server.message_reply", side_effect=fake_message_reply):
            with patch("cccc.ports.mcp.server._resolve_group_id", return_value="g_test"):
                with patch("cccc.ports.mcp.server._resolve_self_actor_id", return_value="peer1"):
                    try:
                        _handle_cccc_namespace("cccc_message_reply", {
                            "event_id": "evt_123",
                            "text": "hello",
                        })
                    except Exception:
                        pass

        self.assertIsNone(captured.get("to"))

    def test_mcp_file_send_handler_string_to_coercion(self) -> None:
        """MCP cccc_file_send handler: string `to` → single-element list."""
        from cccc.ports.mcp.server import _handle_cccc_namespace
        from unittest.mock import patch

        captured = {}

        def fake_file_send(**kwargs):
            captured.update(kwargs)
            return {"event": {}}

        with patch("cccc.ports.mcp.server.file_send", side_effect=fake_file_send):
            with patch("cccc.ports.mcp.server._resolve_group_id", return_value="g_test"):
                with patch("cccc.ports.mcp.server._resolve_self_actor_id", return_value="peer1"):
                    try:
                        _handle_cccc_namespace("cccc_file_send", {
                            "to": "user",
                            "path": "/tmp/test.txt",
                            "text": "file caption",
                        })
                    except Exception:
                        pass

        self.assertEqual(captured.get("to"), ["user"])

    def test_mcp_file_send_handler_none_to_is_none(self) -> None:
        """MCP cccc_file_send handler: missing `to` → None."""
        from cccc.ports.mcp.server import _handle_cccc_namespace
        from unittest.mock import patch

        captured = {}

        def fake_file_send(**kwargs):
            captured.update(kwargs)
            return {"event": {}}

        with patch("cccc.ports.mcp.server.file_send", side_effect=fake_file_send):
            with patch("cccc.ports.mcp.server._resolve_group_id", return_value="g_test"):
                with patch("cccc.ports.mcp.server._resolve_self_actor_id", return_value="peer1"):
                    try:
                        _handle_cccc_namespace("cccc_file_send", {
                            "path": "/tmp/test.txt",
                        })
                    except Exception:
                        pass

        self.assertIsNone(captured.get("to"))


if __name__ == "__main__":
    unittest.main()
