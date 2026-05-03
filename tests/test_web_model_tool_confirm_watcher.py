import os
import tempfile
import unittest
from unittest.mock import Mock, patch


class TestWebModelToolConfirmWatcher(unittest.TestCase):
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

    def test_interval_defaults_to_five_seconds_and_clamps(self) -> None:
        from cccc.daemon.actors import web_model_tool_confirm_watcher as watcher

        old = os.environ.get("CCCC_WEB_MODEL_AUTO_CONFIRM_INTERVAL_SECONDS")
        try:
            os.environ.pop("CCCC_WEB_MODEL_AUTO_CONFIRM_INTERVAL_SECONDS", None)
            self.assertEqual(watcher.web_model_tool_auto_confirm_interval_seconds(), 5.0)
            os.environ["CCCC_WEB_MODEL_AUTO_CONFIRM_INTERVAL_SECONDS"] = "1"
            self.assertEqual(watcher.web_model_tool_auto_confirm_interval_seconds(), 3.0)
            os.environ["CCCC_WEB_MODEL_AUTO_CONFIRM_INTERVAL_SECONDS"] = "100"
            self.assertEqual(watcher.web_model_tool_auto_confirm_interval_seconds(), 60.0)
        finally:
            if old is None:
                os.environ.pop("CCCC_WEB_MODEL_AUTO_CONFIRM_INTERVAL_SECONDS", None)
            else:
                os.environ["CCCC_WEB_MODEL_AUTO_CONFIRM_INTERVAL_SECONDS"] = old

    def test_auto_confirm_can_be_disabled_by_env(self) -> None:
        from cccc.daemon.actors import web_model_tool_confirm_watcher as watcher

        old = os.environ.get("CCCC_WEB_MODEL_AUTO_CONFIRM_TOOLS")
        try:
            os.environ["CCCC_WEB_MODEL_AUTO_CONFIRM_TOOLS"] = "0"
            self.assertFalse(watcher.web_model_tool_auto_confirm_enabled())
            self.assertFalse(watcher.ensure_web_model_tool_confirm_watcher("g-test", "peer1"))
        finally:
            if old is None:
                os.environ.pop("CCCC_WEB_MODEL_AUTO_CONFIRM_TOOLS", None)
            else:
                os.environ["CCCC_WEB_MODEL_AUTO_CONFIRM_TOOLS"] = old

    def test_ensure_watcher_starts_only_when_cdp_is_active(self) -> None:
        from cccc.daemon.actors import web_model_tool_confirm_watcher as watcher

        _, cleanup = self._with_home()
        try:
            watcher.stop_all_web_model_tool_confirm_watchers()
            with patch.object(watcher, "_active_cdp_port", return_value=0), patch.object(watcher.threading, "Thread") as thread_cls:
                self.assertFalse(watcher.ensure_web_model_tool_confirm_watcher("g-test", "peer1"))
                thread_cls.assert_not_called()

            fake_thread = Mock()
            fake_thread.is_alive.return_value = True
            with patch.object(watcher, "_active_cdp_port", return_value=9222), patch.object(
                watcher.threading,
                "Thread",
                return_value=fake_thread,
            ) as thread_cls:
                self.assertTrue(watcher.ensure_web_model_tool_confirm_watcher("g-test", "peer1"))
                thread_cls.assert_called_once()
                fake_thread.start.assert_called_once()
        finally:
            watcher.stop_all_web_model_tool_confirm_watchers()
            cleanup()

    def test_scan_diagnostics_are_recorded_when_click_fails(self) -> None:
        from cccc.daemon.actors import web_model_tool_confirm_watcher as watcher
        from cccc.ports.web_model_browser_sidecar import read_chatgpt_browser_state

        _, cleanup = self._with_home()
        try:
            watcher._record_auto_confirm_scan(
                "g-test",
                "peer1",
                {
                    "clicked": 0,
                    "candidate_count": 1,
                    "pages_seen": 1,
                    "errors": [{"title": "Add decision?", "error": "blocked"}],
                },
            )

            state = read_chatgpt_browser_state("g-test", "peer1")
            self.assertEqual(state.get("auto_confirm_candidate_count"), 1)
            self.assertEqual(state.get("auto_confirm_pages_seen"), 1)
            self.assertEqual((state.get("auto_confirm_last_errors") or [{}])[0].get("title"), "Add decision?")
        finally:
            cleanup()

    def test_reload_window_state_helpers_start_progress_and_close(self) -> None:
        from cccc.daemon.actors import web_model_tool_confirm_watcher as watcher
        from cccc.ports.web_model_browser_sidecar import read_chatgpt_browser_state

        _, cleanup = self._with_home()
        try:
            watcher.start_web_model_browser_reload_window(
                "g-test",
                "peer1",
                reason="browser_delivery",
                delivery_id="delivery-1",
                turn_id="turn-1",
                event_ids=["e1"],
                target_url="https://chatgpt.com/c/test",
            )
            state = read_chatgpt_browser_state("g-test", "peer1")
            self.assertEqual(state.get("auto_reload_active"), True)
            self.assertEqual(state.get("auto_reload_last_progress_reason"), "browser_delivery")
            self.assertEqual(state.get("auto_reload_last_delivery_id"), "delivery-1")
            self.assertEqual(state.get("auto_reload_last_event_ids"), ["e1"])

            self.assertTrue(watcher.record_web_model_browser_progress("g-test", "peer1", reason="mcp_tool", detail="cccc_code_exec"))
            state = read_chatgpt_browser_state("g-test", "peer1")
            self.assertEqual(state.get("auto_reload_last_progress_reason"), "mcp_tool")
            self.assertEqual(state.get("auto_reload_last_progress_detail"), "cccc_code_exec")

            self.assertTrue(watcher.close_web_model_browser_reload_window("g-test", "peer1", reason="complete_turn:done"))
            state = read_chatgpt_browser_state("g-test", "peer1")
            self.assertEqual(state.get("auto_reload_active"), False)
            self.assertEqual(state.get("auto_reload_completed_reason"), "complete_turn:done")
        finally:
            cleanup()

    def test_stale_reload_window_refreshes_bound_chatgpt_page(self) -> None:
        from cccc.daemon.actors import web_model_tool_confirm_watcher as watcher
        from cccc.ports.web_model_browser_sidecar import read_chatgpt_browser_state, record_chatgpt_browser_state

        class FakePage:
            url = "https://chatgpt.com/c/test"

            def __init__(self) -> None:
                self.reload_calls = 0
                self.goto_calls = []

            def reload(self, *, wait_until: str, timeout: int) -> None:
                _ = (wait_until, timeout)
                self.reload_calls += 1

            def goto(self, url: str, *, wait_until: str, timeout: int) -> None:
                _ = (wait_until, timeout)
                self.goto_calls.append(url)
                self.url = url

        class FakeContext:
            def __init__(self, page: FakePage) -> None:
                self.pages = [page]

        class FakeBrowser:
            def __init__(self, page: FakePage) -> None:
                self.contexts = [FakeContext(page)]

        _, cleanup = self._with_home()
        try:
            record_chatgpt_browser_state(
                "g-test",
                "peer1",
                {
                    "conversation_url": "https://chatgpt.com/c/test",
                    "auto_reload_active": True,
                    "auto_reload_window_started_at": "2026-05-03T00:00:00Z",
                    "auto_reload_window_expires_at": "2099-01-01T00:00:00Z",
                    "auto_reload_last_progress_at": "2026-05-03T00:00:00Z",
                },
            )
            page = FakePage()

            result = watcher._maybe_reload_stale_chatgpt_page(
                "g-test",
                "peer1",
                browser=FakeBrowser(page),
                target_url="https://chatgpt.com/c/test",
                preferred_page=page,
            )

            self.assertTrue(result.get("reloaded"), result)
            self.assertEqual(page.reload_calls, 1)
            self.assertEqual(page.goto_calls, [])
            state = read_chatgpt_browser_state("g-test", "peer1")
            self.assertEqual(state.get("auto_reload_count"), 1)
            self.assertEqual(state.get("auto_reload_last_reload_reason"), "no_progress_timeout")
            self.assertEqual(state.get("auto_reload_last_progress_reason"), "auto_reload")
        finally:
            cleanup()

    def test_stale_reload_window_uses_stored_target_when_conversation_is_pending(self) -> None:
        from cccc.daemon.actors import web_model_tool_confirm_watcher as watcher
        from cccc.ports.web_model_browser_sidecar import read_chatgpt_browser_state, record_chatgpt_browser_state

        class FakePage:
            url = "https://chatgpt.com/"

            def __init__(self) -> None:
                self.reload_calls = 0
                self.goto_calls = []

            def reload(self, *, wait_until: str, timeout: int) -> None:
                _ = (wait_until, timeout)
                self.reload_calls += 1

            def goto(self, url: str, *, wait_until: str, timeout: int) -> None:
                _ = (wait_until, timeout)
                self.goto_calls.append(url)
                self.url = url

        class FakeContext:
            def __init__(self, page: FakePage) -> None:
                self.pages = [page]

        class FakeBrowser:
            def __init__(self, page: FakePage) -> None:
                self.contexts = [FakeContext(page)]

        _, cleanup = self._with_home()
        try:
            record_chatgpt_browser_state(
                "g-test",
                "peer1",
                {
                    "conversation_url": "",
                    "auto_reload_target_url": "https://chatgpt.com/",
                    "auto_reload_active": True,
                    "auto_reload_window_started_at": "2026-05-03T00:00:00Z",
                    "auto_reload_window_expires_at": "2099-01-01T00:00:00Z",
                    "auto_reload_last_progress_at": "2026-05-03T00:00:00Z",
                },
            )
            page = FakePage()

            result = watcher._maybe_reload_stale_chatgpt_page(
                "g-test",
                "peer1",
                browser=FakeBrowser(page),
                target_url="",
                preferred_page=page,
            )

            self.assertTrue(result.get("reloaded"), result)
            self.assertEqual(page.reload_calls, 1)
            self.assertEqual(page.goto_calls, [])
            state = read_chatgpt_browser_state("g-test", "peer1")
            self.assertEqual(state.get("auto_reload_last_page_url"), "https://chatgpt.com/")
            self.assertEqual(state.get("auto_reload_last_progress_reason"), "auto_reload")
        finally:
            cleanup()

    def test_auto_confirm_scan_does_not_block_stale_reload(self) -> None:
        from cccc.daemon.actors import web_model_tool_confirm_watcher as watcher
        from cccc.ports import web_model_browser_sidecar as sidecar

        class FakePage:
            url = "https://chatgpt.com/c/test"

            def __init__(self) -> None:
                self.reload_calls = 0

            def reload(self, *, wait_until: str, timeout: int) -> None:
                _ = (wait_until, timeout)
                self.reload_calls += 1

            def goto(self, url: str, *, wait_until: str, timeout: int) -> None:
                _ = (wait_until, timeout)
                self.url = url

        fake_page = FakePage()

        class FakeContext:
            def __init__(self, page: FakePage) -> None:
                self.pages = [page]

        class FakeBrowser:
            def __init__(self, page: FakePage) -> None:
                self.contexts = [FakeContext(page)]

        class FakeChromium:
            def connect_over_cdp(self, _endpoint: str):
                return FakeBrowser(fake_page)

        class FakePlaywright:
            chromium = FakeChromium()

        class FakePlaywrightContext:
            def __enter__(self):
                return FakePlaywright()

            def __exit__(self, _exc_type, _exc, _tb):
                return False

        _, cleanup = self._with_home()
        try:
            sidecar.record_chatgpt_browser_process_state({"cdp_port": 9222})
            sidecar.record_chatgpt_browser_state(
                "g-test",
                "peer1",
                {
                    "conversation_url": "https://chatgpt.com/c/test",
                    "auto_reload_active": True,
                    "auto_reload_window_started_at": "2026-05-03T00:00:00Z",
                    "auto_reload_window_expires_at": "2099-01-01T00:00:00Z",
                    "auto_reload_last_progress_at": "2026-05-03T00:00:00Z",
                    "auto_reload_last_progress_reason": "browser_delivery",
                },
            )
            with patch.object(watcher, "_wait_cdp_endpoint", return_value=True), patch.object(
                watcher,
                "ensure_sync_playwright",
                return_value=lambda: FakePlaywrightContext(),
            ), patch.object(
                watcher.browser_sidecar,
                "_auto_confirm_page_tool_prompts",
                return_value={"clicked": 0, "candidate_count": 1, "errors": [{"title": "Confirm?", "error": "blocked"}]},
            ):
                result = watcher.auto_confirm_chatgpt_tool_prompts("g-test", "peer1")

            self.assertEqual(result.get("clicked"), 0)
            self.assertEqual((result.get("auto_reload") or {}).get("reloaded"), True)
            self.assertEqual(fake_page.reload_calls, 1)
            state = sidecar.read_chatgpt_browser_state("g-test", "peer1")
            self.assertEqual(state.get("auto_confirm_candidate_count"), 1)
            self.assertEqual(state.get("auto_reload_last_progress_reason"), "auto_reload")
        finally:
            cleanup()


if __name__ == "__main__":
    unittest.main()
