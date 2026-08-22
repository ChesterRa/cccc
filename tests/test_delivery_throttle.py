import unittest


class TestDeliveryThrottle(unittest.TestCase):
    def test_recovered_event_is_not_queued_again_after_worker_takes_it(self) -> None:
        from cccc.daemon.messaging.delivery import DeliveryThrottle, PendingMessage

        for kind in ("chat.message", "system.notify"):
            with self.subTest(kind=kind):
                throttle = DeliveryThrottle()
                recovered = PendingMessage(
                    event_id="e1", by="user", to=["a1"], text="recovered", kind=kind
                )
                self.assertEqual(throttle.recover_front("g1", "a1", [recovered]), 1)
                self.assertEqual(
                    [item.event_id for item in throttle.take_pending("g1", "a1")], ["e1"]
                )
                self.assertFalse(
                    throttle.queue_message(
                        "g1",
                        "a1",
                        event_id="e1",
                        by="user",
                        to=["a1"],
                        text="post-commit duplicate",
                        kind=kind,
                        deduplicate_by_event_id=True,
                    )
                )
                self.assertFalse(throttle.has_pending("g1", "a1"))

    def test_system_notify_queue_is_idempotent_by_event_id(self) -> None:
        from types import SimpleNamespace
        from unittest.mock import patch

        from cccc.daemon.messaging import delivery

        throttle = delivery.DeliveryThrottle()
        group = SimpleNamespace(group_id="g1")
        with patch.object(delivery, "THROTTLE", throttle):
            for _ in range(2):
                delivery.queue_system_notify(
                    group,
                    actor_id="a1",
                    event_id="notify-1",
                    notify_kind="info",
                    title="Notice",
                    message="Run once",
                )

        pending = throttle.take_pending("g1", "a1")
        self.assertEqual(
            [(item.event_id, item.kind) for item in pending],
            [("notify-1", "system.notify")],
        )

    def test_reset_actor_keeps_pending_messages(self) -> None:
        from cccc.daemon.messaging.delivery import DeliveryThrottle

        t = DeliveryThrottle()
        t.queue_message(
            "g1",
            "a1",
            event_id="e1",
            by="user",
            to=["@all"],
            text="hello",
            kind="chat.message",
        )
        self.assertTrue(t.has_pending("g1", "a1"))

        t.reset_actor("g1", "a1", keep_pending=True)
        self.assertTrue(t.has_pending("g1", "a1"))

        pending = t.take_pending("g1", "a1")
        self.assertEqual(len(pending), 1)
        self.assertEqual(pending[0].event_id, "e1")
        self.assertEqual(pending[0].text, "hello")

    def test_clear_actor_drops_pending_messages(self) -> None:
        from cccc.daemon.messaging.delivery import DeliveryThrottle

        t = DeliveryThrottle()
        t.queue_message(
            "g1",
            "a1",
            event_id="e1",
            by="user",
            to=["@all"],
            text="hello",
            kind="chat.message",
        )
        self.assertTrue(t.has_pending("g1", "a1"))

        t.clear_actor("g1", "a1")
        self.assertFalse(t.has_pending("g1", "a1"))

    def test_get_delivery_config_falls_back_on_invalid_min_interval(self) -> None:
        from cccc.daemon.messaging.delivery import _get_delivery_config

        class _G:
            doc = {"delivery": {"min_interval_seconds": "invalid"}}

        cfg = _get_delivery_config(_G())
        self.assertEqual(cfg.get("min_interval_seconds"), 0)

    def test_get_delivery_config_clamps_negative_min_interval(self) -> None:
        from cccc.daemon.messaging.delivery import _get_delivery_config

        class _G:
            doc = {"delivery": {"min_interval_seconds": -5}}

        cfg = _get_delivery_config(_G())
        self.assertEqual(cfg.get("min_interval_seconds"), 0)

    def test_next_retry_delay_reflects_pending_retry_backoff(self) -> None:
        from cccc.daemon.messaging.delivery import DeliveryThrottle

        t = DeliveryThrottle()
        t.queue_message(
            "g1",
            "a1",
            event_id="e1",
            by="user",
            to=["@all"],
            text="hello",
            kind="chat.message",
        )
        pending = t.take_pending("g1", "a1")
        t.requeue_front("g1", "a1", pending)

        delay = t.next_retry_delay("g1", "a1", 0)

        self.assertGreater(delay, 4.0)
        self.assertLessEqual(delay, 5.0)

    def test_debug_summary_includes_delivery_inflight(self) -> None:
        from cccc.daemon.messaging.delivery import DeliveryThrottle

        t = DeliveryThrottle()
        t.queue_message(
            "g1",
            "a1",
            event_id="e1",
            by="user",
            to=["@all"],
            text="hello",
            kind="chat.message",
        )

        self.assertTrue(t.try_begin_delivery("g1", "a1"))
        summary = t.debug_summary("g1")
        actor = summary.get("actors", {}).get("a1", {})
        self.assertEqual(actor.get("delivery_inflight"), True)

        t.end_delivery("g1", "a1")


if __name__ == "__main__":
    unittest.main()
