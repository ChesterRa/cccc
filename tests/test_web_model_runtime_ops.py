import json
import os
import shlex
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch


class TestWebModelRuntimeOps(unittest.TestCase):
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

    def _create_group_with_actor(self):
        from cccc.kernel.actors import add_actor
        from cccc.kernel.group import create_group
        from cccc.kernel.registry import load_registry
        from cccc.daemon.runner_state_ops import write_headless_state

        reg = load_registry()
        group = create_group(reg, title="web-model-runtime", topic="")
        add_actor(group, actor_id="peer1", title="Web Model", runtime="web_model", runner="headless")
        write_headless_state(group.group_id, "peer1")
        return group

    def _bind_chatgpt_conversation(self, group, actor_id: str = "peer1", url: str = "https://chatgpt.com/c/test-chat") -> None:
        from cccc.ports.web_model_browser_sidecar import record_chatgpt_browser_state

        record_chatgpt_browser_state(group.group_id, actor_id, {"conversation_url": url})

    def _create_group_with_codex_actor(self):
        from cccc.kernel.actors import add_actor
        from cccc.kernel.group import create_group
        from cccc.kernel.registry import load_registry

        reg = load_registry()
        group = create_group(reg, title="web-model-runtime-invalid", topic="")
        add_actor(group, actor_id="peer1", title="Codex", runtime="codex", runner="headless")
        return group

    def test_wait_next_turn_does_not_advance_cursor_until_complete(self) -> None:
        from cccc.daemon.runner_state_ops import read_headless_state
        from cccc.kernel.inbox import get_cursor, has_chat_ack
        from cccc.kernel.ledger import append_event

        _, cleanup = self._with_home()
        try:
            group = self._create_group_with_actor()
            first = append_event(
                group.ledger_path,
                kind="chat.message",
                group_id=group.group_id,
                scope_key="",
                by="user",
                data={"text": "first task", "to": ["peer1"], "priority": "attention", "reply_required": True},
            )
            second = append_event(
                group.ledger_path,
                kind="chat.message",
                group_id=group.group_id,
                scope_key="",
                by="user",
                data={"text": "second task", "to": ["peer1"]},
            )

            wait, should_stop = self._call(
                "web_model_runtime_wait_next_turn",
                {"group_id": group.group_id, "actor_id": "peer1", "limit": 20},
            )
            self.assertFalse(should_stop)
            self.assertTrue(wait.ok, getattr(wait, "error", None))
            turn = ((wait.result or {}).get("turn") or {})
            self.assertEqual((wait.result or {}).get("status"), "work_available")
            self.assertEqual(turn.get("event_ids"), [first["id"], second["id"]])
            self.assertEqual(get_cursor(group, "peer1"), ("", ""))
            self.assertEqual(str(read_headless_state(group.group_id, "peer1").get("status") or ""), "working")

            repeat, _ = self._call(
                "web_model_runtime_wait_next_turn",
                {"group_id": group.group_id, "actor_id": "peer1", "limit": 20},
            )
            self.assertTrue(repeat.ok, getattr(repeat, "error", None))
            self.assertEqual(((repeat.result or {}).get("turn") or {}).get("event_ids"), [first["id"], second["id"]])

            complete, _ = self._call(
                "web_model_runtime_complete_turn",
                {
                    "group_id": group.group_id,
                    "actor_id": "peer1",
                    "by": "peer1",
                    "turn_id": str(turn.get("turn_id") or ""),
                    "event_ids": [first["id"], second["id"]],
                    "status": "done",
                    "summary": "processed",
                },
            )
            self.assertTrue(complete.ok, getattr(complete, "error", None))
            result = complete.result or {}
            self.assertTrue(bool(result.get("cursor_committed")))
            self.assertEqual((result.get("cursor") or {}).get("event_id"), second["id"])
            self.assertEqual(get_cursor(group, "peer1")[0], second["id"])
            self.assertTrue(has_chat_ack(group, event_id=first["id"], actor_id="peer1"))
            self.assertEqual(str(read_headless_state(group.group_id, "peer1").get("status") or ""), "waiting")
        finally:
            cleanup()

    def test_actor_list_reports_web_model_messages_queued_after_active_turn(self) -> None:
        from cccc.kernel.ledger import append_event

        _, cleanup = self._with_home()
        try:
            group = self._create_group_with_actor()
            active = append_event(
                group.ledger_path,
                kind="chat.message",
                group_id=group.group_id,
                scope_key="",
                by="user",
                data={"text": "active turn", "to": ["peer1"]},
            )
            wait, _ = self._call(
                "web_model_runtime_wait_next_turn",
                {"group_id": group.group_id, "actor_id": "peer1", "limit": 1},
            )
            self.assertTrue(wait.ok, getattr(wait, "error", None))
            self.assertEqual(((wait.result or {}).get("turn") or {}).get("event_ids"), [active["id"]])
            queued_1 = append_event(
                group.ledger_path,
                kind="chat.message",
                group_id=group.group_id,
                scope_key="",
                by="user",
                data={"text": "queued one", "to": ["peer1"]},
            )
            queued_2 = append_event(
                group.ledger_path,
                kind="chat.message",
                group_id=group.group_id,
                scope_key="",
                by="user",
                data={"text": "queued two", "to": ["peer1"]},
            )

            actors, _ = self._call("actor_list", {"group_id": group.group_id, "include_unread": True})

            self.assertTrue(actors.ok, getattr(actors, "error", None))
            actor = ((actors.result or {}).get("actors") or [])[0]
            self.assertEqual(actor.get("unread_count"), 3)
            self.assertEqual(actor.get("web_model_queued_count"), 2)
            self.assertEqual(actor.get("web_model_queued_after_event_id"), active["id"])
            self.assertEqual(actor.get("web_model_queued_latest_event_id"), queued_2["id"])
            self.assertNotEqual(actor.get("web_model_queued_latest_event_id"), queued_1["id"])
        finally:
            cleanup()

    def test_wait_next_turn_rejects_non_web_model_actor(self) -> None:
        _, cleanup = self._with_home()
        try:
            group = self._create_group_with_codex_actor()
            resp, _ = self._call(
                "web_model_runtime_wait_next_turn",
                {"group_id": group.group_id, "actor_id": "peer1"},
            )
            self.assertFalse(resp.ok)
            self.assertEqual(resp.error.code, "invalid_actor_runtime")
        finally:
            cleanup()

    def test_failed_complete_leaves_turn_unread_for_retry(self) -> None:
        from cccc.kernel.inbox import get_cursor
        from cccc.kernel.ledger import append_event

        _, cleanup = self._with_home()
        try:
            group = self._create_group_with_actor()
            self._bind_chatgpt_conversation(group)
            event = append_event(
                group.ledger_path,
                kind="chat.message",
                group_id=group.group_id,
                scope_key="",
                by="user",
                data={"text": "retry me", "to": ["peer1"]},
            )

            complete, _ = self._call(
                "web_model_runtime_complete_turn",
                {
                    "group_id": group.group_id,
                    "actor_id": "peer1",
                    "by": "peer1",
                    "event_ids": [event["id"]],
                    "status": "failed",
                },
            )
            self.assertTrue(complete.ok, getattr(complete, "error", None))
            self.assertFalse(bool((complete.result or {}).get("cursor_committed")))
            self.assertEqual(get_cursor(group, "peer1"), ("", ""))

            wait, _ = self._call(
                "web_model_runtime_wait_next_turn",
                {"group_id": group.group_id, "actor_id": "peer1"},
            )
            self.assertTrue(wait.ok, getattr(wait, "error", None))
            self.assertEqual(((wait.result or {}).get("turn") or {}).get("event_ids"), [event["id"]])
        finally:
            cleanup()

    def test_send_to_web_model_actor_does_not_add_duplicate_headless_notify(self) -> None:
        _, cleanup = self._with_home()
        try:
            group = self._create_group_with_actor()
            send, _ = self._call(
                "send",
                {
                    "group_id": group.group_id,
                    "by": "user",
                    "text": "single pull task",
                    "to": ["peer1"],
                },
            )
            self.assertTrue(send.ok, getattr(send, "error", None))

            wait, _ = self._call(
                "web_model_runtime_wait_next_turn",
                {"group_id": group.group_id, "actor_id": "peer1", "limit": 20},
            )
            self.assertTrue(wait.ok, getattr(wait, "error", None))
            turn = (wait.result or {}).get("turn") or {}
            messages = turn.get("messages") or []
            self.assertEqual([str(item.get("kind") or "") for item in messages], ["chat.message"])
            self.assertIn("single pull task", str(turn.get("coalesced_text") or ""))
            self.assertIn("[cccc] user → peer1", str(turn.get("coalesced_text") or ""))
            self.assertNotIn("[#1", str(turn.get("coalesced_text") or ""))
        finally:
            cleanup()

    def test_browser_delivery_sidecar_submits_turn_and_commits_cursor(self) -> None:
        from cccc.daemon.actors.web_model_browser_delivery import submit_next_web_model_browser_turn
        from cccc.daemon.runner_state_ops import read_headless_state
        from cccc.kernel.inbox import get_cursor
        from cccc.kernel.ledger import append_event, read_last_lines

        td, cleanup = self._with_home()
        old_command = os.environ.get("CCCC_WEB_MODEL_BROWSER_DELIVERY_COMMAND")
        old_mode = os.environ.get("CCCC_WEB_MODEL_DELIVERY_MODE")
        try:
            group = self._create_group_with_actor()
            self._bind_chatgpt_conversation(group)
            event = append_event(
                group.ledger_path,
                kind="chat.message",
                group_id=group.group_id,
                scope_key="",
                by="user",
                data={"text": "browser-delivered task", "to": ["peer1"]},
            )
            capture_path = Path(td) / "sidecar-payload.json"
            sidecar_path = Path(td) / "sidecar.py"
            sidecar_path.write_text(
                "\n".join(
                    [
                        "import json",
                        "import pathlib",
                        "import sys",
                        "payload = json.load(sys.stdin)",
                        (
                            f"pathlib.Path({str(capture_path)!r}).write_text("
                            "json.dumps(payload, ensure_ascii=False), encoding='utf-8')"
                        ),
                        (
                            "print(json.dumps({'ok': True, 'delivery_id': 'delivery-1', "
                            "'browser': {'tab_url': 'https://chatgpt.com/c/1'}}))"
                        ),
                    ]
                ),
                encoding="utf-8",
            )
            os.environ["CCCC_WEB_MODEL_BROWSER_DELIVERY_COMMAND"] = (
                f"{shlex.quote(sys.executable)} {shlex.quote(str(sidecar_path))}"
            )
            os.environ["CCCC_WEB_MODEL_DELIVERY_MODE"] = "browser"

            result = submit_next_web_model_browser_turn(group.group_id, "peer1", trigger_event_id=event["id"])

            self.assertTrue(result.get("ok"), result)
            self.assertEqual(result.get("status"), "submitted")
            self.assertEqual(get_cursor(group, "peer1")[0], event["id"])
            self.assertTrue(bool(result.get("cursor_committed")))
            self.assertEqual(str(read_headless_state(group.group_id, "peer1").get("status") or ""), "waiting")
            payload = json.loads(capture_path.read_text(encoding="utf-8"))
            self.assertEqual(payload.get("schema"), "cccc.web_model_browser_delivery.v1")
            self.assertEqual(payload.get("action"), "submit_turn")
            self.assertEqual(payload.get("group_id"), group.group_id)
            self.assertEqual(payload.get("actor_id"), "peer1")
            self.assertEqual(payload.get("trigger_event_id"), event["id"])
            self.assertEqual(payload.get("event_ids"), [event["id"]])
            self.assertTrue(str(payload.get("delivery_id") or "").startswith("webdelivery:peer1:"))
            self.assertEqual(payload.get("turn_id"), payload.get("delivery_id"))
            prompt = str(payload.get("prompt") or "")
            self.assertIn("Browser batch webdelivery:peer1:", prompt)
            self.assertIn(f"events={event['id']}", prompt)
            self.assertIn("actor=peer1", prompt)
            self.assertIn("Session bootstrap for this browser chat", prompt)
            self.assertIn("You are peer1", prompt)
            self.assertIn("Platform Invariants:", prompt)
            self.assertIn("Web transport:", prompt)
            self.assertIn("do not call cccc_runtime_wait_next_turn", prompt)
            self.assertIn("Text typed only in this web chat is not delivered", prompt)
            self.assertIn("If you respond: use MCP", prompt)
            self.assertIn("terminal output isn't delivered", prompt)
            self.assertIn("Verify reply_to/to", prompt)
            self.assertIn("avoid routine @all", prompt)
            self.assertIn("resume active work unless priority changed", prompt)
            self.assertNotIn("When done: cccc_runtime_complete_turn(", prompt)
            self.assertNotIn("webturn:peer1:", prompt)
            self.assertNotIn("Browser-delivered CCCC turn", prompt)
            self.assertNotIn("complete=", prompt)
            self.assertNotIn("future turns are not blocked", prompt)
            self.assertNotIn("Messages:", prompt)
            self.assertNotIn("Browser-delivered message batch", prompt)
            self.assertNotIn("Work from the messages below", prompt)
            self.assertNotIn("This ChatGPT chat is the browser surface", prompt)
            self.assertIn("[cccc] user → peer1", prompt)
            self.assertNotIn("[#1", prompt)
            self.assertNotIn("Browser Web Model actor", prompt)
            self.assertTrue(
                any("web_model.browser_delivery.submitted" in line for line in read_last_lines(group.ledger_path, 20))
            )
        finally:
            if old_command is None:
                os.environ.pop("CCCC_WEB_MODEL_BROWSER_DELIVERY_COMMAND", None)
            else:
                os.environ["CCCC_WEB_MODEL_BROWSER_DELIVERY_COMMAND"] = old_command
            if old_mode is None:
                os.environ.pop("CCCC_WEB_MODEL_DELIVERY_MODE", None)
            else:
                os.environ["CCCC_WEB_MODEL_DELIVERY_MODE"] = old_mode
            cleanup()

    def test_browser_delivery_reseeds_legacy_bootstrap_marker_without_digest(self) -> None:
        from cccc.daemon.actors.web_model_browser_delivery import submit_next_web_model_browser_turn
        from cccc.kernel.ledger import append_event
        from cccc.ports.web_model_browser_sidecar import record_chatgpt_browser_state

        td, cleanup = self._with_home()
        old_command = os.environ.get("CCCC_WEB_MODEL_BROWSER_DELIVERY_COMMAND")
        old_mode = os.environ.get("CCCC_WEB_MODEL_DELIVERY_MODE")
        try:
            group = self._create_group_with_actor()
            record_chatgpt_browser_state(
                group.group_id,
                "peer1",
                {
                    "conversation_url": "https://chatgpt.com/c/test-chat",
                    "bootstrap_seed_delivered_at": "2026-04-29T00:00:00Z",
                },
            )
            event = append_event(
                group.ledger_path,
                kind="chat.message",
                group_id=group.group_id,
                scope_key="",
                by="user",
                data={"text": "legacy marker should reseed", "to": ["peer1"]},
            )
            capture_path = Path(td) / "sidecar-payload.json"
            sidecar_path = Path(td) / "sidecar.py"
            sidecar_path.write_text(
                "\n".join(
                    [
                        "import json",
                        "import pathlib",
                        "import sys",
                        "payload = json.load(sys.stdin)",
                        (
                            f"pathlib.Path({str(capture_path)!r}).write_text("
                            "json.dumps(payload, ensure_ascii=False), encoding='utf-8')"
                        ),
                        "print(json.dumps({'ok': True, 'delivery_id': 'delivery-legacy'}))",
                    ]
                ),
                encoding="utf-8",
            )
            os.environ["CCCC_WEB_MODEL_BROWSER_DELIVERY_COMMAND"] = (
                f"{shlex.quote(sys.executable)} {shlex.quote(str(sidecar_path))}"
            )
            os.environ["CCCC_WEB_MODEL_DELIVERY_MODE"] = "browser"

            result = submit_next_web_model_browser_turn(group.group_id, "peer1", trigger_event_id=event["id"])

            self.assertTrue(result.get("ok"), result)
            payload = json.loads(capture_path.read_text(encoding="utf-8"))
            prompt = str(payload.get("prompt") or "")
            self.assertTrue(bool(payload.get("bootstrap_seed")))
            self.assertIn("Session bootstrap for this browser chat", prompt)
            self.assertIn("Platform Invariants:", prompt)
            self.assertTrue(str(payload.get("bootstrap_seed_version") or ""))
            self.assertTrue(str(payload.get("bootstrap_seed_digest") or ""))
            self.assertEqual(payload.get("bootstrap_seed_conversation_url"), "https://chatgpt.com/c/test-chat")
        finally:
            if old_command is None:
                os.environ.pop("CCCC_WEB_MODEL_BROWSER_DELIVERY_COMMAND", None)
            else:
                os.environ["CCCC_WEB_MODEL_BROWSER_DELIVERY_COMMAND"] = old_command
            if old_mode is None:
                os.environ.pop("CCCC_WEB_MODEL_DELIVERY_MODE", None)
            else:
                os.environ["CCCC_WEB_MODEL_DELIVERY_MODE"] = old_mode
            cleanup()

    def test_browser_delivery_sidecar_failure_leaves_turn_unread(self) -> None:
        from cccc.daemon.actors.web_model_browser_delivery import submit_next_web_model_browser_turn
        from cccc.daemon.runner_state_ops import read_headless_state
        from cccc.kernel.inbox import get_cursor
        from cccc.kernel.ledger import append_event, read_last_lines

        td, cleanup = self._with_home()
        old_command = os.environ.get("CCCC_WEB_MODEL_BROWSER_DELIVERY_COMMAND")
        old_mode = os.environ.get("CCCC_WEB_MODEL_DELIVERY_MODE")
        try:
            group = self._create_group_with_actor()
            self._bind_chatgpt_conversation(group)
            event = append_event(
                group.ledger_path,
                kind="chat.message",
                group_id=group.group_id,
                scope_key="",
                by="user",
                data={"text": "retry after failed browser delivery", "to": ["peer1"]},
            )
            sidecar_path = Path(td) / "failing-sidecar.py"
            sidecar_path.write_text(
                "\n".join(
                    [
                        "import sys",
                        "sys.stderr.write('browser unavailable')",
                        "sys.exit(2)",
                    ]
                ),
                encoding="utf-8",
            )
            os.environ["CCCC_WEB_MODEL_BROWSER_DELIVERY_COMMAND"] = (
                f"{shlex.quote(sys.executable)} {shlex.quote(str(sidecar_path))}"
            )
            os.environ["CCCC_WEB_MODEL_DELIVERY_MODE"] = "browser"

            result = submit_next_web_model_browser_turn(group.group_id, "peer1", trigger_event_id=event["id"])

            self.assertFalse(result.get("ok"), result)
            self.assertEqual(result.get("status"), "failed")
            self.assertEqual(get_cursor(group, "peer1"), ("", ""))
            state = read_headless_state(group.group_id, "peer1")
            self.assertEqual(str(state.get("status") or ""), "waiting")
            self.assertEqual(str(state.get("active_turn_id") or ""), "")
            self.assertTrue(
                any("web_model.browser_delivery.failed" in line for line in read_last_lines(group.ledger_path, 20))
            )
        finally:
            if old_command is None:
                os.environ.pop("CCCC_WEB_MODEL_BROWSER_DELIVERY_COMMAND", None)
            else:
                os.environ["CCCC_WEB_MODEL_BROWSER_DELIVERY_COMMAND"] = old_command
            if old_mode is None:
                os.environ.pop("CCCC_WEB_MODEL_DELIVERY_MODE", None)
            else:
                os.environ["CCCC_WEB_MODEL_DELIVERY_MODE"] = old_mode
            cleanup()

    def test_browser_delivery_requires_bound_target_chat_before_claiming_turn(self) -> None:
        from cccc.daemon.actors.web_model_browser_delivery import submit_next_web_model_browser_turn
        from cccc.daemon.runner_state_ops import read_headless_state
        from cccc.kernel.inbox import get_cursor
        from cccc.kernel.ledger import append_event

        _, cleanup = self._with_home()
        old_mode = os.environ.get("CCCC_WEB_MODEL_DELIVERY_MODE")
        try:
            group = self._create_group_with_actor()
            event = append_event(
                group.ledger_path,
                kind="chat.message",
                group_id=group.group_id,
                scope_key="",
                by="user",
                data={"text": "needs explicit chat target", "to": ["peer1"]},
            )
            os.environ["CCCC_WEB_MODEL_DELIVERY_MODE"] = "browser"

            result = submit_next_web_model_browser_turn(group.group_id, "peer1", trigger_event_id=event["id"])

            self.assertFalse(result.get("ok"), result)
            self.assertEqual(result.get("status"), "target_chat_required")
            self.assertEqual(get_cursor(group, "peer1"), ("", ""))
            state = read_headless_state(group.group_id, "peer1")
            self.assertEqual(str(state.get("status") or ""), "waiting")
            self.assertEqual(str(state.get("active_turn_id") or ""), "")
        finally:
            if old_mode is None:
                os.environ.pop("CCCC_WEB_MODEL_DELIVERY_MODE", None)
            else:
                os.environ["CCCC_WEB_MODEL_DELIVERY_MODE"] = old_mode
            cleanup()

    def test_browser_delivery_can_auto_bind_new_chat_after_first_submit(self) -> None:
        from cccc.daemon.actors.web_model_browser_delivery import submit_next_web_model_browser_turn
        from cccc.kernel.inbox import get_cursor
        from cccc.kernel.ledger import append_event
        from cccc.ports.web_model_browser_sidecar import read_chatgpt_browser_state, record_chatgpt_browser_state

        td, cleanup = self._with_home()
        old_command = os.environ.get("CCCC_WEB_MODEL_BROWSER_DELIVERY_COMMAND")
        old_mode = os.environ.get("CCCC_WEB_MODEL_DELIVERY_MODE")
        try:
            group = self._create_group_with_actor()
            record_chatgpt_browser_state(
                group.group_id,
                "peer1",
                {
                    "pending_new_chat_bind": True,
                    "pending_new_chat_url": "https://chatgpt.com/",
                },
            )
            event = append_event(
                group.ledger_path,
                kind="chat.message",
                group_id=group.group_id,
                scope_key="",
                by="user",
                data={"text": "create a fresh ChatGPT chat", "to": ["peer1"]},
            )
            capture_path = Path(td) / "sidecar-payload.json"
            sidecar_path = Path(td) / "sidecar.py"
            sidecar_path.write_text(
                "\n".join(
                    [
                        "import json",
                        "import pathlib",
                        "import sys",
                        "payload = json.load(sys.stdin)",
                        (
                            f"pathlib.Path({str(capture_path)!r}).write_text("
                            "json.dumps(payload, ensure_ascii=False), encoding='utf-8')"
                        ),
                        (
                            "print(json.dumps({'ok': True, 'delivery_id': 'delivery-new-chat', "
                            "'browser': {'tab_url': 'https://chatgpt.com/c/new-chat', "
                            "'conversation_url': 'https://chatgpt.com/c/new-chat', "
                            "'auto_bind_new_chat': True}}))"
                        ),
                    ]
                ),
                encoding="utf-8",
            )
            os.environ["CCCC_WEB_MODEL_BROWSER_DELIVERY_COMMAND"] = (
                f"{shlex.quote(sys.executable)} {shlex.quote(str(sidecar_path))}"
            )
            os.environ["CCCC_WEB_MODEL_DELIVERY_MODE"] = "browser"

            result = submit_next_web_model_browser_turn(group.group_id, "peer1", trigger_event_id=event["id"])

            self.assertTrue(result.get("ok"), result)
            self.assertEqual(result.get("status"), "submitted")
            self.assertEqual(get_cursor(group, "peer1")[0], event["id"])
            payload = json.loads(capture_path.read_text(encoding="utf-8"))
            self.assertEqual(payload.get("target_url"), "https://chatgpt.com/")
            self.assertEqual(payload.get("auto_bind_new_chat"), True)
            self.assertEqual(payload.get("bootstrap_seed_conversation_url"), "https://chatgpt.com/")
            state = read_chatgpt_browser_state(group.group_id, "peer1")
            self.assertEqual(state.get("conversation_url"), "https://chatgpt.com/c/new-chat")
            self.assertEqual(state.get("pending_new_chat_bind"), False)
            self.assertEqual(state.get("pending_new_chat_url"), "")
            self.assertEqual(state.get("bootstrap_seed_conversation_url"), "https://chatgpt.com/c/new-chat")
        finally:
            if old_command is None:
                os.environ.pop("CCCC_WEB_MODEL_BROWSER_DELIVERY_COMMAND", None)
            else:
                os.environ["CCCC_WEB_MODEL_BROWSER_DELIVERY_COMMAND"] = old_command
            if old_mode is None:
                os.environ.pop("CCCC_WEB_MODEL_DELIVERY_MODE", None)
            else:
                os.environ["CCCC_WEB_MODEL_DELIVERY_MODE"] = old_mode
            cleanup()

    def test_browser_delivery_new_chat_without_final_url_commits_pending_bind(self) -> None:
        from cccc.daemon.actors.web_model_browser_delivery import submit_next_web_model_browser_turn
        from cccc.kernel.inbox import get_cursor
        from cccc.kernel.ledger import append_event
        from cccc.ports.web_model_browser_sidecar import read_chatgpt_browser_state, record_chatgpt_browser_state

        td, cleanup = self._with_home()
        old_command = os.environ.get("CCCC_WEB_MODEL_BROWSER_DELIVERY_COMMAND")
        old_mode = os.environ.get("CCCC_WEB_MODEL_DELIVERY_MODE")
        try:
            group = self._create_group_with_actor()
            record_chatgpt_browser_state(
                group.group_id,
                "peer1",
                {
                    "pending_new_chat_bind": True,
                    "pending_new_chat_url": "https://chatgpt.com/",
                },
            )
            event = append_event(
                group.ledger_path,
                kind="chat.message",
                group_id=group.group_id,
                scope_key="",
                by="user",
                data={"text": "new chat but no final URL", "to": ["peer1"]},
            )
            sidecar_path = Path(td) / "sidecar.py"
            sidecar_path.write_text(
                "\n".join(
                    [
                        "import json",
                        "import sys",
                        "json.load(sys.stdin)",
                        "print(json.dumps({'ok': True, 'delivery_id': 'delivery-no-url', 'browser': {'tab_url': 'https://chatgpt.com/', 'pending_conversation_url': True, 'submitted_without_conversation_url': True}}))",
                    ]
                ),
                encoding="utf-8",
            )
            os.environ["CCCC_WEB_MODEL_BROWSER_DELIVERY_COMMAND"] = (
                f"{shlex.quote(sys.executable)} {shlex.quote(str(sidecar_path))}"
            )
            os.environ["CCCC_WEB_MODEL_DELIVERY_MODE"] = "browser"

            result = submit_next_web_model_browser_turn(group.group_id, "peer1", trigger_event_id=event["id"])

            self.assertTrue(result.get("ok"), result)
            self.assertEqual(result.get("status"), "submitted")
            self.assertEqual(get_cursor(group, "peer1")[0], event["id"])
            state = read_chatgpt_browser_state(group.group_id, "peer1")
            self.assertEqual(state.get("pending_new_chat_bind"), True)
            self.assertEqual(state.get("pending_new_chat_submitted"), True)
            self.assertEqual(state.get("pending_new_chat_delivery_id"), "delivery-no-url")
            self.assertEqual(state.get("pending_new_chat_last_turn_id"), result.get("turn_id"))
            self.assertEqual(state.get("conversation_url"), "")
            self.assertTrue(str(state.get("bootstrap_seed_delivered_at") or ""))
            self.assertEqual(state.get("bootstrap_seed_version"), "web-model-bootstrap-normal-system-prompt-v2")
            self.assertTrue(str(state.get("bootstrap_seed_digest") or ""))
            self.assertEqual(state.get("bootstrap_seed_conversation_url"), "https://chatgpt.com/")
            submitted = result.get("event") or {}
            data = submitted.get("data") if isinstance(submitted, dict) else {}
            self.assertEqual((data or {}).get("pending_conversation_url"), True)
        finally:
            if old_command is None:
                os.environ.pop("CCCC_WEB_MODEL_BROWSER_DELIVERY_COMMAND", None)
            else:
                os.environ["CCCC_WEB_MODEL_BROWSER_DELIVERY_COMMAND"] = old_command
            if old_mode is None:
                os.environ.pop("CCCC_WEB_MODEL_DELIVERY_MODE", None)
            else:
                os.environ["CCCC_WEB_MODEL_DELIVERY_MODE"] = old_mode
            cleanup()

    def test_browser_delivery_resolved_pending_new_chat_does_not_reseed(self) -> None:
        from cccc.daemon.actors.web_model_browser_delivery import submit_next_web_model_browser_turn
        from cccc.kernel.inbox import get_cursor
        from cccc.kernel.ledger import append_event
        from cccc.ports.web_model_browser_sidecar import read_chatgpt_browser_state, record_chatgpt_browser_state

        td, cleanup = self._with_home()
        old_command = os.environ.get("CCCC_WEB_MODEL_BROWSER_DELIVERY_COMMAND")
        old_mode = os.environ.get("CCCC_WEB_MODEL_DELIVERY_MODE")
        try:
            group = self._create_group_with_actor()
            record_chatgpt_browser_state(
                group.group_id,
                "peer1",
                {
                    "pending_new_chat_bind": True,
                    "pending_new_chat_url": "https://chatgpt.com/",
                },
            )
            first = append_event(
                group.ledger_path,
                kind="chat.message",
                group_id=group.group_id,
                scope_key="",
                by="user",
                data={"text": "new chat creates URL slowly", "to": ["peer1"]},
            )
            first_sidecar = Path(td) / "sidecar-first.py"
            first_sidecar.write_text(
                "\n".join(
                    [
                        "import json",
                        "import sys",
                        "json.load(sys.stdin)",
                        "print(json.dumps({'ok': True, 'delivery_id': 'delivery-pending-url', 'browser': {'tab_url': 'https://chatgpt.com/', 'pending_conversation_url': True, 'submitted_without_conversation_url': True}}))",
                    ]
                ),
                encoding="utf-8",
            )
            os.environ["CCCC_WEB_MODEL_BROWSER_DELIVERY_COMMAND"] = (
                f"{shlex.quote(sys.executable)} {shlex.quote(str(first_sidecar))}"
            )
            os.environ["CCCC_WEB_MODEL_DELIVERY_MODE"] = "browser"

            first_result = submit_next_web_model_browser_turn(group.group_id, "peer1", trigger_event_id=first["id"])
            self.assertTrue(first_result.get("ok"), first_result)
            self.assertEqual(get_cursor(group, "peer1")[0], first["id"])
            first_state = read_chatgpt_browser_state(group.group_id, "peer1")
            self.assertEqual(first_state.get("bootstrap_seed_conversation_url"), "https://chatgpt.com/")

            record_chatgpt_browser_state(
                group.group_id,
                "peer1",
                {"last_tab_url": "https://chatgpt.com/c/delayed-chat"},
            )
            second = append_event(
                group.ledger_path,
                kind="chat.message",
                group_id=group.group_id,
                scope_key="",
                by="user",
                data={"text": "follow-up after URL exists", "to": ["peer1"]},
            )
            capture_path = Path(td) / "sidecar-second-payload.json"
            second_sidecar = Path(td) / "sidecar-second.py"
            second_sidecar.write_text(
                "\n".join(
                    [
                        "import json",
                        "import pathlib",
                        "import sys",
                        "payload = json.load(sys.stdin)",
                        (
                            f"pathlib.Path({str(capture_path)!r}).write_text("
                            "json.dumps(payload, ensure_ascii=False), encoding='utf-8')"
                        ),
                        "print(json.dumps({'ok': True, 'delivery_id': 'delivery-follow-up', 'browser': {'tab_url': 'https://chatgpt.com/c/delayed-chat', 'conversation_url': 'https://chatgpt.com/c/delayed-chat'}}))",
                    ]
                ),
                encoding="utf-8",
            )
            os.environ["CCCC_WEB_MODEL_BROWSER_DELIVERY_COMMAND"] = (
                f"{shlex.quote(sys.executable)} {shlex.quote(str(second_sidecar))}"
            )

            second_result = submit_next_web_model_browser_turn(group.group_id, "peer1", trigger_event_id=second["id"])

            self.assertTrue(second_result.get("ok"), second_result)
            self.assertEqual(get_cursor(group, "peer1")[0], second["id"])
            payload = json.loads(capture_path.read_text(encoding="utf-8"))
            self.assertEqual(payload.get("target_url"), "https://chatgpt.com/c/delayed-chat")
            self.assertEqual(payload.get("bootstrap_seed"), False)
            self.assertNotIn("Session bootstrap for this browser chat", str(payload.get("prompt") or ""))
            state = read_chatgpt_browser_state(group.group_id, "peer1")
            self.assertEqual(state.get("conversation_url"), "https://chatgpt.com/c/delayed-chat")
            self.assertEqual(state.get("bootstrap_seed_conversation_url"), "https://chatgpt.com/c/delayed-chat")
        finally:
            if old_command is None:
                os.environ.pop("CCCC_WEB_MODEL_BROWSER_DELIVERY_COMMAND", None)
            else:
                os.environ["CCCC_WEB_MODEL_BROWSER_DELIVERY_COMMAND"] = old_command
            if old_mode is None:
                os.environ.pop("CCCC_WEB_MODEL_DELIVERY_MODE", None)
            else:
                os.environ["CCCC_WEB_MODEL_DELIVERY_MODE"] = old_mode
            cleanup()

    def test_browser_delivery_waits_for_pending_new_chat_resolution_before_resending(self) -> None:
        from cccc.daemon.actors.web_model_browser_delivery import submit_next_web_model_browser_turn
        from cccc.kernel.inbox import get_cursor
        from cccc.kernel.ledger import append_event
        from cccc.ports.web_model_browser_sidecar import record_chatgpt_browser_state

        td, cleanup = self._with_home()
        old_command = os.environ.get("CCCC_WEB_MODEL_BROWSER_DELIVERY_COMMAND")
        old_mode = os.environ.get("CCCC_WEB_MODEL_DELIVERY_MODE")
        try:
            group = self._create_group_with_actor()
            record_chatgpt_browser_state(
                group.group_id,
                "peer1",
                {
                    "pending_new_chat_bind": True,
                    "pending_new_chat_url": "https://chatgpt.com/",
                    "pending_new_chat_submitted": True,
                    "pending_new_chat_delivery_id": "delivery-pending",
                },
            )
            event = append_event(
                group.ledger_path,
                kind="chat.message",
                group_id=group.group_id,
                scope_key="",
                by="user",
                data={"text": "do not create another new chat", "to": ["peer1"]},
            )
            sidecar_path = Path(td) / "sidecar-should-not-run.py"
            sidecar_path.write_text("raise SystemExit('sidecar should not run while bind is pending')\n", encoding="utf-8")
            os.environ["CCCC_WEB_MODEL_BROWSER_DELIVERY_COMMAND"] = (
                f"{shlex.quote(sys.executable)} {shlex.quote(str(sidecar_path))}"
            )
            os.environ["CCCC_WEB_MODEL_DELIVERY_MODE"] = "browser"

            with patch(
                "cccc.daemon.actors.web_model_browser_delivery.resolve_pending_chatgpt_conversation",
                return_value={"ok": True, "resolved": False, "pending": True, "submitted": True},
            ) as resolve:
                result = submit_next_web_model_browser_turn(group.group_id, "peer1", trigger_event_id=event["id"])

            self.assertTrue(result.get("ok"), result)
            self.assertEqual(result.get("status"), "target_chat_binding_pending")
            self.assertEqual(get_cursor(group, "peer1"), ("", ""))
            resolve.assert_called_once_with(group.group_id, "peer1")
        finally:
            if old_command is None:
                os.environ.pop("CCCC_WEB_MODEL_BROWSER_DELIVERY_COMMAND", None)
            else:
                os.environ["CCCC_WEB_MODEL_BROWSER_DELIVERY_COMMAND"] = old_command
            if old_mode is None:
                os.environ.pop("CCCC_WEB_MODEL_DELIVERY_MODE", None)
            else:
                os.environ["CCCC_WEB_MODEL_DELIVERY_MODE"] = old_mode
            cleanup()

    def test_browser_delivery_retries_stale_pending_new_chat_bind(self) -> None:
        from cccc.daemon.actors.web_model_browser_delivery import submit_next_web_model_browser_turn
        from cccc.kernel.inbox import get_cursor
        from cccc.kernel.ledger import append_event
        from cccc.ports.web_model_browser_sidecar import read_chatgpt_browser_state, record_chatgpt_browser_state

        td, cleanup = self._with_home()
        old_command = os.environ.get("CCCC_WEB_MODEL_BROWSER_DELIVERY_COMMAND")
        old_mode = os.environ.get("CCCC_WEB_MODEL_DELIVERY_MODE")
        try:
            group = self._create_group_with_actor()
            record_chatgpt_browser_state(
                group.group_id,
                "peer1",
                {
                    "pending_new_chat_bind": True,
                    "pending_new_chat_url": "https://chatgpt.com/",
                    "pending_new_chat_submitted": True,
                    "pending_new_chat_submitted_at": "2000-01-01T00:00:00Z",
                    "pending_new_chat_delivery_id": "delivery-stale",
                    "bootstrap_seed_delivered_at": "2000-01-01T00:00:01Z",
                    "bootstrap_seed_version": "web-model-bootstrap-normal-system-prompt-v2",
                    "bootstrap_seed_digest": "old",
                    "bootstrap_seed_conversation_url": "https://chatgpt.com/",
                },
            )
            event = append_event(
                group.ledger_path,
                kind="chat.message",
                group_id=group.group_id,
                scope_key="",
                by="user",
                data={"text": "retry stale new chat", "to": ["peer1"]},
            )
            capture_path = Path(td) / "sidecar-payload.json"
            sidecar_path = Path(td) / "sidecar.py"
            sidecar_path.write_text(
                "\n".join(
                    [
                        "import json",
                        "import pathlib",
                        "import sys",
                        "payload = json.load(sys.stdin)",
                        (
                            f"pathlib.Path({str(capture_path)!r}).write_text("
                            "json.dumps(payload, ensure_ascii=False), encoding='utf-8')"
                        ),
                        (
                            "print(json.dumps({'ok': True, 'delivery_id': 'delivery-retry', "
                            "'browser': {'tab_url': 'https://chatgpt.com/c/retry-chat', "
                            "'conversation_url': 'https://chatgpt.com/c/retry-chat', "
                            "'auto_bind_new_chat': True}}))"
                        ),
                    ]
                ),
                encoding="utf-8",
            )
            os.environ["CCCC_WEB_MODEL_BROWSER_DELIVERY_COMMAND"] = (
                f"{shlex.quote(sys.executable)} {shlex.quote(str(sidecar_path))}"
            )
            os.environ["CCCC_WEB_MODEL_DELIVERY_MODE"] = "browser"

            with patch(
                "cccc.daemon.actors.web_model_browser_delivery.resolve_pending_chatgpt_conversation",
                return_value={"ok": True, "resolved": False, "pending": True, "submitted": True},
            ) as resolve:
                result = submit_next_web_model_browser_turn(group.group_id, "peer1", trigger_event_id=event["id"])

            self.assertTrue(result.get("ok"), result)
            self.assertEqual(result.get("status"), "submitted")
            self.assertEqual(get_cursor(group, "peer1")[0], event["id"])
            payload = json.loads(capture_path.read_text(encoding="utf-8"))
            self.assertEqual(payload.get("target_url"), "https://chatgpt.com/")
            self.assertEqual(payload.get("auto_bind_new_chat"), True)
            self.assertEqual(payload.get("bootstrap_seed"), True)
            state = read_chatgpt_browser_state(group.group_id, "peer1")
            self.assertEqual(state.get("conversation_url"), "https://chatgpt.com/c/retry-chat")
            self.assertEqual(state.get("pending_new_chat_bind"), False)
            resolve.assert_called_once_with(group.group_id, "peer1")
        finally:
            if old_command is None:
                os.environ.pop("CCCC_WEB_MODEL_BROWSER_DELIVERY_COMMAND", None)
            else:
                os.environ["CCCC_WEB_MODEL_BROWSER_DELIVERY_COMMAND"] = old_command
            if old_mode is None:
                os.environ.pop("CCCC_WEB_MODEL_DELIVERY_MODE", None)
            else:
                os.environ["CCCC_WEB_MODEL_DELIVERY_MODE"] = old_mode
            cleanup()

    def test_browser_delivery_resolves_pending_new_chat_then_delivers_to_bound_url(self) -> None:
        from cccc.daemon.actors.web_model_browser_delivery import submit_next_web_model_browser_turn
        from cccc.kernel.inbox import get_cursor
        from cccc.kernel.ledger import append_event
        from cccc.ports.web_model_browser_sidecar import read_chatgpt_browser_state, record_chatgpt_browser_state

        td, cleanup = self._with_home()
        old_command = os.environ.get("CCCC_WEB_MODEL_BROWSER_DELIVERY_COMMAND")
        old_mode = os.environ.get("CCCC_WEB_MODEL_DELIVERY_MODE")
        try:
            group = self._create_group_with_actor()
            record_chatgpt_browser_state(
                group.group_id,
                "peer1",
                {
                    "pending_new_chat_bind": True,
                    "pending_new_chat_url": "https://chatgpt.com/",
                    "pending_new_chat_submitted": True,
                    "pending_new_chat_delivery_id": "delivery-pending",
                },
            )
            event = append_event(
                group.ledger_path,
                kind="chat.message",
                group_id=group.group_id,
                scope_key="",
                by="user",
                data={"text": "send after ChatGPT URL exists", "to": ["peer1"]},
            )
            capture_path = Path(td) / "sidecar-resolved-payload.json"
            sidecar_path = Path(td) / "sidecar-resolved.py"
            sidecar_path.write_text(
                "\n".join(
                    [
                        "import json",
                        "import pathlib",
                        "import sys",
                        "payload = json.load(sys.stdin)",
                        f"pathlib.Path({str(capture_path)!r}).write_text(json.dumps(payload), encoding='utf-8')",
                        "print(json.dumps({'ok': True, 'delivery_id': 'delivery-resolved', 'browser': {'tab_url': 'https://chatgpt.com/c/resolved', 'conversation_url': 'https://chatgpt.com/c/resolved'}}))",
                    ]
                ),
                encoding="utf-8",
            )
            os.environ["CCCC_WEB_MODEL_BROWSER_DELIVERY_COMMAND"] = (
                f"{shlex.quote(sys.executable)} {shlex.quote(str(sidecar_path))}"
            )
            os.environ["CCCC_WEB_MODEL_DELIVERY_MODE"] = "browser"

            def resolve(group_id: str, actor_id: str) -> dict[str, object]:
                record_chatgpt_browser_state(
                    group_id,
                    actor_id,
                    {
                        "conversation_url": "https://chatgpt.com/c/resolved",
                        "pending_new_chat_bind": False,
                        "pending_new_chat_url": "",
                        "pending_new_chat_submitted": False,
                    },
                )
                return {"ok": True, "resolved": True, "conversation_url": "https://chatgpt.com/c/resolved"}

            with patch("cccc.daemon.actors.web_model_browser_delivery.resolve_pending_chatgpt_conversation", side_effect=resolve):
                result = submit_next_web_model_browser_turn(group.group_id, "peer1", trigger_event_id=event["id"])

            self.assertTrue(result.get("ok"), result)
            self.assertEqual(result.get("status"), "submitted")
            self.assertEqual(get_cursor(group, "peer1")[0], event["id"])
            payload = json.loads(capture_path.read_text(encoding="utf-8"))
            self.assertEqual(payload.get("target_url"), "https://chatgpt.com/c/resolved")
            self.assertEqual(payload.get("auto_bind_new_chat"), False)
            state = read_chatgpt_browser_state(group.group_id, "peer1")
            self.assertEqual(state.get("conversation_url"), "https://chatgpt.com/c/resolved")
            self.assertEqual(state.get("pending_new_chat_bind"), False)
        finally:
            if old_command is None:
                os.environ.pop("CCCC_WEB_MODEL_BROWSER_DELIVERY_COMMAND", None)
            else:
                os.environ["CCCC_WEB_MODEL_BROWSER_DELIVERY_COMMAND"] = old_command
            if old_mode is None:
                os.environ.pop("CCCC_WEB_MODEL_DELIVERY_MODE", None)
            else:
                os.environ["CCCC_WEB_MODEL_DELIVERY_MODE"] = old_mode
            cleanup()

    def test_browser_delivery_retries_stale_active_turn_before_next_delivery(self) -> None:
        from cccc.daemon.actors.web_model_browser_delivery import submit_next_web_model_browser_turn
        from cccc.kernel.inbox import get_cursor
        from cccc.kernel.ledger import append_event

        td, cleanup = self._with_home()
        old_command = os.environ.get("CCCC_WEB_MODEL_BROWSER_DELIVERY_COMMAND")
        old_mode = os.environ.get("CCCC_WEB_MODEL_DELIVERY_MODE")
        try:
            group = self._create_group_with_actor()
            self._bind_chatgpt_conversation(group)
            old_event = append_event(
                group.ledger_path,
                kind="chat.message",
                group_id=group.group_id,
                scope_key="",
                by="user",
                data={"text": "already injected", "to": ["peer1"]},
            )
            wait, _ = self._call(
                "web_model_runtime_wait_next_turn",
                {"group_id": group.group_id, "actor_id": "peer1", "limit": 1},
            )
            self.assertTrue(wait.ok, getattr(wait, "error", None))
            new_event = append_event(
                group.ledger_path,
                kind="chat.message",
                group_id=group.group_id,
                scope_key="",
                by="user",
                data={"text": "deliver this one", "to": ["peer1"]},
            )

            capture_path = Path(td) / "sidecar-payload.json"
            sidecar_path = Path(td) / "sidecar.py"
            sidecar_path.write_text(
                "\n".join(
                    [
                        "import json",
                        "import pathlib",
                        "import sys",
                        "payload = json.load(sys.stdin)",
                        (
                            f"pathlib.Path({str(capture_path)!r}).write_text("
                            "json.dumps(payload, ensure_ascii=False), encoding='utf-8')"
                        ),
                        "print(json.dumps({'ok': True, 'delivery_id': 'delivery-2'}))",
                    ]
                ),
                encoding="utf-8",
            )
            os.environ["CCCC_WEB_MODEL_BROWSER_DELIVERY_COMMAND"] = (
                f"{shlex.quote(sys.executable)} {shlex.quote(str(sidecar_path))}"
            )
            os.environ["CCCC_WEB_MODEL_DELIVERY_MODE"] = "browser"

            result = submit_next_web_model_browser_turn(group.group_id, "peer1", trigger_event_id=new_event["id"])

            self.assertTrue(result.get("ok"), result)
            self.assertEqual(get_cursor(group, "peer1")[0], new_event["id"])
            payload = json.loads(capture_path.read_text(encoding="utf-8"))
            self.assertEqual(payload.get("event_ids"), [old_event["id"], new_event["id"]])
        finally:
            if old_command is None:
                os.environ.pop("CCCC_WEB_MODEL_BROWSER_DELIVERY_COMMAND", None)
            else:
                os.environ["CCCC_WEB_MODEL_BROWSER_DELIVERY_COMMAND"] = old_command
            if old_mode is None:
                os.environ.pop("CCCC_WEB_MODEL_DELIVERY_MODE", None)
            else:
                os.environ["CCCC_WEB_MODEL_DELIVERY_MODE"] = old_mode
            cleanup()

    def test_browser_delivery_schedules_even_when_previous_turn_is_active(self) -> None:
        from cccc.daemon.actors.web_model_browser_delivery import schedule_web_model_browser_delivery
        from cccc.daemon.runner_state_ops import update_headless_state

        _, cleanup = self._with_home()
        try:
            group = self._create_group_with_actor()
            update_headless_state(group.group_id, "peer1", status="working", active_turn_id="turn-active")

            with patch("cccc.daemon.actors.web_model_browser_delivery.threading.Thread") as thread_cls:
                scheduled = schedule_web_model_browser_delivery(group_id=group.group_id, actor_id="peer1")

            self.assertTrue(scheduled)
            thread_cls.assert_called_once()
        finally:
            cleanup()

    def test_complete_turn_can_schedule_next_browser_delivery(self) -> None:
        from cccc.kernel.ledger import append_event

        _, cleanup = self._with_home()
        try:
            group = self._create_group_with_actor()
            event = append_event(
                group.ledger_path,
                kind="chat.message",
                group_id=group.group_id,
                scope_key="",
                by="user",
                data={"text": "finish then continue", "to": ["peer1"]},
            )

            with (
                patch(
                    "cccc.daemon.actors.web_model_browser_delivery.web_model_browser_delivery_enabled",
                    return_value=True,
                ),
                patch(
                    "cccc.daemon.actors.web_model_browser_delivery.schedule_web_model_browser_delivery",
                    return_value=True,
                ) as schedule,
            ):
                complete, _ = self._call(
                    "web_model_runtime_complete_turn",
                    {
                        "group_id": group.group_id,
                        "actor_id": "peer1",
                        "by": "peer1",
                        "event_ids": [event["id"]],
                        "status": "done",
                    },
                )

            self.assertTrue(complete.ok, getattr(complete, "error", None))
            self.assertTrue(bool((complete.result or {}).get("followup_delivery_scheduled")))
            schedule.assert_called_once_with(group_id=group.group_id, actor_id="peer1", trigger_event_id=event["id"])
        finally:
            cleanup()

    def test_stopped_web_model_actor_does_not_receive_pull_turn(self) -> None:
        from cccc.daemon.runner_state_ops import remove_headless_state
        from cccc.kernel.actors import update_actor
        from cccc.kernel.ledger import append_event

        _, cleanup = self._with_home()
        try:
            group = self._create_group_with_actor()
            update_actor(group, "peer1", {"enabled": False})
            remove_headless_state(group.group_id, "peer1")
            append_event(
                group.ledger_path,
                kind="chat.message",
                group_id=group.group_id,
                scope_key="",
                by="user",
                data={"text": "do not deliver while stopped", "to": ["peer1"]},
            )

            wait, _ = self._call(
                "web_model_runtime_wait_next_turn",
                {"group_id": group.group_id, "actor_id": "peer1"},
            )
            self.assertTrue(wait.ok, getattr(wait, "error", None))
            self.assertEqual((wait.result or {}).get("status"), "stopped")
            self.assertIsNone((wait.result or {}).get("turn"))
        finally:
            cleanup()

    def test_complete_rejects_non_contiguous_unread_prefix(self) -> None:
        from cccc.kernel.inbox import get_cursor
        from cccc.kernel.ledger import append_event

        _, cleanup = self._with_home()
        try:
            group = self._create_group_with_actor()
            first = append_event(
                group.ledger_path,
                kind="chat.message",
                group_id=group.group_id,
                scope_key="",
                by="user",
                data={"text": "first", "to": ["peer1"]},
            )
            second = append_event(
                group.ledger_path,
                kind="chat.message",
                group_id=group.group_id,
                scope_key="",
                by="user",
                data={"text": "second", "to": ["peer1"]},
            )

            complete, _ = self._call(
                "web_model_runtime_complete_turn",
                {
                    "group_id": group.group_id,
                    "actor_id": "peer1",
                    "by": "peer1",
                    "event_ids": [second["id"]],
                    "status": "done",
                },
            )
            self.assertFalse(complete.ok)
            self.assertEqual(str(getattr(complete.error, "code", "")), "non_contiguous_turn_events")
            self.assertEqual(get_cursor(group, "peer1"), ("", ""))

            valid, _ = self._call(
                "web_model_runtime_complete_turn",
                {
                    "group_id": group.group_id,
                    "actor_id": "peer1",
                    "by": "peer1",
                    "event_ids": [first["id"], second["id"]],
                    "status": "done",
                },
            )
            self.assertTrue(valid.ok, getattr(valid, "error", None))
            self.assertEqual(get_cursor(group, "peer1")[0], second["id"])
        finally:
            cleanup()
