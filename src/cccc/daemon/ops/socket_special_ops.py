"""Special socket-level operation handlers for daemon accept loop."""

from __future__ import annotations

from typing import Any, Callable, Dict, Optional, Set

from ...contracts.v1 import DaemonResponse


def try_handle_socket_special_op(
    req: Any,
    conn: Any,
    *,
    send_json: Callable[[Any, Dict[str, Any]], None],
    dump_response: Callable[[DaemonResponse], Dict[str, Any]],
    error: Callable[[str, str, Optional[Dict[str, Any]]], DaemonResponse],
    actor_running: Callable[[str, str], bool],
    attach_actor_socket: Callable[[str, str, Any], None],
    load_group: Callable[[str], Any],
    find_actor: Callable[[Any, str], Any],
    supported_stream_kinds: Callable[[], Set[str]],
    start_events_stream: Callable[[Any, str, str, Optional[Set[str]], str, str], bool],
) -> bool:
    op = str(getattr(req, "op", "") or "").strip()
    args = getattr(req, "args", None) or {}

    if op == "term_attach":
        group_id = str(args.get("group_id") or "").strip()
        actor_id = str(args.get("actor_id") or "").strip()
        if not group_id:
            resp = error("missing_group_id", "missing group_id")
        elif not actor_id:
            resp = error("missing_actor_id", "missing actor_id")
        elif not actor_running(group_id, actor_id):
            resp = error("actor_not_running", "actor is not running")
        else:
            resp = DaemonResponse(ok=True, result={"group_id": group_id, "actor_id": actor_id})
        try:
            send_json(conn, dump_response(resp))
            if resp.ok:
                attach_actor_socket(group_id, actor_id, conn)
                return True
        except Exception:
            pass
        try:
            conn.close()
        except Exception:
            pass
        return True

    if op == "events_stream":
        group_id = str(args.get("group_id") or "").strip()
        by = str(args.get("by") or "user").strip() or "user"
        since_event_id = str(args.get("since_event_id") or "").strip()
        since_ts = str(args.get("since_ts") or "").strip()
        kinds_raw = args.get("kinds")
        kinds: Optional[Set[str]] = None
        kinds_invalid = False
        if isinstance(kinds_raw, list):
            try:
                allowed = supported_stream_kinds()
                items = {str(x).strip() for x in kinds_raw if isinstance(x, str) and str(x).strip()}
                filtered = {kind for kind in items if kind in allowed}
                if filtered:
                    kinds = filtered
                elif not filtered and any(isinstance(x, str) and str(x).strip() for x in kinds_raw):
                    kinds_invalid = True
                else:
                    kinds = None
            except Exception:
                kinds = None

        if not group_id:
            resp = error("missing_group_id", "missing group_id")
        elif kinds_invalid:
            resp = error(
                "invalid_kinds",
                "no supported kinds requested",
                details={"supported": sorted(supported_stream_kinds())},
            )
        else:
            group = load_group(group_id)
            if group is None:
                resp = error("group_not_found", f"group not found: {group_id}")
            elif by != "user" and not isinstance(find_actor(group, by), dict):
                resp = error("unknown_actor", f"unknown actor: {by}")
            else:
                resp = DaemonResponse(ok=True, result={"group_id": group_id})

        try:
            send_json(conn, dump_response(resp))
            if resp.ok:
                started = start_events_stream(conn, group_id, by, kinds, since_event_id, since_ts)
                if started:
                    return True
        except Exception:
            pass
        try:
            conn.close()
        except Exception:
            pass
        return True

    return False
