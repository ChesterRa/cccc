import os
import tempfile
import unittest
from pathlib import Path
from unittest.mock import Mock, patch

import yaml  # type: ignore


class TestHermesRuntime(unittest.TestCase):
    def _with_home(self):
        old_home = os.environ.get("CCCC_HOME")
        old_hermes_home = os.environ.get("HERMES_HOME")
        td_ctx = tempfile.TemporaryDirectory()
        td = td_ctx.__enter__()
        os.environ["CCCC_HOME"] = td
        os.environ.pop("HERMES_HOME", None)

        def cleanup() -> None:
            if old_home is None:
                os.environ.pop("CCCC_HOME", None)
            else:
                os.environ["CCCC_HOME"] = old_home
            if old_hermes_home is None:
                os.environ.pop("HERMES_HOME", None)
            else:
                os.environ["HERMES_HOME"] = old_hermes_home
            td_ctx.__exit__(None, None, None)

        return Path(td), cleanup

    def _write_ready_config(self, config_path: Path, command: list[str]) -> None:
        config_path.parent.mkdir(parents=True, exist_ok=True)
        config_path.write_text(
            yaml.safe_dump(
                {
                    "mcp_servers": {
                        "cccc": {
                            "command": command[0],
                            "args": command[1:],
                            "env": {
                                "CCCC_HOME": "${CCCC_HOME}",
                                "CCCC_GROUP_ID": "${CCCC_GROUP_ID}",
                                "CCCC_ACTOR_ID": "${CCCC_ACTOR_ID}",
                            },
                            "enabled": True,
                        }
                    }
                },
                sort_keys=False,
            ),
            encoding="utf-8",
        )

    def test_status_defaults_to_user_hermes_home(self) -> None:
        from cccc.kernel.hermes_runtime import hermes_runtime_status

        cccc_home, cleanup = self._with_home()
        user_home = cccc_home / "user"
        try:
            with patch("cccc.kernel.hermes_runtime.Path.home", return_value=user_home), patch(
                "cccc.kernel.hermes_runtime.find_subprocess_executable",
                return_value="/usr/bin/hermes",
            ), patch(
                "cccc.kernel.hermes_runtime._hermes_version",
                return_value="Hermes Agent v0.14.0",
            ):
                status = hermes_runtime_status(home=cccc_home)

            self.assertEqual(status["phase"], "phase1_pty_runtime_mvp")
            self.assertTrue(status["user_facing_actor_runtime_enabled"])
            self.assertFalse(status["setup_ready"])
            self.assertFalse(status["launch_ready"])
            self.assertEqual(status["hermes_home"], str(user_home / ".hermes"))
            self.assertEqual(status["profile"]["name"], "default")
            self.assertIn("profile_missing", status["issues"])
        finally:
            cleanup()

    def test_status_respects_explicit_hermes_home_env(self) -> None:
        from cccc.kernel.hermes_runtime import hermes_runtime_status

        cccc_home, cleanup = self._with_home()
        explicit_home = cccc_home / "explicit-hermes"
        command = ["/abs/cccc", "mcp"]
        try:
            os.environ["HERMES_HOME"] = str(explicit_home)
            self._write_ready_config(explicit_home / "config.yaml", command)
            with patch("cccc.kernel.hermes_runtime.get_cccc_mcp_stdio_command", return_value=command), patch(
                "cccc.kernel.hermes_runtime.find_subprocess_executable",
                return_value="/usr/bin/hermes",
            ):
                status = hermes_runtime_status(home=cccc_home, include_version=False)

            self.assertEqual(status["hermes_home"], str(explicit_home))
            self.assertEqual(status["profile"]["dir"], str(explicit_home))
            self.assertEqual(status["profile"]["config_path"], str(explicit_home / "config.yaml"))
            self.assertEqual(status["mcp"]["status"], "ready")
        finally:
            cleanup()

    def test_status_requires_mcp_placeholders_not_static_actor_ids(self) -> None:
        from cccc.kernel.hermes_runtime import hermes_runtime_status

        cccc_home, cleanup = self._with_home()
        user_home = cccc_home / "user"
        command = ["/abs/cccc", "mcp"]
        config_path = user_home / ".hermes" / "config.yaml"
        try:
            self._write_ready_config(config_path, command)
            with patch("cccc.kernel.hermes_runtime.Path.home", return_value=user_home), patch(
                "cccc.kernel.hermes_runtime.get_cccc_mcp_stdio_command",
                return_value=command,
            ), patch(
                "cccc.kernel.hermes_runtime.find_subprocess_executable",
                return_value="/usr/bin/hermes",
            ):
                ready = hermes_runtime_status(home=cccc_home, include_version=False)
            self.assertEqual(ready["mcp"]["status"], "ready")

            doc = yaml.safe_load(config_path.read_text(encoding="utf-8")) or {}
            doc["mcp_servers"]["cccc"]["env"]["CCCC_ACTOR_ID"] = "peer1"
            config_path.write_text(yaml.safe_dump(doc, sort_keys=False), encoding="utf-8")
            with patch("cccc.kernel.hermes_runtime.Path.home", return_value=user_home), patch(
                "cccc.kernel.hermes_runtime.get_cccc_mcp_stdio_command",
                return_value=command,
            ), patch(
                "cccc.kernel.hermes_runtime.find_subprocess_executable",
                return_value="/usr/bin/hermes",
            ):
                stale = hermes_runtime_status(home=cccc_home, include_version=False)
            self.assertEqual(stale["mcp"]["status"], "stale")
            self.assertFalse(stale["mcp"]["env_placeholders_match"])
        finally:
            cleanup()

    def test_prepare_uses_default_profile_mcp_add_with_confirmation(self) -> None:
        from cccc.kernel import hermes_runtime

        cccc_home, cleanup = self._with_home()
        user_home = cccc_home / "user"
        command = ["/abs/cccc", "mcp"]
        config_path = user_home / ".hermes" / "config.yaml"
        calls: list[tuple[list[str], str | None, str]] = []

        def fake_run(argv, *, hermes_home_path=None, cwd=None, timeout=60, input_text=None, extra_env=None):
            calls.append((list(argv), input_text, str(hermes_home_path or "")))
            if argv[:4] == ["hermes", "mcp", "add", "cccc"]:
                self.assertEqual(input_text, "Y\n")
                self.assertIsNone(hermes_home_path)
                self._write_ready_config(config_path, command)
            return Mock(returncode=0, stdout="", stderr="")

        try:
            with patch("cccc.kernel.hermes_runtime.Path.home", return_value=user_home), patch(
                "cccc.kernel.hermes_runtime.find_subprocess_executable",
                return_value="/usr/bin/hermes",
            ), patch(
                "cccc.kernel.hermes_runtime.get_cccc_mcp_stdio_command",
                return_value=command,
            ), patch.object(hermes_runtime, "_run_hermes_cli", side_effect=fake_run):
                result = hermes_runtime.prepare_hermes_runtime(home=cccc_home, auto_enable_tools=True)

            self.assertTrue(result.get("ok"), result)
            self.assertEqual(
                calls[0][0],
                [
                    "hermes",
                    "mcp",
                    "add",
                    "cccc",
                    "--command",
                    "/abs/cccc",
                    "--args",
                    "mcp",
                    "--env",
                    f"CCCC_HOME={cccc_home}",
                    "CCCC_GROUP_ID=g_probe",
                    "CCCC_ACTOR_ID=hermes-probe",
                ],
            )
            doc = yaml.safe_load(config_path.read_text(encoding="utf-8")) or {}
            env = doc["mcp_servers"]["cccc"]["env"]
            self.assertEqual(env["CCCC_HOME"], "${CCCC_HOME}")
            self.assertEqual(env["CCCC_GROUP_ID"], "${CCCC_GROUP_ID}")
            self.assertEqual(env["CCCC_ACTOR_ID"], "${CCCC_ACTOR_ID}")
        finally:
            cleanup()

    def test_prepare_skips_mcp_add_when_config_ready(self) -> None:
        from cccc.kernel import hermes_runtime

        cccc_home, cleanup = self._with_home()
        user_home = cccc_home / "user"
        command = ["/abs/cccc", "mcp"]
        try:
            self._write_ready_config(user_home / ".hermes" / "config.yaml", command)
            with patch("cccc.kernel.hermes_runtime.Path.home", return_value=user_home), patch(
                "cccc.kernel.hermes_runtime.find_subprocess_executable",
                return_value="/usr/bin/hermes",
            ), patch(
                "cccc.kernel.hermes_runtime.get_cccc_mcp_stdio_command",
                return_value=command,
            ), patch.object(hermes_runtime, "_run_hermes_cli") as mock_run:
                result = hermes_runtime.prepare_hermes_runtime(home=cccc_home, auto_enable_tools=True)

            self.assertTrue(result.get("ok"), result)
            mock_run.assert_not_called()
        finally:
            cleanup()

    def test_placeholder_normalization_preserves_unrelated_comments(self) -> None:
        from cccc.kernel.hermes_runtime import _normalize_mcp_config_placeholders

        cccc_home, cleanup = self._with_home()
        try:
            config_path = cccc_home / "user" / ".hermes" / "config.yaml"
            config_path.parent.mkdir(parents=True, exist_ok=True)
            config_path.write_text(
                (
                    "# keep header\n"
                    "model: ''\n"
                    "mcp_servers:\n"
                    "  cccc:\n"
                    "    command: /abs/cccc\n"
                    "    args:\n"
                    "    - mcp\n"
                    "    env:\n"
                    f"      CCCC_HOME: {cccc_home}\n"
                    "      CCCC_GROUP_ID: g_probe\n"
                    "      CCCC_ACTOR_ID: hermes-probe\n"
                    "    enabled: true\n"
                    "# keep footer\n"
                ),
                encoding="utf-8",
            )

            _normalize_mcp_config_placeholders(config_path)

            text = config_path.read_text(encoding="utf-8")
            self.assertIn("# keep header", text)
            self.assertIn("# keep footer", text)
            self.assertIn("CCCC_HOME: ${CCCC_HOME}", text)
            self.assertIn("CCCC_GROUP_ID: ${CCCC_GROUP_ID}", text)
            self.assertIn("CCCC_ACTOR_ID: ${CCCC_ACTOR_ID}", text)
        finally:
            cleanup()


if __name__ == "__main__":
    unittest.main()
