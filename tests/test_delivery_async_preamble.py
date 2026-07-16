import threading
import time
import unittest
from types import SimpleNamespace
from unittest.mock import patch


class TestAsyncFirstDelivery(unittest.TestCase):
    def _group(self):
        return SimpleNamespace(group_id="g-test", doc={})

    def test_first_flush_returns_without_waiting_for_preamble_submit(self) -> None:
        from cccc.daemon.messaging import delivery

        group = self._group()
        preamble_state = {"sent": False}
        prompt_started = threading.Event()
        message_sent = threading.Event()

        def fake_submit(_group, *, actor_id: str, text: str, file_fallback: bool = False, wait_for_submit: bool = False) -> bool:
            self.assertEqual(actor_id, "peer1")
            if text == "SYSTEM PROMPT":
                self.assertTrue(wait_for_submit)
                prompt_started.set()
                time.sleep(0.2)
                return True
            self.assertIn("hello first", text)
            self.assertTrue(wait_for_submit)
            message_sent.set()
            return True

        with patch.object(delivery, "THROTTLE", delivery.DeliveryThrottle()), patch(
            "cccc.daemon.messaging.delivery.find_actor", return_value={"id": "peer1", "runner": "pty"}
        ), patch("cccc.daemon.messaging.delivery.should_deliver_message", return_value=True), patch(
            "cccc.daemon.messaging.delivery.render_system_prompt", return_value="SYSTEM PROMPT"
        ), patch(
            "cccc.daemon.messaging.delivery.is_preamble_sent", side_effect=lambda _group, _aid: bool(preamble_state["sent"])
        ), patch(
            "cccc.daemon.messaging.delivery.mark_preamble_sent", side_effect=lambda _group, _aid: preamble_state.__setitem__("sent", True)
        ), patch(
            "cccc.daemon.messaging.delivery.pty_runner.SUPERVISOR.startup_times", return_value=(None, None)
        ), patch(
            "cccc.daemon.messaging.delivery.pty_runner.SUPERVISOR.actor_running", return_value=True
        ), patch(
            "cccc.daemon.messaging.delivery.pty_submit_text", side_effect=fake_submit
        ), patch.object(
            delivery, "PREAMBLE_TO_MESSAGE_DELAY_SECONDS", 0.0
        ):
            delivery.queue_chat_message(
                group,
                actor_id="peer1",
                event_id="e1",
                by="user",
                to=["@all"],
                text="hello first",
                ts="2026-03-23T00:00:00Z",
            )

            started = time.monotonic()
            result = delivery.flush_pending_messages(group, actor_id="peer1")
            elapsed = time.monotonic() - started

            self.assertTrue(result)
            self.assertLess(elapsed, 0.1)
            self.assertTrue(prompt_started.wait(0.2))
            self.assertTrue(message_sent.wait(1.0))

    def test_async_first_delivery_serializes_followup_flushes(self) -> None:
        from cccc.daemon.messaging import delivery

        group = self._group()
        preamble_state = {"sent": False}
        prompt_started = threading.Event()
        release_prompt = threading.Event()
        second_sent = threading.Event()
        sent_messages: list[str] = []

        def fake_submit(_group, *, actor_id: str, text: str, file_fallback: bool = False, wait_for_submit: bool = False) -> bool:
            self.assertEqual(actor_id, "peer1")
            if text == "SYSTEM PROMPT":
                self.assertTrue(wait_for_submit)
                prompt_started.set()
                self.assertTrue(release_prompt.wait(1.0))
                return True
            self.assertTrue(wait_for_submit)
            sent_messages.append(text)
            if "hello second" in text:
                second_sent.set()
            return True

        with patch.object(delivery, "THROTTLE", delivery.DeliveryThrottle()), patch(
            "cccc.daemon.messaging.delivery.find_actor", return_value={"id": "peer1", "runner": "pty"}
        ), patch("cccc.daemon.messaging.delivery.should_deliver_message", return_value=True), patch(
            "cccc.daemon.messaging.delivery.render_system_prompt", return_value="SYSTEM PROMPT"
        ), patch(
            "cccc.daemon.messaging.delivery.is_preamble_sent", side_effect=lambda _group, _aid: bool(preamble_state["sent"])
        ), patch(
            "cccc.daemon.messaging.delivery.mark_preamble_sent", side_effect=lambda _group, _aid: preamble_state.__setitem__("sent", True)
        ), patch(
            "cccc.daemon.messaging.delivery.pty_runner.SUPERVISOR.startup_times", return_value=(None, None)
        ), patch(
            "cccc.daemon.messaging.delivery.pty_runner.SUPERVISOR.actor_running", return_value=True
        ), patch(
            "cccc.daemon.messaging.delivery.pty_submit_text", side_effect=fake_submit
        ), patch.object(
            delivery, "PREAMBLE_TO_MESSAGE_DELAY_SECONDS", 0.0
        ):
            delivery.queue_chat_message(
                group,
                actor_id="peer1",
                event_id="e1",
                by="user",
                to=["@all"],
                text="hello first",
                ts="2026-03-23T00:00:00Z",
            )
            self.assertTrue(delivery.flush_pending_messages(group, actor_id="peer1"))
            self.assertTrue(prompt_started.wait(0.2))

            delivery.queue_chat_message(
                group,
                actor_id="peer1",
                event_id="e2",
                by="user",
                to=["@all"],
                text="hello second",
                ts="2026-03-23T00:00:01Z",
            )
            self.assertFalse(delivery.flush_pending_messages(group, actor_id="peer1"))
            self.assertFalse(second_sent.is_set())

            release_prompt.set()
            self.assertTrue(second_sent.wait(1.0))
            self.assertGreaterEqual(len(sent_messages), 2)
            self.assertIn("hello first", sent_messages[0])
            self.assertIn("hello second", sent_messages[1])

    def test_async_first_delivery_requeues_messages_when_worker_raises(self) -> None:
        from cccc.daemon.messaging import delivery

        group = self._group()
        preamble_state = {"sent": False}
        prompt_started = threading.Event()

        def fake_submit(_group, *, actor_id: str, text: str, file_fallback: bool = False, wait_for_submit: bool = False) -> bool:
            self.assertEqual(actor_id, "peer1")
            if wait_for_submit:
                prompt_started.set()
            return True

        with patch.object(delivery, "THROTTLE", delivery.DeliveryThrottle()), patch(
            "cccc.daemon.messaging.delivery.find_actor", return_value={"id": "peer1", "runner": "pty"}
        ), patch("cccc.daemon.messaging.delivery.should_deliver_message", return_value=True), patch(
            "cccc.daemon.messaging.delivery.render_system_prompt", return_value="SYSTEM PROMPT"
        ), patch(
            "cccc.daemon.messaging.delivery.is_preamble_sent", side_effect=lambda _group, _aid: bool(preamble_state["sent"])
        ), patch(
            "cccc.daemon.messaging.delivery.mark_preamble_sent", side_effect=RuntimeError("boom during mark_preamble_sent")
        ), patch(
            "cccc.daemon.messaging.delivery.pty_runner.SUPERVISOR.startup_times", return_value=(None, None)
        ), patch(
            "cccc.daemon.messaging.delivery.pty_runner.SUPERVISOR.actor_running", return_value=True
        ), patch(
            "cccc.daemon.messaging.delivery.pty_submit_text", side_effect=fake_submit
        ), patch.object(
            delivery, "PREAMBLE_TO_MESSAGE_DELAY_SECONDS", 0.0
        ):
            delivery.queue_chat_message(
                group,
                actor_id="peer1",
                event_id="e1",
                by="user",
                to=["@all"],
                text="hello first",
                ts="2026-03-23T00:00:00Z",
            )
            self.assertTrue(delivery.flush_pending_messages(group, actor_id="peer1"))
            self.assertTrue(prompt_started.wait(0.2))

            deadline = time.monotonic() + 1.0
            while time.monotonic() < deadline:
                if delivery.THROTTLE.has_pending("g-test", "peer1"):
                    break
                time.sleep(0.01)

            self.assertTrue(delivery.THROTTLE.has_pending("g-test", "peer1"))
            reacquired = False
            deadline = time.monotonic() + 1.0
            while time.monotonic() < deadline:
                if delivery.THROTTLE.try_begin_delivery("g-test", "peer1"):
                    reacquired = True
                    break
                time.sleep(0.01)

            self.assertTrue(reacquired)
            delivery.THROTTLE.end_delivery("g-test", "peer1")

    def test_first_flush_waits_for_bracketed_paste_before_preamble(self) -> None:
        from cccc.daemon.messaging import delivery

        group = self._group()
        submit_mock = unittest.mock.Mock(return_value=True)
        now = time.monotonic()

        with patch.object(delivery, "THROTTLE", delivery.DeliveryThrottle()), patch(
            "cccc.daemon.messaging.delivery.find_actor", return_value={"id": "peer1", "runner": "pty"}
        ), patch("cccc.daemon.messaging.delivery.should_deliver_message", return_value=True), patch(
            "cccc.daemon.messaging.delivery.render_system_prompt", return_value="SYSTEM PROMPT"
        ), patch(
            "cccc.daemon.messaging.delivery.is_preamble_sent", return_value=False
        ), patch(
            "cccc.daemon.messaging.delivery.pty_runner.SUPERVISOR.startup_times",
            return_value=(now - 1.0, now - 2.0),
        ), patch(
            "cccc.daemon.messaging.delivery.pty_runner.SUPERVISOR.bracketed_paste_status",
            return_value=(False, None),
        ), patch(
            "cccc.daemon.messaging.delivery.pty_submit_text", submit_mock
        ):
            delivery.queue_chat_message(
                group,
                actor_id="peer1",
                event_id="e1",
                by="user",
                to=["@all"],
                text="hello first",
                ts="2026-03-23T00:00:00Z",
            )

            self.assertFalse(delivery.flush_pending_messages(group, actor_id="peer1"))
            self.assertTrue(delivery.THROTTLE.has_pending("g-test", "peer1"))
            submit_mock.assert_not_called()

    def test_request_flush_retries_until_delivery_window_opens(self) -> None:
        from cccc.daemon.messaging import delivery

        group = self._group()
        delivered = threading.Event()
        flush_calls = {"count": 0}

        with patch.object(delivery, "THROTTLE", delivery.DeliveryThrottle()), patch(
            "cccc.daemon.messaging.delivery.pty_runner.SUPERVISOR.actor_running", return_value=True
        ), patch(
            "cccc.daemon.messaging.delivery.get_group_state", return_value="active"
        ), patch.object(
            delivery, "ASYNC_FLUSH_POLL_SECONDS", 0.01
        ), patch.object(
            delivery, "ASYNC_FLUSH_MAX_WAIT_SECONDS", 0.2
        ):
            delivery.queue_chat_message(
                group,
                actor_id="peer1",
                event_id="e1",
                by="user",
                to=["@all"],
                text="hello first",
                ts="2026-03-23T00:00:00Z",
            )

            original_next_retry_delay = delivery.THROTTLE.next_retry_delay

            def fake_flush(_group, *, actor_id: str) -> bool:
                self.assertEqual(actor_id, "peer1")
                flush_calls["count"] += 1
                if flush_calls["count"] == 1:
                    pending = delivery.THROTTLE.take_pending("g-test", "peer1")
                    delivery.THROTTLE.requeue_front("g-test", "peer1", pending)
                    return False
                pending = delivery.THROTTLE.take_pending("g-test", "peer1")
                self.assertEqual(len(pending), 1)
                delivery.THROTTLE.mark_delivered("g-test", "peer1")
                delivered.set()
                return True

            with patch(
                "cccc.daemon.messaging.delivery.flush_pending_messages", side_effect=fake_flush
            ), patch.object(
                delivery.THROTTLE, "next_retry_delay", side_effect=lambda gid, aid, interval: (
                    0.0 if flush_calls["count"] == 0 else original_next_retry_delay(gid, aid, interval)
                ),
            ):
                self.assertTrue(delivery.request_flush_pending_messages(group, actor_id="peer1"))
                self.assertTrue(delivered.wait(1.0))

            self.assertGreaterEqual(flush_calls["count"], 2)

    def test_request_flush_stops_when_actor_is_not_running(self) -> None:
        from cccc.daemon.messaging import delivery

        group = self._group()
        flush_called = threading.Event()

        with patch.object(delivery, "THROTTLE", delivery.DeliveryThrottle()), patch(
            "cccc.daemon.messaging.delivery.pty_runner.SUPERVISOR.actor_running", return_value=False
        ), patch(
            "cccc.daemon.messaging.delivery.get_group_state", return_value="active"
        ), patch.object(
            delivery, "ASYNC_FLUSH_POLL_SECONDS", 0.01
        ), patch.object(
            delivery, "ASYNC_FLUSH_MAX_WAIT_SECONDS", 0.2
        ):
            delivery.queue_chat_message(
                group,
                actor_id="peer1",
                event_id="e1",
                by="user",
                to=["@all"],
                text="hello first",
                ts="2026-03-23T00:00:00Z",
            )

            def fake_flush(_group, *, actor_id: str) -> bool:
                self.assertEqual(actor_id, "peer1")
                flush_called.set()
                return False

            with patch("cccc.daemon.messaging.delivery.flush_pending_messages", side_effect=fake_flush):
                self.assertTrue(delivery.request_flush_pending_messages(group, actor_id="peer1"))
                self.assertTrue(flush_called.wait(1.0))
                deadline = time.monotonic() + 1.0
                while time.monotonic() < deadline:
                    with delivery._ASYNC_FLUSH_LOCK:
                        if ("g-test", "peer1") not in delivery._ASYNC_FLUSH_IN_FLIGHT:
                            break
                    time.sleep(0.01)

            with delivery._ASYNC_FLUSH_LOCK:
                self.assertNotIn(("g-test", "peer1"), delivery._ASYNC_FLUSH_IN_FLIGHT)
            self.assertTrue(delivery.THROTTLE.has_pending("g-test", "peer1"))

    def test_request_flush_stops_when_group_is_paused(self) -> None:
        from cccc.daemon.messaging import delivery

        group = self._group()
        flush_called = threading.Event()

        with patch.object(delivery, "THROTTLE", delivery.DeliveryThrottle()), patch(
            "cccc.daemon.messaging.delivery.pty_runner.SUPERVISOR.actor_running", return_value=True
        ), patch(
            "cccc.daemon.messaging.delivery.get_group_state", return_value="paused"
        ), patch.object(
            delivery, "ASYNC_FLUSH_POLL_SECONDS", 0.01
        ), patch.object(
            delivery, "ASYNC_FLUSH_MAX_WAIT_SECONDS", 0.2
        ):
            delivery.queue_chat_message(
                group,
                actor_id="peer1",
                event_id="e1",
                by="user",
                to=["@all"],
                text="hello first",
                ts="2026-03-23T00:00:00Z",
            )

            def fake_flush(_group, *, actor_id: str) -> bool:
                self.assertEqual(actor_id, "peer1")
                flush_called.set()
                return False

            with patch("cccc.daemon.messaging.delivery.flush_pending_messages", side_effect=fake_flush):
                self.assertTrue(delivery.request_flush_pending_messages(group, actor_id="peer1"))
                self.assertTrue(flush_called.wait(1.0))
                deadline = time.monotonic() + 1.0
                while time.monotonic() < deadline:
                    with delivery._ASYNC_FLUSH_LOCK:
                        if ("g-test", "peer1") not in delivery._ASYNC_FLUSH_IN_FLIGHT:
                            break
                    time.sleep(0.01)

            with delivery._ASYNC_FLUSH_LOCK:
                self.assertNotIn(("g-test", "peer1"), delivery._ASYNC_FLUSH_IN_FLIGHT)
            self.assertTrue(delivery.THROTTLE.has_pending("g-test", "peer1"))


if __name__ == "__main__":
    unittest.main()
