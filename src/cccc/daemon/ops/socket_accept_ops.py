"""Socket accept-loop helpers for daemon."""

from __future__ import annotations

import logging
import socket
from typing import Any, Callable, Dict, Tuple

from ...contracts.v1 import DaemonError, DaemonResponse

REQUEST_READ_TIMEOUT_S = 0.5


def handle_incoming_connection(
    conn: Any,
    *,
    recv_json_line: Callable[[Any], Dict[str, Any]],
    parse_request: Callable[[Dict[str, Any]], Any],
    make_invalid_request_error: Callable[[str], DaemonResponse],
    send_json: Callable[[Any, Dict[str, Any]], None],
    dump_response: Callable[[Any], Dict[str, Any]],
    try_handle_special: Callable[[Any, Any], bool],
    handle_request: Callable[[Any], Tuple[Any, bool]],
    schedule_request: Callable[[Any, Any], bool] | None = None,
    logger: logging.Logger,
) -> bool:
    """Handle a single accepted daemon connection.

    Returns:
        should_exit flag requested by request handling.
    """
    try:
        # Guard the single-threaded accept loop from stalling forever on
        # half-open clients that never send a request line.
        conn.settimeout(REQUEST_READ_TIMEOUT_S)
    except Exception:
        pass

    try:
        raw = recv_json_line(conn)
    except (socket.timeout, TimeoutError, ConnectionResetError, BrokenPipeError, OSError):
        try:
            conn.close()
        except Exception:
            pass
        return False
    try:
        req = parse_request(raw)
    except Exception as e:
        resp = make_invalid_request_error(str(e))
        try:
            send_json(conn, dump_response(resp))
        except (BrokenPipeError, ConnectionResetError, OSError):
            pass
        finally:
            try:
                conn.close()
            except Exception:
                pass
        return False

    if try_handle_special(req, conn):
        return False

    if schedule_request is not None:
        try:
            conn.settimeout(None)
        except Exception:
            pass
        if schedule_request(req, conn):
            return False
        try:
            error_resp = DaemonResponse(
                ok=False,
                error=DaemonError(
                    code="internal_error",
                    message="internal error: request executor unavailable",
                ),
            )
            send_json(conn, dump_response(error_resp))
        except Exception:
            pass
        finally:
            try:
                conn.close()
            except Exception:
                pass
        return False

    should_exit = False
    try:
        resp, should_exit = handle_request(req)
        try:
            send_json(conn, dump_response(resp))
        except (BrokenPipeError, ConnectionResetError, OSError):
            pass
    except Exception as e:
        logger.exception("Unexpected error in handle_request: %s", e)
        try:
            error_resp = DaemonResponse(
                ok=False,
                error=DaemonError(
                    code="internal_error",
                    message=f"internal error: {type(e).__name__}: {e}",
                ),
            )
            send_json(conn, dump_response(error_resp))
        except Exception:
            pass
    finally:
        try:
            conn.close()
        except Exception:
            pass

    return bool(should_exit)
