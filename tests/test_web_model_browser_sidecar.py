import importlib.util
import os
import shutil
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
        from cccc.daemon.actors.web_model_browser_delivery import (
            web_model_browser_delivery_enabled,
        )

        actor = {
            "id": "peer1",
            "runtime": "web_model",
            "runner": "headless",
            "web_model_provider": "chatgpt_web",
        }

        self.assertTrue(web_model_browser_delivery_enabled("g-test", actor))

    def test_browser_delivery_pull_mode_disables_proactive_delivery(self) -> None:
        from cccc.daemon.actors.web_model_browser_delivery import (
            web_model_browser_delivery_enabled,
        )

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
            _conversation_url_from_tab(
                "https://chatgpt.com/g/g-test/c/abc123?model=gpt-5"
            ),
            "https://chatgpt.com/g/g-test/c/abc123",
        )
        self.assertEqual(
            _conversation_url_from_tab("https://chatgpt.com:443/c/abc123?model=gpt-5"),
            "https://chatgpt.com/c/abc123",
        )
        self.assertEqual(_conversation_url_from_tab("https://chatgpt.com/"), "")
        self.assertEqual(_conversation_url_from_tab("http://chatgpt.com/c/abc123"), "")
        self.assertEqual(
            _conversation_url_from_tab("https://evilchatgpt.com/c/abc123"), ""
        )
        self.assertEqual(
            _conversation_url_from_tab("https://chatgpt.com.evil.test/c/abc123"), ""
        )
        self.assertEqual(
            _conversation_url_from_tab(
                "https://chatgpt.com/c/WEB:1c383803-1938-493d-a2f6-060168787e8a"
            ),
            "",
        )
        self.assertEqual(
            _conversation_url_from_tab("https://chatgpt.com/c/WEB%3Atemporary"),
            "",
        )

    def test_chatgpt_conversation_binding_waits_past_provisional_route(self) -> None:
        from cccc.ports.web_model_browser_sidecar import _wait_for_conversation_url

        class Page:
            def __init__(self) -> None:
                self._urls = iter(
                    [
                        "https://chatgpt.com/c/WEB:temporary",
                        "https://chatgpt.com/c/WEB:temporary",
                        "https://chatgpt.com/c/final-chat",
                        "https://chatgpt.com/c/final-chat",
                    ]
                )
                self._last = "https://chatgpt.com/"

            @property
            def url(self) -> str:
                self._last = next(self._urls, self._last)
                return self._last

        self.assertEqual(
            _wait_for_conversation_url(Page(), timeout_seconds=2.0),
            "https://chatgpt.com/c/final-chat",
        )

    def test_health_snapshot_treats_pending_bind_as_recoverable_wait(self) -> None:
        from cccc.ports.web_model_browser_sidecar import (
            build_chatgpt_web_model_health_snapshot,
        )

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
            browser_surface={
                "active": True,
                "state": "ready",
                "controller_attached": True,
                "last_frame_at": "2026-05-04T00:00:00Z",
            },
        )

        self.assertEqual((snapshot.get("browser") or {}).get("state"), "ready")
        self.assertEqual(
            (snapshot.get("target") or {}).get("state"), "new_chat_pending"
        )
        target = snapshot.get("delivery_target") or {}
        self.assertEqual(target.get("state"), "new_chat_armed")
        self.assertEqual(target.get("next_delivery"), "new_chat")
        delivery = snapshot.get("delivery") or {}
        self.assertEqual(delivery.get("state"), "pending_bind")
        self.assertTrue(delivery.get("cursor_committed"))
        self.assertEqual(delivery.get("last_error"), "")
        self.assertEqual(
            (snapshot.get("next_action") or {}).get("recommended"), "wait_for_chat_bind"
        )
        self.assertNotEqual(snapshot.get("tone"), "error")

    def test_health_snapshot_treats_bound_delivery_as_complete(self) -> None:
        from cccc.ports.web_model_browser_sidecar import (
            build_chatgpt_web_model_health_snapshot,
        )

        snapshot = build_chatgpt_web_model_health_snapshot(
            group_id="g-test",
            actor_id="peer1",
            browser_session={
                "active": True,
                "ready": True,
                "tab_url": "https://chatgpt.com/c/newly-created",
                "conversation_url": "https://chatgpt.com/c/newly-created",
                "last_delivery_status": "bound",
                "last_delivery_id": "delivery-1",
                "last_turn_id": "turn-1",
                "last_delivery_at": "2026-05-04T00:00:00Z",
            },
            browser_surface={
                "active": True,
                "state": "ready",
                "controller_attached": True,
                "last_frame_at": "2026-05-04T00:00:00Z",
            },
        )

        self.assertEqual((snapshot.get("target") or {}).get("state"), "bound")
        delivery = snapshot.get("delivery") or {}
        self.assertEqual(delivery.get("state"), "bound")
        self.assertTrue(delivery.get("cursor_committed"))
        self.assertEqual((snapshot.get("next_action") or {}).get("recommended"), "none")
        self.assertEqual(snapshot.get("tone"), "ready")

    def test_health_snapshot_blocks_a_bound_chat_when_live_page_mismatches(self) -> None:
        from cccc.ports.web_model_browser_sidecar import (
            build_chatgpt_web_model_health_snapshot,
        )

        snapshot = build_chatgpt_web_model_health_snapshot(
            group_id="g-test",
            actor_id="peer1",
            browser_session={
                "active": True,
                "ready": True,
                "tab_url": "https://chatgpt.com/",
                "conversation_url": "https://chatgpt.com/c/bound-chat",
            },
            browser_surface={"active": True, "state": "ready"},
        )

        self.assertEqual((snapshot.get("target") or {}).get("state"), "unavailable")
        self.assertEqual(
            (snapshot.get("next_action") or {}).get("recommended"), "bind_chat"
        )
        self.assertEqual(snapshot.get("tone"), "needs")

    def test_health_snapshot_derives_pending_bind_from_existing_fields(self) -> None:
        from cccc.ports.web_model_browser_sidecar import (
            build_chatgpt_web_model_health_snapshot,
        )

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
        self.assertEqual(
            (snapshot.get("next_action") or {}).get("recommended"), "wait_for_chat_bind"
        )

    def test_cached_browser_session_status_checks_stale_cdp_without_page_inspect(
        self,
    ) -> None:
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
                    "browser_readiness": {
                        "pid": 12345,
                        "cdp_port": 9222,
                        "inspection": {
                            "ready": True,
                            "login_required": False,
                            "tab_url": "https://chatgpt.com/c/test-chat",
                        },
                    },
                },
            )

            with (
                patch(
                    "cccc.ports.web_model_browser_sidecar._wait_cdp_endpoint",
                    return_value=True,
                ) as wait_cdp,
                patch(
                    "cccc.ports.web_model_browser_sidecar._inspect_chatgpt_browser",
                    side_effect=AssertionError("unexpected page inspect"),
                ),
            ):
                status = chatgpt_browser_session_cached_status("g-test", "peer1")

            wait_cdp.assert_called_once()
            self.assertTrue(bool(status.get("active")))
            self.assertEqual(status.get("cdp_port"), 9222)
            self.assertEqual(
                status.get("conversation_url"), "https://chatgpt.com/c/test-chat"
            )
            self.assertEqual(
                (status.get("delivery_target") or {}).get("state"),
                "bound_existing_chat",
            )
            self.assertEqual(status.get("last_delivery_id"), "delivery-1")
            self.assertTrue(bool(status.get("ready")))
            self.assertFalse(bool(status.get("login_required")))
            with patch(
                "cccc.ports.web_model_browser_sidecar._wait_cdp_endpoint",
                return_value=False,
            ):
                stale_status = chatgpt_browser_session_cached_status("g-test", "peer1")
            self.assertFalse(bool(stale_status.get("active")))
            self.assertFalse(bool(stale_status.get("login_required")))
        finally:
            cleanup()

    def test_explicit_browser_inspection_is_reused_by_cached_status(self) -> None:
        from cccc.ports.web_model_browser_sidecar import (
            chatgpt_browser_session_cached_status,
            chatgpt_browser_session_status,
            record_chatgpt_browser_process_state,
        )

        _, cleanup = self._with_home()
        try:
            record_chatgpt_browser_process_state(
                {"pid": 12345, "cdp_port": 9222, "visibility": "projected"}
            )
            inspection = {
                "ready": True,
                "login_required": False,
                "tab_url": "https://chatgpt.com/c/test-chat",
            }
            with (
                patch(
                    "cccc.ports.web_model_browser_sidecar._wait_cdp_endpoint",
                    return_value=True,
                ),
                patch(
                    "cccc.ports.web_model_browser_sidecar._inspect_chatgpt_browser",
                    return_value=inspection,
                ) as inspect_browser,
            ):
                inspected = chatgpt_browser_session_status("g-test", "peer1")
                cached = chatgpt_browser_session_cached_status("g-test", "peer1")

            inspect_browser.assert_called_once()
            self.assertTrue(bool(inspected.get("ready")))
            self.assertTrue(bool(cached.get("ready")))
            self.assertEqual(cached.get("tab_url"), "https://chatgpt.com/c/test-chat")
        finally:
            cleanup()

    def test_health_snapshot_reports_browser_delivery_submitting(self) -> None:
        from cccc.ports.web_model_browser_sidecar import (
            build_chatgpt_web_model_health_snapshot,
        )

        with patch(
            "cccc.ports.web_model_browser_sidecar.utc_now_iso",
            return_value="2026-05-04T00:01:00Z",
        ):
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

    def test_health_snapshot_reports_ambiguous_browser_delivery_as_attention_not_failure(
        self,
    ) -> None:
        from cccc.ports.web_model_browser_sidecar import (
            build_chatgpt_web_model_health_snapshot,
        )

        snapshot = build_chatgpt_web_model_health_snapshot(
            group_id="g-test",
            actor_id="peer1",
            browser_session={
                "active": True,
                "ready": True,
                "tab_url": "https://chatgpt.com/c/test",
                "conversation_url": "https://chatgpt.com/c/test",
                "last_delivery_status": "ambiguous",
                "last_delivery_id": "delivery-1",
                "last_turn_id": "turn-1",
                "last_event_ids": ["event-1"],
                "last_error": "submit action was attempted but submission could not be verified",
            },
            browser_surface={"active": True, "state": "ready"},
        )

        delivery = snapshot.get("delivery") or {}
        self.assertEqual(delivery.get("state"), "ambiguous")
        self.assertTrue(delivery.get("cursor_committed"))
        self.assertEqual(
            (snapshot.get("next_action") or {}).get("recommended"), "inspect_error"
        )
        self.assertEqual(snapshot.get("tone"), "needs")

    def test_health_snapshot_ages_out_stale_browser_delivery_submitting(self) -> None:
        from cccc.ports.web_model_browser_sidecar import (
            build_chatgpt_web_model_health_snapshot,
        )

        with patch(
            "cccc.ports.web_model_browser_sidecar.utc_now_iso",
            return_value="2026-05-04T00:03:00Z",
        ):
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
        self.assertEqual(
            (snapshot.get("next_action") or {}).get("recommended"), "retry_delivery"
        )

    def test_health_snapshot_recommends_restart_for_browser_failure(self) -> None:
        from cccc.ports.web_model_browser_sidecar import (
            build_chatgpt_web_model_health_snapshot,
        )

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
            browser_surface={
                "active": True,
                "state": "failed",
                "message": "renderer crashed",
            },
        )

        self.assertEqual(snapshot.get("tone"), "error")
        self.assertEqual((snapshot.get("browser") or {}).get("state"), "failed")
        self.assertEqual(
            (snapshot.get("next_action") or {}).get("recommended"), "restart_browser"
        )

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

    def test_submission_wait_detects_running_state_without_stop_testid(self) -> None:
        from cccc.ports import web_model_browser_sidecar as sidecar

        class _Page:
            @staticmethod
            def evaluate(_script: str) -> bool:
                return True

            def locator(self, _selector: str) -> object:
                raise AssertionError(
                    "broad running-state detection should not require the legacy stop-button test id"
                )

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

    def test_submission_echo_needles_use_fallback_only_without_delivery_markers(
        self,
    ) -> None:
        from cccc.ports import web_model_browser_sidecar as sidecar

        needles = sidecar._submission_echo_needles(
            "plain browser prompt without delivery markers"
        )

        self.assertEqual(needles, ["plain browser prompt without delivery markers"])

    def test_composer_text_reads_contenteditable_via_dom_evaluate(self) -> None:
        from cccc.ports import web_model_browser_sidecar as sidecar

        class _Locator:
            first = None

            def __init__(self) -> None:
                self.first = self

            def evaluate(self, *_args, **_kwargs) -> str:
                return "Inserted prompt"

            def input_value(
                self, *_args, **_kwargs
            ) -> str:  # pragma: no cover - should not be reached
                raise AssertionError("input_value fallback should not be needed")

        class _Page:
            def locator(self, selector: str) -> _Locator:
                self.selector = selector
                return _Locator()

        page = _Page()

        self.assertEqual(
            sidecar._composer_text(page, "#prompt-textarea"), "Inserted prompt"
        )
        self.assertEqual(page.selector, "#prompt-textarea")

    def test_prompt_presence_scan_uses_playwright_evaluate_signature(self) -> None:
        from cccc.ports import web_model_browser_sidecar as sidecar

        class _Page:
            def __init__(self) -> None:
                self.calls: list[tuple[str, str]] = []

            def evaluate(self, script: str, selector: str) -> list[str]:
                self.calls.append((script, selector))
                return ["Browser-delivered prompt still in another composer"]

        page = _Page()

        self.assertTrue(
            sidecar._prompt_present_in_any_composer(
                page,
                "Browser-delivered prompt still in another composer",
                "",
            )
        )
        self.assertEqual(len(page.calls), 1)
        self.assertEqual(page.calls[0][1], "")

    def test_submit_prompt_retypes_when_staged_batch_only_matches_prefix_and_suffix(
        self,
    ) -> None:
        from cccc.ports import web_model_browser_sidecar as sidecar

        shared_prefix = "bootstrap-seed-" + ("a" * 170)
        shared_suffix = ("z" * 130) + "-shared-message-tail"
        staged_prompt = (
            f"{shared_prefix}\ndelivery=old events=event-old\n{shared_suffix}"
        )
        current_prompt = (
            f"{shared_prefix}\ndelivery=new events=event-old,event-new\n{shared_suffix}"
        )
        page = object()

        with (
            patch.object(sidecar, "_submission_echo_found", return_value=False),
            patch.object(
                sidecar, "_visible_input_selector", return_value="#prompt-textarea"
            ),
            patch.object(sidecar, "_composer_text", return_value=staged_prompt),
            patch.object(sidecar, "_clear_and_type_prompt") as clear_and_type,
            patch.object(sidecar, "_wait_for_prompt_inserted", return_value=True),
            patch.object(
                sidecar, "_click_send", return_value="#composer-submit-button"
            ),
            patch.object(sidecar, "_wait_for_submission", return_value="message_echo"),
            patch.object(sidecar.time, "sleep", return_value=None),
        ):
            self.assertFalse(
                sidecar._prompt_exactly_staged(page, "#prompt-textarea", current_prompt)
            )
            result = sidecar._submit_prompt(
                page, current_prompt, input_timeout_seconds=1.0
            )

        clear_and_type.assert_called_once_with(page, "#prompt-textarea", current_prompt)
        self.assertEqual(result.get("submission_evidence"), "message_echo")

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

            def fill(
                self, *_args, **_kwargs
            ) -> None:  # pragma: no cover - should not be reached
                self.fill_called = True
                raise AssertionError(
                    "contenteditable composer should use keyboard input, not fill()"
                )

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
            def press(
                self, _key: str
            ) -> None:  # pragma: no cover - should not be reached
                raise AssertionError(
                    "textarea fill path should not use keyboard fallback"
                )

            def insert_text(
                self, _text: str
            ) -> None:  # pragma: no cover - should not be reached
                raise AssertionError(
                    "textarea fill path should not use keyboard fallback"
                )

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

        self.assertNotIn(
            "button.composer-submit-button-color", sidecar.SEND_BUTTON_SELECTORS
        )
        self.assertNotIn("button.composer-submit-btn", sidecar.SEND_BUTTON_SELECTORS)

    def test_input_selectors_do_not_use_broad_hidden_textarea_fallback(self) -> None:
        from cccc.ports import web_model_browser_sidecar as sidecar

        self.assertNotIn("textarea:not([disabled])", sidecar.INPUT_SELECTORS)

    def test_compatibility_image_is_materialized_under_cccc_home_cache(self) -> None:
        from cccc.ports import web_model_browser_sidecar as sidecar

        _, cleanup = self._with_home()
        try:
            path = sidecar.chatgpt_compatibility_image_path()
            data = path.read_bytes()
            self.assertEqual(path.name, "cccc-mcp-compat.png")
            self.assertEqual(path.parent.name, "web-model")
            self.assertTrue(data.startswith(b"\x89PNG\r\n\x1a\n"))
            self.assertIn((32).to_bytes(4, "big"), data[:32])
            self.assertEqual(sidecar.chatgpt_compatibility_image_path(), path)
            first = sidecar.chatgpt_compatibility_image_path("webdelivery:web1:first")
            same = sidecar.chatgpt_compatibility_image_path("webdelivery:web1:first")
            second = sidecar.chatgpt_compatibility_image_path("webdelivery:web1:second")
            self.assertEqual(first, same)
            self.assertEqual(first.read_bytes(), same.read_bytes())
            self.assertNotEqual(first.name, second.name)
            self.assertNotEqual(first.read_bytes(), second.read_bytes())
            self.assertIn(b"CCCC-Delivery\0", first.read_bytes())
            self.assertEqual(first.read_bytes()[-8:-4], b"IEND")
        finally:
            cleanup()

    def test_compatibility_image_upload_failure_is_retryable_before_submit(self) -> None:
        from cccc.daemon.actors.web_model_browser_delivery import (
            _is_submission_ambiguous_error,
            _is_submission_deferred_error,
        )
        from cccc.ports import web_model_browser_sidecar as sidecar

        class FailingUpload:
            def set_input_files(self, _path: str) -> None:
                raise RuntimeError("synthetic upload failure")

        class Locator:
            first = FailingUpload()

        class Page:
            def evaluate(self, _script: str, _payload: object = None) -> dict[str, object]:
                return {"ready": False, "preview_count": 0}

            def locator(self, _selector: str) -> Locator:
                return Locator()

        _, cleanup = self._with_home()
        try:
            attachment = sidecar.chatgpt_compatibility_image_path(
                "delivery-upload-failure"
            )
            with self.assertRaisesRegex(
                RuntimeError, sidecar.CHATGPT_SUBMIT_DEFERRED_MARKER
            ) as raised:
                sidecar._attach_compatibility_image(
                    Page(),
                    attachment,
                    delivery_id="delivery-upload-failure",
                    timeout_seconds=0.2,
                )
            error = str(raised.exception)
            self.assertTrue(_is_submission_deferred_error(error))
            self.assertFalse(_is_submission_ambiguous_error(error))
        finally:
            cleanup()

    def test_visible_input_selector_returns_marked_candidate(self) -> None:
        from cccc.ports import web_model_browser_sidecar as sidecar

        class _Page:
            def __init__(self) -> None:
                self.selectors = []

            def evaluate(self, _script: str, selectors: list[str]) -> str:
                self.selectors = selectors
                return (
                    '[data-cccc-chatgpt-input-candidate="cccc-chatgpt-composer-input"]'
                )

        page = _Page()

        selector = sidecar._visible_input_selector(page, timeout_seconds=1.0)

        self.assertEqual(
            selector,
            '[data-cccc-chatgpt-input-candidate="cccc-chatgpt-composer-input"]',
        )
        self.assertNotIn("textarea:not([disabled])", page.selectors)

    @unittest.skipUnless(
        importlib.util.find_spec("playwright")
        and (shutil.which("google-chrome") or shutil.which("chromium")),
        "Playwright and a system Chromium browser are required",
    )
    def test_current_chatgpt_dom_uses_visible_composer_and_verified_send(self) -> None:
        from playwright.sync_api import sync_playwright

        from cccc.ports import web_model_browser_sidecar as sidecar

        browser_binary = shutil.which("google-chrome") or shutil.which("chromium")
        html = """<!doctype html><html><body>
<textarea aria-label="Chat with ChatGPT" placeholder="Ask ChatGPT" style="display:none"></textarea>
<main><form>
  <div id="prompt-textarea" class="ProseMirror" role="textbox" contenteditable="true" style="width:440px;min-height:64px"></div>
  <button type="button" data-testid="send-button" aria-label="Send prompt">Send</button>
</form><section id="messages"></section></main>
<script>
const prompt = document.querySelector('#prompt-textarea');
document.querySelector('[data-testid="send-button"]').addEventListener('click', () => {
  globalThis.submitted = prompt.innerText || prompt.textContent || '';
  const message = document.createElement('article');
  message.setAttribute('data-message-author-role', 'user');
  message.textContent = globalThis.submitted;
  document.querySelector('#messages').appendChild(message);
  prompt.textContent = '';
});
</script></body></html>"""

        with sync_playwright() as pw:
            browser = pw.chromium.launch(headless=True, executable_path=browser_binary)
            try:
                page = browser.new_page(viewport={"width": 800, "height": 600})
                page.set_content(html)
                selector = sidecar._visible_input_selector(page, timeout_seconds=1.0)
                result = sidecar._submit_prompt(
                    page,
                    "python parity current DOM",
                    input_timeout_seconds=2.0,
                    submit_timeout_seconds=5.0,
                )

                self.assertEqual(
                    selector,
                    '[data-cccc-chatgpt-input-candidate="cccc-chatgpt-composer-input"]',
                )
                self.assertEqual(page.locator("textarea").input_value(), "")
                self.assertEqual(
                    page.evaluate("globalThis.submitted || ''"),
                    "python parity current DOM",
                )
                self.assertEqual(result.get("submission_evidence"), "message_echo")
                self.assertEqual(
                    result.get("send_selector"), 'button[data-testid="send-button"]'
                )
            finally:
                browser.close()

    @unittest.skipUnless(
        importlib.util.find_spec("playwright")
        and (shutil.which("google-chrome") or shutil.which("chromium")),
        "Playwright and a system Chromium browser are required",
    )
    def test_current_chatgpt_dom_attaches_compatibility_image_once_before_send(
        self,
    ) -> None:
        from playwright.sync_api import sync_playwright

        from cccc.ports import web_model_browser_sidecar as sidecar

        browser_binary = shutil.which("google-chrome") or shutil.which("chromium")
        html = """<!doctype html><html><body><main><form>
<div id="preview-host"></div>
<div class="prosemirror-parent">
  <div id="prompt-textarea" class="ProseMirror" role="textbox" contenteditable="true" style="width:440px;min-height:64px"></div>
</div>
<input id="upload-photos" type="file" accept="image/*" multiple style="display:none" />
<button type="button" data-testid="send-button" aria-label="Send prompt">Send</button>
</form><section id="messages"></section></main><script>
const prompt = document.querySelector('#prompt-textarea');
const bindUpload = input => input.addEventListener('change', () => {
  globalThis.uploadAttempts = (globalThis.uploadAttempts || 0) + 1;
  globalThis.lastFileName = input.files?.[0]?.name || '';
  if (!document.querySelector('#generic-preview')) {
    const preview = document.createElement('div');
    preview.id = 'generic-preview';
    preview.innerHTML = '<img alt="" src="data:image/gif;base64,R0lGODlhAQABAIAAAAAAAP///ywAAAAAAQABAAACAUwAOw==" style="width:48px;height:48px" />';
    document.querySelector('#preview-host').appendChild(preview);
  }
  const replacement = input.cloneNode(true);
  input.replaceWith(replacement);
  bindUpload(replacement);
});
bindUpload(document.querySelector('#upload-photos'));
document.querySelector('[data-testid="send-button"]').addEventListener('click', () => {
  globalThis.sendClicks = (globalThis.sendClicks || 0) + 1;
  globalThis.submitted = prompt.innerText || prompt.textContent || '';
  globalThis.submittedFiles = globalThis.lastFileName ? [globalThis.lastFileName] : [];
  const message = document.createElement('article');
  message.setAttribute('data-message-author-role', 'user');
  message.textContent = globalThis.submitted;
  document.querySelector('#messages').appendChild(message);
  prompt.textContent = '';
});
</script></body></html>"""
        _, cleanup = self._with_home()
        try:
            delivery_id = "webdelivery:peer1:image"
            attachment = sidecar.chatgpt_compatibility_image_path(delivery_id)
            with sync_playwright() as pw:
                browser = pw.chromium.launch(
                    headless=True, executable_path=browser_binary
                )
                try:
                    page = browser.new_page(viewport={"width": 800, "height": 600})
                    page.set_content(html)
                    prompt = "[cccc] Browser batch webdelivery:peer1:image events=one actor=peer1\nhello"
                    first = sidecar._submit_prompt(
                        page,
                        prompt,
                        input_timeout_seconds=2.0,
                        submit_timeout_seconds=5.0,
                        attachment_path=attachment,
                        delivery_id=delivery_id,
                    )
                    second = sidecar._submit_prompt(
                        page,
                        prompt,
                        input_timeout_seconds=2.0,
                        submit_timeout_seconds=5.0,
                        attachment_path=attachment,
                        delivery_id=delivery_id,
                    )

                    self.assertEqual(
                        (first.get("attachment") or {}).get("delivery_id"), delivery_id
                    )
                    self.assertEqual(
                        second.get("send_selector"), "existing:message_echo"
                    )
                    self.assertEqual(page.evaluate("globalThis.uploadAttempts || 0"), 1)
                    self.assertEqual(
                        page.evaluate("globalThis.submittedFiles || []"),
                        [attachment.name],
                    )
                    self.assertEqual(page.evaluate("globalThis.sendClicks || 0"), 1)
                finally:
                    browser.close()
        finally:
            cleanup()

    @unittest.skipUnless(
        importlib.util.find_spec("playwright")
        and (shutil.which("google-chrome") or shutil.which("chromium")),
        "Playwright and a system Chromium browser are required",
    )
    def test_duplicate_compatibility_dialog_recovers_without_reuploading(self) -> None:
        from playwright.sync_api import sync_playwright

        from cccc.ports import web_model_browser_sidecar as sidecar

        browser_binary = shutil.which("google-chrome") or shutil.which("chromium")
        html = """<!doctype html><html><body><main><form>
<div><img alt="" src="data:image/gif;base64,R0lGODlhAQABAIAAAAAAAP///ywAAAAAAQABAAACAUwAOw==" style="width:48px;height:48px" /></div>
<div id="prompt-textarea" class="ProseMirror" role="textbox" contenteditable="true" style="width:440px;min-height:64px"></div>
<input id="upload-photos" type="file" accept="image/*" multiple style="display:none" />
<button type="button" data-testid="send-button" aria-label="Send prompt">Send</button>
</form><section id="messages"></section></main>
<div role="dialog" aria-modal="true" style="position:fixed;inset:0;width:600px;height:400px;background:white">
  <h2>You've already uploaded this file.</h2><p>Try uploading something new.</p><button type="button">OK</button>
</div><script>
const prompt = document.querySelector('#prompt-textarea');
document.querySelector('#upload-photos').addEventListener('change', () => {
  globalThis.uploadAttempts = (globalThis.uploadAttempts || 0) + 1;
});
document.querySelector('[role="dialog"] button').addEventListener('click', event => {
  globalThis.dialogDismissals = (globalThis.dialogDismissals || 0) + 1;
  event.currentTarget.closest('[role="dialog"]').remove();
});
document.querySelector('[data-testid="send-button"]').addEventListener('click', () => {
  globalThis.sendClicks = (globalThis.sendClicks || 0) + 1;
  const message = document.createElement('article');
  message.setAttribute('data-message-author-role', 'user');
  message.textContent = prompt.innerText || prompt.textContent || '';
  document.querySelector('#messages').appendChild(message);
  prompt.textContent = '';
});
</script></body></html>"""
        _, cleanup = self._with_home()
        try:
            delivery_id = "webdelivery:peer1:recovery"
            attachment = sidecar.chatgpt_compatibility_image_path(delivery_id)
            with sync_playwright() as pw:
                browser = pw.chromium.launch(
                    headless=True, executable_path=browser_binary
                )
                try:
                    page = browser.new_page(viewport={"width": 800, "height": 600})
                    page.set_content(html)
                    result = sidecar._submit_prompt(
                        page,
                        "[cccc] Browser batch webdelivery:peer1:recovery events=one actor=peer1\nhello",
                        input_timeout_seconds=2.0,
                        submit_timeout_seconds=5.0,
                        attachment_path=attachment,
                        delivery_id=delivery_id,
                    )

                    self.assertEqual(result.get("submission_evidence"), "message_echo")
                    self.assertEqual(
                        page.evaluate("globalThis.dialogDismissals || 0"), 1
                    )
                    self.assertEqual(page.evaluate("globalThis.uploadAttempts || 0"), 0)
                    self.assertEqual(page.evaluate("globalThis.sendClicks || 0"), 1)
                finally:
                    browser.close()
        finally:
            cleanup()

    def test_submit_prompt_waits_for_delayed_insert_before_clicking_send(self) -> None:
        from cccc.ports import web_model_browser_sidecar as sidecar

        page = object()
        prompt = "Browser-delivered CCCC message"

        with (
            patch.object(
                sidecar, "_visible_input_selector", return_value="#prompt-textarea"
            ),
            patch.object(sidecar, "_clear_and_type_prompt") as clear_prompt,
            patch.object(sidecar, "_composer_text", side_effect=["", "", prompt]),
            patch.object(
                sidecar, "_click_send", return_value="#composer-submit-button"
            ) as click_send,
            patch.object(
                sidecar, "_wait_for_submission", return_value="message_echo"
            ) as wait_for_submission,
            patch.object(sidecar.time, "sleep", return_value=None),
        ):
            result = sidecar._submit_prompt(page, prompt, input_timeout_seconds=1.0)

        clear_prompt.assert_called_once_with(page, "#prompt-textarea", prompt)
        click_send.assert_called_once()
        self.assertIs(click_send.call_args.args[0], page)
        self.assertGreater(
            float(click_send.call_args.kwargs.get("timeout_seconds") or 0.0), 0.0
        )
        wait_for_submission.assert_called_once()
        self.assertEqual(result.get("send_selector"), "#composer-submit-button")
        self.assertEqual(result.get("submission_evidence"), "message_echo")

    def test_submit_prompt_accepts_running_state_without_message_echo(self) -> None:
        from cccc.ports import web_model_browser_sidecar as sidecar

        page = object()
        prompt = "Browser-delivered CCCC message"

        with (
            patch.object(
                sidecar, "_visible_input_selector", return_value="#prompt-textarea"
            ),
            patch.object(sidecar, "_clear_and_type_prompt"),
            patch.object(sidecar, "_wait_for_prompt_inserted", return_value=True),
            patch.object(
                sidecar, "_click_send", return_value="#composer-submit-button"
            ),
            patch.object(
                sidecar, "_wait_for_submission", return_value="stop_without_echo"
            ),
            patch.object(
                sidecar, "_submission_diagnostics", return_value={"stop_visible": True}
            ),
            patch.object(sidecar.time, "sleep", return_value=None),
        ):
            result = sidecar._submit_prompt(page, prompt, input_timeout_seconds=1.0)

        self.assertEqual(result.get("send_selector"), "#composer-submit-button")
        self.assertEqual(result.get("submission_evidence"), "running_without_echo")

    def test_submit_prompt_uses_explicit_send_control_while_chatgpt_is_running(
        self,
    ) -> None:
        from cccc.ports import web_model_browser_sidecar as sidecar

        page = object()
        prompt = "Browser-delivered CCCC message"

        with (
            patch.object(sidecar, "_chatgpt_running_visible", return_value=True),
            patch.object(
                sidecar, "_visible_input_selector", return_value="#prompt-textarea"
            ) as input_selector,
            patch.object(sidecar, "_clear_and_type_prompt") as clear_prompt,
            patch.object(sidecar, "_wait_for_prompt_inserted", return_value=True),
            patch.object(
                sidecar, "_click_send", return_value='button[data-testid="send-button"]'
            ) as click_send,
            patch.object(sidecar, "_wait_for_submission", return_value="message_echo"),
            patch.object(sidecar, "_request_submit_composer") as request_submit,
            patch.object(sidecar.time, "sleep", return_value=None),
        ):
            result = sidecar._submit_prompt(page, prompt, input_timeout_seconds=1.0)

        input_selector.assert_called_once()
        clear_prompt.assert_called_once_with(page, "#prompt-textarea", prompt)
        self.assertEqual(
            click_send.call_args.kwargs.get("input_selector"), "#prompt-textarea"
        )
        request_submit.assert_not_called()
        self.assertEqual(
            result.get("send_selector"), 'button[data-testid="send-button"]'
        )
        self.assertEqual(result.get("submission_evidence"), "message_echo")

    def test_submit_prompt_stages_prompt_but_defers_without_form_or_enter_when_no_safe_send_exists(
        self,
    ) -> None:
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
            patch.object(
                sidecar, "_visible_input_selector", return_value="#prompt-textarea"
            ),
            patch.object(sidecar, "_clear_and_type_prompt") as clear_prompt,
            patch.object(sidecar, "_wait_for_prompt_inserted", return_value=True),
            patch.object(
                sidecar,
                "_click_send",
                side_effect=sidecar._SubmitDeferredState(
                    f"{sidecar.CHATGPT_SUBMIT_DEFERRED_MARKER} no safe Send prompt control"
                ),
            ),
            patch.object(sidecar, "_prompt_present_in_any_composer", return_value=True),
            patch.object(sidecar, "_request_submit_composer") as request_submit,
            patch.object(sidecar.time, "sleep", return_value=None),
        ):
            with self.assertRaisesRegex(
                RuntimeError, sidecar.CHATGPT_SUBMIT_DEFERRED_MARKER
            ):
                sidecar._submit_prompt(page, prompt, input_timeout_seconds=1.0)

        clear_prompt.assert_called_once_with(page, "#prompt-textarea", prompt)
        request_submit.assert_not_called()
        self.assertEqual(page.keyboard.pressed, [])

    def test_submit_prompt_does_not_retype_same_deferred_prompt(self) -> None:
        from cccc.ports import web_model_browser_sidecar as sidecar

        page = object()
        prompt = "Browser-delivered CCCC message"

        with (
            patch.object(
                sidecar, "_visible_input_selector", return_value="#prompt-textarea"
            ),
            patch.object(sidecar, "_prompt_exactly_staged", return_value=True),
            patch.object(sidecar, "_clear_and_type_prompt") as clear_prompt,
            patch.object(sidecar, "_wait_for_prompt_inserted", return_value=True),
            patch.object(
                sidecar,
                "_click_send",
                side_effect=sidecar._SubmitDeferredState(
                    sidecar.CHATGPT_SUBMIT_DEFERRED_ERROR
                ),
            ),
            patch.object(sidecar, "_prompt_present_in_any_composer", return_value=True),
            patch.object(sidecar.time, "sleep", return_value=None),
        ):
            with self.assertRaisesRegex(
                RuntimeError, sidecar.CHATGPT_SUBMIT_DEFERRED_MARKER
            ):
                sidecar._submit_prompt(page, prompt, input_timeout_seconds=1.0)

        clear_prompt.assert_not_called()

    def test_submit_prompt_recovers_echo_before_deferred_retry(self) -> None:
        from cccc.ports import web_model_browser_sidecar as sidecar

        page = object()
        prompt = "[cccc] Browser batch webdelivery:peer1:auto events=abc123def456 actor=peer1"

        with (
            patch.object(sidecar, "_submission_echo_found", side_effect=[False, True]),
            patch.object(
                sidecar, "_visible_input_selector", return_value="#prompt-textarea"
            ),
            patch.object(sidecar, "_clear_and_type_prompt"),
            patch.object(sidecar, "_wait_for_prompt_inserted", return_value=True),
            patch.object(
                sidecar,
                "_click_send",
                side_effect=sidecar._SubmitDeferredState(
                    sidecar.CHATGPT_SUBMIT_DEFERRED_ERROR
                ),
            ),
            patch.object(sidecar.time, "sleep", return_value=None),
        ):
            result = sidecar._submit_prompt(
                page,
                prompt,
                input_timeout_seconds=1.0,
            )

        self.assertEqual(result.get("submission_evidence"), "message_echo")
        self.assertEqual(result.get("send_selector"), "send_control:deferred")

    def test_submit_prompt_fences_changed_page_instead_of_deferred_retry(self) -> None:
        from cccc.ports import web_model_browser_sidecar as sidecar

        class Page:
            url = "https://chatgpt.com/"

        page = Page()
        prompt = "[cccc] Browser batch webdelivery:peer1:auto events=abc123def456 actor=peer1"

        def submit_then_defer(*_args, **_kwargs):
            page.url = "https://chatgpt.com/c/new"
            raise sidecar._SubmitDeferredState(sidecar.CHATGPT_SUBMIT_DEFERRED_ERROR)

        with (
            patch.object(sidecar, "_submission_echo_found", return_value=False),
            patch.object(
                sidecar, "_visible_input_selector", return_value="#prompt-textarea"
            ),
            patch.object(sidecar, "_clear_and_type_prompt"),
            patch.object(sidecar, "_wait_for_prompt_inserted", return_value=True),
            patch.object(
                sidecar,
                "_click_send",
                side_effect=submit_then_defer,
            ),
            patch.object(
                sidecar, "_prompt_present_in_any_composer", return_value=True
            ),
            patch.object(
                sidecar,
                "_submission_diagnostics",
                return_value={"url": "https://chatgpt.com/c/new"},
            ),
            patch.object(sidecar.time, "sleep", return_value=None),
        ):
            with self.assertRaisesRegex(
                RuntimeError, "submission_verification=ambiguous"
            ) as raised:
                sidecar._submit_prompt(
                    page,
                    prompt,
                    input_timeout_seconds=1.0,
                )

        self.assertNotIn(sidecar.CHATGPT_SUBMIT_DEFERRED_MARKER, str(raised.exception))

    def test_submit_prompt_accepts_existing_delivery_echo_before_retrying(self) -> None:
        from cccc.ports import web_model_browser_sidecar as sidecar

        page = object()
        prompt = "[cccc] Browser batch webdelivery:peer1:test events=abc123def456 actor=peer1"

        with (
            patch.object(sidecar, "_submission_echo_found", return_value=True),
            patch.object(sidecar, "_visible_input_selector") as input_selector,
            patch.object(sidecar, "_clear_and_type_prompt") as clear_prompt,
            patch.object(sidecar, "_click_send") as click_send,
        ):
            result = sidecar._submit_prompt(page, prompt, input_timeout_seconds=1.0)

        input_selector.assert_not_called()
        clear_prompt.assert_not_called()
        click_send.assert_not_called()
        self.assertEqual(result.get("send_selector"), "existing:message_echo")
        self.assertEqual(result.get("submission_evidence"), "message_echo")

    def test_submit_prompt_reselects_input_after_focus_failure(self) -> None:
        from cccc.ports import web_model_browser_sidecar as sidecar

        page = object()
        prompt = "Browser-delivered CCCC message"

        with (
            patch.object(
                sidecar,
                "_visible_input_selector",
                side_effect=[
                    "textarea[name='prompt-textarea']",
                    "[data-cccc-chatgpt-input-candidate='cccc-chatgpt-composer-input']",
                ],
            ) as selector,
            patch.object(
                sidecar,
                "_clear_and_type_prompt",
                side_effect=[RuntimeError("element is not visible"), None],
            ) as clear_prompt,
            patch.object(sidecar, "_wait_for_prompt_inserted", return_value=True),
            patch.object(
                sidecar, "_click_send", return_value="#composer-submit-button"
            ),
            patch.object(sidecar, "_wait_for_submission", return_value="message_echo"),
            patch.object(sidecar.time, "sleep", return_value=None),
        ):
            result = sidecar._submit_prompt(page, prompt, input_timeout_seconds=1.0)

        self.assertEqual(selector.call_count, 2)
        self.assertEqual(clear_prompt.call_count, 2)
        self.assertEqual(result.get("submission_evidence"), "message_echo")

    def test_submit_prompt_tries_request_submit_when_send_click_fails(self) -> None:
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
            patch.object(
                sidecar, "_visible_input_selector", return_value="#prompt-textarea"
            ),
            patch.object(sidecar, "_clear_and_type_prompt"),
            patch.object(sidecar, "_wait_for_prompt_inserted", return_value=True),
            patch.object(
                sidecar, "_click_send", side_effect=RuntimeError("send button missing")
            ),
            patch.object(
                sidecar, "_wait_for_submission", return_value="message_echo"
            ) as wait_for_submission,
            patch.object(
                sidecar,
                "_request_submit_composer",
                return_value="form.requestSubmit:button",
            ) as request_submit,
            patch.object(sidecar.time, "sleep", return_value=None),
        ):
            result = sidecar._submit_prompt(page, prompt, input_timeout_seconds=1.0)

        request_submit.assert_called_once_with(page)
        wait_for_submission.assert_called_once()
        self.assertEqual(page.keyboard.pressed, [])
        self.assertEqual(result.get("send_selector"), "form.requestSubmit:button")
        self.assertEqual(result.get("submission_evidence"), "message_echo")

    def test_submit_prompt_does_not_fallback_after_send_click_without_evidence(
        self,
    ) -> None:
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

        with (
            patch.object(
                sidecar, "_visible_input_selector", return_value="#prompt-textarea"
            ),
            patch.object(sidecar, "_clear_and_type_prompt"),
            patch.object(sidecar, "_wait_for_prompt_inserted", return_value=True),
            patch.object(
                sidecar, "_click_send", return_value="#composer-submit-button"
            ),
            patch.object(sidecar, "_wait_for_submission", return_value=""),
            patch.object(sidecar, "_prompt_present_in_any_composer", return_value=True),
            patch.object(
                sidecar,
                "_request_submit_composer",
                return_value="form.requestSubmit:button",
            ) as request_submit,
            patch.object(
                sidecar,
                "_submission_diagnostics",
                return_value={"prompt_chars": 0, "send_enabled_count": 0},
            ),
            patch.object(sidecar.time, "sleep", return_value=None),
        ):
            with self.assertRaisesRegex(RuntimeError, "submit action was attempted"):
                sidecar._submit_prompt(
                    page, "Browser-delivered CCCC message", input_timeout_seconds=1.0
                )

        request_submit.assert_not_called()
        self.assertEqual(page.keyboard.pressed, [])

    def test_submit_prompt_does_not_fallback_when_send_control_is_stop(self) -> None:
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

        with (
            patch.object(
                sidecar, "_visible_input_selector", return_value="#prompt-textarea"
            ),
            patch.object(sidecar, "_clear_and_type_prompt"),
            patch.object(sidecar, "_wait_for_prompt_inserted", return_value=True),
            patch.object(
                sidecar,
                "_click_send",
                side_effect=sidecar._UnsafeSubmitState(
                    "ChatGPT composer control matched stop state"
                ),
            ),
            patch.object(
                sidecar,
                "_request_submit_composer",
                return_value="form.requestSubmit:button",
            ) as request_submit,
            patch.object(sidecar.time, "sleep", return_value=None),
        ):
            with self.assertRaisesRegex(RuntimeError, "stop state"):
                sidecar._submit_prompt(
                    page, "Browser-delivered CCCC message", input_timeout_seconds=1.0
                )

        request_submit.assert_not_called()
        self.assertEqual(page.keyboard.pressed, [])

    def test_submit_prompt_does_not_fallback_after_any_send_click(self) -> None:
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

        with (
            patch.object(
                sidecar, "_visible_input_selector", return_value="#prompt-textarea"
            ),
            patch.object(sidecar, "_clear_and_type_prompt"),
            patch.object(sidecar, "_wait_for_prompt_inserted", return_value=True),
            patch.object(
                sidecar, "_click_send", return_value="#composer-submit-button"
            ),
            patch.object(
                sidecar, "_wait_for_submission", return_value=""
            ) as wait_for_submission,
            patch.object(sidecar, "_prompt_present_in_any_composer", return_value=True),
            patch.object(
                sidecar,
                "_request_submit_composer",
                return_value="form.requestSubmit:button",
            ) as request_submit,
            patch.object(
                sidecar,
                "_submission_diagnostics",
                return_value={"prompt_chars": 42, "send_enabled_count": 1},
            ),
            patch.object(sidecar.time, "sleep", return_value=None),
        ):
            with self.assertRaisesRegex(RuntimeError, "submit action was attempted"):
                sidecar._submit_prompt(
                    page, "Browser-delivered CCCC message", input_timeout_seconds=1.0
                )

        request_submit.assert_not_called()
        wait_for_submission.assert_called_once()
        self.assertEqual(page.keyboard.pressed, [])

    def test_submit_prompt_does_not_fallback_when_clicked_send_starts_running(
        self,
    ) -> None:
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

        with (
            patch.object(
                sidecar, "_visible_input_selector", return_value="#prompt-textarea"
            ),
            patch.object(sidecar, "_clear_and_type_prompt"),
            patch.object(sidecar, "_wait_for_prompt_inserted", return_value=True),
            patch.object(
                sidecar, "_click_send", return_value="#composer-submit-button"
            ),
            patch.object(
                sidecar, "_wait_for_submission", return_value="stop_without_echo"
            ),
            patch.object(
                sidecar,
                "_request_submit_composer",
                return_value="form.requestSubmit:button",
            ) as request_submit,
            patch.object(
                sidecar, "_submission_diagnostics", return_value={"stop_visible": True}
            ),
            patch.object(sidecar.time, "sleep", return_value=None),
        ):
            result = sidecar._submit_prompt(
                page, "Browser-delivered CCCC message", input_timeout_seconds=1.0
            )

        request_submit.assert_not_called()
        self.assertEqual(page.keyboard.pressed, [])
        self.assertEqual(result.get("submission_evidence"), "running_without_echo")

    def test_submit_prompt_treats_invoked_click_exception_as_dispatch_unknown_without_running_signal(
        self,
    ) -> None:
        from cccc.ports import web_model_browser_sidecar as sidecar

        class _Button:
            def count(self) -> int:
                return 1

            def is_visible(self, *_args, **_kwargs) -> bool:
                return True

            def is_disabled(self, *_args, **_kwargs) -> bool:
                return False

            def click(self, *_args, **_kwargs) -> None:
                raise RuntimeError("element detached after click")

        class _Locator:
            @property
            def first(self) -> _Button:
                return _Button()

        class _Page:
            def locator(self, _selector: str) -> _Locator:
                return _Locator()

            def evaluate(self, *_args, **_kwargs) -> bool:
                return False

        page = _Page()

        with (
            patch.object(sidecar, "SEND_BUTTON_SELECTORS", ["#composer-submit-button"]),
            patch.object(
                sidecar, "_visible_input_selector", return_value="#prompt-textarea"
            ),
            patch.object(sidecar, "_clear_and_type_prompt"),
            patch.object(sidecar, "_wait_for_prompt_inserted", return_value=True),
            patch.object(
                sidecar,
                "_composer_control_candidate_selector",
                return_value="#composer-submit-button",
            ),
            patch.object(sidecar, "_wait_for_stable_send_control", return_value=True),
            patch.object(sidecar, "_chatgpt_running_visible", return_value=False),
            patch.object(
                sidecar, "_wait_for_submission", return_value="stop_without_echo"
            ),
            patch.object(
                sidecar,
                "_request_submit_composer",
                return_value="form.requestSubmit:button",
            ) as request_submit,
            patch.object(sidecar.time, "sleep", return_value=None),
        ):
            result = sidecar._submit_prompt(
                page, "Browser-delivered CCCC message", input_timeout_seconds=1.0
            )

        request_submit.assert_not_called()
        self.assertEqual(
            result.get("send_selector"),
            "#composer-submit-button:click_dispatch_unknown",
        )
        self.assertEqual(result.get("submission_evidence"), "click_dispatch_unknown")

    def test_submit_prompt_treats_invoked_click_exception_as_unknown_while_already_running(
        self,
    ) -> None:
        from cccc.ports import web_model_browser_sidecar as sidecar

        class _Button:
            def click(self, *_args, **_kwargs) -> None:
                raise RuntimeError("element detached after click")

        class _Locator:
            @property
            def first(self) -> _Button:
                return _Button()

        class _Page:
            def locator(self, _selector: str) -> _Locator:
                return _Locator()

        page = _Page()

        with (
            patch.object(
                sidecar, "SEND_BUTTON_SELECTORS", ['button[aria-label="Send prompt"]']
            ),
            patch.object(
                sidecar, "_visible_input_selector", return_value="#prompt-textarea"
            ),
            patch.object(sidecar, "_clear_and_type_prompt"),
            patch.object(sidecar, "_wait_for_prompt_inserted", return_value=True),
            patch.object(
                sidecar,
                "_composer_control_candidate_selector",
                return_value="#safe-send-prompt",
            ),
            patch.object(sidecar, "_wait_for_stable_send_control", return_value=True),
            patch.object(sidecar, "_chatgpt_running_visible", return_value=True),
            patch.object(
                sidecar, "_wait_for_submission", return_value="stop_without_echo"
            ),
            patch.object(sidecar, "_prompt_present_in_any_composer", return_value=True),
            patch.object(sidecar.time, "sleep", return_value=None),
        ):
            result = sidecar._submit_prompt(
                page, "Browser-delivered CCCC message", input_timeout_seconds=1.0
            )

        self.assertEqual(
            result.get("send_selector"),
            'button[aria-label="Send prompt"]:click_dispatch_unknown',
        )
        self.assertEqual(result.get("submission_evidence"), "click_dispatch_unknown")

    def test_click_send_waits_for_stable_enabled_send_control(self) -> None:
        from cccc.ports import web_model_browser_sidecar as sidecar

        class _Button:
            def __init__(self) -> None:
                self.disabled_checks = 0
                self.clicks = 0

            def count(self) -> int:
                return 1

            def is_visible(self, *_args, **_kwargs) -> bool:
                return True

            def is_disabled(self, *_args, **_kwargs) -> bool:
                self.disabled_checks += 1
                return self.disabled_checks <= 2

            def click(self, *_args, **_kwargs) -> None:
                self.clicks += 1

        class _Locator:
            def __init__(self, button: _Button) -> None:
                self._button = button

            @property
            def first(self) -> _Button:
                return self._button

        class _Page:
            def __init__(self) -> None:
                self.button = _Button()

            def locator(self, _selector: str) -> _Locator:
                return _Locator(self.button)

            def evaluate(self, *_args, **_kwargs) -> bool:
                return False

        page = _Page()

        with (
            patch.object(sidecar, "SEND_BUTTON_SELECTORS", ["#composer-submit-button"]),
            patch.object(
                sidecar,
                "_composer_control_candidate_selector",
                return_value="#composer-submit-button",
            ),
            patch.object(sidecar, "_chatgpt_running_visible", return_value=False),
        ):
            selector = sidecar._click_send(
                page, input_selector="#prompt-textarea", timeout_seconds=1.5
            )

        self.assertEqual(selector, "#composer-submit-button")
        self.assertGreaterEqual(page.button.disabled_checks, 3)
        self.assertEqual(page.button.clicks, 1)

    def test_click_send_skips_stop_candidate_and_clicks_safe_send_prompt_control(
        self,
    ) -> None:
        from cccc.ports import web_model_browser_sidecar as sidecar

        class _Button:
            def __init__(self) -> None:
                self.clicks = 0

            def click(self, *_args, **_kwargs) -> None:
                self.clicks += 1

        class _Locator:
            def __init__(self, button: _Button) -> None:
                self._button = button

            @property
            def first(self) -> _Button:
                return self._button

        class _Page:
            def __init__(self) -> None:
                self.buttons = {"#stop": _Button(), "#send-prompt": _Button()}

            def locator(self, selector: str) -> _Locator:
                return _Locator(self.buttons[selector])

        page = _Page()

        def stable_control(_page, selector: str, **_kwargs) -> bool:
            if selector == "#stop":
                raise sidecar._UnsafeSubmitState("stop control")
            return True

        with (
            patch.object(sidecar, "SEND_BUTTON_SELECTORS", ["#stop", "#send-prompt"]),
            patch.object(
                sidecar,
                "_composer_control_candidate_selector",
                side_effect=lambda _page, _input, selector: selector,
            ),
            patch.object(
                sidecar, "_wait_for_stable_send_control", side_effect=stable_control
            ),
            patch.object(sidecar, "_chatgpt_running_visible", return_value=True),
        ):
            selector = sidecar._click_send(
                page, input_selector="#prompt-textarea", timeout_seconds=1.0
            )

        self.assertEqual(selector, "#send-prompt")
        self.assertEqual(page.buttons["#stop"].clicks, 0)
        self.assertEqual(page.buttons["#send-prompt"].clicks, 1)

    def test_scored_send_candidate_is_confined_to_real_composer_root(self) -> None:
        from cccc.ports import web_model_browser_sidecar as sidecar

        class _Page:
            def __init__(self) -> None:
                self.script = ""
                self.input_selector = ""

            def evaluate(self, script: str, input_selector: str) -> str:
                self.script = script
                self.input_selector = input_selector
                return ""

        page = _Page()

        selector = sidecar._scored_composer_send_selector(page, "#prompt-textarea")

        self.assertEqual(selector, "")
        self.assertEqual(page.input_selector, "#prompt-textarea")
        self.assertIn("composerRoot.querySelectorAll", page.script)
        self.assertNotIn('prompt?.closest?.("main")', page.script)
        self.assertNotIn(
            "...document.querySelectorAll(\"button, [role='button']\")", page.script
        )
        self.assertNotIn("follow", page.script.lower())

    def test_explicit_send_candidate_is_confined_to_real_composer_root(self) -> None:
        from cccc.ports import web_model_browser_sidecar as sidecar

        class _Page:
            def __init__(self) -> None:
                self.script = ""
                self.args: dict[str, str] = {}

            def evaluate(self, script: str, args: dict[str, str]) -> str:
                self.script = script
                self.args = args
                return ""

        page = _Page()

        selector = sidecar._composer_control_candidate_selector(
            page,
            "#prompt-textarea",
            'button[aria-label*="Send"]',
        )

        self.assertEqual(selector, "")
        self.assertEqual(page.args.get("inputSelector"), "#prompt-textarea")
        self.assertIn("composerRoot.querySelectorAll(candidateSelector)", page.script)
        self.assertNotIn("document.querySelectorAll(candidateSelector)", page.script)
        self.assertNotIn('prompt.closest("main")', page.script)

    def test_submit_prompt_accepts_cleared_composer_without_echo(self) -> None:
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

        with (
            patch.object(
                sidecar, "_visible_input_selector", return_value="#prompt-textarea"
            ),
            patch.object(sidecar, "_clear_and_type_prompt"),
            patch.object(sidecar, "_wait_for_prompt_inserted", return_value=True),
            patch.object(
                sidecar, "_click_send", return_value="#composer-submit-button"
            ),
            patch.object(sidecar, "_wait_for_submission", return_value=""),
            patch.object(
                sidecar, "_prompt_present_in_any_composer", return_value=False
            ),
            patch.object(
                sidecar,
                "_request_submit_composer",
                return_value="form.requestSubmit:button",
            ) as request_submit,
            patch.object(sidecar.time, "sleep", return_value=None),
        ):
            result = sidecar._submit_prompt(
                page, "Browser-delivered CCCC message", input_timeout_seconds=1.0
            )

        request_submit.assert_not_called()
        self.assertEqual(page.keyboard.pressed, [])
        self.assertEqual(result.get("submission_evidence"), "composer_cleared")

    def test_submit_prompt_failure_includes_page_diagnostics(self) -> None:
        from cccc.ports import web_model_browser_sidecar as sidecar

        class _Keyboard:
            def press(self, _key: str) -> None:
                return None

        class _Page:
            keyboard = _Keyboard()

        with (
            patch.object(
                sidecar, "_visible_input_selector", return_value="#prompt-textarea"
            ),
            patch.object(sidecar, "_clear_and_type_prompt"),
            patch.object(sidecar, "_wait_for_prompt_inserted", return_value=True),
            patch.object(
                sidecar, "_click_send", return_value="#composer-submit-button"
            ),
            patch.object(sidecar, "_wait_for_submission", return_value=""),
            patch.object(sidecar, "_prompt_present_in_any_composer", return_value=True),
            patch.object(sidecar, "_request_submit_composer", return_value=""),
            patch.object(
                sidecar,
                "_submission_diagnostics",
                return_value={"prompt_chars": 42, "send_enabled_count": 0},
            ),
            patch.object(sidecar.time, "sleep", return_value=None),
        ):
            with self.assertRaisesRegex(
                RuntimeError, "diagnostics=.*send_enabled_count"
            ):
                sidecar._submit_prompt(_Page(), "hello", input_timeout_seconds=1.0)

    def test_submit_prompt_verification_wait_respects_submit_budget(self) -> None:
        from cccc.ports import web_model_browser_sidecar as sidecar

        class _Keyboard:
            def press(self, _key: str) -> None:
                return None

        class _Page:
            keyboard = _Keyboard()

        captured: dict[str, float] = {}

        def wait_for_submission(_page, _selector, *, prompt, timeout_seconds):
            captured["timeout_seconds"] = float(timeout_seconds)
            return ""

        with (
            patch.object(
                sidecar, "_visible_input_selector", return_value="#prompt-textarea"
            ),
            patch.object(sidecar, "_clear_and_type_prompt"),
            patch.object(sidecar, "_wait_for_prompt_inserted", return_value=True),
            patch.object(
                sidecar, "_click_send", return_value="#composer-submit-button"
            ),
            patch.object(
                sidecar, "_wait_for_submission", side_effect=wait_for_submission
            ),
            patch.object(sidecar, "_prompt_present_in_any_composer", return_value=True),
            patch.object(
                sidecar, "_submission_diagnostics", return_value={"prompt_chars": 42}
            ),
            patch.object(sidecar.time, "sleep", return_value=None),
        ):
            with self.assertRaisesRegex(
                RuntimeError, "submission_verification=ambiguous"
            ):
                sidecar._submit_prompt(
                    _Page(),
                    "hello",
                    input_timeout_seconds=30.0,
                    submit_timeout_seconds=3.0,
                )

        self.assertGreater(captured.get("timeout_seconds", 0.0), 0.0)
        self.assertLess(captured.get("timeout_seconds", 99.0), 3.0)

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

            record_chatgpt_browser_state(
                "g-one", "peer1", {"conversation_url": "https://chatgpt.com/c/a"}
            )
            record_chatgpt_browser_state(
                "g-two", "peer2", {"conversation_url": "https://chatgpt.com/c/b"}
            )

            self.assertEqual(
                read_chatgpt_browser_state("g-one", "peer1").get("conversation_url"),
                "https://chatgpt.com/c/a",
            )
            self.assertEqual(
                read_chatgpt_browser_state("g-two", "peer2").get("conversation_url"),
                "https://chatgpt.com/c/b",
            )
        finally:
            cleanup()

    def test_chatgpt_shared_profile_migrates_existing_actor_profile(self) -> None:
        from cccc.ports.web_model_browser_sidecar import (
            chatgpt_browser_actor_state_root,
            chatgpt_browser_profile_dir,
        )

        _, cleanup = self._with_home()
        try:
            legacy = (
                chatgpt_browser_actor_state_root("g-old", "web_model-1")
                / "chrome_profile"
            )
            legacy.mkdir(parents=True, exist_ok=True)
            (legacy / "legacy-login-marker").write_text("migrate", encoding="utf-8")

            migrated = chatgpt_browser_profile_dir("g-new", "chatgpt-web-1")

            self.assertTrue((migrated / "legacy-login-marker").exists())
        finally:
            cleanup()

    def test_chatgpt_shared_profile_migrates_rust_browser_profile(self) -> None:
        from cccc.ports.web_model_browser_sidecar import chatgpt_browser_profile_dir

        home, cleanup = self._with_home()
        try:
            legacy = (
                Path(home) / "browser-profiles" / "web-model" / "g-old" / "web_model-1"
            )
            legacy.mkdir(parents=True, exist_ok=True)
            (legacy / "rust-login-marker").write_text("migrate", encoding="utf-8")

            migrated = chatgpt_browser_profile_dir("g-old", "web_model-1")

            self.assertTrue((migrated / "rust-login-marker").exists())
        finally:
            cleanup()

    def test_browser_target_is_shared_with_rust_group_storage(self) -> None:
        from cccc.kernel.group import load_group
        from cccc.ports.web_model_browser_sidecar import (
            _write_state,
            chatgpt_browser_actor_state_root,
            read_chatgpt_browser_state,
            record_chatgpt_browser_state,
        )

        home, cleanup = self._with_home()
        try:
            group_id = "g-shared"
            actor_id = "web1"
            group_dir = Path(home) / "groups" / group_id
            group_dir.mkdir(parents=True)
            (group_dir / "group.yaml").write_text(
                "\n".join(
                    [
                        "v: 1",
                        f"group_id: {group_id}",
                        "title: Shared target",
                        "actors: []",
                        "web_model_browser_targets:",
                        f"  {actor_id}:",
                        "    state: bound_existing_chat",
                        "    kind: existing_chat",
                        "    url: https://chatgpt.com/c/from-rust",
                        "    saved_at: '2026-08-07T00:00:00Z'",
                        "    next_delivery: existing_chat",
                        "    last_delivery_status: submitted",
                        "    last_delivery_id: rust-delivery",
                        "    last_delivery_turn_id: rust-turn",
                        "    last_delivery_event_ids: [rust-event]",
                        "    last_submission_evidence:",
                        "      submission_evidence: message_echo",
                        "      send_selector: '#composer-submit-button'",
                        "",
                    ]
                ),
                encoding="utf-8",
            )
            _write_state(
                chatgpt_browser_actor_state_root(group_id, actor_id),
                {"conversation_url": "https://chatgpt.com/c/stale-python"},
            )

            loaded = read_chatgpt_browser_state(group_id, actor_id)
            self.assertEqual(
                loaded.get("conversation_url"),
                "https://chatgpt.com/c/from-rust",
            )
            self.assertEqual(loaded.get("last_delivery_status"), "submitted")
            self.assertEqual(loaded.get("last_delivery_id"), "rust-delivery")
            self.assertEqual(loaded.get("last_turn_id"), "rust-turn")
            self.assertEqual(loaded.get("last_event_ids"), ["rust-event"])
            self.assertEqual(loaded.get("last_submission_evidence"), "message_echo")
            self.assertEqual(
                loaded.get("last_send_selector"), "#composer-submit-button"
            )

            record_chatgpt_browser_state(
                group_id,
                actor_id,
                {
                    "conversation_url": "https://chatgpt.com/c/from-python",
                    "pending_new_chat_bind": False,
                    "target_saved_at": "2026-08-07T01:00:00Z",
                },
            )
            group = load_group(group_id)
            self.assertIsNotNone(group)
            target = (group.doc.get("web_model_browser_targets") or {}).get(actor_id)
            self.assertEqual(target.get("url"), "https://chatgpt.com/c/from-python")
            self.assertEqual(target.get("last_delivery_id"), "rust-delivery")
            self.assertEqual(
                (target.get("last_submission_evidence") or {}).get(
                    "submission_evidence"
                ),
                "message_echo",
            )

            record_chatgpt_browser_state(
                group_id,
                actor_id,
                {
                    "last_delivery_at": "2026-08-07T01:01:00Z",
                    "last_delivery_id": "python-delivery",
                    "last_delivery_status": "ambiguous",
                    "last_submission_evidence": "submit_unverified",
                    "last_send_selector": "button[data-testid='send-button']",
                    "last_turn_id": "python-turn",
                    "last_event_ids": ["python-event"],
                    "last_error": "verification timeout",
                },
            )
            group = load_group(group_id)
            target = (group.doc.get("web_model_browser_targets") or {}).get(actor_id)
            self.assertEqual(target.get("last_delivery_id"), "python-delivery")
            self.assertEqual(
                target.get("last_delivery_status"), "submission_ambiguous"
            )
            self.assertEqual(target.get("last_delivery_turn_id"), "python-turn")
            self.assertEqual(target.get("last_delivery_event_ids"), ["python-event"])
            self.assertEqual(
                (target.get("last_submission_evidence") or {}).get(
                    "submission_evidence"
                ),
                "submit_unverified",
            )

            group.doc["web_model_browser_targets"] = {}
            group.save()
            cleared = read_chatgpt_browser_state(group_id, actor_id)
            self.assertEqual(cleared.get("conversation_url"), "")
            self.assertFalse(cleared.get("pending_new_chat_bind"))
            self.assertEqual(cleared.get("last_delivery_status"), "")
            self.assertEqual(cleared.get("last_delivery_id"), "")
            self.assertEqual(cleared.get("last_submission_evidence"), "")
        finally:
            cleanup()

    def test_provisional_rust_target_is_exposed_as_invalid_not_bound(self) -> None:
        from cccc.ports.web_model_browser_sidecar import (
            _write_state,
            chatgpt_browser_actor_state_root,
            read_chatgpt_browser_state,
        )

        home, cleanup = self._with_home()
        try:
            group_id = "g-provisional"
            actor_id = "web1"
            group_dir = Path(home) / "groups" / group_id
            group_dir.mkdir(parents=True)
            (group_dir / "group.yaml").write_text(
                "\n".join(
                    [
                        "v: 1",
                        f"group_id: {group_id}",
                        "title: Provisional target",
                        "actors: []",
                        "web_model_browser_targets:",
                        f"  {actor_id}:",
                        "    state: bound_existing_chat",
                        "    kind: existing_chat",
                        "    url: https://chatgpt.com/c/WEB:temporary",
                        "    saved_at: '2026-08-09T00:00:00Z'",
                        "    next_delivery: existing_chat",
                        "",
                    ]
                ),
                encoding="utf-8",
            )
            _write_state(
                chatgpt_browser_actor_state_root(group_id, actor_id),
                {"conversation_url": "https://chatgpt.com/c/WEB:temporary"},
            )

            loaded = read_chatgpt_browser_state(group_id, actor_id)

            self.assertEqual(loaded.get("conversation_url"), "")
            self.assertEqual(
                loaded.get("invalid_conversation_url"),
                "https://chatgpt.com/c/WEB:temporary",
            )
        finally:
            cleanup()

    def test_resolve_pending_chatgpt_conversation_binds_from_pending_state_url(
        self,
    ) -> None:
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
                    "last_tab_url": "https://chatgpt.com/c/old-chat",
                    "pending_new_chat_last_tab_url": "https://chatgpt.com/c/newly-created?model=gpt-5",
                    "bootstrap_seed_delivered_at": "2026-05-01T00:00:11Z",
                    "bootstrap_seed_conversation_url": "https://chatgpt.com/",
                },
            )

            result = resolve_pending_chatgpt_conversation("g-test", "peer1")

            self.assertTrue(result.get("ok"), result)
            self.assertTrue(result.get("resolved"), result)
            self.assertEqual(
                result.get("conversation_url"), "https://chatgpt.com/c/newly-created"
            )
            self.assertEqual(result.get("delivery_id"), "browser:test")
            self.assertEqual(result.get("turn_id"), "turn-1")
            self.assertEqual(result.get("event_ids"), ["evt-1"])
            state = read_chatgpt_browser_state("g-test", "peer1")
            self.assertEqual(
                state.get("conversation_url"), "https://chatgpt.com/c/newly-created"
            )
            self.assertEqual(state.get("pending_new_chat_bind"), False)
            self.assertEqual(state.get("pending_new_chat_submitted"), False)
            self.assertEqual(state.get("pending_new_chat_delivery_id"), "")
            self.assertEqual(state.get("last_delivery_status"), "bound")
            self.assertEqual(state.get("last_delivery_id"), "browser:test")
            self.assertEqual(state.get("last_turn_id"), "turn-1")
            self.assertEqual(state.get("last_event_ids"), ["evt-1"])
            self.assertEqual(
                state.get("bootstrap_seed_conversation_url"),
                "https://chatgpt.com/c/newly-created",
            )
        finally:
            cleanup()

    def test_resolve_pending_chatgpt_conversation_ignores_stale_last_tab_after_submit(
        self,
    ) -> None:
        from cccc.ports.web_model_browser_sidecar import (
            read_chatgpt_browser_state,
            record_chatgpt_browser_process_state,
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
                    "pending_new_chat_submitted": True,
                    "pending_new_chat_submitted_at": "2026-05-01T00:00:10Z",
                    "pending_new_chat_delivery_id": "browser:test",
                    "pending_new_chat_last_tab_url": "",
                    "last_tab_url": "https://chatgpt.com/c/old-chat",
                },
            )
            record_chatgpt_browser_process_state({"cdp_port": 0})

            result = resolve_pending_chatgpt_conversation("g-test", "peer1")

            self.assertTrue(result.get("ok"), result)
            self.assertFalse(result.get("resolved"), result)
            self.assertTrue(result.get("pending"), result)
            self.assertTrue(result.get("submitted"), result)
            state = read_chatgpt_browser_state("g-test", "peer1")
            self.assertEqual(state.get("conversation_url"), "")
            self.assertEqual(state.get("pending_new_chat_bind"), True)
        finally:
            cleanup()

    def test_resolve_pending_chatgpt_conversation_ignores_stale_tab_before_submit(
        self,
    ) -> None:
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
        self.assertEqual(
            cm.playwright.chromium.calls,
            [("http://127.0.0.1:9222", {"timeout": sidecar.CDP_CONNECT_TIMEOUT_MS})],
        )

    def test_pending_conversation_resolution_uses_short_cdp_connect_timeout(
        self,
    ) -> None:
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
            sidecar.record_chatgpt_browser_process_state(
                {"cdp_port": 9223, "profile_dir": "/tmp/profile"}
            )
            cm = _CM()
            with (
                patch.object(sidecar, "_wait_cdp_endpoint", return_value=True),
                patch.object(
                    sidecar, "ensure_sync_playwright", return_value=lambda: cm
                ),
            ):
                result = sidecar.resolve_pending_chatgpt_conversation("g-test", "peer1")

            self.assertTrue(result.get("pending"))
            self.assertEqual(
                cm.playwright.chromium.calls,
                [
                    (
                        "http://127.0.0.1:9223",
                        {"timeout": sidecar.CDP_CONNECT_TIMEOUT_MS},
                    )
                ],
            )
        finally:
            cleanup()

    def test_projected_chatgpt_session_requires_system_browser_cdp(self) -> None:
        from cccc.daemon.actors import web_model_browser_session

        _, cleanup = self._with_home()
        try:
            with (
                patch.object(web_model_browser_session.sys, "platform", "linux"),
                patch.object(
                    web_model_browser_session._MANAGER,
                    "open",
                    return_value={"active": True, "metadata": {}},
                ) as open_session,
                patch.object(
                    web_model_browser_session,
                    "close_chatgpt_browser_session",
                    return_value={"active": False},
                ) as close_browser,
            ):
                web_model_browser_session.open_web_model_chatgpt_browser_session(
                    group_id="g-test",
                    actor_id="peer1",
                    width=1280,
                    height=800,
                )

            kwargs = open_session.call_args.kwargs
            self.assertEqual(kwargs.get("key"), "chatgpt_web")
            self.assertEqual(
                tuple(kwargs.get("channel_candidates") or ()), ("chrome", "msedge")
            )
            self.assertEqual(kwargs.get("system_profile_subdir"), "")
            self.assertEqual(kwargs.get("require_system_browser_cdp"), True)
            self.assertEqual(kwargs.get("headless"), False)
            close_browser.assert_called_once_with("g-test", "peer1")
        finally:
            cleanup()

    def test_projected_chatgpt_session_is_headless_on_macos(self) -> None:
        from cccc.daemon.actors import web_model_browser_session
        from cccc.ports import web_model_browser_sidecar as sidecar

        _, cleanup = self._with_home()
        try:
            sidecar.record_chatgpt_browser_process_state(
                {
                    "pid": 1234,
                    "cdp_port": 9222,
                    "profile_dir": str(
                        sidecar.chatgpt_browser_profile_dir("g-test", "peer1")
                    ),
                    "visibility": "projected",
                }
            )
            with (
                patch.object(web_model_browser_session.sys, "platform", "darwin"),
                patch.dict(os.environ, {}, clear=True),
                patch.object(
                    web_model_browser_session._MANAGER,
                    "open",
                    return_value={
                        "active": True,
                        "state": "ready",
                        "metadata": {"cdp_port": 9445, "headless": True},
                    },
                ) as open_session,
                patch.object(
                    web_model_browser_session,
                    "close_chatgpt_browser_session",
                    return_value={"active": False},
                ) as close_browser,
            ):
                web_model_browser_session.open_web_model_chatgpt_browser_session(
                    group_id="g-test",
                    actor_id="peer1",
                    width=1280,
                    height=800,
                )

            kwargs = open_session.call_args.kwargs
            self.assertEqual(kwargs.get("headless"), True)
            self.assertEqual(kwargs.get("require_system_browser_cdp"), True)
            self.assertEqual(kwargs.get("existing_cdp_port"), 0)
            close_browser.assert_called_once_with("g-test", "peer1")
        finally:
            cleanup()

    def test_macos_headless_policy_can_be_disabled_for_login_repair(self) -> None:
        from cccc.daemon.actors.web_model_browser_launch_policy import (
            use_headless_projected_browser,
        )

        with patch.dict(
            os.environ,
            {"CCCC_WEB_MODEL_BROWSER_HEADLESS": "0"},
            clear=True,
        ):
            self.assertFalse(use_headless_projected_browser(platform="darwin"))

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
                patch.object(
                    web_model_browser_session._MANAGER,
                    "open",
                    return_value={"active": True, "metadata": {}},
                ) as open_session,
                patch.object(
                    web_model_browser_session,
                    "close_chatgpt_browser_session",
                    return_value={"active": False},
                ),
            ):
                web_model_browser_session.open_web_model_chatgpt_browser_session(
                    group_id="g-test",
                    actor_id="peer1",
                    width=1280,
                    height=800,
                )

            self.assertEqual(
                open_session.call_args.kwargs.get("url"),
                "https://chatgpt.com/c/bound-chat",
            )
        finally:
            cleanup()

    def test_projected_chatgpt_session_without_saved_target_ignores_last_tab(
        self,
    ) -> None:
        from cccc.daemon.actors import web_model_browser_session
        from cccc.ports.web_model_browser_sidecar import record_chatgpt_browser_state

        _, cleanup = self._with_home()
        try:
            record_chatgpt_browser_state(
                "g-test",
                "peer1",
                {
                    "conversation_url": "",
                    "pending_new_chat_bind": False,
                    "last_tab_url": "https://chatgpt.com/c/old-chat",
                },
            )
            with (
                patch.object(
                    web_model_browser_session._MANAGER,
                    "open",
                    return_value={"active": True, "metadata": {}},
                ) as open_session,
                patch.object(
                    web_model_browser_session,
                    "close_chatgpt_browser_session",
                    return_value={"active": False},
                ),
            ):
                web_model_browser_session.open_web_model_chatgpt_browser_session(
                    group_id="g-test",
                    actor_id="peer1",
                    width=1280,
                    height=800,
                )

            self.assertEqual(
                open_session.call_args.kwargs.get("url"), "https://chatgpt.com/"
            )
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
                patch.object(
                    web_model_browser_session._MANAGER,
                    "open",
                    return_value={"active": True, "metadata": {}},
                ) as open_session,
                patch.object(
                    web_model_browser_session,
                    "close_chatgpt_browser_session",
                    return_value={"active": False},
                ),
            ):
                web_model_browser_session.open_web_model_chatgpt_browser_session(
                    group_id="g-test",
                    actor_id="peer1",
                    width=1280,
                    height=800,
                )

            self.assertEqual(
                open_session.call_args.kwargs.get("url"), "https://chatgpt.com/"
            )
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
                patch.object(
                    web_model_browser_session._MANAGER, "info", return_value=existing
                ),
                patch.object(
                    web_model_browser_session._MANAGER,
                    "open",
                    return_value={"active": True},
                ) as open_session,
                patch.object(
                    web_model_browser_session, "close_chatgpt_browser_session"
                ) as close_browser,
                patch.object(
                    web_model_browser_session,
                    "ensure_web_model_browser_recovery_watcher",
                    return_value=True,
                ),
            ):
                result = (
                    web_model_browser_session.open_web_model_chatgpt_browser_session(
                        group_id="g-test",
                        actor_id="peer1",
                        width=1280,
                        height=800,
                    )
                )

            self.assertEqual(result, existing)
            open_session.assert_not_called()
            close_browser.assert_not_called()
        finally:
            cleanup()

    def test_projected_chatgpt_session_realigns_active_instance_to_saved_chat(
        self,
    ) -> None:
        from cccc.daemon.actors import web_model_browser_session
        from cccc.ports.web_model_browser_sidecar import record_chatgpt_browser_state

        _, cleanup = self._with_home()
        try:
            record_chatgpt_browser_state(
                "g-test",
                "peer1",
                {"conversation_url": "https://chatgpt.com/c/bound-chat"},
            )
            existing = {
                "active": True,
                "state": "ready",
                "url": "https://chatgpt.com/",
                "metadata": {"cdp_port": 9222},
            }
            aligned = {
                **existing,
                "url": "https://chatgpt.com/c/bound-chat",
            }
            with (
                patch.object(
                    web_model_browser_session._MANAGER,
                    "info",
                    side_effect=[existing, aligned],
                ),
                patch.object(
                    web_model_browser_session._MANAGER, "execute", return_value={"ok": True}
                ) as execute,
                patch.object(
                    web_model_browser_session,
                    "ensure_web_model_browser_recovery_watcher",
                    return_value=True,
                ),
            ):
                result = web_model_browser_session.open_web_model_chatgpt_browser_session(
                    group_id="g-test",
                    actor_id="peer1",
                    width=1280,
                    height=800,
                )

            self.assertEqual(result.get("url"), "https://chatgpt.com/c/bound-chat")
            execute.assert_called_once_with(
                key="chatgpt_web",
                kind="navigate",
                payload={"url": "https://chatgpt.com/c/bound-chat"},
                timeout=35.0,
            )
        finally:
            cleanup()

    def test_projected_chatgpt_failed_startup_does_not_persist_dead_cdp_metadata(
        self,
    ) -> None:
        from cccc.daemon.actors import web_model_browser_session
        from cccc.ports import web_model_browser_sidecar as sidecar

        _, cleanup = self._with_home()
        try:
            sidecar.record_chatgpt_browser_process_state(
                {
                    "pid": 1234,
                    "cdp_port": 9222,
                    "profile_dir": str(
                        sidecar.chatgpt_browser_profile_dir("g-test", "peer1")
                    ),
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
                        "profile_dir": str(
                            sidecar.chatgpt_browser_profile_dir("g-test", "peer1")
                        ),
                    },
                },
            )

            state = sidecar.read_chatgpt_browser_process_state()
            self.assertEqual(int(state.get("pid") or 0), 0)
            self.assertEqual(int(state.get("cdp_port") or 0), 0)
            self.assertEqual(str(state.get("visibility") or ""), "projected")
        finally:
            cleanup()

    def test_projected_chatgpt_ready_state_persists_display_ownership(self) -> None:
        from cccc.daemon.actors import web_model_browser_session
        from cccc.ports import web_model_browser_sidecar as sidecar

        _, cleanup = self._with_home()
        try:
            web_model_browser_session._record_projected_browser_state(
                "g-test",
                "peer1",
                {
                    "active": True,
                    "state": "ready",
                    "url": "https://chatgpt.com/",
                    "metadata": {
                        "pid": 4321,
                        "cdp_port": 9333,
                        "browser_binary": "/usr/bin/google-chrome",
                        "display": ":123",
                        "display_owned": True,
                        "display_owner": "cccc_xvfb",
                    },
                },
            )

            state = sidecar.read_chatgpt_browser_process_state()
            self.assertEqual(state.get("display"), ":123")
            self.assertEqual(state.get("display_owned"), True)
            self.assertEqual(state.get("display_owner"), "cccc_xvfb")
        finally:
            cleanup()

    def test_projected_chatgpt_session_closes_stale_starting_instance_before_open(
        self,
    ) -> None:
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
                patch.object(
                    web_model_browser_session._MANAGER,
                    "info",
                    side_effect=[stale, {"active": False, "state": "idle"}],
                ),
                patch.object(
                    web_model_browser_session._MANAGER,
                    "close",
                    return_value={"closed": True},
                ) as close_manager,
                patch.object(
                    web_model_browser_session._MANAGER, "open", return_value=opened
                ) as open_session,
                patch.object(
                    web_model_browser_session, "close_chatgpt_browser_session"
                ) as close_browser,
                patch.object(
                    web_model_browser_session,
                    "ensure_web_model_browser_recovery_watcher",
                    return_value=True,
                ),
            ):
                result = (
                    web_model_browser_session.open_web_model_chatgpt_browser_session(
                        group_id="g-test",
                        actor_id="peer1",
                        width=1280,
                        height=800,
                    )
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
                    "display": ":123",
                    "display_owned": True,
                    "display_owner": "cccc_xvfb",
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
                patch.object(web_model_browser_session.sys, "platform", "linux"),
                patch.object(
                    web_model_browser_session._MANAGER,
                    "info",
                    return_value={"active": False, "state": "idle"},
                ),
                patch.object(
                    web_model_browser_session, "_wait_cdp_endpoint", return_value=True
                ),
                patch.object(
                    web_model_browser_session._MANAGER, "open", return_value=opened
                ) as open_session,
                patch.object(
                    web_model_browser_session, "close_chatgpt_browser_session"
                ) as close_browser,
                patch.object(
                    web_model_browser_session,
                    "ensure_web_model_browser_recovery_watcher",
                    return_value=True,
                ),
            ):
                result = (
                    web_model_browser_session.open_web_model_chatgpt_browser_session(
                        group_id="g-test",
                        actor_id="peer1",
                        width=1280,
                        height=800,
                    )
                )

            self.assertEqual(result, opened)
            kwargs = open_session.call_args.kwargs
            self.assertEqual(kwargs.get("existing_cdp_port"), 9222)
            self.assertEqual(
                (kwargs.get("existing_browser_metadata") or {}).get("pid"), 1234
            )
            close_browser.assert_not_called()
        finally:
            cleanup()

    def test_projected_chatgpt_session_does_not_adopt_unisolated_linux_process(
        self,
    ) -> None:
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
                "url": "https://chatgpt.com/",
                "metadata": {"cdp_port": 9333, "pid": 4321, "display_owned": True},
            }
            with (
                patch.object(web_model_browser_session.sys, "platform", "linux"),
                patch.object(
                    web_model_browser_session._MANAGER,
                    "info",
                    return_value={"active": False, "state": "idle"},
                ),
                patch.object(
                    web_model_browser_session, "_wait_cdp_endpoint", return_value=True
                ),
                patch.object(
                    web_model_browser_session._MANAGER, "open", return_value=opened
                ) as open_session,
                patch.object(
                    web_model_browser_session, "close_chatgpt_browser_session"
                ) as close_browser,
                patch.object(
                    web_model_browser_session,
                    "ensure_web_model_browser_recovery_watcher",
                    return_value=True,
                ),
            ):
                result = (
                    web_model_browser_session.open_web_model_chatgpt_browser_session(
                        group_id="g-test",
                        actor_id="peer1",
                        width=1280,
                        height=800,
                    )
                )

            self.assertEqual(result, opened)
            self.assertEqual(open_session.call_args.kwargs.get("existing_cdp_port"), 0)
            self.assertEqual(
                open_session.call_args.kwargs.get("existing_browser_metadata"), {}
            )
            close_browser.assert_called_once_with("g-test", "peer1")
        finally:
            cleanup()

    def test_projected_chatgpt_session_is_global_across_actor_ids(self) -> None:
        from cccc.daemon.actors import web_model_browser_session

        _, cleanup = self._with_home()
        try:
            with (
                patch.object(
                    web_model_browser_session._MANAGER,
                    "info",
                    return_value={"active": False, "state": "idle"},
                ),
                patch.object(
                    web_model_browser_session._MANAGER,
                    "open",
                    return_value={"active": True, "metadata": {}},
                ) as open_session,
                patch.object(
                    web_model_browser_session,
                    "close_chatgpt_browser_session",
                    return_value={"active": False},
                ),
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
        from cccc.ports.web_model_browser_sidecar import (
            read_chatgpt_browser_state,
            record_chatgpt_browser_state,
        )

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
                patch.object(
                    web_model_browser_session, "close_web_model_chatgpt_browser_session"
                ) as close_session,
                patch.object(
                    web_model_browser_session, "stop_web_model_browser_recovery_watcher"
                ) as stop_watcher,
            ):
                web_model_browser_session.clear_web_model_chatgpt_browser_actor_runtime(
                    group_id="g-test", actor_id="peer1"
                )

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
                patch.object(
                    web_model_browser_session._MANAGER,
                    "info",
                    return_value={"active": True, "metadata": {"cdp_port": 9222}},
                ),
                patch.object(
                    web_model_browser_session._MANAGER,
                    "close",
                    return_value={"closed": True, "browser_surface": {"active": False}},
                ),
                patch.object(
                    web_model_browser_session,
                    "close_chatgpt_browser_session",
                    return_value={"active": False},
                ) as close_browser,
            ):
                result = (
                    web_model_browser_session.close_web_model_chatgpt_browser_session(
                        group_id="g-test",
                        actor_id="peer1",
                    )
                )

            self.assertTrue(result.get("closed"))
            close_browser.assert_called_once_with("g-test", "peer1")
        finally:
            cleanup()

    def test_projected_chatgpt_background_writes_skip_during_delivery_lock(
        self,
    ) -> None:
        from cccc.daemon.actors import web_model_browser_session

        class BusyLock:
            def acquire(self, blocking=True):
                self.blocking = blocking
                return False

            def release(self):  # pragma: no cover - should not be called
                raise AssertionError("busy lock should not be released by caller")

        with (
            patch.object(web_model_browser_session, "_SESSION_WRITE_LOCK", BusyLock()),
            patch.object(
                web_model_browser_session._MANAGER,
                "info",
                return_value={"active": True, "state": "ready"},
            ) as info,
            patch.object(web_model_browser_session._MANAGER, "execute") as execute,
        ):
            reload_result = (
                web_model_browser_session.reload_web_model_chatgpt_browser_session(
                    group_id="g-test",
                    actor_id="peer1",
                    target_url="https://chatgpt.com/c/test",
                )
            )

        self.assertEqual(reload_result.get("error"), "browser_delivery_in_progress")
        info.assert_called_once()
        execute.assert_not_called()

    def test_close_chatgpt_browser_session_cleans_profile_processes_when_pid_state_is_stale(
        self,
    ) -> None:
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
            with patch.object(
                sidecar, "_stop_browser_profile_processes"
            ) as stop_profile:
                sidecar.close_chatgpt_browser_session("g-test", "peer1")

            stop_profile.assert_called_once_with(str(profile_dir))
        finally:
            cleanup()

    def test_profile_process_detection_parses_posix_and_windows_user_data_dir(
        self,
    ) -> None:
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
            sidecar._user_data_dir_from_args(
                ["chrome", "--user-data-dir", "/tmp/cccc-profile"]
            ),
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
