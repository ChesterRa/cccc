import threading
import time
from types import SimpleNamespace
from unittest.mock import patch

from cccc.daemon.messaging import delivery


def _group():
    return SimpleNamespace(group_id="g-test", doc={})


def _wait_for_worker_exit() -> None:
    deadline = time.monotonic() + 1.0
    while time.monotonic() < deadline:
        with delivery._ASYNC_FLUSH_LOCK:
            if ("g-test", "deepseek-1") not in delivery._ASYNC_FLUSH_IN_FLIGHT:
                return
        time.sleep(0.01)


def test_request_flush_retries_running_deepseek_without_pty_process() -> None:
    group = _group()
    delivered = threading.Event()
    flush_calls = 0

    def fake_flush(_group, *, actor_id: str) -> bool:
        nonlocal flush_calls
        assert actor_id == "deepseek-1"
        flush_calls += 1
        pending = delivery.THROTTLE.take_pending("g-test", "deepseek-1")
        if flush_calls == 1:
            delivery.THROTTLE.requeue_front("g-test", "deepseek-1", pending)
            return False
        delivery.THROTTLE.mark_delivered("g-test", "deepseek-1")
        delivered.set()
        return True

    with (
        patch.object(delivery, "THROTTLE", delivery.DeliveryThrottle()),
        patch(
            "cccc.daemon.messaging.delivery.find_actor",
            return_value={
                "id": "deepseek-1",
                "runtime": "deepseek",
                "runner": "headless",
            },
        ),
        patch(
            "cccc.daemon.actors.deepseek_runtime.running", return_value=True
        ) as deepseek_running,
        patch(
            "cccc.daemon.messaging.delivery.pty_runner.SUPERVISOR.actor_running",
            return_value=False,
        ) as pty_running,
        patch("cccc.daemon.messaging.delivery.get_group_state", return_value="active"),
        patch.object(delivery, "ASYNC_FLUSH_POLL_SECONDS", 0.01),
        patch.object(delivery, "ASYNC_FLUSH_MAX_WAIT_SECONDS", 0.2),
        patch(
            "cccc.daemon.messaging.delivery.flush_pending_messages",
            side_effect=fake_flush,
        ),
    ):
        delivery.queue_chat_message(
            group,
            actor_id="deepseek-1",
            event_id="e1",
            by="user",
            to=["deepseek-1"],
            text="hello deepseek",
            ts="2026-03-23T00:00:00Z",
        )
        assert delivery.request_flush_pending_messages(group, actor_id="deepseek-1")
        assert delivered.wait(1.0)
        _wait_for_worker_exit()

    assert flush_calls >= 2
    deepseek_running.assert_called_with(group_id="g-test", actor_id="deepseek-1")
    pty_running.assert_not_called()


def test_request_flush_drains_message_queued_during_first_delivery() -> None:
    group = _group()
    first_started = threading.Event()
    release_first = threading.Event()
    second_delivered = threading.Event()
    flush_calls = 0

    def fake_flush(_group, *, actor_id: str) -> bool:
        nonlocal flush_calls
        assert actor_id == "deepseek-1"
        flush_calls += 1
        pending = delivery.THROTTLE.take_pending("g-test", "deepseek-1")
        assert len(pending) == 1
        delivery.THROTTLE.mark_delivered("g-test", "deepseek-1")
        if flush_calls == 1:
            first_started.set()
            assert release_first.wait(1.0)
        else:
            assert pending[0].event_id == "e2"
            second_delivered.set()
        return True

    with (
        patch.object(delivery, "THROTTLE", delivery.DeliveryThrottle()),
        patch(
            "cccc.daemon.messaging.delivery.find_actor",
            return_value={
                "id": "deepseek-1",
                "runtime": "deepseek",
                "runner": "headless",
            },
        ),
        patch("cccc.daemon.actors.deepseek_runtime.running", return_value=True),
        patch("cccc.daemon.messaging.delivery.get_group_state", return_value="active"),
        patch.object(delivery, "ASYNC_FLUSH_POLL_SECONDS", 0.01),
        patch.object(delivery, "ASYNC_FLUSH_MAX_WAIT_SECONDS", 0.2),
        patch(
            "cccc.daemon.messaging.delivery.flush_pending_messages",
            side_effect=fake_flush,
        ),
    ):
        delivery.queue_chat_message(
            group,
            actor_id="deepseek-1",
            event_id="e1",
            by="user",
            to=["deepseek-1"],
            text="first",
            ts="2026-03-23T00:00:00Z",
        )
        assert delivery.request_flush_pending_messages(group, actor_id="deepseek-1")
        assert first_started.wait(1.0)
        delivery.queue_chat_message(
            group,
            actor_id="deepseek-1",
            event_id="e2",
            by="user",
            to=["deepseek-1"],
            text="second",
            ts="2026-03-23T00:00:01Z",
        )
        assert not delivery.request_flush_pending_messages(group, actor_id="deepseek-1")
        release_first.set()
        assert second_delivered.wait(1.0)
        _wait_for_worker_exit()

    assert flush_calls == 2
    assert not delivery.THROTTLE.has_pending("g-test", "deepseek-1")
