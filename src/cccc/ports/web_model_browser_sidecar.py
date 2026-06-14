"""ChatGPT page automation helpers for CCCC Web Model actors.

The daemon-owned projected browser session is the only browser delivery writer.
This module keeps shared URL/state helpers and page-level ChatGPT automation used
by that session.
"""

from __future__ import annotations

import json
import os
import re
import shutil
import subprocess
import sys
import time
from pathlib import Path
from typing import Any
from urllib.parse import urlsplit, urlunsplit

from ..daemon.browser.projected_browser_runtime import (
    _wait_cdp_endpoint,
    ensure_dir,
    ensure_sync_playwright,
)
from ..paths import ensure_home
from ..util.process import pid_is_alive, terminate_pid
from ..util.fs import atomic_write_json, read_json
from ..util.time import parse_utc_iso, utc_now_iso

CHATGPT_URL = "https://chatgpt.com/"
INPUT_SELECTORS = [
    ".ProseMirror",
    '[contenteditable="true"][data-virtualkeyboard="true"]',
    '[role="textbox"][contenteditable="true"]',
    '[contenteditable="true"]',
    'textarea[data-id="prompt-textarea"]',
    'textarea[name="prompt-textarea"]',
    "#prompt-textarea",
    'textarea[placeholder*="Send a message"]',
    'textarea[aria-label="Message ChatGPT"]',
]
SEND_BUTTON_SELECTORS = [
    "#composer-submit-button",
    'button[data-testid="send-button"]',
    'button[data-testid="composer-submit-button"]',
    'button[data-testid*="composer-send"]',
    'button[type="submit"][data-testid*="send"]',
    'button[aria-label*="Send"]',
    'button[aria-label*="Send prompt"]',
    'button[aria-label*="发送"]',
    'button[aria-label*="送信"]',
]
CDP_CONNECT_TIMEOUT_MS = 5000
DEFAULT_BROWSER_DELIVERY_TIMEOUT_SECONDS = 120.0


class _UnsafeSubmitState(RuntimeError):
    pass


def _normalize_chatgpt_url(value: Any, *, require_conversation: bool = False) -> str:
    raw = str(value or "").strip()
    if not raw:
        return ""
    try:
        parsed = urlsplit(raw)
    except Exception:
        return ""
    if str(parsed.scheme or "").lower() != "https":
        return ""
    host = str(parsed.hostname or "").lower().rstrip(".")
    if host != "chatgpt.com" and not host.endswith(".chatgpt.com"):
        return ""
    try:
        port = parsed.port
    except ValueError:
        return ""
    path = str(parsed.path or "/")
    if require_conversation:
        parts = [part for part in path.split("/") if part]
        has_conversation_id = any(part == "c" and index + 1 < len(parts) for index, part in enumerate(parts))
        if not has_conversation_id:
            return ""
    netloc = host if not port or port == 443 else f"{host}:{port}"
    return urlunsplit(("https", netloc, path, "", ""))


def _conversation_url_from_tab(value: Any) -> str:
    return _normalize_chatgpt_url(value, require_conversation=True)


def _wait_for_conversation_url(page: Any, *, timeout_seconds: float = 15.0) -> str:
    deadline = time.time() + max(1.0, float(timeout_seconds))
    while time.time() < deadline:
        url = _conversation_url_from_tab(str(getattr(page, "url", "") or ""))
        if url:
            return url
        time.sleep(0.25)
    return _conversation_url_from_tab(str(getattr(page, "url", "") or ""))


def _safe_token(value: str, fallback: str) -> str:
    raw = str(value or "").strip()
    cleaned = "".join(ch if ch.isalnum() or ch in {"-", "_"} else "_" for ch in raw).strip("_")
    return cleaned[:96] or fallback


def _actor_state_root(payload: dict[str, Any]) -> Path:
    group_id = _safe_token(str(payload.get("group_id") or ""), "group")
    actor_id = _safe_token(str(payload.get("actor_id") or ""), "actor")
    return ensure_home() / "state" / "web_model_browser" / group_id / actor_id


def chatgpt_browser_actor_state_root(group_id: str, actor_id: str) -> Path:
    return _actor_state_root({"group_id": group_id, "actor_id": actor_id})


def chatgpt_browser_profile_root(group_id: str = "", actor_id: str = "") -> Path:
    """Return the shared ChatGPT browser profile root.

    ChatGPT Web Model is a single runtime seat, so login cookies must be tied to
    the ChatGPT browser surface rather than to a replaceable CCCC actor id.
    """
    _ = (group_id, actor_id)
    return ensure_home() / "state" / "web_model_browser" / "_shared" / "chatgpt_web"


def _state_path(profile_root: Path) -> Path:
    return profile_root / "state.json"


def _profile_dir(profile_root: Path) -> Path:
    path = profile_root / "chrome_profile"
    ensure_dir(path, 0o700)
    return path


def chatgpt_browser_profile_dir(group_id: str, actor_id: str) -> Path:
    _ensure_shared_profile_migrated(group_id, actor_id)
    return _profile_dir(chatgpt_browser_profile_root(group_id, actor_id))


def _dir_has_content(path: Path) -> bool:
    try:
        return any(path.iterdir())
    except Exception:
        return False


def _candidate_legacy_profile_dirs(group_id: str, actor_id: str) -> list[Path]:
    roots: list[Path] = []
    direct = _actor_state_root({"group_id": group_id, "actor_id": actor_id}) / "chrome_profile"
    if direct.exists():
        roots.append(direct)
    base = ensure_home() / "state" / "web_model_browser"
    try:
        for candidate in base.glob("*/*/chrome_profile"):
            if "_shared" in candidate.parts:
                continue
            if candidate not in roots:
                roots.append(candidate)
    except Exception:
        pass
    return sorted(
        [item for item in roots if item.exists() and _dir_has_content(item)],
        key=lambda item: item.stat().st_mtime if item.exists() else 0,
        reverse=True,
    )


def _ensure_shared_profile_migrated(group_id: str, actor_id: str) -> None:
    shared = chatgpt_browser_profile_root(group_id, actor_id) / "chrome_profile"
    if _dir_has_content(shared):
        return
    candidates = _candidate_legacy_profile_dirs(group_id, actor_id)
    if not candidates:
        ensure_dir(shared, 0o700)
        return
    ensure_dir(shared.parent, 0o700)
    try:
        shutil.copytree(candidates[0], shared, dirs_exist_ok=True)
    except Exception:
        ensure_dir(shared, 0o700)


def record_chatgpt_browser_state(group_id: str, actor_id: str, state: dict[str, Any]) -> None:
    state_root = chatgpt_browser_actor_state_root(group_id, actor_id)
    current = _load_state(state_root)
    _write_state(state_root, {**current, **dict(state or {})})


def read_chatgpt_browser_state(group_id: str, actor_id: str) -> dict[str, Any]:
    return _load_state(chatgpt_browser_actor_state_root(group_id, actor_id))


def record_chatgpt_browser_process_state(state: dict[str, Any]) -> None:
    root = chatgpt_browser_profile_root()
    current = _load_state(root)
    _write_state(root, {**current, **dict(state or {})})


def read_chatgpt_browser_process_state() -> dict[str, Any]:
    return _load_state(chatgpt_browser_profile_root())


def reset_chatgpt_browser_actor_runtime_state(group_id: str, actor_id: str) -> None:
    """Clear actor binding/delivery state while preserving the Chrome login profile."""
    state_root = chatgpt_browser_actor_state_root(group_id, actor_id)
    current = _load_state(state_root)
    _write_state(
        state_root,
        {
            **current,
            "conversation_url": "",
            "pending_new_chat_bind": False,
            "pending_new_chat_url": "",
            "pending_new_chat_bind_started_at": "",
            "pending_new_chat_submitted": False,
            "pending_new_chat_submitted_at": "",
            "pending_new_chat_delivery_id": "",
            "pending_new_chat_last_turn_id": "",
            "pending_new_chat_last_event_ids": [],
            "pending_new_chat_last_tab_url": "",
            "new_chat_bound_at": "",
            "target_saved_at": "",
            "bootstrap_seed_delivered_at": "",
            "bootstrap_seed_version": "",
            "bootstrap_seed_digest": "",
            "bootstrap_seed_conversation_url": "",
            "last_delivery_at": "",
            "last_delivery_started_at": "",
            "last_turn_id": "",
            "last_event_ids": [],
            "last_delivery_id": "",
            "last_delivery_status": "",
            "last_submission_evidence": "",
            "last_send_selector": "",
            "auto_reload_active": False,
            "auto_reload_window_started_at": "",
            "auto_reload_window_expires_at": "",
            "auto_reload_last_progress_at": "",
            "auto_reload_last_progress_reason": "",
            "auto_reload_last_progress_detail": "",
            "auto_reload_last_delivery_id": "",
            "auto_reload_last_turn_id": "",
            "auto_reload_last_event_ids": [],
            "auto_reload_target_url": "",
            "auto_reload_last_reload_at": "",
            "auto_reload_last_reload_reason": "",
            "auto_reload_last_page_url": "",
            "auto_reload_count": 0,
            "auto_reload_completed_at": "",
            "auto_reload_completed_reason": "",
            "auto_reload_expired_at": "",
            "auto_reload_last_error": "",
            "last_error": "",
        },
    )


