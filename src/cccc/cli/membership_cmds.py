"""Membership login and reach CLI verbs."""

from __future__ import annotations

import sys
import time

from .common import *  # noqa: F401,F403
from ..kernel.membership import account_origin

__all__ = [
    "cmd_login",
    "cmd_logout",
    "cmd_reach",
]

REACH_LONG_OPERATION_TIMEOUT_SECONDS = 120.0


def _daemon_error(message: str = "daemon unavailable; run `cccc daemon start`") -> int:
    _print_json(
        {
            "ok": False,
            "error": {"code": "membership_network", "message": message},
        }
    )
    return 2


def _membership_copy_lines(membership: dict) -> list[str]:
    hostname = str(membership.get("hostname") or "").strip()
    web = str(membership.get("web_url") or "").strip()
    connector = str(membership.get("connector_url") or "").strip()
    if not (hostname or web or connector):
        return []
    return [
        f"Hostname (people / account page): {hostname or '(none)'}",
        f"Web (this machine, includes admin token): {web or '(none)'}",
        f"ChatGPT connector (secret in the path): {connector or '(none)'}",
        "These are three different strings. Screenshotting the connector URL leaks a key.",
        "Rotating a token changes that URL. Paste it again in ChatGPT after a rotate, cut, or logout.",
    ]


def _print_membership_copy_lines(membership: dict) -> None:
    origin = str(
        membership.get("account_origin")
        or (None if membership.get("logged_in") else account_origin())
        or ""
    ).strip()
    if membership.get("logged_in"):
        state = (
            "cut"
            if membership.get("cut") or membership.get("disabled")
            else ("online" if membership.get("online") else "linked, not published")
        )
        suffix = f"  account: {origin}" if origin else ""
        print(f"Remote access: {state}{suffix}", file=sys.stderr)
    for line in _membership_copy_lines(membership):
        print(line, file=sys.stderr)


def _call(op: str, *, timeout_s: float | None = None, **args: object) -> dict:
    payload = {"by": "user"}
    origin = account_origin()
    if origin:
        payload["account_origin"] = origin
    payload.update(args)
    request = {"op": op, "args": payload}
    if timeout_s is None:
        return call_daemon(request)
    return call_daemon(request, timeout_s=timeout_s)


def cmd_login(_args: argparse.Namespace) -> int:
    if not _ensure_daemon_running():
        return _daemon_error()
    resp = _call("membership_login")
    if not resp.get("ok"):
        error = resp.get("error") if isinstance(resp.get("error"), dict) else {}
        if str(error.get("code") or "") == "unknown_op":
            return _daemon_error(
                "running daemon is too old for membership; stop it with "
                "`cccc daemon stop` and run `cccc login` again"
            )
        _print_json(resp)
        return 2
    membership = (resp.get("result") or {}).get("membership") or {}
    pending = (
        membership.get("pending")
        if isinstance(membership.get("pending"), dict)
        else None
    )
    if pending:
        origin = account_origin()
        if origin:
            print(f"Account: {origin}", file=sys.stderr)
        print(f"Open: {pending.get('verification_uri')}", file=sys.stderr)
        print(f"Code: {pending.get('user_code')}", file=sys.stderr)
        interval = 5
        try:
            interval = max(1, int(pending.get("interval") or 5))
        except (TypeError, ValueError):
            interval = 5
        while True:
            time.sleep(interval)
            poll = _call("membership_login_poll")
            if not poll.get("ok"):
                _print_json(poll)
                return 2
            body = (poll.get("result") or {}).get("membership") or {}
            if body.get("logged_in"):
                _print_membership_copy_lines(body)
                _print_json(poll)
                return 0
            next_pending = (
                body.get("pending") if isinstance(body.get("pending"), dict) else None
            )
            if next_pending:
                try:
                    interval = max(1, int(next_pending.get("interval") or interval))
                except (TypeError, ValueError):
                    pass
    _print_json(resp)
    return 0


def cmd_logout(_args: argparse.Namespace) -> int:
    if not _ensure_daemon_running():
        return _daemon_error()
    resp = _call("membership_logout")
    warning = ((resp.get("result") or {}).get("membership") or {}).get("warning")
    if warning:
        print(str(warning), file=sys.stderr)
    _print_json(resp)
    return 0 if resp.get("ok") else 2


def cmd_reach(args: argparse.Namespace) -> int:
    if not _ensure_daemon_running():
        return _daemon_error()
    action = str(getattr(args, "action", "") or "").strip()
    if action == "on":
        resp = _call(
            "membership_reach_on", timeout_s=REACH_LONG_OPERATION_TIMEOUT_SECONDS
        )
    elif action == "off":
        resp = _call("membership_reach_off")
    elif action == "status":
        resp = _call("membership_status")
    elif action == "install":
        resp = _call(
            "membership_reach_install",
            timeout_s=REACH_LONG_OPERATION_TIMEOUT_SECONDS,
            upgrade=True,
        )
    else:
        _print_json(
            {
                "ok": False,
                "error": {
                    "code": "invalid_args",
                    "message": "reach action must be on, off, status, or install",
                },
            }
        )
        return 2
    if resp.get("ok") and action in {"on", "off", "status"}:
        membership = (resp.get("result") or {}).get("membership") or {}
        if isinstance(membership, dict):
            _print_membership_copy_lines(membership)
    _print_json(resp)
    return 0 if resp.get("ok") else 2
