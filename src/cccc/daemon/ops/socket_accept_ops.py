"""Socket accept-loop helpers for daemon."""

from __future__ import annotations

import logging
from typing import Any, Callable, Dict, Tuple

from ...contracts.v1 import DaemonError, DaemonResponse


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
    logger: logging.Logger,
) -> bool:
    """Handle a single accepted daemon connection.

    Returns:
        should_exit flag requested by request handling.
    """
    raw = recv_json_line(conn)
    try:
        req = parse_request(raw)
    except Exception as e:
        resp = make_invalid_request_error(str(e))
        try:
            send_json(conn, dump_response(resp))
        finally:
            try:
                conn.close()
            except Exception:
                pass
        return False

    if try_handle_special(req, conn):
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
