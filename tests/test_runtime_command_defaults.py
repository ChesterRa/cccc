import unittest
import tempfile
import os
from pathlib import Path
from unittest.mock import patch


class TestRuntimeCommandDefaults(unittest.TestCase):
    def test_kimi_runtime_uses_yolo_flags_for_launch(self) -> None:
        from cccc.kernel.runtime import get_runtime_command_with_flags

        self.assertEqual(get_runtime_command_with_flags("copilot"), ["copilot", "--allow-all"])
        self.assertEqual(get_runtime_command_with_flags("cursor"), ["cursor-agent", "--yolo", "--approve-mcps"])
        self.assertEqual(get_runtime_command_with_flags("devin"), ["devin", "--permission-mode", "dangerous"])
        self.assertEqual(get_runtime_command_with_flags("kiro"), ["kiro-cli", "chat", "--trust-all-tools"])
        self.assertEqual(get_runtime_command_with_flags("kilo"), ["kilo"])
        self.assertEqual(get_runtime_command_with_flags("antigravity"), ["agy"])
        self.assertEqual(get_runtime_command_with_flags("kimi"), ["kimi", "--yolo"])
        self.assertEqual(get_runtime_command_with_flags("hermes"), ["hermes", "--tui", "--yolo"])
        self.assertEqual(get_runtime_command_with_flags("opencode"), ["opencode"])
        self.assertEqual(get_runtime_command_with_flags("grok"), ["grok"])

    def test_update_actor_accepts_cursor_runtime(self) -> None:
        from cccc.kernel.actors import add_actor, update_actor
        from cccc.kernel.group import create_group
        from cccc.kernel.registry import load_registry

        with tempfile.TemporaryDirectory() as td:
            old_home = os.environ.get("CCCC_HOME")
            try:
                os.environ["CCCC_HOME"] = td
                group = create_group(load_registry(), title="runtime-update")
                add_actor(group, actor_id="peer1", runtime="codex")

                actor = update_actor(group, "peer1", {"runtime": "cursor"})

                self.assertEqual(actor.get("runtime"), "cursor")
            finally:
                if old_home is None:
                    os.environ.pop("CCCC_HOME", None)
                else:
                    os.environ["CCCC_HOME"] = old_home

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
