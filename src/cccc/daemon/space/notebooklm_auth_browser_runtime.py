"""Projected browser session used by the NotebookLM auth flow."""

from __future__ import annotations

from pathlib import Path
from typing import Any

from ..browser.projected_browser_runtime import ProjectedBrowserSessionManager

_MANAGER = ProjectedBrowserSessionManager(
    idle_message="No projected NotebookLM auth browser session is active.",
)
_SESSION_KEY = "notebooklm"
_CHANNEL_CANDIDATES = ("chrome", "msedge")
_GOOGLE_COOKIE_URLS = (
    "https://notebooklm.google.com",
    "https://accounts.google.com",
    "https://www.google.com",
)


def open_notebooklm_auth_browser_session(
    *,
    profile_dir: Path,
    url: str,
    width: int,
    height: int,
    seed_storage_state: dict[str, Any] | None = None,
) -> dict[str, Any]:
    return _MANAGER.open(
        key=_SESSION_KEY,
        profile_dir=profile_dir,
        url=url,
        width=width,
        height=height,
        headless=False,
        channel_candidates=_CHANNEL_CANDIDATES,
        seed_storage_state=seed_storage_state,
        require_system_browser_cdp=True,
    )


def get_notebooklm_auth_browser_session_state() -> dict[str, Any]:
    return _MANAGER.info(key=_SESSION_KEY)


def close_notebooklm_auth_browser_session() -> dict[str, Any]:
    return _MANAGER.close(key=_SESSION_KEY)


def close_all_notebooklm_auth_browser_sessions() -> None:
    _MANAGER.close_all()


def can_attach_notebooklm_auth_browser_socket() -> tuple[bool, dict[str, Any]]:
    return _MANAGER.can_attach(key=_SESSION_KEY)


def attach_notebooklm_auth_browser_socket(*, sock) -> bool:
    return _MANAGER.attach_socket(key=_SESSION_KEY, sock=sock)


def notebooklm_auth_browser_page_urls() -> list[str]:
    result = _MANAGER.execute(key=_SESSION_KEY, kind="inspect_page_urls", payload={}, timeout=5.0)
    raw = result.get("page_urls")
    return [str(item or "").strip() for item in raw] if isinstance(raw, list) else []


def notebooklm_auth_browser_storage_state() -> dict[str, Any]:
    result = _MANAGER.execute(key=_SESSION_KEY, kind="inspect_storage_state", payload={}, timeout=5.0)
    raw = result.get("storage_state")
    return dict(raw) if isinstance(raw, dict) else {}


def notebooklm_auth_browser_google_cookies() -> list[dict[str, Any]]:
    result = _MANAGER.execute(
        key=_SESSION_KEY,
        kind="inspect_cookies",
        payload={"urls": list(_GOOGLE_COOKIE_URLS)},
        timeout=5.0,
    )
    raw = result.get("cookies")
    return [item for item in raw if isinstance(item, dict)] if isinstance(raw, list) else []
