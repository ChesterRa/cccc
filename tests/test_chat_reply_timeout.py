import os
import shutil
import tempfile
import threading
import time
import unittest
from unittest import mock


class TestChatReplyTimeout(unittest.TestCase):
    def _with_home(self):
        old_home = os.environ.get("CCCC_HOME")
        old_post_commit_mode = os.environ.get("CCCC_CHAT_POST_COMMIT_MODE")
        td_ctx = tempfile.TemporaryDirectory()
        td = td_ctx.__enter__()
        os.environ["CCCC_HOME"] = td
        os.environ["CCCC_CHAT_POST_COMMIT_MODE"] = "async"

        def cleanup() -> None:
            if old_home is None:
                os.environ.pop("CCCC_HOME", None)
            else:
                os.environ["CCCC_HOME"] = old_home
            if old_post_commit_mode is None:
                os.environ.pop("CCCC_CHAT_POST_COMMIT_MODE", None)
            else:
                os.environ["CCCC_CHAT_POST_COMMIT_MODE"] = old_post_commit_mode
            for attempt in range(5):
                try:
                    shutil.rmtree(td)
                    break
                except FileNotFoundError:
                    break
                except OSError:
                    if attempt >= 4:
                        raise
                    time.sleep(0.05)

        return td, cleanup

    def _call(self, op: str, args: dict):
        from cccc.contracts.v1 import DaemonRequest
        from cccc.daemon.server import handle_request

        return handle_request(DaemonRequest.model_validate({"op": op, "args": args}))

    def test_reply_returns_before_slow_post_commit_automation_finishes(self) -> None:
        from cccc.daemon.messaging.chat_ops import handle_reply
        from cccc.util.conv import coerce_bool

        _, cleanup = self._with_home()
        try:
            created, _ = self._call("group_create", {"title": "reply-timeout", "topic": "", "by": "user"})
            self.assertTrue(created.ok, getattr(created, "error", None))
            group_id = str((created.result or {}).get("group_id") or "").strip()
            original, _ = self._call(
                "send",
                {"group_id": group_id, "by": "user", "to": ["user"], "text": "original"},
            )
            self.assertTrue(original.ok, getattr(original, "error", None))
            original_event = (original.result or {}).get("event") if isinstance(original.result, dict) else {}
            reply_to = str(original_event.get("id") or "").strip()
            self.assertTrue(reply_to)

            automation_started = threading.Event()
            automation_can_finish = threading.Event()

            def slow_automation(_group) -> None:
                automation_started.set()
                automation_can_finish.wait(timeout=1)

            started_at = time.monotonic()
            resp = handle_reply(
                {
                    "group_id": group_id,
                    "by": "user",
                    "reply_to": reply_to,
                    "to": ["user"],
                    "text": "reply should return before automation finishes",
                },
                coerce_bool=coerce_bool,
                normalize_attachments=lambda _group, _raw: [],
                effective_runner_kind=lambda runner: str(runner or "headless"),
                auto_wake_recipients=lambda _group, _to, _by: [],
                automation_on_resume=lambda _group: None,
                automation_on_new_message=slow_automation,
                clear_pending_system_notifies=lambda _group_id, _kinds: None,
            )
            elapsed = time.monotonic() - started_at
            automation_can_finish.set()

            self.assertTrue(resp.ok, getattr(resp, "error", None))
            self.assertLess(elapsed, 0.2)
            self.assertTrue(automation_started.wait(timeout=1))
        finally:
            cleanup()

    def test_send_returns_before_slow_post_commit_automation_finishes(self) -> None:
        from cccc.daemon.messaging.chat_ops import handle_send
        from cccc.util.conv import coerce_bool

        _, cleanup = self._with_home()
        try:
            created, _ = self._call("group_create", {"title": "send-timeout", "topic": "", "by": "user"})
            self.assertTrue(created.ok, getattr(created, "error", None))
            group_id = str((created.result or {}).get("group_id") or "").strip()

            automation_started = threading.Event()
            automation_can_finish = threading.Event()

            def slow_automation(_group) -> None:
                automation_started.set()
                automation_can_finish.wait(timeout=1)

            started_at = time.monotonic()
            resp = handle_send(
                {
                    "group_id": group_id,
                    "by": "user",
                    "to": ["user"],
                    "text": "send should return before automation finishes",
                },
                coerce_bool=coerce_bool,
                normalize_attachments=lambda _group, _raw: [],
                effective_runner_kind=lambda runner: str(runner or "headless"),
                auto_wake_recipients=lambda _group, _to, _by: [],
                automation_on_resume=lambda _group: None,
                automation_on_new_message=slow_automation,
                clear_pending_system_notifies=lambda _group_id, _kinds: None,
            )
            elapsed = time.monotonic() - started_at
            automation_can_finish.set()

            self.assertTrue(resp.ok, getattr(resp, "error", None))
            self.assertLess(elapsed, 0.2)
            self.assertTrue(automation_started.wait(timeout=1))
        finally:
            cleanup()

    def test_send_returns_before_slow_headless_submit_finishes(self) -> None:
        from cccc.daemon.messaging.chat_ops import handle_send
        from cccc.util.conv import coerce_bool

        _, cleanup = self._with_home()
        try:
            created, _ = self._call("group_create", {"title": "headless-submit-timeout", "topic": "", "by": "user"})
            self.assertTrue(created.ok, getattr(created, "error", None))
            group_id = str((created.result or {}).get("group_id") or "").strip()
            add, _ = self._call(
                "actor_add",
                {
                    "group_id": group_id,
                    "actor_id": "peer1",
                    "runtime": "codex",
                    "runner": "headless",
                    "by": "user",
                },
            )
            self.assertTrue(add.ok, getattr(add, "error", None))

            submit_started = threading.Event()
            submit_can_finish = threading.Event()

            def slow_submit(**_kwargs):
                submit_started.set()
                submit_can_finish.wait(timeout=1)
                return True

            started_at = time.monotonic()
            with (
                mock.patch("cccc.daemon.messaging.chat_ops.codex_app_supervisor.actor_running", return_value=True),
                mock.patch("cccc.daemon.messaging.chat_ops.codex_app_supervisor.submit_user_message", side_effect=slow_submit),
            ):
                resp = handle_send(
                    {
                        "group_id": group_id,
                        "by": "user",
                        "to": ["peer1"],
                        "text": "send should return before headless submit finishes",
                    },
                    coerce_bool=coerce_bool,
                    normalize_attachments=lambda _group, _raw: [],
                    effective_runner_kind=lambda runner: str(runner or "headless"),
                    auto_wake_recipients=lambda _group, _to, _by: [],
                    automation_on_resume=lambda _group: None,
                    automation_on_new_message=lambda _group: None,
                    clear_pending_system_notifies=lambda _group_id, _kinds: None,
                )
                elapsed = time.monotonic() - started_at
                submit_can_finish.set()

                self.assertTrue(resp.ok, getattr(resp, "error", None))
                self.assertLess(elapsed, 0.2)
                self.assertTrue(submit_started.wait(timeout=1))
        finally:
            cleanup()

    def test_reply_returns_before_slow_headless_submit_finishes(self) -> None:
        from cccc.daemon.messaging.chat_ops import handle_reply
        from cccc.util.conv import coerce_bool

        _, cleanup = self._with_home()
        try:
            created, _ = self._call("group_create", {"title": "reply-headless-submit-timeout", "topic": "", "by": "user"})
            self.assertTrue(created.ok, getattr(created, "error", None))
            group_id = str((created.result or {}).get("group_id") or "").strip()
            add, _ = self._call(
                "actor_add",
                {
                    "group_id": group_id,
                    "actor_id": "peer1",
                    "runtime": "codex",
                    "runner": "headless",
                    "by": "user",
                },
            )
            self.assertTrue(add.ok, getattr(add, "error", None))
            original, _ = self._call(
                "send",
                {"group_id": group_id, "by": "user", "to": ["user"], "text": "original"},
            )
            self.assertTrue(original.ok, getattr(original, "error", None))
            original_event = (original.result or {}).get("event") if isinstance(original.result, dict) else {}
            reply_to = str(original_event.get("id") or "").strip()
            self.assertTrue(reply_to)

            submit_started = threading.Event()
            submit_can_finish = threading.Event()

            def slow_submit(**_kwargs):
                submit_started.set()
                submit_can_finish.wait(timeout=1)
                return True

            started_at = time.monotonic()
            with (
                mock.patch("cccc.daemon.messaging.chat_ops.codex_app_supervisor.actor_running", return_value=True),
                mock.patch("cccc.daemon.messaging.chat_ops.codex_app_supervisor.submit_user_message", side_effect=slow_submit),
            ):
                resp = handle_reply(
                    {
                        "group_id": group_id,
                        "by": "user",
                        "reply_to": reply_to,
                        "to": ["peer1"],
                        "text": "reply should return before headless submit finishes",
                    },
                    coerce_bool=coerce_bool,
                    normalize_attachments=lambda _group, _raw: [],
                    effective_runner_kind=lambda runner: str(runner or "headless"),
                    auto_wake_recipients=lambda _group, _to, _by: [],
                    automation_on_resume=lambda _group: None,
                    automation_on_new_message=lambda _group: None,
                    clear_pending_system_notifies=lambda _group_id, _kinds: None,
                )
                elapsed = time.monotonic() - started_at
                submit_can_finish.set()

                self.assertTrue(resp.ok, getattr(resp, "error", None))
                self.assertLess(elapsed, 0.2)
                self.assertTrue(submit_started.wait(timeout=1))
        finally:
            cleanup()

    def test_post_commit_thread_start_failure_does_not_raise(self) -> None:
        from cccc.daemon.messaging.post_commit import run_chat_post_commit

        with mock.patch("threading.Thread.start", side_effect=RuntimeError("thread unavailable")):
            run_chat_post_commit("thread-start-failure", lambda: None)


if __name__ == "__main__":
    unittest.main()
