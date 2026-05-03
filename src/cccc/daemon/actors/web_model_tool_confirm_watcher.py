"""Low-frequency auto-confirm watcher for ChatGPT web-model tool prompts."""

from __future__ import annotations

import logging
import os
import threading
from datetime import datetime, timedelta, timezone
from typing import Any, Dict, Optional

from ...kernel.group import load_group
from ...kernel.ledger import append_event
from ...ports import web_model_browser_sidecar as browser_sidecar
from ..browser.projected_browser_runtime import _wait_cdp_endpoint, ensure_sync_playwright
from ...util.time import parse_utc_iso, utc_now_iso

_LOG = logging.getLogger("cccc.daemon.web_model.tool_confirm")
_WATCHERS_LOCK = threading.Lock()
_WATCHERS: dict[tuple[str, str], tuple[threading.Thread, threading.Event]] = {}
_DEFAULT_INTERVAL_SECONDS = 5.0
_DEFAULT_INACTIVITY_RELOAD_SECONDS = 40.0
_DEFAULT_RELOAD_COOLDOWN_SECONDS = 45.0
_DEFAULT_RELOAD_WINDOW_SECONDS = 30.0 * 60.0


def web_model_tool_auto_confirm_enabled() -> bool:
    raw = str(os.environ.get("CCCC_WEB_MODEL_AUTO_CONFIRM_TOOLS") or "").strip().lower()
    return raw not in {"0", "false", "no", "off", "disabled"}


def web_model_tool_auto_confirm_interval_seconds() -> float:
    raw = str(os.environ.get("CCCC_WEB_MODEL_AUTO_CONFIRM_INTERVAL_SECONDS") or "").strip()
    try:
        value = float(raw) if raw else _DEFAULT_INTERVAL_SECONDS
    except Exception:
        value = _DEFAULT_INTERVAL_SECONDS
    return max(3.0, min(value, 60.0))


def web_model_browser_auto_reload_enabled() -> bool:
    raw = str(os.environ.get("CCCC_WEB_MODEL_BROWSER_AUTO_RELOAD") or "").strip().lower()
    return raw not in {"0", "false", "no", "off", "disabled"}


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
) -> None:
    """Start the bounded recovery window after a browser delivery succeeds."""

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
    except Exception:
        pass


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


def _record_auto_confirm(group_id: str, actor_id: str, result: Dict[str, Any]) -> None:
    clicked = int(result.get("clicked") or 0)
    if clicked <= 0:
        return
    now = utc_now_iso()
    try:
        current = browser_sidecar.read_chatgpt_browser_state(group_id, actor_id)
    except Exception:
        current = {}
    try:
        total = int(current.get("auto_confirm_total") or 0) + clicked
    except Exception:
        total = clicked
    details = result.get("details") if isinstance(result.get("details"), list) else []
    page_url = str(result.get("page_url") or "").strip()
    browser_sidecar.record_chatgpt_browser_state(
        group_id,
        actor_id,
        {
            "auto_confirm_last_at": now,
            "auto_confirm_last_count": clicked,
            "auto_confirm_total": total,
            "auto_confirm_last_page_url": page_url,
            "auto_confirm_last_details": details[:browser_sidecar.TOOL_CONFIRM_MAX_CLICKS],
        },
    )
    record_web_model_browser_progress(group_id, actor_id, reason="auto_confirm", detail=page_url)
    try:
        group = load_group(group_id)
        if group is not None:
            append_event(
                group.ledger_path,
                kind="web_model.browser_tool_confirm.approved",
                group_id=group.group_id,
                scope_key="",
                by="system",
                data={
                    "actor_id": actor_id,
                    "clicked": clicked,
                    "page_url": page_url,
                    "details": details[:browser_sidecar.TOOL_CONFIRM_MAX_CLICKS],
                    "auto": True,
                },
            )
    except Exception:
        pass


def _find_reload_page(browser: Any, *, target_url: str, preferred_page: Any = None, fallback_page: Any = None) -> Any:
    normalized_target = browser_sidecar._normalize_chatgpt_url(target_url)
    if preferred_page is not None:
        return preferred_page
    if fallback_page is not None:
        return fallback_page
    contexts = list(getattr(browser, "contexts", []) or [])
    context = contexts[0] if contexts else None
    for candidate_context in contexts:
        for page in list(getattr(candidate_context, "pages", []) or []):
            page_url = browser_sidecar._normalize_chatgpt_url(str(getattr(page, "url", "") or ""))
            if normalized_target and page_url == normalized_target:
                return page
    for candidate_context in contexts:
        for page in list(getattr(candidate_context, "pages", []) or []):
            if browser_sidecar._normalize_chatgpt_url(str(getattr(page, "url", "") or "")):
                return page
    if context is not None:
        try:
            return context.new_page()
        except Exception:
            return None
    try:
        return browser.new_context().new_page()
    except Exception:
        return None


