from __future__ import annotations

import unittest
from types import SimpleNamespace
from unittest.mock import patch


class TestServerRequestQueueRouting(unittest.TestCase):
    def test_message_ops_use_fast_queue_when_group_not_idle(self) -> None:
        from cccc.daemon.server import _request_queue_for

        read_queue = object()
        fast_queue = object()
        slow_queue = object()
        req = SimpleNamespace(op="reply", args={"group_id": "g1"})

        with patch("cccc.daemon.server.load_group", return_value=object()), patch(
            "cccc.daemon.server.get_group_state", return_value="active"
        ):
            selected = _request_queue_for(req, read_queue=read_queue, fast_queue=fast_queue, slow_queue=slow_queue)

        self.assertIs(selected, fast_queue)
        self.assertEqual(req.args.get("__group_state_at_accept"), "active")

    def test_message_ops_use_fast_queue_even_when_group_idle(self) -> None:
        from cccc.daemon.server import _request_queue_for

        read_queue = object()
        fast_queue = object()
        slow_queue = object()
        req = SimpleNamespace(op="send", args={"group_id": "g1"})

        with patch("cccc.daemon.server.load_group", return_value=object()), patch(
            "cccc.daemon.server.get_group_state", return_value="idle"
        ):
            selected = _request_queue_for(req, read_queue=read_queue, fast_queue=fast_queue, slow_queue=slow_queue)

        self.assertIs(selected, fast_queue)
        self.assertEqual(req.args.get("__group_state_at_accept"), "idle")

    def test_read_ops_use_read_queue(self) -> None:
        from cccc.daemon.server import _request_queue_for

        read_queue = object()
        fast_queue = object()
        slow_queue = object()
        req = SimpleNamespace(op="context_get", args={"group_id": "g1"})

        selected = _request_queue_for(req, read_queue=read_queue, fast_queue=fast_queue, slow_queue=slow_queue)

        self.assertIs(selected, read_queue)

    def test_terminal_transcript_ops_stay_on_slow_queue(self) -> None:
        from cccc.daemon.server import _request_queue_for

        read_queue = object()
        fast_queue = object()
        slow_queue = object()

        for op in ("terminal_history", "terminal_tail"):
            with self.subTest(op=op):
                req = SimpleNamespace(op=op, args={"group_id": "g1", "actor_id": "peer1"})

                selected = _request_queue_for(req, read_queue=read_queue, fast_queue=fast_queue, slow_queue=slow_queue)

                self.assertIs(selected, slow_queue)

    def test_slash_capability_state_uses_read_queue(self) -> None:
        from cccc.daemon.server import _request_queue_for

        read_queue = object()
        fast_queue = object()
        slow_queue = object()
        req = SimpleNamespace(
            op="capability_state",
            args={"group_id": "g1", "actor_id": "user", "view": "slash_commands"},
        )

        selected = _request_queue_for(req, read_queue=read_queue, fast_queue=fast_queue, slow_queue=slow_queue)

        self.assertIs(selected, read_queue)

    def test_voice_workspace_state_stays_on_slow_queue_because_get_can_emit_retry_notify(self) -> None:
        from cccc.daemon.server import _request_queue_for

        read_queue = object()
        fast_queue = object()
        slow_queue = object()
        req = SimpleNamespace(
            op="assistant_state",
            args={"group_id": "g1", "assistant_id": "voice_secretary", "view": "voice_workspace"},
        )

        selected = _request_queue_for(req, read_queue=read_queue, fast_queue=fast_queue, slow_queue=slow_queue)

        self.assertIs(selected, slow_queue)
