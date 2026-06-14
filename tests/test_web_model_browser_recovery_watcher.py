import os
import tempfile
import unittest
from unittest.mock import Mock, patch


class TestWebModelBrowserRecoveryWatcher(unittest.TestCase):
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

    def _with_auto_reload(self, value: str | None):
        old = os.environ.get("CCCC_WEB_MODEL_BROWSER_AUTO_RELOAD")
        if value is None:
            os.environ.pop("CCCC_WEB_MODEL_BROWSER_AUTO_RELOAD", None)
        else:
            os.environ["CCCC_WEB_MODEL_BROWSER_AUTO_RELOAD"] = value

        def cleanup() -> None:
            if old is None:
                os.environ.pop("CCCC_WEB_MODEL_BROWSER_AUTO_RELOAD", None)
            else:
                os.environ["CCCC_WEB_MODEL_BROWSER_AUTO_RELOAD"] = old

        return cleanup

    def test_auto_reload_defaults_disabled_and_requires_truthy_env(self) -> None:
        from cccc.daemon.actors import web_model_browser_recovery_watcher as watcher

        cleanup_env = self._with_auto_reload(None)
        try:
            self.assertFalse(watcher.web_model_browser_auto_reload_enabled())
            self.assertFalse(watcher.ensure_web_model_browser_recovery_watcher("g-test", "peer1"))
        finally:
            cleanup_env()

        for value in ("1", "true", "yes", "on", "enabled"):
            cleanup_env = self._with_auto_reload(value)
            try:
                self.assertTrue(watcher.web_model_browser_auto_reload_enabled())
            finally:
                cleanup_env()

        for value in ("0", "false", "no", "off", "disabled", ""):
            cleanup_env = self._with_auto_reload(value)
            try:
                self.assertFalse(watcher.web_model_browser_auto_reload_enabled())
            finally:
                cleanup_env()

    def test_recovery_interval_defaults_to_five_seconds_and_clamps(self) -> None:
        from cccc.daemon.actors import web_model_browser_recovery_watcher as watcher

        old = os.environ.get("CCCC_WEB_MODEL_BROWSER_RECOVERY_INTERVAL_SECONDS")
        try:
            os.environ.pop("CCCC_WEB_MODEL_BROWSER_RECOVERY_INTERVAL_SECONDS", None)
            self.assertEqual(watcher.web_model_browser_recovery_interval_seconds(), 5.0)
            os.environ["CCCC_WEB_MODEL_BROWSER_RECOVERY_INTERVAL_SECONDS"] = "1"
            self.assertEqual(watcher.web_model_browser_recovery_interval_seconds(), 3.0)
            os.environ["CCCC_WEB_MODEL_BROWSER_RECOVERY_INTERVAL_SECONDS"] = "100"
            self.assertEqual(watcher.web_model_browser_recovery_interval_seconds(), 60.0)
        finally:
            if old is None:
                os.environ.pop("CCCC_WEB_MODEL_BROWSER_RECOVERY_INTERVAL_SECONDS", None)
            else:
                os.environ["CCCC_WEB_MODEL_BROWSER_RECOVERY_INTERVAL_SECONDS"] = old

    def test_ensure_watcher_starts_only_when_enabled_and_cdp_is_active(self) -> None:
        from cccc.daemon.actors import web_model_browser_recovery_watcher as watcher

        _, cleanup_home = self._with_home()
        cleanup_env = self._with_auto_reload("1")
        try:
            watcher.stop_all_web_model_browser_recovery_watchers()
            with patch.object(watcher, "_active_cdp_port", return_value=0), patch.object(watcher.threading, "Thread") as thread_cls:
                self.assertFalse(watcher.ensure_web_model_browser_recovery_watcher("g-test", "peer1"))
                thread_cls.assert_not_called()

            fake_thread = Mock()
            fake_thread.is_alive.return_value = True
            with patch.object(watcher, "_active_cdp_port", return_value=9222), patch.object(
                watcher.threading,
                "Thread",
                return_value=fake_thread,
            ) as thread_cls:
                self.assertTrue(watcher.ensure_web_model_browser_recovery_watcher("g-test", "peer1"))
                thread_cls.assert_called_once()
                fake_thread.start.assert_called_once()
        finally:
            watcher.stop_all_web_model_browser_recovery_watchers()
            cleanup_env()
            cleanup_home()

    def test_reload_window_remains_inactive_when_auto_reload_is_disabled(self) -> None:
        from cccc.daemon.actors import web_model_browser_recovery_watcher as watcher
        from cccc.ports.web_model_browser_sidecar import read_chatgpt_browser_state

        _, cleanup_home = self._with_home()
        cleanup_env = self._with_auto_reload(None)
        try:
            started = watcher.start_web_model_browser_reload_window(
                "g-test",
                "peer1",
                reason="browser_delivery",
                delivery_id="delivery-1",
                turn_id="turn-1",
                event_ids=["e1"],
                target_url="https://chatgpt.com/c/test",
            )
            self.assertFalse(started)
            state = read_chatgpt_browser_state("g-test", "peer1")
            self.assertEqual(state.get("auto_reload_active"), False)
            self.assertEqual(state.get("auto_reload_completed_reason"), "disabled")
        finally:
            cleanup_env()
            cleanup_home()

    def test_reload_window_state_helpers_start_progress_and_close_when_enabled(self) -> None:
        from cccc.daemon.actors import web_model_browser_recovery_watcher as watcher
        from cccc.ports.web_model_browser_sidecar import read_chatgpt_browser_state

        _, cleanup_home = self._with_home()
        cleanup_env = self._with_auto_reload("1")
        try:
            self.assertTrue(
                watcher.start_web_model_browser_reload_window(
                    "g-test",
                    "peer1",
                    reason="browser_delivery",
                    delivery_id="delivery-1",
                    turn_id="turn-1",
                    event_ids=["e1"],
                    target_url="https://chatgpt.com/c/test",
                )
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
            cleanup_env()
            cleanup_home()

    def test_stale_reload_window_refreshes_bound_chatgpt_page_when_enabled(self) -> None:
        from cccc.daemon.actors import web_model_browser_recovery_watcher as watcher
        from cccc.ports.web_model_browser_sidecar import read_chatgpt_browser_state, record_chatgpt_browser_state

        _, cleanup_home = self._with_home()
        cleanup_env = self._with_auto_reload("1")
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

            with patch.object(
                watcher,
                "_reload_chatgpt_projected_session",
                return_value={
                    "ok": True,
                    "action": "reload",
                    "before_url": "https://chatgpt.com/c/test",
                    "after_url": "https://chatgpt.com/c/test",
                },
            ) as reload_session:
                result = watcher._maybe_reload_stale_chatgpt_page(
                    "g-test",
                    "peer1",
                    target_url="https://chatgpt.com/c/test",
                )

            self.assertTrue(result.get("reloaded"), result)
            reload_session.assert_called_once_with("g-test", "peer1", target_url="https://chatgpt.com/c/test")
            state = read_chatgpt_browser_state("g-test", "peer1")
            self.assertEqual(state.get("auto_reload_count"), 1)
            self.assertEqual(state.get("auto_reload_last_reload_reason"), "no_progress_timeout")
            self.assertEqual(state.get("auto_reload_last_progress_reason"), "auto_reload")
        finally:
            cleanup_env()
            cleanup_home()

    def test_stale_reload_window_uses_stored_target_when_conversation_is_pending(self) -> None:
        from cccc.daemon.actors import web_model_browser_recovery_watcher as watcher
        from cccc.ports.web_model_browser_sidecar import read_chatgpt_browser_state, record_chatgpt_browser_state

        _, cleanup_home = self._with_home()
        cleanup_env = self._with_auto_reload("1")
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

            with patch.object(
                watcher,
                "_reload_chatgpt_projected_session",
                return_value={
                    "ok": True,
                    "action": "reload",
                    "before_url": "https://chatgpt.com/",
                    "after_url": "https://chatgpt.com/",
                },
            ) as reload_session:
                result = watcher._maybe_reload_stale_chatgpt_page("g-test", "peer1", target_url="")

            self.assertTrue(result.get("reloaded"), result)
            reload_session.assert_called_once_with("g-test", "peer1", target_url="https://chatgpt.com/")
            state = read_chatgpt_browser_state("g-test", "peer1")
            self.assertEqual(state.get("auto_reload_last_page_url"), "https://chatgpt.com/")
            self.assertEqual(state.get("auto_reload_last_progress_reason"), "auto_reload")
        finally:
            cleanup_env()
            cleanup_home()

    def test_stale_reload_window_skips_during_browser_delivery_submit(self) -> None:
        from datetime import datetime, timezone

        from cccc.daemon.actors import web_model_browser_recovery_watcher as watcher
        from cccc.ports.web_model_browser_sidecar import record_chatgpt_browser_state

        _, cleanup_home = self._with_home()
        cleanup_env = self._with_auto_reload("1")
        try:
            record_chatgpt_browser_state(
                "g-test",
                "peer1",
                {
                    "conversation_url": "https://chatgpt.com/c/test",
                    "last_delivery_status": "submitting",
                    "last_delivery_started_at": "2026-05-03T00:00:30Z",
                    "auto_reload_active": True,
                    "auto_reload_window_started_at": "2026-05-03T00:00:00Z",
                    "auto_reload_window_expires_at": "2026-05-03T00:30:00Z",
                    "auto_reload_last_progress_at": "2026-05-03T00:00:00Z",
                },
            )
            with (
                patch.object(watcher, "_now_dt", return_value=datetime(2026, 5, 3, 0, 1, tzinfo=timezone.utc)),
                patch.object(watcher, "_reload_chatgpt_projected_session") as reload_session,
            ):
                result = watcher._maybe_reload_stale_chatgpt_page(
                    "g-test",
                    "peer1",
                    target_url="https://chatgpt.com/c/test",
                )

            self.assertEqual(result.get("reason"), "delivery_submitting")
            reload_session.assert_not_called()
        finally:
            cleanup_env()
            cleanup_home()


if __name__ == "__main__":
    unittest.main()
