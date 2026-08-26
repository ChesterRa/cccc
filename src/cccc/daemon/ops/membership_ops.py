"""Membership login and reach verbs."""

from __future__ import annotations

from typing import Any, Dict, Optional, Sequence, Tuple

from ...contracts.v1 import DaemonError, DaemonResponse
from ...kernel.access_tokens import list_access_tokens
from ...kernel.cloudflared import (
    CloudflaredError,
    ensure as ensure_cloudflared,
    inspect as inspect_cloudflared,
)
from ...kernel.membership import (
    LOGOUT_WARNING,
    account_origin,
    clear_membership,
    load_membership,
    pending_login_expired,
    public_urls,
    remember_membership_error,
    store_device_grant,
    store_pending_login,
    update_membership,
)
from ...kernel.membership_account import (
    AccountError,
    Transport,
    disable_device,
    fetch_device,
    issue_reach,
    poll_device_login,
    start_device_login,
)
from ...kernel.settings import (
    get_remote_access_settings,
    update_remote_access_settings,
)
from ...ports.web.runtime_control import allow_unauthenticated_web_listener
from ...ports.web.runtime_control import read_web_runtime_state, wait_for_web_ready
from ...util.process import pid_is_alive
from ...util.time import utc_now_iso
from . import cloudflared_supervisor


_transport: Optional[Transport] = None
_start_command: Optional[Sequence[str]] = None


def set_account_transport_for_tests(transport: Optional[Transport]) -> None:
    global _transport
    _transport = transport


def set_reach_command_for_tests(command: Optional[Sequence[str]]) -> None:
    global _start_command
    _start_command = command


def _error(
    code: str, message: str, *, details: Optional[Dict[str, Any]] = None
) -> DaemonResponse:
    return DaemonResponse(
        ok=False, error=DaemonError(code=code, message=message, details=(details or {}))
    )


def _require_user(args: Dict[str, Any]) -> Optional[DaemonResponse]:
    by = str(args.get("by") or "user").strip()
    if by and by != "user":
        return _error("permission_denied", "only user can manage membership")
    return None


def _admin_token_count() -> int:
    return sum(1 for token in list_access_tokens() if bool(token.get("is_admin")))


def _requested_origin(args: Dict[str, Any]) -> Optional[str]:
    if "account_origin" in args:
        return account_origin(str(args.get("account_origin") or ""))
    return account_origin()


def _bound_origin(state: Dict[str, Any]) -> Optional[str]:
    value = str(state.get("account_origin") or "").strip()
    if not value:
        pending = state.get("pending_login")
        if isinstance(pending, dict):
            value = str(pending.get("account_origin") or "").strip()
    return account_origin(value) if value else None


def _account_fail(exc: AccountError) -> DaemonResponse:
    remember_membership_error(exc.message)
    return _error(exc.code, exc.message)


def _cloudflared_fail(exc: CloudflaredError) -> DaemonResponse:
    remember_membership_error(exc.message)
    return _error(exc.code, exc.message)


def _apply_cut(remote: Dict[str, Any]) -> Optional[str]:
    def mark_cut(current: Dict[str, Any]) -> None:
        current["disabled"] = True
        if remote.get("hostname"):
            current["hostname"] = remote.get("hostname")

    update_membership(mark_cut)
    try:
        cloudflared_supervisor.stop()
    except RuntimeError as stop_error:
        message = f"failed to stop cloudflared after membership cut: {stop_error}"
        remember_membership_error(message)
        return message
    remote_cfg = get_remote_access_settings()
    if str(remote_cfg.get("provider") or "") == "reach":
        update_remote_access_settings(
            {
                "enabled": False,
                "web_public_url": "",
                "updated_at": utc_now_iso(),
            }
        )
    return None


def _refresh_cut_from_account() -> Tuple[Optional[str], Optional[bool], Optional[bool]]:
    state = load_membership()
    origin = _bound_origin(state)
    token = state.get("device_token")
    if not origin or not token or not state.get("logged_in"):
        return None, None, None
    try:
        remote = fetch_device(origin, str(token), transport=_transport, timeout_s=2.0)
    except AccountError as exc:
        if exc.code not in {"membership_disabled", "membership_not_logged_in"}:
            return None, None, False
        remote = {"disabled": True}
    if remote.get("disabled"):
        return _apply_cut(remote), False, True
    online = remote.get("online")
    return None, online if isinstance(online, bool) else None, True


