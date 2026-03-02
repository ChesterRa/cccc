import os
import time
import unittest
from pathlib import Path


@unittest.skipUnless(os.name == "nt", "Windows-only ConPTY backend check")
class TestWindowsPtyBackend(unittest.TestCase):
    def test_windows_pty_backend_is_available(self) -> None:
        from cccc.runners import pty as pty_runner

        self.assertTrue(
            bool(getattr(pty_runner, "PTY_SUPPORTED", False)),
            msg="Windows PTY backend unavailable (expected ConPTY via pywinpty)",
        )

    def test_conpty_session_smoke_echo_output(self) -> None:
        from cccc.runners import pty as pty_runner

        self.assertTrue(bool(getattr(pty_runner, "PTY_SUPPORTED", False)))

        session = pty_runner.PtySession(
            group_id="g_win",
            actor_id="a_win",
            cwd=Path.cwd(),
            command=["cmd.exe", "/c", "echo", "CCCC_CONPTY_OK"],
            env={},
        )
        try:
            deadline = time.time() + 8.0
            output = b""
            while time.time() < deadline:
                output = session.tail_output(max_bytes=200_000)
                if b"CCCC_CONPTY_OK" in output:
                    break
                if not session.is_running() and output:
                    break
                time.sleep(0.1)
            self.assertIn(
                b"CCCC_CONPTY_OK",
                output,
                msg=f"ConPTY session did not emit expected echo output. tail={output[-200:]}",
            )
        finally:
            session.stop()


if __name__ == "__main__":
    unittest.main()
