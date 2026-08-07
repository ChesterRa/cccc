from __future__ import annotations

import io
import json
import os
import tempfile
import unittest
from contextlib import redirect_stderr, redirect_stdout
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch


class TestImplementationState(unittest.TestCase):
    def test_missing_state_defaults_to_python_and_save_is_explicit(self) -> None:
        from cccc.implementation import (
            implementation_state_path,
            load_selected_implementation,
            save_selected_implementation,
        )

        with tempfile.TemporaryDirectory() as td:
            home = Path(td)
            self.assertEqual(load_selected_implementation(home), "python")
            self.assertFalse(implementation_state_path(home).exists())

            self.assertEqual(save_selected_implementation("rust", home), "rust")
            self.assertEqual(load_selected_implementation(home), "rust")
            self.assertEqual(
                json.loads(implementation_state_path(home).read_text(encoding="utf-8")),
                {"schema": 1, "selected": "rust"},
            )

    def test_invalid_state_never_silently_falls_back(self) -> None:
        from cccc.implementation import ImplementationError, implementation_state_path, load_selected_implementation

        with tempfile.TemporaryDirectory() as td:
            home = Path(td)
            path = implementation_state_path(home)
            path.write_text("not-json", encoding="utf-8")
            with self.assertRaisesRegex(ImplementationError, "cccc python"):
                load_selected_implementation(home)

    def test_legacy_daemon_without_implementation_is_python(self) -> None:
        from cccc.implementation import daemon_implementation

        self.assertEqual(daemon_implementation({"ok": True, "result": {"version": "0.4.32"}}), "python")
        self.assertEqual(
            daemon_implementation({"ok": True, "result": {"implementation": "rust"}}),
            "rust",
        )
        self.assertEqual(
            daemon_implementation({"ok": True, "result": {"implementation": "other"}}),
            "unknown",
        )
        self.assertIsNone(daemon_implementation({"ok": False}))

    def test_rust_probe_requires_exact_product_release_identity(self) -> None:
        import cccc.implementation as implementation

        with tempfile.TemporaryDirectory() as td:
            binary = Path(td) / ("cccc-rust.exe" if os.name == "nt" else "cccc-rust")
            binary.write_bytes(b"native")
            if os.name != "nt":
                binary.chmod(0o755)
            expected = implementation._canonical_python_version(implementation.__version__)
            assert expected is not None
            completed = SimpleNamespace(returncode=0, stdout=f"cccc {expected}\n", stderr="")
            with patch.dict(os.environ, {"CCCC_RUST_BINARY": str(binary)}), patch.object(
                implementation.subprocess, "run", return_value=completed
            ):
                probe = implementation.probe_rust_implementation()
            self.assertTrue(probe["available"])
            self.assertEqual(probe["version"], expected)

            mismatch = SimpleNamespace(returncode=0, stdout="cccc 99.0.0\n", stderr="")
            with patch.dict(os.environ, {"CCCC_RUST_BINARY": str(binary)}), patch.object(
                implementation.subprocess, "run", return_value=mismatch
            ):
                probe = implementation.probe_rust_implementation()
            self.assertFalse(probe["available"])
            self.assertIn("does not match", str(probe["error"]))

    def test_prerelease_versions_normalize_across_pep440_and_semver(self) -> None:
        from cccc.implementation import _canonical_python_version, _rust_version_from_output

        self.assertEqual(_canonical_python_version("0.4.34rc1"), "0.4.34-rc1")
        self.assertEqual(_canonical_python_version("0.4.34b2"), "0.4.34-beta2")
        self.assertEqual(_rust_version_from_output("cccc 0.4.34-rc1"), "0.4.34-rc1")


