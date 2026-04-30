"""Projected ChatGPT browser session for web_model actors.

This owns the interactive browser surface used from the Web settings panel.
The delivery sidecar reuses the same Chrome profile and, when the projected
browser is already open, the same CDP endpoint.
"""

from __future__ import annotations

from typing import Any

from ..browser.projected_browser_runtime import ProjectedBrowserSessionManager
from ...ports.web_model_browser_sidecar import (
    CHATGPT_URL,
    chatgpt_browser_profile_dir,
    close_chatgpt_browser_session,
    read_chatgpt_browser_state,
    record_chatgpt_browser_state,
)
from .web_model_tool_confirm_watcher import (
    ensure_web_model_tool_confirm_watcher,
    stop_all_web_model_tool_confirm_watchers,
    stop_web_model_tool_confirm_watcher,
)

_MANAGER = ProjectedBrowserSessionManager(
    idle_message="No projected ChatGPT browser session is active.",
)
_CHANNEL_CANDIDATES = ("chrome", "msedge", None)


def _session_key(group_id: str, actor_id: str) -> str:
    return f"{str(group_id or '').strip()}::{str(actor_id or '').strip()}"


def _metadata(snapshot: dict[str, Any] | None) -> dict[str, Any]:
    raw = (snapshot or {}).get("metadata")
    return dict(raw) if isinstance(raw, dict) else {}


def _record_sidecar_state(group_id: str, actor_id: str, snapshot: dict[str, Any]) -> None:
    meta = _metadata(snapshot)
    cdp_port = int(meta.get("cdp_port") or 0)
    if cdp_port <= 0:
        return
    profile_dir = chatgpt_browser_profile_dir(group_id, actor_id)
    record_chatgpt_browser_state(
        group_id,
        actor_id,
        {
            "pid": int(meta.get("pid") or 0),
            "cdp_port": cdp_port,
            "browser_binary": str(meta.get("browser_binary") or ""),
            "profile_dir": str(profile_dir),
            "visibility": "projected",
            "started_at": str(snapshot.get("started_at") or ""),
            "last_tab_url": str(snapshot.get("url") or CHATGPT_URL),
        },
    )


def _clear_sidecar_state_if_matching(group_id: str, actor_id: str, snapshot: dict[str, Any]) -> None:
    meta = _metadata(snapshot)
    cdp_port = int(meta.get("cdp_port") or 0)
    current = read_chatgpt_browser_state(group_id, actor_id)
    current_port = int(current.get("cdp_port") or 0)
    current_visibility = str(current.get("visibility") or "").strip().lower()
    if cdp_port > 0 and current_port not in {0, cdp_port} and current_visibility != "projected":
        return
    record_chatgpt_browser_state(
        group_id,
        actor_id,
        {
            "pid": 0,
            "cdp_port": 0,
            "visibility": "projected",
            "last_tab_url": str(snapshot.get("url") or current.get("last_tab_url") or CHATGPT_URL),
        },
    )


def _close_prior_browser_state(group_id: str, actor_id: str) -> None:
    try:
        close_chatgpt_browser_session(group_id, actor_id)
    except Exception:
        pass


def open_web_model_chatgpt_browser_session(
    *,
    group_id: str,
    actor_id: str,
    width: int,
    height: int,
) -> dict[str, Any]:
    profile_dir = chatgpt_browser_profile_dir(group_id, actor_id)
    _ = _MANAGER.close(key=_session_key(group_id, actor_id))
    _close_prior_browser_state(group_id, actor_id)
    state = _MANAGER.open(
        key=_session_key(group_id, actor_id),
        profile_dir=profile_dir,
        url=CHATGPT_URL,
        width=width,
        height=height,
        headless=False,
        channel_candidates=_CHANNEL_CANDIDATES,
        system_profile_subdir="",
    )
    _record_sidecar_state(group_id, actor_id, state)
    ensure_web_model_tool_confirm_watcher(group_id, actor_id)
    return state


def get_web_model_chatgpt_browser_session_state(*, group_id: str, actor_id: str) -> dict[str, Any]:
    state = _MANAGER.info(key=_session_key(group_id, actor_id))
    if bool(state.get("active")):
        _record_sidecar_state(group_id, actor_id, state)
        ensure_web_model_tool_confirm_watcher(group_id, actor_id)
    return state


def close_web_model_chatgpt_browser_session(*, group_id: str, actor_id: str) -> dict[str, Any]:
    stop_web_model_tool_confirm_watcher(group_id, actor_id)
    before = _MANAGER.info(key=_session_key(group_id, actor_id))
    result = _MANAGER.close(key=_session_key(group_id, actor_id))
    try:
        close_chatgpt_browser_session(group_id, actor_id)
    except Exception:
        _clear_sidecar_state_if_matching(group_id, actor_id, before)
    return result


def close_all_web_model_chatgpt_browser_sessions() -> None:
    stop_all_web_model_tool_confirm_watchers()
    _MANAGER.close_all()


def can_attach_web_model_chatgpt_browser_socket(*, group_id: str, actor_id: str):
    return _MANAGER.can_attach(key=_session_key(group_id, actor_id))


def attach_web_model_chatgpt_browser_socket(*, group_id: str, actor_id: str, sock) -> bool:
    return _MANAGER.attach_socket(key=_session_key(group_id, actor_id), sock=sock)
