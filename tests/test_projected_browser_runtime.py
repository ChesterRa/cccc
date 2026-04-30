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

    def is_closed(self) -> bool:
        return False

    def on(self, *_args, **_kwargs) -> None:
        return None

    def set_viewport_size(self, _payload) -> None:
        return None

    def goto(self, url: str, **_kwargs) -> None:
        self.url = url


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

    def launch_persistent_context(self, **kwargs):
        self.launch_calls.append(kwargs)
        return _FakeContext()

    def connect_over_cdp(self, endpoint: str):
        self.connect_calls.append(endpoint)
        return _FakeBrowser()


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


class TestProjectedBrowserRuntime(unittest.TestCase):
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


if __name__ == "__main__":
    unittest.main()
