"""Local cloudflared process supervision. No auto-restart: a dead tunnel is not 'online'."""

from __future__ import annotations

import os
import json
import signal
import shutil
import subprocess
import sys
import time
from pathlib import Path
from typing import Any, Dict, List, Optional, Sequence

from ...kernel.cloudflared import binary_path, inspect, install_dir
from ...util.fs import atomic_write_json


_owned_processes: Dict[int, Any] = {}


def pid_path(home: Optional[Path] = None) -> Path:
    return install_dir(home) / "cloudflared.pid"


def token_path(home: Optional[Path] = None) -> Path:
    return install_dir(home) / "cloudflared.token"


def _pid_is_alive(pid: int) -> bool:
    if pid <= 0:
        return False
    if os.name == "posix" and Path(f"/proc/{pid}/stat").is_file():
        try:
            tail = (
                Path(f"/proc/{pid}/stat").read_text(encoding="utf-8").rsplit(") ", 1)[1]
            )
            if tail.split()[0] == "Z":
                return False
        except (OSError, IndexError):
            pass
    try:
        os.kill(pid, 0)
    except OSError:
        return False
    return True


def _process_executable(pid: int) -> Optional[Path]:
    if sys.platform.startswith("linux"):
        try:
            return Path(f"/proc/{pid}/exe").resolve(strict=True)
        except OSError:
            return None
    if sys.platform == "darwin":
        try:
            import ctypes

            libproc = ctypes.CDLL("/usr/lib/libproc.dylib")
            buffer = ctypes.create_string_buffer(4096)
            if libproc.proc_pidpath(int(pid), buffer, len(buffer)) <= 0:
                return None
            return Path(os.fsdecode(buffer.value)).resolve(strict=True)
        except (AttributeError, OSError, ValueError):
            return None
    if os.name == "nt":
        try:
            import ctypes
            from ctypes import wintypes

            handle = ctypes.windll.kernel32.OpenProcess(0x1000, False, int(pid))
            if not handle:
                return None
            try:
                size = wintypes.DWORD(32768)
                buffer = ctypes.create_unicode_buffer(size.value)
                if not ctypes.windll.kernel32.QueryFullProcessImageNameW(
                    handle, 0, buffer, ctypes.byref(size)
                ):
                    return None
                return Path(buffer.value).resolve(strict=True)
            finally:
                ctypes.windll.kernel32.CloseHandle(handle)
        except (AttributeError, OSError, ValueError):
            return None
    try:
        output = subprocess.run(
            ["ps", "-p", str(pid), "-o", "comm="],
            check=False,
            capture_output=True,
            text=True,
        )
    except OSError:
        return None
    if output.returncode != 0 or not output.stdout.strip():
        return None
    try:
        return Path(output.stdout.strip()).resolve(strict=True)
    except OSError:
        return None


def _resolve_executable(value: str) -> Path:
    candidate = Path(value)
    if not candidate.is_absolute():
        located = shutil.which(value)
        candidate = Path(located) if located else candidate
    return candidate.resolve(strict=True)


def _pid_matches_executable(pid: int, expected: Path) -> bool:
    actual = _process_executable(pid)
    if actual is None:
        return False
    try:
        return os.path.samefile(actual, expected)
    except OSError:
        return os.path.normcase(os.path.realpath(actual)) == os.path.normcase(
            os.path.realpath(expected)
        )


def _tracked_process(home: Optional[Path] = None) -> Optional[tuple[int, Path]]:
    path = pid_path(home)
    if not path.is_file():
        return None
    try:
        raw = path.read_text(encoding="utf-8").strip()
        if raw.startswith("{"):
            marker = json.loads(raw)
            pid = int(marker.get("pid") or 0)
            executable = Path(str(marker.get("executable") or ""))
            if not executable.is_absolute():
                raise ValueError("tracked executable path must be absolute")
            executable = Path(os.path.realpath(executable))
        else:
            pid = int(raw)
            executable = binary_path(home).resolve(strict=False)
    except (OSError, ValueError, TypeError, json.JSONDecodeError) as exc:
        raise RuntimeError("cloudflared PID marker is malformed") from exc
    if pid <= 0:
        raise RuntimeError("cloudflared PID marker is invalid")
    return pid, executable


def _tracked_pid(home: Optional[Path] = None) -> Optional[int]:
    tracked = _tracked_process(home)
    return tracked[0] if tracked is not None else None


def running_pid(home: Optional[Path] = None) -> Optional[int]:
    try:
        tracked = _tracked_process(home)
    except RuntimeError:
        return None
    if tracked is None:
        return None
    pid, executable = tracked
    owned = _owned_processes.get(pid)
    if owned is not None and owned.poll() is not None:
        _owned_processes.pop(pid, None)
        _unlink_if_exists(pid_path(home))
        _unlink_if_exists(token_path(home))
        return None
    if not _pid_is_alive(pid):
        _owned_processes.pop(pid, None)
        _unlink_if_exists(pid_path(home))
        _unlink_if_exists(token_path(home))
        return None
    if owned is None and not _pid_matches_executable(pid, executable):
        return None
    return pid


