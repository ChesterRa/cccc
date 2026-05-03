import io
import json
import os
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch


class TestWebModelBrowserSidecar(unittest.TestCase):
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

    def test_browser_delivery_uses_builtin_sidecar_for_chatgpt_provider(self) -> None:
        from cccc.daemon.actors.web_model_browser_delivery import (
            resolve_web_model_browser_delivery_command,
            web_model_browser_delivery_enabled,
        )

        actor = {
            "id": "peer1",
            "runtime": "web_model",
            "runner": "headless",
            "web_model_provider": "chatgpt_web",
        }

        self.assertTrue(web_model_browser_delivery_enabled("g-test", actor))
        self.assertEqual(
            resolve_web_model_browser_delivery_command(actor),
            [sys.executable, "-m", "cccc.ports.web_model_browser_sidecar"],
        )

    def test_browser_delivery_pull_mode_disables_builtin_sidecar(self) -> None:
        from cccc.daemon.actors.web_model_browser_delivery import web_model_browser_delivery_enabled

        actor = {
            "id": "peer1",
            "runtime": "web_model",
            "runner": "headless",
            "web_model_provider": "chatgpt_web",
            "web_model_delivery_mode": "pull",
        }

        self.assertFalse(web_model_browser_delivery_enabled("g-test", actor))

    def test_sidecar_dry_run_validates_stdin_payload(self) -> None:
        from cccc.ports import web_model_browser_sidecar

        payload = {
            "schema": "cccc.web_model_browser_delivery.v1",
            "action": "submit_turn",
            "provider": "chatgpt_web",
            "group_id": "g-test",
            "actor_id": "peer1",
            "turn_id": "turn-1",
            "event_ids": ["evt-1"],
            "prompt": "Use CCCC MCP tools.",
        }
        stdin = io.StringIO(json.dumps(payload))
        stdout = io.StringIO()

        with patch.object(sys, "stdin", stdin), patch.object(sys, "stdout", stdout):
            code = web_model_browser_sidecar.main(["--dry-run"])

        self.assertEqual(code, 0)
        out = json.loads(stdout.getvalue())
        self.assertTrue(out.get("ok"))
        self.assertEqual((out.get("browser") or {}).get("dry_run"), True)
        self.assertEqual((out.get("browser") or {}).get("turn_id"), "turn-1")

    def test_sidecar_returns_error_for_invalid_schema(self) -> None:
        from cccc.ports.web_model_browser_sidecar import run_payload

        result = run_payload({"schema": "bad", "action": "submit_turn", "prompt": "x"}, dry_run=True)

        self.assertFalse(result.get("ok"))
        self.assertIn("schema", str(result.get("error") or ""))

    def test_browser_launch_command_supports_background_xvfb_mode(self) -> None:
        from cccc.ports.web_model_browser_sidecar import _browser_launch_command

        with patch("cccc.ports.web_model_browser_sidecar.shutil.which", return_value="/usr/bin/xvfb-run"):
            cmd = _browser_launch_command("/usr/bin/google-chrome", Path("/tmp/profile"), 9222, "background")

        self.assertEqual(cmd[:4], ["/usr/bin/xvfb-run", "-a", "-s", "-screen 0 1280x900x24"])
        self.assertIn("/usr/bin/google-chrome", cmd)
        self.assertIn("--remote-debugging-port=9222", cmd)

    def test_delivery_visibility_defaults_to_background_on_linux_when_xvfb_exists(self) -> None:
        from cccc.ports import web_model_browser_sidecar as sidecar

        with (
            patch.object(sidecar.sys, "platform", "linux"),
            patch.object(sidecar.shutil, "which", return_value="/usr/bin/xvfb-run"),
        ):
            self.assertEqual(sidecar._default_delivery_visibility(), "background")

    def test_chatgpt_conversation_url_normalization_strips_query(self) -> None:
        from cccc.ports.web_model_browser_sidecar import _conversation_url_from_tab

        self.assertEqual(
            _conversation_url_from_tab("https://chatgpt.com/c/abc123?model=gpt-5"),
            "https://chatgpt.com/c/abc123",
        )
        self.assertEqual(
            _conversation_url_from_tab("https://chatgpt.com/g/g-test/c/abc123?model=gpt-5"),
            "https://chatgpt.com/g/g-test/c/abc123",
        )
        self.assertEqual(
            _conversation_url_from_tab("https://chatgpt.com:443/c/abc123?model=gpt-5"),
            "https://chatgpt.com/c/abc123",
        )
        self.assertEqual(_conversation_url_from_tab("https://chatgpt.com/"), "")
        self.assertEqual(_conversation_url_from_tab("http://chatgpt.com/c/abc123"), "")
        self.assertEqual(_conversation_url_from_tab("https://evilchatgpt.com/c/abc123"), "")
        self.assertEqual(_conversation_url_from_tab("https://chatgpt.com.evil.test/c/abc123"), "")

    def test_submission_wait_accepts_composer_clear_after_send_attempt(self) -> None:
        from cccc.ports import web_model_browser_sidecar as sidecar

        class _Locator:
            first = None

            def __init__(self) -> None:
                self.first = self

            def is_visible(self, timeout: int = 0) -> bool:
                return False

        class _Page:
            def locator(self, selector: str) -> _Locator:
                return _Locator()

        with (
            patch.object(sidecar, "_composer_text", return_value=""),
            patch.object(sidecar, "_submission_echo_found", return_value=False),
        ):
            self.assertEqual(
                sidecar._wait_for_submission(
                    _Page(),
                    "#prompt-textarea",
                    prompt="hello",
                    timeout_seconds=1.0,
                    accept_composer_clear=True,
                ),
                "composer_cleared",
            )

    def test_composer_text_reads_contenteditable_via_dom_evaluate(self) -> None:
        from cccc.ports import web_model_browser_sidecar as sidecar

        class _Locator:
            first = None

            def __init__(self) -> None:
                self.first = self

            def evaluate(self, *_args, **_kwargs) -> str:
                return "Inserted prompt"

            def input_value(self, *_args, **_kwargs) -> str:  # pragma: no cover - should not be reached
                raise AssertionError("input_value fallback should not be needed")

        class _Page:
            def locator(self, selector: str) -> _Locator:
                self.selector = selector
                return _Locator()

        page = _Page()

        self.assertEqual(sidecar._composer_text(page, "#prompt-textarea"), "Inserted prompt")
        self.assertEqual(page.selector, "#prompt-textarea")

    def test_submit_prompt_waits_for_delayed_insert_before_clicking_send(self) -> None:
        from cccc.ports import web_model_browser_sidecar as sidecar

        page = object()
        prompt = "Browser-delivered CCCC message"

        with (
            patch.object(sidecar, "_visible_input_selector", return_value="#prompt-textarea"),
            patch.object(sidecar, "_clear_and_type_prompt") as clear_prompt,
            patch.object(sidecar, "_composer_text", side_effect=["", "", prompt]),
            patch.object(sidecar, "_click_send", return_value="#composer-submit-button") as click_send,
            patch.object(sidecar, "_wait_for_submission", return_value="stop_button") as wait_for_submission,
            patch.object(sidecar.time, "sleep", return_value=None),
        ):
            result = sidecar._submit_prompt(page, prompt, input_timeout_seconds=1.0)

        clear_prompt.assert_called_once_with(page, "#prompt-textarea", prompt)
        click_send.assert_called_once_with(page)
        wait_for_submission.assert_called_once()
        self.assertEqual(result.get("send_selector"), "#composer-submit-button")
        self.assertEqual(result.get("submission_evidence"), "stop_button")

    def test_chatgpt_profile_is_shared_while_actor_state_stays_separate(self) -> None:
        from cccc.ports.web_model_browser_sidecar import (
            chatgpt_browser_profile_dir,
            read_chatgpt_browser_state,
            record_chatgpt_browser_state,
        )

        _, cleanup = self._with_home()
        try:
            profile_a = chatgpt_browser_profile_dir("g-one", "peer1")
            profile_b = chatgpt_browser_profile_dir("g-two", "peer2")

            self.assertEqual(profile_a, profile_b)
            (profile_a / "login-cookie-marker").write_text("shared", encoding="utf-8")
            self.assertTrue((profile_b / "login-cookie-marker").exists())

            record_chatgpt_browser_state("g-one", "peer1", {"conversation_url": "https://chatgpt.com/c/a"})
            record_chatgpt_browser_state("g-two", "peer2", {"conversation_url": "https://chatgpt.com/c/b"})

            self.assertEqual(read_chatgpt_browser_state("g-one", "peer1").get("conversation_url"), "https://chatgpt.com/c/a")
            self.assertEqual(read_chatgpt_browser_state("g-two", "peer2").get("conversation_url"), "https://chatgpt.com/c/b")
        finally:
            cleanup()

    def test_chatgpt_shared_profile_migrates_existing_actor_profile(self) -> None:
        from cccc.ports.web_model_browser_sidecar import chatgpt_browser_actor_state_root, chatgpt_browser_profile_dir

        _, cleanup = self._with_home()
        try:
            legacy = chatgpt_browser_actor_state_root("g-old", "web_model-1") / "chrome_profile"
            legacy.mkdir(parents=True, exist_ok=True)
            (legacy / "legacy-login-marker").write_text("migrate", encoding="utf-8")

            migrated = chatgpt_browser_profile_dir("g-new", "chatgpt-web-1")

            self.assertTrue((migrated / "legacy-login-marker").exists())
        finally:
            cleanup()

    def test_resolve_pending_chatgpt_conversation_binds_from_state_url(self) -> None:
        from cccc.ports.web_model_browser_sidecar import (
            read_chatgpt_browser_state,
            record_chatgpt_browser_state,
            resolve_pending_chatgpt_conversation,
        )

        _, cleanup = self._with_home()
        try:
            record_chatgpt_browser_state(
                "g-test",
                "peer1",
                {
                    "pending_new_chat_bind": True,
                    "pending_new_chat_url": "https://chatgpt.com/",
                    "pending_new_chat_bind_started_at": "2026-05-01T00:00:00Z",
                    "pending_new_chat_submitted": True,
                    "pending_new_chat_submitted_at": "2026-05-01T00:00:10Z",
                    "pending_new_chat_delivery_id": "browser:test",
                    "pending_new_chat_last_turn_id": "turn-1",
                    "pending_new_chat_last_event_ids": ["evt-1"],
                    "last_tab_url": "https://chatgpt.com/c/newly-created?model=gpt-5",
                    "bootstrap_seed_delivered_at": "2026-05-01T00:00:11Z",
                    "bootstrap_seed_conversation_url": "https://chatgpt.com/",
                },
            )

            result = resolve_pending_chatgpt_conversation("g-test", "peer1")

            self.assertTrue(result.get("ok"), result)
            self.assertTrue(result.get("resolved"), result)
            self.assertEqual(result.get("conversation_url"), "https://chatgpt.com/c/newly-created")
            state = read_chatgpt_browser_state("g-test", "peer1")
            self.assertEqual(state.get("conversation_url"), "https://chatgpt.com/c/newly-created")
            self.assertEqual(state.get("pending_new_chat_bind"), False)
            self.assertEqual(state.get("pending_new_chat_submitted"), False)
            self.assertEqual(state.get("pending_new_chat_delivery_id"), "")
            self.assertEqual(state.get("bootstrap_seed_conversation_url"), "https://chatgpt.com/c/newly-created")
        finally:
            cleanup()

    def test_resolve_pending_chatgpt_conversation_ignores_stale_tab_before_submit(self) -> None:
        from cccc.ports.web_model_browser_sidecar import (
            read_chatgpt_browser_state,
            record_chatgpt_browser_state,
            resolve_pending_chatgpt_conversation,
        )

        _, cleanup = self._with_home()
        try:
            record_chatgpt_browser_state(
                "g-test",
                "peer1",
                {
                    "conversation_url": "",
                    "pending_new_chat_bind": True,
                    "pending_new_chat_url": "https://chatgpt.com/",
                    "pending_new_chat_bind_started_at": "2026-05-01T00:00:00Z",
                    "pending_new_chat_submitted": False,
                    "last_tab_url": "https://chatgpt.com/c/old-chat",
                },
            )

            result = resolve_pending_chatgpt_conversation("g-test", "peer1")

            self.assertTrue(result.get("ok"), result)
            self.assertFalse(result.get("resolved"), result)
            self.assertTrue(result.get("pending"), result)
            self.assertFalse(result.get("submitted"), result)
            state = read_chatgpt_browser_state("g-test", "peer1")
            self.assertEqual(state.get("conversation_url"), "")
            self.assertEqual(state.get("pending_new_chat_bind"), True)
            self.assertEqual(state.get("pending_new_chat_url"), "https://chatgpt.com/")
        finally:
            cleanup()

    def test_browser_launch_command_supports_true_headless_opt_in(self) -> None:
        from cccc.ports.web_model_browser_sidecar import _browser_launch_command

        cmd = _browser_launch_command("/usr/bin/google-chrome", Path("/tmp/profile"), 9222, "headless")

        self.assertEqual(cmd[0], "/usr/bin/google-chrome")
        self.assertIn("--headless=new", cmd)
        self.assertIn("--disable-gpu", cmd)

    def test_tool_confirm_matcher_script_targets_chatgpt_tool_confirm_panel(self) -> None:
        from cccc.ports.web_model_browser_sidecar import _chatgpt_tool_confirm_script

        script = _chatgpt_tool_confirm_script()

        self.assertIn("共享数据包括", script)
        self.assertIn("详细信息", script)
        self.assertIn("btn-primary", script)
        self.assertIn("btn-secondary", script)
        self.assertIn('querySelector("h2")', script)
        self.assertIn('querySelector("p")', script)
        self.assertIn("hasDetailsControl", script)
        self.assertIn("hasSharedDataText(root)", script)
        self.assertIn("data-cccc-auto-confirm-candidate-id", script)
        self.assertNotIn("confirmLabels", script)

    def test_auto_confirm_page_tool_prompts_skips_non_chatgpt_pages(self) -> None:
        from cccc.ports.web_model_browser_sidecar import _auto_confirm_page_tool_prompts

        class FakePage:
            url = "https://evilchatgpt.com/c/test-chat"

            def evaluate(self, *_args, **_kwargs):  # pragma: no cover - should not be called
                raise AssertionError("evaluate should not run for non-ChatGPT pages")

        result = _auto_confirm_page_tool_prompts(FakePage())

        self.assertEqual(result.get("clicked"), 0)
        self.assertEqual(result.get("skipped"), "non_chatgpt_page")

    def test_auto_confirm_page_tool_prompts_uses_dom_script(self) -> None:
        from cccc.ports.web_model_browser_sidecar import _auto_confirm_page_tool_prompts

        class FakePage:
            url = "https://chatgpt.com/c/test-chat"

            def __init__(self):
                self.args = None
                self.script = ""
                self.clicked = False

            def evaluate(self, script, args):
                self.script = str(script)
                self.args = args
                return {
                    "clicked": 0,
                    "candidates": [
                        {
                            "candidate_id": "cand-1",
                            "title": "Delete docs?",
                            "label": "确认",
                        }
                    ],
                }

            def locator(self, selector):
                self.selector = selector

                class FakeLocator:
                    @property
                    def first(self):
                        return self

                    def count(self):
                        return 1

                    def click(self, timeout=0):
                        page.clicked = True

                page = self
                return FakeLocator()

        page = FakePage()
        result = _auto_confirm_page_tool_prompts(page, max_clicks=2)

        self.assertEqual(result.get("clicked"), 1)
        self.assertEqual(result.get("candidate_count"), 1)
        self.assertEqual((result.get("details") or [{}])[0].get("title"), "Delete docs?")
        self.assertEqual(page.args, {"maxClicks": 2})
        self.assertTrue(page.clicked)
        self.assertIn('button[data-cccc-auto-confirm-candidate-id="cand-1"]', page.selector)
        self.assertIn("shared data", page.script)

    def test_session_status_action_does_not_require_prompt(self) -> None:
        from cccc.ports.web_model_browser_sidecar import run_payload

        with patch("cccc.ports.web_model_browser_sidecar.chatgpt_browser_session_status", return_value={"active": False}):
            result = run_payload(
                {
                    "schema": "cccc.web_model_browser_delivery.v1",
                    "action": "session_status",
                    "group_id": "g-test",
                    "actor_id": "peer1",
                },
                dry_run=True,
            )

        self.assertTrue(result.get("ok"))
        self.assertEqual((result.get("browser") or {}).get("active"), False)

    def test_background_delivery_reuses_active_projected_browser(self) -> None:
        from cccc.ports import web_model_browser_sidecar as sidecar

        _, cleanup = self._with_home()
        try:
            root = sidecar.chatgpt_browser_profile_root("g-test", "peer1")
            sidecar.record_chatgpt_browser_process_state(
                {
                    "pid": 1234,
                    "cdp_port": 9222,
                    "visibility": "projected",
                    "profile_dir": str(sidecar.chatgpt_browser_profile_dir("g-test", "peer1")),
                },
            )

            with patch.object(sidecar, "_wait_cdp_endpoint", return_value=True), patch.object(sidecar, "_stop_browser_state") as stop:
                state = sidecar._start_or_reuse_browser(root, visibility="background")

            self.assertEqual(state.get("cdp_port"), 9222)
            self.assertEqual(state.get("visibility"), "projected")
            self.assertTrue(state.get("reused"))
            stop.assert_not_called()
        finally:
            cleanup()

    def test_projected_chatgpt_session_requires_system_browser_cdp(self) -> None:
        from cccc.daemon.actors import web_model_browser_session

        _, cleanup = self._with_home()
        try:
            with (
                patch.object(web_model_browser_session._MANAGER, "open", return_value={"active": True, "metadata": {}}) as open_session,
                patch.object(web_model_browser_session, "close_chatgpt_browser_session", return_value={"active": False}) as close_browser,
            ):
                web_model_browser_session.open_web_model_chatgpt_browser_session(
                    group_id="g-test",
                    actor_id="peer1",
                    width=1280,
                    height=800,
                )

            kwargs = open_session.call_args.kwargs
            self.assertEqual(kwargs.get("key"), "chatgpt_web")
            self.assertEqual(tuple(kwargs.get("channel_candidates") or ()), ("chrome", "msedge"))
            self.assertEqual(kwargs.get("system_profile_subdir"), "")
            self.assertEqual(kwargs.get("require_system_browser_cdp"), True)
            close_browser.assert_called_once_with("g-test", "peer1")
        finally:
            cleanup()

    def test_projected_chatgpt_session_opens_bound_conversation(self) -> None:
        from cccc.daemon.actors import web_model_browser_session
        from cccc.ports.web_model_browser_sidecar import record_chatgpt_browser_state

        _, cleanup = self._with_home()
        try:
            record_chatgpt_browser_state(
                "g-test",
                "peer1",
                {
                    "conversation_url": "https://chatgpt.com/c/bound-chat?model=gpt-5",
                    "last_tab_url": "https://chatgpt.com/",
                },
            )
            with (
                patch.object(web_model_browser_session._MANAGER, "open", return_value={"active": True, "metadata": {}}) as open_session,
                patch.object(web_model_browser_session, "close_chatgpt_browser_session", return_value={"active": False}),
            ):
                web_model_browser_session.open_web_model_chatgpt_browser_session(
                    group_id="g-test",
                    actor_id="peer1",
                    width=1280,
                    height=800,
                )

            self.assertEqual(open_session.call_args.kwargs.get("url"), "https://chatgpt.com/c/bound-chat")
        finally:
            cleanup()

    def test_projected_chatgpt_session_opens_new_chat_when_armed(self) -> None:
        from cccc.daemon.actors import web_model_browser_session
        from cccc.ports.web_model_browser_sidecar import record_chatgpt_browser_state

        _, cleanup = self._with_home()
        try:
            record_chatgpt_browser_state(
                "g-test",
                "peer1",
                {
                    "conversation_url": "https://chatgpt.com/c/old-chat",
                    "pending_new_chat_bind": True,
                    "pending_new_chat_url": "https://chatgpt.com/",
                    "last_tab_url": "https://chatgpt.com/c/old-chat",
                },
            )
            with (
                patch.object(web_model_browser_session._MANAGER, "open", return_value={"active": True, "metadata": {}}) as open_session,
                patch.object(web_model_browser_session, "close_chatgpt_browser_session", return_value={"active": False}),
            ):
                web_model_browser_session.open_web_model_chatgpt_browser_session(
                    group_id="g-test",
                    actor_id="peer1",
                    width=1280,
                    height=800,
                )

            self.assertEqual(open_session.call_args.kwargs.get("url"), "https://chatgpt.com/")
        finally:
            cleanup()

    def test_projected_chatgpt_session_reuses_active_instance(self) -> None:
        from cccc.daemon.actors import web_model_browser_session

        _, cleanup = self._with_home()
        try:
            existing = {
                "active": True,
                "state": "ready",
                "url": "https://chatgpt.com/c/current-chat",
                "metadata": {"cdp_port": 9222},
            }
            with (
                patch.object(web_model_browser_session._MANAGER, "info", return_value=existing),
                patch.object(web_model_browser_session._MANAGER, "open", return_value={"active": True}) as open_session,
                patch.object(web_model_browser_session, "close_chatgpt_browser_session") as close_browser,
                patch.object(web_model_browser_session, "ensure_web_model_tool_confirm_watcher", return_value=True),
            ):
                result = web_model_browser_session.open_web_model_chatgpt_browser_session(
                    group_id="g-test",
                    actor_id="peer1",
                    width=1280,
                    height=800,
                )

            self.assertEqual(result, existing)
            open_session.assert_not_called()
            close_browser.assert_not_called()
        finally:
            cleanup()

    def test_projected_chatgpt_session_adopts_existing_shared_cdp_process(self) -> None:
        from cccc.daemon.actors import web_model_browser_session
        from cccc.ports import web_model_browser_sidecar as sidecar

        _, cleanup = self._with_home()
        try:
            profile_dir = sidecar.chatgpt_browser_profile_dir("g-test", "peer1")
            sidecar.record_chatgpt_browser_process_state(
                {
                    "pid": 1234,
                    "cdp_port": 9222,
                    "profile_dir": str(profile_dir),
                    "visibility": "projected",
                    "browser_binary": "/usr/bin/google-chrome",
                    "started_at": "2026-05-03T00:00:00Z",
                }
            )
            opened = {
                "active": True,
                "state": "ready",
                "url": "https://chatgpt.com/c/current-chat",
                "started_at": "2026-05-03T00:01:00Z",
                "metadata": {
                    "cdp_port": 9222,
                    "pid": 1234,
                    "profile_dir": str(profile_dir),
                    "adopted": True,
                },
            }
            with (
                patch.object(web_model_browser_session._MANAGER, "info", return_value={"active": False, "state": "idle"}),
                patch.object(web_model_browser_session, "_wait_cdp_endpoint", return_value=True),
                patch.object(web_model_browser_session._MANAGER, "open", return_value=opened) as open_session,
                patch.object(web_model_browser_session, "close_chatgpt_browser_session") as close_browser,
                patch.object(web_model_browser_session, "ensure_web_model_tool_confirm_watcher", return_value=True),
            ):
                result = web_model_browser_session.open_web_model_chatgpt_browser_session(
                    group_id="g-test",
                    actor_id="peer1",
                    width=1280,
                    height=800,
                )

            self.assertEqual(result, opened)
            kwargs = open_session.call_args.kwargs
            self.assertEqual(kwargs.get("existing_cdp_port"), 9222)
            self.assertEqual((kwargs.get("existing_browser_metadata") or {}).get("pid"), 1234)
            close_browser.assert_not_called()
        finally:
            cleanup()

    def test_projected_chatgpt_session_is_global_across_actor_ids(self) -> None:
        from cccc.daemon.actors import web_model_browser_session

        _, cleanup = self._with_home()
        try:
            with (
                patch.object(web_model_browser_session._MANAGER, "info", return_value={"active": False, "state": "idle"}),
                patch.object(web_model_browser_session._MANAGER, "open", return_value={"active": True, "metadata": {}}) as open_session,
                patch.object(web_model_browser_session, "close_chatgpt_browser_session", return_value={"active": False}),
            ):
                web_model_browser_session.open_web_model_chatgpt_browser_session(
                    group_id="g-one",
                    actor_id="peer1",
                    width=1280,
                    height=800,
                )
                web_model_browser_session.open_web_model_chatgpt_browser_session(
                    group_id="g-two",
                    actor_id="peer2",
                    width=1280,
                    height=800,
                )

            keys = [call.kwargs.get("key") for call in open_session.call_args_list]
            self.assertEqual(keys, ["chatgpt_web", "chatgpt_web"])
        finally:
            cleanup()

    def test_clear_web_model_actor_runtime_keeps_global_browser_open(self) -> None:
        from cccc.daemon.actors import web_model_browser_session
        from cccc.ports.web_model_browser_sidecar import read_chatgpt_browser_state, record_chatgpt_browser_state

        _, cleanup = self._with_home()
        try:
            record_chatgpt_browser_state(
                "g-test",
                "peer1",
                {
                    "conversation_url": "https://chatgpt.com/c/old-chat",
                    "last_turn_id": "turn-old",
                    "last_event_ids": ["evt-old"],
                },
            )
            with (
                patch.object(web_model_browser_session, "close_web_model_chatgpt_browser_session") as close_session,
                patch.object(web_model_browser_session, "stop_web_model_tool_confirm_watcher") as stop_watcher,
            ):
                web_model_browser_session.clear_web_model_chatgpt_browser_actor_runtime(group_id="g-test", actor_id="peer1")

            close_session.assert_not_called()
            stop_watcher.assert_called_once_with("g-test", "peer1")
            state = read_chatgpt_browser_state("g-test", "peer1")
            self.assertEqual(state.get("conversation_url"), "")
            self.assertEqual(state.get("last_turn_id"), "")
            self.assertEqual(state.get("last_event_ids"), [])
        finally:
            cleanup()

    def test_projected_chatgpt_close_also_closes_sidecar_browser_state(self) -> None:
        from cccc.daemon.actors import web_model_browser_session

        _, cleanup = self._with_home()
        try:
            with (
                patch.object(web_model_browser_session._MANAGER, "info", return_value={"active": True, "metadata": {"cdp_port": 9222}}),
                patch.object(web_model_browser_session._MANAGER, "close", return_value={"closed": True, "browser_surface": {"active": False}}),
                patch.object(web_model_browser_session, "close_chatgpt_browser_session", return_value={"active": False}) as close_browser,
            ):
                result = web_model_browser_session.close_web_model_chatgpt_browser_session(
                    group_id="g-test",
                    actor_id="peer1",
                )

            self.assertTrue(result.get("closed"))
            close_browser.assert_called_once_with("g-test", "peer1")
        finally:
            cleanup()

    def test_close_chatgpt_browser_session_cleans_profile_processes_when_pid_state_is_stale(self) -> None:
        from cccc.ports import web_model_browser_sidecar as sidecar

        _, cleanup = self._with_home()
        try:
            profile_dir = sidecar.chatgpt_browser_profile_dir("g-test", "peer1")
            sidecar.record_chatgpt_browser_process_state(
                {
                    "pid": 0,
                    "cdp_port": 0,
                    "profile_dir": str(profile_dir),
                    "visibility": "projected",
                },
            )
            with patch.object(sidecar, "_stop_browser_profile_processes") as stop_profile:
                sidecar.close_chatgpt_browser_session("g-test", "peer1")

            stop_profile.assert_called_once_with(str(profile_dir))
        finally:
            cleanup()

    def test_profile_process_detection_parses_posix_and_windows_user_data_dir(self) -> None:
        from cccc.ports import web_model_browser_sidecar as sidecar

        self.assertEqual(
            sidecar._user_data_dir_from_command_line(
                '/opt/google/chrome/chrome --user-data-dir="/tmp/CCCC Profile" https://chatgpt.com/'
            ),
            "/tmp/CCCC Profile",
        )
        self.assertEqual(
            sidecar._user_data_dir_from_command_line(
                r'"C:\Program Files\Google\Chrome\Application\chrome.exe" --user-data-dir="C:\Users\dodd\AppData\CCCC ChatGPT"'
            ),
            r"C:\Users\dodd\AppData\CCCC ChatGPT",
        )
        self.assertEqual(
            sidecar._user_data_dir_from_args(["chrome", "--user-data-dir", "/tmp/cccc-profile"]),
            "/tmp/cccc-profile",
        )

    def test_profile_process_pids_from_ps_matches_exact_profile(self) -> None:
        from cccc.ports import web_model_browser_sidecar as sidecar

        profile = Path("/tmp/cccc-profile")
        fake_proc = type(
            "FakeProc",
            (),
            {
                "returncode": 0,
                "stdout": (
                    '111 /opt/google/chrome/chrome --user-data-dir="/tmp/cccc-profile" https://chatgpt.com/\\n'
                    '222 /opt/google/chrome/chrome --user-data-dir="/tmp/cccc-profile-other" https://chatgpt.com/\\n'
                ),
            },
        )()
        with patch.object(sidecar.subprocess, "run", return_value=fake_proc):
            self.assertEqual(sidecar._profile_process_pids_from_ps(profile), [111])


if __name__ == "__main__":
    unittest.main()
