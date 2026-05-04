import socket
import time
import unittest
from unittest.mock import patch


class _FakeProc:
    def __init__(self, line: str = "123\n") -> None:
        self.stdout = _FakeStdout(line)
        self.returncode = None
        self.terminated = False
        self.killed = False
        self.pid = 4321

    def poll(self):
        return self.returncode

    def terminate(self) -> None:
        self.terminated = True
        self.returncode = 0

    def wait(self, timeout=None):
        self.returncode = 0
        return 0

    def kill(self) -> None:
        self.killed = True
        self.returncode = -9


class _FakeStdout:
    def __init__(self, line: str) -> None:
        self._line = line
        self.closed = False

    def fileno(self) -> int:
        return 0

    def readline(self) -> str:
        line = self._line
        self._line = ""
        return line

    def close(self) -> None:
        self.closed = True


class _FakeSelector:
    def register(self, *_args, **_kwargs) -> None:
        return None

    def select(self, timeout=None):
        return [(object(), object())]

    def close(self) -> None:
        return None


class _FakeCdpSession:
    def send(self, _method: str, _params=None):
        return {"data": ""}

    def detach(self) -> None:
        return None


class _FakePage:
    def __init__(self) -> None:
        self.url = "http://127.0.0.1:3000"
        self.screenshot_calls = []

    def is_closed(self) -> bool:
        return False

    def on(self, *_args, **_kwargs) -> None:
        return None

    def set_viewport_size(self, _payload) -> None:
        return None

    def goto(self, url: str, **_kwargs) -> None:
        self.url = url

    def screenshot(self, **kwargs):
        self.screenshot_calls.append(dict(kwargs))
        return b"frame"


class _FakeContext:
    def __init__(self) -> None:
        self.pages = [_FakePage()]

    def on(self, *_args, **_kwargs) -> None:
        return None

    def new_page(self):
        page = _FakePage()
        self.pages.append(page)
        return page

    def new_cdp_session(self, _page):
        return _FakeCdpSession()

    def storage_state(self):
        return {"cookies": [], "origins": []}

    def add_cookies(self, _payload) -> None:
        return None

    def cookies(self, _urls):
        return []

    def close(self) -> None:
        return None


class _FakeBrowser:
    def __init__(self) -> None:
        self.contexts = [_FakeContext()]


class _FakeChromium:
    def __init__(self) -> None:
        self.launch_calls = []
        self.connect_calls = []
        self.last_context = None
        self.last_browser = None

    def launch_persistent_context(self, **kwargs):
        self.launch_calls.append(kwargs)
        self.last_context = _FakeContext()
        return self.last_context

    def connect_over_cdp(self, endpoint: str):
        self.connect_calls.append(endpoint)
        self.last_browser = _FakeBrowser()
        return self.last_browser


class _FakePlaywright:
    def __init__(self) -> None:
        self.chromium = _FakeChromium()


class _FakePlaywrightCM:
    def __init__(self) -> None:
        self.playwright = _FakePlaywright()

    def __enter__(self):
        return self.playwright

    def __exit__(self, exc_type, exc, tb):
        return False


class _FakeSubmitRuntime:
    def __init__(self, page: _FakePage) -> None:
        self.page = page
        self.metadata = {"profile_dir": "/tmp/chatgpt-profile", "cdp_port": 4567, "pid": 7654}

    def current_url(self) -> str:
        return self.page.url


def _recv_socket_line(sock: socket.socket, *, timeout: float = 1.0) -> str:
    sock.settimeout(timeout)
    data = b""
    while b"\n" not in data:
        chunk = sock.recv(4096)
        if not chunk:
            break
        data += chunk
    return data.decode("utf-8", errors="replace")


