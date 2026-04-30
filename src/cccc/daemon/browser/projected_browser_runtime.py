"""Shared projected-browser runtime used by Presentation and auth flows.

This module owns the daemon-local Chromium session model:

1. one Playwright runtime per session
2. one active controller socket at a time
3. JPEG frame projection over JSON-lines sockets
4. input relay and lightweight inspection commands on the same runtime thread
"""

from __future__ import annotations

import base64
import json
import os
import queue
import selectors
import shutil
import socket
import subprocess
import sys
import threading
import time
from pathlib import Path
from typing import Any, Iterable, Optional
from urllib.request import urlopen

from ...util.time import utc_now_iso

_FRAME_INTERVAL_SECONDS = 0.35
_SOCKET_READ_TIMEOUT_SECONDS = 0.2
_START_WAIT_TIMEOUT_SECONDS = 20.0


def ensure_dir(path: Path, mode: int = 0o700) -> None:
    path.mkdir(parents=True, exist_ok=True)
    try:
        os.chmod(path, mode)
    except Exception:
        pass


def reset_dir(path: Path) -> None:
    try:
        shutil.rmtree(path)
    except FileNotFoundError:
        pass
    except Exception:
        pass


def install_playwright_package() -> None:
    proc = subprocess.run(
        [
            sys.executable,
            "-m",
            "pip",
            "install",
            "--disable-pip-version-check",
            "playwright>=1.40,<2",
        ],
        capture_output=True,
        text=True,
        timeout=900,
    )
    if proc.returncode != 0:
        detail = str(proc.stderr or "").strip() or str(proc.stdout or "").strip() or "pip install playwright failed"
        raise RuntimeError(detail[:1000])


def install_playwright_chromium() -> None:
    proc = subprocess.run(
        [sys.executable, "-m", "playwright", "install", "chromium"],
        capture_output=True,
        text=True,
        timeout=600,
    )
    if proc.returncode != 0:
        detail = str(proc.stderr or "").strip() or str(proc.stdout or "").strip() or "playwright install chromium failed"
        raise RuntimeError(detail[:800])


def ensure_sync_playwright():
    try:
        from playwright.sync_api import sync_playwright

        return sync_playwright
    except Exception:
        install_playwright_package()
    try:
        from playwright.sync_api import sync_playwright

        return sync_playwright
    except Exception as exc:
        raise RuntimeError(f"failed to initialize Playwright after auto-install: {exc}") from exc


def _terminate_process(proc: Any) -> None:
    if proc is None:
        return
    try:
        if proc.poll() is None:
            proc.terminate()
            try:
                proc.wait(timeout=3.0)
            except Exception:
                proc.kill()
    except Exception:
        pass


class _VirtualDisplay:
    def __init__(self, *, proc: Any, display: str) -> None:
        self.proc = proc
        self.display = str(display or "").strip()

    def env_overlay(self) -> dict[str, str]:
        return {"DISPLAY": self.display}

    def close(self) -> None:
        _terminate_process(self.proc)


def _start_virtual_display(*, width: int, height: int) -> _VirtualDisplay | None:
    if os.name == "nt":
        return None
    binary = shutil.which("Xvfb")
    if not binary:
        if str(os.environ.get("DISPLAY") or "").strip():
            return None
        raise RuntimeError("headed browser surface requires DISPLAY or Xvfb")
    proc = subprocess.Popen(
        [
            binary,
            "-displayfd",
            "1",
            "-screen",
            "0",
            f"{max(1024, int(width))}x{max(768, int(height))}x24",
            "-nolisten",
            "unix",
            "-nolisten",
            "tcp",
        ],
        stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL,
        text=True,
    )
    selector = selectors.DefaultSelector()
    display = ""
    try:
        if proc.stdout is None:
            raise RuntimeError("Xvfb did not expose a display fd")
        selector.register(proc.stdout, selectors.EVENT_READ)
        deadline = time.time() + 5.0
        while time.time() < deadline:
            if proc.poll() is not None:
                raise RuntimeError("Xvfb exited before a display became ready")
            events = selector.select(timeout=max(0.1, deadline - time.time()))
            if not events:
                continue
            line = str(proc.stdout.readline() or "").strip()
            if not line:
                continue
            if line.startswith(":"):
                display = line
            else:
                display = f":{line}"
            break
    finally:
        try:
            selector.close()
        except Exception:
            pass
        try:
            if proc.stdout is not None:
                proc.stdout.close()
        except Exception:
            pass
    if not display:
        _terminate_process(proc)
        raise RuntimeError("Xvfb did not report a usable display")
    return _VirtualDisplay(proc=proc, display=display)


