"""Master-token headless auth: mint NotebookLM web cookies from a durable Google
``aas_et/`` master token, with no per-session browser.

Flow (proven against the live API in the #1638 spike):

    oauth_token (single-use, from one EmbeddedSetup browser sign-in)
      --exchange_token-->  aas_et/ master token   (durable; persisted 0600)
      --perform_oauth-->   ya29 OAuthLogin token
      --OAuthLogin?issueuberauth=1-->  uberauth
      --MergeSession-->    SID/SAPISID/__Secure-1PSID/... cookie jar

The minted jar authorizes the existing web client (batchexecute, upload,
download). After MergeSession the mint also fires one ``RotateCookies`` POST to
add ``__Secure-1PSIDTS`` (the rotating freshness partner of ``__Secure-1PSID``),
so the stored jar is complete at rest. That POST is best-effort — if Google
withholds it, the standard inline recovery still mints it on first load from
``SID`` + ``APISID``/``SAPISID`` (secondary binding).

SECURITY: the master token is full-account, durable, infostealer-grade — use a
dedicated/throwaway account only. Never log the oauth_token, master token, ya29,
uberauth, or cookie values.
"""

from __future__ import annotations

import asyncio
import json
import logging
import os
import secrets
import threading
from collections.abc import Iterator
from contextlib import contextmanager
from pathlib import Path
from typing import Any

import httpx

# perform_oauth for the OAuthLogin token rides the Chromecast app + signature
# (the spike confirmed the labs-tailwind app's sig downscopes; chromecast yields
# a uberauth-capable token; the labs-tailwind app's sig downscopes to email).
_MASTER_APP = "com.google.android.apps.chromecast.app"
_MASTER_SIG = "24bb24c05e47e0aefa68a58a766179d9b613a600"
_OAUTHLOGIN_SERVICE = "oauth2:https://www.google.com/accounts/OAuthLogin"

# MergeSession requires SID + a secondary binding (APISID+SAPISID or OSID) so the
# client's _recover_psidts_inline can mint __Secure-1PSIDTS on first load.
_REQUIRED_MINTED_COOKIES = {"SID", "APISID", "SAPISID"}

_MASTER_TOKEN_VERSION = 1

logger = logging.getLogger("notebooklm.auth.master_token")

# Serializes the global-logger save/restore in _quiet_gpsoauth_logging so
# overlapping re-mints on different threads (asyncio.to_thread) can't stomp each
# other's saved levels. ponytail: one process-wide lock; the window is one short
# sync RPC, so contention is negligible.
_LOG_LOCK = threading.Lock()


class MasterTokenError(Exception):
    """The master token (or its exchange) was rejected — re-bootstrap needed.

    Raised for revoked/expired master tokens, gpsoauth failures, and a minted
    cookie jar missing the cookies the web client needs. Carries no secrets.
    """


def _require_gpsoauth() -> Any:
    try:
        import gpsoauth  # noqa: PLC0415  (lazy: optional [headless] extra)
    except ImportError as exc:  # pragma: no cover - import guard
        raise MasterTokenError(
            "Master-token auth needs gpsoauth. Install: pip install 'notebooklm-py[headless]'"
        ) from exc
    return gpsoauth


@contextmanager
def _quiet_gpsoauth_logging() -> Iterator[None]:
    """Silence urllib3/requests DEBUG bodies around the gpsoauth call so the
    master token / ya29 in request bodies never reach a debug log sink."""
    names = ("urllib3", "requests", "urllib3.connectionpool")
    with _LOG_LOCK:
        saved = {n: logging.getLogger(n).level for n in names}
        try:
            for n in names:
                logging.getLogger(n).setLevel(logging.WARNING)
            yield
        finally:
            for n, lvl in saved.items():
                logging.getLogger(n).setLevel(lvl)


def generate_android_id() -> str:
    """Random stable 64-bit hex Android id, generated once per install and
    persisted with the token. Changing it can re-trip Google's new-device risk
    signal on re-mint, so callers must reuse the stored value."""
    return secrets.token_hex(8)