class TestProjectedBrowserRuntime(unittest.TestCase):
    def test_projected_browser_session_captures_frames_only_for_viewers(self) -> None:
        from cccc.daemon.browser import projected_browser_runtime as runtime

        fake_cm = _FakePlaywrightCM()
        with (
            patch.object(runtime, "ensure_sync_playwright", return_value=lambda: fake_cm),
            patch.object(runtime, "_start_virtual_display", return_value=None),
            patch.object(runtime, "_system_browser_binaries", return_value=[]),
        ):
            manager = runtime.ProjectedBrowserSessionManager(idle_message="No test browser session.")
            try:
                state = manager.open(
                    key="test-capture-session",
                    profile_dir=runtime.Path("/tmp/projected-browser-capture-test"),
                    url="https://example.com",
                    width=1280,
                    height=800,
                    headless=False,
                    channel_candidates=(None,),
                )
                self.assertEqual(state["state"], "ready")

                time.sleep(0.25)
                # No viewer is attached, so the background session should not burn
                # screenshot work just to keep ChatGPT alive for delivery.
                self.assertEqual(manager.info(key="test-capture-session")["last_frame_seq"], 0)

                runtime_sock, viewer_sock = socket.socketpair()
                try:
                    self.assertTrue(manager.attach_socket(key="test-capture-session", sock=runtime_sock))
                    deadline = time.time() + 1.5
                    while manager.info(key="test-capture-session")["last_frame_seq"] <= 0 and time.time() < deadline:
                        time.sleep(0.05)
                    self.assertGreater(manager.info(key="test-capture-session")["last_frame_seq"], 0)
                finally:
                    try:
                        viewer_sock.sendall(b'{"t":"disconnect"}\n')
                    except Exception:
                        pass
                    try:
                        viewer_sock.close()
                    except Exception:
                        pass
            finally:
                manager.close(key="test-capture-session")

    def test_chatgpt_submit_prompt_command_uses_projected_session_page(self) -> None:
        from cccc.daemon.browser import projected_browser_runtime as runtime

        page = _FakePage()
        page.url = "https://chatgpt.com/"
        projected = _FakeSubmitRuntime(page)
        session = runtime.ProjectedBrowserSession(
            session_key="test-browser-session",
            profile_dir=runtime.Path("/tmp/projected-browser-test"),
            url="https://chatgpt.com",
            width=1280,
            height=800,
            headless=False,
            channel_candidates=("chrome",),
        )

        with (
            patch(
                "cccc.ports.web_model_browser_sidecar._submit_prompt",
                return_value={"send_selector": "#composer-submit-button", "submission_evidence": "stop_button"},
            ) as submit_prompt,
            patch("cccc.ports.web_model_browser_sidecar._mark_page_pending_delivery") as mark_pending,
            patch("cccc.ports.web_model_browser_sidecar._wait_for_conversation_url") as wait_conversation,
        ):
            result = session._apply_command(
                projected,
                "chatgpt_submit_prompt",
                {
                    "prompt": "review this change",
                    "target_url": "https://chatgpt.com/c/bound-session",
                    "auto_bind_new_chat": False,
                    "delivery_id": "delivery-1",
                    "input_timeout_seconds": 12,
                },
            )

        submit_prompt.assert_called_once_with(page, "review this change", input_timeout_seconds=12.0)
        mark_pending.assert_not_called()
        wait_conversation.assert_not_called()
        self.assertEqual(page.url, "https://chatgpt.com/c/bound-session")
        browser = result["browser"]
        self.assertEqual(browser["conversation_url"], "https://chatgpt.com/c/bound-session")
        self.assertFalse(browser["pending_conversation_url"])
        self.assertEqual(browser["submission_evidence"], "stop_button")
        self.assertEqual(browser["cdp_port"], 4567)
        self.assertEqual(browser["pid"], 7654)

    def test_chatgpt_submit_prompt_command_reports_pending_new_chat_bind(self) -> None:
        from cccc.daemon.browser import projected_browser_runtime as runtime

        page = _FakePage()
        page.url = "https://chatgpt.com/"
        projected = _FakeSubmitRuntime(page)
        session = runtime.ProjectedBrowserSession(
            session_key="test-browser-session",
            profile_dir=runtime.Path("/tmp/projected-browser-test"),
            url="https://chatgpt.com",
            width=1280,
            height=800,
            headless=False,
            channel_candidates=("chrome",),
        )

        with (
            patch(
                "cccc.ports.web_model_browser_sidecar._submit_prompt",
                return_value={"send_selector": "#composer-submit-button", "submission_evidence": "message_echo"},
            ),
            patch("cccc.ports.web_model_browser_sidecar._mark_page_pending_delivery") as mark_pending,
            patch("cccc.ports.web_model_browser_sidecar._wait_for_conversation_url", return_value="") as wait_conversation,
        ):
            result = session._apply_command(
                projected,
                "chatgpt_submit_prompt",
                {
                    "prompt": "start in a fresh chat",
                    "target_url": "https://chatgpt.com/",
                    "auto_bind_new_chat": True,
                    "delivery_id": "delivery-new-chat",
                    "new_chat_bind_timeout_seconds": 3,
                },
            )

        mark_pending.assert_called_once_with(page, "delivery-new-chat")
        wait_conversation.assert_called_once_with(page, timeout_seconds=3.0)
        browser = result["browser"]
        self.assertEqual(browser["conversation_url"], "")
        self.assertTrue(browser["pending_conversation_url"])
        self.assertTrue(browser["submitted_without_conversation_url"])
        self.assertEqual(browser["submission_evidence"], "message_echo")

    def test_multiple_projected_browser_viewers_do_not_evict_each_other(self) -> None:
        from cccc.daemon.browser import projected_browser_runtime as runtime

        session = runtime.ProjectedBrowserSession(
            session_key="test-browser-session",
            profile_dir=runtime.Path("/tmp/projected-browser-test"),
            url="https://chatgpt.com",
            width=1280,
            height=800,
            headless=False,
            channel_candidates=("chrome",),
        )
        first_runtime_sock, first_viewer_sock = socket.socketpair()
        second_runtime_sock, second_viewer_sock = socket.socketpair()
        try:
            self.assertTrue(session.attach_socket(first_runtime_sock))
            self.assertTrue(session.attach_socket(second_runtime_sock))

            first_line = _recv_socket_line(first_viewer_sock)
            second_line = _recv_socket_line(second_viewer_sock)
            self.assertIn('"t": "state"', first_line)
            self.assertIn('"t": "state"', second_line)
            self.assertTrue(session.snapshot()["controller_attached"])
        finally:
            for sock in (first_viewer_sock, second_viewer_sock):
                try:
                    sock.sendall(b'{"t":"disconnect"}\n')
                except Exception:
                    pass
                try:
                    sock.close()
                except Exception:
                    pass

    def test_headed_launch_uses_xvfb_env_when_display_missing(self) -> None:
        from cccc.daemon.browser import projected_browser_runtime as runtime

        xvfb_proc = _FakeProc()
        fake_cm = _FakePlaywrightCM()
        with patch.object(runtime, "ensure_sync_playwright", return_value=lambda: fake_cm), patch.object(
            runtime.shutil,
            "which",
            side_effect=lambda name: "/usr/bin/Xvfb" if name == "Xvfb" else None,
        ), patch.object(
            runtime.subprocess,
            "Popen",
            return_value=xvfb_proc,
        ), patch.object(
            runtime.selectors,
            "DefaultSelector",
            return_value=_FakeSelector(),
        ), patch.dict(runtime.os.environ, {}, clear=True):
            launched = runtime.launch_projected_browser_runtime(
                profile_dir=runtime.Path("/tmp/projected-browser-test"),
                url="https://example.com",
                width=1280,
                height=800,
                headless=False,
                channel_candidates=(None,),
            )

        launch_kwargs = fake_cm.playwright.chromium.launch_calls[0]
        self.assertFalse(bool(launch_kwargs.get("headless")))
        self.assertEqual(str((launch_kwargs.get("env") or {}).get("DISPLAY") or ""), ":123")
        self.assertIn("xvfb", str(getattr(launched, "strategy", "") or ""))
        launched.close()
        self.assertTrue(xvfb_proc.terminated or xvfb_proc.killed)

    def test_headed_launch_prefers_isolated_xvfb_even_when_display_exists(self) -> None:
        from cccc.daemon.browser import projected_browser_runtime as runtime

        xvfb_proc = _FakeProc()
        fake_cm = _FakePlaywrightCM()
        with patch.object(runtime, "ensure_sync_playwright", return_value=lambda: fake_cm), patch.object(
            runtime.shutil,
            "which",
            side_effect=lambda name: "/usr/bin/Xvfb" if name == "Xvfb" else None,
        ), patch.object(
            runtime.subprocess,
            "Popen",
            return_value=xvfb_proc,
        ), patch.object(
            runtime.selectors,
            "DefaultSelector",
            return_value=_FakeSelector(),
        ), patch.dict(runtime.os.environ, {"DISPLAY": ":0"}, clear=True):
            launched = runtime.launch_projected_browser_runtime(
                profile_dir=runtime.Path("/tmp/projected-browser-test"),
                url="https://example.com",
                width=1280,
                height=800,
                headless=False,
                channel_candidates=(None,),
            )

        launch_kwargs = fake_cm.playwright.chromium.launch_calls[0]
        self.assertEqual(str((launch_kwargs.get("env") or {}).get("DISPLAY") or ""), ":123")
        self.assertIn("xvfb", str(getattr(launched, "strategy", "") or ""))
        launched.close()
        self.assertTrue(xvfb_proc.terminated or xvfb_proc.killed)

    def test_headed_launch_does_not_fallback_to_host_display_when_isolation_fails(self) -> None:
        from cccc.daemon.browser import projected_browser_runtime as runtime

        fake_cm = _FakePlaywrightCM()
        with patch.object(runtime, "ensure_sync_playwright", return_value=lambda: fake_cm), patch.object(
            runtime, "_start_virtual_display", side_effect=RuntimeError("xvfb failed")
        ), patch.dict(runtime.os.environ, {"DISPLAY": ":0"}, clear=True):
            with self.assertRaisesRegex(RuntimeError, "xvfb failed"):
                runtime.launch_projected_browser_runtime(
                    profile_dir=runtime.Path("/tmp/projected-browser-test"),
                    url="https://example.com",
                    width=1280,
                    height=800,
                    headless=False,
                    channel_candidates=(None,),
                )

        self.assertEqual(fake_cm.playwright.chromium.launch_calls, [])

    def test_headed_launch_prefers_system_browser_cdp_when_available(self) -> None:
        from cccc.daemon.browser import projected_browser_runtime as runtime

        browser_proc = _FakeProc()
        fake_cm = _FakePlaywrightCM()
        with patch.object(runtime, "ensure_sync_playwright", return_value=lambda: fake_cm), patch.object(
            runtime, "_start_virtual_display", return_value=None
        ), patch.object(
            runtime, "_system_browser_binaries", return_value=["/usr/bin/google-chrome"]
        ), patch.object(
            runtime, "_pick_free_port", return_value=9222
        ), patch.object(
            runtime, "_wait_cdp_endpoint", return_value=True
        ), patch.object(
            runtime.subprocess, "Popen", return_value=browser_proc
        ), patch.dict(runtime.os.environ, {"DISPLAY": ":99"}, clear=True):
            launched = runtime.launch_projected_browser_runtime(
                profile_dir=runtime.Path("/tmp/projected-browser-test"),
                url="https://accounts.google.com",
                width=1280,
                height=800,
                headless=False,
                channel_candidates=("chrome", None),
            )

        self.assertEqual(fake_cm.playwright.chromium.connect_calls, ["http://127.0.0.1:9222"])
        self.assertEqual(fake_cm.playwright.chromium.launch_calls, [])
        self.assertIn("system_browser_cdp", str(getattr(launched, "strategy", "") or ""))
        launched.close()
        self.assertTrue(browser_proc.terminated or browser_proc.killed)

    def test_system_browser_can_use_profile_dir_directly(self) -> None:
        from cccc.daemon.browser import projected_browser_runtime as runtime

        browser_proc = _FakeProc()
        fake_cm = _FakePlaywrightCM()
        with patch.object(runtime, "ensure_sync_playwright", return_value=lambda: fake_cm), patch.object(
            runtime, "_start_virtual_display", return_value=None
        ), patch.object(
            runtime, "_system_browser_binaries", return_value=["/usr/bin/google-chrome"]
        ), patch.object(
            runtime, "_pick_free_port", return_value=9333
        ), patch.object(
            runtime, "_wait_cdp_endpoint", return_value=True
        ), patch.object(
            runtime.subprocess, "Popen", return_value=browser_proc
        ) as popen, patch.dict(runtime.os.environ, {"DISPLAY": ":99"}, clear=True):
            launched = runtime.launch_projected_browser_runtime(
                profile_dir=runtime.Path("/tmp/web-model-chatgpt-profile"),
                url="https://chatgpt.com",
                width=1280,
                height=800,
                headless=False,
                channel_candidates=("chrome",),
                system_profile_subdir="",
            )

        cmd = popen.call_args.args[0]
        self.assertIn("--user-data-dir=/tmp/web-model-chatgpt-profile", cmd)
        self.assertEqual(getattr(launched, "metadata", {}).get("cdp_port"), 9333)
        self.assertEqual(getattr(launched, "metadata", {}).get("pid"), 4321)
        launched.close()
        self.assertTrue(browser_proc.terminated or browser_proc.killed)

    def test_missing_browser_channels_can_fallback_to_managed_chromium(self) -> None:
        from cccc.daemon.browser import projected_browser_runtime as runtime

        fake_cm = _FakePlaywrightCM()
        launch_calls = []

        def launch_persistent_context(**kwargs):
            launch_calls.append(dict(kwargs))
            if kwargs.get("channel"):
                raise RuntimeError(f"Chromium distribution {kwargs.get('channel')!r} is not found")
            return _FakeContext()

        with patch.object(runtime, "ensure_sync_playwright", return_value=lambda: fake_cm), patch.object(
            runtime, "_start_virtual_display", return_value=None
        ), patch.object(
            runtime, "_system_browser_binaries", return_value=[]
        ), patch.object(
            fake_cm.playwright.chromium,
            "launch_persistent_context",
            side_effect=launch_persistent_context,
        ), patch.dict(runtime.os.environ, {"DISPLAY": ":99"}, clear=True):
            launched = runtime.launch_projected_browser_runtime(
                profile_dir=runtime.Path("/tmp/projected-browser-managed-fallback"),
                url="https://chatgpt.com",
                width=1280,
                height=800,
                headless=False,
                channel_candidates=("chrome", "msedge", None),
            )

        self.assertEqual([call.get("channel") for call in launch_calls], ["chrome", "msedge", None])
        self.assertEqual(str(getattr(launched, "strategy", "") or ""), "playwright_chromium")
        launched.close()

    def test_require_system_browser_cdp_disables_managed_chromium_fallback(self) -> None:
        from cccc.daemon.browser import projected_browser_runtime as runtime

        fake_cm = _FakePlaywrightCM()
        with patch.object(runtime, "ensure_sync_playwright", return_value=lambda: fake_cm), patch.object(
            runtime, "_start_virtual_display", return_value=None
        ), patch.object(
            runtime, "_system_browser_binaries", return_value=[]
        ), patch.dict(runtime.os.environ, {"DISPLAY": ":99"}, clear=True):
            with self.assertRaisesRegex(RuntimeError, "managed Playwright Chromium is not supported"):
                runtime.launch_projected_browser_runtime(
                    profile_dir=runtime.Path("/tmp/projected-browser-no-managed-fallback"),
                    url="https://chatgpt.com",
                    width=1280,
                    height=800,
                    headless=False,
                    channel_candidates=("chrome", "msedge", None),
                    require_system_browser_cdp=True,
                )

        self.assertEqual(fake_cm.playwright.chromium.launch_calls, [])

    def test_existing_system_browser_cdp_can_be_adopted_without_relaunch(self) -> None:
        from cccc.daemon.browser import projected_browser_runtime as runtime

        fake_cm = _FakePlaywrightCM()
        with patch.object(runtime, "ensure_sync_playwright", return_value=lambda: fake_cm), patch.object(
            runtime, "_start_virtual_display"
        ) as start_display, patch.object(
            runtime, "_wait_cdp_endpoint", return_value=True
        ), patch.object(
            runtime.subprocess, "Popen"
        ) as popen, patch.dict(runtime.os.environ, {"DISPLAY": ":99"}, clear=True):
            launched = runtime.launch_projected_browser_runtime(
                profile_dir=runtime.Path("/tmp/web-model-chatgpt-profile"),
                url="https://chatgpt.com/c/adopted-chat",
                width=1280,
                height=800,
                headless=False,
                channel_candidates=("chrome", "msedge"),
                require_system_browser_cdp=True,
                existing_cdp_port=9444,
                existing_browser_metadata={
                    "pid": 1234,
                    "profile_dir": "/tmp/web-model-chatgpt-profile",
                    "browser_binary": "/usr/bin/google-chrome",
                },
            )

        self.assertEqual(fake_cm.playwright.chromium.connect_calls, ["http://127.0.0.1:9444"])
        self.assertEqual(fake_cm.playwright.chromium.launch_calls, [])
        start_display.assert_not_called()
        popen.assert_not_called()
        self.assertEqual(str(getattr(launched, "strategy", "") or ""), "system_browser_cdp:adopted")
        self.assertTrue(bool((getattr(launched, "metadata", {}) or {}).get("adopted")))
        browser = fake_cm.playwright.chromium.last_browser
        self.assertEqual(browser.contexts[0].pages[0].url, "https://chatgpt.com/c/adopted-chat")
        launched.close()

    def test_capture_frame_uses_playwright_timeout(self) -> None:
        from cccc.daemon.browser import projected_browser_runtime as runtime

        page = _FakePage()
        context = _FakeContext()
        projected = runtime.PlaywrightProjectedRuntime(
            playwright_cm=_FakePlaywrightCM(),
            context=context,
            page=page,
            cdp_session=_FakeCdpSession(),
            width=1280,
            height=800,
            strategy="test",
        )

        self.assertEqual(projected.capture_frame(), b"frame")
        self.assertEqual(page.screenshot_calls[0].get("type"), "jpeg")
        self.assertEqual(page.screenshot_calls[0].get("quality"), 60)
        self.assertEqual(page.screenshot_calls[0].get("full_page"), False)
        self.assertEqual(page.screenshot_calls[0].get("timeout"), runtime._FRAME_CAPTURE_TIMEOUT_MS)


if __name__ == "__main__":
    unittest.main()
