import base64
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
    def __init__(self) -> None:
        self.handlers = {}
        self.send_calls = []
        self.detached = False
        self.page_enabled = False

    def on(self, event: str, handler) -> None:
        self.handlers[str(event)] = handler

    def send(self, method: str, params=None):
        self.send_calls.append((str(method), dict(params or {})))
        if method == "Page.enable":
            self.page_enabled = True
        if method == "Page.startScreencast" and self.page_enabled:
            handler = self.handlers.get("Page.screencastFrame")
            if handler is not None:
                handler(
                    {
                        "data": base64.b64encode(b"frame").decode("ascii"),
                        "sessionId": 1,
                    }
                )
        return {"data": ""}

    def detach(self) -> None:
        self.detached = True


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

    def wait_for_timeout(self, _timeout_ms: int) -> None:
        return None


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

    def connect_over_cdp(self, endpoint: str, **kwargs):
        self.connect_calls.append((endpoint, dict(kwargs)))
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


class _FailingCaptureRuntime:
    strategy = "test"
    metadata = {}

    def __init__(self) -> None:
        self.url = "https://chatgpt.com/"
        self.capture_calls = 0
        self.closed = False

    def current_url(self) -> str:
        return self.url

    def capture_frame(self) -> bytes:
        self.capture_calls += 1
        raise RuntimeError("renderer is busy")

    def close(self) -> None:
        self.closed = True


class _CountingCaptureRuntime:
    strategy = "test"
    metadata = {}

    def __init__(self) -> None:
        self.url = "https://chatgpt.com/"
        self.capture_calls = 0
        self.command_urls: list[str] = []

    def current_url(self) -> str:
        return self.url

    def capture_frame(self) -> bytes:
        self.capture_calls += 1
        return b"frame"

    def navigate(self, url: str) -> None:
        self.command_urls.append(url)
        self.url = url

    def close(self) -> None:
        return None


