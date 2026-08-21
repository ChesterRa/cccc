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
    get_web_branding: Callable[[], Dict[str, Any]],
    update_web_branding_settings: Callable[[Dict[str, Any]], Dict[str, Any]],
) -> Optional[Tuple[DaemonResponse, bool]]:
    if op == "ping":
        return (
            DaemonResponse(
                ok=True,
                result={
                    "version": version,
                    "implementation": "python",
                    "pid": pid_provider(),
                    "ts": now_iso(),
                    "ipc_v": 1,
                    "capabilities": {
                        "events_stream": True,
                        "remote_access": True,
                        "presentation_browser_attach": True,
                        "presentation_browser_vnc_attach": True,
                        "space_provider_auth_browser_attach": True,
                        "space_provider_auth_browser_vnc_attach": True,
                        "web_model_browser_attach": True,
                        "web_model_browser_vnc_attach": True,
                        "term_attachment_status": False,
                        "term_attach_snapshot_v1": False,
                        "assistant_state": True,
                        "assistant_voice_recording_lease": True,
                        "assistant_voice_model_install": True,
                    },
                },
            ),
            False,
        )

    if op == "shutdown":
        expected_pid = args.get("expected_pid")
        if expected_pid is not None:
            if isinstance(expected_pid, bool) or not isinstance(expected_pid, int) or expected_pid <= 0:
                return _error("invalid_args", "expected_pid must be a positive integer"), False
            current_pid = pid_provider()
            if expected_pid != current_pid:
                return (
                    _error(
                        "daemon_owner_mismatch",
                        f"shutdown expected daemon pid {expected_pid}, but connected daemon pid is {current_pid}",
                    ),
                    False,
                )
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

    if op == "branding_get":
        return DaemonResponse(ok=True, result={"branding": get_web_branding()}), False

    if op == "branding_update":
        by = str(args.get("by") or "user").strip()
        if by and by != "user":
            return _error("permission_denied", "only user can update global branding settings"), False
        patch = args.get("patch") if isinstance(args.get("patch"), dict) else {}
        if not patch:
            return DaemonResponse(ok=True, result={"branding": get_web_branding()}), False
        try:
            updated = update_web_branding_settings(dict(patch))
            return DaemonResponse(ok=True, result={"branding": updated}), False
        except Exception as e:
            return _error("branding_update_failed", str(e)), False

    return None
