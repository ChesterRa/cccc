"""Optional browser recovery watcher for ChatGPT Web Model deliveries."""

from __future__ import annotations

import logging
import os
import threading
from datetime import datetime, timedelta, timezone
from typing import Any, Dict, Optional

from ...kernel.group import load_group
from ...kernel.ledger import append_event
from ...ports import web_model_browser_sidecar as browser_sidecar
from ...util.time import parse_utc_iso, utc_now_iso
from ..browser.projected_browser_runtime import _wait_cdp_endpoint

_LOG = logging.getLogger("cccc.daemon.web_model.browser_recovery")
_WATCHERS_LOCK = threading.Lock()
_WATCHERS: dict[tuple[str, str], tuple[threading.Thread, threading.Event]] = {}
_DEFAULT_RECOVERY_INTERVAL_SECONDS = 5.0
_DEFAULT_INACTIVITY_RELOAD_SECONDS = 60.0
_DEFAULT_RELOAD_COOLDOWN_SECONDS = 45.0
_DEFAULT_RELOAD_WINDOW_SECONDS = 30.0 * 60.0
_DELIVERY_SUBMITTING_RELOAD_GRACE_SECONDS = 180.0


def _truthy_env(name: str) -> bool:
    raw = str(os.environ.get(name) or "").strip().lower()
    return raw in {"1", "true", "yes", "on", "enabled"}


def web_model_browser_auto_reload_enabled() -> bool:
    return _truthy_env("CCCC_WEB_MODEL_BROWSER_AUTO_RELOAD")


def web_model_browser_recovery_interval_seconds() -> float:
    raw = str(os.environ.get("CCCC_WEB_MODEL_BROWSER_RECOVERY_INTERVAL_SECONDS") or "").strip()
    try:
        value = float(raw) if raw else _DEFAULT_RECOVERY_INTERVAL_SECONDS
    except Exception:
        value = _DEFAULT_RECOVERY_INTERVAL_SECONDS
    return max(3.0, min(value, 60.0))


def web_model_browser_auto_reload_inactivity_seconds() -> float:
    raw = str(os.environ.get("CCCC_WEB_MODEL_BROWSER_AUTO_RELOAD_INACTIVITY_SECONDS") or "").strip()
    try:
        value = float(raw) if raw else _DEFAULT_INACTIVITY_RELOAD_SECONDS
    except Exception:
        value = _DEFAULT_INACTIVITY_RELOAD_SECONDS
    return max(10.0, min(value, 600.0))


def web_model_browser_auto_reload_cooldown_seconds() -> float:
    raw = str(os.environ.get("CCCC_WEB_MODEL_BROWSER_AUTO_RELOAD_COOLDOWN_SECONDS") or "").strip()
    try:
        value = float(raw) if raw else _DEFAULT_RELOAD_COOLDOWN_SECONDS
    except Exception:
        value = _DEFAULT_RELOAD_COOLDOWN_SECONDS
    return max(10.0, min(value, 600.0))


def web_model_browser_auto_reload_window_seconds() -> float:
    raw = str(os.environ.get("CCCC_WEB_MODEL_BROWSER_AUTO_RELOAD_WINDOW_SECONDS") or "").strip()
    try:
        value = float(raw) if raw else _DEFAULT_RELOAD_WINDOW_SECONDS
    except Exception:
        value = _DEFAULT_RELOAD_WINDOW_SECONDS
    return max(60.0, min(value, 7200.0))


def _iso_from_dt(value: datetime) -> str:
    return value.astimezone(timezone.utc).isoformat().replace("+00:00", "Z")


def _now_dt() -> datetime:
    return datetime.now(timezone.utc)


def _state_dt(state: Dict[str, Any], key: str) -> Optional[datetime]:
    return parse_utc_iso(str(state.get(key) or ""))


def _delivery_submit_recent(state: Dict[str, Any], *, now_dt: Optional[datetime] = None) -> bool:
    if str(state.get("last_delivery_status") or "").strip().lower() != "submitting":
        return False
    active_now = now_dt or _now_dt()
    started = _state_dt(state, "last_delivery_started_at") or _state_dt(state, "last_delivery_at") or active_now
    return (active_now - started).total_seconds() < _DELIVERY_SUBMITTING_RELOAD_GRACE_SECONDS