def _load_state(profile_root: Path) -> dict[str, Any]:
    state = read_json(_state_path(profile_root))
    return state if isinstance(state, dict) else {}


def _write_state(profile_root: Path, state: dict[str, Any]) -> None:
    ensure_dir(profile_root, 0o700)
    atomic_write_json(_state_path(profile_root), {**state, "updated_at": utc_now_iso()})


def _normalize_visibility(value: str) -> str:
    raw = str(value or "").strip().lower()
    if raw in {"projected", "embedded", "browser_surface"}:
        return "projected"
    if raw in {"background", "hidden", "xvfb", "virtual"}:
        return "background"
    if raw in {"headless", "true_headless", "chrome_headless"}:
        return "headless"
    return "visible"


def _managed_profile_dir(profile_dir: str | Path) -> Path | None:
    try:
        path = Path(profile_dir).resolve()
        root = chatgpt_browser_profile_root().resolve()
        if path.is_relative_to(root):
            return path
    except Exception:
        return None
    return None


def _same_profile_path(left: str | Path, right: str | Path) -> bool:
    try:
        left_norm = os.path.normcase(os.path.abspath(os.path.expanduser(str(left or ""))))
        right_norm = os.path.normcase(os.path.abspath(os.path.expanduser(str(right or ""))))
    except Exception:
        return False
    return bool(left_norm and right_norm and left_norm == right_norm)


def _user_data_dir_from_args(args: list[str]) -> str:
    for index, raw in enumerate(args):
        arg = str(raw or "").strip()
        if arg.startswith("--user-data-dir="):
            return arg.split("=", 1)[1].strip().strip("\"'")
        if arg == "--user-data-dir" and index + 1 < len(args):
            return str(args[index + 1] or "").strip().strip("\"'")
    return ""


def _user_data_dir_from_command_line(command: str) -> str:
    text = str(command or "").strip()
    if not text:
        return ""
    match = re.search(r"--user-data-dir=(?:\"([^\"]+)\"|'([^']+)'|(\S+))", text)
    if match:
        return str(next((item for item in match.groups() if item), "") or "").strip()
    match = re.search(r"--user-data-dir\s+(?:\"([^\"]+)\"|'([^']+)'|(\S+))", text)
    if match:
        return str(next((item for item in match.groups() if item), "") or "").strip()
    return ""


def _profile_process_pids_from_proc(profile: Path) -> list[int]:
    proc_root = Path("/proc")
    if not proc_root.exists():
        return []
    pids: list[int] = []
    current_pid = os.getpid()
    for child in proc_root.iterdir():
        if not child.name.isdigit():
            continue
        try:
            pid = int(child.name)
        except Exception:
            continue
        if pid <= 0 or pid == current_pid:
            continue
        try:
            raw = (child / "cmdline").read_bytes()
        except Exception:
            continue
        if not raw:
            continue
        args = [part.decode("utf-8", errors="ignore") for part in raw.split(b"\0") if part]
        user_data_dir = _user_data_dir_from_args(args)
        if user_data_dir and _same_profile_path(user_data_dir, profile):
            pids.append(pid)
    return pids


def _profile_process_pids_from_ps(profile: Path) -> list[int]:
    try:
        proc = subprocess.run(
            ["ps", "-axo", "pid=,command="],
            capture_output=True,
            text=True,
            timeout=5,
        )
    except Exception:
        return []
    if int(getattr(proc, "returncode", 1) or 0) != 0:
        return []
    pids: list[int] = []
    current_pid = os.getpid()
    for raw_line in str(proc.stdout or "").splitlines():
        line = raw_line.strip()
        if not line:
            continue
        parts = line.split(None, 1)
        if len(parts) != 2:
            continue
        try:
            pid = int(parts[0])
        except Exception:
            continue
        if pid <= 0 or pid == current_pid:
            continue
        user_data_dir = _user_data_dir_from_command_line(parts[1])
        if user_data_dir and _same_profile_path(user_data_dir, profile):
            pids.append(pid)
    return pids


def _profile_process_pids_from_windows(profile: Path) -> list[int]:
    powershell = shutil.which("powershell.exe") or shutil.which("powershell") or shutil.which("pwsh")
    if not powershell:
        return []
    command = (
        "Get-CimInstance Win32_Process | "
        "Select-Object ProcessId,CommandLine | "
        "ConvertTo-Json -Compress"
    )
    try:
        proc = subprocess.run(
            [powershell, "-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", command],
            capture_output=True,
            text=True,
            timeout=10,
        )
    except Exception:
        return []
    if int(getattr(proc, "returncode", 1) or 0) != 0:
        return []
    try:
        raw = json.loads(str(proc.stdout or "null"))
    except Exception:
        return []
    rows = raw if isinstance(raw, list) else [raw]
    pids: list[int] = []
    current_pid = os.getpid()
    for item in rows:
        if not isinstance(item, dict):
            continue
        try:
            pid = int(item.get("ProcessId") or 0)
        except Exception:
            continue
        if pid <= 0 or pid == current_pid:
            continue
        user_data_dir = _user_data_dir_from_command_line(str(item.get("CommandLine") or ""))
        if user_data_dir and _same_profile_path(user_data_dir, profile):
            pids.append(pid)
    return pids


def _profile_process_pids(profile_dir: str | Path) -> list[int]:
    profile = _managed_profile_dir(profile_dir)
    if profile is None:
        return []
    if os.name == "nt":
        pids = _profile_process_pids_from_windows(profile)
    else:
        pids = _profile_process_pids_from_proc(profile) or _profile_process_pids_from_ps(profile)
    return sorted(set(pids), reverse=True)


def _stop_browser_profile_processes(profile_dir: str | Path) -> None:
    pids = _profile_process_pids(profile_dir)
    if not pids:
        return

    for pid in pids:
        terminate_pid(pid, timeout_s=0.2, include_group=True, force=False)
    deadline = time.time() + 3.0
    while time.time() < deadline and any(pid_is_alive(pid) for pid in pids):
        time.sleep(0.1)
    for pid in pids:
        if not pid_is_alive(pid):
            continue
        terminate_pid(pid, timeout_s=0.5, include_group=True, force=True)


def _stop_browser_state(state: dict[str, Any]) -> None:
    pid = int(state.get("pid") or 0)
    profile_dir = str(state.get("profile_dir") or "").strip()
    if pid <= 0:
        if profile_dir:
            _stop_browser_profile_processes(profile_dir)
        return
    terminate_pid(pid, timeout_s=3.0, include_group=True, force=True)
    if profile_dir:
        _stop_browser_profile_processes(profile_dir)


def _visible_input_selector(page: Any, *, timeout_seconds: float) -> str:
    deadline = time.time() + max(1.0, float(timeout_seconds))
    last_error = ""
    while time.time() < deadline:
        try:
            candidate = page.evaluate(
                """selectors => {
                    const isVisible = (node) => {
                        if (!node) return false;
                        const rect = node.getBoundingClientRect();
                        const style = window.getComputedStyle(node);
                        return rect.width > 0 && rect.height > 0 && style.visibility !== "hidden" && style.display !== "none";
                    };
                    const isEditable = (node) => {
                        if (!node || !isVisible(node)) return false;
                        if (node.matches("textarea")) return !node.disabled && !node.readOnly;
                        if (node.matches("input")) {
                            const type = String(node.type || "text").toLowerCase();
                            return !node.disabled && !node.readOnly && !/password|search|email|url|number|tel|file|hidden|checkbox|radio|submit|button|reset/.test(type);
                        }
                        return node.isContentEditable || node.getAttribute("contenteditable") === "true" || node.getAttribute("role") === "textbox";
                    };
                    const score = (node) => {
                        const rect = node.getBoundingClientRect();
                        const label = [
                            node.getAttribute("aria-label") || "",
                            node.getAttribute("placeholder") || "",
                            node.getAttribute("name") || "",
                            node.getAttribute("id") || "",
                            node.getAttribute("data-testid") || "",
                        ].join(" ").toLowerCase();
                        let out = 0;
                        if (/prompt|message|ask|chat|query|input/.test(label)) out += 80;
                        if (node.matches("textarea")) out += 50;
                        if (node.isContentEditable || node.getAttribute("contenteditable") === "true") out += 35;
                        if (node.getAttribute("role") === "textbox") out += 25;
                        if (String(node.className || "").toLowerCase().includes("fallback")) out -= 200;
                        if (rect.width >= 260 && rect.height >= 26) out += 20;
                        out += Math.min(180, Math.max(0, (rect.width * rect.height) / 2500));
                        out += Math.max(0, rect.y / 8);
                        return out;
                    };
                    const nodes = [];
                    const seen = new Set();
                    const addNode = (node) => {
                        if (!node || seen.has(node)) return;
                        seen.add(node);
                        nodes.push(node);
                    };
                    for (const selector of selectors || []) {
                        try {
                            document.querySelectorAll(selector).forEach(addNode);
                        } catch (_error) {}
                    }
                    [
                        ...document.querySelectorAll("main [contenteditable='true'], main [role='textbox'], main textarea"),
                        ...document.querySelectorAll("[contenteditable='true'], [role='textbox'], textarea, input"),
                    ].forEach(addNode);
                    let best = null;
                    let bestScore = -Infinity;
                    for (const node of nodes) {
                        if (!isEditable(node)) continue;
                        const candidateScore = score(node);
                        if (candidateScore > bestScore) {
                            best = node;
                            bestScore = candidateScore;
                        }
                    }
                    if (!best) return "";
                    const marker = "cccc-chatgpt-composer-input";
                    for (const node of document.querySelectorAll("[data-cccc-chatgpt-input-candidate]")) {
                        node.removeAttribute("data-cccc-chatgpt-input-candidate");
                    }
                    best.setAttribute("data-cccc-chatgpt-input-candidate", marker);
                    return `[data-cccc-chatgpt-input-candidate="${marker}"]`;
                }""",
                INPUT_SELECTORS,
            )
            if str(candidate or "").strip():
                return str(candidate)
        except Exception as exc:
            last_error = str(exc)
        time.sleep(0.2)
    raise RuntimeError(last_error or "ChatGPT composer input not found; log in and enable the CCCC connector")