def _maybe_reload_stale_chatgpt_page(
    group_id: str,
    actor_id: str,
    *,
    browser: Any,
    target_url: str,
    preferred_page: Any = None,
    fallback_page: Any = None,
) -> Dict[str, Any]:
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

    page = _find_reload_page(browser, target_url=normalized_target, preferred_page=preferred_page, fallback_page=fallback_page)
    if page is None:
        browser_sidecar.record_chatgpt_browser_state(
            group_id,
            actor_id,
            {"auto_reload_last_error": "no_chatgpt_page_available"},
        )
        return {"reloaded": False, "reason": "no_page"}

    before_url = str(getattr(page, "url", "") or "")
    try:
        if browser_sidecar._normalize_chatgpt_url(before_url) != normalized_target:
            page.goto(normalized_target, wait_until="domcontentloaded", timeout=30000)
            action = "goto_target"
        else:
            page.reload(wait_until="domcontentloaded", timeout=30000)
            action = "reload"
        after_url = str(getattr(page, "url", "") or normalized_target)
    except Exception as exc:
        error = str(exc)[:1000]
        browser_sidecar.record_chatgpt_browser_state(
            group_id,
            actor_id,
            {"auto_reload_last_error": error},
        )
        return {"reloaded": False, "reason": "reload_failed", "error": error}

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


def _record_auto_confirm_scan(group_id: str, actor_id: str, result: Dict[str, Any]) -> None:
    clicked = int(result.get("clicked") or 0)
    candidate_count = int(result.get("candidate_count") or 0)
    errors = result.get("errors") if isinstance(result.get("errors"), list) else []
    if clicked <= 0 and candidate_count <= 0 and not errors:
        return
    try:
        browser_sidecar.record_chatgpt_browser_state(
            group_id,
            actor_id,
            {
                "auto_confirm_scan_at": utc_now_iso(),
                "auto_confirm_pages_seen": int(result.get("pages_seen") or 0),
                "auto_confirm_candidate_count": max(0, candidate_count),
                "auto_confirm_last_errors": errors[:browser_sidecar.TOOL_CONFIRM_MAX_CLICKS],
            },
        )
    except Exception:
        pass