def exchange_master_token(email: str, oauth_token: str, android_id: str) -> str:
    """One-time: a single-use EmbeddedSetup ``oauth_token`` -> durable ``aas_et/``
    master token. Raises :class:`MasterTokenError` on rejection (no secret leak)."""
    gpsoauth = _require_gpsoauth()
    try:
        with _quiet_gpsoauth_logging():
            res = gpsoauth.exchange_token(email, oauth_token, android_id)
    except Exception as exc:  # noqa: BLE001 — any gpsoauth/transport failure; never leak the body
        raise MasterTokenError("exchange_token failed (network or gpsoauth error).") from exc
    token = res.get("Token")
    if not token:
        # res may carry Error/ErrorDetail (no secrets); include only the code.
        raise MasterTokenError(
            f"exchange_token rejected the oauth_token (Error={res.get('Error', 'unknown')}). "
            "The oauth_token is single-use and short-lived — re-capture it."
        )
    return str(token)


async def mint_cookies(email: str, master_token: str, android_id: str) -> httpx.Cookies:
    """Mint a fresh NotebookLM web cookie jar from the master token.

    perform_oauth (sync, run inline — it is a single short request) -> ya29, then
    OAuthLogin?issueuberauth=1 -> uberauth -> MergeSession -> Set-Cookie jar.
    Raises :class:`MasterTokenError` if the token is revoked or the jar lacks the
    cookies the web client needs.
    """
    gpsoauth = _require_gpsoauth()

    def _perform() -> Any:
        with _quiet_gpsoauth_logging():
            return gpsoauth.perform_oauth(
                email,
                master_token,
                android_id,
                service=_OAUTHLOGIN_SERVICE,
                app=_MASTER_APP,
                client_sig=_MASTER_SIG,
            )

    try:
        # perform_oauth is a sync (requests) network call — off-thread it so it
        # never blocks the event loop of a live client during layer-4 recovery.
        oauth = await asyncio.to_thread(_perform)
    except Exception as exc:  # noqa: BLE001 — any gpsoauth/transport failure; never leak the body
        raise MasterTokenError("perform_oauth failed (network or gpsoauth error).") from exc
    bearer = oauth.get("Auth")
    if not bearer:
        raise MasterTokenError(
            f"perform_oauth rejected the master token (Error={oauth.get('Error', 'unknown')}). "
            "Re-bootstrap with `notebooklm login --master-token`."
        )

    # Wrap the cookie-mint HTTP legs: an unwrapped httpx error would escape the
    # refresh path AND its ``.request.url`` embeds the uberauth token. Re-raise as
    # a secret-free MasterTokenError so the caller declines gracefully.
    try:
        async with httpx.AsyncClient(follow_redirects=True, timeout=30.0) as client:
            auth = {"Authorization": f"Bearer {bearer}"}
            uber = await client.get(
                "https://accounts.google.com/OAuthLogin",
                params={"source": "ChromiumBrowser", "issueuberauth": "1"},
                headers=auth,
            )
            uberauth = uber.text.strip()
            if uber.status_code != 200 or not uberauth or " " in uberauth:
                raise MasterTokenError("OAuthLogin did not return a uberauth token.")
            await client.get(
                "https://accounts.google.com/MergeSession",
                params={
                    "service": "mail",
                    "continue": "https://www.google.com",
                    "uberauth": uberauth,
                },
                headers=auth,
            )
            # Mint __Secure-1PSIDTS now too (the rotating freshness partner of
            # __Secure-1PSID) so the stored jar is complete and valid at rest — no
            # first-call recovery needed and `auth check` passes immediately. This
            # is the same RotateCookies POST the keepalive/inline recovery use; it
            # needs the SID + APISID/SAPISID binding the MergeSession jar already
            # carries. Best-effort: Google may withhold it, and inline recovery
            # remains the fallback, so a failure here must not fail the mint.
            from .keepalive import (  # noqa: PLC0415 (low-level; avoid import cycle)
                _KEEPALIVE_ROTATE_BODY,
                _KEEPALIVE_ROTATE_HEADERS,
                KEEPALIVE_ROTATE_URL,
            )

            try:
                await client.post(
                    KEEPALIVE_ROTATE_URL,
                    headers=_KEEPALIVE_ROTATE_HEADERS,
                    content=_KEEPALIVE_ROTATE_BODY,
                )
            except httpx.HTTPError as exc:
                logger.debug("RotateCookies during mint failed (non-fatal): %s", exc)
            jar = httpx.Cookies()
            for cookie in client.cookies.jar:
                jar.jar.set_cookie(cookie)
    except httpx.HTTPError:
        raise MasterTokenError(
            "cookie minting failed (network error reaching accounts.google.com)."
        ) from None  # drop the httpx __cause__ whose URL carries the uberauth

    names = {c.name for c in jar.jar}
    missing = _REQUIRED_MINTED_COOKIES - names
    if missing:
        raise MasterTokenError(
            f"Minted cookie jar is missing required cookies: {sorted(missing)}. "
            "MergeSession may have changed; the session would fail PSIDTS recovery."
        )
    return jar