def _clear_and_type_prompt(page: Any, selector: str, prompt: str) -> None:
    locator = page.locator(selector).first
    locator.click(timeout=5000)
    try:
        can_fill = bool(
            locator.evaluate(
                """(el) => {
                    if (!el) return false;
                    const tag = String(el.tagName || "").toLowerCase();
                    return (tag === "textarea" || tag === "input") && !el.isContentEditable;
                }""",
                timeout=1000,
            )
        )
    except Exception:
        can_fill = False
    if can_fill:
        try:
            locator.fill(prompt, timeout=5000)
            return
        except Exception:
            pass
    try:
        locator.evaluate(
            """(el) => {
                if (!el || !el.isContentEditable) return;
                el.focus();
            }""",
            timeout=1000,
        )
    except Exception:
        pass
    modifier = "Meta" if sys.platform == "darwin" else "Control"
    page.keyboard.press(f"{modifier}+A")
    page.keyboard.press("Backspace")
    page.keyboard.insert_text(prompt)
    try:
        locator.evaluate(
            """(el) => {
                el.dispatchEvent(new InputEvent('input', { bubbles: true, inputType: 'insertText', data: '' }));
                el.dispatchEvent(new Event('change', { bubbles: true }));
            }"""
        )
    except Exception:
        pass


def _composer_text(page: Any, selector: str) -> str:
    locator = page.locator(selector).first
    try:
        text = locator.evaluate(
            """(el) => {
                if (!el) return "";
                const value = "value" in el ? String(el.value || "") : "";
                if (value.trim()) return value;
                return String(el.innerText || el.textContent || "");
            }""",
            timeout=1000,
        )
        if str(text or "").strip():
            return str(text or "").strip()
    except Exception:
        pass
    try:
        return str(locator.input_value(timeout=150) or "").strip()
    except Exception:
        pass
    try:
        return str(locator.inner_text(timeout=150) or "").strip()
    except Exception:
        pass
    try:
        return str(locator.text_content(timeout=150) or "").strip()
    except Exception:
        return ""


def _normalize_composer_text(value: str) -> str:
    return " ".join(str(value or "").replace("\xa0", " ").split())


def _prompt_inserted(page: Any, selector: str, prompt: str) -> bool:
    actual = _normalize_composer_text(_composer_text(page, selector))
    expected = _normalize_composer_text(prompt)
    if not actual or not expected:
        return False
    if expected in actual:
        return True
    prefix = expected[: min(160, len(expected))]
    suffix = expected[-min(120, len(expected)) :]
    return bool(prefix and prefix in actual) and (len(expected) <= 200 or bool(suffix and suffix in actual))


def _prompt_present_in_any_composer(page: Any, prompt: str, selector: str = "") -> bool:
    expected = _normalize_composer_text(prompt)
    if not expected:
        return False
    if selector:
        try:
            if _prompt_inserted(page, selector, prompt):
                return True
        except Exception:
            pass
    prefix = expected[: min(160, len(expected))]
    suffix = expected[-min(120, len(expected)) :]
    try:
        texts = page.evaluate(
            """selector => {
                const isVisible = (node) => {
                    if (!node) return false;
                    const rect = node.getBoundingClientRect();
                    const style = window.getComputedStyle(node);
                    return rect.width > 0 && rect.height > 0 && style.visibility !== "hidden" && style.display !== "none";
                };
                const isEditable = (node) => {
                    if (!node || !isVisible(node)) return false;
                    if (node.matches("textarea")) return !node.disabled && !node.readOnly;
                    if (node.matches("input")) {
                        const type = String(node.type || "text").toLowerCase();
                        return !node.disabled && !node.readOnly && !/password|search|email|url|number|tel|file|hidden|checkbox|radio|submit|button|reset/.test(type);
                    }
                    return node.isContentEditable || node.getAttribute("contenteditable") === "true" || node.getAttribute("role") === "textbox";
                };
                const readText = (node) => {
                    if (!node) return "";
                    const value = "value" in node ? String(node.value || "") : "";
                    if (value.trim()) return value;
                    return String(node.innerText || node.textContent || "");
                };
                const nodes = [];
                const seen = new Set();
                const addNode = (node) => {
                    if (!node || seen.has(node)) return;
                    seen.add(node);
                    nodes.push(node);
                };
                if (selector) {
                    try {
                        document.querySelectorAll(selector).forEach(addNode);
                    } catch (_error) {}
                }
                [
                    ...document.querySelectorAll("[data-cccc-chatgpt-input-candidate]"),
                    ...document.querySelectorAll("main textarea, main [role='textbox'], main [contenteditable='true']"),
                    ...document.querySelectorAll("textarea, [role='textbox'], [contenteditable='true']"),
                ].forEach(addNode);
                return nodes.filter(isEditable).map(readText).filter((text) => String(text || "").trim()).slice(0, 12);
            }""",
            selector,
        )
    except Exception:
        return False
    if not isinstance(texts, list):
        return False
    for raw in texts:
        actual = _normalize_composer_text(str(raw or ""))
        if not actual:
            continue
        if expected in actual:
            return True
        if prefix and prefix in actual and (len(expected) <= 200 or bool(suffix and suffix in actual)):
            return True
    return False


def _wait_for_prompt_inserted(page: Any, selector: str, prompt: str, *, timeout_seconds: float = 3.0) -> bool:
    deadline = time.time() + max(0.5, float(timeout_seconds))
    while time.time() < deadline:
        if _prompt_present_in_any_composer(page, prompt, selector):
            return True
        time.sleep(0.1)
    return _prompt_present_in_any_composer(page, prompt, selector)


def _raise_if_chatgpt_running_before_submit(page: Any) -> None:
    if _chatgpt_running_visible(page):
        raise _UnsafeSubmitState(
            "ChatGPT is currently responding; refusing to click the composer control because it may be the stop button"
        )


def _chatgpt_started_after_submit_attempt(page: Any, *, timeout_seconds: float = 0.75) -> bool:
    deadline = time.time() + max(0.0, float(timeout_seconds))
    while True:
        if _chatgpt_running_visible(page):
            return True
        if time.time() >= deadline:
            return False
        time.sleep(0.05)


def _selector_resolves_to_stop_control(page: Any, selector: str) -> bool:
    raw_selector = str(selector or "").strip()
    if not raw_selector:
        return False
    try:
        return bool(
            page.evaluate(
                """selector => {
                    let node = null;
                    try {
                        node = document.querySelector(selector);
                    } catch (_error) {
                        return false;
                    }
                    if (!node) return false;
                    const label = [
                        node.getAttribute("aria-label") || "",
                        node.getAttribute("title") || "",
                        node.getAttribute("data-testid") || "",
                        node.id || "",
                        node.className || "",
                        node.innerText || node.textContent || "",
                    ].join(" ").replace(/\\s+/g, " ").trim().toLowerCase();
                    return /\\bstop\\b|停止|中止|cancel generation|interrupt/.test(label);
                }""",
                raw_selector,
            )
        )
    except Exception:
        return False


def _send_control_state(page: Any, selector: str) -> str:
    button = page.locator(selector).first
    if button.count() <= 0:
        return "missing"
    if not button.is_visible(timeout=250):
        return "hidden"
    try:
        if button.is_disabled(timeout=250):
            return "disabled"
    except Exception:
        pass
    if _selector_resolves_to_stop_control(page, selector):
        raise _UnsafeSubmitState(
            f"ChatGPT composer control matched stop state for selector {selector}; refusing to click"
        )
    return "ready"


def _wait_for_stable_send_control(
    page: Any,
    selector: str,
    *,
    deadline: float,
    stable_seconds: float = 0.35,
) -> bool:
    stable_since = 0.0
    wait_deadline = min(deadline, time.time() + max(0.45, float(stable_seconds) + 0.25))
    while time.time() < wait_deadline:
        _raise_if_chatgpt_running_before_submit(page)
        state = _send_control_state(page, selector)
        if state == "ready":
            now = time.time()
            if stable_since <= 0:
                stable_since = now
            if now - stable_since >= max(0.0, float(stable_seconds)):
                return True
        elif state in {"missing", "hidden"}:
            return False
        else:
            stable_since = 0.0
        time.sleep(0.05)
    return False


