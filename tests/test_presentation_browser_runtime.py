import json
import os
import socket
import tempfile
import time
import unittest
from unittest.mock import patch


class _FakeRuntime:
    def __init__(self) -> None:
        self.strategy = "fake_cdp"
        self._url = "http://127.0.0.1:3000"
        self.actions: list[tuple[str, object]] = []
        self.closed = False
        self.frames = 0

    def current_url(self) -> str:
        return self._url

    def capture_frame(self) -> bytes:
        self.frames += 1
        return f"frame-{self.frames}".encode("utf-8")

    def click(self, *, x: float, y: float, button: str = "left") -> None:
        self.actions.append(("click", (int(x), int(y), button)))

    def scroll(self, *, dx: float, dy: float) -> None:
        self.actions.append(("scroll", (int(dx), int(dy))))

    def key_press(self, *, key: str) -> None:
        self.actions.append(("key", key))

    def input_text(self, *, text: str) -> None:
        self.actions.append(("text", text))

    def resize(self, *, width: int, height: int) -> None:
        self.actions.append(("resize", (width, height)))

    def navigate(self, *, url: str) -> None:
        self._url = url
        self.actions.append(("navigate", url))

    def refresh(self) -> None:
        self.actions.append(("refresh", True))

    def back(self) -> None:
        self.actions.append(("back", True))

    def close(self) -> None:
        self.closed = True


