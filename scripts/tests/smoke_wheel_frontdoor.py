#!/usr/bin/env python3
from __future__ import annotations

import argparse
import ctypes
import json
import os
import subprocess
import sys
import tempfile
import time
from pathlib import Path


INITIALIZE_REQUEST = '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}\n'


def _run(
    command: list[str],
    *,
    env: dict[str, str],
    input_text: str | None = None,
    check: bool = True,
    timeout: float = 30.0,
) -> subprocess.CompletedProcess[str]:
    # A Windows daemon grandchild can inherit a PIPE handle after its launcher
    # exits, leaving subprocess.run() waiting forever for EOF. A regular file
    # preserves diagnostics without coupling completion to the process tree.
    with tempfile.TemporaryFile(mode="w+", encoding="utf-8") as output:
        completed = subprocess.run(
            command,
            env=env,
            input=input_text,
            stdout=output,
            stderr=subprocess.STDOUT,
            text=True,
            timeout=timeout,
            check=False,
        )
        output.seek(0)
        stdout = output.read()
    completed = subprocess.CompletedProcess(completed.args, completed.returncode, stdout, None)
    if check and completed.returncode != 0:
        rendered = " ".join(command)
        raise RuntimeError(f"command failed ({completed.returncode}): {rendered}\n{completed.stdout}")
    return completed


def _process_is_running(pid: int) -> bool:
    if os.name != "nt":
        try:
            os.kill(pid, 0)
        except ProcessLookupError:
            return False
        except PermissionError:
            return True
        if sys.platform.startswith("linux"):
            try:
                stat = Path(f"/proc/{pid}/stat").read_text(encoding="utf-8")
            except OSError:
                pass
            else:
                closing_parenthesis = stat.rfind(")")
                if closing_parenthesis >= 0:
                    fields = stat[closing_parenthesis + 1 :].split()
                    if fields and fields[0] == "Z":
                        return False
        return True

    synchronize = 0x00100000
    wait_timeout = 0x00000102
    kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
    handle = kernel32.OpenProcess(synchronize, False, pid)
    if not handle:
        return False
    try:
        return kernel32.WaitForSingleObject(handle, 0) == wait_timeout
    finally:
        kernel32.CloseHandle(handle)


def _wait_for_exit(pid: int, *, timeout: float = 10.0) -> None:
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if not _process_is_running(pid):
            return
        time.sleep(0.05)
    raise RuntimeError(f"daemon process {pid} did not exit")


def _wait_for_removal(path: Path, *, timeout: float = 10.0) -> None:
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if not path.exists():
            return
        time.sleep(0.05)
    raise RuntimeError(f"daemon did not remove {path}")