def _click_send(page: Any, *, timeout_seconds: float = 5.0) -> str:
    deadline = time.time() + max(0.5, float(timeout_seconds))
    last_error = ""
    while time.time() < deadline:
        _raise_if_chatgpt_running_before_submit(page)
        for selector in SEND_BUTTON_SELECTORS:
            try:
                if not _wait_for_stable_send_control(page, selector, deadline=deadline):
                    continue
                page.locator(selector).first.click(timeout=5000)
                return selector
            except _UnsafeSubmitState:
                raise
            except Exception as exc:
                last_error = str(exc)
                if _chatgpt_started_after_submit_attempt(page):
                    return f"{selector}:post_click_running"
        try:
            candidate_selector = page.evaluate(
                """() => {
                    const isVisible = (node) => {
                        if (!node) return false;
                        const rect = node.getBoundingClientRect();
                        const style = window.getComputedStyle(node);
                        return rect.width > 0 && rect.height > 0 && style.visibility !== "hidden" && style.display !== "none";
                    };
                    const isDisabled = (node) => !!node.disabled || String(node.getAttribute("aria-disabled") || "").toLowerCase() === "true";
                    const labelOf = (node) => [
                        node.getAttribute("aria-label") || "",
                        node.getAttribute("title") || "",
                        node.getAttribute("data-testid") || "",
                        node.id || "",
                        node.className || "",
                        node.innerText || node.textContent || "",
                    ].join(" ").replace(/\\s+/g, " ").trim().toLowerCase();
                    const editable = (node) => {
                        if (!node || !isVisible(node)) return false;
                        if (node.matches("textarea")) return !node.disabled && !node.readOnly;
                        if (node.matches("input")) return !node.disabled && !node.readOnly && !/password|search|email|url|number|tel|file|hidden|checkbox|radio|submit|button|reset/i.test(String(node.type || "text"));
                        return node.isContentEditable || node.getAttribute("contenteditable") === "true" || node.getAttribute("role") === "textbox";
                    };
                    const promptCandidates = [
                        ...document.querySelectorAll("[data-cccc-chatgpt-input-candidate]"),
                        ...document.querySelectorAll("main textarea, main [role='textbox'], main [contenteditable='true']"),
                        ...document.querySelectorAll("textarea, [role='textbox'], [contenteditable='true']"),
                    ];
                    const prompt = promptCandidates.find(editable) || document.activeElement;
                    const composerRoot =
                        prompt?.closest?.("form") ||
                        prompt?.closest?.("[data-testid*='composer' i], [data-testid*='prompt' i], [data-testid*='chat-input' i], [aria-label*='message' i], [aria-label*='prompt' i]") ||
                        prompt?.closest?.("main") ||
                        null;
                    const promptRect = prompt?.getBoundingClientRect?.() || null;
                    const score = (button) => {
                        const rect = button.getBoundingClientRect();
                        const label = labelOf(button);
                        let out = 0;
                        if (button.matches("#composer-submit-button, button[data-testid='send-button'], button[data-testid='composer-submit-button'], button[data-testid*='composer-send'], button[aria-label*='Send'], button[aria-label*='发送'], button[aria-label*='送信']")) out += 120;
                        if (/send|submit|run|go|ask|reply|发送|送信/.test(label)) out += 90;
                        if (/stop|cancel|retry|signin|sign in|log in|login|continue with|google|microsoft|apple/.test(label)) out -= 160;
                        if (button.getAttribute("type") === "submit") out += 35;
                        if (composerRoot && composerRoot.contains(button)) out += 170;
                        if (rect.width >= 16 && rect.height >= 16) out += 10;
                        out += Math.max(0, rect.y / 10);
                        out += Math.max(0, rect.x / 20);
                        if (promptRect) {
                            const cx = rect.x + rect.width / 2;
                            const cy = rect.y + rect.height / 2;
                            const dx = Math.abs(cx - (promptRect.x + promptRect.width));
                            const dy = Math.abs(cy - (promptRect.y + promptRect.height / 2));
                            out += Math.max(0, 140 - dx / 6 - dy / 4);
                        }
                        return out;
                    };
                    const pool = [];
                    const seen = new Set();
                    const local = composerRoot ? [...composerRoot.querySelectorAll("button, [role='button']")] : [];
                    for (const node of [...local, ...document.querySelectorAll("button, [role='button']")]) {
                        if (!node || seen.has(node)) continue;
                        seen.add(node);
                        if (!isVisible(node) || isDisabled(node)) continue;
                        pool.push(node);
                    }
                    let best = null;
                    let bestScore = -Infinity;
                    for (const button of pool) {
                        const candidateScore = score(button);
                        if (candidateScore > bestScore) {
                            best = button;
                            bestScore = candidateScore;
                        }
                    }
                    if (!best || bestScore < 60) return "";
                    const marker = "cccc-chatgpt-send-candidate";
                    for (const node of document.querySelectorAll("[data-cccc-chatgpt-send-candidate]")) {
                        node.removeAttribute("data-cccc-chatgpt-send-candidate");
                    }
                    best.setAttribute("data-cccc-chatgpt-send-candidate", marker);
                    return `[data-cccc-chatgpt-send-candidate="${marker}"]`;
                }"""
            )
            candidate_selector = str(candidate_selector or "").strip()
            if candidate_selector:
                _raise_if_chatgpt_running_before_submit(page)
                if not _wait_for_stable_send_control(page, candidate_selector, deadline=deadline):
                    continue
                page.locator(candidate_selector).first.click(timeout=5000)
                return "scored:composer-submit"
        except _UnsafeSubmitState:
            raise
        except Exception as exc:
            last_error = str(exc)
            if _chatgpt_started_after_submit_attempt(page):
                return "scored:composer-submit:post_click_running"
        time.sleep(0.15)
    raise RuntimeError(last_error or "ChatGPT send button not found or disabled")


def _request_submit_composer(page: Any) -> str:
    try:
        result = page.evaluate(
            """() => {
                const isVisible = (node) => {
                    if (!node) return false;
                    const rect = node.getBoundingClientRect();
                    const style = window.getComputedStyle(node);
                    return rect.width > 0 && rect.height > 0 && style.visibility !== "hidden" && style.display !== "none";
                };
                const isDisabled = (node) => !!node.disabled || String(node.getAttribute("aria-disabled") || "").toLowerCase() === "true";
                const labelOf = (node) => [
                    node.getAttribute("aria-label") || "",
                    node.getAttribute("title") || "",
                    node.getAttribute("data-testid") || "",
                    node.id || "",
                    node.className || "",
                    node.innerText || node.textContent || "",
                ].join(" ").replace(/\\s+/g, " ").trim().toLowerCase();
                const stopSelectors = [
                    '[data-testid="stop-button"]',
                    '[data-testid*="stop" i]',
                    'button[aria-label*="Stop" i]',
                    'button[aria-label*="停止"]',
                    'button[aria-label*="中止"]',
                    'button[title*="Stop" i]',
                    'button[title*="停止"]',
                    '[role="button"][aria-label*="Stop" i]',
                    '[role="button"][aria-label*="停止"]',
                    '[role="button"][aria-label*="中止"]',
                ];
                for (const stopSelector of stopSelectors) {
                    try {
                        if (Array.from(document.querySelectorAll(stopSelector)).some(isVisible)) return "";
                    } catch (_error) {}
                }
                for (const button of Array.from(document.querySelectorAll("button, [role='button']"))) {
                    if (isVisible(button) && /\\bstop\\b|停止|中止|cancel generation|interrupt/.test(labelOf(button))) return "";
                }
                const editable = (node) => {
                    if (!node || !isVisible(node)) return false;
                    if (node.matches("textarea")) return !node.disabled && !node.readOnly;
                    if (node.matches("input")) return !node.disabled && !node.readOnly && !/password|search|email|url|number|tel|file|hidden|checkbox|radio|submit|button|reset/i.test(String(node.type || "text"));
                    return node.isContentEditable || node.getAttribute("contenteditable") === "true" || node.getAttribute("role") === "textbox";
                };
                const promptCandidates = [
                    ...document.querySelectorAll("[data-cccc-chatgpt-input-candidate]"),
                    ...document.querySelectorAll("main textarea, main [role='textbox'], main [contenteditable='true']"),
                    ...document.querySelectorAll("textarea, [role='textbox'], [contenteditable='true']"),
                ];
                const prompt = promptCandidates.find(editable) || document.activeElement;
                const form = prompt?.closest?.("form") || null;
                if (!form || typeof form.requestSubmit !== "function") return "";
                const submit = Array.from(form.querySelectorAll("button, [role='button']")).find((button) => {
                    if (!isVisible(button) || isDisabled(button)) return false;
                    const label = labelOf(button);
                    if (/stop|cancel|retry|signin|sign in|log in|login|google|microsoft|apple/.test(label)) return false;
                    return button.getAttribute("type") === "submit" || /send|submit|run|go|ask|reply|发送|送信/.test(label);
                });
                form.requestSubmit(submit || undefined);
                return submit ? "form.requestSubmit:button" : "form.requestSubmit";
            }"""
        )
        return str(result or "").strip()
    except Exception:
        return ""