def storage_state_from_jar(jar: httpx.Cookies, *, email: str | None = None) -> dict[str, Any]:
    """Convert a minted jar to a Playwright ``storage_state`` dict the existing
    loader (``build_httpx_cookies_from_storage``) consumes, including the
    ``notebooklm`` account namespace. Reuses ``_cookie_to_storage_state`` so
    secure/httpOnly/expires and ``__Secure-`` prefixes survive (see #365)."""
    from .cookies import _cookie_to_storage_state  # noqa: PLC0415 (avoid import cycle)

    state: dict[str, Any] = {
        "cookies": [_cookie_to_storage_state(c) for c in jar.jar],
        "origins": [],
    }
    if email is not None:
        # Mirrors _auth/account.write_account_metadata's namespace shape.
        state["notebooklm"] = {"version": 1, "account": {"authuser": 0, "email": email}}
    return state


def persist_minted_jar(path: Path, jar: httpx.Cookies, *, email: str | None) -> None:
    """Replace the cookies in ``storage_state.json`` with a freshly-minted jar,
    preserving existing CLI context (notebook_id/conversation_id) and refreshing
    the account namespace. Serialized on the shared storage lock so it never
    tears against a running keepalive. Old cookies are *replaced*, not merged —
    a re-mint is a brand-new session."""
    from filelock import FileLock  # noqa: PLC0415 (transitive dep)

    from .paths import _storage_state_lock_path  # noqa: PLC0415 (avoid import cycle)

    path.parent.mkdir(parents=True, exist_ok=True)
    with FileLock(str(_storage_state_lock_path(path)), timeout=10.0):
        data: dict[str, Any] = {}
        if path.exists():
            try:
                loaded = json.loads(path.read_text(encoding="utf-8"))
                data = loaded if isinstance(loaded, dict) else {}
            except json.JSONDecodeError:
                data = {}
        data["cookies"] = storage_state_from_jar(jar)["cookies"]
        data.setdefault("origins", [])
        ns_raw = data.get("notebooklm")
        ns: dict[str, Any] = ns_raw if isinstance(ns_raw, dict) else {}
        ns["version"] = 1
        ns["account"] = {"authuser": 0, **({"email": email} if email else {})}
        data["notebooklm"] = ns
        # 0600: the jar holds live session cookies (SID/SAPISID/__Secure-1PSID…).
        # Hidden .{name}.tmp matches write_master_token so neither leaves a
        # visible *.tmp artifact mid-write.
        tmp = path.with_name(f".{path.name}.tmp")
        fd = os.open(tmp, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
        with os.fdopen(fd, "w", encoding="utf-8") as f:
            json.dump(data, f)
        tmp.replace(path)


# --- master_token.json persistence (mode 0600, beside storage_state.json) ---


def read_master_token(path: Path) -> dict[str, Any] | None:
    """Read a ``master_token.json`` record, or ``None`` if absent. Raises
    :class:`MasterTokenError` on a malformed/old-version file."""
    if not path.exists():
        return None
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        raise MasterTokenError(f"Unreadable master_token.json: {exc}") from exc
    if not isinstance(data, dict):  # e.g. a bare JSON array — avoid .get AttributeError
        raise MasterTokenError("master_token.json is malformed or an unsupported version.")
    required = ("master_token", "email", "android_id")
    if data.get("version") != _MASTER_TOKEN_VERSION or any(not data.get(k) for k in required):
        raise MasterTokenError("master_token.json is malformed or an unsupported version.")
    return data


def write_master_token(path: Path, *, email: str, master_token: str, android_id: str) -> None:
    """Persist a master-token record at mode 0600 (full-account credential)."""
    path.parent.mkdir(parents=True, exist_ok=True)
    payload = {
        "version": _MASTER_TOKEN_VERSION,
        "email": email,
        "android_id": android_id,
        "master_token": master_token,
    }
    # Create the temp at 0600 from the start (don't widen-then-chmod) — this is a
    # full-account credential. umask cannot widen 0600 (it has no group/other bits).
    tmp = path.parent / f".{path.name}.tmp"
    fd = os.open(tmp, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
    with os.fdopen(fd, "w", encoding="utf-8") as f:
        json.dump(payload, f)
    tmp.replace(path)