def _system_browser_binaries(channel: str) -> list[str]:
    name = str(channel or "").strip().lower()
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

    if name == "chrome":
        if os.name == "nt":
            _append(r"C:\Program Files\Google\Chrome\Application\chrome.exe")
            _append(r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe")
        elif sys.platform == "darwin":
            _append("/Applications/Google Chrome.app/Contents/MacOS/Google Chrome")
        else:
            _append("google-chrome")
            _append("google-chrome-stable")
    elif name == "msedge":
        if os.name == "nt":
            _append(r"C:\Program Files\Microsoft\Edge\Application\msedge.exe")
            _append(r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe")
        elif sys.platform == "darwin":
            _append("/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge")
        else:
            _append("microsoft-edge")
            _append("microsoft-edge-stable")
    return out


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


def _page_urls_from_context(context: Any) -> list[str]:
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


def collect_storage_state(context: Any) -> dict[str, Any]:
    state: dict[str, Any] = {}
    try:
        raw_state = context.storage_state()
        if isinstance(raw_state, dict):
            state = dict(raw_state)
    except Exception:
        state = {}
    if not isinstance(state.get("cookies"), list):
        state["cookies"] = []
    if not isinstance(state.get("origins"), list):
        state["origins"] = []
    return state


def get_cookies_for_urls(context: Any, urls: Iterable[str]) -> list[dict[str, Any]]:
    targets = [str(item or "").strip() for item in (urls or []) if str(item or "").strip()]
    if not targets:
        return []
    try:
        fetched = context.cookies(targets)
    except Exception:
        return []
    if not isinstance(fetched, list):
        return []
    return [item for item in fetched if isinstance(item, dict)]


def seed_context_with_storage_state(context: Any, storage_state: dict[str, Any] | None) -> int:
    cookies = storage_state.get("cookies") if isinstance(storage_state, dict) else None
    if not isinstance(cookies, list) or not cookies:
        return 0
    payload: list[dict[str, Any]] = []
    for item in cookies:
        if not isinstance(item, dict):
            continue
        name = str(item.get("name") or "").strip()
        value = str(item.get("value") or "")
        domain = str(item.get("domain") or "").strip()
        path = str(item.get("path") or "/").strip() or "/"
        if not name or not domain:
            continue
        row: dict[str, Any] = {
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


class PlaywrightProjectedRuntime:
    def __init__(
        self,
        *,
        playwright_cm: Any,
        context: Any,
        page: Any,
        cdp_session: Any,
        width: int,
        height: int,
        strategy: str,
        cleanup_callbacks: Iterable[Any] = (),
        metadata: dict[str, Any] | None = None,
    ) -> None:
        self._lock = threading.RLock()
        self._playwright_cm = playwright_cm
        self._context = context
        self._page = page
        self._cdp = cdp_session
        self.strategy = str(strategy or "playwright_chromium_cdp")
        self.width = int(width)
        self.height = int(height)
        self.metadata = dict(metadata or {})
        self._cleanup_callbacks = list(cleanup_callbacks or [])
        self._page_history: list[Any] = []
        self._known_page_ids: set[int] = set()
        self._register_page(page)
        try:
            self._context.on("page", self._handle_new_page)
        except Exception:
            pass

    @property
    def page(self) -> Any:
        with self._lock:
            return self._page

    def _is_page_live(self, page: Any) -> bool:
        if page is None:
            return False
        try:
            return not bool(page.is_closed())
        except Exception:
            return False

    def _remember_page(self, page: Any) -> None:
        if not self._is_page_live(page):
            return
        self._page_history = [item for item in self._page_history if item is not page and self._is_page_live(item)]
        self._page_history.append(page)

    def _pop_previous_page(self) -> Any | None:
        while self._page_history:
            candidate = self._page_history.pop()
            if self._is_page_live(candidate):
                return candidate
        return None

    def _register_page(self, page: Any) -> None:
        if page is None:
            return
        page_id = id(page)
        if page_id in self._known_page_ids:
            return
        self._known_page_ids.add(page_id)
        try:
            page.on("close", lambda *_args, _page=page: self._handle_closed_page(_page))
        except Exception:
            pass

    def _bind_cdp(self, page: Any) -> None:
        old_cdp = self._cdp
        self._page = page
        self._cdp = self._context.new_cdp_session(page)
        try:
            self._cdp.send("Page.enable")
        except Exception:
            pass
        try:
            page.set_viewport_size({"width": self.width, "height": self.height})
        except Exception:
            pass
        if old_cdp is not None and old_cdp is not self._cdp:
            try:
                old_cdp.detach()
            except Exception:
                pass

    def _activate_page(self, page: Any, *, remember_previous: bool) -> None:
        if not self._is_page_live(page):
            return
        with self._lock:
            current = self._page
            if remember_previous and current is not None and current is not page and self._is_page_live(current):
                self._remember_page(current)
            self._register_page(page)
            self._bind_cdp(page)

    def _handle_new_page(self, page: Any) -> None:
        if page is None:
            return
        try:
            page.wait_for_load_state("domcontentloaded", timeout=15000)
        except Exception:
            pass
        self._activate_page(page, remember_previous=True)

    def _handle_closed_page(self, page: Any) -> None:
        fallback = None
        with self._lock:
            self._page_history = [item for item in self._page_history if item is not page and self._is_page_live(item)]
            if page is not self._page:
                return
            fallback = self._pop_previous_page()
            if fallback is None:
                for candidate in reversed(list(getattr(self._context, "pages", []) or [])):
                    if candidate is not page and self._is_page_live(candidate):
                        fallback = candidate
                        break
        if fallback is not None:
            self._activate_page(fallback, remember_previous=False)

    def current_url(self) -> str:
        try:
            page = self.page
            return str(getattr(page, "url", "") or "").strip()
        except Exception:
            return ""

    def page_urls(self) -> list[str]:
        with self._lock:
            return _page_urls_from_context(self._context)

    def storage_state(self) -> dict[str, Any]:
        with self._lock:
            return collect_storage_state(self._context)

    def cookies_for_urls(self, urls: Iterable[str]) -> list[dict[str, Any]]:
        with self._lock:
            return get_cookies_for_urls(self._context, urls)

    def capture_frame(self) -> bytes:
        with self._lock:
            cdp = self._cdp
        payload = cdp.send(
            "Page.captureScreenshot",
            {
                "format": "jpeg",
                "quality": 60,
                "captureBeyondViewport": False,
                "fromSurface": True,
            },
        )
        data = str((payload or {}).get("data") or "")
        return base64.b64decode(data) if data else b""

    def click(self, *, x: float, y: float, button: str = "left") -> None:
        self.page.mouse.click(float(x), float(y), button=str(button or "left"))

    def scroll(self, *, dx: float, dy: float) -> None:
        self.page.mouse.wheel(float(dx), float(dy))

    def key_press(self, *, key: str) -> None:
        self.page.keyboard.press(str(key or ""))

    def input_text(self, *, text: str) -> None:
        self.page.keyboard.insert_text(str(text or ""))

    def resize(self, *, width: int, height: int) -> None:
        self.width = int(width)
        self.height = int(height)
        self.page.set_viewport_size({"width": self.width, "height": self.height})

    def navigate(self, *, url: str) -> None:
        self.page.goto(str(url or ""), wait_until="domcontentloaded", timeout=30000)

    def refresh(self) -> None:
        self.page.reload(wait_until="domcontentloaded", timeout=30000)

    def back(self) -> None:
        page = self.page
        try:
            result = page.go_back(wait_until="domcontentloaded", timeout=10000)
        except Exception:
            result = None
        if result is not None:
            return
        with self._lock:
            fallback = self._pop_previous_page()
        if fallback is not None:
            self._activate_page(fallback, remember_previous=False)

    def close(self) -> None:
        try:
            if self._cdp is not None:
                self._cdp.detach()
        except Exception:
            pass
        try:
            if self._context is not None:
                self._context.close()
        except Exception:
            pass
        try:
            self._playwright_cm.__exit__(None, None, None)
        except Exception:
            pass
        for callback in reversed(self._cleanup_callbacks):
            try:
                callback()
            except Exception:
                pass


def launch_projected_browser_runtime(
    *,
    profile_dir: Path,
    url: str,
    width: int,
    height: int,
    headless: bool = False,
    channel_candidates: Iterable[str | None] = (None,),
    seed_storage_state: dict[str, Any] | None = None,
    system_profile_subdir: str | None = None,
) -> PlaywrightProjectedRuntime:
    sync_playwright = ensure_sync_playwright()
    playwright_cm = sync_playwright()
    pw = playwright_cm.__enter__()

    browser_env = {str(k): str(v) for k, v in os.environ.items()}
    cleanup_callbacks: list[Any] = []
    strategy_suffix = ""
    if not bool(headless):
        virtual_display = _start_virtual_display(width=width, height=height)
        if virtual_display is not None:
            browser_env.update(virtual_display.env_overlay())
            cleanup_callbacks.append(virtual_display.close)
            strategy_suffix = "_xvfb"

    def _launch_system_browser_once(channel: str) -> PlaywrightProjectedRuntime | None:
        if bool(headless):
            return None
        binaries = _system_browser_binaries(channel)
        if not binaries:
            return None
        for binary in binaries:
            port = _pick_free_port()
            if system_profile_subdir is None:
                system_profile_dir = profile_dir / f"system_{channel}"
            else:
                subdir = str(system_profile_subdir or "").strip()
                system_profile_dir = profile_dir / subdir if subdir else profile_dir
            ensure_dir(system_profile_dir, 0o700)
            proc = None
            try:
                proc = subprocess.Popen(
                    [
                        binary,
                        f"--remote-debugging-port={int(port)}",
                        f"--user-data-dir={system_profile_dir}",
                        "--no-first-run",
                        "--no-default-browser-check",
                        f"--window-size={max(1024, int(width))},{max(768, int(height))}",
                        str(url).strip() or "about:blank",
                    ],
                    stdout=subprocess.DEVNULL,
                    stderr=subprocess.DEVNULL,
                    text=True,
                    env=browser_env,
                )
                if not _wait_cdp_endpoint(port, timeout_seconds=12.0):
                    raise RuntimeError("cdp endpoint did not become ready")
                browser = pw.chromium.connect_over_cdp(f"http://127.0.0.1:{int(port)}")
                contexts = list(getattr(browser, "contexts", []) or [])
                context = contexts[0] if contexts else None
                if context is None:
                    raise RuntimeError("cdp connected but no browser context became available")
                _ = seed_context_with_storage_state(context, seed_storage_state)
                pages = list(getattr(context, "pages", []) or [])
                page = pages[0] if pages else context.new_page()
                page.set_viewport_size({"width": int(width), "height": int(height)})
                cdp_session = context.new_cdp_session(page)
                try:
                    cdp_session.send("Page.enable")
                except Exception:
                    pass
                strategy = f"system_browser_cdp:{Path(binary).name}{strategy_suffix}"
                return PlaywrightProjectedRuntime(
                    playwright_cm=playwright_cm,
                    context=context,
                    page=page,
                    cdp_session=cdp_session,
                    width=width,
                    height=height,
                    strategy=strategy,
                    metadata={
                        "cdp_port": int(port),
                        "pid": int(getattr(proc, "pid", 0) or 0),
                        "profile_dir": str(system_profile_dir),
                        "browser_binary": str(binary),
                        "channel": str(channel),
                    },
                    cleanup_callbacks=[lambda proc=proc: _terminate_process(proc), *cleanup_callbacks],
                )
            except Exception:
                _terminate_process(proc)
                continue
        return None

    def _launch_once(channel: str | None) -> PlaywrightProjectedRuntime:
        if channel:
            system_browser = _launch_system_browser_once(channel)
            if system_browser is not None:
                return system_browser
        launch_kwargs: dict[str, Any] = {
            "user_data_dir": str(profile_dir),
            "headless": bool(headless),
            "viewport": {"width": int(width), "height": int(height)},
            "env": browser_env,
        }
        if channel:
            launch_kwargs["channel"] = str(channel)
        context = pw.chromium.launch_persistent_context(**launch_kwargs)
        _ = seed_context_with_storage_state(context, seed_storage_state)
        pages = list(getattr(context, "pages", []) or [])
        page = pages[0] if pages else context.new_page()
        page.set_viewport_size({"width": int(width), "height": int(height)})
        cdp_session = context.new_cdp_session(page)
        try:
            cdp_session.send("Page.enable")
        except Exception:
            pass
        if str(url or "").strip():
            page.goto(str(url).strip(), wait_until="domcontentloaded", timeout=30000)
        if channel:
            strategy = f"playwright_channel:{channel}{'_headless' if headless else strategy_suffix}"
        else:
            strategy = "playwright_chromium_cdp" if headless else f"playwright_chromium{strategy_suffix}"
        return PlaywrightProjectedRuntime(
            playwright_cm=playwright_cm,
            context=context,
            page=page,
            cdp_session=cdp_session,
            width=width,
            height=height,
            strategy=strategy,
            metadata={
                "cdp_port": 0,
                "pid": 0,
                "profile_dir": str(profile_dir),
                "browser_binary": "",
                "channel": str(channel or "playwright"),
            },
            cleanup_callbacks=cleanup_callbacks,
        )

    normalized_candidates: list[str | None] = []
    seen: set[str] = set()
    for raw in channel_candidates or (None,):
        if raw is None:
            key = "__managed_chromium__"
            if key in seen:
                continue
            seen.add(key)
            normalized_candidates.append(None)
            continue
        text = str(raw or "").strip()
        if not text or text in seen:
            continue
        seen.add(text)
        normalized_candidates.append(text)
    if not normalized_candidates:
        normalized_candidates = [None]

    errors: list[str] = []
    for candidate in normalized_candidates:
        try:
            return _launch_once(candidate)
        except Exception as exc:
            message = str(exc or "")
            needs_install = candidate is None and ("Executable doesn't exist" in message or "playwright install" in message)
            if needs_install:
                try:
                    install_playwright_chromium()
                    return _launch_once(candidate)
                except Exception as retry_exc:
                    errors.append(str(retry_exc or "failed to install Chromium"))
                    continue
            errors.append(message or "unable to launch browser runtime")

    try:
        playwright_cm.__exit__(None, None, None)
    except Exception:
        pass
    for callback in reversed(cleanup_callbacks):
        try:
            callback()
        except Exception:
            pass
    raise RuntimeError(errors[-1][:1600] if errors else "unable to launch browser runtime")


class ProjectedBrowserSession:
    def __init__(
        self,
        *,
        session_key: str,
        profile_dir: Path,
        url: str,
        width: int,
        height: int,
        headless: bool,
        channel_candidates: Iterable[str | None],
        system_profile_subdir: str | None = None,
    ) -> None:
        self.session_key = str(session_key or "").strip()
        self.profile_dir = Path(profile_dir)
        self.initial_url = str(url or "").strip()
        self.width = max(640, min(int(width), 2560))
        self.height = max(480, min(int(height), 1600))
        self.headless = bool(headless)
        self.channel_candidates = tuple(channel_candidates or (None,))
        self.system_profile_subdir = system_profile_subdir
        self._lock = threading.Lock()
        self._frame_cond = threading.Condition(self._lock)
        self._commands: "queue.Queue[tuple[str, dict[str, Any], Optional[queue.Queue[dict[str, Any]]]]]" = queue.Queue()
        self._stop_event = threading.Event()
        self._thread = threading.Thread(
            target=self._run,
            daemon=True,
            name=f"cccc-browser-{self.session_key[:48]}",
        )
        self._controller_attached = False
        self._controller_socket: Optional[socket.socket] = None
        self._controller_generation = 0
        self._state = "starting"
        self._message = "Preparing browser runtime..."
        self._error: dict[str, Any] = {}
        self._strategy = ""
        self._url = self.initial_url
        self._updated_at = utc_now_iso()
        self._started_at = self._updated_at
        self._last_frame_seq = 0
        self._last_frame_at = ""
        self._last_frame_bytes = b""
        self._seed_storage_state: dict[str, Any] | None = None
        self._metadata: dict[str, Any] = {}

    def set_seed_storage_state(self, storage_state: dict[str, Any] | None) -> None:
        self._seed_storage_state = dict(storage_state or {}) if isinstance(storage_state, dict) else None

    def start(self) -> None:
        ensure_dir(self.profile_dir, 0o700)
        self._thread.start()

    def close(self) -> None:
        self._stop_event.set()
        try:
            self._commands.put_nowait(("close", {}, None))
        except Exception:
            pass
        self._thread.join(timeout=5.0)
        with self._lock:
            self._controller_attached = False
            self._controller_socket = None
            if self._state not in {"failed", "closed"}:
                self._state = "closed"
                self._message = "Browser surface closed."
                self._updated_at = utc_now_iso()
            self._frame_cond.notify_all()

    def wait_until_started(self, timeout: float = _START_WAIT_TIMEOUT_SECONDS) -> dict[str, Any]:
        deadline = time.time() + max(1.0, float(timeout))
        while time.time() < deadline:
            snapshot = self.snapshot()
            if snapshot["state"] in {"ready", "failed"}:
                return snapshot
            time.sleep(0.05)
        return self.snapshot()

    def snapshot(self) -> dict[str, Any]:
        with self._lock:
            return {
                "active": self._state in {"starting", "ready", "failed"},
                "state": self._state,
                "message": self._message,
                "error": dict(self._error),
                "strategy": self._strategy,
                "url": self._url,
                "width": self.width,
                "height": self.height,
                "started_at": self._started_at,
                "updated_at": self._updated_at,
                "last_frame_seq": self._last_frame_seq,
                "last_frame_at": self._last_frame_at,
                "controller_attached": bool(self._controller_attached),
                "metadata": dict(self._metadata),
            }

    def can_attach(self) -> tuple[bool, dict[str, Any]]:
        with self._lock:
            if self._state not in {"starting", "ready"}:
                message = str(self._error.get("message") or self._message or "browser surface is not active")
                return False, {"code": "browser_surface_not_active", "message": message, "details": dict(self._error)}
            return True, {}

    def attach_socket(self, sock: socket.socket) -> bool:
        with self._lock:
            if self._state not in {"starting", "ready"}:
                return False
            old_sock = self._controller_socket
            self._controller_generation += 1
            generation = self._controller_generation
            self._controller_attached = True
            self._controller_socket = sock
            self._updated_at = utc_now_iso()
        # Evict old controller outside the lock to avoid deadlocks.
        if old_sock is not None:
            try:
                old_sock.close()
            except Exception:
                pass
        threading.Thread(
            target=self._serve_socket,
            args=(sock, generation),
            daemon=True,
            name=f"cccc-browser-stream-{self.session_key[:48]}",
        ).start()
        return True

    def wait_for_frame(self, *, after_seq: int, timeout: float) -> Optional[dict[str, Any]]:
        deadline = time.time() + max(0.0, float(timeout))
        with self._frame_cond:
            while self._last_frame_seq <= int(after_seq) and not self._stop_event.is_set():
                remaining = deadline - time.time()
                if remaining <= 0:
                    return None
                self._frame_cond.wait(timeout=remaining)
            if self._last_frame_seq <= int(after_seq):
                return None
            return {
                "seq": self._last_frame_seq,
                "captured_at": self._last_frame_at,
                "bytes": bytes(self._last_frame_bytes),
                "width": self.width,
                "height": self.height,
                "url": self._url,
            }

    def submit_command(self, kind: str, payload: dict[str, Any], *, timeout: float = 10.0) -> dict[str, Any]:
        reply: "queue.Queue[dict[str, Any]]" = queue.Queue(maxsize=1)
        self._commands.put((str(kind or "").strip().lower(), dict(payload or {}), reply))
        try:
            result = reply.get(timeout=max(0.1, float(timeout)))
        except queue.Empty as exc:
            raise RuntimeError("browser command timed out") from exc
        if not bool(result.get("ok")):
            raise RuntimeError(str(result.get("message") or "browser command failed"))
        return result

    def _set_state(self, state: str, *, message: str, error: Optional[dict[str, Any]] = None) -> None:
        with self._lock:
            self._state = str(state or self._state)
            self._message = str(message or self._message)
            self._error = dict(error or {})
            self._updated_at = utc_now_iso()
            self._frame_cond.notify_all()

    def _record_frame(self, frame_bytes: bytes) -> None:
        with self._frame_cond:
            self._last_frame_seq += 1
            self._last_frame_bytes = bytes(frame_bytes)
            self._last_frame_at = utc_now_iso()
            self._updated_at = self._last_frame_at
            self._frame_cond.notify_all()

    def _apply_command(self, runtime: PlaywrightProjectedRuntime, kind: str, payload: dict[str, Any]) -> dict[str, Any]:
        if kind == "ping":
            return {"ok": True}
        if kind == "navigate":
            runtime.navigate(url=str(payload.get("url") or "").strip())
        elif kind == "back":
            runtime.back()
        elif kind == "refresh":
            runtime.refresh()
        elif kind == "click":
            runtime.click(
                x=float(payload.get("x") or 0.0),
                y=float(payload.get("y") or 0.0),
                button=str(payload.get("button") or "left"),
            )
        elif kind == "scroll":
            runtime.scroll(
                dx=float(payload.get("dx") or 0.0),
                dy=float(payload.get("dy") or 0.0),
            )
        elif kind == "key":
            runtime.key_press(key=str(payload.get("key") or ""))
        elif kind == "text":
            runtime.input_text(text=str(payload.get("text") or ""))
        elif kind == "resize":
            width = max(640, min(int(payload.get("width") or self.width), 2560))
            height = max(480, min(int(payload.get("height") or self.height), 1600))
            runtime.resize(width=width, height=height)
            self.width = width
            self.height = height
        elif kind == "inspect_page_urls":
            return {"ok": True, "page_urls": runtime.page_urls()}
        elif kind == "inspect_storage_state":
            return {"ok": True, "storage_state": runtime.storage_state()}
        elif kind == "inspect_cookies":
            raw_urls = payload.get("urls")
            urls = raw_urls if isinstance(raw_urls, list) else []
            return {"ok": True, "cookies": runtime.cookies_for_urls(urls)}
        elif kind == "close":
            self._stop_event.set()
            return {"ok": True}
        else:
            raise RuntimeError(f"unsupported browser command: {kind}")
        with self._lock:
            self._url = str(runtime.current_url() or self._url)
            self._updated_at = utc_now_iso()
        return {"ok": True}

    def _run(self) -> None:
        runtime: Optional[PlaywrightProjectedRuntime] = None
        try:
            runtime = launch_projected_browser_runtime(
                profile_dir=self.profile_dir,
                url=self.initial_url,
                width=self.width,
                height=self.height,
                headless=self.headless,
                channel_candidates=self.channel_candidates,
                seed_storage_state=self._seed_storage_state,
                system_profile_subdir=self.system_profile_subdir,
            )
            with self._lock:
                self._strategy = str(getattr(runtime, "strategy", "") or "")
                self._metadata = dict(getattr(runtime, "metadata", {}) or {})
                self._url = str(runtime.current_url() or self.initial_url)
                self._updated_at = utc_now_iso()
            self._set_state("ready", message=f"Browser surface ready ({self._strategy or 'chromium'}).")
            next_frame_at = 0.0
            consecutive_capture_failures = 0
            while not self._stop_event.is_set():
                timeout = max(0.05, min(0.20, next_frame_at - time.time())) if next_frame_at else 0.05
                try:
                    kind, payload, reply = self._commands.get(timeout=timeout)
                except queue.Empty:
                    kind, payload, reply = "", {}, None

                if kind:
                    try:
                        result = self._apply_command(runtime, kind, payload)
                    except Exception as exc:
                        result = {"ok": False, "message": str(exc or "browser command failed")}
                    if reply is not None:
                        try:
                            reply.put_nowait(result)
                        except Exception:
                            pass
                    if kind == "close":
                        break

                now = time.time()
                if next_frame_at and now < next_frame_at:
                    continue
                with self._lock:
                    self._url = str(runtime.current_url() or self._url)
                try:
                    frame = runtime.capture_frame()
                except Exception:
                    consecutive_capture_failures += 1
                    if consecutive_capture_failures >= 5:
                        raise
                    frame = None
                else:
                    consecutive_capture_failures = 0
                if frame:
                    self._record_frame(frame)
                next_frame_at = time.time() + _FRAME_INTERVAL_SECONDS
        except Exception as exc:
            self._set_state(
                "failed",
                message=f"Browser surface failed: {exc}",
                error={"code": "browser_surface_runtime_failed", "message": str(exc)},
            )
        finally:
            try:
                if runtime is not None:
                    runtime.close()
            except Exception:
                pass
            with self._lock:
                if self._state not in {"failed", "closed"}:
                    self._state = "closed"
                    self._message = "Browser surface closed."
                    self._updated_at = utc_now_iso()
                self._frame_cond.notify_all()

    def _serve_socket(self, sock: socket.socket, generation: int = 0) -> None:
        buffer = b""
        last_seq = 0
        sent_state_marker = ""
        try:
            sock.settimeout(_SOCKET_READ_TIMEOUT_SECONDS)
        except Exception:
            pass
        try:
            while not self._stop_event.is_set():
                snapshot = self.snapshot()
                state_marker = json.dumps(
                    {
                        "state": snapshot["state"],
                        "message": snapshot["message"],
                        "url": snapshot["url"],
                        "seq": snapshot["last_frame_seq"],
                        "updated_at": snapshot["updated_at"],
                    },
                    sort_keys=True,
                )
                if state_marker != sent_state_marker:
                    if not _send_json_line(
                        sock,
                        {
                            "t": "state",
                            **snapshot,
                        },
                    ):
                        break
                    sent_state_marker = state_marker
                    if snapshot["state"] == "failed":
                        break

                frame = self.wait_for_frame(after_seq=last_seq, timeout=0.25)
                if frame is not None:
                    if not _send_json_line(
                        sock,
                        {
                            "t": "frame",
                            "seq": frame["seq"],
                            "captured_at": frame["captured_at"],
                            "mime": "image/jpeg",
                            "data_base64": base64.b64encode(frame["bytes"]).decode("ascii"),
                            "width": frame["width"],
                            "height": frame["height"],
                            "url": frame["url"],
                        },
                    ):
                        break
                    last_seq = int(frame["seq"])

                incoming, buffer, disconnected = _recv_json_line_nonblocking(sock, buffer)
                if disconnected:
                    break
                if incoming is not None:
                    kind = str(incoming.get("t") or "").strip().lower()
                    if kind in {"disconnect", "close"}:
                        break
                    try:
                        # Controller input is best-effort. During login flows the page can
                        # briefly block Playwright commands while navigation or screenshot
                        # capture is in progress; waiting synchronously here turns that
                        # transient congestion into a user-visible error.
                        self._commands.put((kind, incoming, None))
                    except Exception as exc:
                        if not _send_json_line(
                            sock,
                            {
                                "t": "error",
                                "code": "browser_surface_command_failed",
                                "message": str(exc),
                            },
                        ):
                            break
        finally:
            with self._lock:
                # Only reset if we are still the current controller (not evicted).
                if self._controller_generation == generation:
                    self._controller_attached = False
                    self._controller_socket = None
                    self._updated_at = utc_now_iso()
                self._frame_cond.notify_all()
            try:
                sock.close()
            except Exception:
                pass


def _send_json_line(sock: socket.socket, obj: dict[str, Any]) -> bool:
    try:
        sock.sendall((json.dumps(obj, ensure_ascii=False) + "\n").encode("utf-8"))
        return True
    except Exception:
        return False


def _recv_json_line_nonblocking(
    sock: socket.socket,
    buffer: bytes,
) -> tuple[Optional[dict[str, Any]], bytes, bool]:
    if b"\n" in buffer:
        line, remainder = buffer.split(b"\n", 1)
        try:
            return json.loads(line.decode("utf-8", errors="replace")), remainder, False
        except Exception:
            return None, remainder, False

    try:
        chunk = sock.recv(65536)
    except socket.timeout:
        return None, buffer, False
    except Exception:
        return None, buffer, True

    if not chunk:
        return None, b"", True

    buffer += chunk
    if len(buffer) > 2_000_000:
        return None, b"", True
    if b"\n" not in buffer:
        return None, buffer, False
    line, remainder = buffer.split(b"\n", 1)
    try:
        return json.loads(line.decode("utf-8", errors="replace")), remainder, False
    except Exception:
        return None, remainder, False


class ProjectedBrowserSessionManager:
    def __init__(self, *, idle_message: str) -> None:
        self._idle_message = str(idle_message or "No browser surface session is active.")
        self._lock = threading.Lock()
        self._sessions: dict[str, ProjectedBrowserSession] = {}

    def open(
        self,
        *,
        key: str,
        profile_dir: Path,
        url: str,
        width: int,
        height: int,
        headless: bool = False,
        channel_candidates: Iterable[str | None] = (None,),
        seed_storage_state: dict[str, Any] | None = None,
        system_profile_subdir: str | None = None,
    ) -> dict[str, Any]:
        normalized_key = str(key or "").strip()
        replacement = ProjectedBrowserSession(
            session_key=normalized_key,
            profile_dir=profile_dir,
            url=url,
            width=width,
            height=height,
            headless=headless,
            channel_candidates=channel_candidates,
            system_profile_subdir=system_profile_subdir,
        )
        replacement.set_seed_storage_state(seed_storage_state)
        previous: Optional[ProjectedBrowserSession] = None
        with self._lock:
            previous = self._sessions.get(normalized_key)
            self._sessions[normalized_key] = replacement
        if previous is not None:
            previous.close()
        replacement.start()
        return replacement.wait_until_started()

    def info(self, *, key: str) -> dict[str, Any]:
        normalized_key = str(key or "").strip()
        with self._lock:
            session = self._sessions.get(normalized_key)
        if session is None:
            return {
                "active": False,
                "state": "idle",
                "message": self._idle_message,
                "error": {},
                "strategy": "",
                "url": "",
                "width": 0,
                "height": 0,
                "started_at": "",
                "updated_at": "",
                "last_frame_seq": 0,
                "last_frame_at": "",
                "controller_attached": False,
            }
        return session.snapshot()

    def close(self, *, key: str) -> dict[str, Any]:
        normalized_key = str(key or "").strip()
        with self._lock:
            session = self._sessions.pop(normalized_key, None)
        if session is None:
            return {"closed": False, "browser_surface": self.info(key=normalized_key)}
        session.close()
        return {"closed": True, "browser_surface": self.info(key=normalized_key)}

    def close_all(self) -> None:
        with self._lock:
            sessions = list(self._sessions.values())
            self._sessions.clear()
        for session in sessions:
            try:
                session.close()
            except Exception:
                pass

    def can_attach(self, *, key: str) -> tuple[bool, dict[str, Any]]:
        normalized_key = str(key or "").strip()
        with self._lock:
            session = self._sessions.get(normalized_key)
        if session is None:
            return False, {
                "code": "browser_surface_not_found",
                "message": self._idle_message,
                "details": {},
            }
        return session.can_attach()

    def attach_socket(self, *, key: str, sock: socket.socket) -> bool:
        normalized_key = str(key or "").strip()
        with self._lock:
            session = self._sessions.get(normalized_key)
        if session is None:
            return False
        return session.attach_socket(sock)

    def execute(self, *, key: str, kind: str, payload: dict[str, Any], timeout: float = 10.0) -> dict[str, Any]:
        normalized_key = str(key or "").strip()
        with self._lock:
            session = self._sessions.get(normalized_key)
        if session is None:
            raise RuntimeError(self._idle_message)
        return session.submit_command(kind, payload, timeout=timeout)
