"""Daemon-local browser-surface runtime for Presentation slots.

Presentation keeps the lifecycle policy, while the shared browser mechanics live
in ``cccc.daemon.browser.projected_browser_runtime``.
"""

from __future__ import annotations

from pathlib import Path

from ...paths import ensure_home
from ..browser.projected_browser_runtime import (
    ProjectedBrowserSessionManager,
    ensure_dir,
    launch_projected_browser_runtime as _shared_launch_projected_browser_runtime,
    reset_dir,
)

_MANAGER = ProjectedBrowserSessionManager(
    idle_message="No browser surface session is active for this slot.",
)
_CHANNEL_CANDIDATES = ("chrome", "msedge", None)


def _safe_group_token(group_id: str) -> str:
    raw = str(group_id or "").strip()
    cleaned = "".join(ch if ch.isalnum() or ch in {"-", "_"} else "_" for ch in raw)
    cleaned = cleaned.strip("_")
    return cleaned[:96] or "group"


def _safe_slot_token(slot_id: str) -> str:
    raw = str(slot_id or "").strip().lower().replace("_", "-")
    cleaned = "".join(ch if ch.isalnum() or ch in {"-", "_"} else "_" for ch in raw)
    cleaned = cleaned.strip("_")
    return cleaned[:96] or "slot"


def _browser_profile_root(group_id: str, slot_id: str) -> Path:
    return (
        ensure_home()
        / "state"
        / "presentation_browser"
        / _safe_group_token(group_id)
        / _safe_slot_token(slot_id)
    )


def _browser_profile_dir(group_id: str, slot_id: str) -> Path:
    root = _browser_profile_root(group_id, slot_id) / "profile"
    ensure_dir(root, 0o700)
    return root


def _reset_browser_profile_dir(group_id: str, slot_id: str) -> None:
    reset_dir(_browser_profile_root(group_id, slot_id))


def _session_key(group_id: str, slot_id: str) -> str:
    return f"{str(group_id or '').strip()}::{str(slot_id or '').strip()}"


def _launch_browser_surface_runtime(*, group_id: str, slot_id: str, url: str, width: int, height: int):
    return _shared_launch_projected_browser_runtime(
        profile_dir=_browser_profile_dir(group_id, slot_id),
        url=url,
        width=width,
        height=height,
        headless=False,
        channel_candidates=_CHANNEL_CANDIDATES,
    )


def open_browser_surface_session(*, group_id: str, slot_id: str, url: str, width: int, height: int) -> dict[str, object]:
    _ = _MANAGER.close(key=_session_key(group_id, slot_id))
    _reset_browser_profile_dir(group_id, slot_id)
    return _MANAGER.open(
        key=_session_key(group_id, slot_id),
        profile_dir=_browser_profile_dir(group_id, slot_id),
        url=url,
        width=width,
        height=height,
        headless=False,
        channel_candidates=_CHANNEL_CANDIDATES,
    )


def get_browser_surface_session_state(*, group_id: str, slot_id: str) -> dict[str, object]:
    return _MANAGER.info(key=_session_key(group_id, slot_id))


def close_browser_surface_session(*, group_id: str, slot_id: str) -> dict[str, object]:
    return _MANAGER.close(key=_session_key(group_id, slot_id))


def close_all_browser_surface_sessions() -> None:
    _MANAGER.close_all()


def can_attach_browser_surface_socket(*, group_id: str, slot_id: str):
    return _MANAGER.can_attach(key=_session_key(group_id, slot_id))


def attach_browser_surface_socket(*, group_id: str, slot_id: str, sock) -> bool:
    return _MANAGER.attach_socket(key=_session_key(group_id, slot_id), sock=sock)


def attach_browser_surface_socket_with_mode(*, group_id: str, slot_id: str, sock, viewer_mode: str = "auto") -> bool:
    return _MANAGER.attach_socket_with_mode(key=_session_key(group_id, slot_id), sock=sock, viewer_mode=viewer_mode)


def can_attach_browser_surface_vnc_socket(*, group_id: str, slot_id: str):
    return _MANAGER.can_attach_vnc(key=_session_key(group_id, slot_id))


def attach_browser_surface_vnc_socket(*, group_id: str, slot_id: str, sock) -> bool:
    return _MANAGER.attach_vnc_socket(key=_session_key(group_id, slot_id), sock=sock)
