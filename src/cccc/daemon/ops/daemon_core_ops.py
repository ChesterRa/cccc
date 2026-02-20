"""Core daemon operation handlers (ping/shutdown/observability)."""

from __future__ import annotations

from typing import Any, Callable, Dict, Optional, Tuple

from ...contracts.v1 import DaemonError, DaemonResponse


def _error(code: str, message: str, *, details: Optional[Dict[str, Any]] = None) -> DaemonResponse:
    return DaemonResponse(ok=False, error=DaemonError(code=code, message=message, details=(details or {})))


def try_handle_daemon_core_op(
    op: str,
    args: Dict[str, Any],
    *,
    version: str,
    pid_provider: Callable[[], int],
    now_iso: Callable[[], str],
    get_observability: Callable[[], Dict[str, Any]],
    update_observability_settings: Callable[[Dict[str, Any]], Dict[str, Any]],
    apply_observability_settings: Callable[[Dict[str, Any]], None],
) -> Optional[Tuple[DaemonResponse, bool]]:
    if op == "ping":
        return (
            DaemonResponse(
                ok=True,
                result={
                    "version": version,
                    "pid": pid_provider(),
                    "ts": now_iso(),
                    "ipc_v": 1,
                    "capabilities": {
                        "events_stream": True,
                        "remote_access": True,
                    },
                },
            ),
            False,
        )

    if op == "shutdown":
        return DaemonResponse(ok=True, result={"message": "shutting down"}), True

    if op == "observability_get":
        return DaemonResponse(ok=True, result={"observability": get_observability()}), False

    if op == "observability_update":
        by = str(args.get("by") or "user").strip()
        if by and by != "user":
            return _error("permission_denied", "only user can update global observability settings"), False
        patch = args.get("patch") if isinstance(args.get("patch"), dict) else {}
        if not patch:
            return DaemonResponse(ok=True, result={"observability": get_observability()}), False
        try:
            updated = update_observability_settings(dict(patch))
            apply_observability_settings(updated)
            return DaemonResponse(ok=True, result={"observability": updated}), False
        except Exception as e:
            return _error("observability_update_failed", str(e)), False

    return None
