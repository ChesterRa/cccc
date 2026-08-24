"""Local membership identity stored under CCCC_HOME/secrets."""

from __future__ import annotations

import ipaddress
import json
import os
from datetime import datetime, timedelta, timezone
from pathlib import Path
from typing import Any, Callable, Dict, Optional
from urllib.parse import urlencode, urlparse

from ..paths import ensure_home
from ..util.file_lock import acquire_lockfile, release_lockfile
from ..util.fs import atomic_write_json
from ..util.time import utc_now_iso


LOGOUT_WARNING = (
    "This device and its public hostname were retired. "
    "The next login creates a new device and hostname."
)
DEFAULT_ACCOUNT_ORIGIN = "https://account.cccc.sh"
RETIRED_ACCOUNT_ORIGINS = {
    "http://account.cccc.foo",
    "https://account.cccc.foo",
}


def _canonical_account_origin(value: str) -> str:
    origin = value.rstrip("/")
    if origin.lower() in RETIRED_ACCOUNT_ORIGINS:
        return DEFAULT_ACCOUNT_ORIGIN
    return origin


def membership_path(home: Optional[Path] = None) -> Path:
    base = Path(home) if home is not None else ensure_home()
    return base / "secrets" / "membership.json"


def membership_lock_path(home: Optional[Path] = None) -> Path:
    return membership_path(home).with_name("membership.json.lock")


def account_origin(override: Optional[str] = None) -> Optional[str]:
    if override is not None:
        value = str(override).strip()
        return _canonical_account_origin(value) if value else None
    value = str(os.environ.get("CCCC_ACCOUNT_ORIGIN") or "").strip()
    return _canonical_account_origin(value or DEFAULT_ACCOUNT_ORIGIN)


def default_state() -> Dict[str, Any]:
    return {
        "logged_in": False,
        "account_origin": None,
        "device_id": None,
        "device_token": None,
        "hostname": None,
        "tunnel_token": None,
        "disabled": False,
        "last_error": None,
        "pending_login": None,
    }


def _as_optional_text(value: Any) -> Optional[str]:
    text = str(value or "").strip()
    return text or None


def normalize_reach_hostname(value: Any) -> Optional[str]:
    text = _as_optional_text(value)
    if text is None:
        return None
    if any(
        character.isspace() or ord(character) < 32 or ord(character) == 127
        for character in text
    ):
        return None
    candidate = text if "://" in text else f"https://{text}"
    try:
        parsed = urlparse(candidate)
        port = parsed.port
        hostname = parsed.hostname
    except ValueError:
        return None
    if (
        parsed.scheme.lower() != "https"
        or not hostname
        or parsed.username is not None
        or parsed.password is not None
        or parsed.path not in {"", "/"}
        or parsed.params
        or "?" in candidate
        or "#" in candidate
        or "%" in parsed.netloc
        or "\\" in parsed.netloc
    ):
        return None

    try:
        address = ipaddress.ip_address(hostname)
    except ValueError:
        try:
            canonical_host = hostname.encode("idna").decode("ascii").lower()
        except UnicodeError:
            return None
        dns_host = (
            canonical_host[:-1] if canonical_host.endswith(".") else canonical_host
        )
        labels = dns_host.split(".")
        if (
            not dns_host
            or len(dns_host) > 253
            or any(
                not label
                or len(label) > 63
                or label.startswith("-")
                or label.endswith("-")
                or not all(
                    character.isascii() and (character.isalnum() or character == "-")
                    for character in label
                )
                for label in labels
            )
        ):
            return None
    else:
        canonical_host = (
            f"[{address.compressed}]" if address.version == 6 else address.compressed
        )

    port_suffix = f":{port}" if port is not None and port != 443 else ""
    return f"https://{canonical_host}{port_suffix}"


def _as_optional_account_origin(value: Any) -> Optional[str]:
    text = _as_optional_text(value)
    return _canonical_account_origin(text) if text else None


def _load_membership_unlocked(path: Path) -> Dict[str, Any]:
    state = default_state()
    if not path.is_file():
        return state
    try:
        raw = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError):
        return state
    if not isinstance(raw, dict):
        return state
    state["logged_in"] = bool(raw.get("logged_in"))
    state["account_origin"] = _as_optional_account_origin(raw.get("account_origin"))
    state["device_id"] = _as_optional_text(raw.get("device_id"))
    state["device_token"] = _as_optional_text(raw.get("device_token"))
    state["hostname"] = _as_optional_text(raw.get("hostname"))
    state["tunnel_token"] = _as_optional_text(raw.get("tunnel_token"))
    state["disabled"] = bool(raw.get("disabled"))
    state["last_error"] = _as_optional_text(raw.get("last_error"))
    pending = raw.get("pending_login")
    state["pending_login"] = dict(pending) if isinstance(pending, dict) else None
    return state