def _append_auto_reload_event(group_id: str, actor_id: str, *, kind: str, data: Dict[str, Any]) -> None:
    try:
        group = load_group(group_id)
        if group is None:
            return
        append_event(
            group.ledger_path,
            kind=kind,
            group_id=group.group_id,
            scope_key="",
            by="system",
            data={"actor_id": actor_id, **dict(data or {})},
        )
    except Exception:
        pass


def start_web_model_browser_reload_window(
    group_id: str,
    actor_id: str,
    *,
    reason: str = "browser_delivery",
    delivery_id: str = "",
    turn_id: str = "",
    event_ids: Optional[list[str]] = None,
    target_url: str = "",
) -> bool:
    """Start a bounded auto-reload recovery window when the feature is enabled."""

    if not web_model_browser_auto_reload_enabled():
        try:
            browser_sidecar.record_chatgpt_browser_state(
                group_id,
                actor_id,
                {
                    "auto_reload_active": False,
                    "auto_reload_completed_reason": "disabled",
                    "auto_reload_last_error": "",
                },
            )
        except Exception:
            pass
        return False

    now_dt = _now_dt()
    now = _iso_from_dt(now_dt)
    expires_at = _iso_from_dt(now_dt + timedelta(seconds=web_model_browser_auto_reload_window_seconds()))
    try:
        browser_sidecar.record_chatgpt_browser_state(
            group_id,
            actor_id,
            {
                "auto_reload_active": True,
                "auto_reload_window_started_at": now,
                "auto_reload_window_expires_at": expires_at,
                "auto_reload_last_progress_at": now,
                "auto_reload_last_progress_reason": str(reason or "browser_delivery"),
                "auto_reload_last_progress_detail": str(delivery_id or turn_id or "").strip(),
                "auto_reload_last_delivery_id": str(delivery_id or "").strip(),
                "auto_reload_last_turn_id": str(turn_id or "").strip(),
                "auto_reload_last_event_ids": list(event_ids or []),
                "auto_reload_target_url": str(target_url or "").strip(),
                "auto_reload_completed_at": "",
                "auto_reload_completed_reason": "",
                "auto_reload_expired_at": "",
                "auto_reload_last_error": "",
            },
        )
        return True
    except Exception:
        return False


def record_web_model_browser_progress(group_id: str, actor_id: str, *, reason: str, detail: str = "") -> bool:
    """Record progress while a reload recovery window is active."""

    try:
        state = browser_sidecar.read_chatgpt_browser_state(group_id, actor_id)
    except Exception:
        return False
    if not bool(state.get("auto_reload_active")):
        return False
    now = utc_now_iso()
    try:
        browser_sidecar.record_chatgpt_browser_state(
            group_id,
            actor_id,
            {
                "auto_reload_last_progress_at": now,
                "auto_reload_last_progress_reason": str(reason or "activity").strip() or "activity",
                "auto_reload_last_progress_detail": str(detail or "").strip()[:200],
                "auto_reload_last_error": "",
            },
        )
        return True
    except Exception:
        return False


def close_web_model_browser_reload_window(group_id: str, actor_id: str, *, reason: str = "complete_turn", detail: str = "") -> bool:
    try:
        state = browser_sidecar.read_chatgpt_browser_state(group_id, actor_id)
    except Exception:
        return False
    if not bool(state.get("auto_reload_active")):
        return False
    try:
        browser_sidecar.record_chatgpt_browser_state(
            group_id,
            actor_id,
            {
                "auto_reload_active": False,
                "auto_reload_completed_at": utc_now_iso(),
                "auto_reload_completed_reason": str(reason or "complete_turn").strip() or "complete_turn",
                "auto_reload_last_progress_detail": str(detail or "").strip()[:200],
            },
        )
        return True
    except Exception:
        return False


def _key(group_id: str, actor_id: str) -> tuple[str, str]:
    return (str(group_id or "").strip(), str(actor_id or "").strip())