def _submission_diagnostics(page: Any, selector: str) -> dict[str, Any]:
    try:
        probe = page.evaluate(
            """selector => {
                const isVisible = (node) => {
                    if (!node) return false;
                    const rect = node.getBoundingClientRect();
                    const style = window.getComputedStyle(node);
                    return rect.width > 0 && rect.height > 0 && style.visibility !== "hidden" && style.display !== "none";
                };
                const text = String(document.body?.innerText || "").slice(0, 5000);
                const prompt = document.querySelector(selector);
                const promptText = prompt
                    ? String("value" in prompt ? prompt.value || "" : prompt.innerText || prompt.textContent || "")
                    : "";
                const sendButtons = Array.from(document.querySelectorAll("button, [role='button']")).filter((button) => {
                    if (!isVisible(button)) return false;
                    const label = [
                        button.getAttribute("aria-label") || "",
                        button.getAttribute("title") || "",
                        button.getAttribute("data-testid") || "",
                        button.innerText || button.textContent || "",
                    ].join(" ").toLowerCase();
                    if (/stop|cancel|retry|signin|sign in|log in|login|google|microsoft|apple/.test(label)) return false;
                    return button.getAttribute("type") === "submit" || /send|submit|run|go|ask|reply|发送|送信/.test(label);
                });
                const stopSelectors = [
                    '[data-testid="stop-button"]',
                    '[data-testid*="stop" i]',
                    'button[aria-label*="Stop" i]',
                    'button[aria-label*="停止"]',
                    'button[aria-label*="中止"]',
                    'button[title*="Stop" i]',
                    'button[title*="停止"]',
                    '[role="button"][aria-label*="Stop" i]',
                    '[role="button"][aria-label*="停止"]',
                    '[role="button"][aria-label*="中止"]',
                ];
                const stopVisible = stopSelectors.some((stopSelector) => {
                    try {
                        return Array.from(document.querySelectorAll(stopSelector)).some(isVisible);
                    } catch (_error) {
                        return false;
                    }
                }) || Array.from(document.querySelectorAll("button, [role='button']")).some((button) => {
                    if (!isVisible(button)) return false;
                    const label = [
                        button.getAttribute("aria-label") || "",
                        button.getAttribute("title") || "",
                        button.getAttribute("data-testid") || "",
                        button.id || "",
                        button.innerText || button.textContent || "",
                    ].join(" ").replace(/\\s+/g, " ").trim().toLowerCase();
                    return /\\bstop\\b|停止|中止/.test(label);
                });
                const loginLike = /log in|sign in|continue with|登录|登入/i.test(text);
                const challengeLike = /verify you are human|human verification|captcha|turnstile|arkose|access denied|unusual traffic/i.test(text);
                return {
                    url: location.href || "",
                    ready_state: document.readyState || "",
                    prompt_found: !!prompt,
                    prompt_chars: promptText.trim().length,
                    send_candidate_count: sendButtons.length,
                    send_enabled_count: sendButtons.filter((button) => !button.disabled && String(button.getAttribute("aria-disabled") || "").toLowerCase() !== "true").length,
                    stop_visible: stopVisible,
                    login_like: loginLike,
                    challenge_like: challengeLike,
                };
            }""",
            selector,
        )
        if isinstance(probe, dict):
            return probe
    except Exception as exc:
        return {"error": str(exc)[:500]}
    return {}


def _submission_echo_found(page: Any, prompt: str) -> bool:
    needles = _submission_echo_needles(prompt)
    if not needles:
        return False
    try:
        return bool(
            page.evaluate(
                """needles => {
                    const normalize = value => String(value || "").replace(/\\s+/g, " ").trim();
                    const targets = Array.isArray(needles) ? needles.map(normalize).filter(Boolean) : [];
                    if (!targets.length) return false;
                    const candidates = [
                        ...document.querySelectorAll('[data-message-author-role="user"]'),
                        ...document.querySelectorAll('[data-testid*="conversation-turn"]'),
                        ...document.querySelectorAll('main article'),
                    ];
                    for (const node of candidates) {
                        const text = normalize(node.innerText || node.textContent || "");
                        if (targets.some(target => text.includes(target))) return true;
                    }
                    return false;
                }""",
                needles,
            )
        )
    except Exception:
        return False


def _submission_echo_needles(prompt: str) -> list[str]:
    normalized = " ".join(str(prompt or "").split())
    if not normalized:
        return []
    needles: list[str] = []
    batch_match = re.search(r"\[cccc\]\s+Browser batch\s+([^\s]+)", normalized)
    if batch_match:
        needles.append(f"Browser batch {batch_match.group(1)}")
    event_match = re.search(r"\bevents=([0-9a-fA-F,]{12,})", normalized)
    if event_match:
        needles.append(f"events={event_match.group(1)}")
    if needles:
        return needles
    fallback = normalized[:120] if len(normalized) >= 24 else normalized
    if fallback:
        needles.append(fallback)
    deduped: list[str] = []
    for item in needles:
        text = str(item or "").strip()
        if text and text not in deduped:
            deduped.append(text)
    return deduped


def _chatgpt_running_visible(page: Any) -> bool:
    try:
        return bool(
            page.evaluate(
                """() => {
                    const isVisible = (node) => {
                        if (!node) return false;
                        const rect = node.getBoundingClientRect();
                        const style = window.getComputedStyle(node);
                        return rect.width > 0 && rect.height > 0 && style.visibility !== "hidden" && style.display !== "none";
                    };
                    const labelOf = (node) => [
                        node.getAttribute("aria-label") || "",
                        node.getAttribute("title") || "",
                        node.getAttribute("data-testid") || "",
                        node.id || "",
                        node.innerText || node.textContent || "",
                    ].join(" ").replace(/\\s+/g, " ").trim().toLowerCase();
                    const selectors = [
                        '[data-testid="stop-button"]',
                        '[data-testid*="stop" i]',
                        'button[aria-label*="Stop" i]',
                        'button[aria-label*="停止"]',
                        'button[aria-label*="中止"]',
                        'button[title*="Stop" i]',
                        'button[title*="停止"]',
                        '[role="button"][aria-label*="Stop" i]',
                        '[role="button"][aria-label*="停止"]',
                        '[role="button"][aria-label*="中止"]',
                    ];
                    for (const selector of selectors) {
                        try {
                            if (Array.from(document.querySelectorAll(selector)).some(isVisible)) return true;
                        } catch (_error) {}
                    }
                    for (const button of Array.from(document.querySelectorAll("button, [role='button']"))) {
                        if (!isVisible(button)) continue;
                        if (/\\bstop\\b|停止|中止/.test(labelOf(button))) return true;
                    }
                    return false;
                }"""
            )
        )
    except Exception:
        pass
    try:
        return bool(page.locator('[data-testid="stop-button"]').first.is_visible(timeout=150))
    except Exception:
        return False


def _wait_for_submission(
    page: Any,
    selector: str,
    *,
    prompt: str,
    timeout_seconds: float = 8.0,
) -> str:
    deadline = time.time() + max(0.2, float(timeout_seconds))
    stop_seen = False
    while time.time() < deadline:
        if _submission_echo_found(page, prompt):
            return "message_echo"
        if _chatgpt_running_visible(page):
            stop_seen = True
        time.sleep(0.15)
    return "stop_without_echo" if stop_seen else ""


def _raise_submission_unverified(page: Any, selector: str, *, attempted_action: str) -> None:
    diagnostics = _submission_diagnostics(page, selector)
    raise RuntimeError(
        "ChatGPT submit action was attempted but submission could not be verified; "
        "submission_verification=ambiguous; "
        f"attempted_action={attempted_action}; "
        f"diagnostics={json.dumps(diagnostics, ensure_ascii=False, separators=(',', ':'))[:1200]}"
    )


def _wait_after_submit_action(
    page: Any,
    selector: str,
    prompt: str,
    action: str,
    *,
    timeout_seconds: float,
) -> dict[str, Any] | None:
    evidence = _wait_for_submission(page, selector, prompt=prompt, timeout_seconds=timeout_seconds)
    if evidence == "message_echo":
        return {"input_selector": selector, "send_selector": action, "submission_evidence": evidence}
    if evidence == "stop_without_echo":
        return {"input_selector": selector, "send_selector": action, "submission_evidence": "running_without_echo"}
    if not _prompt_present_in_any_composer(page, prompt, selector):
        return {"input_selector": selector, "send_selector": action, "submission_evidence": "composer_cleared"}
    return None


def _remaining_submit_timeout(deadline: float, *, maximum: float = 20.0, reserve_seconds: float = 0.75) -> float:
    remaining = float(deadline) - time.time() - max(0.0, float(reserve_seconds))
    if remaining <= 0:
        return 0.0
    return min(float(maximum), remaining)