def _live_web_port() -> int:
    runtime = read_web_runtime_state()
    host = str(runtime.get("host") or "").strip().lower()
    runtime_id = str(runtime.get("runtime_id") or "").strip()
    try:
        pid = int(runtime.get("pid") or 0)
        port = int(runtime.get("port") or 0)
    except (TypeError, ValueError):
        pid = 0
        port = 0
    if pid <= 0 or port <= 0 or port > 65535 or not pid_is_alive(pid):
        raise RuntimeError(
            "CCCC Web is not running with a known live binding; start `cccc` before enabling reach"
        )
    if not runtime_id:
        raise RuntimeError(
            "CCCC Web runtime identity is missing; restart `cccc` before enabling reach"
        )
    if host not in {"127.0.0.1", "localhost", "0.0.0.0"}:
        raise RuntimeError(
            "CCCC Web must accept connections on 127.0.0.1 before reach can start"
        )
    if not wait_for_web_ready(
        host="127.0.0.1",
        port=port,
        timeout_s=0.5,
        expected_runtime_id=runtime_id,
    ):
        raise RuntimeError(
            "CCCC Web recorded binding did not prove its runtime identity; "
            "restart `cccc` before enabling reach"
        )
    return port


def _status_payload() -> Dict[str, Any]:
    state = load_membership()
    remote = get_remote_access_settings()
    provider = str(remote.get("provider") or "off").strip().lower()
    enabled = bool(remote.get("enabled"))
    helper = inspect_cloudflared()
    child = cloudflared_supervisor.status()
    url_source = None
    if state.get("logged_in"):
        url_source = state.get("hostname") or remote.get("web_public_url")
    urls = public_urls(url_source)
    pending = (
        state.get("pending_login")
        if isinstance(state.get("pending_login"), dict)
        else None
    )
    if pending_login_expired(pending):
        pending = None
    cut = bool(state.get("disabled"))
    online = provider == "reach" and enabled and bool(child.get("running")) and not cut
    result: Dict[str, Any] = {
        "logged_in": bool(state.get("logged_in")),
        "device_id": state.get("device_id"),
        "hostname": urls["hostname"],
        "web_url": urls["web_url"],
        "online": online,
        "cut": cut,
        "disabled": cut,
        "in_reach": provider == "reach",
        "reach_supported": bool(helper.get("supported")),
        "account_origin": _bound_origin(state)
        or (account_origin() if not state.get("logged_in") else None),
        "last_error": state.get("last_error"),
        "cloudflared": {
            "installed": bool(helper.get("installed")),
            "matches_pin": bool(helper.get("matches_pin")),
            "version": helper.get("version"),
            "pinned_version": helper.get("pinned_version"),
            "running": bool(child.get("running")),
        },
    }
    if pending and not state.get("logged_in"):
        result["pending"] = {
            "user_code": pending.get("user_code"),
            "verification_uri": pending.get("verification_uri"),
            "verification_uri_complete": pending.get("verification_uri_complete"),
            "interval": pending.get("interval"),
            "expires_at": pending.get("expires_at"),
        }
    return {"membership": result}


def handle_membership_status(args: Dict[str, Any]) -> DaemonResponse:
    denied = _require_user(args)
    if denied is not None:
        return denied
    cleanup_error, account_online, account_reachable = _refresh_cut_from_account()
    if cleanup_error:
        return _error("membership_subprocess", cleanup_error)
    result = _status_payload()
    if account_reachable is not None:
        result["membership"]["account_reachable"] = account_reachable
    if account_online is False:
        result["membership"]["online"] = False
    return DaemonResponse(ok=True, result=result)


def handle_membership_login(args: Dict[str, Any]) -> DaemonResponse:
    denied = _require_user(args)
    if denied is not None:
        return denied
    existing = load_membership()
    if existing.get("logged_in") and existing.get("device_token"):
        return DaemonResponse(ok=True, result=_status_payload())
    pending = (
        existing.get("pending_login")
        if isinstance(existing.get("pending_login"), dict)
        else None
    )
    if pending is not None and not pending_login_expired(pending):
        return DaemonResponse(ok=True, result=_status_payload())
    origin = _requested_origin(args)
    if origin is None:
        remember_membership_error("membership account service is not configured")
        return _error(
            "membership_unavailable", "membership account service is not configured"
        )
    try:
        started = start_device_login(origin, transport=_transport)
    except AccountError as exc:
        return _account_fail(exc)
    store_pending_login(started, issuer=origin)
    return DaemonResponse(ok=True, result=_status_payload())