class _FakeVncServer:
    display = ":123"
    port = 5901
    started_at = "2026-05-09T00:00:00Z"
    closed = False

    def alive(self) -> bool:
        return True

    def snapshot(self):
        return {
            "available": True,
            "display": self.display,
            "port": self.port,
            "started_at": self.started_at,
            "pid": 9876,
        }

    def close(self) -> None:
        self.closed = True


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
    def test_x11vnc_env_strips_wayland_session_markers(self) -> None:
        from cccc.daemon.browser import projected_browser_runtime as runtime

        with patch.dict(
            runtime.os.environ,
            {
                "DISPLAY": ":0",
                "WAYLAND_DISPLAY": "wayland-0",
                "WAYLAND_SOCKET": "socket",
                "XDG_SESSION_TYPE": "wayland",
                "PATH": "/usr/bin",
            },
            clear=True,
        ):
            env = runtime._x11vnc_env(":42")

        self.assertEqual(env.get("DISPLAY"), ":42")
        self.assertEqual(env.get("XDG_SESSION_TYPE"), "x11")
        self.assertNotIn("WAYLAND_DISPLAY", env)
        self.assertNotIn("WAYLAND_SOCKET", env)

    def test_x11vnc_start_uses_sanitized_x11_env(self) -> None:
        from cccc.daemon.browser import projected_browser_runtime as runtime

        captured: dict[str, object] = {}

        def fake_popen(_args, **kwargs):
            captured["env"] = dict(kwargs.get("env") or {})
            return _FakeProc()

        with patch.object(runtime.shutil, "which", return_value="/usr/bin/x11vnc"), patch.object(
            runtime.subprocess,
            "Popen",
            side_effect=fake_popen,
        ), patch.object(runtime, "_wait_tcp_endpoint", return_value=True), patch.dict(
            runtime.os.environ,
            {
                "DISPLAY": ":0",
                "WAYLAND_DISPLAY": "wayland-0",
                "WAYLAND_SOCKET": "socket",
                "XDG_SESSION_TYPE": "wayland",
                "PATH": "/usr/bin",
            },
            clear=True,
        ):
            server, error = runtime._ProjectedVncServer.start(display=":42", display_owned=True)

        self.assertEqual(error, "")
        self.assertIsNotNone(server)
        env = captured.get("env")
        self.assertIsInstance(env, dict)
        self.assertEqual((env or {}).get("DISPLAY"), ":42")
        self.assertEqual((env or {}).get("XDG_SESSION_TYPE"), "x11")
        self.assertNotIn("WAYLAND_DISPLAY", env or {})
        self.assertNotIn("WAYLAND_SOCKET", env or {})
        if server is not None:
            server.close()

    def test_x11vnc_refuses_non_cccc_owned_display(self) -> None:
        from cccc.daemon.browser import projected_browser_runtime as runtime

        with patch.object(runtime.shutil, "which", return_value="/usr/bin/x11vnc"), patch.object(
            runtime.subprocess,
            "Popen",
        ) as popen:
            server, error = runtime._ProjectedVncServer.start(display=":0", display_owned=False)

        self.assertIsNone(server)
        self.assertEqual(error, "display_not_cccc_owned")
        popen.assert_not_called()

    def test_x11vnc_error_summary_preserves_wayland_cause(self) -> None:
        from cccc.daemon.browser import projected_browser_runtime as runtime

        message = runtime._x11vnc_start_error(
            "x11vnc endpoint did not become ready",
            "Wayland display server detected. Exiting.",
        )
        self.assertIn("x11vnc_wayland_env_detected", message)

    def test_projected_browser_session_captures_frames_only_for_viewers(self) -> None:
        from cccc.daemon.browser import projected_browser_runtime as runtime

        fake_cm = _FakePlaywrightCM()
        with (
            patch.object(runtime, "ensure_sync_playwright", return_value=lambda: fake_cm),
            patch.object(runtime, "_start_virtual_display", return_value=None),
            patch.object(runtime, "_system_browser_binaries", return_value=[]),
            patch.object(runtime._ProjectedVncServer, "start", return_value=(None, "disabled")),
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

    def test_projected_browser_capture_failures_do_not_fail_session_or_block_commands(self) -> None:
        from cccc.daemon.browser import projected_browser_runtime as runtime

        fake_runtime = _FailingCaptureRuntime()
        with patch.object(runtime, "launch_projected_browser_runtime", return_value=fake_runtime):
            manager = runtime.ProjectedBrowserSessionManager(idle_message="No test browser session.")
            try:
                state = manager.open(
                    key="test-capture-failure-session",
                    profile_dir=runtime.Path("/tmp/projected-browser-capture-failure-test"),
                    url="https://chatgpt.com/",
                    width=1280,
                    height=800,
                    headless=False,
                    channel_candidates=("chrome",),
                )
                self.assertEqual(state["state"], "ready")

                runtime_sock, viewer_sock = socket.socketpair()
                try:
                    self.assertTrue(manager.attach_socket(key="test-capture-failure-session", sock=runtime_sock))
                    deadline = time.time() + 1.0
                    while fake_runtime.capture_calls <= 0 and time.time() < deadline:
                        time.sleep(0.05)
                    self.assertGreater(fake_runtime.capture_calls, 0)
                    self.assertEqual(manager.info(key="test-capture-failure-session")["state"], "ready")
                    self.assertEqual(manager.info(key="test-capture-failure-session")["last_frame_seq"], 0)
                    self.assertEqual(
                        manager.execute(key="test-capture-failure-session", kind="ping", payload={}, timeout=1.0),
                        {"ok": True},
                    )
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
                manager.close(key="test-capture-failure-session")

    def test_projected_browser_drains_commands_before_next_capture(self) -> None:
        from cccc.daemon.browser import projected_browser_runtime as runtime

        fake_runtime = _CountingCaptureRuntime()
        with patch.object(runtime, "launch_projected_browser_runtime", return_value=fake_runtime):
            manager = runtime.ProjectedBrowserSessionManager(idle_message="No test browser session.")
            try:
                state = manager.open(
                    key="test-command-priority-session",
                    profile_dir=runtime.Path("/tmp/projected-browser-command-priority-test"),
                    url="https://chatgpt.com/",
                    width=1280,
                    height=800,
                    headless=False,
                    channel_candidates=("chrome",),
                )
                self.assertEqual(state["state"], "ready")

                runtime_sock, viewer_sock = socket.socketpair()
                try:
                    self.assertTrue(manager.attach_socket(key="test-command-priority-session", sock=runtime_sock))
                    deadline = time.time() + 1.0
                    while fake_runtime.capture_calls <= 0 and time.time() < deadline:
                        time.sleep(0.05)
                    first_capture_count = fake_runtime.capture_calls
                    self.assertGreater(first_capture_count, 0)

                    for idx in range(3):
                        result = manager.execute(
                            key="test-command-priority-session",
                            kind="navigate",
                            payload={"url": f"https://chatgpt.com/c/test-{idx}"},
                            timeout=1.0,
                        )
                        self.assertTrue(result.get("ok"))

                    self.assertEqual(
                        fake_runtime.command_urls,
                        [
                            "https://chatgpt.com/c/test-0",
                            "https://chatgpt.com/c/test-1",
                            "https://chatgpt.com/c/test-2",
                        ],
                    )
                    self.assertEqual(fake_runtime.capture_calls, first_capture_count)
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
                manager.close(key="test-command-priority-session")

    def test_vnc_viewer_mode_suppresses_cdp_frame_capture(self) -> None:
        from cccc.daemon.browser import projected_browser_runtime as runtime

        fake_runtime = _CountingCaptureRuntime()
        fake_vnc = _FakeVncServer()
        with patch.object(runtime, "launch_projected_browser_runtime", return_value=fake_runtime), patch.object(
            runtime._ProjectedVncServer,
            "start",
            return_value=(fake_vnc, ""),
        ):
            manager = runtime.ProjectedBrowserSessionManager(idle_message="No test browser session.")
            try:
                state = manager.open(
                    key="test-vnc-session",
                    profile_dir=runtime.Path("/tmp/projected-browser-vnc-test"),
                    url="https://chatgpt.com/",
                    width=1280,
                    height=800,
                    headless=False,
                    channel_candidates=("chrome",),
                )
                self.assertEqual(state["state"], "ready")
                self.assertEqual(state["viewer"]["kind"], "vnc")

                runtime_sock, viewer_sock = socket.socketpair()
                try:
                    self.assertTrue(
                        manager.attach_socket_with_mode(
                            key="test-vnc-session",
                            sock=runtime_sock,
                            viewer_mode="auto",
                        )
                    )
                    line = _recv_socket_line(viewer_sock)
                    self.assertIn('"t": "state"', line)
                    time.sleep(0.25)
                    self.assertEqual(fake_runtime.capture_calls, 0)
                finally:
                    try:
                        viewer_sock.sendall(b'{"t":"disconnect"}\n')
                    except Exception:
                        pass
                    viewer_sock.close()
            finally:
                manager.close(key="test-vnc-session")
        self.assertTrue(fake_vnc.closed)

    def test_socket_command_read_does_not_wait_for_timeout(self) -> None:
        from cccc.daemon.browser import projected_browser_runtime as runtime

        runtime_sock, viewer_sock = socket.socketpair()
        try:
            runtime_sock.settimeout(1.0)
            started = time.time()
            incoming, buffer, disconnected = runtime._recv_json_line_nonblocking(runtime_sock, b"")
            elapsed = time.time() - started
            self.assertIsNone(incoming)
            self.assertEqual(buffer, b"")
            self.assertFalse(disconnected)
            self.assertLess(elapsed, 0.1)
        finally:
            runtime_sock.close()
            viewer_sock.close()

    def test_frame_stream_does_not_emit_state_for_every_frame(self) -> None:
        from cccc.daemon.browser import projected_browser_runtime as runtime

        fake_runtime = _CountingCaptureRuntime()
        with patch.object(runtime, "launch_projected_browser_runtime", return_value=fake_runtime):
            manager = runtime.ProjectedBrowserSessionManager(idle_message="No test browser session.")
            try:
                state = manager.open(
                    key="test-state-frame-decoupling-session",
                    profile_dir=runtime.Path("/tmp/projected-browser-state-frame-test"),
                    url="https://chatgpt.com/",
                    width=1280,
                    height=800,
                    headless=False,
                    channel_candidates=("chrome",),
                )
                self.assertEqual(state["state"], "ready")

                runtime_sock, viewer_sock = socket.socketpair()
                lines: list[str] = []
                buffer = b""
                try:
                    viewer_sock.settimeout(0.05)
                    self.assertTrue(manager.attach_socket(key="test-state-frame-decoupling-session", sock=runtime_sock))
                    deadline = time.time() + 0.8
                    while time.time() < deadline and len(lines) < 8:
                        try:
                            chunk = viewer_sock.recv(65536)
                        except socket.timeout:
                            continue
                        if not chunk:
                            break
                        buffer += chunk
                        while b"\n" in buffer:
                            line, buffer = buffer.split(b"\n", 1)
                            lines.append(line.decode("utf-8", errors="replace"))
                    state_lines = [line for line in lines if '"t": "state"' in line]
                    frame_lines = [line for line in lines if '"t": "frame"' in line]
                    self.assertGreaterEqual(len(frame_lines), 2)
                    self.assertEqual(len(state_lines), 1)
                finally:
                    try:
                        viewer_sock.sendall(b'{"t":"disconnect"}\n')
                    except Exception:
                        pass
                    viewer_sock.close()
            finally:
                manager.close(key="test-state-frame-decoupling-session")

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
                return_value={"send_selector": "#composer-submit-button", "submission_evidence": "message_echo"},
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

        self.assertEqual(submit_prompt.call_count, 1)
        submit_args, submit_kwargs = submit_prompt.call_args
        self.assertEqual(submit_args, (page, "review this change"))
        self.assertEqual(submit_kwargs.get("input_timeout_seconds"), 12.0)
        self.assertLessEqual(float(submit_kwargs.get("submit_timeout_seconds") or 0), 12.0)
        self.assertGreater(float(submit_kwargs.get("submit_timeout_seconds") or 0), 0.0)
        mark_pending.assert_not_called()
        wait_conversation.assert_not_called()
        self.assertEqual(page.url, "https://chatgpt.com/c/bound-session")
        browser = result["browser"]
        self.assertEqual(browser["conversation_url"], "https://chatgpt.com/c/bound-session")
        self.assertFalse(browser["pending_conversation_url"])
        self.assertEqual(browser["submission_evidence"], "message_echo")
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

    def test_chatgpt_auto_confirm_command_uses_projected_session_page(self) -> None:
        from cccc.daemon.browser import projected_browser_runtime as runtime

        page = _FakePage()
        page.url = "https://chatgpt.com/c/bound-session"
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

        with patch(
            "cccc.ports.web_model_browser_sidecar._auto_confirm_page_tool_prompts",
            return_value={
                "clicked": 1,
                "candidate_count": 1,
                "details": [{"title": "Run tool?", "label": "Confirm"}],
            },
        ) as auto_confirm:
            result = session._apply_command(
                projected,
                "chatgpt_auto_confirm_tools",
                {
                    "target_url": "https://chatgpt.com/c/bound-session",
                    "max_clicks": 2,
                },
            )

        auto_confirm.assert_called_once_with(page, max_clicks=2)
        self.assertTrue(result.get("browser_active"))
        self.assertEqual(result.get("clicked"), 1)
        self.assertEqual(result.get("candidate_count"), 1)
        self.assertEqual(result.get("pages_seen"), 1)
        self.assertEqual(result.get("page_url"), "https://chatgpt.com/c/bound-session")

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
        ), patch.object(
            runtime.sys,
            "platform",
            "linux",
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
        self.assertIn("--app=https://example.com", list(launch_kwargs.get("args") or []))
        self.assertIn("--window-position=0,0", list(launch_kwargs.get("args") or []))
        self.assertIn("xvfb", str(getattr(launched, "strategy", "") or ""))
        self.assertEqual((getattr(launched, "metadata", {}) or {}).get("display_owned"), True)
        self.assertEqual((getattr(launched, "metadata", {}) or {}).get("display_owner"), "cccc_xvfb")
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
        ), patch.object(
            runtime.sys,
            "platform",
            "linux",
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
        self.assertEqual((getattr(launched, "metadata", {}) or {}).get("display_owned"), True)
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

    def test_macos_headed_launch_does_not_require_display_or_xvfb(self) -> None:
        from cccc.daemon.browser import projected_browser_runtime as runtime

        browser_proc = _FakeProc()
        fake_cm = _FakePlaywrightCM()
        with patch.object(runtime.sys, "platform", "darwin"), patch.object(
            runtime, "ensure_sync_playwright", return_value=lambda: fake_cm
        ), patch.object(
            runtime, "_system_browser_binaries", return_value=["/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"]
        ), patch.object(
            runtime, "_pick_free_port", return_value=9444
        ), patch.object(
            runtime, "_wait_cdp_endpoint", return_value=True
        ), patch.object(
            runtime.subprocess, "Popen", return_value=browser_proc
        ) as popen, patch.dict(runtime.os.environ, {}, clear=True):
            launched = runtime.launch_projected_browser_runtime(
                profile_dir=runtime.Path("/tmp/projected-browser-macos-profile"),
                url="https://chatgpt.com",
                width=1280,
                height=800,
                headless=False,
                channel_candidates=("chrome",),
            )

        self.assertEqual(fake_cm.playwright.chromium.connect_calls, [("http://127.0.0.1:9444", {"timeout": 15000})])
        cmd = popen.call_args.args[0]
        self.assertIn("--app=https://chatgpt.com", cmd)
        metadata = getattr(launched, "metadata", {}) or {}
        self.assertEqual(metadata.get("display"), "")
        self.assertEqual(metadata.get("display_owned"), False)
        self.assertEqual(metadata.get("display_owner"), "")
        self.assertIn("system_browser_cdp", str(getattr(launched, "strategy", "") or ""))
        launched.close()
        self.assertTrue(browser_proc.terminated or browser_proc.killed)

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
        ) as popen, patch.dict(runtime.os.environ, {"DISPLAY": ":99"}, clear=True):
            launched = runtime.launch_projected_browser_runtime(
                profile_dir=runtime.Path("/tmp/projected-browser-test"),
                url="https://accounts.google.com",
                width=1280,
                height=800,
                headless=False,
                channel_candidates=("chrome", None),
            )

        self.assertEqual(fake_cm.playwright.chromium.connect_calls, [("http://127.0.0.1:9222", {"timeout": 15000})])
        self.assertEqual(fake_cm.playwright.chromium.launch_calls, [])
        cmd = popen.call_args.args[0]
        self.assertIn("--app=https://accounts.google.com", cmd)
        self.assertNotIn("https://accounts.google.com", [arg for arg in cmd if not str(arg).startswith("--app=")])
        self.assertIn("system_browser_cdp", str(getattr(launched, "strategy", "") or ""))
        self.assertEqual((getattr(launched, "metadata", {}) or {}).get("display_owned"), False)
        self.assertEqual((getattr(launched, "metadata", {}) or {}).get("display_owner"), "")
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

        self.assertEqual(fake_cm.playwright.chromium.connect_calls, [("http://127.0.0.1:9444", {"timeout": 15000})])
        self.assertEqual(fake_cm.playwright.chromium.launch_calls, [])
        start_display.assert_not_called()
        popen.assert_not_called()
        self.assertEqual(str(getattr(launched, "strategy", "") or ""), "system_browser_cdp:adopted")
        self.assertTrue(bool((getattr(launched, "metadata", {}) or {}).get("adopted")))
        browser = fake_cm.playwright.chromium.last_browser
        self.assertEqual(browser.contexts[0].pages[0].url, "https://chatgpt.com/c/adopted-chat")
        launched.close()

    def test_capture_frame_uses_cdp_screencast(self) -> None:
        from cccc.daemon.browser import projected_browser_runtime as runtime

        page = _FakePage()
        context = _FakeContext()
        cdp = _FakeCdpSession()
        projected = runtime.PlaywrightProjectedRuntime(
            playwright_cm=_FakePlaywrightCM(),
            context=context,
            page=page,
            cdp_session=cdp,
            width=1280,
            height=800,
            strategy="test",
        )

        self.assertEqual(projected.capture_frame(), b"frame")
        self.assertEqual(page.screenshot_calls, [])
        self.assertIn(("Page.enable", {}), cdp.send_calls)
        self.assertIn(
            (
                "Page.startScreencast",
                {
                    "format": "jpeg",
                    "quality": 70,
                    "maxWidth": 1280,
                    "maxHeight": 800,
                    "everyNthFrame": 1,
                },
            ),
            cdp.send_calls,
        )
        self.assertLess(
            cdp.send_calls.index(("Page.enable", {})),
            cdp.send_calls.index(
                (
                    "Page.startScreencast",
                    {
                        "format": "jpeg",
                        "quality": 70,
                        "maxWidth": 1280,
                        "maxHeight": 800,
                        "everyNthFrame": 1,
                    },
                )
            ),
        )
        self.assertIn(("Page.screencastFrameAck", {"sessionId": 1}), cdp.send_calls)
        projected.close()
        self.assertIn(("Page.stopScreencast", {}), cdp.send_calls)


if __name__ == "__main__":
    unittest.main()
