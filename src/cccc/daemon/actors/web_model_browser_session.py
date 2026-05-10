"""Shared projected ChatGPT browser session for ChatGPT Web Model actors.

The Web settings/runtime panels and default browser delivery use the same
daemon-owned browser session.
"""

from __future__ import annotations

import threading
import time
from pathlib import Path
from typing import Any

from ..browser.projected_browser_runtime import ProjectedBrowserSessionManager, _wait_cdp_endpoint
from ...util.time import parse_utc_iso, utc_now_iso
from ...ports.web_model_browser_sidecar import (
    CHATGPT_URL,
    TOOL_CONFIRM_MAX_CLICKS,
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
_SESSION_WRITE_LOCK = threading.RLock()
_STARTING_STALE_SECONDS = 30.0
_WARMUP_RETRY_SECONDS = 30.0
_WARMUP_LOCK = threading.RLock()
_WARMUP_IN_FLIGHT: set[str] = set()
_WARMUP_LAST_ATTEMPT: dict[str, float] = {}


def _session_key(group_id: str, actor_id: str) -> str:
    _ = (group_id, actor_id)
    return _GLOBAL_SESSION_KEY


def _metadata(snapshot: dict[str, Any] | None) -> dict[str, Any]:
    raw = (snapshot or {}).get("metadata")
    return dict(raw) if isinstance(raw, dict) else {}


def _record_projected_browser_state(group_id: str, actor_id: str, snapshot: dict[str, Any]) -> None:
    meta = _metadata(snapshot)
    snapshot_state = str((snapshot or {}).get("state") or "").strip().lower()
    if snapshot_state == "failed":
        _clear_projected_browser_state_if_matching(group_id, actor_id, snapshot)
        return
    if snapshot_state != "ready":
        return
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


def _clear_projected_browser_state_if_matching(group_id: str, actor_id: str, snapshot: dict[str, Any]) -> None:
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


def _age_seconds(ts: str) -> float | None:
    dt = parse_utc_iso(str(ts or "").strip())
    now_dt = parse_utc_iso(utc_now_iso())
    if dt is None or now_dt is None:
        return None
    return max(0.0, (now_dt - dt).total_seconds())


def _stale_starting_surface(surface: dict[str, Any]) -> bool:
    if str((surface or {}).get("state") or "").strip() != "starting":
        return False
    age = _age_seconds(str((surface or {}).get("started_at") or (surface or {}).get("updated_at") or ""))
    return age is not None and age >= _STARTING_STALE_SECONDS


def _close_stale_starting_surface(group_id: str, actor_id: str, surface: dict[str, Any]) -> dict[str, Any]:
    if not _stale_starting_surface(surface):
        return surface
    try:
        _MANAGER.close(key=_session_key(group_id, actor_id))
    except Exception:
        pass
    try:
        close_chatgpt_browser_session(group_id, actor_id)
    except Exception:
        pass
    return _MANAGER.info(key=_session_key(group_id, actor_id))


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
    existing = _close_stale_starting_surface(group_id, actor_id, existing)
    if bool(existing.get("active")) and str(existing.get("state") or "").strip() in {"starting", "ready"}:
        _record_projected_browser_state(group_id, actor_id, existing)
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
    _record_projected_browser_state(group_id, actor_id, state)
    ensure_web_model_tool_confirm_watcher(group_id, actor_id)
    return state


def get_web_model_chatgpt_browser_session_state(*, group_id: str, actor_id: str) -> dict[str, Any]:
    state = _MANAGER.info(key=_session_key(group_id, actor_id))
    state = _close_stale_starting_surface(group_id, actor_id, state)
    if bool(state.get("active")):
        _record_projected_browser_state(group_id, actor_id, state)
        ensure_web_model_tool_confirm_watcher(group_id, actor_id)
    return state


def schedule_web_model_chatgpt_browser_session_warmup(
    *,
    group_id: str,
    actor_id: str,
    reason: str = "",
    retry_seconds: float = _WARMUP_RETRY_SECONDS,
) -> bool:
    gid = str(group_id or "").strip()
    aid = str(actor_id or "").strip()
    if not gid or not aid:
        return False
    key = _session_key(gid, aid)
    surface = _MANAGER.info(key=key)
    surface = _close_stale_starting_surface(gid, aid, surface)
    if bool(surface.get("active")) and str(surface.get("state") or "").strip() in {"ready", "starting"}:
        _record_projected_browser_state(gid, aid, surface)
        ensure_web_model_tool_confirm_watcher(gid, aid)
        return False
    now = time.monotonic()
    retry_window = max(0.0, float(retry_seconds or 0.0))
    with _WARMUP_LOCK:
        if key in _WARMUP_IN_FLIGHT:
            return False
        last = float(_WARMUP_LAST_ATTEMPT.get(key) or 0.0)
        if retry_window > 0 and now - last < retry_window:
            return False
        _WARMUP_IN_FLIGHT.add(key)
        _WARMUP_LAST_ATTEMPT[key] = now

    def _worker() -> None:
        try:
            with _SESSION_WRITE_LOCK:
                result = open_web_model_chatgpt_browser_session(
                    group_id=gid,
                    actor_id=aid,
                    width=1366,
                    height=900,
                )
            state = str((result or {}).get("state") or "").strip()
            record_chatgpt_browser_state(
                gid,
                aid,
                {
                    "browser_warmup_at": utc_now_iso(),
                    "browser_warmup_reason": str(reason or ""),
                    "browser_warmup_state": state,
                    "browser_warmup_error": "",
                },
            )
        except Exception as exc:
            record_chatgpt_browser_state(
                gid,
                aid,
                {
                    "browser_warmup_at": utc_now_iso(),
                    "browser_warmup_reason": str(reason or ""),
                    "browser_warmup_state": "failed",
                    "browser_warmup_error": str(exc)[:1200],
                },
            )
        finally:
            with _WARMUP_LOCK:
                _WARMUP_IN_FLIGHT.discard(key)

    threading.Thread(
        target=_worker,
        name=f"cccc-chatgpt-browser-warmup-{gid}-{aid}",
        daemon=True,
    ).start()
    return True


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
    with _SESSION_WRITE_LOCK:
        return _submit_prompt_via_web_model_chatgpt_browser_session_locked(
            group_id=group_id,
            actor_id=actor_id,
            prompt=prompt,
            target_url=target_url,
            auto_bind_new_chat=auto_bind_new_chat,
            delivery_id=delivery_id,
            timeout_seconds=timeout_seconds,
            input_timeout_seconds=input_timeout_seconds,
            new_chat_bind_timeout_seconds=new_chat_bind_timeout_seconds,
        )


def _submit_prompt_via_web_model_chatgpt_browser_session_locked(
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
            "command_timeout_seconds": float(timeout_seconds or 120.0),
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


def reload_web_model_chatgpt_browser_session(
    *,
    group_id: str,
    actor_id: str,
    target_url: str = "",
    timeout_seconds: float = 35.0,
) -> dict[str, Any]:
    acquired = _SESSION_WRITE_LOCK.acquire(blocking=False)
    if not acquired:
        return {
            "ok": False,
            "error": "browser_delivery_in_progress",
            "browser_surface": _MANAGER.info(key=_session_key(group_id, actor_id)),
        }
    try:
        return _reload_web_model_chatgpt_browser_session_locked(
            group_id=group_id,
            actor_id=actor_id,
            target_url=target_url,
            timeout_seconds=timeout_seconds,
        )
    finally:
        _SESSION_WRITE_LOCK.release()


def _reload_web_model_chatgpt_browser_session_locked(
    *,
    group_id: str,
    actor_id: str,
    target_url: str = "",
    timeout_seconds: float = 35.0,
) -> dict[str, Any]:
    surface = _MANAGER.info(key=_session_key(group_id, actor_id))
    surface = _close_stale_starting_surface(group_id, actor_id, surface)
    state = str(surface.get("state") or "").strip()
    if not bool(surface.get("active")) or state not in {"ready", "starting"}:
        return {
            "ok": False,
            "error": "projected_session_not_active",
            "browser_surface": surface,
        }
    normalized_target = _normalize_chatgpt_url(target_url)
    before_url = str(surface.get("url") or "")
    if normalized_target and _normalize_chatgpt_url(before_url) != normalized_target:
        kind = "navigate"
        payload = {"url": normalized_target}
        action = "goto_target"
    else:
        kind = "refresh"
        payload = {}
        action = "reload"
    _MANAGER.execute(
        key=_session_key(group_id, actor_id),
        kind=kind,
        payload=payload,
        timeout=max(5.0, float(timeout_seconds or 35.0)),
    )
    after = _MANAGER.info(key=_session_key(group_id, actor_id))
    _record_projected_browser_state(group_id, actor_id, after)
    return {
        "ok": True,
        "action": action,
        "before_url": before_url,
        "after_url": str(after.get("url") or normalized_target or before_url),
        "browser_surface": after,
    }


def auto_confirm_web_model_chatgpt_tool_prompts(
    *,
    group_id: str,
    actor_id: str,
    target_url: str = "",
    max_clicks: int = 3,
    timeout_seconds: float = 12.0,
) -> dict[str, Any]:
    acquired = _SESSION_WRITE_LOCK.acquire(blocking=False)
    if not acquired:
        return {
            "browser_active": True,
            "clicked": 0,
            "candidate_count": 0,
            "details": [],
            "errors": [],
            "pages_seen": 0,
            "skipped": "browser_delivery_in_progress",
            "browser_surface": _MANAGER.info(key=_session_key(group_id, actor_id)),
        }
    try:
        return _auto_confirm_web_model_chatgpt_tool_prompts_locked(
            group_id=group_id,
            actor_id=actor_id,
            target_url=target_url,
            max_clicks=max_clicks,
            timeout_seconds=timeout_seconds,
        )
    finally:
        _SESSION_WRITE_LOCK.release()


def _auto_confirm_web_model_chatgpt_tool_prompts_locked(
    *,
    group_id: str,
    actor_id: str,
    target_url: str = "",
    max_clicks: int = 3,
    timeout_seconds: float = 12.0,
) -> dict[str, Any]:
    surface = _MANAGER.info(key=_session_key(group_id, actor_id))
    surface = _close_stale_starting_surface(group_id, actor_id, surface)
    state = str(surface.get("state") or "").strip()
    if not bool(surface.get("active")) or state not in {"ready", "starting"}:
        return {
            "browser_active": False,
            "clicked": 0,
            "candidate_count": 0,
            "details": [],
            "errors": [],
            "pages_seen": 0,
            "browser_surface": surface,
        }
    normalized_target = _normalize_chatgpt_url(target_url)
    try:
        click_limit = int(max_clicks or TOOL_CONFIRM_MAX_CLICKS)
    except Exception:
        click_limit = TOOL_CONFIRM_MAX_CLICKS
    result = _MANAGER.execute(
        key=_session_key(group_id, actor_id),
        kind="chatgpt_auto_confirm_tools",
        payload={
            "target_url": normalized_target,
            "max_clicks": max(1, min(click_limit, TOOL_CONFIRM_MAX_CLICKS)),
        },
        timeout=max(2.0, float(timeout_seconds or 12.0)),
    )
    after = _MANAGER.info(key=_session_key(group_id, actor_id))
    _record_projected_browser_state(group_id, actor_id, after)
    return {
        "browser_active": True,
        "clicked": max(0, int(result.get("clicked") or 0)),
        "candidate_count": max(0, int(result.get("candidate_count") or 0)),
        "details": result.get("details") if isinstance(result.get("details"), list) else [],
        "errors": result.get("errors") if isinstance(result.get("errors"), list) else [],
        "pages_seen": max(0, int(result.get("pages_seen") or 0)),
        "page_url": str(result.get("page_url") or after.get("url") or ""),
        "skipped": str(result.get("skipped") or ""),
        "browser_surface": after,
    }


def close_web_model_chatgpt_browser_session(*, group_id: str, actor_id: str) -> dict[str, Any]:
    stop_web_model_tool_confirm_watcher(group_id, actor_id)
    before = _MANAGER.info(key=_session_key(group_id, actor_id))
    result = _MANAGER.close(key=_session_key(group_id, actor_id))
    try:
        close_chatgpt_browser_session(group_id, actor_id)
    except Exception:
        _clear_projected_browser_state_if_matching(group_id, actor_id, before)
    return result


def clear_web_model_chatgpt_browser_actor_runtime(*, group_id: str, actor_id: str) -> None:
    """Drop actor binding/delivery state while keeping the global ChatGPT page alive."""
    stop_web_model_tool_confirm_watcher(group_id, actor_id)
    reset_chatgpt_browser_actor_runtime_state(group_id, actor_id)


def close_all_web_model_chatgpt_browser_sessions() -> None:
    stop_all_web_model_tool_confirm_watchers()
    _MANAGER.close_all()


def can_attach_web_model_chatgpt_browser_socket(*, group_id: str, actor_id: str):
    _close_stale_starting_surface(group_id, actor_id, _MANAGER.info(key=_session_key(group_id, actor_id)))
    return _MANAGER.can_attach(key=_session_key(group_id, actor_id))


def attach_web_model_chatgpt_browser_socket(*, group_id: str, actor_id: str, sock, viewer_mode: str = "auto") -> bool:
    return _MANAGER.attach_socket_with_mode(key=_session_key(group_id, actor_id), sock=sock, viewer_mode=viewer_mode)


def can_attach_web_model_chatgpt_browser_vnc_socket(*, group_id: str, actor_id: str):
    _close_stale_starting_surface(group_id, actor_id, _MANAGER.info(key=_session_key(group_id, actor_id)))
    return _MANAGER.can_attach_vnc(key=_session_key(group_id, actor_id))


def attach_web_model_chatgpt_browser_vnc_socket(*, group_id: str, actor_id: str, sock) -> bool:
    return _MANAGER.attach_vnc_socket(key=_session_key(group_id, actor_id), sock=sock)
