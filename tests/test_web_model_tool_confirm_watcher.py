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

    def test_interval_defaults_to_three_seconds_and_clamps(self) -> None:
        from cccc.daemon.actors import web_model_tool_confirm_watcher as watcher

        old = os.environ.get("CCCC_WEB_MODEL_AUTO_CONFIRM_INTERVAL_SECONDS")
        try:
            os.environ.pop("CCCC_WEB_MODEL_AUTO_CONFIRM_INTERVAL_SECONDS", None)
            self.assertEqual(watcher.web_model_tool_auto_confirm_interval_seconds(), 3.0)
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


if __name__ == "__main__":
    unittest.main()
