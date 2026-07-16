import unittest


class TestDeliveryThrottle(unittest.TestCase):
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
            doc = {"delivery": {"min_interval_seconds": "invalid", "auto_mark_on_delivery": "true"}}

        cfg = _get_delivery_config(_G())
        self.assertEqual(cfg.get("min_interval_seconds"), 0)
        self.assertTrue(bool(cfg.get("auto_mark_on_delivery")))

    def test_get_delivery_config_defaults_auto_mark_on_delivery_to_true(self) -> None:
        from cccc.daemon.messaging.delivery import _get_delivery_config, should_auto_mark_on_delivery

        class _G:
            doc = {}

        cfg = _get_delivery_config(_G())
        self.assertTrue(bool(cfg.get("auto_mark_on_delivery")))
        self.assertTrue(should_auto_mark_on_delivery(_G()))

    def test_get_delivery_config_preserves_explicit_auto_mark_false(self) -> None:
        from cccc.daemon.messaging.delivery import _get_delivery_config, should_auto_mark_on_delivery

        class _G:
            doc = {"delivery": {"auto_mark_on_delivery": False}}

        cfg = _get_delivery_config(_G())
        self.assertFalse(bool(cfg.get("auto_mark_on_delivery")))
        self.assertFalse(should_auto_mark_on_delivery(_G()))

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
