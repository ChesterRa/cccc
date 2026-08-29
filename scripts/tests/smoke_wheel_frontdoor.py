#!/usr/bin/env python3
"""Smoke the Rust-only CCCC wheel through pip's real install lifecycle."""

from __future__ import annotations

import argparse
import base64
import csv
import ctypes
import hashlib
import io
import json
import os
import shutil
import subprocess
import sys
import tempfile
import time
import urllib.request
import zipfile
from pathlib import Path


MCP_REQUESTS = """{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}
{"jsonrpc":"2.0","method":"notifications/initialized","params":{}}
{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}
"""


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


def _wait_for_exit(pid: int, *, timeout: float = 15.0) -> None:
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if not _process_is_running(pid):
            return
        time.sleep(0.05)
    raise RuntimeError(f"CCCC process {pid} did not exit")


def _wait_for_child_exit(process: subprocess.Popen[str], *, timeout: float = 15.0) -> None:
    try:
        process.wait(timeout=timeout)
    except subprocess.TimeoutExpired as error:
        raise RuntimeError(f"CCCC process {process.pid} did not exit") from error


def _wait_for_removal(path: Path, *, timeout: float = 10.0) -> None:
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if not path.exists():
            return
        time.sleep(0.05)
    raise RuntimeError(f"CCCC did not remove {path}")


def _wheel_digest(data: bytes) -> str:
    digest = base64.urlsafe_b64encode(hashlib.sha256(data).digest()).rstrip(b"=")
    return f"sha256={digest.decode('ascii')}"


def _legacy_wheel(root: Path) -> Path:
    """Create the smallest old-layout wheel needed to prove upgrade cleanup."""
    stem = "cccc_pair-0.0.0"
    record = f"{stem}.dist-info/RECORD"
    executable_suffix = ".exe" if os.name == "nt" else ""
    entries = {
        "cccc/__init__.py": b"LEGACY_CCCC = True\n",
        f"{stem}.data/scripts/cccc{executable_suffix}": b"legacy cccc",
        f"{stem}.data/scripts/ccccd{executable_suffix}": b"legacy ccccd",
        f"{stem}.dist-info/METADATA": (
            b"Metadata-Version: 2.1\nName: cccc-pair\nVersion: 0.0.0\n\n"
        ),
        f"{stem}.dist-info/WHEEL": (
            b"Wheel-Version: 1.0\nGenerator: cccc-upgrade-fixture\n"
            b"Root-Is-Purelib: true\nTag: py3-none-any\n\n"
        ),
    }
    rows = [[name, _wheel_digest(data), str(len(data))] for name, data in entries.items()]
    rows.append([record, "", ""])
    rendered = io.StringIO(newline="")
    csv.writer(rendered, lineterminator="\n").writerows(rows)
    wheel = root / f"{stem}-py3-none-any.whl"
    with zipfile.ZipFile(wheel, "w", compression=zipfile.ZIP_DEFLATED) as archive:
        for name, data in entries.items():
            archive.writestr(name, data)
        archive.writestr(record, rendered.getvalue().encode())
    return wheel


