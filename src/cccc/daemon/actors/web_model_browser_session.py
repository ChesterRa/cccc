"""Shared projected ChatGPT browser session for ChatGPT Web Model actors.

The Web settings/runtime panels and default browser delivery use the same
daemon-owned browser session. The legacy sidecar path is reserved for custom
delivery commands and recovery compatibility.
"""

from __future__ import annotations

from pathlib import Path
from typing import Any

from ..browser.projected_browser_runtime import ProjectedBrowserSessionManager, _wait_cdp_endpoint
from ...ports.web_model_browser_sidecar import (
    CHATGPT_URL,
    _conversation_url_from_tab,
    _normalize_chatgpt_url,
    chatgpt_browser_profile_dir,
    close_chatgpt_browser_session,
    read_chatgpt_browser_process_state,
    read_chatgpt_browser_state,
    record_chatgpt_browser_process_state,
    record_chatgpt_browser_state,
    reset_chatgpt_browser_actor_runtime_state,
)
from .web_model_tool_confirm_watcher import (
    ensure_web_model_tool_confirm_watcher,
    stop_all_web_model_tool_confirm_watchers,
    stop_web_model_tool_confirm_watcher,
)

_MANAGER = ProjectedBrowserSessionManager(
    idle_message="No projected ChatGPT browser session is active.",
)
_CHANNEL_CANDIDATES = ("chrome", "msedge")
_GLOBAL_SESSION_KEY = "chatgpt_web"


def _session_key(group_id: str, actor_id: str) -> str:
    _ = (group_id, actor_id)
    return _GLOBAL_SESSION_KEY


def _metadata(snapshot: dict[str, Any] | None) -> dict[str, Any]:
    raw = (snapshot or {}).get("metadata")
    return dict(raw) if isinstance(raw, dict) else {}


def _record_sidecar_state(group_id: str, actor_id: str, snapshot: dict[str, Any]) -> None:
    meta = _metadata(snapshot)
    cdp_port = int(meta.get("cdp_port") or 0)
    if cdp_port <= 0:
        return
    profile_dir = chatgpt_browser_profile_dir(group_id, actor_id)
    process_update = {
        "pid": int(meta.get("pid") or 0),
        "cdp_port": cdp_port,
        "browser_binary": str(meta.get("browser_binary") or ""),
        "profile_dir": str(profile_dir),
        "visibility": "projected",
        "started_at": str(meta.get("started_at") or snapshot.get("started_at") or ""),
        "last_tab_url": str(snapshot.get("url") or CHATGPT_URL),
    }
    record_chatgpt_browser_process_state(process_update)
    record_chatgpt_browser_state(
        group_id,
        actor_id,
        {
            "last_tab_url": str(snapshot.get("url") or CHATGPT_URL),
        },
    )


def _clear_sidecar_state_if_matching(group_id: str, actor_id: str, snapshot: dict[str, Any]) -> None:
    meta = _metadata(snapshot)
    cdp_port = int(meta.get("cdp_port") or 0)
    current = read_chatgpt_browser_process_state()
    actor_state = read_chatgpt_browser_state(group_id, actor_id)
    current_port = int(current.get("cdp_port") or 0)
    current_visibility = str(current.get("visibility") or "").strip().lower()
    if cdp_port > 0 and current_port not in {0, cdp_port} and current_visibility != "projected":
        return
    record_chatgpt_browser_process_state(
        {
            "pid": 0,
            "cdp_port": 0,
            "visibility": "projected",
            "last_tab_url": str(snapshot.get("url") or current.get("last_tab_url") or CHATGPT_URL),
        }
    )
    record_chatgpt_browser_state(
        group_id,
        actor_id,
        {
            "last_tab_url": str(snapshot.get("url") or actor_state.get("last_tab_url") or current.get("last_tab_url") or CHATGPT_URL),
        },
    )


def _close_prior_browser_state(group_id: str, actor_id: str) -> None:
    try:
        close_chatgpt_browser_session(group_id, actor_id)
    except Exception:
        pass


def _open_url_for_actor(group_id: str, actor_id: str) -> str:
    state = read_chatgpt_browser_state(group_id, actor_id)
    if bool(state.get("pending_new_chat_bind")):
        return _normalize_chatgpt_url(state.get("pending_new_chat_url")) or CHATGPT_URL
    conversation_url = _conversation_url_from_tab(state.get("conversation_url"))
    if conversation_url:
        return conversation_url
    last_conversation_url = _conversation_url_from_tab(state.get("last_tab_url"))
    if last_conversation_url:
        return last_conversation_url
    return CHATGPT_URL


def _same_path(left: str | Path, right: str | Path) -> bool:
    try:
        return Path(left).expanduser().resolve() == Path(right).expanduser().resolve()
    except Exception:
        return str(left or "").strip() == str(right or "").strip()


def _adoptable_shared_browser_state(group_id: str, actor_id: str) -> dict[str, Any]:
    state = read_chatgpt_browser_process_state()
    port = int(state.get("cdp_port") or 0)
    if port <= 0:
        return {}
    expected_profile = chatgpt_browser_profile_dir(group_id, actor_id)
    recorded_profile = str(state.get("profile_dir") or "").strip()
    if recorded_profile and not _same_path(recorded_profile, expected_profile):
        return {}
    if not _wait_cdp_endpoint(port, timeout_seconds=0.7):
        return {}
    return {**state, "profile_dir": str(expected_profile), "cdp_port": port}