def _normalized_state(state: Dict[str, Any]) -> Dict[str, Any]:
    merged = default_state()
    merged.update(state or {})
    pending = merged.get("pending_login")
    return {
        "logged_in": bool(merged.get("logged_in")),
        "account_origin": _as_optional_account_origin(merged.get("account_origin")),
        "device_id": _as_optional_text(merged.get("device_id")),
        "device_token": _as_optional_text(merged.get("device_token")),
        "hostname": _as_optional_text(merged.get("hostname")),
        "tunnel_token": _as_optional_text(merged.get("tunnel_token")),
        "disabled": bool(merged.get("disabled")),
        "last_error": _as_optional_text(merged.get("last_error")),
        "pending_login": dict(pending) if isinstance(pending, dict) else None,
    }


def _write_membership_unlocked(path: Path, state: Dict[str, Any]) -> Dict[str, Any]:
    normalized = _normalized_state(state)
    atomic_write_json(path, normalized)
    try:
        path.chmod(0o600)
    except OSError:
        pass
    return normalized


def load_membership(home: Optional[Path] = None) -> Dict[str, Any]:
    path = membership_path(home)
    lock = acquire_lockfile(membership_lock_path(home), blocking=True)
    try:
        return _load_membership_unlocked(path)
    finally:
        release_lockfile(lock)


def save_membership(
    state: Dict[str, Any], home: Optional[Path] = None
) -> Dict[str, Any]:
    path = membership_path(home)
    lock = acquire_lockfile(membership_lock_path(home), blocking=True)
    try:
        return _write_membership_unlocked(path, state)
    finally:
        release_lockfile(lock)


def update_membership(
    change: Callable[[Dict[str, Any]], None],
    home: Optional[Path] = None,
) -> Dict[str, Any]:
    path = membership_path(home)
    lock = acquire_lockfile(membership_lock_path(home), blocking=True)
    try:
        state = _load_membership_unlocked(path)
        change(state)
        return _write_membership_unlocked(path, state)
    finally:
        release_lockfile(lock)


def clear_membership(home: Optional[Path] = None) -> None:
    path = membership_path(home)
    lock = acquire_lockfile(membership_lock_path(home), blocking=True)
    try:
        try:
            path.unlink()
        except FileNotFoundError:
            return
    finally:
        release_lockfile(lock)


def remember_membership_error(
    message: str, home: Optional[Path] = None
) -> Dict[str, Any]:
    def change(state: Dict[str, Any]) -> None:
        state["last_error"] = _as_optional_text(message)

    return update_membership(change, home)


def store_pending_login(
    pending: Dict[str, Any],
    *,
    issuer: str,
    home: Optional[Path] = None,
) -> Dict[str, Any]:
    expires_in = int(pending.get("expires_in") or 900)
    expires_at = datetime.now(timezone.utc) + timedelta(seconds=max(30, expires_in))

    def change(state: Dict[str, Any]) -> None:
        state["account_origin"] = _canonical_account_origin(issuer)
        state["pending_login"] = {
            "device_code": pending["device_code"],
            "user_code": pending["user_code"],
            "verification_uri": pending["verification_uri"],
            "verification_uri_complete": _as_optional_text(
                pending.get("verification_uri_complete")
            ),
            "interval": int(pending.get("interval") or 5),
            "expires_at": expires_at.replace(microsecond=0)
            .isoformat()
            .replace("+00:00", "Z"),
            "account_origin": _canonical_account_origin(issuer),
        }
        state["last_error"] = None

    return update_membership(change, home)


def pending_login_expired(pending: Optional[Dict[str, Any]]) -> bool:
    if not isinstance(pending, dict):
        return True
    raw = str(pending.get("expires_at") or "").strip()
    if not raw:
        return True
    try:
        expires = datetime.fromisoformat(raw.replace("Z", "+00:00"))
    except ValueError:
        return True
    if expires.tzinfo is None:
        expires = expires.replace(tzinfo=timezone.utc)
    return datetime.now(timezone.utc) >= expires


def store_device_grant(
    grant: Dict[str, Any],
    *,
    issuer: str,
    home: Optional[Path] = None,
) -> Dict[str, Any]:
    def change(state: Dict[str, Any]) -> None:
        state["logged_in"] = True
        state["account_origin"] = _canonical_account_origin(issuer)
        state["device_id"] = _as_optional_text(grant.get("device_id"))
        state["device_token"] = _as_optional_text(grant.get("device_token"))
        if grant.get("hostname"):
            state["hostname"] = _as_optional_text(grant.get("hostname"))
        state["pending_login"] = None
        state["disabled"] = False
        state["last_error"] = None

    return update_membership(change, home)


def _first_admin_token(home: Optional[Path] = None) -> Optional[str]:
    from .access_tokens import list_access_tokens

    for item in list_access_tokens(home):
        if item.get("is_admin"):
            token = str(item.get("token") or "").strip()
            if token:
                return token
    return None


def public_urls(
    hostname: Optional[str], home: Optional[Path] = None
) -> Dict[str, Optional[str]]:
    origin = normalize_reach_hostname(hostname)
    web_url = None
    if origin:
        admin = _first_admin_token(home)
        if admin:
            web_url = f"{origin}/ui/?{urlencode({'token': admin})}"
    return {
        "hostname": origin,
        "web_url": web_url,
    }