class InstalledWheelSmoke:
    def __init__(self, root: Path, *, expect_rust: bool) -> None:
        self.root = root
        self.home = root / "home"
        self.venv = root / "venv"
        scripts = self.venv / ("Scripts" if os.name == "nt" else "bin")
        self.python = scripts / ("python.exe" if os.name == "nt" else "python")
        self.launcher = scripts / ("cccc.exe" if os.name == "nt" else "cccc")
        self.expect_rust = expect_rust
        self.env = os.environ.copy()
        for key in ("CCCC_LAUNCHER_PATH", "CCCC_RUST_BINARY", "PYTHONPATH", "VIRTUAL_ENV"):
            self.env.pop(key, None)
        self.env["CCCC_HOME"] = str(self.home)
        self.env["PYTHONNOUSERSITE"] = "1"

    @property
    def pid_path(self) -> Path:
        return self.home / "daemon" / "ccccd.pid"

    @property
    def address_path(self) -> Path:
        return self.home / "daemon" / "ccccd.addr.json"

    def cccc(
        self,
        *args: str,
        input_text: str | None = None,
        check: bool = True,
        timeout: float = 30.0,
    ) -> subprocess.CompletedProcess[str]:
        return _run(
            [str(self.launcher), *args],
            env=self.env,
            input_text=input_text,
            check=check,
            timeout=timeout,
        )

    def daemon_pid(self) -> int:
        raw = self.pid_path.read_text(encoding="utf-8").strip()
        if not raw.isdigit() or int(raw) <= 0:
            raise RuntimeError(f"invalid daemon pid in {self.pid_path}: {raw!r}")
        return int(raw)

    def expect_status(self, *, selected: str, daemon: str) -> None:
        output = self.cccc("status").stdout
        expected = [
            f"Selected:    {selected}",
            f"Daemon:      {daemon}",
            "Python:      available",
            f"Rust:        {'available' if self.expect_rust else 'unavailable'}",
        ]
        missing = [line for line in expected if line not in output]
        if missing:
            raise RuntimeError(f"status omitted {missing!r}:\n{output}")

    def expect_mcp_initialize(self, implementation: str) -> None:
        output = self.cccc(
            implementation,
            "mcp",
            input_text=INITIALIZE_REQUEST,
            timeout=20.0,
        ).stdout
        for line in output.splitlines():
            try:
                payload = json.loads(line)
            except json.JSONDecodeError:
                continue
            if payload.get("id") != 1:
                continue
            result = payload.get("result")
            if isinstance(result, dict):
                server = result.get("serverInfo")
                if isinstance(server, dict) and server.get("name") == "cccc-mcp":
                    return
        raise RuntimeError(f"{implementation} MCP initialize did not return cccc-mcp server info:\n{output}")

    def stop_for_cleanup(self) -> None:
        if self.launcher.exists():
            self.cccc("daemon", "stop", check=False, timeout=10.0)

    def run(self) -> None:
        self.expect_status(selected="python", daemon="stopped")
        self.expect_mcp_initialize("python")

        self.cccc("python", "daemon", "start")
        python_pid = self.daemon_pid()
        self.expect_status(selected="python", daemon="running (python)")

        if self.expect_rust:
            self.cccc("rust", "daemon", "start")
            rust_pid = self.daemon_pid()
            _wait_for_exit(python_pid)
            self.expect_status(selected="rust", daemon="running (rust)")
            self.expect_mcp_initialize("rust")

            self.cccc("python", "daemon", "start")
            python_pid = self.daemon_pid()
            _wait_for_exit(rust_pid)
            self.expect_status(selected="python", daemon="running (python)")

        self.cccc("daemon", "stop")
        _wait_for_exit(python_pid)
        _wait_for_removal(self.address_path)
        self.expect_status(selected="python", daemon="stopped")

        if self.expect_rust:
            self.cccc("rust", "daemon", "start")
            rust_pid = self.daemon_pid()
            self.expect_status(selected="rust", daemon="running (rust)")
            self.cccc("daemon", "stop")
            _wait_for_exit(rust_pid)
            _wait_for_removal(self.address_path)
            self.expect_status(selected="rust", daemon="stopped")


def main() -> int:
    parser = argparse.ArgumentParser(description="Smoke an installed CCCC wheel through its public launcher")
    parser.add_argument("wheel", type=Path)
    parser.add_argument("--expect-rust", choices=("available", "unavailable"), required=True)
    args = parser.parse_args()

    wheel = args.wheel.resolve()
    if not wheel.is_file():
        parser.error(f"wheel does not exist: {wheel}")

    # Keep this short: macOS limits Unix-domain socket paths to roughly 104 bytes,
    # and the daemon socket is created below this temporary root.
    with tempfile.TemporaryDirectory(prefix="c4-wheel-") as raw_root:
        smoke = InstalledWheelSmoke(
            Path(raw_root),
            expect_rust=args.expect_rust == "available",
        )
        try:
            _run([sys.executable, "-m", "venv", str(smoke.venv)], env=smoke.env, timeout=60.0)
            _run(
                [
                    str(smoke.python),
                    "-m",
                    "pip",
                    "install",
                    "--quiet",
                    "--disable-pip-version-check",
                    "--force-reinstall",
                    str(wheel),
                ],
                env=smoke.env,
                timeout=180.0,
            )
            smoke.run()
        finally:
            smoke.stop_for_cleanup()

    flavor = "dual-engine" if args.expect_rust == "available" else "portable Python"
    print(f"OK: installed {flavor} wheel passed launcher, MCP, and daemon lifecycle smoke")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
