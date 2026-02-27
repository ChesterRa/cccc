from __future__ import annotations

import asyncio
import json
import os
import secrets
import shutil
import socket
import subprocess
import sys
import threading
import time
from copy import deepcopy
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Dict, Optional
from urllib.parse import urlparse
from urllib.request import urlopen

from ...providers.notebooklm.health import parse_notebooklm_auth_json
from ...providers.notebooklm.compat import probe_notebooklm_vendor
from ...providers.notebooklm.errors import NotebookLMProviderError
from ...paths import ensure_home
from .group_space_store import (
    get_space_provider_state,
    load_space_provider_secrets,
    set_space_provider_state,
    update_space_provider_secrets,
)

_FLOW_LOCK = threading.Lock()
_FLOW_STATE: Dict[str, Any] = {
    "provider": "notebooklm",
    "state": "idle",
    "phase": "idle",
    "session_id": "",
    "started_at": "",
    "updated_at": "",
    "finished_at": "",
    "message": "",
    "error": {},
}
_FLOW_THREAD: Optional[threading.Thread] = None
_FLOW_CANCEL_EVENT: Optional[threading.Event] = None
_SPACE_PROVIDER_SECRET_KEY = "NOTEBOOKLM_AUTH_JSON"


def _now_iso() -> str:
    return datetime.now(timezone.utc).isoformat().replace("+00:00", "Z")


def _snapshot_state() -> Dict[str, Any]:
    with _FLOW_LOCK:
        out = deepcopy(_FLOW_STATE)
    return out


def _update_state(**updates: Any) -> Dict[str, Any]:
    with _FLOW_LOCK:
        _FLOW_STATE.update(updates)
        _FLOW_STATE["updated_at"] = _now_iso()
        if not str(_FLOW_STATE.get("provider") or "").strip():
            _FLOW_STATE["provider"] = "notebooklm"
        out = deepcopy(_FLOW_STATE)
    return out


def _set_failed(*, code: str, message: str) -> Dict[str, Any]:
    return _update_state(
        state="failed",
        phase="failed",
        finished_at=_now_iso(),
        message=str(message or "NotebookLM connect failed"),
        error={"code": str(code or "space_provider_auth_flow_failed"), "message": str(message or "failed")},
    )


def _set_canceled(message: str = "Connect canceled") -> Dict[str, Any]:
    return _update_state(
        state="canceled",
        phase="canceled",
        finished_at=_now_iso(),
        message=str(message or "Connect canceled"),
        error={},
    )


def _set_succeeded(message: str = "Connected") -> Dict[str, Any]:
    return _update_state(
        state="succeeded",
        phase="done",
        finished_at=_now_iso(),
        message=str(message or "Connected"),
        error={},
    )


def _is_notebooklm_url(url: str) -> bool:
    raw = str(url or "").strip()
    if not raw:
        return False
    try:
        host = str(urlparse(raw).hostname or "").lower()
    except Exception:
        return False
    return host.endswith("notebooklm.google.com")


def _cancel_requested(cancel_event: threading.Event, *, session_id: str) -> bool:
    if not cancel_event.is_set():
        return False
    _set_canceled(message=f"Connect canceled ({session_id})")
    return True


def _install_playwright_chromium() -> None:
    cmd = [sys.executable, "-m", "playwright", "install", "chromium"]
    proc = subprocess.run(cmd, capture_output=True, text=True, timeout=600)
    if proc.returncode != 0:
        stderr = str(proc.stderr or "").strip()
        stdout = str(proc.stdout or "").strip()
        detail = stderr or stdout or "playwright install chromium failed"
        raise RuntimeError(detail[:800])


def _install_playwright_package() -> None:
    cmd = [
        sys.executable,
        "-m",
        "pip",
        "install",
        "--disable-pip-version-check",
        "playwright>=1.40,<2",
    ]
    proc = subprocess.run(cmd, capture_output=True, text=True, timeout=900)
    if proc.returncode != 0:
        stderr = str(proc.stderr or "").strip()
        stdout = str(proc.stdout or "").strip()
        detail = stderr or stdout or "pip install playwright failed"
        raise RuntimeError(detail[:1000])