def handle_membership_login_poll(args: Dict[str, Any]) -> DaemonResponse:
    denied = _require_user(args)
    if denied is not None:
        return denied
    state = load_membership()
    if state.get("logged_in") and state.get("device_token"):
        return DaemonResponse(ok=True, result=_status_payload())
    pending = (
        state.get("pending_login")
        if isinstance(state.get("pending_login"), dict)
        else None
    )
    if pending_login_expired(pending):
        remember_membership_error("device code expired")
        return _error(
            "membership_network", "device code expired; run `cccc login` again"
        )
    assert isinstance(pending, dict)
    origin = _bound_origin(state)
    if origin is None:
        return _error(
            "membership_unavailable",
            "membership issuer is missing; run `cccc logout` and `cccc login` again",
        )
    try:
        grant = poll_device_login(
            origin, str(pending.get("device_code") or ""), transport=_transport
        )
    except AccountError as exc:
        if exc.retryable:
            if exc.retry_after_delta:
                try:
                    interval = max(1, int(pending.get("interval") or 5))
                except (TypeError, ValueError):
                    interval = 5
                pending["interval"] = interval + exc.retry_after_delta

                def update_pending(current: Dict[str, Any]) -> None:
                    current["pending_login"] = pending
                    current["last_error"] = None

                update_membership(update_pending)
            return DaemonResponse(ok=True, result=_status_payload())
        if exc.terminal_authorization:

            def clear_terminal_pending(current: Dict[str, Any]) -> None:
                current_pending = current.get("pending_login")
                if isinstance(current_pending, dict) and str(
                    current_pending.get("device_code") or ""
                ) == str(pending.get("device_code") or ""):
                    current["pending_login"] = None

            update_membership(clear_terminal_pending)
        return _account_fail(exc)
    store_device_grant(grant, issuer=origin)
    return DaemonResponse(ok=True, result=_status_payload())


def handle_membership_logout(args: Dict[str, Any]) -> DaemonResponse:
    denied = _require_user(args)
    if denied is not None:
        return denied
    state = load_membership()
    remote = get_remote_access_settings()
    try:
        cloudflared_supervisor.stop()
    except RuntimeError as exc:
        remember_membership_error(str(exc))
        return _error("membership_subprocess", f"failed to stop cloudflared: {exc}")
    remote_url = str(remote.get("web_public_url") or "").strip().rstrip("/")
    hostname = str(state.get("hostname") or "").strip().rstrip("/")
    retires_reach = str(
        remote.get("provider") or ""
    ).strip().lower() == "reach" or bool(
        hostname and remote_url and hostname == remote_url
    )
    origin = _bound_origin(state)
    token = str(state.get("device_token") or "").strip()
    if origin and token:
        try:
            disable_device(origin, token, transport=_transport)
        except AccountError as exc:
            if exc.code not in {"membership_not_logged_in", "membership_disabled"}:
                return _account_fail(exc)
    if retires_reach:
        update_remote_access_settings(
            {
                "enabled": False,
                "web_public_url": "",
                "updated_at": utc_now_iso(),
            }
        )
    clear_membership()
    result = _status_payload()
    result["membership"]["warning"] = LOGOUT_WARNING
    return DaemonResponse(ok=True, result=result)


def handle_membership_reach_install(args: Dict[str, Any]) -> DaemonResponse:
    denied = _require_user(args)
    if denied is not None:
        return denied
    upgrade = bool(args.get("upgrade"))
    try:
        ensure_cloudflared(upgrade=upgrade)
    except CloudflaredError as exc:
        return _cloudflared_fail(exc)
    return DaemonResponse(ok=True, result=_status_payload())