def _active_cdp_port(group_id: str, actor_id: str) -> int:
    _ = (group_id, actor_id)
    try:
        state = browser_sidecar.read_chatgpt_browser_process_state()
    except Exception:
        return 0
    try:
        port = int(state.get("cdp_port") or 0)
    except Exception:
        return 0
    if port <= 0:
        return 0
    return port if _wait_cdp_endpoint(port, timeout_seconds=0.4) else 0


def _reload_chatgpt_projected_session(group_id: str, actor_id: str, *, target_url: str) -> Dict[str, Any]:
    from .web_model_browser_session import reload_web_model_chatgpt_browser_session

    return reload_web_model_chatgpt_browser_session(group_id=group_id, actor_id=actor_id, target_url=target_url)


def _maybe_reload_stale_chatgpt_page(group_id: str, actor_id: str, *, target_url: str) -> Dict[str, Any]:
    if not web_model_browser_auto_reload_enabled():
        return {"reloaded": False, "reason": "disabled"}
    try:
        state = browser_sidecar.read_chatgpt_browser_state(group_id, actor_id)
    except Exception as exc:
        return {"reloaded": False, "reason": "state_unavailable", "error": str(exc)[:300]}
    normalized_target = browser_sidecar._normalize_chatgpt_url(target_url) or browser_sidecar._normalize_chatgpt_url(
        state.get("auto_reload_target_url")
    )
    if not normalized_target:
        return {"reloaded": False, "reason": "no_target_url"}
    if not bool(state.get("auto_reload_active")):
        return {"reloaded": False, "reason": "inactive"}
    now_dt = _now_dt()
    if _delivery_submit_recent(state, now_dt=now_dt):
        return {"reloaded": False, "reason": "delivery_submitting"}
    started_at = _state_dt(state, "auto_reload_window_started_at") or now_dt
    expires_at = _state_dt(state, "auto_reload_window_expires_at") or (
        started_at + timedelta(seconds=web_model_browser_auto_reload_window_seconds())
    )
    if now_dt >= expires_at:
        browser_sidecar.record_chatgpt_browser_state(
            group_id,
            actor_id,
            {
                "auto_reload_active": False,
                "auto_reload_expired_at": _iso_from_dt(now_dt),
                "auto_reload_completed_reason": "window_expired",
            },
        )
        _append_auto_reload_event(
            group_id,
            actor_id,
            kind="web_model.browser_auto_reload.expired",
            data={
                "target_url": normalized_target,
                "window_started_at": str(state.get("auto_reload_window_started_at") or ""),
                "window_expires_at": _iso_from_dt(expires_at),
            },
        )
        return {"reloaded": False, "reason": "window_expired"}

    last_progress = _state_dt(state, "auto_reload_last_progress_at") or started_at
    inactivity_seconds = web_model_browser_auto_reload_inactivity_seconds()
    if (now_dt - last_progress).total_seconds() < inactivity_seconds:
        return {"reloaded": False, "reason": "recent_progress"}
    last_reload = _state_dt(state, "auto_reload_last_reload_at")
    cooldown_seconds = web_model_browser_auto_reload_cooldown_seconds()
    if last_reload is not None and (now_dt - last_reload).total_seconds() < cooldown_seconds:
        return {"reloaded": False, "reason": "cooldown"}

    try:
        reload_result = _reload_chatgpt_projected_session(group_id, actor_id, target_url=normalized_target)
    except Exception as exc:
        error = str(exc)[:1000]
        browser_sidecar.record_chatgpt_browser_state(
            group_id,
            actor_id,
            {"auto_reload_last_error": error},
        )
        return {"reloaded": False, "reason": "reload_failed", "error": error}
    if not bool(reload_result.get("ok")):
        error = str(reload_result.get("error") or "projected_session_reload_failed")[:1000]
        browser_sidecar.record_chatgpt_browser_state(
            group_id,
            actor_id,
            {"auto_reload_last_error": error},
        )
        return {"reloaded": False, "reason": str(reload_result.get("error") or "reload_failed"), "error": error}
    action = str(reload_result.get("action") or "reload")
    before_url = str(reload_result.get("before_url") or "")
    after_url = str(reload_result.get("after_url") or normalized_target)

    try:
        current = browser_sidecar.read_chatgpt_browser_state(group_id, actor_id)
    except Exception:
        current = {}
    try:
        count = int(current.get("auto_reload_count") or 0) + 1
    except Exception:
        count = 1
    now = _iso_from_dt(now_dt)
    browser_sidecar.record_chatgpt_browser_state(
        group_id,
        actor_id,
        {
            "auto_reload_count": count,
            "auto_reload_last_reload_at": now,
            "auto_reload_last_reload_reason": "no_progress_timeout",
            "auto_reload_last_page_url": after_url,
            "auto_reload_last_progress_at": now,
            "auto_reload_last_progress_reason": "auto_reload",
            "auto_reload_last_progress_detail": action,
            "last_tab_url": after_url,
            "auto_reload_last_error": "",
        },
    )
    _append_auto_reload_event(
        group_id,
        actor_id,
        kind="web_model.browser_auto_reload.reloaded",
        data={
            "target_url": normalized_target,
            "before_url": before_url,
            "after_url": after_url,
            "action": action,
            "inactivity_seconds": inactivity_seconds,
            "cooldown_seconds": cooldown_seconds,
            "reload_count": count,
        },
    )
    return {"reloaded": True, "action": action, "before_url": before_url, "after_url": after_url, "reload_count": count}