def _ensure_sync_playwright():
    try:
        from playwright.sync_api import sync_playwright

        return sync_playwright
    except Exception:
        _update_state(
            state="running",
            phase="installing_playwright",
            message="Installing Playwright (first run only)...",
            error={},
        )
        _install_playwright_package()
    try:
        from playwright.sync_api import sync_playwright

        return sync_playwright
    except Exception as e:
        raise RuntimeError(f"failed to initialize Playwright after auto-install: {e}") from e


class _AuthBrowserSession:
    def __init__(
        self,
        *,
        browser: Optional[Any],
        context: Optional[Any],
        strategy: str,
        process: Optional[subprocess.Popen[str]] = None,
    ) -> None:
        self.browser = browser
        self.context = context
        self.strategy = strategy
        self.process = process

    def close(self) -> None:
        try:
            if self.context is not None:
                self.context.close()
        except Exception:
            pass
        try:
            if self.browser is not None:
                self.browser.close()
        except Exception:
            pass
        proc = self.process
        if proc is not None:
            try:
                if proc.poll() is None:
                    proc.terminate()
                    try:
                        proc.wait(timeout=3.0)
                    except Exception:
                        proc.kill()
            except Exception:
                pass


def _ensure_dir(path: Path, mode: int = 0o700) -> None:
    path.mkdir(parents=True, exist_ok=True)
    try:
        os.chmod(path, mode)
    except Exception:
        pass


def _managed_browser_profile_dir() -> Path:
    root = ensure_home() / "state" / "notebooklm_auth" / "browser_profile"
    _ensure_dir(root, 0o700)
    return root


def _pick_free_port() -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
        sock.bind(("127.0.0.1", 0))
        sock.listen(1)
        return int(sock.getsockname()[1])


def _wait_cdp_endpoint(port: int, *, timeout_seconds: float) -> bool:
    url = f"http://127.0.0.1:{int(port)}/json/version"
    deadline = time.time() + max(1.0, float(timeout_seconds))
    while time.time() < deadline:
        try:
            with urlopen(url, timeout=0.8) as resp:
                if int(getattr(resp, "status", 0) or 0) == 200:
                    return True
        except Exception:
            pass
        time.sleep(0.2)
    return False


