"""Low-frequency auto-confirm watcher for ChatGPT web-model tool prompts."""

from __future__ import annotations

import logging
import os
import threading
from typing import Any, Dict, Optional

from ...kernel.group import load_group
from ...kernel.ledger import append_event
from ...ports import web_model_browser_sidecar as browser_sidecar
from ..browser.projected_browser_runtime import _wait_cdp_endpoint, ensure_sync_playwright
from ...util.time import utc_now_iso

_LOG = logging.getLogger("cccc.daemon.web_model.tool_confirm")
_WATCHERS_LOCK = threading.Lock()
_WATCHERS: dict[tuple[str, str], tuple[threading.Thread, threading.Event]] = {}
_DEFAULT_INTERVAL_SECONDS = 8.0


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
    sync_playwright = ensure_sync_playwright()
    with sync_playwright() as pw:
        browser = pw.chromium.connect_over_cdp(f"http://127.0.0.1:{port}")
        contexts = list(getattr(browser, "contexts", []) or [])
        for context in contexts:
            for page in list(getattr(context, "pages", []) or []):
                page_url = str(getattr(page, "url", "") or "")
                if "chatgpt.com" not in page_url:
                    continue
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
    _record_auto_confirm_scan(group_id, actor_id, result)
    _record_auto_confirm(group_id, actor_id, result)
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