def _watcher_worker(group_id: str, actor_id: str, stop_event: threading.Event, *, logger: Optional[logging.Logger] = None) -> None:
    log = logger or _LOG
    key = _key(group_id, actor_id)
    missing_cdp_cycles = 0
    try:
        while not stop_event.is_set():
            try:
                if _active_cdp_port(group_id, actor_id) > 0:
                    missing_cdp_cycles = 0
                else:
                    missing_cdp_cycles += 1
                result = _maybe_reload_stale_chatgpt_page(group_id, actor_id, target_url="")
                if bool(result.get("reloaded")):
                    log.info("[web-model-browser-recovery] reloaded group=%s actor=%s action=%s", group_id, actor_id, result.get("action"))
            except Exception as exc:
                log.debug("[web-model-browser-recovery] scan failed group=%s actor=%s error=%s", group_id, actor_id, exc)
            if missing_cdp_cycles >= 3:
                break
            if stop_event.wait(web_model_browser_recovery_interval_seconds()):
                break
    finally:
        with _WATCHERS_LOCK:
            current = _WATCHERS.get(key)
            if current and current[1] is stop_event:
                _WATCHERS.pop(key, None)


def ensure_web_model_browser_recovery_watcher(group_id: str, actor_id: str, *, logger: Optional[logging.Logger] = None) -> bool:
    gid, aid = _key(group_id, actor_id)
    if not gid or not aid or not web_model_browser_auto_reload_enabled():
        return False
    if _active_cdp_port(gid, aid) <= 0:
        return False
    key = (gid, aid)
    with _WATCHERS_LOCK:
        current = _WATCHERS.get(key)
        if current and current[0].is_alive():
            return False
        stop_event = threading.Event()
        thread = threading.Thread(
            target=_watcher_worker,
            args=(gid, aid, stop_event),
            kwargs={"logger": logger},
            name=f"cccc-web-model-browser-recovery-{gid}-{aid}",
            daemon=True,
        )
        _WATCHERS[key] = (thread, stop_event)
        thread.start()
        return True


def stop_web_model_browser_recovery_watcher(group_id: str, actor_id: str) -> bool:
    key = _key(group_id, actor_id)
    with _WATCHERS_LOCK:
        current = _WATCHERS.pop(key, None)
    if not current:
        return False
    current[1].set()
    return True


def stop_all_web_model_browser_recovery_watchers() -> None:
    with _WATCHERS_LOCK:
        watchers = list(_WATCHERS.values())
        _WATCHERS.clear()
    for _, stop_event in watchers:
        stop_event.set()