def handle_membership_reach_on(args: Dict[str, Any]) -> DaemonResponse:
    denied = _require_user(args)
    if denied is not None:
        return denied
    if allow_unauthenticated_web_listener():
        remember_membership_error(
            "CCCC_WEB_ALLOW_UNAUTHENTICATED is incompatible with reach"
        )
        return _error(
            "membership_gate",
            "CCCC_WEB_ALLOW_UNAUTHENTICATED is incompatible with reach",
        )
    if _admin_token_count() == 0:
        remember_membership_error(
            "an administrator access token is required before reach can start"
        )
        return _error(
            "membership_gate",
            "an administrator access token is required before reach can start",
        )
    remote = get_remote_access_settings()
    provider = str(remote.get("provider") or "off").strip().lower()
    if provider == "tailscale" and bool(remote.get("enabled")):
        remember_membership_error(f"remote access is already using {provider}")
        return _error(
            "membership_gate",
            f"remote access is already using {provider}; turn it off before `cccc reach on`",
        )
    state = load_membership()
    if not state.get("logged_in") or not state.get("device_token"):
        remember_membership_error("not logged in")
        return _error("membership_not_logged_in", "not logged in; run `cccc login`")
    cleanup_error, _account_online, _account_reachable = _refresh_cut_from_account()
    if cleanup_error:
        return _error("membership_subprocess", cleanup_error)
    state = load_membership()
    if state.get("disabled"):
        remember_membership_error("this device has been disabled")
        return _error("membership_disabled", "this device has been disabled")
    origin = _bound_origin(state)
    if origin is None:
        remember_membership_error("membership account service is not configured")
        return _error(
            "membership_unavailable",
            "membership issuer is missing; run `cccc logout` and `cccc login` again",
        )
    if _start_command is None:
        try:
            ensure_cloudflared(upgrade=False)
        except CloudflaredError as exc:
            return _cloudflared_fail(exc)
    try:
        origin_port = _live_web_port()
    except RuntimeError as exc:
        remember_membership_error(str(exc))
        return _error("membership_gate", str(exc))
    try:
        creds = issue_reach(
            origin,
            str(state.get("device_token")),
            origin_port=origin_port,
            transport=_transport,
        )
    except AccountError as exc:
        if exc.code in {"membership_disabled", "membership_not_logged_in"}:
            if cleanup_error := _apply_cut({"disabled": True}):
                return _error("membership_subprocess", cleanup_error)
            if exc.code == "membership_not_logged_in":
                message = (
                    "this linked device no longer exists; relink this installation"
                )
                remember_membership_error(message)
                return _error("membership_disabled", message)
        return _account_fail(exc)

    def store_reach(current: Dict[str, Any]) -> None:
        current["hostname"] = creds["hostname"]
        current["tunnel_token"] = creds["tunnel_token"]
        current["last_error"] = None

    update_membership(store_reach)
    try:
        cloudflared_supervisor.start(creds["tunnel_token"], command=_start_command)
    except Exception as exc:
        remember_membership_error(str(exc))
        return _error("membership_subprocess", f"failed to start cloudflared: {exc}")
    try:
        update_remote_access_settings(
            {
                "provider": "reach",
                "enabled": True,
                "require_access_token": True,
                "web_public_url": creds["hostname"],
                "updated_at": utc_now_iso(),
            }
        )
    except Exception:
        try:
            cloudflared_supervisor.stop()
        except RuntimeError as stop_error:
            remember_membership_error(
                "failed to persist reach state and failed to stop cloudflared: "
                f"{stop_error}"
            )
        raise
    return DaemonResponse(ok=True, result=_status_payload())


def handle_membership_reach_off(args: Dict[str, Any]) -> DaemonResponse:
    denied = _require_user(args)
    if denied is not None:
        return denied
    remote = get_remote_access_settings()
    provider = str(remote.get("provider") or "off").strip().lower()
    if provider != "reach":
        return _error(
            "membership_not_in_reach", "reach is not the active remote access provider"
        )
    try:
        cloudflared_supervisor.stop()
    except RuntimeError as exc:
        remember_membership_error(str(exc))
        return _error("membership_subprocess", f"failed to stop cloudflared: {exc}")
    if bool(remote.get("enabled")):
        update_remote_access_settings({"enabled": False, "updated_at": utc_now_iso()})
    return DaemonResponse(ok=True, result=_status_payload())


def try_handle_membership_op(op: str, args: Dict[str, Any]) -> Optional[DaemonResponse]:
    if op == "membership_status":
        return handle_membership_status(args)
    if op == "membership_login":
        return handle_membership_login(args)
    if op == "membership_login_poll":
        return handle_membership_login_poll(args)
    if op == "membership_logout":
        return handle_membership_logout(args)
    if op == "membership_reach_on":
        return handle_membership_reach_on(args)
    if op == "membership_reach_off":
        return handle_membership_reach_off(args)
    if op == "membership_reach_install":
        return handle_membership_reach_install(args)
    return None
