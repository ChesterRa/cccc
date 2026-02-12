import unittest


class TestDeliveryThrottle(unittest.TestCase):
    def test_reset_actor_keeps_pending_messages(self) -> None:
        from cccc.daemon.delivery import DeliveryThrottle

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
        from cccc.daemon.delivery import DeliveryThrottle

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
        from cccc.daemon.delivery import _get_delivery_config

        class _G:
            doc = {"delivery": {"min_interval_seconds": "invalid", "auto_mark_on_delivery": "true"}}

        cfg = _get_delivery_config(_G())
        self.assertEqual(cfg.get("min_interval_seconds"), 0)
        self.assertTrue(bool(cfg.get("auto_mark_on_delivery")))

    def test_get_delivery_config_clamps_negative_min_interval(self) -> None:
        from cccc.daemon.delivery import _get_delivery_config

        class _G:
            doc = {"delivery": {"min_interval_seconds": -5}}

        cfg = _get_delivery_config(_G())
        self.assertEqual(cfg.get("min_interval_seconds"), 0)


if __name__ == "__main__":
    unittest.main()