class InstalledWheelSmoke:
    def __init__(self, root: Path, *, source_binary: Path | None) -> None:
        self.root = root
        self.home = root / "home"
        self.venv = root / "venv"
        self.scripts = self.venv / ("Scripts" if os.name == "nt" else "bin")
        self.python = self.scripts / ("python.exe" if os.name == "nt" else "python")
        self.launcher = self.scripts / ("cccc.exe" if os.name == "nt" else "cccc")
        self.install_marker = self.scripts / ".cccc-standalone"
        self.source_binary = source_binary
        self.web_process: subprocess.Popen[str] | None = None
        self.web_output = None
        self.env = os.environ.copy()
        for key in ("CCCC_LAUNCHER_PATH", "CCCC_RUST_BINARY", "PYTHONPATH", "VIRTUAL_ENV"):
            self.env.pop(key, None)
        self.env["CCCC_HOME"] = str(self.home)
        self.env["PYTHONNOUSERSITE"] = "1"
        self.env["PATH"] = str(self.scripts) + os.pathsep + self.env.get("PATH", "")

    @property
    def pid_path(self) -> Path:
        return self.home / "daemon" / "ccccd.pid"

    @property
    def address_path(self) -> Path:
        return self.home / "daemon" / "ccccd.addr.json"

    @property
    def web_runtime_path(self) -> Path:
        return self.home / "daemon" / "web_runtime.json"

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

    def expect_status(self, daemon: str) -> None:
        output = self.cccc("status").stdout
        expected = [
            f"Daemon:      {daemon}",
        ]
        missing = [line for line in expected if line not in output]
        if missing:
            raise RuntimeError(f"status omitted {missing!r}:\n{output}")
        retired = [line for line in ("Selected:", "Python:", "Rust:") if line in output]
        if retired:
            raise RuntimeError(f"status retained engine rows {retired!r}:\n{output}")

    def expect_mcp(self) -> None:
        output = self.cccc("mcp", input_text=MCP_REQUESTS, timeout=20.0).stdout
        responses: dict[int, dict] = {}
        for line in output.splitlines():
            try:
                payload = json.loads(line)
            except json.JSONDecodeError:
                continue
            if isinstance(payload, dict) and isinstance(payload.get("id"), int):
                responses[payload["id"]] = payload
        server = responses.get(1, {}).get("result", {}).get("serverInfo", {})
        tools = responses.get(2, {}).get("result", {}).get("tools", [])
        if server.get("name") != "cccc-mcp":
            raise RuntimeError(f"MCP initialize did not identify cccc-mcp:\n{output}")
        if not any(tool.get("name") == "cccc_help" for tool in tools if isinstance(tool, dict)):
            raise RuntimeError(f"MCP read-only tools/list omitted cccc_help:\n{output}")

    def start_web_and_expect_health(self) -> subprocess.Popen[str]:
        self.web_output = tempfile.TemporaryFile(mode="w+", encoding="utf-8")
        self.web_process = subprocess.Popen(
            [str(self.launcher), "--port", "0"],
            env=self.env,
            stdin=subprocess.DEVNULL,
            stdout=self.web_output,
            stderr=subprocess.STDOUT,
            text=True,
        )
        deadline = time.monotonic() + 30.0
        opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
        last_error = "Web runtime record was not created"
        while time.monotonic() < deadline:
            if self.web_process.poll() is not None:
                self.web_output.seek(0)
                raise RuntimeError(f"combined CCCC exited before Web readiness:\n{self.web_output.read()}")
            try:
                runtime = json.loads(self.web_runtime_path.read_text(encoding="utf-8"))
                port = int(runtime["port"])
                with opener.open(f"http://127.0.0.1:{port}/api/v1/health", timeout=1.0) as response:
                    payload = json.load(response)
                if response.status == 200 and payload.get("result", {}).get("status") == "ok":
                    return self.web_process
            except (OSError, ValueError, KeyError, json.JSONDecodeError) as error:
                last_error = str(error)
            time.sleep(0.1)
        raise RuntimeError(f"Rust Web health did not become ready: {last_error}")

    def expect_update_refusal(self) -> None:
        result = self.cccc("update", "--check", check=False)
        if result.returncode == 0:
            raise RuntimeError("pip-owned CCCC unexpectedly accepted standalone self-update")
        if 'python -m pip install --upgrade "cccc-pair>=0.4.36"' not in result.stdout:
            raise RuntimeError(f"self-update refusal omitted the pip ownership remedy:\n{result.stdout}")

    def verify_installed_binary(self) -> None:
        if self.source_binary is not None and self.launcher.read_bytes() != self.source_binary.read_bytes():
            raise RuntimeError("pip-installed executable differs from the release binary")
        resolved = shutil.which("cccc", path=self.env["PATH"])
        if resolved is None or Path(resolved).resolve() != self.launcher.resolve():
            raise RuntimeError(f"PATH does not resolve to the wheel-owned CCCC command: {resolved!r}")

    def stop_for_cleanup(self) -> None:
        if self.launcher.exists():
            self.cccc("daemon", "stop", check=False, timeout=15.0)
        if self.web_process is not None and self.web_process.poll() is None:
            self.web_process.terminate()
            try:
                self.web_process.wait(timeout=5.0)
            except subprocess.TimeoutExpired:
                self.web_process.kill()
                self.web_process.wait(timeout=5.0)
        if self.web_output is not None:
            self.web_output.close()

    def run(self) -> None:
        self.verify_installed_binary()
        for command in (("--version",), ("version",)):
            version = self.cccc(*command).stdout.strip()
            if not version.startswith("cccc ") or "(rust)" in version.lower():
                raise RuntimeError(
                    f"unexpected CCCC version output for {command!r}: {version!r}"
                )
        self.expect_status("stopped")
        self.expect_mcp()
        self.expect_update_refusal()

        self.cccc("daemon", "start")
        daemon_pid = self.daemon_pid()
        self.expect_status("running")
        self.cccc("daemon", "stop")
        _wait_for_exit(daemon_pid)
        _wait_for_removal(self.address_path)
        self.expect_status("stopped")

        web_process = self.start_web_and_expect_health()
        self.expect_status("running")
        self.cccc("daemon", "stop")
        _wait_for_child_exit(web_process)
        _wait_for_removal(self.address_path)
        _wait_for_removal(self.web_runtime_path)
        self.expect_status("stopped")