def auto_confirm_chatgpt_tool_prompts(group_id: str, actor_id: str) -> Dict[str, Any]:
    state = browser_sidecar.read_chatgpt_browser_state(group_id, actor_id)
    process_state = browser_sidecar.read_chatgpt_browser_process_state()
    port = int(process_state.get("cdp_port") or 0)
    if port <= 0 or not _wait_cdp_endpoint(port, timeout_seconds=0.4):
        return {"browser_active": False, "clicked": 0, "details": []}
    target_url = browser_sidecar._normalize_chatgpt_url(state.get("conversation_url"))
    clicked = 0
    candidate_count = 0
    details: list[dict[str, Any]] = []
    errors: list[dict[str, Any]] = []
    pages_seen = 0
    target_page: Any = None
    fallback_page: Any = None
    reload_result: Dict[str, Any] = {}
    sync_playwright = ensure_sync_playwright()
    with sync_playwright() as pw:
        browser = pw.chromium.connect_over_cdp(f"http://127.0.0.1:{port}")
        contexts = list(getattr(browser, "contexts", []) or [])
        for context in contexts:
            for page in list(getattr(context, "pages", []) or []):
                page_url = str(getattr(page, "url", "") or "")
                if "chatgpt.com" not in page_url:
                    continue
                normalized_page_url = browser_sidecar._normalize_chatgpt_url(page_url)
                if normalized_page_url and fallback_page is None:
                    fallback_page = page
                if target_url and normalized_page_url == target_url and target_page is None:
                    target_page = page
                if target_url and browser_sidecar._normalize_chatgpt_url(page_url) != target_url:
                    continue
                pages_seen += 1
                result = browser_sidecar._auto_confirm_page_tool_prompts(
                    page,
                    max_clicks=max(1, browser_sidecar.TOOL_CONFIRM_MAX_CLICKS - clicked),
                )
                candidate_count += int(result.get("candidate_count") or 0)
                page_clicked = int(result.get("clicked") or 0)
                raw_errors = result.get("errors") if isinstance(result.get("errors"), list) else []
                for error in raw_errors:
                    if isinstance(error, dict):
                        errors.append({**error, "page_url": page_url})
                if page_clicked > 0:
                    clicked += page_clicked
                    raw_details = result.get("details") if isinstance(result.get("details"), list) else []
                    for detail in raw_details:
                        if isinstance(detail, dict):
                            details.append({**detail, "page_url": page_url})
                    if clicked >= browser_sidecar.TOOL_CONFIRM_MAX_CLICKS:
                        break
            if clicked >= browser_sidecar.TOOL_CONFIRM_MAX_CLICKS:
                break
        interim = {
            "browser_active": True,
            "clicked": clicked,
            "candidate_count": candidate_count,
            "details": details[:browser_sidecar.TOOL_CONFIRM_MAX_CLICKS],
            "errors": errors[:browser_sidecar.TOOL_CONFIRM_MAX_CLICKS],
            "pages_seen": pages_seen,
        }
        if details:
            interim["page_url"] = str(details[0].get("page_url") or "")
        _record_auto_confirm(group_id, actor_id, interim)
        reload_result = _maybe_reload_stale_chatgpt_page(
            group_id,
            actor_id,
            browser=browser,
            target_url=target_url,
            preferred_page=target_page,
            fallback_page=fallback_page,
        )
    result = {
        "browser_active": True,
        "clicked": clicked,
        "candidate_count": candidate_count,
        "details": details[:browser_sidecar.TOOL_CONFIRM_MAX_CLICKS],
        "errors": errors[:browser_sidecar.TOOL_CONFIRM_MAX_CLICKS],
        "pages_seen": pages_seen,
    }
    if details:
        result["page_url"] = str(details[0].get("page_url") or "")
    if reload_result:
        result["auto_reload"] = reload_result
    _record_auto_confirm_scan(group_id, actor_id, result)
    return result


def _watcher_worker(group_id: str, actor_id: str, stop_event: threading.Event, *, logger: Optional[logging.Logger] = None) -> None:
    log = logger or _LOG
    key = _key(group_id, actor_id)
    missing_cdp_cycles = 0
    try:
        while not stop_event.is_set():
            try:
                result = auto_confirm_chatgpt_tool_prompts(group_id, actor_id)
                if bool(result.get("browser_active")):
                    missing_cdp_cycles = 0
                else:
                    missing_cdp_cycles += 1
                clicked = int(result.get("clicked") or 0)
                if clicked > 0:
                    log.info("[web-model-tool-confirm] approved group=%s actor=%s clicked=%s", group_id, actor_id, clicked)
            except Exception as exc:
                log.debug("[web-model-tool-confirm] scan failed group=%s actor=%s error=%s", group_id, actor_id, exc)
            if missing_cdp_cycles >= 3:
                break
            if stop_event.wait(web_model_tool_auto_confirm_interval_seconds()):
                break
    finally:
        with _WATCHERS_LOCK:
            current = _WATCHERS.get(key)
            if current and current[1] is stop_event:
                _WATCHERS.pop(key, None)


def ensure_web_model_tool_confirm_watcher(group_id: str, actor_id: str, *, logger: Optional[logging.Logger] = None) -> bool:
    gid, aid = _key(group_id, actor_id)
    if not gid or not aid or not web_model_tool_auto_confirm_enabled():
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
            name=f"cccc-web-model-tool-confirm-{gid}-{aid}",
            daemon=True,
        )
        _WATCHERS[key] = (thread, stop_event)
        thread.start()
        return True


def stop_web_model_tool_confirm_watcher(group_id: str, actor_id: str) -> bool:
    key = _key(group_id, actor_id)
    with _WATCHERS_LOCK:
        current = _WATCHERS.pop(key, None)
    if not current:
        return False
    current[1].set()
    return True


def stop_all_web_model_tool_confirm_watchers() -> None:
    with _WATCHERS_LOCK:
        watchers = list(_WATCHERS.values())
        _WATCHERS.clear()
    for _, stop_event in watchers:
        stop_event.set()
