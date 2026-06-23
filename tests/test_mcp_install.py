import json
import os
import subprocess
import tempfile
import unittest
from pathlib import Path
from unittest.mock import Mock, call, patch

from cccc.daemon.mcp_install import build_mcp_add_command, ensure_mcp_installed, is_mcp_installed, prepare_runtime_mcp_env
from cccc.kernel.runtime import get_cccc_mcp_stdio_command


class TestMcpInstall(unittest.TestCase):
    def test_is_mcp_installed_unknown_runtime_false(self) -> None:
        self.assertFalse(is_mcp_installed("unknown-runtime"))

    def test_ensure_mcp_installed_skips_non_auto_runtime(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            cwd = Path(td)
            with patch("cccc.daemon.mcp_install.subprocess.run") as mock_run:
                ok = ensure_mcp_installed("unknown-runtime", cwd, auto_mcp_runtimes=("claude", "codex"))
                self.assertTrue(ok)
                mock_run.assert_not_called()

    def test_prepare_runtime_mcp_env_opencode_injects_inline_config(self) -> None:
        env = {
            "CCCC_HOME": "/tmp/cccc-home",
            "CCCC_GROUP_ID": "g_123",
            "CCCC_ACTOR_ID": "peer1",
            "OPENCODE_CONFIG_CONTENT": json.dumps({"mcp": {"other": {"type": "local", "command": ["other"]}}}),
        }

        with patch("cccc.daemon.mcp_install.get_cccc_mcp_stdio_command", return_value=["/abs/cccc", "mcp"]):
            prepared = prepare_runtime_mcp_env("opencode", env)

        doc = json.loads(prepared["OPENCODE_CONFIG_CONTENT"])
        self.assertEqual(doc["mcp"]["other"]["command"], ["other"])
        self.assertEqual(
            doc["mcp"]["cccc"],
            {
                "type": "local",
                "command": ["/abs/cccc", "mcp"],
                "enabled": True,
                "environment": {
                    "CCCC_HOME": "/tmp/cccc-home",
                    "CCCC_GROUP_ID": "g_123",
                    "CCCC_ACTOR_ID": "peer1",
                },
            },
        )
        with patch("cccc.daemon.mcp_install.get_cccc_mcp_stdio_command", return_value=["/abs/cccc", "mcp"]):
            self.assertTrue(is_mcp_installed("opencode", env=prepared))

    def test_ensure_mcp_installed_opencode_uses_prepared_env_without_cli(self) -> None:
        env = prepare_runtime_mcp_env(
            "opencode",
            {
                "CCCC_HOME": "/tmp/cccc-home",
                "CCCC_GROUP_ID": "g_123",
                "CCCC_ACTOR_ID": "peer1",
            },
        )
        with tempfile.TemporaryDirectory() as td:
            cwd = Path(td)
            with patch("cccc.daemon.mcp_install.subprocess.run") as mock_run:
                ok = ensure_mcp_installed("opencode", cwd, auto_mcp_runtimes=("opencode",), env=env)
                self.assertTrue(ok)
                mock_run.assert_not_called()

    def test_ensure_mcp_installed_opencode_missing_inline_config_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            cwd = Path(td)
            with patch("cccc.daemon.mcp_install.subprocess.run") as mock_run:
                ok = ensure_mcp_installed("opencode", cwd, auto_mcp_runtimes=("opencode",), env={})
                self.assertFalse(ok)
                mock_run.assert_not_called()

    def test_build_mcp_add_command_hermes_uses_safe_prepare_wrapper(self) -> None:
        with patch("cccc.daemon.mcp_install.get_cccc_mcp_stdio_command", return_value=["/abs/cccc", "mcp"]):
            self.assertEqual(
                build_mcp_add_command("hermes"),
                ["cccc", "runtime", "hermes", "prepare", "--yes"],
            )

    def test_build_mcp_add_command_grok_uses_command_args_and_unbuffered_env(self) -> None:
        with patch("cccc.daemon.mcp_install.get_cccc_mcp_stdio_command", return_value=["/abs/cccc", "mcp"]):
            self.assertEqual(
                build_mcp_add_command("grok"),
                [
                    "grok",
                    "mcp",
                    "add",
                    "cccc",
                    "--command",
                    "/abs/cccc",
                    "--args",
                    "mcp",
                    "--env",
                    "PYTHONUNBUFFERED=1",
                ],
            )

    def test_build_mcp_add_command_grok_handles_python_module_fallback_args(self) -> None:
        with patch(
            "cccc.daemon.mcp_install.get_cccc_mcp_stdio_command",
            return_value=["/usr/bin/python", "-m", "cccc.ports.mcp.main"],
        ):
            self.assertEqual(
                build_mcp_add_command("grok"),
                [
                    "grok",
                    "mcp",
                    "add",
                    "cccc",
                    "--command",
                    "/usr/bin/python",
                    "--args=-m",
                    "--args=cccc.ports.mcp.main",
                    "--env",
                    "PYTHONUNBUFFERED=1",
                ],
            )

    def test_build_mcp_add_command_devin_uses_user_scope_stdio(self) -> None:
        with patch("cccc.daemon.mcp_install.get_cccc_mcp_stdio_command", return_value=["/abs/cccc", "mcp"]):
            self.assertEqual(
                build_mcp_add_command("devin"),
                ["devin", "mcp", "add", "-s", "user", "cccc", "--", "/abs/cccc", "mcp"],
            )

    def test_build_mcp_add_command_kiro_uses_global_stdio_config(self) -> None:
        with patch("cccc.daemon.mcp_install.get_cccc_mcp_stdio_command", return_value=["/abs/cccc", "mcp"]):
            self.assertEqual(
                build_mcp_add_command("kiro"),
                [
                    "kiro-cli",
                    "mcp",
                    "add",
                    "--name",
                    "cccc",
                    "--scope",
                    "global",
                    "--command",
                    "/abs/cccc",
                    "--args=mcp",
                    "--force",
                ],
            )

    def test_build_mcp_add_command_kiro_handles_python_module_fallback_args(self) -> None:
        with patch(
            "cccc.daemon.mcp_install.get_cccc_mcp_stdio_command",
            return_value=["/usr/bin/python", "-m", "cccc.ports.mcp.main"],
        ):
            self.assertEqual(
                build_mcp_add_command("kiro"),
                [
                    "kiro-cli",
                    "mcp",
                    "add",
                    "--name",
                    "cccc",
                    "--scope",
                    "global",
                    "--command",
                    "/usr/bin/python",
                    "--args=-m",
                    "--args=cccc.ports.mcp.main",
                    "--force",
                ],
            )

    def test_is_mcp_installed_devin_parses_stdio_debug_output(self) -> None:
        output = (
            'Server: cccc\n'
            'Stdio(McpServerStdio { name: "cccc", command: "/abs/cccc", args: ["mcp"], env: [], meta: None })\n'
        )
        with patch("cccc.daemon.mcp_install.get_cccc_mcp_stdio_command", return_value=["/abs/cccc", "mcp"]), patch(
            "cccc.daemon.mcp_install._run_cli",
            return_value=Mock(returncode=0, stdout=output, stderr=""),
        ) as mock_run:
            self.assertTrue(is_mcp_installed("devin"))
        mock_run.assert_called_once_with(["devin", "mcp", "get", "cccc"], timeout=10, env=None)

    def test_is_mcp_installed_devin_rejects_wrong_stdio_command(self) -> None:
        output = (
            'Server: cccc\n'
            'Stdio(McpServerStdio { name: "cccc", command: "/old/cccc", args: ["mcp"], env: [], meta: None })\n'
        )
        with patch("cccc.daemon.mcp_install.get_cccc_mcp_stdio_command", return_value=["/abs/cccc", "mcp"]), patch(
            "cccc.daemon.mcp_install._run_cli",
            return_value=Mock(returncode=0, stdout=output, stderr=""),
        ):
            self.assertFalse(is_mcp_installed("devin"))

    def test_is_mcp_installed_devin_rejects_missing_stdio_command(self) -> None:
        output = (
            "Server: cccc\n"
            'Stdio(McpServerStdio { name: "cccc", args: ["mcp"], env: [], meta: None })\n'
        )
        with patch("cccc.daemon.mcp_install.get_cccc_mcp_stdio_command", return_value=["/abs/cccc", "mcp"]), patch(
            "cccc.daemon.mcp_install._run_cli",
            return_value=Mock(returncode=0, stdout=output, stderr=""),
        ):
            self.assertFalse(is_mcp_installed("devin"))

    def test_is_mcp_installed_devin_checks_actor_cwd_when_provided(self) -> None:
        output = (
            'Server: cccc\n'
            'Stdio(McpServerStdio { name: "cccc", command: "/abs/cccc", args: ["mcp"], env: [], meta: None })\n'
        )
        with tempfile.TemporaryDirectory() as td:
            cwd = Path(td)
            with patch("cccc.daemon.mcp_install.get_cccc_mcp_stdio_command", return_value=["/abs/cccc", "mcp"]), patch(
                "cccc.daemon.mcp_install._run_cli",
                return_value=Mock(returncode=0, stdout=output, stderr=""),
            ) as mock_run:
                self.assertTrue(is_mcp_installed("devin", cwd=cwd))
        mock_run.assert_called_once_with(["devin", "mcp", "get", "cccc"], cwd=cwd, timeout=10, env=None)

    def test_is_mcp_installed_kiro_reads_kiro_home_settings_config(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            kiro_home = Path(td) / "kiro-home"
            config_path = kiro_home / "settings" / "mcp.json"
            config_path.parent.mkdir(parents=True, exist_ok=True)
            config_path.write_text(
                json.dumps(
                    {
                        "mcpServers": {
                            "cccc": {
                                "command": "/abs/cccc",
                                "args": ["mcp"],
                                "env": {},
                            }
                        }
                    }
                ),
                encoding="utf-8",
            )

            with patch("cccc.daemon.mcp_install.get_cccc_mcp_stdio_command", return_value=["/abs/cccc", "mcp"]):
                self.assertTrue(is_mcp_installed("kiro", env={"KIRO_HOME": str(kiro_home)}))

    def test_is_mcp_installed_kiro_uses_process_kiro_home_env(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            kiro_home = Path(td) / "kiro-home"
            config_path = kiro_home / "settings" / "mcp.json"
            config_path.parent.mkdir(parents=True, exist_ok=True)
            config_path.write_text(
                json.dumps({"mcpServers": {"cccc": {"command": "/abs/cccc", "args": ["mcp"], "env": {}}}}),
                encoding="utf-8",
            )

            with patch.dict(os.environ, {"KIRO_HOME": str(kiro_home)}, clear=False), patch(
                "cccc.daemon.mcp_install.get_cccc_mcp_stdio_command",
                return_value=["/abs/cccc", "mcp"],
            ):
                self.assertTrue(is_mcp_installed("kiro"))

    def test_is_mcp_installed_kiro_rejects_stale_command(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            kiro_home = Path(td) / "kiro-home"
            config_path = kiro_home / "settings" / "mcp.json"
            config_path.parent.mkdir(parents=True, exist_ok=True)
            config_path.write_text(
                json.dumps({"mcpServers": {"cccc": {"command": "/old/cccc", "args": ["mcp"], "env": {}}}}),
                encoding="utf-8",
            )

            with patch("cccc.daemon.mcp_install.get_cccc_mcp_stdio_command", return_value=["/abs/cccc", "mcp"]):
                self.assertFalse(is_mcp_installed("kiro", env={"KIRO_HOME": str(kiro_home)}))

    def test_is_mcp_installed_kiro_prefers_local_stale_config_over_global_ready(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            cwd = root / "repo"
            kiro_home = root / "kiro-home"
            local_config_path = cwd / ".kiro" / "settings" / "mcp.json"
            global_config_path = kiro_home / "settings" / "mcp.json"
            local_config_path.parent.mkdir(parents=True, exist_ok=True)
            global_config_path.parent.mkdir(parents=True, exist_ok=True)
            local_config_path.write_text(
                json.dumps({"mcpServers": {"cccc": {"command": "/old/cccc", "args": ["mcp"], "env": {}}}}),
                encoding="utf-8",
            )
            global_config_path.write_text(
                json.dumps({"mcpServers": {"cccc": {"command": "/abs/cccc", "args": ["mcp"], "env": {}}}}),
                encoding="utf-8",
            )

            with patch("cccc.daemon.mcp_install.get_cccc_mcp_stdio_command", return_value=["/abs/cccc", "mcp"]):
                self.assertFalse(is_mcp_installed("kiro", cwd=cwd, env={"KIRO_HOME": str(kiro_home)}))

    def test_is_mcp_installed_kiro_accepts_local_ready_config(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            cwd = Path(td) / "repo"
            local_config_path = cwd / ".kiro" / "settings" / "mcp.json"
            local_config_path.parent.mkdir(parents=True, exist_ok=True)
            local_config_path.write_text(
                json.dumps({"mcpServers": {"cccc": {"command": "/abs/cccc", "args": ["mcp"], "env": {}}}}),
                encoding="utf-8",
            )

            with patch("cccc.daemon.mcp_install.get_cccc_mcp_stdio_command", return_value=["/abs/cccc", "mcp"]):
                self.assertTrue(is_mcp_installed("kiro", cwd=cwd, env={"KIRO_HOME": str(Path(td) / "kiro-home")}))

    def test_ensure_mcp_installed_devin_adds_cccc_stdio(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            cwd = Path(td)
            with patch("cccc.daemon.mcp_install._runtime_mcp_state", side_effect=["missing", "ready"]), patch(
                "cccc.daemon.mcp_install.get_cccc_mcp_stdio_command",
                return_value=["/abs/cccc", "mcp"],
            ), patch("cccc.daemon.mcp_install.resolve_subprocess_argv", side_effect=lambda argv: list(argv)):
                with patch("cccc.daemon.mcp_install.subprocess.run") as mock_run:
                    mock_run.return_value.returncode = 0
                    ok = ensure_mcp_installed("devin", cwd, auto_mcp_runtimes=("devin",))
                    self.assertTrue(ok)
                    mock_run.assert_called_once_with(
                        ["devin", "mcp", "add", "-s", "user", "cccc", "--", "/abs/cccc", "mcp"],
                        capture_output=True,
                        text=True,
                        cwd=str(cwd),
                        timeout=30,
                    )

    def test_ensure_mcp_installed_kiro_adds_cccc_stdio(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            cwd = Path(td)
            with patch("cccc.daemon.mcp_install._runtime_mcp_state", side_effect=["missing", "ready"]), patch(
                "cccc.daemon.mcp_install.get_cccc_mcp_stdio_command",
                return_value=["/abs/cccc", "mcp"],
            ), patch("cccc.daemon.mcp_install.resolve_subprocess_argv", side_effect=lambda argv: list(argv)):
                with patch("cccc.daemon.mcp_install.subprocess.run") as mock_run:
                    mock_run.return_value.returncode = 0
                    ok = ensure_mcp_installed("kiro", cwd, auto_mcp_runtimes=("kiro",))
                    self.assertTrue(ok)
                    mock_run.assert_called_once_with(
                        [
                            "kiro-cli",
                            "mcp",
                            "add",
                            "--name",
                            "cccc",
                            "--scope",
                            "global",
                            "--command",
                            "/abs/cccc",
                            "--args=mcp",
                            "--force",
                        ],
                        capture_output=True,
                        text=True,
                        cwd=str(cwd),
                        timeout=30,
                    )

    def test_ensure_mcp_installed_kiro_repairs_stale_global_config(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            cwd = Path(td)
            with patch("cccc.daemon.mcp_install._runtime_mcp_state", side_effect=["stale", "ready"]), patch(
                "cccc.daemon.mcp_install.get_cccc_mcp_stdio_command",
                return_value=["/abs/cccc", "mcp"],
            ), patch("cccc.daemon.mcp_install.resolve_subprocess_argv", side_effect=lambda argv: list(argv)):
                with patch("cccc.daemon.mcp_install.subprocess.run") as mock_run:
                    mock_run.return_value.returncode = 0
                    ok = ensure_mcp_installed("kiro", cwd, auto_mcp_runtimes=("kiro",))
                    self.assertTrue(ok)
                    self.assertEqual(
                        mock_run.call_args_list,
                        [
                            call(
                                ["kiro-cli", "mcp", "remove", "--name", "cccc", "--scope", "global"],
                                capture_output=True,
                                text=True,
                                cwd=str(cwd),
                                timeout=30,
                            ),
                            call(
                                [
                                    "kiro-cli",
                                    "mcp",
                                    "add",
                                    "--name",
                                    "cccc",
                                    "--scope",
                                    "global",
                                    "--command",
                                    "/abs/cccc",
                                    "--args=mcp",
                                    "--force",
                                ],
                                capture_output=True,
                                text=True,
                                cwd=str(cwd),
                                timeout=30,
                            ),
                        ],
                    )

    def test_ensure_mcp_installed_devin_repairs_stale_user_scope_config(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            cwd = Path(td)
            with patch("cccc.daemon.mcp_install._runtime_mcp_state", side_effect=["stale", "ready"]), patch(
                "cccc.daemon.mcp_install.get_cccc_mcp_stdio_command",
                return_value=["/abs/cccc", "mcp"],
            ), patch("cccc.daemon.mcp_install.resolve_subprocess_argv", side_effect=lambda argv: list(argv)):
                with patch("cccc.daemon.mcp_install.subprocess.run") as mock_run:
                    mock_run.return_value.returncode = 0
                    ok = ensure_mcp_installed("devin", cwd, auto_mcp_runtimes=("devin",))
                    self.assertTrue(ok)
                    self.assertEqual(
                        mock_run.call_args_list,
                        [
                            call(
                                ["devin", "mcp", "remove", "-s", "user", "cccc"],
                                capture_output=True,
                                text=True,
                                cwd=str(cwd),
                                timeout=30,
                            ),
                            call(
                                ["devin", "mcp", "add", "-s", "user", "cccc", "--", "/abs/cccc", "mcp"],
                                capture_output=True,
                                text=True,
                                cwd=str(cwd),
                                timeout=30,
                            ),
                        ],
                    )

    def test_is_mcp_installed_grok_reads_json_list_and_validates_env(self) -> None:
        payload = [
            {
                "name": "cccc",
                "command": "/abs/cccc",
                "args": ["mcp"],
                "env": {"PYTHONUNBUFFERED": "1"},
                "enabled": True,
            }
        ]
        with patch("cccc.daemon.mcp_install.get_cccc_mcp_stdio_command", return_value=["/abs/cccc", "mcp"]), patch(
            "cccc.daemon.mcp_install._run_cli",
            return_value=Mock(returncode=0, stdout=json.dumps(payload), stderr=""),
        ) as mock_run:
            self.assertTrue(is_mcp_installed("grok"))
        mock_run.assert_called_once_with(["grok", "mcp", "list", "--json"], timeout=10, env=None)

    def test_is_mcp_installed_grok_rejects_missing_unbuffered_env(self) -> None:
        payload = [
            {
                "name": "cccc",
                "command": "/abs/cccc",
                "args": ["mcp"],
                "env": {},
                "enabled": True,
            }
        ]
        with patch("cccc.daemon.mcp_install.get_cccc_mcp_stdio_command", return_value=["/abs/cccc", "mcp"]), patch(
            "cccc.daemon.mcp_install._run_cli",
            return_value=Mock(returncode=0, stdout=json.dumps(payload), stderr=""),
        ):
            self.assertFalse(is_mcp_installed("grok"))

    def test_is_mcp_installed_kimi_reads_config_and_validates_command(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            home = Path(td)
            config_path = home / ".kimi" / "mcp.json"
            config_path.parent.mkdir(parents=True, exist_ok=True)
            config_path.write_text(
                json.dumps(
                    {
                        "mcpServers": {
                            "cccc": {
                                "command": r"C:\CCCC\cccc.exe",
                                "args": ["mcp"],
                            }
                        }
                    }
                ),
                encoding="utf-8",
            )

            with patch("cccc.daemon.mcp_install.sys.platform", "win32"), patch(
                "cccc.daemon.mcp_install.get_cccc_mcp_stdio_command",
                return_value=[r"C:\CCCC\cccc.exe", "mcp"],
            ), patch("cccc.daemon.mcp_install.Path.home", return_value=home):
                self.assertTrue(is_mcp_installed("kimi"))

    def test_is_mcp_installed_droid_windows_rejects_backslash_stripped_command(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            home = Path(td)
            config_path = home / ".factory" / "mcp.json"
            config_path.parent.mkdir(parents=True, exist_ok=True)
            config_path.write_text(
                json.dumps(
                    {
                        "mcpServers": {
                            "cccc": {
                                "type": "stdio",
                                "command": "C:CCCCcccc.exe",
                                "args": ["mcp"],
                                "disabled": False,
                            }
                        }
                    }
                ),
                encoding="utf-8",
            )

            with patch("cccc.daemon.mcp_install.sys.platform", "win32"), patch(
                "cccc.daemon.mcp_install.get_cccc_mcp_stdio_command",
                return_value=[r"C:\CCCC\cccc.exe", "mcp"],
            ), patch("cccc.daemon.mcp_install.Path.home", return_value=home):
                self.assertFalse(is_mcp_installed("droid"))

    def test_ensure_mcp_installed_kimi_adds_cccc_stdio(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            cwd = Path(td)
            with patch("cccc.daemon.mcp_install._runtime_mcp_state", side_effect=["missing", "ready"]), patch(
                "cccc.daemon.mcp_install.get_cccc_mcp_stdio_command",
                return_value=["/abs/cccc", "mcp"],
            ), patch("cccc.daemon.mcp_install.resolve_subprocess_argv", side_effect=lambda argv: list(argv)):
                with patch("cccc.daemon.mcp_install.subprocess.run") as mock_run:
                    mock_run.return_value.returncode = 0
                    ok = ensure_mcp_installed("kimi", cwd, auto_mcp_runtimes=("kimi",))
                    self.assertTrue(ok)
                    mock_run.assert_called_once_with(
                        ["kimi", "mcp", "add", "--transport", "stdio", "cccc", "--", "/abs/cccc", "mcp"],
                        capture_output=True,
                        text=True,
                        cwd=str(cwd),
                        timeout=30,
                    )

    def test_ensure_mcp_installed_grok_adds_cccc_stdio(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            cwd = Path(td)
            env = {"HOME": str(Path(td) / "home")}
            with patch("cccc.daemon.mcp_install._runtime_mcp_state", side_effect=["missing", "ready"]), patch(
                "cccc.daemon.mcp_install.get_cccc_mcp_stdio_command",
                return_value=["/abs/cccc", "mcp"],
            ), patch("cccc.daemon.mcp_install.resolve_subprocess_argv", side_effect=lambda argv: list(argv)):
                with patch("cccc.daemon.mcp_install.subprocess.run") as mock_run:
                    mock_run.return_value.returncode = 0
                    ok = ensure_mcp_installed("grok", cwd, auto_mcp_runtimes=("grok",), env=env)
                    self.assertTrue(ok)
                    mock_run.assert_called_once()
                    self.assertEqual(
                        mock_run.call_args.args[0],
                        [
                            "grok",
                            "mcp",
                            "add",
                            "cccc",
                            "--command",
                            "/abs/cccc",
                            "--args",
                            "mcp",
                            "--env",
                            "PYTHONUNBUFFERED=1",
                        ],
                    )
                    self.assertEqual(mock_run.call_args.kwargs.get("capture_output"), True)
                    self.assertEqual(mock_run.call_args.kwargs.get("text"), True)
                    self.assertEqual(mock_run.call_args.kwargs.get("cwd"), str(cwd))
                    self.assertEqual(mock_run.call_args.kwargs.get("timeout"), 30)
                    run_env = mock_run.call_args.kwargs.get("env")
                    self.assertIsInstance(run_env, dict)
                    self.assertEqual((run_env or {}).get("HOME"), env["HOME"])

    def test_ensure_mcp_installed_kimi_verifies_against_actor_home_env(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            cwd = Path(td)
            actor_home = Path(td) / "actor-home"
            env = {"HOME": str(actor_home)}

            def fake_run(argv, **kwargs):
                run_home = Path((kwargs.get("env") or {}).get("HOME") or "")
                self.assertEqual(run_home, actor_home)
                config_path = run_home / ".kimi" / "mcp.json"
                config_path.parent.mkdir(parents=True, exist_ok=True)
                config_path.write_text(
                    json.dumps({"mcpServers": {"cccc": {"command": "/abs/cccc", "args": ["mcp"]}}}),
                    encoding="utf-8",
                )
                return Mock(returncode=0, stdout="", stderr="")

            with patch("cccc.daemon.mcp_install.get_cccc_mcp_stdio_command", return_value=["/abs/cccc", "mcp"]), patch(
                "cccc.daemon.mcp_install.resolve_subprocess_argv", side_effect=lambda argv: list(argv)
            ), patch("cccc.daemon.mcp_install.subprocess.run", side_effect=fake_run):
                ok = ensure_mcp_installed("kimi", cwd, auto_mcp_runtimes=("kimi",), env=env)
                self.assertTrue(ok)
                config_path = actor_home / ".kimi" / "mcp.json"
                self.assertTrue(config_path.exists())
                self.assertTrue(is_mcp_installed("kimi", env=env))

    def test_ensure_mcp_installed_claude_windows_repairs_stale_config(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            cwd = Path(td)
            with patch("cccc.daemon.mcp_install.sys.platform", "win32"), patch(
                "cccc.daemon.mcp_install.get_cccc_mcp_stdio_command",
                return_value=[r"C:\CCCC\cccc.exe", "mcp"],
            ), patch(
                "cccc.daemon.mcp_install.resolve_subprocess_argv",
                side_effect=lambda argv: list(argv),
            ):
                with patch("cccc.daemon.mcp_install.subprocess.run") as mock_run:
                    mock_run.side_effect = [
                        Mock(
                            returncode=0,
                            stdout=(
                                "cccc:\n"
                                "  Scope: User config\n"
                                "  Type: stdio\n"
                                "  Command: C:\\Old\\cccc.exe\n"
                                "  Args: mcp\n"
                            ).encode(),
                        ),
                        Mock(returncode=0, stdout="", stderr=""),
                        Mock(returncode=0, stdout="", stderr=""),
                        Mock(
                            returncode=0,
                            stdout=(
                                "cccc:\n"
                                "  Scope: User config\n"
                                "  Type: stdio\n"
                                "  Command: C:\\CCCC\\cccc.exe\n"
                                "  Args: mcp\n"
                            ).encode(),
                        ),
                    ]
                    ok = ensure_mcp_installed("claude", cwd, auto_mcp_runtimes=("claude",))
                    self.assertTrue(ok)
                    self.assertEqual(
                        mock_run.call_args_list,
                        [
                            call(
                                ["claude", "mcp", "get", "cccc"],
                                capture_output=True,
                                text=False,
                                timeout=10,
                            ),
                            call(
                                ["claude", "mcp", "remove", "cccc", "-s", "user"],
                                capture_output=True,
                                text=True,
                                cwd=str(cwd),
                                timeout=30,
                            ),
                            call(
                                ["claude", "mcp", "add", "-s", "user", "cccc", "--", "C:\\CCCC\\cccc.exe", "mcp"],
                                capture_output=True,
                                text=True,
                                cwd=str(cwd),
                                timeout=30,
                            ),
                            call(
                                ["claude", "mcp", "get", "cccc"],
                                capture_output=True,
                                text=False,
                                timeout=10,
                            ),
                        ],
                    )

    def test_ensure_mcp_installed_codex_does_not_persist_actor_context_env(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            cwd = Path(td)
            env = {
                "CODEX_HOME": "/tmp/cccc-isolated-codex-home",
                "OPENAI_API_KEY": "sk-test",
                "CCCC_HOME": "/tmp/cccc-actor-home",
                "CCCC_GROUP_ID": "g_actor",
                "CCCC_ACTOR_ID": "peer1",
            }
            with patch("cccc.daemon.mcp_install._runtime_mcp_state", side_effect=["missing", "ready"]), patch(
                "cccc.daemon.mcp_install.get_cccc_mcp_stdio_command",
                return_value=["/abs/cccc", "mcp"],
            ), patch("cccc.daemon.mcp_install.resolve_subprocess_argv", side_effect=lambda argv: list(argv)):
                with patch("cccc.daemon.mcp_install.subprocess.run") as mock_run:
                    mock_run.return_value.returncode = 0
                    ok = ensure_mcp_installed("codex", cwd, auto_mcp_runtimes=("codex",), env=env)
                    self.assertTrue(ok)
                    mock_run.assert_called_once()
                    args, kwargs = mock_run.call_args
                    self.assertEqual(args[0], ["codex", "mcp", "add", "cccc", "--", "/abs/cccc", "mcp"])
                    self.assertEqual(kwargs["cwd"], str(cwd))
                    run_env = kwargs.get("env")
                    self.assertIsInstance(run_env, dict)
                    self.assertEqual((run_env or {}).get("CODEX_HOME"), env["CODEX_HOME"])
                    self.assertEqual((run_env or {}).get("OPENAI_API_KEY"), env["OPENAI_API_KEY"])
                    self.assertNotIn("CCCC_HOME", run_env or {})
                    self.assertNotIn("CCCC_GROUP_ID", run_env or {})
                    self.assertNotIn("CCCC_ACTOR_ID", run_env or {})

    def test_is_mcp_installed_codex_rejects_persisted_actor_context_env(self) -> None:
        with patch("cccc.daemon.mcp_install.sys.platform", "linux"), patch(
            "cccc.daemon.mcp_install.resolve_subprocess_argv",
            side_effect=lambda argv: list(argv),
        ), patch("cccc.daemon.mcp_install.subprocess.run") as mock_run:
            mock_run.return_value.returncode = 0
            mock_run.return_value.stdout = (
                "cccc\n"
                "  enabled: true\n"
                "  transport: stdio\n"
                "  command: /abs/cccc\n"
                "  args: mcp\n"
                "  env: CCCC_ACTOR_ID=*****, CCCC_GROUP_ID=*****, CCCC_HOME=*****\n"
            )
            mock_run.return_value.stderr = ""

            self.assertFalse(is_mcp_installed("codex"))

    def test_is_mcp_installed_codex_strips_actor_context_env_from_probe(self) -> None:
        with patch.dict(os.environ, {"CCCC_HOME": "/tmp/bad", "CCCC_GROUP_ID": "g_bad", "CCCC_ACTOR_ID": "peer1"}, clear=False), patch(
            "cccc.daemon.mcp_install.sys.platform", "linux"
        ), patch(
            "cccc.daemon.mcp_install.resolve_subprocess_argv",
            side_effect=lambda argv: list(argv),
        ), patch(
            "cccc.daemon.mcp_install.subprocess.run"
        ) as mock_run:
            mock_run.return_value.returncode = 0
            mock_run.return_value.stdout = "cccc\n  enabled: true\n  transport: stdio\n  command: /abs/cccc\n  args: mcp\n"
            mock_run.return_value.stderr = ""

            with patch("cccc.daemon.mcp_install.get_cccc_mcp_stdio_command", return_value=["/abs/cccc", "mcp"]):
                self.assertTrue(is_mcp_installed("codex"))

            run_env = mock_run.call_args.kwargs.get("env")
            self.assertIsInstance(run_env, dict)
            self.assertNotIn("CCCC_HOME", run_env or {})
            self.assertNotIn("CCCC_GROUP_ID", run_env or {})
            self.assertNotIn("CCCC_ACTOR_ID", run_env or {})

    def test_ensure_mcp_installed_hermes_prepares_default_profile(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            cwd = Path(td) / "repo"
            cwd.mkdir()
            cccc_home = Path(td) / "cccc-home"
            env = {"CCCC_HOME": str(cccc_home)}
            calls = []

            def fake_state(runtime, *, env=None):
                calls.append(("state", runtime, dict(env or {})))
                return "missing" if len(calls) == 1 else "ready"

            with patch("cccc.daemon.mcp_install._runtime_mcp_state", side_effect=fake_state), patch(
                "cccc.daemon.mcp_install.prepare_hermes_runtime",
                return_value={"ok": True},
            ) as prepare:
                ok = ensure_mcp_installed("hermes", cwd, auto_mcp_runtimes=("hermes",), env=env)

            self.assertTrue(ok)
            prepare.assert_called_once_with(
                home=cccc_home.resolve(),
                cwd=cwd,
                auto_enable_tools=True,
                force_mcp=False,
                hermes_home_override=None,
            )

    def test_ensure_mcp_installed_hermes_respects_explicit_hermes_home(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            cwd = Path(td) / "repo"
            cwd.mkdir()
            cccc_home = Path(td) / "cccc-home"
            hermes_home = Path(td) / "hermes-home"
            env = {"CCCC_HOME": str(cccc_home), "HERMES_HOME": str(hermes_home)}
            calls = []

            def fake_state(runtime, *, env=None):
                calls.append(("state", runtime, dict(env or {})))
                return "missing" if len(calls) == 1 else "ready"

            with patch("cccc.daemon.mcp_install._runtime_mcp_state", side_effect=fake_state), patch(
                "cccc.daemon.mcp_install.prepare_hermes_runtime",
                return_value={"ok": True},
            ) as prepare:
                ok = ensure_mcp_installed("hermes", cwd, auto_mcp_runtimes=("hermes",), env=env)

            self.assertTrue(ok)
            prepare.assert_called_once_with(
                home=cccc_home.resolve(),
                cwd=cwd,
                auto_enable_tools=True,
                force_mcp=False,
                hermes_home_override=hermes_home,
            )

    def test_ensure_mcp_installed_returns_false_when_initial_probe_times_out(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            cwd = Path(td)

            with patch(
                "cccc.daemon.mcp_install._runtime_mcp_state",
                side_effect=subprocess.TimeoutExpired(cmd=["codex"], timeout=10),
            ):
                self.assertFalse(ensure_mcp_installed("codex", cwd, auto_mcp_runtimes=("codex",)))

    def test_is_mcp_installed_codex_uses_resolved_windows_cli_path(self) -> None:
        with patch("cccc.daemon.mcp_install.sys.platform", "linux"), patch("cccc.daemon.mcp_install.resolve_subprocess_argv", return_value=[r"C:\Tools\codex.cmd", "mcp", "get", "cccc"]), patch(
            "cccc.daemon.mcp_install.subprocess.run"
        ) as mock_run:
            mock_run.return_value.returncode = 0
            mock_run.return_value.stdout = "cccc\n  enabled: true\n  transport: stdio\n"
            mock_run.return_value.stderr = ""

            self.assertTrue(is_mcp_installed("codex"))

        mock_run.assert_called_once()
        args, kwargs = mock_run.call_args
        self.assertEqual(args[0], [r"C:\Tools\codex.cmd", "mcp", "get", "cccc"])
        self.assertEqual(kwargs["timeout"], 10)
        self.assertTrue(kwargs["text"])
        run_env = kwargs.get("env")
        self.assertIsInstance(run_env, dict)
        self.assertNotIn("CCCC_HOME", run_env or {})
        self.assertNotIn("CCCC_GROUP_ID", run_env or {})
        self.assertNotIn("CCCC_ACTOR_ID", run_env or {})

    def test_ensure_mcp_installed_codex_uses_resolved_windows_cli_path(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            cwd = Path(td)
            with patch("cccc.daemon.mcp_install._runtime_mcp_state", side_effect=["missing", "ready"]), patch(
                "cccc.daemon.mcp_install.get_cccc_mcp_stdio_command",
                return_value=["C:\\CCCC\\cccc.exe", "mcp"],
            ), patch(
                "cccc.daemon.mcp_install.resolve_subprocess_argv",
                return_value=[r"C:\Tools\codex.cmd", "mcp", "add", "cccc", "--", "C:\\CCCC\\cccc.exe", "mcp"],
            ), patch("cccc.daemon.mcp_install.subprocess.run") as mock_run:
                mock_run.return_value.returncode = 0

                ok = ensure_mcp_installed("codex", cwd, auto_mcp_runtimes=("codex",))

            self.assertTrue(ok)
            mock_run.assert_called_once()
            args, kwargs = mock_run.call_args
            self.assertEqual(args[0], [r"C:\Tools\codex.cmd", "mcp", "add", "cccc", "--", "C:\\CCCC\\cccc.exe", "mcp"])
            self.assertEqual(kwargs["cwd"], str(cwd))
            self.assertEqual(kwargs["timeout"], 30)
            self.assertTrue(kwargs["text"])
            run_env = kwargs.get("env")
            self.assertIsInstance(run_env, dict)
            self.assertNotIn("CCCC_HOME", run_env or {})
            self.assertNotIn("CCCC_GROUP_ID", run_env or {})
            self.assertNotIn("CCCC_ACTOR_ID", run_env or {})

    def test_ensure_mcp_installed_codex_windows_repairs_stale_config(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            cwd = Path(td)
            with patch("cccc.daemon.mcp_install.sys.platform", "win32"), patch(
                "cccc.daemon.mcp_install.get_cccc_mcp_stdio_command",
                return_value=["C:\\CCCC\\cccc.exe", "mcp"],
            ), patch(
                "cccc.daemon.mcp_install.resolve_subprocess_argv",
                side_effect=lambda argv: list(argv),
            ):
                with patch("cccc.daemon.mcp_install.subprocess.run") as mock_run:
                    mock_run.side_effect = [
                        Mock(
                            returncode=0,
                            stdout=(
                                "cccc\n"
                                "  enabled: true\n"
                                "  transport: stdio\n"
                                "  command: cccc\n"
                                "  args: mcp\n"
                            ),
                        ),
                        Mock(returncode=0, stdout="", stderr=""),
                        Mock(
                            returncode=0,
                            stdout=(
                                "cccc\n"
                                "  enabled: true\n"
                                "  transport: stdio\n"
                                "  command: C:\\CCCC\\cccc.exe\n"
                                "  args: mcp\n"
                            ),
                        ),
                    ]
                    ok = ensure_mcp_installed("codex", cwd, auto_mcp_runtimes=("codex",))
                    self.assertTrue(ok)
                    self.assertEqual(
                        mock_run.call_args_list,
                        [
                            call(
                                ["codex", "mcp", "get", "cccc"],
                                capture_output=True,
                                text=True,
                                timeout=10,
                                env=mock_run.call_args_list[0].kwargs.get("env"),
                            ),
                            call(
                                ["codex", "mcp", "add", "cccc", "--", "C:\\CCCC\\cccc.exe", "mcp"],
                                capture_output=True,
                                text=True,
                                cwd=str(cwd),
                                timeout=30,
                                env=mock_run.call_args_list[1].kwargs.get("env"),
                            ),
                            call(
                                ["codex", "mcp", "get", "cccc"],
                                capture_output=True,
                                text=True,
                                timeout=10,
                                env=mock_run.call_args_list[2].kwargs.get("env"),
                            ),
                        ],
                    )
                    for item in mock_run.call_args_list:
                        run_env = item.kwargs.get("env")
                        self.assertIsInstance(run_env, dict)
                        self.assertNotIn("CCCC_HOME", run_env or {})
                        self.assertNotIn("CCCC_GROUP_ID", run_env or {})
                        self.assertNotIn("CCCC_ACTOR_ID", run_env or {})

    def test_get_cccc_mcp_stdio_command_prefers_sibling_entrypoint(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            bin_dir = Path(td)
            python_exe = bin_dir / "python.exe"
            cccc_exe = bin_dir / "cccc.exe"
            python_exe.write_text("", encoding="utf-8")
            cccc_exe.write_text("", encoding="utf-8")
            with patch("cccc.kernel.runtime.sys.platform", "win32"), patch(
                "cccc.kernel.runtime.sys.executable",
                str(python_exe),
            ), patch("cccc.kernel.runtime.shutil.which", return_value=None):
                self.assertEqual(get_cccc_mcp_stdio_command(), [str(cccc_exe.resolve()), "mcp"])

    def test_get_cccc_mcp_stdio_command_falls_back_to_python_module(self) -> None:
        with patch("cccc.kernel.runtime.sys.platform", "win32"), patch(
            "cccc.kernel.runtime.sys.executable",
            "C:\\Python312\\python.exe",
        ), patch("cccc.kernel.runtime.shutil.which", return_value=None):
            self.assertEqual(
                get_cccc_mcp_stdio_command(),
                ["C:\\Python312\\python.exe", "-m", "cccc.ports.mcp.main"],
            )


if __name__ == "__main__":
    unittest.main()