def _pip(python: Path, env: dict[str, str], *args: str) -> None:
    _run(
        [str(python), "-m", "pip", "--disable-pip-version-check", *args],
        env=env,
        timeout=180.0,
    )


def _assert_python_product_absent(python: Path, env: dict[str, str]) -> None:
    result = _run(
        [
            str(python),
            "-c",
            "import importlib.util; print(importlib.util.find_spec('cccc'))",
        ],
        env=env,
    )
    if result.stdout.strip() != "None":
        raise RuntimeError(f"Rust-only wheel left an importable Python product: {result.stdout}")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("wheel", type=Path)
    parser.add_argument("--binary", type=Path)
    args = parser.parse_args()

    wheel = args.wheel.resolve()
    binary = args.binary.resolve() if args.binary is not None else None
    if not wheel.is_file():
        parser.error(f"wheel does not exist: {wheel}")
    if binary is not None and not binary.is_file():
        parser.error(f"release binary does not exist: {binary}")

    # Keep this short: macOS limits Unix-domain socket paths to roughly 104 bytes.
    with tempfile.TemporaryDirectory(prefix="c4-wheel-") as raw_root:
        smoke = InstalledWheelSmoke(Path(raw_root), source_binary=binary)
        try:
            _run([sys.executable, "-m", "venv", str(smoke.venv)], env=smoke.env, timeout=60.0)
            legacy = _legacy_wheel(Path(raw_root))
            _pip(smoke.python, smoke.env, "install", "--quiet", str(legacy))
            if not smoke.launcher.is_file():
                raise RuntimeError("legacy upgrade fixture did not install cccc")
            if not any(smoke.scripts.glob("ccccd*")):
                raise RuntimeError("legacy upgrade fixture did not install ccccd")
            smoke.install_marker.write_text("standalone-v1\n", encoding="utf-8")

            _pip(smoke.python, smoke.env, "install", "--quiet", "--upgrade", str(wheel))
            if any(smoke.scripts.glob("ccccd*")):
                raise RuntimeError("Rust-only wheel upgrade left the retired ccccd command")
            if smoke.install_marker.read_text(encoding="utf-8") != "pip-v1\n":
                raise RuntimeError("pip install did not replace standalone ownership")
            _assert_python_product_absent(smoke.python, smoke.env)
            smoke.run()

            sentinel = smoke.home / "wheel-uninstall-must-preserve-home"
            sentinel.write_text("preserve\n", encoding="utf-8")
            _pip(smoke.python, smoke.env, "install", "--quiet", "--force-reinstall", str(wheel))
            smoke.verify_installed_binary()
            if smoke.install_marker.read_text(encoding="utf-8") != "pip-v1\n":
                raise RuntimeError("pip reinstall lost package-manager ownership")
            _pip(smoke.python, smoke.env, "uninstall", "--yes", "--quiet", "cccc-pair")
            if smoke.launcher.exists():
                raise RuntimeError("pip uninstall left the CCCC command behind")
            if smoke.install_marker.exists():
                raise RuntimeError("pip uninstall left its ownership marker behind")
            if not sentinel.is_file():
                raise RuntimeError("pip uninstall removed user-owned CCCC_HOME state")
            _assert_python_product_absent(smoke.python, smoke.env)
        finally:
            smoke.stop_for_cleanup()

    print("OK: Rust-only wheel passed upgrade cleanup, CLI, MCP, daemon, Web, and uninstall smoke")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
