"""Special socket-level operation handlers for daemon accept loop."""

from __future__ import annotations

from typing import Any, Callable, Dict, Optional, Set

from ...contracts.v1 import DaemonResponse
from ..group.presentation_browser_runtime import (
    attach_browser_surface_socket_with_mode,
    attach_browser_surface_vnc_socket,
    can_attach_browser_surface_socket,
    can_attach_browser_surface_vnc_socket,
)
from ..space.notebooklm_auth_browser_runtime import (
    attach_notebooklm_auth_browser_socket_with_mode,
    attach_notebooklm_auth_browser_vnc_socket,
    can_attach_notebooklm_auth_browser_socket,
    can_attach_notebooklm_auth_browser_vnc_socket,
)
from ..actors.web_model_browser_session import (
    attach_web_model_chatgpt_browser_socket,
    attach_web_model_chatgpt_browser_vnc_socket,
    can_attach_web_model_chatgpt_browser_socket,
    can_attach_web_model_chatgpt_browser_vnc_socket,
)


def _set_blocking_io(conn: Any) -> None:
    try:
        conn.settimeout(None)
    except Exception:
        pass


def try_handle_socket_special_op(
    req: Any,
    conn: Any,
    *,
    send_json: Callable[[Any, Dict[str, Any]], None],
    dump_response: Callable[[DaemonResponse], Dict[str, Any]],
    error: Callable[[str, str, Optional[Dict[str, Any]]], DaemonResponse],
    actor_running: Callable[[str, str], bool],
    attach_actor_socket: Callable[[str, str, Any, Optional[int]], None],
    load_group: Callable[[str], Any],
    find_actor: Callable[[Any, str], Any],
    effective_runner_kind: Callable[[str], str],
    supported_stream_kinds: Callable[[], Set[str]],
    start_events_stream: Callable[[Any, str, str, Optional[Set[str]], str, str], bool],
) -> bool:
    op = str(getattr(req, "op", "") or "").strip()
    args = getattr(req, "args", None) or {}

    if op == "term_attach":
        group_id = str(args.get("group_id") or "").strip()
        actor_id = str(args.get("actor_id") or "").strip()
        since_raw = args.get("since")
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
                            details={
                                "runner": runner_kind,
                                "runner_effective": runner_effective,
                            },
                        )
                    elif not actor_running(group_id, actor_id):
                        resp = error("actor_not_running", "actor is not running")
                    else:
                        resp = DaemonResponse(ok=True, result={"group_id": group_id, "actor_id": actor_id})
        try:
            send_json(conn, dump_response(resp))
            if resp.ok:
                _set_blocking_io(conn)
                attach_actor_socket(group_id, actor_id, conn, since)
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
                _set_blocking_io(conn)
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

    if op == "presentation_browser_attach":
        group_id = str(args.get("group_id") or "").strip()
        slot_id = str(args.get("slot") or "").strip().lower().replace("_", "-")
        by = str(args.get("by") or "user").strip() or "user"
        if not group_id:
            resp = error("missing_group_id", "missing group_id")
        elif slot_id not in {"slot-1", "slot-2", "slot-3", "slot-4"}:
            resp = error("invalid_slot", "slot must be one of: slot-1, slot-2, slot-3, slot-4")
        else:
            group = load_group(group_id)
            if group is None:
                resp = error("group_not_found", f"group not found: {group_id}")
            elif by != "user" and not isinstance(find_actor(group, by), dict):
                resp = error("unknown_actor", f"unknown actor: {by}")
            else:
                ok, info = can_attach_browser_surface_socket(group_id=group_id, slot_id=slot_id)
                if not ok:
                    resp = error(
                        str(info.get("code") or "browser_surface_attach_failed"),
                        str(info.get("message") or "browser surface attach failed"),
                        details=dict(info.get("details") or {}),
                    )
                else:
                    resp = DaemonResponse(ok=True, result={"group_id": group_id})

        try:
            send_json(conn, dump_response(resp))
            if resp.ok:
                _set_blocking_io(conn)
                if attach_browser_surface_socket_with_mode(
                    group_id=group_id,
                    slot_id=slot_id,
                    sock=conn,
                    viewer_mode=str(args.get("viewer_mode") or "auto"),
                ):
                    return True
        except Exception:
            pass
        try:
            conn.close()
        except Exception:
            pass
        return True

    if op == "presentation_browser_vnc_attach":
        group_id = str(args.get("group_id") or "").strip()
        slot_id = str(args.get("slot") or "").strip().lower().replace("_", "-")
        by = str(args.get("by") or "user").strip() or "user"
        if not group_id:
            resp = error("missing_group_id", "missing group_id")
        elif slot_id not in {"slot-1", "slot-2", "slot-3", "slot-4"}:
            resp = error("invalid_slot", "slot must be one of: slot-1, slot-2, slot-3, slot-4")
        else:
            group = load_group(group_id)
            if group is None:
                resp = error("group_not_found", f"group not found: {group_id}")
            elif by != "user" and not isinstance(find_actor(group, by), dict):
                resp = error("unknown_actor", f"unknown actor: {by}")
            else:
                ok, info = can_attach_browser_surface_vnc_socket(group_id=group_id, slot_id=slot_id)
                if not ok:
                    resp = error(
                        str(info.get("code") or "browser_vnc_attach_failed"),
                        str(info.get("message") or "browser VNC attach failed"),
                        details=dict(info.get("details") or {}),
                    )
                else:
                    resp = DaemonResponse(ok=True, result={"group_id": group_id})

        try:
            send_json(conn, dump_response(resp))
            if resp.ok:
                _set_blocking_io(conn)
                if attach_browser_surface_vnc_socket(group_id=group_id, slot_id=slot_id, sock=conn):
                    return True
        except Exception:
            pass
        try:
            conn.close()
        except Exception:
            pass
        return True

    if op == "space_provider_auth_browser_attach":
        provider = str(args.get("provider") or "").strip().lower()
        by = str(args.get("by") or "user").strip() or "user"
        if provider != "notebooklm":
            resp = error("space_job_invalid", f"unsupported provider auth browser attach: {provider or 'unknown'}")
        elif by != "user":
            resp = error("space_permission_denied", "only user can attach provider auth browser surface")
        else:
            ok, info = can_attach_notebooklm_auth_browser_socket()
            if not ok:
                resp = error(
                    str(info.get("code") or "browser_surface_attach_failed"),
                    str(info.get("message") or "browser surface attach failed"),
                    details=dict(info.get("details") or {}),
                )
            else:
                resp = DaemonResponse(ok=True, result={"provider": "notebooklm"})

        try:
            send_json(conn, dump_response(resp))
            if resp.ok:
                _set_blocking_io(conn)
                if attach_notebooklm_auth_browser_socket_with_mode(
                    sock=conn,
                    viewer_mode=str(args.get("viewer_mode") or "auto"),
                ):
                    return True
        except Exception:
            pass
        try:
            conn.close()
        except Exception:
            pass
        return True

    if op == "space_provider_auth_browser_vnc_attach":
        provider = str(args.get("provider") or "").strip().lower()
        by = str(args.get("by") or "user").strip() or "user"
        if provider != "notebooklm":
            resp = error("space_job_invalid", f"unsupported provider auth browser VNC attach: {provider or 'unknown'}")
        elif by != "user":
            resp = error("space_permission_denied", "only user can attach provider auth browser surface")
        else:
            ok, info = can_attach_notebooklm_auth_browser_vnc_socket()
            if not ok:
                resp = error(
                    str(info.get("code") or "browser_vnc_attach_failed"),
                    str(info.get("message") or "browser VNC attach failed"),
                    details=dict(info.get("details") or {}),
                )
            else:
                resp = DaemonResponse(ok=True, result={"provider": provider})

        try:
            send_json(conn, dump_response(resp))
            if resp.ok:
                _set_blocking_io(conn)
                if attach_notebooklm_auth_browser_vnc_socket(sock=conn):
                    return True
        except Exception:
            pass
        try:
            conn.close()
        except Exception:
            pass
        return True

    if op == "web_model_browser_attach":
        group_id = str(args.get("group_id") or "").strip()
        actor_id = str(args.get("actor_id") or "").strip()
        if group_id or actor_id:
            group = load_group(group_id) if group_id else None
            if group is None:
                resp = error("group_not_found", f"group not found: {group_id}")
            else:
                actor = find_actor(group, actor_id)
                if not isinstance(actor, dict):
                    resp = error("actor_not_found", f"actor not found: {actor_id}")
                elif str(actor.get("runtime") or "").strip().lower() != "web_model":
                    resp = error(
                        "invalid_actor_runtime",
                        "ChatGPT browser sessions can only be bound to actors using runtime=web_model",
                        details={"group_id": group_id, "actor_id": actor_id},
                    )
                else:
                    ok, info = can_attach_web_model_chatgpt_browser_socket(group_id=group_id, actor_id=actor_id)
                    if not ok:
                        resp = error(
                            str(info.get("code") or "browser_surface_attach_failed"),
                            str(info.get("message") or "browser surface attach failed"),
                            details=dict(info.get("details") or {}),
                        )
                    else:
                        resp = DaemonResponse(ok=True, result={"group_id": group_id, "actor_id": actor_id})
        else:
            ok, info = can_attach_web_model_chatgpt_browser_socket(group_id="", actor_id="")
            if not ok:
                resp = error(
                    str(info.get("code") or "browser_surface_attach_failed"),
                    str(info.get("message") or "browser surface attach failed"),
                    details=dict(info.get("details") or {}),
                )
            else:
                resp = DaemonResponse(ok=True, result={"group_id": "", "actor_id": ""})

        try:
            send_json(conn, dump_response(resp))
            if resp.ok:
                _set_blocking_io(conn)
                if attach_web_model_chatgpt_browser_socket(
                    group_id=group_id,
                    actor_id=actor_id,
                    sock=conn,
                    viewer_mode=str(args.get("viewer_mode") or "auto"),
                ):
                    return True
        except Exception:
            pass
        try:
            conn.close()
        except Exception:
            pass
        return True

    if op == "web_model_browser_vnc_attach":
        group_id = str(args.get("group_id") or "").strip()
        actor_id = str(args.get("actor_id") or "").strip()
        if group_id or actor_id:
            group = load_group(group_id) if group_id else None
            if group is None:
                resp = error("group_not_found", f"group not found: {group_id}")
            else:
                actor = find_actor(group, actor_id)
                if not isinstance(actor, dict):
                    resp = error("actor_not_found", f"actor not found: {actor_id}")
                elif str(actor.get("runtime") or "").strip().lower() != "web_model":
                    resp = error(
                        "invalid_actor_runtime",
                        "ChatGPT browser sessions can only be bound to actors using runtime=web_model",
                        details={"group_id": group_id, "actor_id": actor_id},
                    )
                else:
                    ok, info = can_attach_web_model_chatgpt_browser_vnc_socket(group_id=group_id, actor_id=actor_id)
                    if not ok:
                        resp = error(
                            str(info.get("code") or "browser_vnc_attach_failed"),
                            str(info.get("message") or "browser VNC attach failed"),
                            details=dict(info.get("details") or {}),
                        )
                    else:
                        resp = DaemonResponse(ok=True, result={"group_id": group_id, "actor_id": actor_id})
        else:
            ok, info = can_attach_web_model_chatgpt_browser_vnc_socket(group_id="", actor_id="")
            if not ok:
                resp = error(
                    str(info.get("code") or "browser_vnc_attach_failed"),
                    str(info.get("message") or "browser VNC attach failed"),
                    details=dict(info.get("details") or {}),
                )
            else:
                resp = DaemonResponse(ok=True, result={"group_id": "", "actor_id": ""})

        try:
            send_json(conn, dump_response(resp))
            if resp.ok:
                _set_blocking_io(conn)
                if attach_web_model_chatgpt_browser_vnc_socket(group_id=group_id, actor_id=actor_id, sock=conn):
                    return True
        except Exception:
            pass
        try:
            conn.close()
        except Exception:
            pass
        return True

    return False