def _submit_prompt(
    page: Any,
    prompt: str,
    *,
    input_timeout_seconds: float,
    submit_timeout_seconds: float | None = None,
) -> dict[str, Any]:
    submit_budget = max(1.0, float(submit_timeout_seconds if submit_timeout_seconds is not None else input_timeout_seconds))
    submit_deadline = time.time() + submit_budget

    def _wait_after_action(action: str) -> dict[str, Any] | None:
        wait_seconds = _remaining_submit_timeout(submit_deadline)
        if wait_seconds <= 0:
            return None
        return _wait_after_submit_action(page, selector, prompt, action, timeout_seconds=wait_seconds)

    _raise_if_chatgpt_running_before_submit(page)
    selector = _visible_input_selector(page, timeout_seconds=min(float(input_timeout_seconds), submit_budget))
    try:
        _clear_and_type_prompt(page, selector, prompt)
    except Exception as exc:
        first_error = str(exc)
        retry_timeout = min(3.0, max(1.0, _remaining_submit_timeout(submit_deadline, maximum=3.0, reserve_seconds=0.25)))
        selector = _visible_input_selector(page, timeout_seconds=retry_timeout)
        try:
            _clear_and_type_prompt(page, selector, prompt)
        except Exception as retry_exc:
            raise RuntimeError(
                "ChatGPT composer input was found but could not be focused; "
                f"first_error={first_error[:400]}; retry_error={str(retry_exc)[:400]}"
            ) from retry_exc
    insert_timeout = _remaining_submit_timeout(submit_deadline, maximum=3.0, reserve_seconds=0.25)
    if not _wait_for_prompt_inserted(page, selector, prompt, timeout_seconds=max(0.2, insert_timeout)):
        raise RuntimeError("ChatGPT prompt insertion did not stick")
    settle_seconds = min(0.5, max(0.0, _remaining_submit_timeout(submit_deadline, maximum=0.5, reserve_seconds=0.25)))
    if settle_seconds > 0:
        time.sleep(settle_seconds)
    _raise_if_chatgpt_running_before_submit(page)
    send_selector = ""
    try:
        click_timeout = _remaining_submit_timeout(submit_deadline, maximum=5.0, reserve_seconds=0.25)
        if click_timeout > 0:
            send_selector = _click_send(page, timeout_seconds=max(0.5, click_timeout))
    except _UnsafeSubmitState:
        raise
    except Exception:
        send_selector = ""
    if send_selector:
        result = _wait_after_action(send_selector)
        if result is not None:
            return result
        _raise_submission_unverified(page, selector, attempted_action=send_selector)
    _raise_if_chatgpt_running_before_submit(page)
    request_submit = _request_submit_composer(page)
    if request_submit:
        result = _wait_after_action(request_submit)
        if result is not None:
            return result
        _raise_submission_unverified(page, selector, attempted_action=request_submit)
    _raise_if_chatgpt_running_before_submit(page)
    page.keyboard.press("Enter")
    result = _wait_after_action("keyboard:Enter")
    if result is not None:
        return result
    _raise_submission_unverified(page, selector, attempted_action="keyboard:Enter")


def _inspect_chatgpt_browser(
    cdp_port: int,
    *,
    bring_to_front: bool = False,
    ensure_page: bool = False,
    input_timeout_seconds: float = 1.5,
) -> dict[str, Any]:
    sync_playwright = ensure_sync_playwright()
    with sync_playwright() as pw:
        browser = pw.chromium.connect_over_cdp(
            f"http://127.0.0.1:{int(cdp_port)}",
            timeout=CDP_CONNECT_TIMEOUT_MS,
        )
        contexts = list(getattr(browser, "contexts", []) or [])
        context = contexts[0] if contexts else browser.new_context()
        pages = list(getattr(context, "pages", []) or [])
        page = next((item for item in pages if _normalize_chatgpt_url(str(item.url or ""))), None)
        fallback_url = str(getattr(pages[0], "url", "") or "") if pages else ""
        if page is None and ensure_page:
            page = context.new_page()
            page.goto(CHATGPT_URL, wait_until="domcontentloaded", timeout=30000)
        if page is None:
            return {
                "tab_url": fallback_url,
                "ready": False,
                "login_required": True,
                "message": "ChatGPT sign-in is required" if fallback_url else "ChatGPT tab is not open",
            }
        if bring_to_front:
            page.bring_to_front()
        if not _normalize_chatgpt_url(str(page.url or "")):
            page.goto(CHATGPT_URL, wait_until="domcontentloaded", timeout=30000)
        selector = ""
        ready = False
        try:
            selector = _visible_input_selector(page, timeout_seconds=input_timeout_seconds)
            ready = True
        except Exception:
            ready = False
        return {
            "tab_url": str(page.url or ""),
            "ready": ready,
            "login_required": not ready,
            "input_selector": selector,
        }


def _combined_session_state(actor_state: dict[str, Any], browser_state: dict[str, Any]) -> dict[str, Any]:
    out = dict(actor_state or {})
    for key in ("pid", "cdp_port", "browser_binary", "profile_dir", "visibility", "started_at"):
        if key in browser_state:
            out[key] = browser_state.get(key)
    return out


def _health_next_action(recommended: str, label: str, reason: str) -> dict[str, str]:
    return {
        "recommended": str(recommended or "none").strip() or "none",
        "label": str(label or "").strip(),
        "reason": str(reason or "").strip(),
    }


def _delivery_timeout_seconds(session: dict[str, Any]) -> float:
    raw = session.get("last_delivery_timeout_seconds")
    if raw in (None, ""):
        raw = os.environ.get("CCCC_WEB_MODEL_BROWSER_DELIVERY_TIMEOUT_SECONDS")
    try:
        value = float(raw)
    except Exception:
        value = DEFAULT_BROWSER_DELIVERY_TIMEOUT_SECONDS
    return max(5.0, min(value, 3600.0))


def _seconds_since_iso(ts: str) -> float | None:
    dt = parse_utc_iso(str(ts or "").strip())
    now_dt = parse_utc_iso(utc_now_iso())
    if dt is None or now_dt is None:
        return None
    return max(0.0, (now_dt - dt).total_seconds())


def _stale_submitting_delivery(session: dict[str, Any]) -> bool:
    started_at = str(session.get("last_delivery_started_at") or session.get("last_delivery_at") or "").strip()
    age = _seconds_since_iso(started_at)
    return age is not None and age >= _delivery_timeout_seconds(session)


def _chatgpt_delivery_target_snapshot(session: dict[str, Any]) -> dict[str, Any]:
    conversation_url = _conversation_url_from_tab(session.get("conversation_url"))
    pending_new_chat = bool(session.get("pending_new_chat_bind"))
    pending_submitted = bool(session.get("pending_new_chat_submitted"))
    pending_url = _normalize_chatgpt_url(session.get("pending_new_chat_url")) or CHATGPT_URL
    saved_at = str(
        session.get("target_saved_at")
        or session.get("new_chat_bound_at")
        or session.get("pending_new_chat_bind_started_at")
        or ""
    ).strip()
    if conversation_url:
        return {
            "state": "bound_existing_chat",
            "kind": "existing_chat",
            "url": conversation_url,
            "saved_at": saved_at,
            "next_delivery": "existing_chat",
            "label": "Existing ChatGPT chat",
            "detail": "Next delivery goes to the saved ChatGPT conversation URL.",
        }
    if pending_new_chat and pending_submitted:
        return {
            "state": "new_chat_submitted",
            "kind": "new_chat",
            "url": pending_url,
            "saved_at": saved_at,
            "submitted_at": str(session.get("pending_new_chat_submitted_at") or "").strip(),
            "delivery_id": str(session.get("pending_new_chat_delivery_id") or "").strip(),
            "next_delivery": "wait_for_new_chat_bind",
            "label": "Binding new ChatGPT chat",
            "detail": "The first prompt was submitted; CCCC is waiting for ChatGPT to expose the final /c/... URL.",
        }
    if pending_new_chat:
        return {
            "state": "new_chat_armed",
            "kind": "new_chat",
            "url": pending_url,
            "saved_at": saved_at,
            "next_delivery": "new_chat",
            "label": "New ChatGPT chat on next delivery",
            "detail": "Next delivery starts a fresh ChatGPT chat, then binds its final /c/... URL.",
        }
    return {
        "state": "none",
        "kind": "none",
        "url": "",
        "saved_at": saved_at,
        "next_delivery": "blocked",
        "label": "No target selected",
        "detail": "Save an existing ChatGPT conversation or choose new-chat delivery.",
    }


