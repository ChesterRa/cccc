import unittest
import tempfile
from pathlib import Path
from unittest.mock import patch


class TestRuntimeCommandDefaults(unittest.TestCase):
    def test_kimi_runtime_uses_yolo_flags_for_launch(self) -> None:
        from cccc.kernel.runtime import get_runtime_command_with_flags

        self.assertEqual(get_runtime_command_with_flags("kimi"), ["kimi", "--yolo"])
        self.assertEqual(get_runtime_command_with_flags("hermes"), ["hermes", "--tui", "--yolo"])
        self.assertEqual(get_runtime_command_with_flags("opencode"), ["opencode"])
        self.assertEqual(get_runtime_command_with_flags("grok"), ["grok"])

    def test_cccc_mcp_stdio_command_prefers_unresolved_venv_entrypoint(self) -> None:
        from cccc.kernel.runtime import get_cccc_mcp_stdio_command

        with tempfile.TemporaryDirectory() as td:
            venv = Path(td) / ".venv"
            bin_dir = venv / "bin"
            bin_dir.mkdir(parents=True)
            python = bin_dir / "python"
            cccc = bin_dir / "cccc"
            python.write_text("", encoding="utf-8")
            cccc.write_text("", encoding="utf-8")
            with patch("cccc.kernel.runtime.sys.platform", "linux"), patch(
                "cccc.kernel.runtime.sys.executable",
                str(python),
            ), patch("cccc.kernel.runtime.sys.prefix", str(venv)), patch(
                "cccc.kernel.runtime.shutil.which",
                return_value=None,
            ):
                self.assertEqual(get_cccc_mcp_stdio_command(), [str(cccc.resolve()), "mcp"])


if __name__ == "__main__":
    unittest.main()
