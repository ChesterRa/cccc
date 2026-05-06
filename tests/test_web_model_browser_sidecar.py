import os
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

    def test_browser_delivery_enabled_for_chatgpt_provider(self) -> None:
        from cccc.daemon.actors.web_model_browser_delivery import web_model_browser_delivery_enabled

        actor = {
            "id": "peer1",
            "runtime": "web_model",
            "runner": "headless",
            "web_model_provider": "chatgpt_web",
        }

        self.assertTrue(web_model_browser_delivery_enabled("g-test", actor))

    def test_browser_delivery_pull_mode_disables_proactive_delivery(self) -> None:
        from cccc.daemon.actors.web_model_browser_delivery import web_model_browser_delivery_enabled

        actor = {
            "id": "peer1",
            "runtime": "web_model",
            "runner": "headless",
            "web_model_provider": "chatgpt_web",
            "web_model_delivery_mode": "pull",
        }

        self.assertFalse(web_model_browser_delivery_enabled("g-test", actor))

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

    def test_health_snapshot_treats_pending_bind_as_recoverable_wait(self) -> None:
        from cccc.ports.web_model_browser_sidecar import build_chatgpt_web_model_health_snapshot

        snapshot = build_chatgpt_web_model_health_snapshot(
            group_id="g-test",
            actor_id="peer1",
            browser_session={
                "active": True,
                "ready": True,
                "tab_url": "https://chatgpt.com/",
                "pending_new_chat_bind": True,
                "pending_new_chat_url": "https://chatgpt.com/",
                "last_delivery_status": "pending",
                "last_delivery_id": "delivery-1",
                "last_turn_id": "turn-1",
                "last_error": "conversation_url_pending",
            },
            browser_surface={"active": True, "state": "ready", "controller_attached": True, "last_frame_at": "2026-05-04T00:00:00Z"},
        )

        self.assertEqual((snapshot.get("browser") or {}).get("state"), "ready")
        self.assertEqual((snapshot.get("target") or {}).get("state"), "new_chat_pending")
        delivery = snapshot.get("delivery") or {}
        self.assertEqual(delivery.get("state"), "pending_bind")
        self.assertTrue(delivery.get("cursor_committed"))
        self.assertEqual(delivery.get("last_error"), "")
        self.assertEqual((snapshot.get("next_action") or {}).get("recommended"), "wait_for_chat_bind")
        self.assertNotEqual(snapshot.get("tone"), "error")

    def test_health_snapshot_derives_pending_bind_from_existing_fields(self) -> None:
        from cccc.ports.web_model_browser_sidecar import build_chatgpt_web_model_health_snapshot

        snapshot = build_chatgpt_web_model_health_snapshot(
            group_id="g-test",
            actor_id="peer1",
            browser_session={
                "active": True,
                "ready": True,
                "tab_url": "https://chatgpt.com/",
                "pending_new_chat_bind": True,
                "pending_new_chat_submitted": True,
                "last_delivery_at": "2026-05-04T00:00:00Z",
                "last_error": "conversation_url_pending",
            },
            browser_surface={"active": True, "state": "ready"},
        )

        self.assertEqual((snapshot.get("delivery") or {}).get("state"), "pending_bind")
        self.assertEqual((snapshot.get("delivery") or {}).get("last_error"), "")
        self.assertEqual((snapshot.get("next_action") or {}).get("recommended"), "wait_for_chat_bind")

    def test_cached_browser_session_status_checks_stale_cdp_without_page_inspect(self) -> None:
        from cccc.ports.web_model_browser_sidecar import (
            chatgpt_browser_session_cached_status,
            record_chatgpt_browser_process_state,
            record_chatgpt_browser_state,
        )

        _, cleanup = self._with_home()
        try:
            record_chatgpt_browser_process_state(
                {
                    "pid": 12345,
                    "cdp_port": 9222,
                    "visibility": "projected",
                    "last_tab_url": "https://chatgpt.com/",
                }
            )
            record_chatgpt_browser_state(
                "g-test",
                "peer1",
                {
                    "conversation_url": "https://chatgpt.com/c/test-chat",
                    "last_delivery_status": "submitted",
                    "last_delivery_id": "delivery-1",
                },
            )

            with (
                patch("cccc.ports.web_model_browser_sidecar._wait_cdp_endpoint", return_value=True) as wait_cdp,
                patch("cccc.ports.web_model_browser_sidecar._inspect_chatgpt_browser", side_effect=AssertionError("unexpected page inspect")),
            ):
                status = chatgpt_browser_session_cached_status("g-test", "peer1")

            wait_cdp.assert_called_once()
            self.assertTrue(bool(status.get("active")))
            self.assertEqual(status.get("cdp_port"), 9222)
            self.assertEqual(status.get("conversation_url"), "https://chatgpt.com/c/test-chat")
            self.assertEqual(status.get("last_delivery_id"), "delivery-1")
            self.assertFalse(bool(status.get("ready")))
            self.assertFalse(bool(status.get("login_required")))
            with patch("cccc.ports.web_model_browser_sidecar._wait_cdp_endpoint", return_value=False):
                stale_status = chatgpt_browser_session_cached_status("g-test", "peer1")
            self.assertFalse(bool(stale_status.get("active")))
            self.assertFalse(bool(stale_status.get("login_required")))
        finally:
            cleanup()

    def test_health_snapshot_reports_browser_delivery_submitting(self) -> None:
        from cccc.ports.web_model_browser_sidecar import build_chatgpt_web_model_health_snapshot

        with patch("cccc.ports.web_model_browser_sidecar.utc_now_iso", return_value="2026-05-04T00:01:00Z"):
            snapshot = build_chatgpt_web_model_health_snapshot(
                group_id="g-test",
                actor_id="peer1",
                browser_session={
                    "active": True,
                    "ready": True,
                    "tab_url": "https://chatgpt.com/c/test",
                    "conversation_url": "https://chatgpt.com/c/test",
                    "last_delivery_status": "submitting",
                    "last_delivery_started_at": "2026-05-04T00:00:00Z",
                    "last_delivery_timeout_seconds": 120,
                    "last_delivery_id": "delivery-1",
                    "last_turn_id": "turn-1",
                    "last_event_ids": ["event-1"],
                },
                browser_surface={"active": True, "state": "ready"},
            )

        delivery = snapshot.get("delivery") or {}
        self.assertEqual(delivery.get("state"), "submitting")
        self.assertFalse(delivery.get("cursor_committed"))
        self.assertEqual((snapshot.get("next_action") or {}).get("recommended"), "none")

    def test_health_snapshot_ages_out_stale_browser_delivery_submitting(self) -> None:
        from cccc.ports.web_model_browser_sidecar import build_chatgpt_web_model_health_snapshot

        with patch("cccc.ports.web_model_browser_sidecar.utc_now_iso", return_value="2026-05-04T00:03:00Z"):
            snapshot = build_chatgpt_web_model_health_snapshot(
                group_id="g-test",
                actor_id="peer1",
                browser_session={
                    "active": True,
                    "ready": True,
                    "tab_url": "https://chatgpt.com/c/test",
                    "conversation_url": "https://chatgpt.com/c/test",
                    "last_delivery_status": "submitting",
                    "last_delivery_started_at": "2026-05-04T00:00:00Z",
                    "last_delivery_timeout_seconds": 120,
                    "last_delivery_id": "delivery-1",
                    "last_turn_id": "turn-1",
                    "last_event_ids": ["event-1"],
                },
                browser_surface={"active": True, "state": "ready"},
            )

        delivery = snapshot.get("delivery") or {}
        self.assertEqual(delivery.get("state"), "failed")
        self.assertEqual(delivery.get("last_error"), "delivery_submitting_stale")
        self.assertFalse(delivery.get("cursor_committed"))
        self.assertEqual((snapshot.get("next_action") or {}).get("recommended"), "retry_delivery")

    def test_health_snapshot_recommends_restart_for_browser_failure(self) -> None:
        from cccc.ports.web_model_browser_sidecar import build_chatgpt_web_model_health_snapshot

        snapshot = build_chatgpt_web_model_health_snapshot(
            group_id="g-test",
            actor_id="peer1",
            browser_session={
                "active": True,
                "ready": False,
                "error": "browser command timed out",
                "conversation_url": "https://chatgpt.com/c/test",
                "last_delivery_status": "submitted",
            },
            browser_surface={"active": True, "state": "failed", "message": "renderer crashed"},
        )

        self.assertEqual(snapshot.get("tone"), "error")
        self.assertEqual((snapshot.get("browser") or {}).get("state"), "failed")
        self.assertEqual((snapshot.get("next_action") or {}).get("recommended"), "restart_browser")

    def test_submission_wait_does_not_accept_composer_clear_as_delivery(self) -> None:
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
            patch.object(sidecar.time, "time", side_effect=[0.0, 0.0, 2.0]),
        ):
            self.assertFalse(
                sidecar._wait_for_submission(
                    _Page(),
                    "#prompt-textarea",
                    prompt="hello",
                    timeout_seconds=1.0,
                )
            )

    def test_submission_wait_does_not_accept_stop_button_without_echo(self) -> None:
        from cccc.ports import web_model_browser_sidecar as sidecar

        class _Locator:
            first = None

            def __init__(self) -> None:
                self.first = self

            def is_visible(self, timeout: int = 0) -> bool:
                return True

        class _Page:
            def locator(self, selector: str) -> _Locator:
                return _Locator()

        with (
            patch.object(sidecar, "_submission_echo_found", return_value=False),
            patch.object(sidecar.time, "time", side_effect=[0.0, 0.0, 2.0]),
            patch.object(sidecar.time, "sleep", return_value=None),
        ):
            self.assertEqual(
                sidecar._wait_for_submission(
                    _Page(),
                    "#prompt-textarea",
                    prompt="hello",
                    timeout_seconds=1.0,
                ),
                "stop_without_echo",
            )

    def test_submission_echo_needles_prefer_delivery_markers(self) -> None:
        from cccc.ports import web_model_browser_sidecar as sidecar

        prompt = (
            "[CCCC] Session bootstrap for this browser chat:\n"
            "[cccc] Browser batch webdelivery:peer1:abc123 events=4f83d1b3133d49ef8584fcfd2f2ca55f actor=peer1\n"
            "[cccc] user -> peer1: hello"
        )

        needles = sidecar._submission_echo_needles(prompt)

        self.assertIn("Browser batch webdelivery:peer1:abc123", needles)
        self.assertIn("events=4f83d1b3133d49ef8584fcfd2f2ca55f", needles)
        self.assertNotIn("[CCCC] Session bootstrap for this browser chat:", needles)

    def test_submission_echo_needles_use_fallback_only_without_delivery_markers(self) -> None:
        from cccc.ports import web_model_browser_sidecar as sidecar

        needles = sidecar._submission_echo_needles("plain browser prompt without delivery markers")

        self.assertEqual(needles, ["plain browser prompt without delivery markers"])

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

    def test_clear_and_type_prompt_uses_keyboard_for_contenteditable(self) -> None:
        from cccc.ports import web_model_browser_sidecar as sidecar

        class _Keyboard:
            def __init__(self) -> None:
                self.pressed: list[str] = []
                self.inserted: list[str] = []

            def press(self, key: str) -> None:
                self.pressed.append(key)

            def insert_text(self, text: str) -> None:
                self.inserted.append(text)

        class _Locator:
            first = None

            def __init__(self) -> None:
                self.first = self
                self.clicked = False
                self.fill_called = False
                self.evaluate_calls = 0

            def click(self, *_args, **_kwargs) -> None:
                self.clicked = True

            def evaluate(self, *_args, **_kwargs):
                self.evaluate_calls += 1
                if self.evaluate_calls == 1:
                    return False
                return None

            def fill(self, *_args, **_kwargs) -> None:  # pragma: no cover - should not be reached
                self.fill_called = True
                raise AssertionError("contenteditable composer should use keyboard input, not fill()")

        class _Page:
            def __init__(self) -> None:
                self.keyboard = _Keyboard()
                self.locator_obj = _Locator()

            def locator(self, _selector: str) -> _Locator:
                return self.locator_obj

        page = _Page()

        sidecar._clear_and_type_prompt(page, "#prompt-textarea", "hello")

        self.assertTrue(page.locator_obj.clicked)
        self.assertFalse(page.locator_obj.fill_called)
        self.assertIn("Backspace", page.keyboard.pressed)
        self.assertEqual(page.keyboard.inserted, ["hello"])

    def test_clear_and_type_prompt_uses_fill_for_textarea(self) -> None:
        from cccc.ports import web_model_browser_sidecar as sidecar

        class _Keyboard:
            def press(self, _key: str) -> None:  # pragma: no cover - should not be reached
                raise AssertionError("textarea fill path should not use keyboard fallback")

            def insert_text(self, _text: str) -> None:  # pragma: no cover - should not be reached
                raise AssertionError("textarea fill path should not use keyboard fallback")

        class _Locator:
            first = None

            def __init__(self) -> None:
                self.first = self
                self.filled = ""

            def click(self, *_args, **_kwargs) -> None:
                return None

            def evaluate(self, *_args, **_kwargs):
                return True

            def fill(self, text: str, *_args, **_kwargs) -> None:
                self.filled = text

        class _Page:
            def __init__(self) -> None:
                self.keyboard = _Keyboard()
                self.locator_obj = _Locator()

            def locator(self, _selector: str) -> _Locator:
                return self.locator_obj

        page = _Page()

        sidecar._clear_and_type_prompt(page, "textarea", "hello")

        self.assertEqual(page.locator_obj.filled, "hello")

    def test_send_selectors_do_not_target_voice_button_by_style_class(self) -> None:
        from cccc.ports import web_model_browser_sidecar as sidecar

        self.assertNotIn("button.composer-submit-button-color", sidecar.SEND_BUTTON_SELECTORS)
        self.assertNotIn("button.composer-submit-btn", sidecar.SEND_BUTTON_SELECTORS)

    def test_input_selectors_do_not_use_broad_hidden_textarea_fallback(self) -> None:
        from cccc.ports import web_model_browser_sidecar as sidecar

        self.assertNotIn("textarea:not([disabled])", sidecar.INPUT_SELECTORS)

    def test_visible_input_selector_returns_marked_candidate(self) -> None:
        from cccc.ports import web_model_browser_sidecar as sidecar

        class _Page:
            def __init__(self) -> None:
                self.selectors = []

            def evaluate(self, _script: str, selectors: list[str]) -> str:
                self.selectors = selectors
                return '[data-cccc-chatgpt-input-candidate="cccc-chatgpt-composer-input"]'

        page = _Page()

        selector = sidecar._visible_input_selector(page, timeout_seconds=1.0)

        self.assertEqual(selector, '[data-cccc-chatgpt-input-candidate="cccc-chatgpt-composer-input"]')
        self.assertNotIn("textarea:not([disabled])", page.selectors)

    def test_submit_prompt_waits_for_delayed_insert_before_clicking_send(self) -> None:
        from cccc.ports import web_model_browser_sidecar as sidecar

        page = object()
        prompt = "Browser-delivered CCCC message"

        with (
            patch.object(sidecar, "_visible_input_selector", return_value="#prompt-textarea"),
            patch.object(sidecar, "_clear_and_type_prompt") as clear_prompt,
            patch.object(sidecar, "_composer_text", side_effect=["", "", prompt]),
            patch.object(sidecar, "_click_send", return_value="#composer-submit-button") as click_send,
            patch.object(sidecar, "_wait_for_submission", return_value="message_echo") as wait_for_submission,
            patch.object(sidecar.time, "sleep", return_value=None),
        ):
            result = sidecar._submit_prompt(page, prompt, input_timeout_seconds=1.0)

        clear_prompt.assert_called_once_with(page, "#prompt-textarea", prompt)
        click_send.assert_called_once_with(page)
        wait_for_submission.assert_called_once()
        self.assertEqual(result.get("send_selector"), "#composer-submit-button")
        self.assertEqual(result.get("submission_evidence"), "message_echo")

    def test_submit_prompt_rejects_stop_button_without_message_echo(self) -> None:
        from cccc.ports import web_model_browser_sidecar as sidecar

        page = object()
        prompt = "Browser-delivered CCCC message"

        with (
            patch.object(sidecar, "_visible_input_selector", return_value="#prompt-textarea"),
            patch.object(sidecar, "_clear_and_type_prompt"),
            patch.object(sidecar, "_wait_for_prompt_inserted", return_value=True),
            patch.object(sidecar, "_click_send", return_value="#composer-submit-button"),
            patch.object(sidecar, "_wait_for_submission", return_value="stop_without_echo"),
            patch.object(sidecar, "_submission_diagnostics", return_value={"stop_visible": True}),
            patch.object(sidecar.time, "sleep", return_value=None),
        ):
            with self.assertRaisesRegex(RuntimeError, "submitted message did not appear"):
                sidecar._submit_prompt(page, prompt, input_timeout_seconds=1.0)

    def test_submit_prompt_reselects_input_after_focus_failure(self) -> None:
        from cccc.ports import web_model_browser_sidecar as sidecar

        page = object()
        prompt = "Browser-delivered CCCC message"

        with (
            patch.object(sidecar, "_visible_input_selector", side_effect=["textarea[name='prompt-textarea']", "[data-cccc-chatgpt-input-candidate='cccc-chatgpt-composer-input']"]) as selector,
            patch.object(sidecar, "_clear_and_type_prompt", side_effect=[RuntimeError("element is not visible"), None]) as clear_prompt,
            patch.object(sidecar, "_wait_for_prompt_inserted", return_value=True),
            patch.object(sidecar, "_click_send", return_value="#composer-submit-button"),
            patch.object(sidecar, "_wait_for_submission", return_value="message_echo"),
            patch.object(sidecar.time, "sleep", return_value=None),
        ):
            result = sidecar._submit_prompt(page, prompt, input_timeout_seconds=1.0)

        self.assertEqual(selector.call_count, 2)
        self.assertEqual(clear_prompt.call_count, 2)
        self.assertEqual(result.get("submission_evidence"), "message_echo")

    def test_submit_prompt_tries_request_submit_before_keyboard_fallback(self) -> None:
        from cccc.ports import web_model_browser_sidecar as sidecar

        class _Keyboard:
            def __init__(self) -> None:
                self.pressed: list[str] = []

            def press(self, key: str) -> None:
                self.pressed.append(key)

        class _Page:
            def __init__(self) -> None:
                self.keyboard = _Keyboard()

        page = _Page()
        prompt = "Browser-delivered CCCC message"

        with (
            patch.object(sidecar, "_visible_input_selector", return_value="#prompt-textarea"),
            patch.object(sidecar, "_clear_and_type_prompt"),
            patch.object(sidecar, "_wait_for_prompt_inserted", return_value=True),
            patch.object(sidecar, "_click_send", return_value="#composer-submit-button"),
            patch.object(sidecar, "_wait_for_submission", side_effect=["", "message_echo"]) as wait_for_submission,
            patch.object(sidecar, "_request_submit_composer", return_value="form.requestSubmit:button") as request_submit,
            patch.object(sidecar.time, "sleep", return_value=None),
        ):
            result = sidecar._submit_prompt(page, prompt, input_timeout_seconds=1.0)

        request_submit.assert_called_once_with(page)
        self.assertEqual(wait_for_submission.call_count, 2)
        self.assertEqual(page.keyboard.pressed, [])
        self.assertEqual(result.get("send_selector"), "form.requestSubmit:button")
        self.assertEqual(result.get("submission_evidence"), "message_echo")

    def test_submit_prompt_failure_includes_page_diagnostics(self) -> None:
        from cccc.ports import web_model_browser_sidecar as sidecar

        class _Keyboard:
            def press(self, _key: str) -> None:
                return None

        class _Page:
            keyboard = _Keyboard()

        with (
            patch.object(sidecar, "_visible_input_selector", return_value="#prompt-textarea"),
            patch.object(sidecar, "_clear_and_type_prompt"),
            patch.object(sidecar, "_wait_for_prompt_inserted", return_value=True),
            patch.object(sidecar, "_click_send", return_value="#composer-submit-button"),
            patch.object(sidecar, "_wait_for_submission", return_value=""),
            patch.object(sidecar, "_request_submit_composer", return_value=""),
            patch.object(sidecar, "_submission_diagnostics", return_value={"prompt_chars": 42, "send_enabled_count": 0}),
            patch.object(sidecar.time, "sleep", return_value=None),
        ):
            with self.assertRaisesRegex(RuntimeError, "diagnostics=.*send_enabled_count"):
                sidecar._submit_prompt(_Page(), "hello", input_timeout_seconds=1.0)

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

    def test_chatgpt_browser_inspection_uses_short_cdp_connect_timeout(self) -> None:
        from cccc.ports import web_model_browser_sidecar as sidecar

        class _Context:
            pages: list[object] = []

        class _Browser:
            contexts = [_Context()]

        class _Chromium:
            def __init__(self) -> None:
                self.calls: list[tuple[str, dict[str, object]]] = []

            def connect_over_cdp(self, endpoint: str, **kwargs):
                self.calls.append((endpoint, dict(kwargs)))
                return _Browser()

        class _Playwright:
            def __init__(self) -> None:
                self.chromium = _Chromium()

        class _CM:
            def __init__(self) -> None:
                self.playwright = _Playwright()

            def __enter__(self):
                return self.playwright

            def __exit__(self, exc_type, exc, tb):
                return False

        cm = _CM()

        with patch.object(sidecar, "ensure_sync_playwright", return_value=lambda: cm):
            result = sidecar._inspect_chatgpt_browser(9222)

        self.assertFalse(result.get("ready"))
        self.assertEqual(cm.playwright.chromium.calls, [("http://127.0.0.1:9222", {"timeout": sidecar.CDP_CONNECT_TIMEOUT_MS})])

    def test_pending_conversation_resolution_uses_short_cdp_connect_timeout(self) -> None:
        from cccc.ports import web_model_browser_sidecar as sidecar

        class _Context:
            pages: list[object] = []

        class _Browser:
            contexts = [_Context()]

        class _Chromium:
            def __init__(self) -> None:
                self.calls: list[tuple[str, dict[str, object]]] = []

            def connect_over_cdp(self, endpoint: str, **kwargs):
                self.calls.append((endpoint, dict(kwargs)))
                return _Browser()

        class _Playwright:
            def __init__(self) -> None:
                self.chromium = _Chromium()

        class _CM:
            def __init__(self) -> None:
                self.playwright = _Playwright()

            def __enter__(self):
                return self.playwright

            def __exit__(self, exc_type, exc, tb):
                return False

        _, cleanup = self._with_home()
        try:
            sidecar.record_chatgpt_browser_state(
                "g-test",
                "peer1",
                {
                    "conversation_url": "",
                    "pending_new_chat_bind": True,
                    "pending_new_chat_url": "https://chatgpt.com/",
                    "pending_new_chat_submitted": True,
                    "pending_new_chat_delivery_id": "browser:test",
                    "last_tab_url": "",
                },
            )
            sidecar.record_chatgpt_browser_process_state({"cdp_port": 9223, "profile_dir": "/tmp/profile"})
            cm = _CM()
            with (
                patch.object(sidecar, "_wait_cdp_endpoint", return_value=True),
                patch.object(sidecar, "ensure_sync_playwright", return_value=lambda: cm),
            ):
                result = sidecar.resolve_pending_chatgpt_conversation("g-test", "peer1")

            self.assertTrue(result.get("pending"))
            self.assertEqual(cm.playwright.chromium.calls, [("http://127.0.0.1:9223", {"timeout": sidecar.CDP_CONNECT_TIMEOUT_MS})])
        finally:
            cleanup()

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

    def test_projected_chatgpt_failed_startup_does_not_persist_dead_cdp_metadata(self) -> None:
        from cccc.daemon.actors import web_model_browser_session
        from cccc.ports import web_model_browser_sidecar as sidecar

        _, cleanup = self._with_home()
        try:
            sidecar.record_chatgpt_browser_process_state(
                {
                    "pid": 1234,
                    "cdp_port": 9222,
                    "profile_dir": str(sidecar.chatgpt_browser_profile_dir("g-test", "peer1")),
                    "visibility": "projected",
                    "last_tab_url": "https://chatgpt.com/",
                }
            )

            web_model_browser_session._record_projected_browser_state(
                "g-test",
                "peer1",
                {
                    "active": True,
                    "state": "failed",
                    "url": "https://chatgpt.com/",
                    "metadata": {
                        "pid": 4321,
                        "cdp_port": 9333,
                        "profile_dir": str(sidecar.chatgpt_browser_profile_dir("g-test", "peer1")),
                    },
                },
            )

            state = sidecar.read_chatgpt_browser_process_state()
            self.assertEqual(int(state.get("pid") or 0), 0)
            self.assertEqual(int(state.get("cdp_port") or 0), 0)
            self.assertEqual(str(state.get("visibility") or ""), "projected")
        finally:
            cleanup()

    def test_projected_chatgpt_session_closes_stale_starting_instance_before_open(self) -> None:
        from cccc.daemon.actors import web_model_browser_session

        _, cleanup = self._with_home()
        try:
            stale = {
                "active": True,
                "state": "starting",
                "started_at": "2026-05-01T00:00:00Z",
                "updated_at": "2026-05-01T00:00:00Z",
                "metadata": {"cdp_port": 9222, "pid": 1234},
            }
            opened = {
                "active": True,
                "state": "ready",
                "url": "https://chatgpt.com/",
                "metadata": {"cdp_port": 9333, "pid": 4321},
            }
            with (
                patch.object(web_model_browser_session._MANAGER, "info", side_effect=[stale, {"active": False, "state": "idle"}]),
                patch.object(web_model_browser_session._MANAGER, "close", return_value={"closed": True}) as close_manager,
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
            close_manager.assert_called_once_with(key="chatgpt_web")
            close_browser.assert_any_call("g-test", "peer1")
            open_session.assert_called_once()
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

    def test_projected_chatgpt_close_also_closes_recorded_browser_process(self) -> None:
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

    def test_projected_chatgpt_background_writes_skip_during_delivery_lock(self) -> None:
        from cccc.daemon.actors import web_model_browser_session

        class BusyLock:
            def acquire(self, blocking=True):
                self.blocking = blocking
                return False

            def release(self):  # pragma: no cover - should not be called
                raise AssertionError("busy lock should not be released by caller")

        with (
            patch.object(web_model_browser_session, "_SESSION_WRITE_LOCK", BusyLock()),
            patch.object(web_model_browser_session._MANAGER, "info", return_value={"active": True, "state": "ready"}) as info,
            patch.object(web_model_browser_session._MANAGER, "execute") as execute,
        ):
            reload_result = web_model_browser_session.reload_web_model_chatgpt_browser_session(
                group_id="g-test",
                actor_id="peer1",
                target_url="https://chatgpt.com/c/test",
            )
            confirm_result = web_model_browser_session.auto_confirm_web_model_chatgpt_tool_prompts(
                group_id="g-test",
                actor_id="peer1",
                target_url="https://chatgpt.com/c/test",
            )

        self.assertEqual(reload_result.get("error"), "browser_delivery_in_progress")
        self.assertEqual(confirm_result.get("skipped"), "browser_delivery_in_progress")
        self.assertGreaterEqual(info.call_count, 2)
        execute.assert_not_called()

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