def _unlink_if_exists(path: Path) -> None:
    try:
        path.unlink()
    except FileNotFoundError:
        pass


def _wait_for_exit(pid: int, timeout_s: float) -> bool:
    deadline = time.monotonic() + max(0.0, timeout_s)
    while time.monotonic() < deadline:
        try:
            waited, _status = os.waitpid(pid, os.WNOHANG)
            if waited == pid:
                return True
        except (ChildProcessError, OSError):
            pass
        if not _pid_is_alive(pid):
            return True
        time.sleep(0.05)
    return not _pid_is_alive(pid)


def stop(home: Optional[Path] = None) -> None:
    tracked = _tracked_process(home)
    if tracked is None:
        _unlink_if_exists(token_path(home))
        return
    pid, executable = tracked
    owned = _owned_processes.get(pid)
    if owned is not None and owned.poll() is not None:
        _owned_processes.pop(pid, None)
        _unlink_if_exists(pid_path(home))
        _unlink_if_exists(token_path(home))
        return
    if not _pid_is_alive(pid):
        _owned_processes.pop(pid, None)
        _unlink_if_exists(pid_path(home))
        _unlink_if_exists(token_path(home))
        return
    if owned is None and not _pid_matches_executable(pid, executable):
        raise RuntimeError(
            f"tracked PID {pid} is not the tracked cloudflared executable; refusing to terminate it"
        )
    try:
        os.kill(pid, signal.SIGTERM)
    except ProcessLookupError:
        _owned_processes.pop(pid, None)
        _unlink_if_exists(pid_path(home))
        _unlink_if_exists(token_path(home))
        return
    except OSError as exc:
        raise RuntimeError(f"failed to stop cloudflared process {pid}: {exc}") from exc
    if not _wait_for_exit(pid, 5.0):
        try:
            os.kill(pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        except OSError as exc:
            raise RuntimeError(
                f"failed to terminate cloudflared process {pid}: {exc}"
            ) from exc
        if not _wait_for_exit(pid, 2.0):
            raise RuntimeError(f"cloudflared process {pid} did not exit")
    _owned_processes.pop(pid, None)
    _unlink_if_exists(pid_path(home))
    _unlink_if_exists(token_path(home))


def _write_token_file(token: str, home: Optional[Path]) -> Path:
    path = token_path(home)
    path.parent.mkdir(parents=True, exist_ok=True)
    fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
    try:
        with os.fdopen(fd, "w", encoding="utf-8") as handle:
            handle.write(token)
            handle.write("\n")
    except BaseException:
        _unlink_if_exists(path)
        raise
    try:
        path.chmod(0o600)
    except OSError:
        pass
    return path


def start(
    tunnel_token: str,
    *,
    home: Optional[Path] = None,
    command: Optional[Sequence[str]] = None,
) -> Dict[str, Any]:
    token = str(tunnel_token or "").strip()
    if not token:
        raise RuntimeError("missing tunnel token")
    stop(home)
    argv: List[str]
    token_file: Optional[Path] = None
    if command is not None:
        argv = [str(part) for part in command]
    else:
        installed = inspect(home)
        if not installed.get("matches_pin"):
            raise RuntimeError("pinned cloudflared is not installed")
        token_file = _write_token_file(token, home)
        argv = [
            str(binary_path(home)),
            "tunnel",
            "--no-autoupdate",
            "run",
            "--token-file",
            str(token_file),
        ]
    log_path = install_dir(home) / "cloudflared.log"
    log_path.parent.mkdir(parents=True, exist_ok=True)
    handle = log_path.open("ab")
    try:
        try:
            proc = subprocess.Popen(
                argv,
                stdin=subprocess.DEVNULL,
                stdout=handle,
                stderr=subprocess.STDOUT,
                start_new_session=True,
            )
        except BaseException:
            if token_file is not None:
                _unlink_if_exists(token_file)
            raise
    finally:
        handle.close()
    time.sleep(0.1)
    if proc.poll() is not None:
        if token_file is not None:
            _unlink_if_exists(token_file)
        raise RuntimeError(f"cloudflared exited during startup; see {log_path}")
    try:
        executable = _resolve_executable(argv[0])
        atomic_write_json(
            pid_path(home),
            {"schema": 1, "pid": proc.pid, "executable": str(executable)},
        )
    except BaseException:
        try:
            try:
                proc.terminate()
            except OSError:
                pass
            proc.wait(timeout=2)
        except subprocess.TimeoutExpired:
            try:
                proc.kill()
            except OSError:
                pass
            proc.wait(timeout=2)
        finally:
            if token_file is not None:
                _unlink_if_exists(token_file)
        raise
    _owned_processes[proc.pid] = proc
    return {"pid": proc.pid, "argv0": argv[0]}


def status(home: Optional[Path] = None) -> Dict[str, Any]:
    pid = running_pid(home)
    return {"running": pid is not None, "pid": pid}
