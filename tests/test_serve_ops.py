from __future__ import annotations

import threading
import os
import tempfile
import time
import unittest
from pathlib import Path
from unittest.mock import patch


class TestServeOps(unittest.TestCase):
    def test_start_supervisor_watchdog_sets_stop_event_when_supervisor_exits(self) -> None:
        from cccc.daemon import serve_ops

        stop_event = threading.Event()
        states = [True, False]

        def _pid_alive(_pid: int) -> bool:
            if states:
                return states.pop(0)
            return False

        thread = serve_ops.start_supervisor_watchdog_thread(
            stop_event=stop_event,
            supervisor_pid=4321,
            pid_alive=_pid_alive,
            interval_seconds=0.01,
        )

        self.assertIsNotNone(thread)
        assert thread is not None
        thread.join(timeout=1.0)
        self.assertTrue(stop_event.is_set())

    def test_start_supervisor_watchdog_returns_none_without_pid(self) -> None:
        from cccc.daemon import serve_ops

        stop_event = threading.Event()

        thread = serve_ops.start_supervisor_watchdog_thread(
            stop_event=stop_event,
            supervisor_pid=0,
            pid_alive=lambda _pid: True,
            interval_seconds=0.01,
        )

        self.assertIsNone(thread)

    def test_actor_activity_ledger_append_failure_is_logged(self) -> None:
        from cccc.daemon import serve_ops
        from cccc.kernel.actors import add_actor
        from cccc.kernel.group import create_group, load_group
        from cccc.kernel.registry import load_registry

        old_home = os.environ.get("CCCC_HOME")
        with tempfile.TemporaryDirectory() as home:
            os.environ["CCCC_HOME"] = home
            try:
                reg = load_registry()
                created = create_group(reg, title="actor-activity-log", topic="")
                group = load_group(created.group_id)
                self.assertIsNotNone(group)
                add_actor(group, actor_id="peer1", title="Peer 1", runtime="codex", runner="headless")  # type: ignore[arg-type]
                group.save()  # type: ignore[union-attr]

                class _Broadcaster:
                    def publish(self, event: dict) -> None:
                        _ = event

                class _CodexSupervisor:
                    @staticmethod
                    def get_state(group_id: str, actor_id: str) -> dict:
                        return {
                            "group_id": group_id,
                            "actor_id": actor_id,
                            "status": "working",
                            "current_task_id": "turn-1",
                            "updated_at": "2026-04-11T00:00:00Z",
                        }

                    @staticmethod
                    def actor_running(_group_id: str, _actor_id: str) -> bool:
                        return True

                stop_event = threading.Event()
                with patch("cccc.kernel.ledger.append_event", side_effect=RuntimeError("disk denied")), \
                     self.assertLogs("cccc.daemon.serve_ops", level="WARNING") as logs:
                    thread = serve_ops.start_actor_activity_thread(
                        stop_event=stop_event,
                        home=Path(home),
                        pty_supervisor=object(),
                        headless_supervisor=object(),
                        codex_supervisor=_CodexSupervisor(),
                        event_broadcaster=_Broadcaster(),
                        load_group=load_group,
                        interval_seconds=1.0,
                    )
                    time.sleep(0.25)
                    stop_event.set()
                    thread.join(timeout=1.0)

                output = "\n".join(logs.output)
                self.assertIn("actor_activity ledger append failed", output)
                self.assertIn(f"group={created.group_id}", output)
                self.assertIn("actor_count=1", output)
                self.assertIn("disk denied", output)
            finally:
                if old_home is None:
                    os.environ.pop("CCCC_HOME", None)
                else:
                    os.environ["CCCC_HOME"] = old_home

    def test_actor_activity_prewarms_running_web_model_browser_session(self) -> None:
        from cccc.daemon import serve_ops
        from cccc.daemon.runner_state_ops import write_headless_state
        from cccc.kernel.actors import add_actor
        from cccc.kernel.group import create_group, load_group
        from cccc.kernel.registry import load_registry

        old_home = os.environ.get("CCCC_HOME")
        with tempfile.TemporaryDirectory() as home:
            os.environ["CCCC_HOME"] = home
            try:
                reg = load_registry()
                created = create_group(reg, title="web-model-activity", topic="")
                group = load_group(created.group_id)
                self.assertIsNotNone(group)
                assert group is not None
                add_actor(
                    group,
                    actor_id="chatgpt-web-1",
                    title="ChatGPT Web Model",
                    runtime="web_model",
                    runner="headless",
                    env={"CCCC_WEB_MODEL_DELIVERY_MODE": "browser"},
                )
                group.save()
                write_headless_state(created.group_id, "chatgpt-web-1")

                class _Broadcaster:
                    def publish(self, event: dict) -> None:
                        _ = event

                stop_event = threading.Event()
                with patch(
                    "cccc.daemon.actors.web_model_browser_session.schedule_web_model_chatgpt_browser_session_warmup",
                    return_value=True,
                ) as warmup:
                    thread = serve_ops.start_actor_activity_thread(
                        stop_event=stop_event,
                        home=Path(home),
                        pty_supervisor=object(),
                        headless_supervisor=object(),
                        codex_supervisor=object(),
                        event_broadcaster=_Broadcaster(),
                        load_group=load_group,
                        interval_seconds=1.0,
                    )
                    for _ in range(20):
                        if warmup.called:
                            break
                        time.sleep(0.05)
                    stop_event.set()
                    thread.join(timeout=1.0)

                warmup.assert_called()
                kwargs = warmup.call_args.kwargs
                self.assertEqual(kwargs.get("group_id"), created.group_id)
                self.assertEqual(kwargs.get("actor_id"), "chatgpt-web-1")
                self.assertEqual(kwargs.get("reason"), "actor_activity_running")
            finally:
                if old_home is None:
                    os.environ.pop("CCCC_HOME", None)
                else:
                    os.environ["CCCC_HOME"] = old_home

    def test_actor_activity_does_not_prewarm_pull_web_model_browser_session(self) -> None:
        from cccc.daemon import serve_ops
        from cccc.daemon.runner_state_ops import write_headless_state
        from cccc.kernel.actors import add_actor
        from cccc.kernel.group import create_group, load_group
        from cccc.kernel.registry import load_registry

        old_home = os.environ.get("CCCC_HOME")
        with tempfile.TemporaryDirectory() as home:
            os.environ["CCCC_HOME"] = home
            try:
                reg = load_registry()
                created = create_group(reg, title="web-model-pull-activity", topic="")
                group = load_group(created.group_id)
                self.assertIsNotNone(group)
                assert group is not None
                add_actor(
                    group,
                    actor_id="web-model-1",
                    title="Web Model",
                    runtime="web_model",
                    runner="headless",
                    env={"CCCC_WEB_MODEL_DELIVERY_MODE": "remote_mcp"},
                )
                group.save()
                write_headless_state(created.group_id, "web-model-1")

                class _Broadcaster:
                    def publish(self, event: dict) -> None:
                        _ = event

                stop_event = threading.Event()
                with patch(
                    "cccc.daemon.actors.web_model_browser_session.schedule_web_model_chatgpt_browser_session_warmup",
                    return_value=True,
                ) as warmup:
                    thread = serve_ops.start_actor_activity_thread(
                        stop_event=stop_event,
                        home=Path(home),
                        pty_supervisor=object(),
                        headless_supervisor=object(),
                        codex_supervisor=object(),
                        event_broadcaster=_Broadcaster(),
                        load_group=load_group,
                        interval_seconds=1.0,
                    )
                    time.sleep(0.25)
                    stop_event.set()
                    thread.join(timeout=1.0)

                warmup.assert_not_called()
            finally:
                if old_home is None:
                    os.environ.pop("CCCC_HOME", None)
                else:
                    os.environ["CCCC_HOME"] = old_home


if __name__ == "__main__":
    unittest.main()
