import json
import os
import tempfile
import unittest
from pathlib import Path
from unittest.mock import ANY, patch


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

    def _attach_repo_scope(self, group, root: Path):
        from cccc.kernel.group import attach_scope_to_group
        from cccc.kernel.registry import load_registry
        from cccc.kernel.scope import detect_scope

        root.mkdir(parents=True, exist_ok=True)
        (root / "README.md").write_text("repo context\n", encoding="utf-8")
        return attach_scope_to_group(load_registry(), group, detect_scope(root), set_active=True)

    def _projected_submit_result(
        self,
        *,
        delivery_id: str = "delivery-1",
        tab_url: str = "https://chatgpt.com/c/test-chat",
        conversation_url: str = "https://chatgpt.com/c/test-chat",
        pending_conversation_url: bool = False,
        submitted_without_conversation_url: bool = False,
        submission_evidence: str = "message_echo",
        send_selector: str = "#composer-submit-button",
    ) -> dict:
        browser: dict[str, object] = {
            "tab_url": tab_url,
            "submission_evidence": submission_evidence,
            "send_selector": send_selector,
        }
        if conversation_url:
            browser["conversation_url"] = conversation_url
        if pending_conversation_url:
            browser["pending_conversation_url"] = True
        if submitted_without_conversation_url:
            browser["submitted_without_conversation_url"] = True
        return {
            "ok": True,
            "delivery_id": delivery_id,
            "transport": "projected_session",
            "browser_surface": {"state": "ready", "url": tab_url},
            "browser": browser,
        }

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

    def test_browser_delivery_projected_session_submits_turn_and_commits_cursor(self) -> None:
        from cccc.daemon.actors.web_model_browser_delivery import submit_next_web_model_browser_turn
        from cccc.daemon.runner_state_ops import read_headless_state
        from cccc.kernel.inbox import get_cursor
        from cccc.kernel.ledger import append_event, read_last_lines
        from cccc.ports.web_model_browser_sidecar import read_chatgpt_browser_state

        _, cleanup = self._with_home()
        old_mode = os.environ.get("CCCC_WEB_MODEL_DELIVERY_MODE")
        old_reload = os.environ.get("CCCC_WEB_MODEL_BROWSER_AUTO_RELOAD")
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
            os.environ["CCCC_WEB_MODEL_DELIVERY_MODE"] = "browser"
            os.environ.pop("CCCC_WEB_MODEL_BROWSER_AUTO_RELOAD", None)
            calls: list[dict] = []

            def submit_via_session(**kwargs) -> dict:
                calls.append(dict(kwargs))
                return self._projected_submit_result(
                    delivery_id="delivery-1",
                    tab_url="https://chatgpt.com/c/1",
                    conversation_url="https://chatgpt.com/c/1",
                )

            with patch(
                "cccc.daemon.actors.web_model_browser_session.submit_prompt_via_web_model_chatgpt_browser_session",
                side_effect=submit_via_session,
            ):
                result = submit_next_web_model_browser_turn(group.group_id, "peer1", trigger_event_id=event["id"])

            self.assertTrue(result.get("ok"), result)
            self.assertEqual(result.get("status"), "submitted")
            self.assertEqual(get_cursor(group, "peer1")[0], event["id"])
            self.assertTrue(bool(result.get("cursor_committed")))
            self.assertEqual(str(read_headless_state(group.group_id, "peer1").get("status") or ""), "waiting")
            browser_state = read_chatgpt_browser_state(group.group_id, "peer1")
            self.assertEqual(browser_state.get("auto_reload_active"), False)
            self.assertEqual(browser_state.get("auto_reload_completed_reason"), "disabled")
            self.assertEqual(len(calls), 1)
            payload = calls[0]
            self.assertEqual(payload.get("group_id"), group.group_id)
            self.assertEqual(payload.get("actor_id"), "peer1")
            self.assertEqual(payload.get("target_url"), "https://chatgpt.com/c/test-chat")
            self.assertEqual(payload.get("auto_bind_new_chat"), False)
            self.assertTrue(str(payload.get("delivery_id") or "").startswith("webdelivery:peer1:"))
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
            if old_mode is None:
                os.environ.pop("CCCC_WEB_MODEL_DELIVERY_MODE", None)
            else:
                os.environ["CCCC_WEB_MODEL_DELIVERY_MODE"] = old_mode
            if old_reload is None:
                os.environ.pop("CCCC_WEB_MODEL_BROWSER_AUTO_RELOAD", None)
            else:
                os.environ["CCCC_WEB_MODEL_BROWSER_AUTO_RELOAD"] = old_reload
            cleanup()

    def test_browser_delivery_does_not_include_nomcp_fallback_when_public_https_is_available(self) -> None:
        from cccc.daemon.actors.web_model_browser_delivery import submit_next_web_model_browser_turn
        from cccc.kernel.ledger import append_event

        home, cleanup = self._with_home()
        old_mode = os.environ.get("CCCC_WEB_MODEL_DELIVERY_MODE")
        old_public_url = os.environ.get("CCCC_WEB_PUBLIC_URL")
        try:
            group = self._create_group_with_actor()
            group = self._attach_repo_scope(group, Path(home) / "repo")
            self._bind_chatgpt_conversation(group, url="https://chatgpt.com/c/nomcp-fallback")
            event = append_event(
                group.ledger_path,
                kind="chat.message",
                group_id=group.group_id,
                scope_key="",
                by="user",
                data={"text": "review this if MCP is unavailable", "to": ["peer1"]},
            )
            os.environ["CCCC_WEB_MODEL_DELIVERY_MODE"] = "browser"
            os.environ["CCCC_WEB_PUBLIC_URL"] = "https://cccc.example.test/ui/"
            prompts: list[str] = []

            def submit_via_session(**kwargs) -> dict:
                prompts.append(str(kwargs.get("prompt") or ""))
                return self._projected_submit_result(
                    delivery_id="delivery-nomcp",
                    tab_url="https://chatgpt.com/c/nomcp-fallback",
                    conversation_url="https://chatgpt.com/c/nomcp-fallback",
                )

            with patch(
                "cccc.daemon.actors.web_model_browser_session.submit_prompt_via_web_model_chatgpt_browser_session",
                side_effect=submit_via_session,
            ):
                result = submit_next_web_model_browser_turn(group.group_id, "peer1", trigger_event_id=event["id"])

            self.assertTrue(result.get("ok"), result)
            self.assertEqual(len(prompts), 1)
            prompt = prompts[0]
            self.assertIn("review this if MCP is unavailable", prompt)
            self.assertIn("If you respond: use MCP", prompt)
            self.assertNotIn("No-MCP", prompt)
            self.assertNotIn("/nomcp/", prompt)
            self.assertNotIn("delivery-direct-test", prompt)
        finally:
            if old_mode is None:
                os.environ.pop("CCCC_WEB_MODEL_DELIVERY_MODE", None)
            else:
                os.environ["CCCC_WEB_MODEL_DELIVERY_MODE"] = old_mode
            if old_public_url is None:
                os.environ.pop("CCCC_WEB_PUBLIC_URL", None)
            else:
                os.environ["CCCC_WEB_PUBLIC_URL"] = old_public_url
            cleanup()

    def test_browser_delivery_uses_projected_chatgpt_session(self) -> None:
        from cccc.daemon.actors.web_model_browser_delivery import submit_next_web_model_browser_turn
        from cccc.kernel.inbox import get_cursor
        from cccc.kernel.ledger import append_event, read_last_lines
        from cccc.ports.web_model_browser_sidecar import read_chatgpt_browser_state

        _, cleanup = self._with_home()
        old_mode = os.environ.get("CCCC_WEB_MODEL_DELIVERY_MODE")
        try:
            group = self._create_group_with_actor()
            self._bind_chatgpt_conversation(group, url="https://chatgpt.com/c/bound-session")
            event = append_event(
                group.ledger_path,
                kind="chat.message",
                group_id=group.group_id,
                scope_key="",
                by="user",
                data={"text": "deliver through the daemon browser session", "to": ["peer1"]},
            )
            os.environ["CCCC_WEB_MODEL_DELIVERY_MODE"] = "browser"
            calls: list[str] = []
            observed_submit_states: list[str] = []

            def submit_via_session(**kwargs) -> dict:
                calls.append(f"projected:{kwargs.get('group_id')}:{kwargs.get('actor_id')}")
                self.assertEqual(kwargs.get("target_url"), "https://chatgpt.com/c/bound-session")
                self.assertIn("deliver through the daemon browser session", str(kwargs.get("prompt") or ""))
                observed = read_chatgpt_browser_state(group.group_id, "peer1")
                observed_submit_states.append(str(observed.get("last_delivery_status") or ""))
                self.assertEqual(observed.get("last_event_ids"), [event["id"]])
                return {
                    "ok": True,
                    "delivery_id": "delivery-builtin",
                    "transport": "projected_session",
                    "browser_surface": {
                        "state": "ready",
                        "url": "https://chatgpt.com/c/bound-session",
                    },
                    "browser": {
                        "tab_url": "https://chatgpt.com/c/bound-session",
                        "conversation_url": "https://chatgpt.com/c/bound-session",
                        "submission_evidence": "message_echo",
                    },
                }

            with patch(
                "cccc.daemon.actors.web_model_browser_session.submit_prompt_via_web_model_chatgpt_browser_session",
                side_effect=submit_via_session,
            ):
                result = submit_next_web_model_browser_turn(group.group_id, "peer1", trigger_event_id=event["id"])

            self.assertTrue(result.get("ok"), result)
            self.assertEqual(result.get("status"), "submitted")
            self.assertEqual(calls, [f"projected:{group.group_id}:peer1"])
            self.assertEqual(observed_submit_states, ["submitting"])
            self.assertEqual(get_cursor(group, "peer1")[0], event["id"])
            data = ((result.get("event") or {}).get("data") or {})
            self.assertEqual((data.get("browser_surface") or {}).get("state"), "ready")
            self.assertEqual(data.get("delivery_transport"), "projected_session")
            browser_state = read_chatgpt_browser_state(group.group_id, "peer1")
            self.assertEqual(browser_state.get("last_delivery_status"), "submitted")
            self.assertEqual(browser_state.get("last_submission_evidence"), "message_echo")
            self.assertEqual(browser_state.get("last_event_ids"), [event["id"]])
            self.assertTrue(
                any("web_model.browser_delivery.submitting" in line for line in read_last_lines(group.ledger_path, 20))
            )
        finally:
            if old_mode is None:
                os.environ.pop("CCCC_WEB_MODEL_DELIVERY_MODE", None)
            else:
                os.environ["CCCC_WEB_MODEL_DELIVERY_MODE"] = old_mode
            cleanup()

    def test_builtin_browser_delivery_retries_once_after_transient_projected_page_close(self) -> None:
        from cccc.daemon.actors.web_model_browser_delivery import submit_next_web_model_browser_turn
        from cccc.kernel.inbox import get_cursor
        from cccc.kernel.ledger import append_event

        _, cleanup = self._with_home()
        old_mode = os.environ.get("CCCC_WEB_MODEL_DELIVERY_MODE")
        try:
            group = self._create_group_with_actor()
            self._bind_chatgpt_conversation(group, url="https://chatgpt.com/c/bound-session")
            event = append_event(
                group.ledger_path,
                kind="chat.message",
                group_id=group.group_id,
                scope_key="",
                by="user",
                data={"text": "retry transient page close", "to": ["peer1"]},
            )
            os.environ["CCCC_WEB_MODEL_DELIVERY_MODE"] = "browser"
            calls: list[str] = []

            def submit_via_session(**kwargs) -> dict:
                calls.append(str(kwargs.get("delivery_id") or ""))
                if len(calls) == 1:
                    raise RuntimeError("Page.evaluate: Target page, context or browser has been closed")
                return {
                    "ok": True,
                    "delivery_id": "delivery-after-retry",
                    "transport": "projected_session",
                    "browser_surface": {
                        "state": "ready",
                        "url": "https://chatgpt.com/c/bound-session",
                    },
                    "browser": {
                        "tab_url": "https://chatgpt.com/c/bound-session",
                        "conversation_url": "https://chatgpt.com/c/bound-session",
                        "submission_evidence": "message_echo",
                    },
                }

            with (
                patch(
                    "cccc.daemon.actors.web_model_browser_session.submit_prompt_via_web_model_chatgpt_browser_session",
                    side_effect=submit_via_session,
                ),
                patch("cccc.daemon.actors.web_model_browser_session.close_web_model_chatgpt_browser_session") as close_session,
            ):
                result = submit_next_web_model_browser_turn(group.group_id, "peer1", trigger_event_id=event["id"])

            self.assertTrue(result.get("ok"), result)
            self.assertEqual(result.get("status"), "submitted")
            self.assertEqual(len(calls), 2)
            close_session.assert_called_once_with(group_id=group.group_id, actor_id="peer1")
            self.assertEqual(get_cursor(group, "peer1")[0], event["id"])
        finally:
            if old_mode is None:
                os.environ.pop("CCCC_WEB_MODEL_DELIVERY_MODE", None)
            else:
                os.environ["CCCC_WEB_MODEL_DELIVERY_MODE"] = old_mode
            cleanup()

    def test_builtin_browser_delivery_retries_once_after_transient_hidden_input_click(self) -> None:
        from cccc.daemon.actors.web_model_browser_delivery import submit_next_web_model_browser_turn
        from cccc.kernel.inbox import get_cursor
        from cccc.kernel.ledger import append_event

        _, cleanup = self._with_home()
        old_mode = os.environ.get("CCCC_WEB_MODEL_DELIVERY_MODE")
        try:
            group = self._create_group_with_actor()
            self._bind_chatgpt_conversation(group, url="https://chatgpt.com/c/bound-session")
            event = append_event(
                group.ledger_path,
                kind="chat.message",
                group_id=group.group_id,
                scope_key="",
                by="user",
                data={"text": "retry hidden input click", "to": ["peer1"]},
            )
            os.environ["CCCC_WEB_MODEL_DELIVERY_MODE"] = "browser"
            calls: list[str] = []

            def submit_via_session(**kwargs) -> dict:
                calls.append(str(kwargs.get("delivery_id") or ""))
                if len(calls) == 1:
                    raise RuntimeError(
                        'Locator.click: Timeout 5000ms exceeded; locator("textarea:not([disabled])").first; element is not visible'
                    )
                return self._projected_submit_result(delivery_id="delivery-after-retry")

            with (
                patch(
                    "cccc.daemon.actors.web_model_browser_session.submit_prompt_via_web_model_chatgpt_browser_session",
                    side_effect=submit_via_session,
                ),
                patch("cccc.daemon.actors.web_model_browser_session.close_web_model_chatgpt_browser_session") as close_session,
            ):
                result = submit_next_web_model_browser_turn(group.group_id, "peer1", trigger_event_id=event["id"])

            self.assertTrue(result.get("ok"), result)
            self.assertEqual(result.get("status"), "submitted")
            self.assertEqual(len(calls), 2)
            close_session.assert_called_once_with(group_id=group.group_id, actor_id="peer1")
            self.assertEqual(get_cursor(group, "peer1")[0], event["id"])
        finally:
            if old_mode is None:
                os.environ.pop("CCCC_WEB_MODEL_DELIVERY_MODE", None)
            else:
                os.environ["CCCC_WEB_MODEL_DELIVERY_MODE"] = old_mode
            cleanup()

    def test_builtin_browser_delivery_retries_once_after_inserted_without_submit(self) -> None:
        from cccc.daemon.actors.web_model_browser_delivery import submit_next_web_model_browser_turn
        from cccc.kernel.inbox import get_cursor
        from cccc.kernel.ledger import append_event

        _, cleanup = self._with_home()
        old_mode = os.environ.get("CCCC_WEB_MODEL_DELIVERY_MODE")
        try:
            group = self._create_group_with_actor()
            self._bind_chatgpt_conversation(group, url="https://chatgpt.com/c/bound-session")
            event = append_event(
                group.ledger_path,
                kind="chat.message",
                group_id=group.group_id,
                scope_key="",
                by="user",
                data={"text": "retry inserted but not submitted", "to": ["peer1"]},
            )
            os.environ["CCCC_WEB_MODEL_DELIVERY_MODE"] = "browser"
            calls: list[str] = []

            def submit_via_session(**kwargs) -> dict:
                calls.append(str(kwargs.get("delivery_id") or ""))
                if len(calls) == 1:
                    raise RuntimeError(
                        "ChatGPT prompt was inserted but did not submit; diagnostics={\"send_enabled_count\":0}"
                    )
                return self._projected_submit_result(delivery_id="delivery-after-retry")

            with (
                patch(
                    "cccc.daemon.actors.web_model_browser_session.submit_prompt_via_web_model_chatgpt_browser_session",
                    side_effect=submit_via_session,
                ),
                patch("cccc.daemon.actors.web_model_browser_session.close_web_model_chatgpt_browser_session") as close_session,
            ):
                result = submit_next_web_model_browser_turn(group.group_id, "peer1", trigger_event_id=event["id"])

            self.assertTrue(result.get("ok"), result)
            self.assertEqual(result.get("status"), "submitted")
            self.assertEqual(len(calls), 2)
            close_session.assert_called_once_with(group_id=group.group_id, actor_id="peer1")
            self.assertEqual(get_cursor(group, "peer1")[0], event["id"])
        finally:
            if old_mode is None:
                os.environ.pop("CCCC_WEB_MODEL_DELIVERY_MODE", None)
            else:
                os.environ["CCCC_WEB_MODEL_DELIVERY_MODE"] = old_mode
            cleanup()

    def test_browser_delivery_reseeds_legacy_bootstrap_marker_without_digest(self) -> None:
        from cccc.daemon.actors.web_model_browser_delivery import submit_next_web_model_browser_turn
        from cccc.kernel.ledger import append_event
        from cccc.ports.web_model_browser_sidecar import record_chatgpt_browser_state

        _, cleanup = self._with_home()
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
            os.environ["CCCC_WEB_MODEL_DELIVERY_MODE"] = "browser"
            calls: list[dict] = []

            def submit_via_session(**kwargs) -> dict:
                calls.append(dict(kwargs))
                return self._projected_submit_result(delivery_id="delivery-legacy")

            with patch(
                "cccc.daemon.actors.web_model_browser_session.submit_prompt_via_web_model_chatgpt_browser_session",
                side_effect=submit_via_session,
            ):
                result = submit_next_web_model_browser_turn(group.group_id, "peer1", trigger_event_id=event["id"])

            self.assertTrue(result.get("ok"), result)
            self.assertEqual(len(calls), 1)
            prompt = str(calls[0].get("prompt") or "")
            self.assertIn("Session bootstrap for this browser chat", prompt)
            self.assertIn("Platform Invariants:", prompt)
        finally:
            if old_mode is None:
                os.environ.pop("CCCC_WEB_MODEL_DELIVERY_MODE", None)
            else:
                os.environ["CCCC_WEB_MODEL_DELIVERY_MODE"] = old_mode
            cleanup()

    def test_browser_delivery_reseeds_after_headless_actor_restart(self) -> None:
        from cccc.daemon.actors import web_model_browser_delivery as delivery
        from cccc.daemon.runner_state_ops import write_headless_state
        from cccc.kernel.actors import find_actor
        from cccc.kernel.ledger import append_event
        from cccc.ports.web_model_browser_sidecar import record_chatgpt_browser_state

        _, cleanup = self._with_home()
        old_mode = os.environ.get("CCCC_WEB_MODEL_DELIVERY_MODE")
        try:
            group = self._create_group_with_actor()
            actor = find_actor(group, "peer1")
            seed_text = delivery._build_web_model_bootstrap_seed(group, actor or {})
            seed_digest = delivery._bootstrap_seed_digest(seed_text)
            record_chatgpt_browser_state(
                group.group_id,
                "peer1",
                {
                    "conversation_url": "https://chatgpt.com/c/test-chat",
                    "bootstrap_seed_delivered_at": "2000-01-01T00:00:00Z",
                    "bootstrap_seed_version": "web-model-bootstrap-normal-system-prompt-v2",
                    "bootstrap_seed_digest": seed_digest,
                    "bootstrap_seed_conversation_url": "https://chatgpt.com/c/test-chat",
                },
            )
            write_headless_state(group.group_id, "peer1")
            event = append_event(
                group.ledger_path,
                kind="chat.message",
                group_id=group.group_id,
                scope_key="",
                by="user",
                data={"text": "first message after restart should reseed", "to": ["peer1"]},
            )
            os.environ["CCCC_WEB_MODEL_DELIVERY_MODE"] = "browser"
            calls: list[dict] = []

            def submit_via_session(**kwargs) -> dict:
                calls.append(dict(kwargs))
                return self._projected_submit_result(delivery_id="delivery-restart-seed")

            with patch(
                "cccc.daemon.actors.web_model_browser_session.submit_prompt_via_web_model_chatgpt_browser_session",
                side_effect=submit_via_session,
            ):
                result = delivery.submit_next_web_model_browser_turn(group.group_id, "peer1", trigger_event_id=event["id"])

            self.assertTrue(result.get("ok"), result)
            self.assertEqual(len(calls), 1)
            prompt = str(calls[0].get("prompt") or "")
            self.assertIn("Session bootstrap for this browser chat", prompt)
            self.assertIn("first message after restart should reseed", prompt)
        finally:
            if old_mode is None:
                os.environ.pop("CCCC_WEB_MODEL_DELIVERY_MODE", None)
            else:
                os.environ["CCCC_WEB_MODEL_DELIVERY_MODE"] = old_mode
            cleanup()

    def test_browser_delivery_projected_session_failure_marks_turn_failed_without_redelivery(self) -> None:
        from cccc.daemon.actors.web_model_browser_delivery import submit_next_web_model_browser_turn
        from cccc.daemon.runner_state_ops import read_headless_state
        from cccc.kernel.inbox import unread_messages
        from cccc.kernel.inbox import get_cursor
        from cccc.kernel.ledger import append_event, read_last_lines
        from cccc.ports.web_model_browser_sidecar import read_chatgpt_browser_state

        _, cleanup = self._with_home()
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
            os.environ["CCCC_WEB_MODEL_DELIVERY_MODE"] = "browser"

            with patch(
                "cccc.daemon.actors.web_model_browser_session.submit_prompt_via_web_model_chatgpt_browser_session",
                side_effect=RuntimeError("browser unavailable"),
            ):
                result = submit_next_web_model_browser_turn(group.group_id, "peer1", trigger_event_id=event["id"])

            self.assertFalse(result.get("ok"), result)
            self.assertEqual(result.get("status"), "failed")
            self.assertTrue(result.get("cursor_committed"))
            self.assertEqual(get_cursor(group, "peer1")[0], event["id"])
            self.assertEqual(unread_messages(group, actor_id="peer1", limit=10, kind_filter="all"), [])
            state = read_headless_state(group.group_id, "peer1")
            self.assertEqual(str(state.get("status") or ""), "waiting")
            self.assertEqual(str(state.get("active_turn_id") or ""), "")
            browser_state = read_chatgpt_browser_state(group.group_id, "peer1")
            self.assertEqual(browser_state.get("last_delivery_status"), "failed")
            self.assertIn("browser unavailable", str(browser_state.get("last_error") or ""))
            failed_events = [
                json.loads(line)
                for line in read_last_lines(group.ledger_path, 20)
                if "web_model.browser_delivery.failed" in line
            ]
            self.assertTrue(failed_events)
            self.assertEqual((failed_events[-1].get("data") or {}).get("event_ids"), [event["id"]])
            self.assertEqual((failed_events[-1].get("data") or {}).get("cursor_committed"), True)

            second = append_event(
                group.ledger_path,
                kind="chat.message",
                group_id=group.group_id,
                scope_key="",
                by="user",
                data={"text": "manual retry after visible failure", "to": ["peer1"]},
            )
            prompts: list[str] = []

            def submit_success(**kwargs: object) -> dict:
                prompts.append(str(kwargs.get("prompt") or ""))
                return self._projected_submit_result(delivery_id="delivery-after-failure")

            with patch(
                "cccc.daemon.actors.web_model_browser_session.submit_prompt_via_web_model_chatgpt_browser_session",
                side_effect=submit_success,
            ):
                result2 = submit_next_web_model_browser_turn(group.group_id, "peer1", trigger_event_id=second["id"])

            self.assertTrue(result2.get("ok"), result2)
            self.assertEqual(result2.get("status"), "submitted")
            self.assertEqual(get_cursor(group, "peer1")[0], second["id"])
            self.assertEqual(len(prompts), 1)
            self.assertIn("manual retry after visible failure", prompts[0])
            self.assertNotIn("retry after failed browser delivery", prompts[0])
        finally:
            if old_mode is None:
                os.environ.pop("CCCC_WEB_MODEL_DELIVERY_MODE", None)
            else:
                os.environ["CCCC_WEB_MODEL_DELIVERY_MODE"] = old_mode
            cleanup()

    def test_browser_delivery_ambiguous_submit_commits_cursor_without_failed_status(self) -> None:
        from cccc.daemon.actors.web_model_browser_delivery import submit_next_web_model_browser_turn
        from cccc.daemon.runner_state_ops import read_headless_state
        from cccc.kernel.inbox import get_cursor, unread_messages
        from cccc.kernel.ledger import append_event, read_last_lines
        from cccc.ports.web_model_browser_sidecar import read_chatgpt_browser_state

        _, cleanup = self._with_home()
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
                data={"text": "possibly submitted message", "to": ["peer1"]},
            )
            os.environ["CCCC_WEB_MODEL_DELIVERY_MODE"] = "browser"

            with patch(
                "cccc.daemon.actors.web_model_browser_session.submit_prompt_via_web_model_chatgpt_browser_session",
                side_effect=RuntimeError(
                    "ChatGPT submit action was attempted but submission could not be verified; "
                    "submission_verification=ambiguous; attempted_action=#composer-submit-button"
                ),
            ):
                result = submit_next_web_model_browser_turn(group.group_id, "peer1", trigger_event_id=event["id"])

            self.assertFalse(result.get("ok"), result)
            self.assertEqual(result.get("status"), "ambiguous")
            self.assertTrue(result.get("cursor_committed"))
            self.assertEqual(get_cursor(group, "peer1")[0], event["id"])
            self.assertEqual(unread_messages(group, actor_id="peer1", limit=10, kind_filter="all"), [])
            state = read_headless_state(group.group_id, "peer1")
            self.assertEqual(str(state.get("status") or ""), "waiting")
            browser_state = read_chatgpt_browser_state(group.group_id, "peer1")
            self.assertEqual(browser_state.get("last_delivery_status"), "ambiguous")
            self.assertEqual(browser_state.get("last_submission_evidence"), "submit_unverified")
            ambiguous_events = [
                json.loads(line)
                for line in read_last_lines(group.ledger_path, 20)
                if "web_model.browser_delivery.ambiguous" in line
            ]
            self.assertTrue(ambiguous_events)
            self.assertEqual((ambiguous_events[-1].get("data") or {}).get("event_ids"), [event["id"]])
            self.assertEqual((ambiguous_events[-1].get("data") or {}).get("cursor_committed"), True)
        finally:
            if old_mode is None:
                os.environ.pop("CCCC_WEB_MODEL_DELIVERY_MODE", None)
            else:
                os.environ["CCCC_WEB_MODEL_DELIVERY_MODE"] = old_mode
            cleanup()

    def test_browser_delivery_weak_submit_evidence_is_unverified_not_submitted(self) -> None:
        from cccc.daemon.actors.web_model_browser_delivery import submit_next_web_model_browser_turn
        from cccc.kernel.inbox import get_cursor, unread_messages
        from cccc.kernel.ledger import append_event, read_last_lines
        from cccc.ports.web_model_browser_sidecar import read_chatgpt_browser_state

        _, cleanup = self._with_home()
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
                data={"text": "maybe submitted message", "to": ["peer1"]},
            )
            os.environ["CCCC_WEB_MODEL_DELIVERY_MODE"] = "browser"

            with patch(
                "cccc.daemon.actors.web_model_browser_session.submit_prompt_via_web_model_chatgpt_browser_session",
                return_value=self._projected_submit_result(
                    delivery_id="delivery-weak",
                    submission_evidence="running_without_echo",
                ),
            ):
                result = submit_next_web_model_browser_turn(group.group_id, "peer1", trigger_event_id=event["id"])

            self.assertFalse(result.get("ok"), result)
            self.assertEqual(result.get("status"), "ambiguous")
            self.assertTrue(result.get("cursor_committed"))
            self.assertEqual(get_cursor(group, "peer1")[0], event["id"])
            self.assertEqual(unread_messages(group, actor_id="peer1", limit=10, kind_filter="all"), [])
            browser_state = read_chatgpt_browser_state(group.group_id, "peer1")
            self.assertEqual(browser_state.get("last_delivery_status"), "ambiguous")
            self.assertEqual(browser_state.get("last_submission_evidence"), "running_without_echo")
            ambiguous_events = [
                json.loads(line)
                for line in read_last_lines(group.ledger_path, 20)
                if "web_model.browser_delivery.ambiguous" in line
            ]
            self.assertTrue(ambiguous_events)
            data = ambiguous_events[-1].get("data") or {}
            self.assertEqual(data.get("submission_evidence"), "running_without_echo")
            self.assertEqual((data.get("browser") or {}).get("submission_evidence"), "running_without_echo")
            self.assertEqual(data.get("cursor_committed"), True)
        finally:
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

        _, cleanup = self._with_home()
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
            os.environ["CCCC_WEB_MODEL_DELIVERY_MODE"] = "browser"
            calls: list[dict] = []

            def submit_via_session(**kwargs) -> dict:
                calls.append(dict(kwargs))
                return self._projected_submit_result(
                    delivery_id="delivery-new-chat",
                    tab_url="https://chatgpt.com/c/new-chat",
                    conversation_url="https://chatgpt.com/c/new-chat",
                )

            with patch(
                "cccc.daemon.actors.web_model_browser_session.submit_prompt_via_web_model_chatgpt_browser_session",
                side_effect=submit_via_session,
            ):
                result = submit_next_web_model_browser_turn(group.group_id, "peer1", trigger_event_id=event["id"])

            self.assertTrue(result.get("ok"), result)
            self.assertEqual(result.get("status"), "submitted")
            self.assertEqual(get_cursor(group, "peer1")[0], event["id"])
            self.assertEqual(calls[0].get("target_url"), "https://chatgpt.com/")
            self.assertEqual(calls[0].get("auto_bind_new_chat"), True)
            self.assertIn("Session bootstrap for this browser chat", str(calls[0].get("prompt") or ""))
            state = read_chatgpt_browser_state(group.group_id, "peer1")
            self.assertEqual(state.get("conversation_url"), "https://chatgpt.com/c/new-chat")
            self.assertEqual(state.get("pending_new_chat_bind"), False)
            self.assertEqual(state.get("pending_new_chat_url"), "")
            self.assertEqual(state.get("bootstrap_seed_conversation_url"), "https://chatgpt.com/c/new-chat")
        finally:
            if old_mode is None:
                os.environ.pop("CCCC_WEB_MODEL_DELIVERY_MODE", None)
            else:
                os.environ["CCCC_WEB_MODEL_DELIVERY_MODE"] = old_mode
            cleanup()

    def test_browser_delivery_new_chat_without_final_url_commits_delivered_turn(self) -> None:
        from cccc.daemon.actors.web_model_browser_delivery import submit_next_web_model_browser_turn
        from cccc.kernel.inbox import get_cursor
        from cccc.kernel.ledger import append_event, read_last_lines
        from cccc.ports.web_model_browser_sidecar import read_chatgpt_browser_state, record_chatgpt_browser_state

        _, cleanup = self._with_home()
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
            os.environ["CCCC_WEB_MODEL_DELIVERY_MODE"] = "browser"

            def submit_via_session(**kwargs) -> dict:
                return self._projected_submit_result(
                    delivery_id="delivery-no-url",
                    tab_url="https://chatgpt.com/",
                    conversation_url="",
                    pending_conversation_url=True,
                    submitted_without_conversation_url=True,
                )

            with (
                patch(
                    "cccc.daemon.actors.web_model_browser_session.submit_prompt_via_web_model_chatgpt_browser_session",
                    side_effect=submit_via_session,
                ),
                patch(
                    "cccc.daemon.actors.web_model_browser_recovery_watcher.ensure_web_model_browser_recovery_watcher",
                    return_value=True,
                ) as ensure_watcher,
            ):
                result = submit_next_web_model_browser_turn(group.group_id, "peer1", trigger_event_id=event["id"])

            self.assertTrue(result.get("ok"), result)
            self.assertEqual(result.get("status"), "target_chat_binding_pending")
            ensure_watcher.assert_called_once_with(group.group_id, "peer1", logger=ANY)
            self.assertTrue(result.get("cursor_committed"))
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
            self.assertEqual((data or {}).get("cursor_committed"), True)
            self.assertTrue(
                any("web_model.browser_delivery.pending" in line for line in read_last_lines(group.ledger_path, 20))
            )
        finally:
            if old_mode is None:
                os.environ.pop("CCCC_WEB_MODEL_DELIVERY_MODE", None)
            else:
                os.environ["CCCC_WEB_MODEL_DELIVERY_MODE"] = old_mode
            cleanup()

    def test_browser_delivery_new_chat_weak_evidence_is_ambiguous_not_pending_bind(self) -> None:
        from cccc.daemon.actors.web_model_browser_delivery import submit_next_web_model_browser_turn
        from cccc.kernel.inbox import get_cursor, unread_messages
        from cccc.kernel.ledger import append_event, read_last_lines
        from cccc.ports.web_model_browser_sidecar import read_chatgpt_browser_state, record_chatgpt_browser_state

        _, cleanup = self._with_home()
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
                data={"text": "new chat weak evidence", "to": ["peer1"]},
            )
            os.environ["CCCC_WEB_MODEL_DELIVERY_MODE"] = "browser"

            def submit_via_session(**kwargs) -> dict:
                return self._projected_submit_result(
                    delivery_id="delivery-new-chat-weak",
                    tab_url="https://chatgpt.com/",
                    conversation_url="",
                    pending_conversation_url=True,
                    submitted_without_conversation_url=True,
                    submission_evidence="running_without_echo",
                )

            with patch(
                "cccc.daemon.actors.web_model_browser_session.submit_prompt_via_web_model_chatgpt_browser_session",
                side_effect=submit_via_session,
            ):
                result = submit_next_web_model_browser_turn(group.group_id, "peer1", trigger_event_id=event["id"])

            self.assertFalse(result.get("ok"), result)
            self.assertEqual(result.get("status"), "ambiguous")
            self.assertTrue(result.get("cursor_committed"))
            self.assertEqual(get_cursor(group, "peer1")[0], event["id"])
            self.assertEqual(unread_messages(group, actor_id="peer1", limit=10, kind_filter="all"), [])
            state = read_chatgpt_browser_state(group.group_id, "peer1")
            self.assertEqual(state.get("pending_new_chat_bind"), True)
            self.assertNotEqual(state.get("pending_new_chat_submitted"), True)
            self.assertEqual(state.get("last_delivery_status"), "ambiguous")
            self.assertEqual(state.get("last_submission_evidence"), "running_without_echo")
            lines = read_last_lines(group.ledger_path, 30)
            self.assertFalse(any("web_model.browser_delivery.pending" in line for line in lines))
            self.assertTrue(any("web_model.browser_delivery.ambiguous" in line for line in lines))
        finally:
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

        _, cleanup = self._with_home()
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
            os.environ["CCCC_WEB_MODEL_DELIVERY_MODE"] = "browser"

            def first_submit(**kwargs) -> dict:
                return self._projected_submit_result(
                    delivery_id="delivery-pending-url",
                    tab_url="https://chatgpt.com/",
                    conversation_url="",
                    pending_conversation_url=True,
                    submitted_without_conversation_url=True,
                )

            with patch(
                "cccc.daemon.actors.web_model_browser_session.submit_prompt_via_web_model_chatgpt_browser_session",
                side_effect=first_submit,
            ):
                first_result = submit_next_web_model_browser_turn(group.group_id, "peer1", trigger_event_id=first["id"])
            self.assertTrue(first_result.get("ok"), first_result)
            self.assertEqual(first_result.get("status"), "target_chat_binding_pending")
            self.assertEqual(get_cursor(group, "peer1")[0], first["id"])
            first_state = read_chatgpt_browser_state(group.group_id, "peer1")
            self.assertEqual(first_state.get("bootstrap_seed_conversation_url"), "https://chatgpt.com/")

            record_chatgpt_browser_state(
                group.group_id,
                "peer1",
                {"pending_new_chat_last_tab_url": "https://chatgpt.com/c/delayed-chat"},
            )
            second = append_event(
                group.ledger_path,
                kind="chat.message",
                group_id=group.group_id,
                scope_key="",
                by="user",
                data={"text": "follow-up after URL exists", "to": ["peer1"]},
            )
            calls: list[dict] = []

            def second_submit(**kwargs) -> dict:
                calls.append(dict(kwargs))
                return self._projected_submit_result(
                    delivery_id="delivery-follow-up",
                    tab_url="https://chatgpt.com/c/delayed-chat",
                    conversation_url="https://chatgpt.com/c/delayed-chat",
                )

            with patch(
                "cccc.daemon.actors.web_model_browser_session.submit_prompt_via_web_model_chatgpt_browser_session",
                side_effect=second_submit,
            ):
                second_result = submit_next_web_model_browser_turn(group.group_id, "peer1", trigger_event_id=second["id"])

            self.assertTrue(second_result.get("ok"), second_result)
            self.assertEqual(get_cursor(group, "peer1")[0], second["id"])
            self.assertEqual(calls[0].get("target_url"), "https://chatgpt.com/c/delayed-chat")
            self.assertNotIn("Session bootstrap for this browser chat", str(calls[0].get("prompt") or ""))
            state = read_chatgpt_browser_state(group.group_id, "peer1")
            self.assertEqual(state.get("conversation_url"), "https://chatgpt.com/c/delayed-chat")
            self.assertEqual(state.get("bootstrap_seed_conversation_url"), "https://chatgpt.com/c/delayed-chat")
        finally:
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

        _, cleanup = self._with_home()
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
            os.environ["CCCC_WEB_MODEL_DELIVERY_MODE"] = "browser"

            with (
                patch(
                    "cccc.daemon.actors.web_model_browser_delivery.resolve_pending_chatgpt_conversation",
                    return_value={"ok": True, "resolved": False, "pending": True, "submitted": True},
                ) as resolve,
                patch(
                    "cccc.daemon.actors.web_model_browser_session.submit_prompt_via_web_model_chatgpt_browser_session",
                    side_effect=AssertionError("submit should not run while bind is pending"),
                ) as submit,
            ):
                result = submit_next_web_model_browser_turn(group.group_id, "peer1", trigger_event_id=event["id"])

            self.assertTrue(result.get("ok"), result)
            self.assertEqual(result.get("status"), "target_chat_binding_pending")
            self.assertEqual(get_cursor(group, "peer1"), ("", ""))
            resolve.assert_called_once_with(group.group_id, "peer1")
            submit.assert_not_called()
        finally:
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

        _, cleanup = self._with_home()
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
            os.environ["CCCC_WEB_MODEL_DELIVERY_MODE"] = "browser"
            calls: list[dict] = []

            def submit_via_session(**kwargs) -> dict:
                calls.append(dict(kwargs))
                return self._projected_submit_result(
                    delivery_id="delivery-retry",
                    tab_url="https://chatgpt.com/c/retry-chat",
                    conversation_url="https://chatgpt.com/c/retry-chat",
                )

            with (
                patch(
                    "cccc.daemon.actors.web_model_browser_delivery.resolve_pending_chatgpt_conversation",
                    return_value={"ok": True, "resolved": False, "pending": True, "submitted": True},
                ) as resolve,
                patch(
                    "cccc.daemon.actors.web_model_browser_session.submit_prompt_via_web_model_chatgpt_browser_session",
                    side_effect=submit_via_session,
                ),
            ):
                result = submit_next_web_model_browser_turn(group.group_id, "peer1", trigger_event_id=event["id"])

            self.assertTrue(result.get("ok"), result)
            self.assertEqual(result.get("status"), "submitted")
            self.assertEqual(get_cursor(group, "peer1")[0], event["id"])
            self.assertEqual(calls[0].get("target_url"), "https://chatgpt.com/")
            self.assertEqual(calls[0].get("auto_bind_new_chat"), True)
            self.assertIn("Session bootstrap for this browser chat", str(calls[0].get("prompt") or ""))
            state = read_chatgpt_browser_state(group.group_id, "peer1")
            self.assertEqual(state.get("conversation_url"), "https://chatgpt.com/c/retry-chat")
            self.assertEqual(state.get("pending_new_chat_bind"), False)
            resolve.assert_called_once_with(group.group_id, "peer1")
        finally:
            if old_mode is None:
                os.environ.pop("CCCC_WEB_MODEL_DELIVERY_MODE", None)
            else:
                os.environ["CCCC_WEB_MODEL_DELIVERY_MODE"] = old_mode
            cleanup()

    def test_browser_delivery_resolves_pending_new_chat_then_delivers_to_bound_url(self) -> None:
        from cccc.daemon.actors.web_model_browser_delivery import submit_next_web_model_browser_turn
        from cccc.kernel.inbox import get_cursor, set_cursor
        from cccc.kernel.ledger import append_event, read_last_lines
        from cccc.ports.web_model_browser_sidecar import read_chatgpt_browser_state, record_chatgpt_browser_state

        _, cleanup = self._with_home()
        old_mode = os.environ.get("CCCC_WEB_MODEL_DELIVERY_MODE")
        try:
            group = self._create_group_with_actor()
            pending_event = append_event(
                group.ledger_path,
                kind="chat.message",
                group_id=group.group_id,
                scope_key="",
                by="user",
                data={"text": "first message creates a new ChatGPT chat", "to": ["peer1"]},
            )
            set_cursor(group, "peer1", event_id=str(pending_event["id"]), ts=str(pending_event["ts"]))
            record_chatgpt_browser_state(
                group.group_id,
                "peer1",
                {
                    "pending_new_chat_bind": True,
                    "pending_new_chat_url": "https://chatgpt.com/",
                    "pending_new_chat_submitted": True,
                    "pending_new_chat_delivery_id": "delivery-pending",
                    "pending_new_chat_last_turn_id": "turn-pending",
                    "pending_new_chat_last_event_ids": [pending_event["id"]],
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
            os.environ["CCCC_WEB_MODEL_DELIVERY_MODE"] = "browser"
            calls: list[dict] = []

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
                return {
                    "ok": True,
                    "resolved": True,
                    "conversation_url": "https://chatgpt.com/c/resolved",
                    "target_url": "https://chatgpt.com/",
                    "delivery_id": "delivery-pending",
                    "turn_id": "turn-pending",
                    "event_ids": [pending_event["id"]],
                    "bound_at": "2026-05-01T00:00:20Z",
                }

            def submit_via_session(**kwargs) -> dict:
                calls.append(dict(kwargs))
                return self._projected_submit_result(
                    delivery_id="delivery-resolved",
                    tab_url="https://chatgpt.com/c/resolved",
                    conversation_url="https://chatgpt.com/c/resolved",
                )

            with (
                patch("cccc.daemon.actors.web_model_browser_delivery.resolve_pending_chatgpt_conversation", side_effect=resolve),
                patch(
                    "cccc.daemon.actors.web_model_browser_session.submit_prompt_via_web_model_chatgpt_browser_session",
                    side_effect=submit_via_session,
                ),
            ):
                result = submit_next_web_model_browser_turn(group.group_id, "peer1", trigger_event_id=event["id"])

            self.assertTrue(result.get("ok"), result)
            self.assertEqual(result.get("status"), "submitted")
            self.assertEqual(get_cursor(group, "peer1")[0], event["id"])
            self.assertEqual(calls[0].get("target_url"), "https://chatgpt.com/c/resolved")
            self.assertEqual(calls[0].get("auto_bind_new_chat"), False)
            state = read_chatgpt_browser_state(group.group_id, "peer1")
            self.assertEqual(state.get("conversation_url"), "https://chatgpt.com/c/resolved")
            self.assertEqual(state.get("pending_new_chat_bind"), False)
            lines = [json.loads(line) for line in read_last_lines(group.ledger_path, 20) if line.strip()]
            bound_events = [line for line in lines if line.get("kind") == "web_model.browser_delivery.bound"]
            self.assertEqual(len(bound_events), 1)
            self.assertEqual((bound_events[-1].get("data") or {}).get("event_ids"), [pending_event["id"]])
            self.assertEqual((bound_events[-1].get("data") or {}).get("delivery_id"), "delivery-pending")
            self.assertEqual((bound_events[-1].get("data") or {}).get("bound_conversation_url"), "https://chatgpt.com/c/resolved")
        finally:
            if old_mode is None:
                os.environ.pop("CCCC_WEB_MODEL_DELIVERY_MODE", None)
            else:
                os.environ["CCCC_WEB_MODEL_DELIVERY_MODE"] = old_mode
            cleanup()

    def test_pending_new_chat_bound_event_append_is_idempotent(self) -> None:
        from cccc.daemon.actors.web_model_browser_delivery import append_pending_new_chat_bound_event
        from cccc.kernel.ledger import append_event, read_last_lines

        _, cleanup = self._with_home()
        try:
            group = self._create_group_with_actor()
            pending_event = append_event(
                group.ledger_path,
                kind="chat.message",
                group_id=group.group_id,
                scope_key="",
                by="user",
                data={"text": "first message creates a new ChatGPT chat", "to": ["peer1"]},
            )
            resolution = {
                "ok": True,
                "resolved": True,
                "conversation_url": "https://chatgpt.com/c/resolved",
                "target_url": "https://chatgpt.com/",
                "delivery_id": "delivery-pending",
                "turn_id": "turn-pending",
                "event_ids": [pending_event["id"]],
                "submitted_at": "2026-05-01T00:00:10Z",
                "bound_at": "2026-05-01T00:00:20Z",
            }

            first = append_pending_new_chat_bound_event(group.group_id, "peer1", resolution)
            second = append_pending_new_chat_bound_event(group.group_id, "peer1", resolution)

            self.assertTrue(first)
            self.assertTrue(second)
            self.assertEqual(first.get("id"), second.get("id"))
            lines = [json.loads(line) for line in read_last_lines(group.ledger_path, 20) if line.strip()]
            bound_events = [
                line for line in lines
                if line.get("kind") == "web_model.browser_delivery.bound"
                and ((line.get("data") or {}).get("delivery_id") == "delivery-pending")
            ]
            self.assertEqual(len(bound_events), 1)
            self.assertEqual((bound_events[0].get("data") or {}).get("event_ids"), [pending_event["id"]])
            self.assertEqual((bound_events[0].get("data") or {}).get("bound_conversation_url"), "https://chatgpt.com/c/resolved")
        finally:
            cleanup()

    def test_browser_delivery_retries_stale_active_turn_before_next_delivery(self) -> None:
        from cccc.daemon.actors.web_model_browser_delivery import submit_next_web_model_browser_turn
        from cccc.kernel.inbox import get_cursor
        from cccc.kernel.ledger import append_event

        _, cleanup = self._with_home()
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
            os.environ["CCCC_WEB_MODEL_DELIVERY_MODE"] = "browser"
            calls: list[dict] = []

            def submit_via_session(**kwargs) -> dict:
                calls.append(dict(kwargs))
                return self._projected_submit_result(delivery_id="delivery-2")

            with patch(
                "cccc.daemon.actors.web_model_browser_session.submit_prompt_via_web_model_chatgpt_browser_session",
                side_effect=submit_via_session,
            ):
                result = submit_next_web_model_browser_turn(group.group_id, "peer1", trigger_event_id=new_event["id"])

            self.assertTrue(result.get("ok"), result)
            self.assertEqual(get_cursor(group, "peer1")[0], new_event["id"])
            prompt = str(calls[0].get("prompt") or "")
            self.assertIn(f"events={old_event['id']},{new_event['id']}", prompt)
        finally:
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

    def test_complete_turn_closes_browser_auto_reload_window(self) -> None:
        from cccc.kernel.ledger import append_event
        from cccc.ports.web_model_browser_sidecar import read_chatgpt_browser_state, record_chatgpt_browser_state

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
                data={"text": "complete closes reload", "to": ["peer1"]},
            )
            record_chatgpt_browser_state(
                group.group_id,
                "peer1",
                {
                    "auto_reload_active": True,
                    "auto_reload_window_started_at": "2026-05-03T00:00:00Z",
                    "auto_reload_window_expires_at": "2099-01-01T00:00:00Z",
                    "auto_reload_last_progress_at": "2026-05-03T00:00:00Z",
                },
            )

            complete, _ = self._call(
                "web_model_runtime_complete_turn",
                {
                    "group_id": group.group_id,
                    "actor_id": "peer1",
                    "by": "peer1",
                    "turn_id": "turn-1",
                    "event_ids": [event["id"]],
                    "status": "done",
                },
            )

            self.assertTrue(complete.ok, getattr(complete, "error", None))
            state = read_chatgpt_browser_state(group.group_id, "peer1")
            self.assertEqual(state.get("auto_reload_active"), False)
            self.assertEqual(state.get("auto_reload_completed_reason"), "complete_turn:done")
            self.assertEqual(state.get("auto_reload_last_progress_detail"), "turn-1")
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
