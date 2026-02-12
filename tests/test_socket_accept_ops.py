import logging
import unittest

from cccc.contracts.v1 import DaemonError, DaemonResponse
from cccc.daemon.ops.socket_accept_ops import handle_incoming_connection


class _FakeConn:
    def __init__(self) -> None:
        self.closed = False

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


if __name__ == "__main__":
    unittest.main()
