"""One-shot browser delivery sidecar for CCCC web_model actors.

The daemon calls this command with one JSON payload on stdin. The sidecar owns
only browser prompt submission; the website model still reports results through
the CCCC remote MCP connector.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import shutil
import subprocess
import sys
import time
import uuid
from pathlib import Path
from typing import Any, Optional
from urllib.parse import urlsplit, urlunsplit

from ..daemon.browser.projected_browser_runtime import (
    _pick_free_port,
    _system_browser_binaries,
    _wait_cdp_endpoint,
    ensure_dir,
    ensure_sync_playwright,
)
from ..paths import ensure_home
from ..util.process import pid_is_alive, terminate_pid
from ..util.fs import atomic_write_json, read_json
from ..util.time import utc_now_iso

CHATGPT_URL = "https://chatgpt.com/"
INPUT_SELECTORS = [
    'textarea[data-id="prompt-textarea"]',
    'textarea[placeholder*="Send a message"]',
    'textarea[aria-label="Message ChatGPT"]',
    "textarea:not([disabled])",
    'textarea[name="prompt-textarea"]',
    "#prompt-textarea",
    ".ProseMirror",
    '[contenteditable="true"][data-virtualkeyboard="true"]',
    '[contenteditable="true"]',
]
SEND_BUTTON_SELECTORS = [
    "#composer-submit-button",
    'button[data-testid="send-button"]',
    'button[data-testid="composer-submit-button"]',
    'button[data-testid*="composer-send"]',
    "button.composer-submit-btn",
    "button.composer-submit-button-color",
    'form button[type="submit"]',
    'button[type="submit"][data-testid*="send"]',
    'button[aria-label*="Send"]',
    'button[aria-label*="Send prompt"]',
    'button[aria-label*="发送"]',
    'button[aria-label*="送信"]',
]
TOOL_CONFIRM_MAX_CLICKS = 3


def _chatgpt_tool_confirm_script() -> str:
    return r"""
    (args) => {
        const maxClicks = Math.max(1, Math.min(Number(args?.maxClicks || 1), 3));
        const clickedRecentlyMs = 30000;
        const now = Date.now();
        const rejectLabels = new Set(["拒绝", "deny", "cancel", "取消"]);
        const sharedDataNeedles = [
            "共享数据包括",
            "shared data",
            "data shared",
            "will share",
            "share data",
            "shared with",
            "data includes"
        ];
        const detailsNeedles = ["详细信息", "details", "learn more", "more details"];

        const textOf = (node) => String(node?.innerText || node?.textContent || "").trim();
        const normalized = (value) => String(value || "").trim().replace(/\s+/g, " ");
        const labelKey = (value) => normalized(value).toLowerCase();
        const isVisible = (node) => {
            if (!node || !(node instanceof HTMLElement)) return false;
            const style = window.getComputedStyle(node);
            if (style.visibility === "hidden" || style.display === "none") return false;
            const rect = node.getBoundingClientRect();
            return rect.width > 0 && rect.height > 0;
        };
        const hasRejectButton = (root) => {
            for (const button of root.querySelectorAll("button")) {
                if (!isVisible(button)) continue;
                if (button.classList.contains("btn-secondary") && rejectLabels.has(labelKey(textOf(button)))) return true;
                if (rejectLabels.has(labelKey(textOf(button)))) return true;
            }
            return false;
        };
        const hasSharedDataText = (root) => {
            const text = labelKey(textOf(root));
            return sharedDataNeedles.some((needle) => text.includes(needle));
        };
        const hasDetailsControl = (root) => {
            const text = labelKey(textOf(root));
            if (detailsNeedles.some((needle) => text.includes(needle))) return true;
            for (const button of root.querySelectorAll("button")) {
                if (!isVisible(button)) continue;
                const label = labelKey(textOf(button));
                if (detailsNeedles.some((needle) => label.includes(needle))) return true;
            }
            return false;
        };
        const hasToolConfirmBody = (root) => {
            if (!root.querySelector("h2")) return false;
            if (hasSharedDataText(root)) return true;
            if (root.querySelector("p") && hasDetailsControl(root)) return true;
            return false;
        };
        const panelFor = (button) => {
            let node = button;
            for (let depth = 0; node && depth < 10; depth += 1, node = node.parentElement) {
                if (!(node instanceof HTMLElement)) continue;
                if (!isVisible(node)) continue;
                if (!hasToolConfirmBody(node)) continue;
                if (!hasRejectButton(node)) continue;
                return node;
            }
            return null;
        };

        const out = [];
        for (const button of document.querySelectorAll("button")) {
            if (out.length >= maxClicks) break;
            if (!isVisible(button)) continue;
            if (button.disabled || button.getAttribute("aria-disabled") === "true") continue;
            const label = labelKey(textOf(button));
            if (!button.classList.contains("btn-primary") && label !== "确认") continue;
            const clickedAt = Number(button.getAttribute("data-cccc-auto-confirm-clicked-at") || 0);
            if (clickedAt && now - clickedAt < clickedRecentlyMs) continue;
            const panel = panelFor(button);
            if (!panel) continue;
            const title = normalized(textOf(panel.querySelector("h2"))).slice(0, 160);
            const candidateId = `cccc-tool-confirm-${now}-${out.length}`;
            button.setAttribute("data-cccc-auto-confirm-candidate-id", candidateId);
            out.push({ candidate_id: candidateId, title, label: normalized(textOf(button)).slice(0, 32) });
        }
        return { clicked: 0, candidates: out, details: out };
    }
    """


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


def _default_delivery_visibility() -> str:
    if sys.platform.startswith("linux") and shutil.which("xvfb-run"):
        return "background"
    return "visible"


def _json_result(payload: dict[str, Any]) -> str:
    return json.dumps(payload, ensure_ascii=False, separators=(",", ":"))


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
            "bootstrap_seed_delivered_at": "",
            "bootstrap_seed_version": "",
            "bootstrap_seed_digest": "",
            "bootstrap_seed_conversation_url": "",
            "last_delivery_at": "",
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


def _browser_channel_candidates() -> list[str]:
    raw = str(os.environ.get("CCCC_WEB_MODEL_BROWSER_CHANNELS") or "").strip()
    if raw:
        return [item.strip() for item in raw.split(",") if item.strip()]
    channel = str(os.environ.get("CCCC_WEB_MODEL_BROWSER_CHANNEL") or "").strip()
    if channel:
        return [channel]
    return ["chrome", "msedge"]


def _browser_binary_candidates() -> list[str]:
    explicit = str(os.environ.get("CCCC_WEB_MODEL_BROWSER_BINARY") or "").strip()
    out: list[str] = []
    if explicit:
        out.append(explicit)
    for channel in _browser_channel_candidates():
        out.extend(_system_browser_binaries(channel))
    seen: set[str] = set()
    deduped: list[str] = []
    for item in out:
        key = str(item or "").strip()
        if not key or key in seen:
            continue
        seen.add(key)
        deduped.append(key)
    return deduped


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


def _browser_visibility_from_env(default: str = "visible") -> str:
    raw = (
        os.environ.get("CCCC_WEB_MODEL_BROWSER_VISIBILITY")
        or os.environ.get("CCCC_WEB_MODEL_BROWSER_MODE")
        or os.environ.get("CCCC_WEB_MODEL_BROWSER_HEADLESS")
        or default
    )
    if str(raw).strip().lower() in {"1", "true", "yes"}:
        return "headless"
    return _normalize_visibility(str(raw))


def _browser_launch_command(binary: str, profile_dir: Path, port: int, visibility: str) -> list[str]:
    normalized = _normalize_visibility(visibility)
    browser_cmd = [
        binary,
        f"--remote-debugging-port={int(port)}",
        f"--user-data-dir={profile_dir}",
        "--no-first-run",
        "--no-default-browser-check",
        "--new-window",
        CHATGPT_URL,
    ]
    if normalized == "headless":
        return [*browser_cmd[:4], "--headless=new", "--disable-gpu", *browser_cmd[4:]]
    if normalized == "background":
        xvfb = shutil.which("xvfb-run")
        if not xvfb:
            raise RuntimeError("background browser mode requires xvfb-run; install xvfb or use visible browser mode")
        return [xvfb, "-a", "-s", "-screen 0 1280x900x24", *browser_cmd]
    return browser_cmd


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


def _start_or_reuse_browser(profile_root: Path, *, visibility: str = "visible") -> dict[str, Any]:
    normalized_visibility = _normalize_visibility(visibility)
    state = _load_state(profile_root)
    port = int(state.get("cdp_port") or 0)
    pid = int(state.get("pid") or 0)
    if port > 0 and _wait_cdp_endpoint(port, timeout_seconds=0.7):
        prior_visibility = _normalize_visibility(str(state.get("visibility") or "visible"))
        if prior_visibility == "projected" or normalized_visibility == "visible" or prior_visibility == normalized_visibility:
            return {**state, "cdp_port": port, "pid": pid, "visibility": prior_visibility, "reused": True}
        _stop_browser_state(state)
        time.sleep(0.4)

    binaries = _browser_binary_candidates()
    if not binaries:
        raise RuntimeError(
            "no Chrome/Edge browser binary found; set CCCC_WEB_MODEL_BROWSER_BINARY "
            "or install Google Chrome/Microsoft Edge"
        )

    profile_dir = _profile_dir(profile_root)
    last_error = ""
    for binary in binaries:
        proc: subprocess.Popen[str] | None = None
        port = _pick_free_port()
        try:
            cmd = _browser_launch_command(binary, profile_dir, port, normalized_visibility)
            proc = subprocess.Popen(
                cmd,
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
                text=True,
                start_new_session=True,
            )
            if not _wait_cdp_endpoint(port, timeout_seconds=12.0):
                raise RuntimeError("CDP endpoint did not become ready")
            state = {
                "pid": int(proc.pid or 0),
                "cdp_port": int(port),
                "browser_binary": str(binary),
                "profile_dir": str(profile_dir),
                "visibility": normalized_visibility,
                "started_at": utc_now_iso(),
                "reused": False,
            }
            _write_state(profile_root, state)
            return state
        except Exception as exc:
            last_error = str(exc)
            try:
                if proc is not None and proc.poll() is None:
                    proc.terminate()
            except Exception:
                pass
            continue
    raise RuntimeError(last_error or "failed to start browser")


def _visible_input_selector(page: Any, *, timeout_seconds: float) -> str:
    deadline = time.time() + max(1.0, float(timeout_seconds))
    last_error = ""
    while time.time() < deadline:
        for selector in INPUT_SELECTORS:
            try:
                locator = page.locator(selector).first
                if locator.count() > 0 and locator.is_visible(timeout=250):
                    return selector
            except Exception as exc:
                last_error = str(exc)
        try:
            candidate = page.evaluate(
                """() => {
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
                            return !node.disabled && !node.readOnly && !/password|search|email|url|number|tel/.test(type);
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
                        if (rect.width >= 260 && rect.height >= 26) out += 20;
                        out += Math.min(180, Math.max(0, (rect.width * rect.height) / 2500));
                        out += Math.max(0, rect.y / 8);
                        return out;
                    };
                    const nodes = [
                        ...document.querySelectorAll("main textarea, main [role='textbox'], main [contenteditable='true']"),
                        ...document.querySelectorAll("textarea, input, [role='textbox'], [contenteditable='true']"),
                    ];
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
                }"""
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
        locator.fill(prompt, timeout=5000)
        return
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


def _wait_for_prompt_inserted(page: Any, selector: str, prompt: str, *, timeout_seconds: float = 3.0) -> bool:
    deadline = time.time() + max(0.5, float(timeout_seconds))
    while time.time() < deadline:
        if _prompt_inserted(page, selector, prompt):
            return True
        time.sleep(0.1)
    return _prompt_inserted(page, selector, prompt)


def _click_send(page: Any, *, timeout_seconds: float = 5.0) -> str:
    deadline = time.time() + max(0.5, float(timeout_seconds))
    last_error = ""
    while time.time() < deadline:
        for selector in SEND_BUTTON_SELECTORS:
            try:
                button = page.locator(selector).first
                if button.count() <= 0:
                    continue
                if not button.is_visible(timeout=250):
                    continue
                try:
                    if button.is_disabled(timeout=250):
                        continue
                except Exception:
                    pass
                button.click(timeout=5000)
                return selector
            except Exception as exc:
                last_error = str(exc)
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
                        if (node.matches("input")) return !node.disabled && !node.readOnly && !/password|search|email|url|number|tel/i.test(String(node.type || "text"));
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
            if str(candidate_selector or "").strip():
                page.locator(str(candidate_selector)).first.click(timeout=5000)
                return "scored:composer-submit"
        except Exception as exc:
            last_error = str(exc)
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
                const editable = (node) => {
                    if (!node || !isVisible(node)) return false;
                    if (node.matches("textarea")) return !node.disabled && !node.readOnly;
                    if (node.matches("input")) return !node.disabled && !node.readOnly && !/password|search|email|url|number|tel/i.test(String(node.type || "text"));
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
                    const label = [
                        button.getAttribute("aria-label") || "",
                        button.getAttribute("title") || "",
                        button.getAttribute("data-testid") || "",
                        button.innerText || button.textContent || "",
                    ].join(" ").toLowerCase();
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
                const stopVisible = Array.from(document.querySelectorAll('[data-testid="stop-button"], button[aria-label*="Stop" i], button[aria-label*="停止"]')).some(isVisible);
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


def _auto_confirm_page_tool_prompts(page: Any, *, max_clicks: int = TOOL_CONFIRM_MAX_CLICKS) -> dict[str, Any]:
    url = str(getattr(page, "url", "") or "")
    if not _normalize_chatgpt_url(url):
        return {"clicked": 0, "details": [], "skipped": "non_chatgpt_page"}
    try:
        result = page.evaluate(
            _chatgpt_tool_confirm_script(),
            {"maxClicks": max(1, min(int(max_clicks or 1), TOOL_CONFIRM_MAX_CLICKS))},
        )
    except Exception as exc:
        return {"clicked": 0, "details": [], "error": str(exc)[:1000]}
    if not isinstance(result, dict):
        return {"clicked": 0, "details": []}
    candidates = result.get("candidates") if isinstance(result.get("candidates"), list) else []
    details: list[dict[str, Any]] = []
    errors: list[dict[str, Any]] = []
    clicked = 0
    for raw in candidates[: max(1, min(int(max_clicks or 1), TOOL_CONFIRM_MAX_CLICKS))]:
        if not isinstance(raw, dict):
            continue
        candidate_id = str(raw.get("candidate_id") or "").strip()
        if not candidate_id:
            continue
        selector = f'button[data-cccc-auto-confirm-candidate-id="{candidate_id}"]'
        try:
            locator = page.locator(selector).first
            if locator.count() <= 0:
                continue
            locator.click(timeout=3000)
            clicked += 1
            details.append(
                {
                    "title": str(raw.get("title") or "")[:160],
                    "label": str(raw.get("label") or "")[:32],
                }
            )
            if clicked >= TOOL_CONFIRM_MAX_CLICKS:
                break
        except Exception as exc:
            errors.append(
                {
                    "title": str(raw.get("title") or "")[:160],
                    "error": str(exc)[:300],
                }
            )
    out: dict[str, Any] = {
        "clicked": max(0, clicked),
        "candidate_count": len(candidates),
        "details": details[:TOOL_CONFIRM_MAX_CLICKS],
    }
    if errors:
        out["errors"] = errors[:TOOL_CONFIRM_MAX_CLICKS]
    return out


def _submission_echo_found(page: Any, prompt: str) -> bool:
    normalized = " ".join(str(prompt or "").split())
    if not normalized:
        return False
    excerpt = normalized[:120]
    if len(excerpt) < 24:
        excerpt = normalized
    try:
        return bool(
            page.evaluate(
                """needle => {
                    const normalize = value => String(value || "").replace(/\\s+/g, " ").trim();
                    const target = normalize(needle);
                    if (!target) return false;
                    const candidates = [
                        ...document.querySelectorAll('[data-message-author-role="user"]'),
                        ...document.querySelectorAll('[data-testid*="conversation-turn"]'),
                        ...document.querySelectorAll('main article'),
                    ];
                    for (const node of candidates) {
                        if (normalize(node.innerText || node.textContent || "").includes(target)) return true;
                    }
                    return false;
                }""",
                excerpt,
            )
        )
    except Exception:
        return False


def _wait_for_submission(
    page: Any,
    selector: str,
    *,
    prompt: str,
    timeout_seconds: float = 8.0,
) -> str:
    deadline = time.time() + max(1.0, float(timeout_seconds))
    while time.time() < deadline:
        try:
            stop_visible = page.locator('[data-testid="stop-button"]').first.is_visible(timeout=150)
            if stop_visible:
                return "stop_button"
        except Exception:
            pass
        if not _composer_text(page, selector):
            if _submission_echo_found(page, prompt):
                return "message_echo"
        time.sleep(0.15)
    return ""


def _submit_prompt(page: Any, prompt: str, *, input_timeout_seconds: float) -> dict[str, Any]:
    selector = _visible_input_selector(page, timeout_seconds=input_timeout_seconds)
    _clear_and_type_prompt(page, selector, prompt)
    if not _wait_for_prompt_inserted(page, selector, prompt):
        raise RuntimeError("ChatGPT prompt insertion did not stick")
    time.sleep(0.5)
    send_selector = ""
    try:
        send_selector = _click_send(page)
    except Exception:
        send_selector = ""
    evidence = _wait_for_submission(page, selector, prompt=prompt)
    if evidence:
        return {"input_selector": selector, "send_selector": send_selector or "keyboard", "submission_evidence": evidence}
    request_submit = _request_submit_composer(page)
    if request_submit:
        evidence = _wait_for_submission(page, selector, prompt=prompt, timeout_seconds=4.0)
        if evidence:
            return {"input_selector": selector, "send_selector": request_submit, "submission_evidence": evidence}
    page.keyboard.press("Enter")
    evidence = _wait_for_submission(page, selector, prompt=prompt, timeout_seconds=4.0)
    if evidence:
        return {"input_selector": selector, "send_selector": send_selector or "keyboard:Enter", "submission_evidence": evidence}
    modifier = "Meta" if sys.platform == "darwin" else "Control"
    page.keyboard.press(f"{modifier}+Enter")
    evidence = _wait_for_submission(page, selector, prompt=prompt, timeout_seconds=4.0)
    if evidence:
        return {"input_selector": selector, "send_selector": send_selector or f"keyboard:{modifier}+Enter", "submission_evidence": evidence}
    diagnostics = _submission_diagnostics(page, selector)
    raise RuntimeError(
        "ChatGPT prompt was inserted but did not submit; "
        f"diagnostics={json.dumps(diagnostics, ensure_ascii=False, separators=(',', ':'))[:1200]}"
    )


def _inspect_chatgpt_browser(
    cdp_port: int,
    *,
    bring_to_front: bool = False,
    ensure_page: bool = False,
    input_timeout_seconds: float = 1.5,
) -> dict[str, Any]:
    sync_playwright = ensure_sync_playwright()
    with sync_playwright() as pw:
        browser = pw.chromium.connect_over_cdp(f"http://127.0.0.1:{int(cdp_port)}")
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

    conversation_url = str(session.get("conversation_url") or "").strip()
    pending_new_chat = bool(session.get("pending_new_chat_bind"))
    pending_new_chat_url = str(session.get("pending_new_chat_url") or "").strip()
    if conversation_url:
        target_state = "bound"
        target_label = "Bound chat"
        target_reason = "Browser delivery targets the bound ChatGPT conversation."
        target_url = conversation_url
    elif pending_new_chat:
        target_state = "new_chat_pending"
        target_label = "New chat pending"
        target_reason = "Next delivery creates or finishes binding a ChatGPT conversation."
        target_url = pending_new_chat_url or CHATGPT_URL
    else:
        target_state = "missing"
        target_label = "No target selected"
        target_reason = "Choose an existing ChatGPT chat or arm new-chat delivery."
        target_url = ""

    raw_delivery_status = str(session.get("last_delivery_status") or "").strip().lower()
    last_delivery_at = str(session.get("last_delivery_at") or "").strip()
    pending_bind_delivery = raw_delivery_status == "pending" or (
        not conversation_url
        and pending_new_chat
        and (bool(session.get("pending_new_chat_submitted")) or last_error == "conversation_url_pending")
    )
    if pending_bind_delivery:
        delivery_state = "pending_bind"
        delivery_label = "Binding chat"
        delivery_reason = "Prompt was submitted; waiting for ChatGPT to assign the chat URL."
    elif raw_delivery_status == "failed":
        delivery_state = "failed"
        delivery_label = "Delivery failed"
        delivery_reason = last_error or "The last ChatGPT delivery did not complete."
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
    elif target_state == "missing":
        next_action = _health_next_action("bind_chat", "Choose a target ChatGPT chat", target_reason)
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
        },
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
            "cursor_committed": delivery_state in {"submitted", "pending_bind"},
        },
        "next_action": next_action,
    }


def _session_payload(state: dict[str, Any], inspection: dict[str, Any] | None = None) -> dict[str, Any]:
    port = int(state.get("cdp_port") or 0)
    alive = port > 0 and _wait_cdp_endpoint(port, timeout_seconds=0.4)
    payload = {
        "active": alive,
        "pid": int(state.get("pid") or 0),
        "cdp_port": port,
        "profile_dir": str(chatgpt_browser_profile_dir("", "")),
        "visibility": _normalize_visibility(str(state.get("visibility") or "visible")),
        "started_at": str(state.get("started_at") or ""),
        "updated_at": str(state.get("updated_at") or ""),
        "last_delivery_at": str(state.get("last_delivery_at") or ""),
        "last_delivery_id": str(state.get("last_delivery_id") or ""),
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
        "bootstrap_seed_delivered_at": str(state.get("bootstrap_seed_delivered_at") or ""),
        "auto_confirm_scan_at": str(state.get("auto_confirm_scan_at") or ""),
        "auto_confirm_pages_seen": int(state.get("auto_confirm_pages_seen") or 0),
        "auto_confirm_candidate_count": int(state.get("auto_confirm_candidate_count") or 0),
        "auto_confirm_last_at": str(state.get("auto_confirm_last_at") or ""),
        "auto_confirm_last_count": int(state.get("auto_confirm_last_count") or 0),
        "auto_confirm_total": int(state.get("auto_confirm_total") or 0),
        "auto_confirm_last_page_url": str(state.get("auto_confirm_last_page_url") or ""),
        "auto_confirm_last_details": state.get("auto_confirm_last_details") if isinstance(state.get("auto_confirm_last_details"), list) else [],
        "auto_confirm_last_errors": state.get("auto_confirm_last_errors") if isinstance(state.get("auto_confirm_last_errors"), list) else [],
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
        "login_required": True,
    }
    if inspection:
        payload.update(inspection)
    return payload


def chatgpt_browser_session_status(group_id: str, actor_id: str) -> dict[str, Any]:
    try:
        resolve_pending_chatgpt_conversation(group_id, actor_id)
    except Exception:
        pass
    actor_state = read_chatgpt_browser_state(group_id, actor_id)
    browser_state = read_chatgpt_browser_process_state()
    state = _combined_session_state(actor_state, browser_state)
    port = int(browser_state.get("cdp_port") or 0)
    if port <= 0 or not _wait_cdp_endpoint(port, timeout_seconds=0.4):
        return _session_payload(state)
    try:
        inspection = _inspect_chatgpt_browser(port, input_timeout_seconds=0.8)
    except Exception as exc:
        inspection = {"ready": False, "login_required": True, "error": str(exc)[:1000]}
    return _session_payload(state, inspection)


def _record_pending_new_chat_bound(state_root: Path, state: dict[str, Any], conversation_url: str) -> dict[str, Any]:
    normalized = _conversation_url_from_tab(conversation_url)
    if not normalized:
        return {"ok": False, "resolved": False, "error": "invalid_conversation_url"}
    now = utc_now_iso()
    pending_url = _normalize_chatgpt_url(state.get("pending_new_chat_url")) or CHATGPT_URL
    seed_url = _normalize_chatgpt_url(state.get("bootstrap_seed_conversation_url"))
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
        "last_tab_url": normalized,
        "last_error": "",
    }
    if seed_url and seed_url == pending_url and str(state.get("bootstrap_seed_delivered_at") or "").strip():
        update["bootstrap_seed_conversation_url"] = normalized
    _write_state(state_root, {**state, **update})
    return {"ok": True, "resolved": True, "conversation_url": normalized}


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
        state.get("last_tab_url"),
        state.get("auto_confirm_last_page_url"),
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
        browser = pw.chromium.connect_over_cdp(f"http://127.0.0.1:{port}")
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


def open_chatgpt_browser_session(group_id: str, actor_id: str, *, visibility: str = "visible") -> dict[str, Any]:
    _ensure_shared_profile_migrated(group_id, actor_id)
    browser_root = chatgpt_browser_profile_root(group_id, actor_id)
    normalized_visibility = _normalize_visibility(visibility)
    state = _start_or_reuse_browser(browser_root, visibility=normalized_visibility)
    port = int(state.get("cdp_port") or 0)
    inspection = _inspect_chatgpt_browser(
        port,
        bring_to_front=normalized_visibility == "visible",
        ensure_page=True,
    )
    record_chatgpt_browser_process_state({**state, "last_tab_url": str(inspection.get("tab_url") or "")})
    record_chatgpt_browser_state(group_id, actor_id, {"last_tab_url": str(inspection.get("tab_url") or "")})
    actor_state = read_chatgpt_browser_state(group_id, actor_id)
    browser_state = read_chatgpt_browser_process_state()
    return _session_payload(_combined_session_state(actor_state, browser_state), inspection)


def close_chatgpt_browser_session(group_id: str, actor_id: str) -> dict[str, Any]:
    actor_state = read_chatgpt_browser_state(group_id, actor_id)
    state = read_chatgpt_browser_process_state()
    _stop_browser_state(state)
    next_state = {**state, "pid": 0, "cdp_port": 0}
    record_chatgpt_browser_process_state(next_state)
    return _session_payload(_combined_session_state(actor_state, next_state))


def submit_to_chatgpt(payload: dict[str, Any]) -> dict[str, Any]:
    prompt = str(payload.get("prompt") or "").strip()
    if not prompt:
        raise RuntimeError("payload.prompt is required")
    group_id = str(payload.get("group_id") or "").strip()
    actor_id = str(payload.get("actor_id") or "").strip()
    _ensure_shared_profile_migrated(group_id, actor_id)
    browser_root = chatgpt_browser_profile_root(group_id, actor_id)
    actor_state_root = chatgpt_browser_actor_state_root(group_id, actor_id)
    prior_state = _load_state(actor_state_root)
    visibility = _normalize_visibility(str(payload.get("browser_visibility") or "") or _browser_visibility_from_env(_default_delivery_visibility()))
    auto_bind_new_chat = bool(payload.get("auto_bind_new_chat"))
    pending_url = _normalize_chatgpt_url(prior_state.get("pending_new_chat_url")) if auto_bind_new_chat else ""
    target_url = (
        _normalize_chatgpt_url(payload.get("target_url"))
        or pending_url
        or _normalize_chatgpt_url(prior_state.get("conversation_url"))
    )
    browser_state = _start_or_reuse_browser(browser_root, visibility=visibility)
    cdp_port = int(browser_state.get("cdp_port") or 0)
    if cdp_port <= 0:
        raise RuntimeError("browser CDP port is unavailable")
    delivery_id = f"browser:{uuid.uuid4().hex}"

    sync_playwright = ensure_sync_playwright()
    with sync_playwright() as pw:
        browser = pw.chromium.connect_over_cdp(f"http://127.0.0.1:{cdp_port}")
        contexts = list(getattr(browser, "contexts", []) or [])
        context = contexts[0] if contexts else browser.new_context()
        pages = list(getattr(context, "pages", []) or [])
        page = None
        if target_url:
            page = next((_page for _page in pages if _normalize_chatgpt_url(getattr(_page, "url", "")) == target_url), None)
        if page is None:
            page = next((item for item in pages if _normalize_chatgpt_url(str(item.url or ""))), None)
        if page is None:
            page = context.new_page()
            page.goto(target_url or CHATGPT_URL, wait_until="domcontentloaded", timeout=30000)
        else:
            if visibility == "visible":
                page.bring_to_front()
            current_url = _normalize_chatgpt_url(str(page.url or ""))
            if target_url and current_url != target_url:
                page.goto(target_url, wait_until="domcontentloaded", timeout=30000)
            elif not _normalize_chatgpt_url(str(page.url or "")):
                page.goto(CHATGPT_URL, wait_until="domcontentloaded", timeout=30000)
        current_chatgpt_url = _normalize_chatgpt_url(str(page.url or ""))
        if not current_chatgpt_url:
            raise RuntimeError(f"ChatGPT sign-in required before delivery; current page is {str(page.url or '')[:200]}")
        if auto_bind_new_chat:
            _mark_page_pending_delivery(page, delivery_id)
        submit = _submit_prompt(
            page,
            prompt,
            input_timeout_seconds=float(os.environ.get("CCCC_WEB_MODEL_BROWSER_INPUT_TIMEOUT_SECONDS") or 30.0),
        )
        conversation_url = _conversation_url_from_tab(str(page.url or ""))
        if auto_bind_new_chat and not conversation_url:
            conversation_url = _wait_for_conversation_url(
                page,
                timeout_seconds=float(os.environ.get("CCCC_WEB_MODEL_NEW_CHAT_BIND_TIMEOUT_SECONDS") or 20.0),
            )
        tab_url = str(page.url or "")
        pending_conversation_url = bool(auto_bind_new_chat and not conversation_url)
        conversation_url = conversation_url or ("" if pending_conversation_url else target_url)

    now = utc_now_iso()
    pending_url_for_state = _normalize_chatgpt_url(target_url) or pending_url or CHATGPT_URL
    _write_state(
        actor_state_root,
        {
            **prior_state,
            **({"conversation_url": conversation_url} if conversation_url else {}),
            **(
                {
                    "pending_new_chat_bind": False,
                    "pending_new_chat_url": "",
                    "pending_new_chat_bind_started_at": "",
                    "pending_new_chat_submitted": False,
                    "pending_new_chat_submitted_at": "",
                    "pending_new_chat_delivery_id": "",
                    "pending_new_chat_last_turn_id": "",
                    "pending_new_chat_last_event_ids": [],
                    "pending_new_chat_last_tab_url": "",
                    "new_chat_bound_at": utc_now_iso(),
                    "last_error": "",
                }
                if auto_bind_new_chat and conversation_url
                else {}
            ),
            **(
                {
                    "conversation_url": "",
                    "pending_new_chat_bind": True,
                    "pending_new_chat_url": pending_url_for_state,
                    "pending_new_chat_submitted": True,
                    "pending_new_chat_submitted_at": now,
                    "pending_new_chat_delivery_id": delivery_id,
                    "pending_new_chat_last_turn_id": str(payload.get("turn_id") or ""),
                    "pending_new_chat_last_event_ids": list(payload.get("event_ids") or []),
                    "pending_new_chat_last_tab_url": tab_url,
                    "last_error": "conversation_url_pending",
                }
                if pending_conversation_url
                else {}
            ),
            "last_delivery_at": now,
            "last_turn_id": str(payload.get("turn_id") or ""),
            "last_event_ids": list(payload.get("event_ids") or []),
            "last_tab_url": tab_url,
            "last_delivery_id": delivery_id,
            "last_delivery_status": "pending" if pending_conversation_url else "submitted",
            "last_submission_evidence": str(submit.get("submission_evidence") or ""),
            "last_send_selector": str(submit.get("send_selector") or ""),
            **({"last_error": ""} if not pending_conversation_url else {}),
            **(
                {
                    "bootstrap_seed_delivered_at": now,
                    "bootstrap_seed_version": str(payload.get("bootstrap_seed_version") or ""),
                    "bootstrap_seed_digest": str(payload.get("bootstrap_seed_digest") or ""),
                    "bootstrap_seed_conversation_url": str(conversation_url or payload.get("bootstrap_seed_conversation_url") or target_url or ""),
                }
                if bool(payload.get("bootstrap_seed"))
                else {}
            ),
        },
    )
    record_chatgpt_browser_process_state({**browser_state, "last_tab_url": tab_url})
    return {
        "ok": True,
        "delivery_id": delivery_id,
        "browser": {
            "provider": str(payload.get("provider") or "chatgpt_web"),
            "tab_url": tab_url,
            "conversation_url": conversation_url,
            "auto_bind_new_chat": auto_bind_new_chat,
            "pending_conversation_url": pending_conversation_url,
            "submitted_without_conversation_url": pending_conversation_url,
            "profile_dir": str(chatgpt_browser_profile_dir(group_id, actor_id)),
            "cdp_port": cdp_port,
            "pid": int(browser_state.get("pid") or 0),
            "reused": bool(browser_state.get("reused")),
            **submit,
        },
    }


def run_payload(payload: dict[str, Any], *, dry_run: bool = False) -> dict[str, Any]:
    action = str(payload.get("action") or "").strip()
    if str(payload.get("schema") or "") != "cccc.web_model_browser_delivery.v1":
        return {"ok": False, "error": "unsupported or missing schema"}
    if action in {"open_session", "session_status", "close_session"}:
        group_id = str(payload.get("group_id") or "").strip()
        actor_id = str(payload.get("actor_id") or "").strip()
        if not group_id or not actor_id:
            return {"ok": False, "error": "group_id and actor_id are required"}
        try:
            if action == "open_session":
                return {
                    "ok": True,
                    "browser": open_chatgpt_browser_session(
                        group_id,
                        actor_id,
                        visibility=_normalize_visibility(str(payload.get("browser_visibility") or "visible")),
                    ),
                }
            if action == "close_session":
                return {"ok": True, "browser": close_chatgpt_browser_session(group_id, actor_id)}
            return {"ok": True, "browser": chatgpt_browser_session_status(group_id, actor_id)}
        except Exception as exc:
            return {"ok": False, "error": str(exc)[:2000]}
    if action != "submit_turn":
        return {"ok": False, "error": f"unsupported action: {action or '<empty>'}"}
    prompt = str(payload.get("prompt") or "")
    if not prompt.strip():
        return {"ok": False, "error": "missing prompt"}
    if dry_run:
        return {
            "ok": True,
            "delivery_id": f"dry-run:{uuid.uuid4().hex}",
            "browser": {
                "provider": str(payload.get("provider") or "chatgpt_web"),
                "dry_run": True,
                "prompt_chars": len(prompt),
                "turn_id": str(payload.get("turn_id") or ""),
            },
        }
    try:
        return submit_to_chatgpt(payload)
    except Exception as exc:
        return {"ok": False, "error": str(exc)[:2000]}


def _read_stdin_json() -> dict[str, Any]:
    raw = sys.stdin.read()
    if not raw.strip():
        return {}
    parsed = json.loads(raw)
    if not isinstance(parsed, dict):
        raise ValueError("stdin payload must be a JSON object")
    return parsed


def main(argv: Optional[list[str]] = None) -> int:
    parser = argparse.ArgumentParser(description="Submit CCCC web_model turns into ChatGPT web.")
    parser.add_argument("--dry-run", action="store_true", help="Validate stdin payload without opening a browser.")
    args = parser.parse_args(argv)
    try:
        result = run_payload(_read_stdin_json(), dry_run=bool(args.dry_run))
    except Exception as exc:
        result = {"ok": False, "error": str(exc)[:2000]}
    sys.stdout.write(_json_result(result) + "\n")
    return 0 if bool(result.get("ok")) else 1


if __name__ == "__main__":
    raise SystemExit(main())
