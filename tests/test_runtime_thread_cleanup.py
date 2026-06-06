import tempfile
import threading
import time
import unittest
from pathlib import Path


class TestRuntimeThreadCleanup(unittest.TestCase):
    def test_codex_stop_waits_for_worker_threads_to_exit(self) -> None:
        from cccc.daemon.codex_app_sessions import CodexAppSession

        class FakeProc:
            pid = 12345
            stdin = None

            def __init__(self) -> None:
                self._released = threading.Event()

            def poll(self):
                return None

            def terminate(self) -> None:
                self._released.set()

            def wait(self, timeout=None):
                return 0

            def kill(self) -> None:
                self._released.set()

        with tempfile.TemporaryDirectory() as td:
            session = CodexAppSession(group_id="g_cleanup", actor_id="peer1", cwd=Path(td), env={})
            proc = FakeProc()
            worker_started = threading.Event()

            def worker() -> None:
                worker_started.set()
                proc._released.wait(timeout=1)
                time.sleep(0.2)

            thread = threading.Thread(target=worker, name="test-codex-worker")
            thread.start()
            self.assertTrue(worker_started.wait(timeout=1))
            with session._lock:
                session._proc = proc  # type: ignore[assignment]
                session._running = True
                session._stdout_thread = thread

            session.stop()

            self.assertFalse(thread.is_alive())


if __name__ == "__main__":
    unittest.main()
