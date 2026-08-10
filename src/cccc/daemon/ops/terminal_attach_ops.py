"""PTY socket-upgrade handshake and replay-boundary coordination."""

from __future__ import annotations

from typing import Any, Callable, Dict, Optional

from ...contracts.v1 import DaemonResponse


def _response(
    *,
    base_result: Dict[str, Any],
    mode: str,
    terminal_writable: bool,
    writer_replaced: bool,
    since: Optional[int],
    replay_start: int,
    replay_end: int,
) -> DaemonResponse:
    replay_cursor = max(int(since) if since is not None else 0, int(replay_start))
    return DaemonResponse(
        ok=True,
        result={
            **base_result,
            "terminal_mode": mode,
            "terminal_writable": bool(terminal_writable),
            "writer_replaced": bool(writer_replaced),
            "replay_cursor": replay_cursor,
            "replay_end_cursor": max(replay_cursor, int(replay_end)),
        },
    )


def handle_terminal_attach(
    req: Any,
    conn: Any,
    *,
    send_json: Callable[[Any, Dict[str, Any]], None],
    dump_response: Callable[[DaemonResponse], Dict[str, Any]],
    error: Callable[[str, str, Optional[Dict[str, Any]]], DaemonResponse],
    actor_running: Callable[[str, str], bool],
    attach_actor_socket: Callable[..., Any],
    backlog_start_offset: Callable[[str, str], int],
    backlog_end_offset: Optional[Callable[[str, str], int]],
    load_group: Callable[[str], Any],
    find_actor: Callable[[Any, str], Any],
    effective_runner_kind: Callable[[str], str],
    set_blocking_io: Callable[[Any], None],
) -> bool:
    args = getattr(req, "args", None) or {}
    group_id = str(args.get("group_id") or "").strip()
    actor_id = str(args.get("actor_id") or "").strip()
    since_raw = args.get("since")
    mode = str(args.get("mode") or "control").strip().lower()
    if mode not in {"control", "viewer"}:
        mode = "control"
    takeover = bool(args.get("takeover")) if mode == "control" else False
    since: Optional[int] = None
    if since_raw is not None and str(since_raw).strip() != "":
        try:
            since = int(since_raw)
        except Exception:
            since = None

    if not group_id:
        resp = error("missing_group_id", "missing group_id")
    elif not actor_id:
        resp = error("missing_actor_id", "missing actor_id")
    else:
        group = load_group(group_id)
        if group is None:
            resp = error("group_not_found", f"group not found: {group_id}")
        else:
            actor = find_actor(group, actor_id)
            if not isinstance(actor, dict):
                resp = error("actor_not_found", f"actor not found: {actor_id}")
            else:
                runner_kind = str(actor.get("runner") or "pty").strip() or "pty"
                runner_effective = effective_runner_kind(runner_kind)
                if runner_effective != "pty":
                    resp = error(
                        "not_pty_actor",
                        "terminal attach is only available for PTY actors",
                        details={"runner": runner_kind, "runner_effective": runner_effective},
                    )
                elif not actor_running(group_id, actor_id):
                    resp = error("actor_not_running", "actor is not running")
                else:
                    resp = DaemonResponse(
                        ok=True,
                        result={"group_id": group_id, "actor_id": actor_id},
                    )

    try:
        if not resp.ok:
            send_json(conn, dump_response(resp))
        else:
            base_result = dict(resp.result) if isinstance(resp.result, dict) else {}
            response_sent = False

            def send_attach_response(
                replay_start: int,
                replay_end: int,
                attach_state: Optional[Dict[str, Any]] = None,
            ) -> None:
                nonlocal response_sent
                if response_sent:
                    return
                actual = attach_state if isinstance(attach_state, dict) else {}
                actual_mode = str(actual.get("mode") or mode).strip().lower()
                if actual_mode not in {"control", "viewer"}:
                    actual_mode = mode
                terminal_writable = (
                    bool(actual.get("writable")) if "writable" in actual else actual_mode == "control"
                )
                writer_replaced = (
                    bool(actual.get("writer_replaced"))
                    if "writer_replaced" in actual
                    else bool(takeover)
                )
                attach_resp = _response(
                    base_result=base_result,
                    mode=actual_mode,
                    terminal_writable=terminal_writable,
                    writer_replaced=writer_replaced,
                    since=since,
                    replay_start=replay_start,
                    replay_end=replay_end,
                )
                send_json(conn, dump_response(attach_resp))
                response_sent = True

            set_blocking_io(conn)
            try:
                attach_result = attach_actor_socket(
                    group_id,
                    actor_id,
                    conn,
                    since,
                    mode,
                    takeover,
                    on_replay_snapshot=send_attach_response,
                )
            except TypeError:
                # Compatibility for older/custom adapters. Built-in PTY
                # backends always use the atomic snapshot callback above.
                try:
                    start_offset = int(backlog_start_offset(group_id, actor_id))
                except Exception:
                    start_offset = 0
                try:
                    end_offset = (
                        int(backlog_end_offset(group_id, actor_id))
                        if backlog_end_offset is not None
                        else start_offset
                    )
                except Exception:
                    end_offset = start_offset
                send_attach_response(start_offset, end_offset)
                try:
                    attach_actor_socket(group_id, actor_id, conn, since, mode, takeover)
                except TypeError:
                    attach_actor_socket(group_id, actor_id, conn, since)
                attach_result = None
            if isinstance(attach_result, dict) and attach_result.get("error"):
                raise RuntimeError(str(attach_result.get("error")))
            return True
    except Exception:
        pass
    try:
        conn.close()
    except Exception:
        pass
    return True