class TestPresentationBrowserRuntime(unittest.TestCase):
    def _with_home(self):
        old_home = os.environ.get("CCCC_HOME")
        td_ctx = tempfile.TemporaryDirectory()
        td = td_ctx.__enter__()
        os.environ["CCCC_HOME"] = td

        def cleanup() -> None:
            td_ctx.__exit__(None, None, None)
            if old_home is None:
                os.environ.pop("CCCC_HOME", None)
            else:
                os.environ["CCCC_HOME"] = old_home

        return td, cleanup

    def _read_json_line(self, sock: socket.socket, timeout: float = 5.0) -> dict:
        sock.settimeout(timeout)
        buf = b""
        while b"\n" not in buf:
            chunk = sock.recv(65536)
            if not chunk:
                raise RuntimeError("socket closed before newline")
            buf += chunk
        line = buf.split(b"\n", 1)[0]
        return json.loads(line.decode("utf-8", errors="replace"))

    def test_runtime_attach_streams_frames_and_relays_commands(self) -> None:
        _, cleanup = self._with_home()
        try:
            from cccc.daemon.group import presentation_browser_runtime as runtime
            from cccc.daemon.browser import projected_browser_runtime as browser_runtime

            fake_runtime = _FakeRuntime()
            with patch.object(browser_runtime, "launch_projected_browser_runtime", return_value=fake_runtime):
                state = runtime.open_browser_surface_session(
                    group_id="g_demo",
                    slot_id="slot-1",
                    url="http://127.0.0.1:3000",
                    width=1280,
                    height=800,
                )
                self.assertEqual(state.get("state"), "ready")
                launch_kwargs = browser_runtime.launch_projected_browser_runtime.call_args.kwargs
                self.assertFalse(bool(launch_kwargs.get("headless")))
                self.assertEqual(tuple(launch_kwargs.get("channel_candidates") or ()), ("chrome", "msedge", None))

                left, right = socket.socketpair()
                try:
                    self.assertTrue(runtime.attach_browser_surface_socket(group_id="g_demo", slot_id="slot-1", sock=left))

                    first = self._read_json_line(right)
                    self.assertEqual(first.get("t"), "state")
                    self.assertEqual(first.get("state"), "ready")

                    frame = self._read_json_line(right)
                    self.assertEqual(frame.get("t"), "frame")
                    self.assertEqual(frame.get("mime"), "image/jpeg")
                    self.assertTrue(str(frame.get("data_base64") or "").strip())

                    right.sendall((json.dumps({"t": "click", "x": 120, "y": 240, "button": "left"}) + "\n").encode("utf-8"))
                    right.sendall((json.dumps({"t": "text", "text": "hello"}) + "\n").encode("utf-8"))
                    right.sendall((json.dumps({"t": "key", "key": "Enter"}) + "\n").encode("utf-8"))
                    right.sendall((json.dumps({"t": "scroll", "dx": 0, "dy": 360}) + "\n").encode("utf-8"))
                    right.sendall((json.dumps({"t": "resize", "width": 1440, "height": 900}) + "\n").encode("utf-8"))
                    right.sendall((json.dumps({"t": "back"}) + "\n").encode("utf-8"))
                    right.sendall((json.dumps({"t": "navigate", "url": "http://127.0.0.1:8848/demo"}) + "\n").encode("utf-8"))

                    deadline = time.time() + 3.0
                    while time.time() < deadline and len(fake_runtime.actions) < 7:
                        time.sleep(0.05)

                    self.assertIn(("click", (120, 240, "left")), fake_runtime.actions)
                    self.assertIn(("text", "hello"), fake_runtime.actions)
                    self.assertIn(("key", "Enter"), fake_runtime.actions)
                    self.assertIn(("scroll", (0, 360)), fake_runtime.actions)
                    self.assertIn(("resize", (1440, 900)), fake_runtime.actions)
                    self.assertIn(("back", True), fake_runtime.actions)
                    self.assertIn(("navigate", "http://127.0.0.1:8848/demo"), fake_runtime.actions)
                finally:
                    try:
                        right.close()
                    except Exception:
                        pass

                closed = runtime.close_browser_surface_session(group_id="g_demo", slot_id="slot-1")
                self.assertTrue(bool(closed.get("closed")))
                self.assertTrue(fake_runtime.closed)
        finally:
            try:
                from cccc.daemon.group.presentation_browser_runtime import close_all_browser_surface_sessions

                close_all_browser_surface_sessions()
            except Exception:
                pass
            cleanup()

    def test_socket_disconnect_keeps_session_alive_for_reconnect(self) -> None:
        _, cleanup = self._with_home()
        try:
            from cccc.daemon.group import presentation_browser_runtime as runtime
            from cccc.daemon.browser import projected_browser_runtime as browser_runtime

            fake_runtime = _FakeRuntime()
            with patch.object(browser_runtime, "launch_projected_browser_runtime", return_value=fake_runtime):
                state = runtime.open_browser_surface_session(
                    group_id="g_demo",
                    slot_id="slot-1",
                    url="http://127.0.0.1:3000",
                    width=1280,
                    height=800,
                )
                self.assertEqual(state.get("state"), "ready")

                left, right = socket.socketpair()
                self.assertTrue(runtime.attach_browser_surface_socket(group_id="g_demo", slot_id="slot-1", sock=left))
                _ = self._read_json_line(right)
                right.close()

                deadline = time.time() + 2.0
                snapshot = runtime.get_browser_surface_session_state(group_id="g_demo", slot_id="slot-1")
                while time.time() < deadline and snapshot.get("controller_attached"):
                    time.sleep(0.05)
                    snapshot = runtime.get_browser_surface_session_state(group_id="g_demo", slot_id="slot-1")

                self.assertEqual(snapshot.get("state"), "ready")
                self.assertTrue(bool(snapshot.get("active")))
                self.assertFalse(bool(snapshot.get("controller_attached")))

                left2, right2 = socket.socketpair()
                try:
                    self.assertTrue(runtime.attach_browser_surface_socket(group_id="g_demo", slot_id="slot-1", sock=left2))
                    second_state = self._read_json_line(right2)
                    self.assertEqual(second_state.get("t"), "state")
                    self.assertEqual(second_state.get("state"), "ready")
                finally:
                    try:
                        right2.close()
                    except Exception:
                        pass
                runtime.close_browser_surface_session(group_id="g_demo", slot_id="slot-1")
        finally:
            try:
                from cccc.daemon.group.presentation_browser_runtime import close_all_browser_surface_sessions

                close_all_browser_surface_sessions()
            except Exception:
                pass
            cleanup()

    def test_controller_input_does_not_wait_for_command_ack(self) -> None:
        _, cleanup = self._with_home()
        try:
            from cccc.daemon.group import presentation_browser_runtime as runtime
            from cccc.daemon.browser import projected_browser_runtime as browser_runtime

            fake_runtime = _FakeRuntime()
            with (
                patch.object(browser_runtime, "launch_projected_browser_runtime", return_value=fake_runtime),
                patch.object(browser_runtime.ProjectedBrowserSession, "submit_command") as submit_command,
            ):
                state = runtime.open_browser_surface_session(
                    group_id="g_demo",
                    slot_id="slot-1",
                    url="http://127.0.0.1:3000",
                    width=1280,
                    height=800,
                )
                self.assertEqual(state.get("state"), "ready")

                left, right = socket.socketpair()
                try:
                    self.assertTrue(runtime.attach_browser_surface_socket(group_id="g_demo", slot_id="slot-1", sock=left))
                    _ = self._read_json_line(right)
                    right.sendall((json.dumps({"t": "click", "x": 1, "y": 2, "button": "left"}) + "\n").encode("utf-8"))

                    deadline = time.time() + 2.0
                    while time.time() < deadline and not fake_runtime.actions:
                        time.sleep(0.05)

                    self.assertIn(("click", (1, 2, "left")), fake_runtime.actions)
                    submit_command.assert_not_called()
                finally:
                    try:
                        right.close()
                    except Exception:
                        pass
                runtime.close_browser_surface_session(group_id="g_demo", slot_id="slot-1")
        finally:
            try:
                from cccc.daemon.group.presentation_browser_runtime import close_all_browser_surface_sessions

                close_all_browser_surface_sessions()
            except Exception:
                pass
            cleanup()

    def test_runtime_sessions_are_slot_scoped(self) -> None:
        _, cleanup = self._with_home()
        try:
            from cccc.daemon.group import presentation_browser_runtime as runtime
            from cccc.daemon.browser import projected_browser_runtime as browser_runtime

            first_runtime = _FakeRuntime()
            second_runtime = _FakeRuntime()
            second_runtime._url = "http://127.0.0.1:4000"
            with patch.object(browser_runtime, "launch_projected_browser_runtime", side_effect=[first_runtime, second_runtime]):
                first = runtime.open_browser_surface_session(
                    group_id="g_demo",
                    slot_id="slot-1",
                    url="http://127.0.0.1:3000",
                    width=1280,
                    height=800,
                )
                second = runtime.open_browser_surface_session(
                    group_id="g_demo",
                    slot_id="slot-2",
                    url="http://127.0.0.1:4000",
                    width=1280,
                    height=800,
                )

                self.assertEqual(first.get("state"), "ready")
                self.assertEqual(second.get("state"), "ready")
                slot_one = runtime.get_browser_surface_session_state(group_id="g_demo", slot_id="slot-1")
                slot_two = runtime.get_browser_surface_session_state(group_id="g_demo", slot_id="slot-2")
                self.assertEqual(slot_one.get("url"), "http://127.0.0.1:3000")
                self.assertEqual(slot_two.get("url"), "http://127.0.0.1:4000")

                runtime.close_browser_surface_session(group_id="g_demo", slot_id="slot-1")
                slot_two_after = runtime.get_browser_surface_session_state(group_id="g_demo", slot_id="slot-2")
                self.assertEqual(slot_two_after.get("state"), "ready")
                self.assertTrue(bool(slot_two_after.get("active")))
        finally:
            try:
                from cccc.daemon.group.presentation_browser_runtime import close_all_browser_surface_sessions

                close_all_browser_surface_sessions()
            except Exception:
                pass
            cleanup()


if __name__ == "__main__":
    unittest.main()