def build_chatgpt_web_model_health_snapshot(
    *,
    group_id: str,
    actor_id: str,
    browser_session: dict[str, Any],
    browser_surface: dict[str, Any] | None = None,
) -> dict[str, Any]:
    """Build a read-only operational summary from existing Web Model browser state.

    This intentionally does not introduce a new state source. It translates the
    browser/session/delivery fields already persisted by the ChatGPT Web Model
    path into one UI- and agent-readable snapshot.
    """

    session = dict(browser_session or {})
    surface = dict(browser_surface or {})
    surface_error = surface.get("error") if isinstance(surface.get("error"), dict) else {}
    surface_error_text = str(surface_error.get("message") or surface.get("message") or "").strip()
    session_error = str(session.get("error") or "").strip()
    last_error = str(session.get("last_error") or "").strip()
    surface_state = str(surface.get("state") or "").strip().lower()
    url = str(
        session.get("tab_url")
        or surface.get("url")
        or session.get("last_tab_url")
        or session.get("conversation_url")
        or ""
    ).strip()

    active = bool(session.get("active") or surface.get("active"))
    ready = bool(session.get("ready"))
    login_required = bool(session.get("login_required"))
    if surface_state == "failed" or session_error:
        browser_state = "failed"
        browser_label = "Check failed"
        browser_reason = session_error or surface_error_text or "ChatGPT browser check failed."
    elif ready:
        browser_state = "ready"
        browser_label = "Ready"
        browser_reason = "Signed in and reachable."
    elif active and login_required:
        browser_state = "sign_in_required"
        browser_label = "Needs sign-in"
        browser_reason = "Open ChatGPT and sign in with this browser profile."
    elif active:
        browser_state = "open"
        browser_label = "Open"
        browser_reason = "Browser is open; ChatGPT readiness is not confirmed yet."
    else:
        browser_state = "closed"
        browser_label = "Not open"
        browser_reason = "Open ChatGPT to sign in or inspect the page."

    delivery_target = _chatgpt_delivery_target_snapshot(session)
    conversation_url = str(session.get("conversation_url") or "").strip()
    pending_new_chat = bool(session.get("pending_new_chat_bind"))
    pending_new_chat_url = str(session.get("pending_new_chat_url") or "").strip()
    if conversation_url:
        target_state = "bound"
        target_label = str(delivery_target.get("label") or "Bound chat")
        target_reason = str(delivery_target.get("detail") or "Browser delivery targets the bound ChatGPT conversation.")
        target_url = conversation_url
    elif pending_new_chat:
        target_state = "new_chat_pending"
        target_label = str(delivery_target.get("label") or "New chat pending")
        target_reason = str(delivery_target.get("detail") or "Next delivery creates or finishes binding a ChatGPT conversation.")
        target_url = pending_new_chat_url or CHATGPT_URL
    else:
        target_state = "missing"
        target_label = str(delivery_target.get("label") or "No target selected")
        target_reason = str(delivery_target.get("detail") or "Save an existing ChatGPT chat or choose new-chat delivery.")
        target_url = ""

    raw_delivery_status = str(session.get("last_delivery_status") or "").strip().lower()
    last_delivery_at = str(session.get("last_delivery_at") or "").strip()
    pending_bind_delivery = raw_delivery_status == "pending" or (
        not conversation_url
        and pending_new_chat
        and (bool(session.get("pending_new_chat_submitted")) or last_error == "conversation_url_pending")
    )
    stale_submitting_delivery = raw_delivery_status == "submitting" and _stale_submitting_delivery(session)
    if pending_bind_delivery:
        delivery_state = "pending_bind"
        delivery_label = "Binding chat"
        delivery_reason = "Prompt was submitted; waiting for ChatGPT to assign the chat URL."
    elif stale_submitting_delivery:
        delivery_state = "failed"
        delivery_label = "Delivery interrupted"
        delivery_reason = "Browser delivery started but did not finish before the submit timeout."
        last_error = last_error or "delivery_submitting_stale"
    elif raw_delivery_status == "submitting":
        delivery_state = "submitting"
        delivery_label = "Submitting"
        delivery_reason = "CCCC is currently injecting this batch into the ChatGPT browser session."
    elif raw_delivery_status == "ambiguous":
        delivery_state = "ambiguous"
        delivery_label = "Delivery unverified"
        delivery_reason = last_error or "CCCC attempted to submit the prompt, but could not verify whether ChatGPT accepted it."
    elif raw_delivery_status == "failed":
        delivery_state = "failed"
        delivery_label = "Delivery failed"
        delivery_reason = last_error or "The last ChatGPT delivery did not complete."
    elif raw_delivery_status == "bound":
        delivery_state = "bound"
        delivery_label = "Chat bound"
        delivery_reason = "The submitted prompt has been bound to a ChatGPT conversation."
    elif raw_delivery_status == "submitted" or last_delivery_at:
        delivery_state = "submitted"
        delivery_label = "Submitted"
        delivery_reason = str(session.get("last_submission_evidence") or "").strip() or "The last browser delivery was submitted."
    else:
        delivery_state = "idle"
        delivery_label = "No recent delivery"
        delivery_reason = "No browser delivery has been recorded yet."

    if browser_state == "failed":
        next_action = _health_next_action("restart_browser", "Restart ChatGPT browser", browser_reason)
    elif browser_state == "closed":
        next_action = _health_next_action("open_chatgpt", "Open ChatGPT", browser_reason)
    elif browser_state == "sign_in_required":
        next_action = _health_next_action("login_chatgpt", "Sign in to ChatGPT", browser_reason)
    elif delivery_state == "pending_bind":
        next_action = _health_next_action("wait_for_chat_bind", "Wait for ChatGPT chat binding", delivery_reason)
    elif delivery_state == "submitting":
        next_action = _health_next_action("none", "Submitting to ChatGPT", delivery_reason)
    elif target_state == "missing":
        next_action = _health_next_action("bind_chat", "Choose a target ChatGPT chat", target_reason)
    elif delivery_state == "ambiguous":
        next_action = _health_next_action("inspect_error", "Inspect ChatGPT delivery", delivery_reason)
    elif delivery_state == "failed":
        next_action = _health_next_action("retry_delivery", "Retry or reload ChatGPT delivery", delivery_reason)
    else:
        next_action = _health_next_action("none", "No action needed", "ChatGPT Web Model is ready for browser delivery.")

    if delivery_state == "failed" or browser_state == "failed":
        tone = "error"
    elif str(next_action.get("recommended") or "none") != "none":
        tone = "needs"
    elif browser_state == "ready" and target_state in {"bound", "new_chat_pending"}:
        tone = "ready"
    else:
        tone = "neutral"

    return {
        "schema": "cccc.web_model.health.v1",
        "group_id": str(group_id or "").strip(),
        "actor_id": str(actor_id or "").strip(),
        "tone": tone,
        "summary": str(next_action.get("label") or "").strip(),
        "browser": {
            "state": browser_state,
            "label": browser_label,
            "reason": browser_reason,
            "active": active,
            "ready": ready,
            "logged_in_guess": ready,
            "url": url,
            "viewer_attached": bool(surface.get("controller_attached")),
            "last_frame_at": str(surface.get("last_frame_at") or ""),
        },
        "target": {
            "state": target_state,
            "label": target_label,
            "reason": target_reason,
            "url": target_url,
            "saved_at": str(delivery_target.get("saved_at") or ""),
            "next_delivery": str(delivery_target.get("next_delivery") or ""),
        },
        "delivery_target": delivery_target,
        "delivery": {
            "state": delivery_state,
            "label": delivery_label,
            "reason": delivery_reason,
            "last_delivery_id": str(session.get("last_delivery_id") or ""),
            "last_turn_id": str(session.get("last_turn_id") or ""),
            "last_event_ids": session.get("last_event_ids") if isinstance(session.get("last_event_ids"), list) else [],
            "last_delivery_at": last_delivery_at,
            "last_submission_evidence": str(session.get("last_submission_evidence") or ""),
            "last_send_selector": str(session.get("last_send_selector") or ""),
            "last_error": "" if delivery_state == "pending_bind" and last_error == "conversation_url_pending" else last_error,
            "cursor_committed": delivery_state in {"submitted", "bound", "pending_bind", "ambiguous"},
        },
        "next_action": next_action,
    }


def _session_payload(
    state: dict[str, Any],
    inspection: dict[str, Any] | None = None,
    *,
    check_alive: bool = True,
) -> dict[str, Any]:
    port = int(state.get("cdp_port") or 0)
    alive = port > 0 and (not check_alive or _wait_cdp_endpoint(port, timeout_seconds=0.4))
    payload = {
        "active": alive,
        "pid": int(state.get("pid") or 0),
        "cdp_port": port,
        "profile_dir": str(chatgpt_browser_profile_dir("", "")),
        "visibility": _normalize_visibility(str(state.get("visibility") or "visible")),
        "started_at": str(state.get("started_at") or ""),
        "updated_at": str(state.get("updated_at") or ""),
        "last_delivery_at": str(state.get("last_delivery_at") or ""),
        "last_delivery_started_at": str(state.get("last_delivery_started_at") or ""),
        "last_delivery_id": str(state.get("last_delivery_id") or ""),
        "last_delivery_timeout_seconds": _delivery_timeout_seconds(state),
        "last_delivery_status": str(state.get("last_delivery_status") or ""),
        "last_submission_evidence": str(state.get("last_submission_evidence") or ""),
        "last_send_selector": str(state.get("last_send_selector") or ""),
        "last_turn_id": str(state.get("last_turn_id") or ""),
        "last_event_ids": state.get("last_event_ids") if isinstance(state.get("last_event_ids"), list) else [],
        "last_tab_url": str(state.get("last_tab_url") or ""),
        "conversation_url": str(state.get("conversation_url") or ""),
        "pending_new_chat_bind": bool(state.get("pending_new_chat_bind")),
        "pending_new_chat_url": str(state.get("pending_new_chat_url") or ""),
        "pending_new_chat_bind_started_at": str(state.get("pending_new_chat_bind_started_at") or ""),
        "pending_new_chat_submitted": bool(state.get("pending_new_chat_submitted")),
        "pending_new_chat_submitted_at": str(state.get("pending_new_chat_submitted_at") or ""),
        "pending_new_chat_delivery_id": str(state.get("pending_new_chat_delivery_id") or ""),
        "pending_new_chat_last_turn_id": str(state.get("pending_new_chat_last_turn_id") or ""),
        "pending_new_chat_last_event_ids": state.get("pending_new_chat_last_event_ids")
        if isinstance(state.get("pending_new_chat_last_event_ids"), list)
        else [],
        "pending_new_chat_last_tab_url": str(state.get("pending_new_chat_last_tab_url") or ""),
        "new_chat_bound_at": str(state.get("new_chat_bound_at") or ""),
        "target_saved_at": str(state.get("target_saved_at") or ""),
        "bootstrap_seed_delivered_at": str(state.get("bootstrap_seed_delivered_at") or ""),
        "auto_reload_active": bool(state.get("auto_reload_active")),
        "auto_reload_window_started_at": str(state.get("auto_reload_window_started_at") or ""),
        "auto_reload_window_expires_at": str(state.get("auto_reload_window_expires_at") or ""),
        "auto_reload_last_progress_at": str(state.get("auto_reload_last_progress_at") or ""),
        "auto_reload_last_progress_reason": str(state.get("auto_reload_last_progress_reason") or ""),
        "auto_reload_last_progress_detail": str(state.get("auto_reload_last_progress_detail") or ""),
        "auto_reload_last_delivery_id": str(state.get("auto_reload_last_delivery_id") or ""),
        "auto_reload_last_turn_id": str(state.get("auto_reload_last_turn_id") or ""),
        "auto_reload_last_event_ids": state.get("auto_reload_last_event_ids") if isinstance(state.get("auto_reload_last_event_ids"), list) else [],
        "auto_reload_target_url": str(state.get("auto_reload_target_url") or ""),
        "auto_reload_last_reload_at": str(state.get("auto_reload_last_reload_at") or ""),
        "auto_reload_last_reload_reason": str(state.get("auto_reload_last_reload_reason") or ""),
        "auto_reload_last_page_url": str(state.get("auto_reload_last_page_url") or ""),
        "auto_reload_count": int(state.get("auto_reload_count") or 0),
        "auto_reload_completed_at": str(state.get("auto_reload_completed_at") or ""),
        "auto_reload_completed_reason": str(state.get("auto_reload_completed_reason") or ""),
        "auto_reload_expired_at": str(state.get("auto_reload_expired_at") or ""),
        "auto_reload_last_error": str(state.get("auto_reload_last_error") or ""),
        "ready": False,
        "login_required": False,
    }
    if inspection:
        payload.update(inspection)
    payload["delivery_target"] = _chatgpt_delivery_target_snapshot(payload)
    return payload


