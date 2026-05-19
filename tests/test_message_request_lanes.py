from __future__ import annotations

import logging
import threading
import time
import unittest
from types import SimpleNamespace


class _FakeConn:
    def __init__(self) -> None:
        self.closed = False
        self.payloads: list[dict] = []

    def close(self) -> None:
        self.closed = True


class TestMessageRequestLanes(unittest.TestCase):
    def test_same_group_requests_run_fifo(self) -> None:
        from cccc.daemon.messaging.message_request_lanes import MessageRequestLanes

        stop_event = threading.Event()
        first_started = threading.Event()
        release_first = threading.Event()
        order: list[str] = []

        def handle_request(req):
            order.append(f"start:{req.args['label']}")
            if req.args["label"] == "first":
                first_started.set()
                release_first.wait(timeout=2)
            order.append(f"end:{req.args['label']}")
            return SimpleNamespace(ok=True, result={"label": req.args["label"]}), False

        lanes = MessageRequestLanes(
            stop_event=stop_event,
            handle_request=handle_request,
            send_json=lambda conn, payload: conn.payloads.append(payload),
            dump_response=lambda resp: {"ok": bool(resp.ok), "result": dict(resp.result)},
            logger=logging.getLogger("test"),
            on_should_exit=stop_event.set,
            max_concurrent_groups=2,
        )

        try:
            self.assertTrue(lanes.submit(conn=_FakeConn(), req=SimpleNamespace(op="send", args={"group_id": "g1", "label": "first"})))
            self.assertTrue(first_started.wait(timeout=1))
            self.assertTrue(lanes.submit(conn=_FakeConn(), req=SimpleNamespace(op="send", args={"group_id": "g1", "label": "second"})))
            time.sleep(0.05)

            self.assertEqual(order, ["start:first"])

            release_first.set()
            self.assertTrue(lanes.wait_for_idle_for_tests(timeout=2))
            self.assertEqual(order, ["start:first", "end:first", "start:second", "end:second"])
        finally:
            stop_event.set()

    def test_different_group_requests_run_concurrently(self) -> None:
        from cccc.daemon.messaging.message_request_lanes import MessageRequestLanes

        stop_event = threading.Event()
        first_started = threading.Event()
        second_started = threading.Event()
        release_first = threading.Event()

        def handle_request(req):
            if req.args["group_id"] == "g1":
                first_started.set()
                release_first.wait(timeout=2)
            if req.args["group_id"] == "g2":
                second_started.set()
            return SimpleNamespace(ok=True, result={"group_id": req.args["group_id"]}), False

        lanes = MessageRequestLanes(
            stop_event=stop_event,
            handle_request=handle_request,
            send_json=lambda conn, payload: conn.payloads.append(payload),
            dump_response=lambda resp: {"ok": bool(resp.ok), "result": dict(resp.result)},
            logger=logging.getLogger("test"),
            on_should_exit=stop_event.set,
            max_concurrent_groups=2,
        )

        try:
            self.assertTrue(lanes.submit(conn=_FakeConn(), req=SimpleNamespace(op="send", args={"group_id": "g1"})))
            self.assertTrue(first_started.wait(timeout=1))
            self.assertTrue(lanes.submit(conn=_FakeConn(), req=SimpleNamespace(op="reply", args={"group_id": "g2"})))

            self.assertTrue(second_started.wait(timeout=1))

            release_first.set()
            self.assertTrue(lanes.wait_for_idle_for_tests(timeout=2))
        finally:
            stop_event.set()

    def test_global_concurrency_limit_defers_extra_groups(self) -> None:
        from cccc.daemon.messaging.message_request_lanes import MessageRequestLanes

        stop_event = threading.Event()
        first_started = threading.Event()
        release_first = threading.Event()
        started_groups: list[str] = []

        def handle_request(req):
            started_groups.append(req.args["group_id"])
            if req.args["group_id"] == "g1":
                first_started.set()
                release_first.wait(timeout=2)
            return SimpleNamespace(ok=True, result={"group_id": req.args["group_id"]}), False

        lanes = MessageRequestLanes(
            stop_event=stop_event,
            handle_request=handle_request,
            send_json=lambda conn, payload: conn.payloads.append(payload),
            dump_response=lambda resp: {"ok": bool(resp.ok), "result": dict(resp.result)},
            logger=logging.getLogger("test"),
            on_should_exit=stop_event.set,
            max_concurrent_groups=1,
        )

        try:
            self.assertTrue(lanes.submit(conn=_FakeConn(), req=SimpleNamespace(op="send", args={"group_id": "g1"})))
            self.assertTrue(first_started.wait(timeout=1))
            self.assertTrue(lanes.submit(conn=_FakeConn(), req=SimpleNamespace(op="send", args={"group_id": "g2"})))
            time.sleep(0.05)

            self.assertEqual(started_groups, ["g1"])

            release_first.set()
            self.assertTrue(lanes.wait_for_idle_for_tests(timeout=2))
            self.assertEqual(started_groups, ["g1", "g2"])
        finally:
            stop_event.set()


if __name__ == "__main__":
    unittest.main()