def open_web_model_chatgpt_browser_session(
    *,
    group_id: str,
    actor_id: str,
    width: int,
    height: int,
) -> dict[str, Any]:
    existing = _MANAGER.info(key=_session_key(group_id, actor_id))
    if bool(existing.get("active")) and str(existing.get("state") or "").strip() in {"starting", "ready"}:
        _record_sidecar_state(group_id, actor_id, existing)
        ensure_web_model_tool_confirm_watcher(group_id, actor_id)
        return existing

    profile_dir = chatgpt_browser_profile_dir(group_id, actor_id)
    start_url = _open_url_for_actor(group_id, actor_id)
    adopt_state = _adoptable_shared_browser_state(group_id, actor_id)
    if not adopt_state:
        _close_prior_browser_state(group_id, actor_id)
    state = _MANAGER.open(
        key=_session_key(group_id, actor_id),
        profile_dir=profile_dir,
        url=start_url,
        width=width,
        height=height,
        headless=False,
        channel_candidates=_CHANNEL_CANDIDATES,
        system_profile_subdir="",
        require_system_browser_cdp=True,
        existing_cdp_port=int(adopt_state.get("cdp_port") or 0),
        existing_browser_metadata=adopt_state,
    )
    if adopt_state and str(state.get("state") or "").strip() == "failed":
        _close_prior_browser_state(group_id, actor_id)
        state = _MANAGER.open(
            key=_session_key(group_id, actor_id),
            profile_dir=profile_dir,
            url=start_url,
            width=width,
            height=height,
            headless=False,
            channel_candidates=_CHANNEL_CANDIDATES,
            system_profile_subdir="",
            require_system_browser_cdp=True,
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


def submit_prompt_via_web_model_chatgpt_browser_session(
    *,
    group_id: str,
    actor_id: str,
    prompt: str,
    target_url: str,
    auto_bind_new_chat: bool,
    delivery_id: str,
    timeout_seconds: float,
    input_timeout_seconds: float = 30.0,
    new_chat_bind_timeout_seconds: float = 20.0,
) -> dict[str, Any]:
    surface = open_web_model_chatgpt_browser_session(
        group_id=group_id,
        actor_id=actor_id,
        width=1366,
        height=900,
    )
    state = str(surface.get("state") or "").strip()
    if state not in {"ready", "starting"}:
        message = str(surface.get("message") or "ChatGPT browser session is not ready").strip()
        raise RuntimeError(message or "ChatGPT browser session is not ready")
    result = _MANAGER.execute(
        key=_session_key(group_id, actor_id),
        kind="chatgpt_submit_prompt",
        payload={
            "prompt": str(prompt or ""),
            "target_url": str(target_url or ""),
            "auto_bind_new_chat": bool(auto_bind_new_chat),
            "delivery_id": str(delivery_id or ""),
            "input_timeout_seconds": float(input_timeout_seconds or 30.0),
            "new_chat_bind_timeout_seconds": float(new_chat_bind_timeout_seconds or 20.0),
        },
        timeout=max(5.0, float(timeout_seconds or 120.0)),
    )
    browser = result.get("browser") if isinstance(result.get("browser"), dict) else {}
    tab_url = str(browser.get("tab_url") or browser.get("conversation_url") or target_url or surface.get("url") or CHATGPT_URL)
    record_chatgpt_browser_process_state(
        {
            "last_tab_url": tab_url,
            "cdp_port": int(browser.get("cdp_port") or (_metadata(surface).get("cdp_port") or 0)),
            "pid": int(browser.get("pid") or (_metadata(surface).get("pid") or 0)),
            "profile_dir": str(browser.get("profile_dir") or chatgpt_browser_profile_dir(group_id, actor_id)),
            "visibility": "projected",
        }
    )
    record_chatgpt_browser_state(
        group_id,
        actor_id,
        {
            "last_tab_url": tab_url,
            **({"conversation_url": str(browser.get("conversation_url") or "")} if str(browser.get("conversation_url") or "").strip() else {}),
        },
    )
    browser_surface = get_web_model_chatgpt_browser_session_state(group_id=group_id, actor_id=actor_id)
    return {
        "ok": True,
        "delivery_id": str(delivery_id or browser.get("delivery_id") or ""),
        "browser": browser,
        "browser_surface": browser_surface,
        "transport": "projected_session",
    }


def close_web_model_chatgpt_browser_session(*, group_id: str, actor_id: str) -> dict[str, Any]:
    stop_web_model_tool_confirm_watcher(group_id, actor_id)
    before = _MANAGER.info(key=_session_key(group_id, actor_id))
    result = _MANAGER.close(key=_session_key(group_id, actor_id))
    try:
        close_chatgpt_browser_session(group_id, actor_id)
    except Exception:
        _clear_sidecar_state_if_matching(group_id, actor_id, before)
    return result


def clear_web_model_chatgpt_browser_actor_runtime(*, group_id: str, actor_id: str) -> None:
    """Drop actor binding/delivery state while keeping the global ChatGPT page alive."""
    stop_web_model_tool_confirm_watcher(group_id, actor_id)
    reset_chatgpt_browser_actor_runtime_state(group_id, actor_id)


def close_all_web_model_chatgpt_browser_sessions() -> None:
    stop_all_web_model_tool_confirm_watchers()
    _MANAGER.close_all()


def can_attach_web_model_chatgpt_browser_socket(*, group_id: str, actor_id: str):
    return _MANAGER.can_attach(key=_session_key(group_id, actor_id))


def attach_web_model_chatgpt_browser_socket(*, group_id: str, actor_id: str, sock) -> bool:
    return _MANAGER.attach_socket(key=_session_key(group_id, actor_id), sock=sock)