def chatgpt_browser_session_cached_status(group_id: str, actor_id: str) -> dict[str, Any]:
    """Return persisted ChatGPT browser state without inspecting the page.

    The cheap CDP liveness check prevents a stale persisted port from being
    surfaced as an active sign-in-required browser after daemon restart.
    """

    actor_state = read_chatgpt_browser_state(group_id, actor_id)
    browser_state = read_chatgpt_browser_process_state()
    state = _combined_session_state(actor_state, browser_state)
    return _session_payload(state)


def chatgpt_browser_session_status(group_id: str, actor_id: str) -> dict[str, Any]:
    resolution: dict[str, Any] = {}
    try:
        resolution = resolve_pending_chatgpt_conversation(group_id, actor_id)
    except Exception:
        pass
    actor_state = read_chatgpt_browser_state(group_id, actor_id)
    browser_state = read_chatgpt_browser_process_state()
    state = _combined_session_state(actor_state, browser_state)
    port = int(browser_state.get("cdp_port") or 0)
    if port <= 0 or not _wait_cdp_endpoint(port, timeout_seconds=0.4):
        payload = _session_payload(state)
        if bool(resolution.get("resolved")):
            payload["pending_new_chat_resolution"] = dict(resolution)
        return payload
    try:
        inspection = _inspect_chatgpt_browser(port, input_timeout_seconds=0.8)
    except Exception as exc:
        inspection = {"ready": False, "login_required": True, "error": str(exc)[:1000]}
    payload = _session_payload(state, inspection)
    if bool(resolution.get("resolved")):
        payload["pending_new_chat_resolution"] = dict(resolution)
    return payload


def _record_pending_new_chat_bound(state_root: Path, state: dict[str, Any], conversation_url: str) -> dict[str, Any]:
    normalized = _conversation_url_from_tab(conversation_url)
    if not normalized:
        return {"ok": False, "resolved": False, "error": "invalid_conversation_url"}
    now = utc_now_iso()
    pending_url = _normalize_chatgpt_url(state.get("pending_new_chat_url")) or CHATGPT_URL
    seed_url = _normalize_chatgpt_url(state.get("bootstrap_seed_conversation_url"))
    pending_delivery_id = str(state.get("pending_new_chat_delivery_id") or "").strip()
    pending_turn_id = str(state.get("pending_new_chat_last_turn_id") or "").strip()
    pending_event_ids = [
        str(item or "").strip()
        for item in (state.get("pending_new_chat_last_event_ids") if isinstance(state.get("pending_new_chat_last_event_ids"), list) else [])
        if str(item or "").strip()
    ]
    update = {
        "conversation_url": normalized,
        "pending_new_chat_bind": False,
        "pending_new_chat_url": "",
        "pending_new_chat_bind_started_at": "",
        "pending_new_chat_submitted": False,
        "pending_new_chat_submitted_at": "",
        "pending_new_chat_delivery_id": "",
        "pending_new_chat_last_turn_id": "",
        "pending_new_chat_last_event_ids": [],
        "pending_new_chat_last_tab_url": "",
        "new_chat_bound_at": now,
        "target_saved_at": now,
        "last_tab_url": normalized,
        "last_delivery_status": "bound",
        "last_delivery_id": pending_delivery_id or str(state.get("last_delivery_id") or "").strip(),
        "last_turn_id": pending_turn_id or str(state.get("last_turn_id") or "").strip(),
        "last_event_ids": pending_event_ids or (state.get("last_event_ids") if isinstance(state.get("last_event_ids"), list) else []),
        "last_delivery_at": now,
        "last_error": "",
    }
    if seed_url and seed_url == pending_url and str(state.get("bootstrap_seed_delivered_at") or "").strip():
        update["bootstrap_seed_conversation_url"] = normalized
    _write_state(state_root, {**state, **update})
    return {
        "ok": True,
        "resolved": True,
        "conversation_url": normalized,
        "target_url": pending_url,
        "delivery_id": pending_delivery_id,
        "turn_id": pending_turn_id,
        "event_ids": pending_event_ids,
        "submitted_at": str(state.get("pending_new_chat_submitted_at") or "").strip(),
        "bound_at": now,
    }


def _page_pending_delivery_id(page: Any) -> str:
    try:
        value = page.evaluate("() => sessionStorage.getItem('cccc_pending_new_chat_delivery_id') || ''")
    except Exception:
        return ""
    return str(value or "").strip()


def _mark_page_pending_delivery(page: Any, delivery_id: str) -> None:
    token = str(delivery_id or "").strip()
    if not token:
        return
    try:
        page.evaluate("value => sessionStorage.setItem('cccc_pending_new_chat_delivery_id', value)", token)
    except Exception:
        pass


def resolve_pending_chatgpt_conversation(group_id: str, actor_id: str) -> dict[str, Any]:
    """Resolve a previously submitted new ChatGPT chat once ChatGPT assigns /c/..."""
    state_root = chatgpt_browser_actor_state_root(group_id, actor_id)
    state = _load_state(state_root)
    if not bool(state.get("pending_new_chat_bind")):
        conversation_url = _conversation_url_from_tab(state.get("conversation_url"))
        return {"ok": True, "resolved": bool(conversation_url), "conversation_url": conversation_url, "pending": False}
    if not bool(state.get("pending_new_chat_submitted")):
        return {"ok": True, "resolved": False, "pending": True, "submitted": False}
    candidates = (
        state.get("conversation_url"),
        state.get("pending_new_chat_last_tab_url"),
    )
    for candidate in candidates:
        conversation_url = _conversation_url_from_tab(candidate)
        if conversation_url:
            return _record_pending_new_chat_bound(state_root, state, conversation_url)
    browser_state = read_chatgpt_browser_process_state()
    port = int(browser_state.get("cdp_port") or 0)
    if port <= 0 or not _wait_cdp_endpoint(port, timeout_seconds=0.4):
        return {"ok": True, "resolved": False, "pending": True, "submitted": True, "browser_active": False}
    expected_delivery_id = str(state.get("pending_new_chat_delivery_id") or "").strip()
    sync_playwright = ensure_sync_playwright()
    with sync_playwright() as pw:
        browser = pw.chromium.connect_over_cdp(
            f"http://127.0.0.1:{port}",
            timeout=CDP_CONNECT_TIMEOUT_MS,
        )
        contexts = list(getattr(browser, "contexts", []) or [])
        matching: list[str] = []
        fallback: list[str] = []
        for context in contexts:
            for page in list(getattr(context, "pages", []) or []):
                page_url = str(getattr(page, "url", "") or "")
                conversation_url = _conversation_url_from_tab(page_url)
                if not conversation_url:
                    continue
                if expected_delivery_id and _page_pending_delivery_id(page) == expected_delivery_id:
                    matching.append(conversation_url)
                else:
                    fallback.append(conversation_url)
        unique_matching = list(dict.fromkeys(matching))
        if len(unique_matching) == 1:
            return _record_pending_new_chat_bound(state_root, state, unique_matching[0])
        unique_fallback = list(dict.fromkeys(fallback))
        if not expected_delivery_id and len(unique_fallback) == 1:
            return _record_pending_new_chat_bound(state_root, state, unique_fallback[0])
    return {
        "ok": True,
        "resolved": False,
        "pending": True,
        "submitted": True,
        "browser_active": True,
        "ambiguous": True,
    }


def close_chatgpt_browser_session(group_id: str, actor_id: str) -> dict[str, Any]:
    actor_state = read_chatgpt_browser_state(group_id, actor_id)
    state = read_chatgpt_browser_process_state()
    _stop_browser_state(state)
    next_state = {**state, "pid": 0, "cdp_port": 0}
    record_chatgpt_browser_process_state(next_state)
    return _session_payload(_combined_session_state(actor_state, next_state))