class TestLauncher(unittest.TestCase):
    def _home_env(self, home: Path):
        return patch.dict(os.environ, {"CCCC_HOME": str(home)}, clear=False)

    def test_selector_persists_before_dispatch(self) -> None:
        import cccc.launcher as launcher

        with patch.object(launcher, "_switch", return_value="python") as switch, patch.object(
            launcher, "_dispatch", return_value=7
        ) as dispatch:
            self.assertEqual(launcher.main(["rust", "doctor"]), 7)
        switch.assert_called_once_with("rust")
        dispatch.assert_called_once_with("rust", ["doctor"])

    def test_bare_launch_follows_persisted_implementation(self) -> None:
        import cccc.launcher as launcher

        with patch.object(launcher, "load_selected_implementation", return_value="rust"), patch.object(
            launcher, "_dispatch", return_value=0
        ) as dispatch:
            self.assertEqual(launcher.main([]), 0)
        dispatch.assert_called_once_with("rust", [])

    def test_version_is_product_stable_and_does_not_load_an_engine(self) -> None:
        import cccc.launcher as launcher

        output = io.StringIO()
        with patch.object(launcher, "_dispatch") as dispatch, redirect_stdout(output):
            self.assertEqual(launcher.main(["version"]), 0)
        dispatch.assert_not_called()
        self.assertEqual(output.getvalue().strip(), launcher.__version__)

    def test_unavailable_rust_fails_without_python_fallback(self) -> None:
        import cccc.launcher as launcher
        from cccc.implementation import ImplementationError

        error = io.StringIO()
        with patch.object(launcher, "load_selected_implementation", return_value="rust"), patch.object(
            launcher, "require_rust_implementation", side_effect=ImplementationError("missing native payload")
        ), patch.object(launcher, "_python_main") as python_main, redirect_stderr(error):
            self.assertEqual(launcher.main(["doctor"]), 1)
        python_main.assert_not_called()
        self.assertIn("missing native payload", error.getvalue())

    def test_switch_stops_processes_only_when_implementation_changes(self) -> None:
        import cccc.launcher as launcher
        from cccc.implementation import load_selected_implementation, save_selected_implementation

        with tempfile.TemporaryDirectory() as td:
            home = Path(td)
            save_selected_implementation("python", home)
            with self._home_env(home), patch.object(
                launcher, "_ping_daemon", return_value={"ok": True, "result": {"implementation": "python"}}
            ), patch.object(launcher, "_stop_active_processes") as stop:
                launcher._switch("python")
            stop.assert_not_called()
            self.assertEqual(load_selected_implementation(home), "python")

            with self._home_env(home), patch.object(launcher, "require_rust_implementation"), patch.object(
                launcher, "_ping_daemon", return_value={"ok": True, "result": {"implementation": "python"}}
            ), patch.object(launcher, "_stop_active_processes") as stop:
                launcher._switch("rust")
            stop.assert_called_once_with(home.resolve())
            self.assertEqual(load_selected_implementation(home), "rust")

    def test_failed_rust_preflight_preserves_selection_and_processes(self) -> None:
        import cccc.launcher as launcher
        from cccc.implementation import ImplementationError, load_selected_implementation, save_selected_implementation

        with tempfile.TemporaryDirectory() as td:
            home = Path(td)
            save_selected_implementation("python", home)
            with self._home_env(home), patch.object(
                launcher, "require_rust_implementation", side_effect=ImplementationError("version mismatch")
            ), patch.object(launcher, "_stop_active_processes") as stop:
                with self.assertRaisesRegex(ImplementationError, "version mismatch"):
                    launcher._switch("rust")
            stop.assert_not_called()
            self.assertEqual(load_selected_implementation(home), "python")

    def test_explicit_python_selector_repairs_corrupt_selection_state(self) -> None:
        import cccc.launcher as launcher
        from cccc.implementation import implementation_state_path, load_selected_implementation

        with tempfile.TemporaryDirectory() as td:
            home = Path(td)
            implementation_state_path(home).write_text("not-json", encoding="utf-8")
            with self._home_env(home), patch.object(
                launcher, "_ping_daemon", return_value={"ok": False}
            ), patch.object(launcher, "_stop_active_processes") as stop:
                self.assertIsNone(launcher._switch("python"))
            stop.assert_called_once_with(home.resolve())
            self.assertEqual(load_selected_implementation(home), "python")

    def test_running_daemon_drift_is_reconciled_even_when_selection_matches(self) -> None:
        import cccc.launcher as launcher
        from cccc.implementation import save_selected_implementation

        with tempfile.TemporaryDirectory() as td:
            home = Path(td)
            save_selected_implementation("python", home)
            with self._home_env(home), patch.object(
                launcher, "_ping_daemon", return_value={"ok": True, "result": {"implementation": "rust"}}
            ), patch.object(launcher, "_stop_active_processes") as stop:
                launcher._switch("python")
            stop.assert_called_once_with(home.resolve())

    def test_update_is_owned_by_python_launcher(self) -> None:
        import cccc.launcher as launcher

        with patch.object(launcher, "_python_main", return_value=0) as python_main:
            self.assertEqual(launcher.main(["update"]), 0)
        python_main.assert_called_once_with(["update"])

    def test_meta_command_after_global_options_remains_python_owned(self) -> None:
        import cccc.launcher as launcher

        argv = ["--port", "9000", "update", "--check"]
        with patch.object(launcher, "_switch", return_value="python") as switch, patch.object(
            launcher, "_python_main", return_value=0
        ) as python_main, patch.object(launcher, "_dispatch") as dispatch:
            self.assertEqual(launcher.main(["rust", *argv]), 0)
        switch.assert_called_once_with("rust")
        python_main.assert_called_once_with(argv)
        dispatch.assert_not_called()

    def test_meta_command_routing_supports_global_option_aliases_and_equals(self) -> None:
        import cccc.launcher as launcher

        cases = [
            ["--web-port=9000", "status"],
            ["--web-host", "127.0.0.1", "version"],
        ]
        for argv in cases:
            with self.subTest(argv=argv), patch.object(
                launcher, "_python_main", return_value=0
            ) as python_main, patch.object(launcher, "_dispatch") as dispatch:
                self.assertEqual(launcher.main(argv), 0)
            python_main.assert_called_once_with(argv)
            dispatch.assert_not_called()

    def test_global_option_value_named_like_meta_command_is_not_misrouted(self) -> None:
        import cccc.launcher as launcher

        argv = ["--host", "status", "doctor"]
        with patch.object(launcher, "load_selected_implementation", return_value="rust"), patch.object(
            launcher, "_dispatch", return_value=0
        ) as dispatch, patch.object(launcher, "_python_main") as python_main:
            self.assertEqual(launcher.main(argv), 0)
        dispatch.assert_called_once_with("rust", argv)
        python_main.assert_not_called()

    def test_python_dispatch_supplies_the_product_update_lifecycle_hook(self) -> None:
        import cccc.launcher as launcher

        with tempfile.TemporaryDirectory() as td, self._home_env(Path(td)), patch(
            "cccc.cli.main.main", return_value=0
        ) as python_main, patch.object(launcher, "_stop_active_processes") as stop:
            self.assertEqual(launcher._python_main(["update"]), 0)
            hook = python_main.call_args.kwargs["before_product_update"]
            hook()
        stop.assert_called_once_with(Path(td).resolve())

    def test_update_preparation_runs_after_validation_and_before_pip(self) -> None:
        from cccc.cli import system_cmds

        calls: list[str] = []
        inspection = {"install_kind": "standard", "version": "0.4.33"}
        completed = SimpleNamespace(returncode=0, stdout="", stderr="")
        args = SimpleNamespace(
            check=False,
            _before_product_update=lambda: calls.append("prepare"),
        )
        with patch.object(
            system_cmds, "_inspect_update_target", return_value=(inspection, ["pip", "install"])
        ), patch.object(
            system_cmds.subprocess,
            "run",
            side_effect=lambda *_args, **_kwargs: calls.append("pip") or completed,
        ), patch.object(
            system_cmds,
            "_find_installed_distribution",
            return_value=SimpleNamespace(version="0.4.33"),
        ), redirect_stdout(io.StringIO()):
            self.assertEqual(system_cmds.cmd_update(args), 0)
        self.assertEqual(calls, ["prepare", "pip"])

    def test_update_check_does_not_prepare_or_run_pip(self) -> None:
        from cccc.cli import system_cmds

        prepare = patch.object(system_cmds.subprocess, "run")
        stop_calls: list[str] = []
        args = SimpleNamespace(
            check=True,
            _before_product_update=lambda: stop_calls.append("prepare"),
        )
        with patch.object(
            system_cmds,
            "_inspect_update_target",
            return_value=({"install_kind": "standard", "version": "0.4.33"}, ["pip", "install"]),
        ), prepare as run, redirect_stdout(io.StringIO()):
            self.assertEqual(system_cmds.cmd_update(args), 0)
        run.assert_not_called()
        self.assertEqual(stop_calls, [])

    def test_unlocked_rust_web_pid_is_treated_as_stale(self) -> None:
        import cccc.launcher as launcher

        with tempfile.TemporaryDirectory() as td:
            home = Path(td)
            lock = home / "daemon" / "cccc-web.lock"
            lock.parent.mkdir(parents=True)
            lock.write_text("424242\n", encoding="utf-8")
            self.assertEqual(launcher._rust_web_launcher_pid(home), 0)

    def test_rust_dispatch_exports_stable_public_launcher_to_setup(self) -> None:
        import cccc.launcher as launcher

        public = Path("/tmp/cccc-public")
        native = Path("/tmp/cccc-rust")
        with patch.object(launcher, "_public_launcher_path", return_value=public), patch.object(
            launcher.os, "execve", side_effect=OSError("test stop")
        ) as execve:
            with self.assertRaisesRegex(OSError, "test stop"):
                launcher._dispatch_rust(native, ["setup"])
        executable, command, env = execve.call_args.args
        self.assertEqual(executable, str(native))
        self.assertEqual(command, [str(native), "setup"])
        self.assertEqual(env["CCCC_LAUNCHER_PATH"], str(public))

    def test_immediate_launch_failure_restores_previous_selection(self) -> None:
        import cccc.launcher as launcher

        with patch.object(launcher, "_dispatch", side_effect=OSError("cannot exec")), patch.object(
            launcher, "save_selected_implementation"
        ) as save:
            with self.assertRaisesRegex(OSError, "cannot exec"):
                launcher._dispatch_after_switch("rust", ["doctor"], previous="python")
        save.assert_called_once_with("python")

    def test_module_execution_does_not_leak_private_python_path_into_mcp_config(self) -> None:
        import cccc.launcher as launcher

        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            module = root / "launcher.py"
            command = root / ("cccc.exe" if os.name == "nt" else "cccc")
            module.write_text("", encoding="utf-8")
            command.write_text("", encoding="utf-8")
            with patch.object(launcher.sys, "argv", [str(module)]), patch.object(
                launcher.shutil, "which", return_value=str(command)
            ):
                self.assertEqual(launcher._public_launcher_path(), command.resolve())

    def test_legacy_daemon_entry_resolves_its_sibling_public_launcher(self) -> None:
        import cccc.launcher as launcher

        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            daemon = root / ("ccccd.exe" if os.name == "nt" else "ccccd")
            command = root / ("cccc.exe" if os.name == "nt" else "cccc")
            daemon.write_text("", encoding="utf-8")
            command.write_text("", encoding="utf-8")
            with patch.object(launcher.sys, "argv", [str(daemon)]), patch.object(
                launcher.shutil, "which", return_value=None
            ):
                self.assertEqual(launcher._public_launcher_path(), command.resolve())

    def test_status_distinguishes_product_implementations_from_agent_runtimes(self) -> None:
        from cccc.cli import system_cmds
        from cccc.implementation import save_selected_implementation

        with tempfile.TemporaryDirectory() as td:
            home = Path(td)
            save_selected_implementation("rust", home)
            ping = {"ok": True, "result": {"implementation": "rust"}}

            def daemon(request):
                return ping if request.get("op") == "ping" else {"ok": True, "result": {"groups": []}}

            output = io.StringIO()
            with self._home_env(home), patch.object(system_cmds, "call_daemon", side_effect=daemon), patch(
                "cccc.kernel.runtime.detect_all_runtimes", return_value=[]
            ), patch(
                "cccc.implementation.probe_rust_implementation",
                return_value={"available": True, "version": "0.4.33", "path": "/native", "error": None},
            ), redirect_stdout(output):
                self.assertEqual(system_cmds.cmd_status(SimpleNamespace()), 0)
        text = output.getvalue()
        self.assertIn("Selected:    rust", text)
        self.assertIn("Daemon:      running (rust)", text)
        self.assertIn("Rust:        available (0.4.33)", text)
        self.assertIn("Runtimes:    (none detected)", text)

    def test_legacy_daemon_command_follows_the_selected_launcher(self) -> None:
        import cccc.daemon_launcher as daemon_launcher

        with patch.object(daemon_launcher, "launcher_main", return_value=7) as launch:
            self.assertEqual(daemon_launcher.main(["start"]), 7)
        launch.assert_called_once_with(["daemon", "start"])

    def test_legacy_daemon_stop_preserves_python_supervised_lifecycle(self) -> None:
        import cccc.daemon_launcher as daemon_launcher
        import cccc.launcher as launcher

        with patch.object(launcher, "load_selected_implementation", return_value="python"), patch(
            "cccc.daemon_main.main", return_value=0
        ) as daemon_main:
            self.assertEqual(daemon_launcher.main(["stop"]), 0)
        daemon_main.assert_called_once_with(["stop"])

    def test_python_daemon_command_accepts_foreground_run(self) -> None:
        from cccc.cli.main import build_parser

        args = build_parser().parse_args(["daemon", "run"])
        self.assertEqual(args.action, "run")


if __name__ == "__main__":
    unittest.main()
