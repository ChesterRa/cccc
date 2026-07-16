import logging
import unittest

from cccc.contracts.v1 import DaemonError, DaemonResponse
from cccc.daemon.ops.socket_accept_ops import handle_incoming_connection


class _FakeConn:
    def __init__(self) -> None:
        self.closed = False
        self.timeout = None

    def settimeout(self, value) -> None:
        self.timeout = value

    def close(self) -> None:
        self.closed = True


class TestSocketAcceptOps(unittest.TestCase):
    def test_invalid_request_path(self) -> None:
        conn = _FakeConn()
        sent: list[dict] = []
        should_exit = handle_incoming_connection(
            conn,
            recv_json_line=lambda _conn: {"broken": True},
            parse_request=lambda _raw: (_ for _ in ()).throw(ValueError("bad")),
            make_invalid_request_error=lambda err: DaemonResponse(
                ok=False,
                error=DaemonError(code="invalid_request", message="invalid request", details={"error": err}),
            ),
            send_json=lambda _conn, payload: sent.append(payload),
            dump_response=lambda resp: resp.model_dump(),
            try_handle_special=lambda _req, _conn: False,
            handle_request=lambda _req: (DaemonResponse(ok=True, result={}), False),
            logger=logging.getLogger("test"),
        )
        self.assertFalse(should_exit)
        self.assertTrue(conn.closed)
        self.assertTrue(sent)
        self.assertFalse(bool(sent[0].get("ok")))
        self.assertEqual(conn.timeout, 0.5)

    def test_invalid_request_send_broken_pipe_is_ignored(self) -> None:
        conn = _FakeConn()
        should_exit = handle_incoming_connection(
            conn,
            recv_json_line=lambda _conn: {"broken": True},
            parse_request=lambda _raw: (_ for _ in ()).throw(ValueError("bad")),
            make_invalid_request_error=lambda err: DaemonResponse(
                ok=False,
                error=DaemonError(code="invalid_request", message="invalid request", details={"error": err}),
            ),
            send_json=lambda _conn, _payload: (_ for _ in ()).throw(BrokenPipeError("gone")),
            dump_response=lambda resp: resp.model_dump(),
            try_handle_special=lambda _req, _conn: False,
            handle_request=lambda _req: (DaemonResponse(ok=True, result={}), False),
            logger=logging.getLogger("test"),
        )
        self.assertFalse(should_exit)
        self.assertTrue(conn.closed)

    def test_read_timeout_closes_connection(self) -> None:
        conn = _FakeConn()
        sent: list[dict] = []
        should_exit = handle_incoming_connection(
            conn,
            recv_json_line=lambda _conn: (_ for _ in ()).throw(TimeoutError("slow client")),
            parse_request=lambda raw: raw,
            make_invalid_request_error=lambda err: DaemonResponse(
                ok=False,
                error=DaemonError(code="invalid_request", message="invalid request", details={"error": err}),
            ),
            send_json=lambda _conn, payload: sent.append(payload),
            dump_response=lambda resp: resp.model_dump(),
            try_handle_special=lambda _req, _conn: False,
            handle_request=lambda _req: (DaemonResponse(ok=True, result={}), False),
            logger=logging.getLogger("test"),
        )
        self.assertFalse(should_exit)
        self.assertTrue(conn.closed)
        self.assertEqual(sent, [])

    def test_special_handler_keeps_connection_open(self) -> None:
        conn = _FakeConn()
        should_exit = handle_incoming_connection(
            conn,
            recv_json_line=lambda _conn: {"op": "special"},
            parse_request=lambda raw: raw,
            make_invalid_request_error=lambda err: DaemonResponse(
                ok=False,
                error=DaemonError(code="invalid_request", message="invalid request", details={"error": err}),
            ),
            send_json=lambda _conn, _payload: None,
            dump_response=lambda resp: resp.model_dump(),
            try_handle_special=lambda _req, _conn: True,
            handle_request=lambda _req: (DaemonResponse(ok=True, result={}), False),
            logger=logging.getLogger("test"),
        )
        self.assertFalse(should_exit)
        self.assertFalse(conn.closed)

    def test_exception_in_request_handler_returns_internal_error(self) -> None:
        conn = _FakeConn()
        sent: list[dict] = []
        should_exit = handle_incoming_connection(
            conn,
            recv_json_line=lambda _conn: {"op": "x"},
            parse_request=lambda raw: raw,
            make_invalid_request_error=lambda err: DaemonResponse(
                ok=False,
                error=DaemonError(code="invalid_request", message="invalid request", details={"error": err}),
            ),
            send_json=lambda _conn, payload: sent.append(payload),
            dump_response=lambda resp: resp.model_dump(),
            try_handle_special=lambda _req, _conn: False,
            handle_request=lambda _req: (_ for _ in ()).throw(RuntimeError("boom")),
            logger=logging.getLogger("test"),
        )
        self.assertFalse(should_exit)
        self.assertTrue(conn.closed)
        self.assertTrue(sent)
        self.assertEqual(
            str((sent[0].get("error") or {}).get("code") or ""),
            "internal_error",
        )

    def test_schedule_request_hands_off_connection_without_direct_execution(self) -> None:
        conn = _FakeConn()
        scheduled: list[tuple[dict, _FakeConn]] = []
        should_exit = handle_incoming_connection(
            conn,
            recv_json_line=lambda _conn: {"op": "x"},
            parse_request=lambda raw: raw,
            make_invalid_request_error=lambda err: DaemonResponse(
                ok=False,
                error=DaemonError(code="invalid_request", message="invalid request", details={"error": err}),
            ),
            send_json=lambda _conn, _payload: None,
            dump_response=lambda resp: resp.model_dump(),
            try_handle_special=lambda _req, _conn: False,
            handle_request=lambda _req: (_ for _ in ()).throw(AssertionError("should not execute directly")),
            schedule_request=lambda req, queued_conn: scheduled.append((req, queued_conn)) or True,
            logger=logging.getLogger("test"),
        )
        self.assertFalse(should_exit)
        self.assertFalse(conn.closed)
        self.assertEqual(conn.timeout, None)
        self.assertEqual(len(scheduled), 1)
        self.assertEqual(scheduled[0][0], {"op": "x"})
        self.assertIs(scheduled[0][1], conn)


if __name__ == "__main__":
    unittest.main()
