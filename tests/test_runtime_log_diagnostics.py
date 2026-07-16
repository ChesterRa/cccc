import unittest


class TestRuntimeLogDiagnostics(unittest.TestCase):
    def test_exit_marker_detection_does_not_enable_log_reading(self) -> None:
        from cccc.daemon.runtime_log_diagnostics import runtime_log_tail, terminal_output_needs_runtime_log

        marker = "Process exited with code 1 before producing terminal output."

        self.assertTrue(terminal_output_needs_runtime_log(marker))
        self.assertEqual(runtime_log_tail("kimi", env={"HOME": "/private"}), "")

    def test_empty_terminal_output_does_not_enable_log_reading(self) -> None:
        from cccc.daemon.runtime_log_diagnostics import runtime_log_tail, terminal_output_needs_runtime_log

        self.assertFalse(terminal_output_needs_runtime_log(""))
        self.assertEqual(runtime_log_tail("kimi", env={"KIMI_SHARE_DIR": "/shared"}), "")


if __name__ == "__main__":
    unittest.main()