def _system_browser_candidates() -> list[str]:
    out: list[str] = []
    seen: set[str] = set()

    def _append(raw: str) -> None:
        val = str(raw or "").strip()
        if not val:
            return
        path = ""
        if os.path.isabs(val):
            if os.path.exists(val):
                path = val
        else:
            resolved = shutil.which(val)
            if resolved:
                path = resolved
        if path and path not in seen:
            seen.add(path)
            out.append(path)

    # Explicit override for advanced users/testing.
    _append(str(os.environ.get("CCCC_NOTEBOOKLM_BROWSER") or ""))

    if os.name == "nt":
        _append(r"C:\Program Files\Google\Chrome\Application\chrome.exe")
        _append(r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe")
        _append(r"C:\Program Files\Microsoft\Edge\Application\msedge.exe")
        _append(r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe")
    elif sys.platform == "darwin":
        _append("/Applications/Google Chrome.app/Contents/MacOS/Google Chrome")
        _append("/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge")
        _append("/Applications/Chromium.app/Contents/MacOS/Chromium")
    else:
        _append("google-chrome")
        _append("google-chrome-stable")
        _append("microsoft-edge")
        _append("microsoft-edge-stable")
        _append("chromium")
        _append("chromium-browser")
    return out


def _start_system_browser_over_cdp(playwright_obj: Any) -> Optional[_AuthBrowserSession]:
    for binary in _system_browser_candidates():
        port = _pick_free_port()
        profile_dir = _managed_browser_profile_dir() / "system_browser"
        _ensure_dir(profile_dir, 0o700)
        cmd = [
            binary,
            f"--remote-debugging-port={int(port)}",
            f"--user-data-dir={profile_dir}",
            "--no-first-run",
            "--no-default-browser-check",
            "--new-window",
            "https://notebooklm.google.com/",
        ]
        proc: Optional[subprocess.Popen[str]] = None
        try:
            proc = subprocess.Popen(
                cmd,
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
                text=True,
            )
            if not _wait_cdp_endpoint(port, timeout_seconds=12.0):
                raise RuntimeError("cdp endpoint did not become ready")
            browser = playwright_obj.chromium.connect_over_cdp(f"http://127.0.0.1:{int(port)}")
            context = None
            wait_deadline = time.time() + 8.0
            while time.time() < wait_deadline:
                contexts = list(getattr(browser, "contexts", []) or [])
                if contexts:
                    context = contexts[0]
                    break
                time.sleep(0.2)
            if context is None:
                raise RuntimeError("cdp connected but no browser context became available")
            return _AuthBrowserSession(
                browser=browser,
                context=context,
                strategy=f"system_browser_cdp:{Path(binary).name}",
                process=proc,
            )
        except Exception:
            if proc is not None:
                try:
                    if proc.poll() is None:
                        proc.terminate()
                        try:
                            proc.wait(timeout=2.0)
                        except Exception:
                            proc.kill()
                except Exception:
                    pass
            continue
    return None


def _start_channel_browser(playwright_obj: Any, *, channel: str) -> Optional[_AuthBrowserSession]:
    try:
        profile_dir = _managed_browser_profile_dir() / f"playwright_{channel}"
        _ensure_dir(profile_dir, 0o700)
        context = playwright_obj.chromium.launch_persistent_context(
            user_data_dir=str(profile_dir),
            channel=channel,
            headless=False,
        )
        return _AuthBrowserSession(
            browser=getattr(context, "browser", None),
            context=context,
            strategy=f"playwright_channel:{channel}",
        )
    except Exception:
        return None


def _start_playwright_chromium(playwright_obj: Any) -> _AuthBrowserSession:
    try:
        profile_dir = _managed_browser_profile_dir() / "playwright_chromium"
        _ensure_dir(profile_dir, 0o700)
        context = playwright_obj.chromium.launch_persistent_context(
            user_data_dir=str(profile_dir),
            headless=False,
        )
        return _AuthBrowserSession(
            browser=getattr(context, "browser", None),
            context=context,
            strategy="playwright_chromium",
        )
    except Exception as e:
        msg = str(e)
        if "Executable doesn't exist" in msg or "playwright install" in msg:
            _update_state(
                state="running",
                phase="installing_browser_runtime",
                message="Installing Chromium runtime (first run only)...",
                error={},
            )
            _install_playwright_chromium()
            profile_dir = _managed_browser_profile_dir() / "playwright_chromium"
            _ensure_dir(profile_dir, 0o700)
            context = playwright_obj.chromium.launch_persistent_context(
                user_data_dir=str(profile_dir),
                headless=False,
            )
            return _AuthBrowserSession(
                browser=getattr(context, "browser", None),
                context=context,
                strategy="playwright_chromium",
            )
        raise RuntimeError(msg[:1600] or "unable to start Chromium") from e


def _start_browser_session(playwright_obj: Any) -> _AuthBrowserSession:
    # Strategy order:
    # 1) Real system browser via CDP (best chance to satisfy Google sign-in checks)
    # 2) Playwright channel browser (chrome/msedge if available)
    # 3) Playwright-managed Chromium fallback
    session = _start_system_browser_over_cdp(playwright_obj)
    if session is not None:
        return session
    for channel in ("chrome", "msedge"):
        session = _start_channel_browser(playwright_obj, channel=channel)
        if session is not None:
            return session
    return _start_playwright_chromium(playwright_obj)


def _page_urls(context: Any) -> list[str]:
    urls: list[str] = []
    try:
        pages = list(getattr(context, "pages", []) or [])
    except Exception:
        pages = []
    for page in pages:
        try:
            u = str(getattr(page, "url", "") or "").strip()
        except Exception:
            u = ""
        if u:
            urls.append(u)
    return urls


def _collect_storage_state(context: Any) -> Dict[str, Any]:
    state: Dict[str, Any] = {}
    try:
        raw_state = context.storage_state()
        if isinstance(raw_state, dict):
            state = dict(raw_state)
    except Exception:
        state = {}
    cookies = state.get("cookies")
    has_cookies = isinstance(cookies, list) and len(cookies) > 0
    if not has_cookies:
        fallback_cookies: list[Dict[str, Any]] = []
        try:
            fetched = context.cookies(
                [
                    "https://notebooklm.google.com",
                    "https://accounts.google.com",
                    "https://www.google.com",
                ]
            )
            if isinstance(fetched, list):
                fallback_cookies = [item for item in fetched if isinstance(item, dict)]
        except Exception:
            fallback_cookies = []
        if fallback_cookies:
            state["cookies"] = fallback_cookies
            state.setdefault("origins", [])
    if not isinstance(state.get("cookies"), list):
        state["cookies"] = []
    if not isinstance(state.get("origins"), list):
        state["origins"] = []
    return state


def _seed_context_with_storage_state(context: Any, storage_state: Dict[str, Any]) -> int:
    cookies = storage_state.get("cookies") if isinstance(storage_state, dict) else None
    if not isinstance(cookies, list) or not cookies:
        return 0
    payload: list[Dict[str, Any]] = []
    for item in cookies:
        if not isinstance(item, dict):
            continue
        name = str(item.get("name") or "").strip()
        value = str(item.get("value") or "")
        domain = str(item.get("domain") or "").strip()
        path = str(item.get("path") or "/").strip() or "/"
        if not name or not domain:
            continue
        row: Dict[str, Any] = {
            "name": name,
            "value": value,
            "domain": domain,
            "path": path,
        }
        expires = item.get("expires")
        if isinstance(expires, (int, float)):
            row["expires"] = float(expires)
        if "httpOnly" in item:
            row["httpOnly"] = bool(item.get("httpOnly"))
        if "secure" in item:
            row["secure"] = bool(item.get("secure"))
        same_site = str(item.get("sameSite") or "").strip()
        if same_site:
            row["sameSite"] = same_site
        payload.append(row)
    if not payload:
        return 0
    try:
        context.add_cookies(payload)
        return len(payload)
    except Exception:
        return 0


def _load_saved_storage_state() -> Dict[str, Any] | None:
    raw_env = str(os.environ.get("CCCC_NOTEBOOKLM_AUTH_JSON") or "").strip()
    if raw_env:
        try:
            return parse_notebooklm_auth_json(raw_env, label="CCCC_NOTEBOOKLM_AUTH_JSON")
        except Exception:
            return None
    try:
        secrets_map = load_space_provider_secrets("notebooklm")
    except Exception:
        return None
    raw = str(secrets_map.get(_SPACE_PROVIDER_SECRET_KEY) or "").strip()
    if not raw:
        return None
    try:
        return parse_notebooklm_auth_json(raw, label=_SPACE_PROVIDER_SECRET_KEY)
    except Exception:
        return None


def _is_hard_auth_failure(message: str) -> bool:
    text = str(message or "").strip().lower()
    if not text:
        return False
    markers = (
        "missing required cookies",
        "missing cookies",
        "authentication expired",
        "re-authenticate",
        "run 'notebooklm login'",
        "redirected to:",
        "space_provider_auth_invalid",
        "missing sid",
    )
    return any(token in text for token in markers)


def _run_coroutine_sync(coro: Any) -> Any:
    result_holder: Dict[str, Any] = {}
    error_holder: Dict[str, BaseException] = {}

    def _runner() -> None:
        try:
            result_holder["value"] = asyncio.run(coro)
        except BaseException as e:  # pragma: no cover - exercised via auth flow runtime path
            error_holder["error"] = e

    thread = threading.Thread(target=_runner, daemon=True)
    thread.start()
    thread.join()
    if "error" in error_holder:
        raise error_holder["error"]
    return result_holder.get("value")


def _verify_storage_state(storage_state: Dict[str, Any]) -> None:
    try:
        from ...providers.notebooklm._vendor.notebooklm.auth import (
            extract_cookies_from_storage,
            fetch_tokens,
        )
    except Exception as e:
        raise RuntimeError(f"NotebookLM vendor package unavailable: {e}") from e

    auth_json = json.dumps(storage_state, ensure_ascii=False)
    _ = parse_notebooklm_auth_json(auth_json, label=_SPACE_PROVIDER_SECRET_KEY)
    cookies = extract_cookies_from_storage(storage_state)
    _ = _run_coroutine_sync(fetch_tokens(cookies))
    compat = probe_notebooklm_vendor()
    if not bool(compat.compatible):
        raise RuntimeError(str(compat.reason or "NotebookLM vendor compatibility mismatch"))


def _persist_storage_state(storage_state: Dict[str, Any]) -> None:
    auth_json = json.dumps(storage_state, ensure_ascii=False)
    _ = update_space_provider_secrets(
        "notebooklm",
        set_vars={_SPACE_PROVIDER_SECRET_KEY: auth_json},
        unset_keys=[],
        clear=False,
    )


def _connect_worker(*, session_id: str, timeout_seconds: int, cancel_event: threading.Event) -> None:
    prior_state = get_space_provider_state("notebooklm")
    prior_enabled = bool(prior_state.get("enabled"))
    prior_real_enabled = bool(prior_state.get("real_enabled"))

    def _mark_provider_error(message: str) -> None:
        mode = "degraded" if prior_enabled else "disabled"
        _ = set_space_provider_state(
            "notebooklm",
            enabled=prior_enabled,
            real_enabled=prior_real_enabled,
            mode=mode,
            last_error=str(message or "NotebookLM connect failed"),
            touch_health=True,
        )

    def _mark_provider_connected(*, mode: str = "active", last_error: str = "") -> None:
        _ = set_space_provider_state(
            "notebooklm",
            enabled=True,
            real_enabled=True,
            mode=str(mode or "active"),
            last_error=str(last_error or ""),
            touch_health=True,
        )

    def _try_reuse_saved_credential() -> bool:
        saved = _load_saved_storage_state()
        if not isinstance(saved, dict):
            return False
        cookies = saved.get("cookies")
        if not isinstance(cookies, list) or not cookies:
            return False
        _update_state(
            state="running",
            phase="verifying_saved_credential",
            message="Checking existing Google credential...",
            error={},
        )
        try:
            _verify_storage_state(saved)
            _mark_provider_connected(mode="active", last_error="")
            _set_succeeded("Google account already connected.")
            return True
        except Exception as e:
            message = str(e)
            if _is_hard_auth_failure(message):
                return False
            _persist_storage_state(saved)
            _mark_provider_connected(mode="active", last_error="")
            _set_succeeded("Google credential reused (verification deferred).")
            return True

    try:
        if _try_reuse_saved_credential():
            return
        _update_state(
            state="running",
            phase="preparing_browser",
            message="Preparing browser...",
            error={},
        )
        try:
            sync_playwright = _ensure_sync_playwright()
        except Exception as e:
            message = f"Failed to prepare browser runtime for Google sign-in: {e}"
            _mark_provider_error(message)
            _set_failed(
                code="space_provider_auth_flow_dependency_missing",
                message=message,
            )
            return

        deadline = time.time() + max(60, min(timeout_seconds, 1800))
        with sync_playwright() as pw:
            browser_session = _start_browser_session(pw)
            try:
                context = browser_session.context
                saved_state = _load_saved_storage_state()
                restored_count = 0
                if isinstance(saved_state, dict):
                    restored_count = _seed_context_with_storage_state(context, saved_state)
                pages = list(getattr(context, "pages", []) or [])
                page = pages[0] if pages else context.new_page()
                next_probe_at = 0.0
                _update_state(
                    state="running",
                    phase="waiting_user_login",
                    message=(
                        f"Browser opened ({browser_session.strategy}). "
                        f"{'Restored previous session cookies. ' if restored_count > 0 else ''}"
                        "Sign in with Google in the opened window."
                    ),
                    error={},
                )
                if not _is_notebooklm_url(str(getattr(page, "url", "") or "")):
                    page.goto("https://notebooklm.google.com/", wait_until="domcontentloaded", timeout=120000)
                while True:
                    if _cancel_requested(cancel_event, session_id=session_id):
                        return
                    if time.time() >= deadline:
                        message = "Google sign-in timed out. Please retry Connect."
                        _mark_provider_error(message)
                        _set_failed(
                            code="space_provider_auth_flow_timeout",
                            message=message,
                        )
                        return
                    now_ts = time.time()
                    if now_ts < next_probe_at:
                        time.sleep(0.5)
                        continue

                    urls = _page_urls(context)
                    has_notebook_page = any(_is_notebooklm_url(u) for u in urls)
                    storage_state = _collect_storage_state(context)
                    cookies = storage_state.get("cookies") if isinstance(storage_state, dict) else None
                    has_any_cookies = isinstance(cookies, list) and len(cookies) > 0
                    if not has_any_cookies:
                        if has_notebook_page:
                            wait_msg = "NotebookLM opened. Waiting for Google session cookies..."
                        else:
                            wait_msg = "Browser opened. Waiting for NotebookLM sign-in to complete..."
                        _update_state(
                            state="running",
                            phase="waiting_user_login",
                            message=wait_msg,
                            error={},
                        )
                    if has_any_cookies:
                        persisted = False
                        try:
                            _ = parse_notebooklm_auth_json(
                                json.dumps(storage_state, ensure_ascii=False),
                                label=_SPACE_PROVIDER_SECRET_KEY,
                            )
                            _persist_storage_state(storage_state)
                            persisted = True
                        except NotebookLMProviderError:
                            _update_state(
                                state="running",
                                phase="waiting_user_login",
                                message="Waiting for complete Google session cookies...",
                                error={},
                            )
                            next_probe_at = now_ts + 4.0
                            time.sleep(0.5)
                            continue
                        _update_state(
                            state="running",
                            phase="verifying_session",
                            message=f"Verifying session and saving credential... (cookies={len(cookies)})",
                            error={},
                        )
                        try:
                            _verify_storage_state(storage_state)
                        except NotebookLMProviderError:
                            _update_state(
                                state="running",
                                phase="waiting_user_login",
                                message="Sign-in detected but session is incomplete. Keep the NotebookLM tab open for a moment...",
                                error={},
                            )
                            next_probe_at = now_ts + 4.0
                            time.sleep(0.5)
                            continue
                        except Exception as e:
                            msg = str(e)
                            if "vendor package unavailable" in msg or "compatibility mismatch" in msg:
                                raise
                            if persisted and (not _is_hard_auth_failure(msg)):
                                _mark_provider_connected(mode="active", last_error="")
                                _set_succeeded("Google account connected.")
                                return
                            brief = (msg or "verification pending").replace("\n", " ").strip()
                            if len(brief) > 160:
                                brief = brief[:160] + "..."
                            _update_state(
                                state="running",
                                phase="waiting_user_login",
                                message=f"Sign-in detected, verification pending: {brief}",
                                error={},
                            )
                            next_probe_at = now_ts + 4.0
                            time.sleep(0.5)
                            continue
                        _mark_provider_connected(mode="active", last_error="")
                        _set_succeeded("Google account connected.")
                        return
                    next_probe_at = now_ts + 2.0
                    time.sleep(0.5)
            finally:
                browser_session.close()
    except Exception as e:
        _mark_provider_error(str(e))
        _set_failed(code="space_provider_auth_flow_failed", message=str(e))


def start_notebooklm_auth_flow(*, timeout_seconds: int = 900) -> Dict[str, Any]:
    global _FLOW_THREAD, _FLOW_CANCEL_EVENT
    with _FLOW_LOCK:
        active = _FLOW_THREAD is not None and _FLOW_THREAD.is_alive()
        if active:
            return deepcopy(_FLOW_STATE)
        session_id = f"nbl_auth_{secrets.token_hex(6)}"
        cancel_event = threading.Event()
        _FLOW_CANCEL_EVENT = cancel_event
        _FLOW_THREAD = threading.Thread(
            target=_connect_worker,
            name="cccc-notebooklm-auth",
            kwargs={
                "session_id": session_id,
                "timeout_seconds": int(timeout_seconds or 900),
                "cancel_event": cancel_event,
            },
            daemon=True,
        )
        _FLOW_STATE.update(
            {
                "provider": "notebooklm",
                "state": "running",
                "phase": "starting",
                "session_id": session_id,
                "started_at": _now_iso(),
                "updated_at": _now_iso(),
                "finished_at": "",
                "message": "Starting Google connect flow...",
                "error": {},
            }
        )
        _FLOW_THREAD.start()
        return deepcopy(_FLOW_STATE)


def get_notebooklm_auth_flow_status() -> Dict[str, Any]:
    return _snapshot_state()


def cancel_notebooklm_auth_flow() -> Dict[str, Any]:
    with _FLOW_LOCK:
        cancel_event = _FLOW_CANCEL_EVENT
        running = _FLOW_THREAD is not None and _FLOW_THREAD.is_alive()
    if running and isinstance(cancel_event, threading.Event):
        cancel_event.set()
        return _update_state(
            state="running",
            phase="canceling",
            message="Cancel requested...",
        )
    return _snapshot_state()
