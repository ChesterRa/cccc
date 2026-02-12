import unittest

from cccc.contracts.v1 import DaemonRequest
from cccc.daemon.ops.socket_special_ops import try_handle_socket_special_op


class _FakeConn:
    def __init__(self) -> None:
        self.closed = False

    def close(self) -> None:
        self.closed = True


class TestSocketSpecialOps(unittest.TestCase):
    def test_unknown_op_not_handled(self) -> None:
        req = DaemonRequest.model_validate({"op": "nope", "args": {}})
        conn = _FakeConn()
        sent: list[dict] = []

        handled = try_handle_socket_special_op(
            req,
            conn,
            send_json=lambda _conn, payload: sent.append(payload),
            dump_response=lambda resp: resp.model_dump(),
            error=lambda code, msg, details=None: self._error_payload(code, msg, details),
            actor_running=lambda _gid, _aid: False,
            attach_actor_socket=lambda _gid, _aid, _sock: None,
            load_group=lambda _gid: None,
            find_actor=lambda _group, _by: None,
            supported_stream_kinds=lambda: {"chat.message"},
            start_events_stream=lambda *_args: False,
        )
        self.assertFalse(handled)
        self.assertFalse(conn.closed)
        self.assertEqual(sent, [])

    def test_term_attach_success_transfers_socket(self) -> None:
        req = DaemonRequest.model_validate({"op": "term_attach", "args": {"group_id": "g1", "actor_id": "a1"}})
        conn = _FakeConn()
        sent: list[dict] = []
        attached: list[tuple[str, str]] = []

        handled = try_handle_socket_special_op(
            req,
            conn,
            send_json=lambda _conn, payload: sent.append(payload),
            dump_response=lambda resp: resp.model_dump(),
            error=lambda code, msg, details=None: self._error_payload(code, msg, details),
            actor_running=lambda _gid, _aid: True,
            attach_actor_socket=lambda gid, aid, _sock: attached.append((gid, aid)),
            load_group=lambda _gid: None,
            find_actor=lambda _group, _by: None,
            supported_stream_kinds=lambda: {"chat.message"},
            start_events_stream=lambda *_args: False,
        )
        self.assertTrue(handled)
        self.assertFalse(conn.closed)
        self.assertEqual(attached, [("g1", "a1")])
        self.assertTrue(sent and bool(sent[0].get("ok")))

    def test_events_stream_invalid_kinds_returns_error(self) -> None:
        req = DaemonRequest.model_validate(
            {"op": "events_stream", "args": {"group_id": "g1", "kinds": ["unknown.kind"]}}
        )
        conn = _FakeConn()
        sent: list[dict] = []

        handled = try_handle_socket_special_op(
            req,
            conn,
            send_json=lambda _conn, payload: sent.append(payload),
            dump_response=lambda resp: resp.model_dump(),
            error=lambda code, msg, details=None: self._error_payload(code, msg, details),
            actor_running=lambda _gid, _aid: False,
            attach_actor_socket=lambda _gid, _aid, _sock: None,
            load_group=lambda _gid: {"group_id": "g1"},
            find_actor=lambda _group, _by: {"id": "x"},
            supported_stream_kinds=lambda: {"chat.message"},
            start_events_stream=lambda *_args: False,
        )
        self.assertTrue(handled)
        self.assertTrue(conn.closed)
        self.assertTrue(sent)
        payload = sent[0]
        self.assertFalse(bool(payload.get("ok")))
        error = payload.get("error") if isinstance(payload.get("error"), dict) else {}
        self.assertEqual(str(error.get("code") or ""), "invalid_kinds")

    def test_events_stream_success_starts_stream(self) -> None:
        req = DaemonRequest.model_validate({"op": "events_stream", "args": {"group_id": "g1"}})
        conn = _FakeConn()
        sent: list[dict] = []
        started: list[tuple[str, str]] = []

        handled = try_handle_socket_special_op(
            req,
            conn,
            send_json=lambda _conn, payload: sent.append(payload),
            dump_response=lambda resp: resp.model_dump(),
            error=lambda code, msg, details=None: self._error_payload(code, msg, details),
            actor_running=lambda _gid, _aid: False,
            attach_actor_socket=lambda _gid, _aid, _sock: None,
            load_group=lambda _gid: {"group_id": "g1"},
            find_actor=lambda _group, _by: {"id": "x"},
            supported_stream_kinds=lambda: {"chat.message"},
            start_events_stream=lambda _sock, group_id, by, _kinds, _since_event_id, _since_ts: started.append(
                (group_id, by)
            )
            or True,
        )
        self.assertTrue(handled)
        self.assertFalse(conn.closed)
        self.assertTrue(sent and bool(sent[0].get("ok")))
        self.assertEqual(started, [("g1", "user")])

    @staticmethod
    def _error_payload(code: str, message: str, details=None):
        from cccc.contracts.v1 import DaemonError, DaemonResponse

        return DaemonResponse(ok=False, error=DaemonError(code=code, message=message, details=(details or {})))


if __name__ == "__main__":
    unittest.main()
