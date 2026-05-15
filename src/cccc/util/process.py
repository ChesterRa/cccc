from __future__ import annotations

import ctypes
import os
import signal
import shutil
import subprocess
import time
from collections.abc import Sequence
from pathlib import Path
from typing import Any, Optional, Union

SignalValue = Union[int, signal.Signals]
SOFT_TERMINATE_SIGNAL: SignalValue = getattr(signal, "SIGTERM", signal.SIGINT)
HARD_TERMINATE_SIGNAL: SignalValue = getattr(signal, "SIGKILL", SOFT_TERMINATE_SIGNAL)


def find_subprocess_executable(command: str) -> Optional[str]:
    """Resolve an executable path suitable for direct subprocess execution."""
    raw = str(command or "").strip()
    if not raw:
        return None

    try:
        resolved = shutil.which(raw)
    except Exception:
        resolved = None

    if resolved:
        try:
            return str(Path(resolved).resolve())
        except Exception:
            return str(resolved)

    try:
        path = Path(raw)
    except Exception:
        path = None
    if path is not None and path.exists():
        try:
            return str(path.resolve())
        except Exception:
            return str(path)

    if os.name == "nt" and not _looks_like_path(raw):
        for directory in _iter_windows_user_bin_dirs():
            for name in _windows_command_name_candidates(raw):
                candidate = directory / name
                if not candidate.exists():
                    continue
                # Preserve the discovered path spelling instead of resolving
                # symlinks. On macOS test environments, tempfile roots under
                # /var are often symlinked to /private/var; resolving here
                # changes the string unexpectedly and breaks callers that only
                # need a directly executable path token.
                return str(candidate)

    return None


def _looks_like_path(command: str) -> bool:
    raw = str(command or "").strip()
    if not raw:
        return False
    if raw.startswith("."):
        return True
    return any(sep and sep in raw for sep in (os.sep, os.altsep))


def _iter_windows_user_bin_dirs() -> list[Path]:
    home = Path.home()
    candidates = [
        home / "bin",
        home / ".local" / "bin",
    ]
    local_appdata = str(os.environ.get("LOCALAPPDATA") or "").strip()
    appdata = str(os.environ.get("APPDATA") or "").strip()
    if local_appdata:
        candidates.append(Path(local_appdata) / "Microsoft" / "WinGet" / "Links")
    if appdata:
        candidates.append(Path(appdata) / "npm")

    unique: list[Path] = []
    seen: set[str] = set()
    for candidate in candidates:
        try:
            resolved = str(candidate.resolve())
        except Exception:
            resolved = str(candidate)
        if not resolved or resolved in seen:
            continue
        seen.add(resolved)
        unique.append(candidate)
    return unique


def _windows_command_name_candidates(command: str) -> list[str]:
    raw = str(command or "").strip()
    if not raw:
        return []
    suffix = Path(raw).suffix.lower()
    if suffix:
        return [raw]
    return [raw, f"{raw}.exe", f"{raw}.cmd", f"{raw}.bat"]


def resolve_subprocess_executable(command: str) -> str:
    """Return the safest executable token for subprocess calls."""
    raw = str(command or "").strip()
    if not raw:
        return raw
    if os.name != "nt" and not _looks_like_path(raw):
        return raw
    return find_subprocess_executable(raw) or raw


def resolve_subprocess_argv(argv: Sequence[str]) -> list[str]:
    """Resolve the executable token in an argv list for subprocess calls."""
    parts = [str(part) for part in (argv or [])]
    if not parts:
        return []
    parts[0] = resolve_subprocess_executable(parts[0])
    return parts


def _windows_pythonw_executable(executable: str) -> Optional[str]:
    raw = str(executable or "").strip()
    if os.name != "nt" or not raw:
        return None
    try:
        path = Path(raw)
    except Exception:
        return None
    if path.name.lower() == "pythonw.exe":
        return str(path)
    if path.stem.lower() != "python":
        return None
    candidate = path.with_name("pythonw.exe")
    if not candidate.exists():
        return None
    try:
        return str(candidate.resolve())
    except Exception:
        return str(candidate)


def resolve_background_python_argv(argv: Sequence[str]) -> list[str]:
    """Resolve Python background-service argv, preferring pythonw.exe on Windows."""
    parts = [str(part) for part in (argv or [])]
    if not parts:
        return []
    if os.name != "nt":
        return parts
    parts = resolve_subprocess_argv(parts)
    windowless = _windows_pythonw_executable(parts[0])
    if windowless:
        parts[0] = windowless
    return parts


def supervised_process_popen_kwargs() -> dict[str, Any]:
    """Return platform-appropriate Popen kwargs for supervised background services."""
    if os.name != "nt":
        return {"start_new_session": True}

    creationflags = int(getattr(subprocess, "CREATE_NEW_PROCESS_GROUP", 0))
    detached_process = int(getattr(subprocess, "DETACHED_PROCESS", 0))
    if detached_process:
        creationflags |= detached_process
    return {"creationflags": creationflags} if creationflags else {}


def _windows_taskkill_pid_tree(pid: int, *, force: bool) -> bool:
    if os.name != "nt":
        return False
    target_pid = int(pid or 0)
    if target_pid <= 0:
        return False
    argv = ["taskkill", "/PID", str(target_pid), "/T"]
    if force:
        argv.append("/F")
    try:
        result = subprocess.run(
            argv,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            check=False,
            text=True,
            timeout=5.0,
        )
        if int(getattr(result, "returncode", 1) or 0) == 0:
            return True
    except Exception:
        pass
    return not pid_is_alive(target_pid)


def _windows_kernel32() -> Any:
    if os.name != "nt":
        return None
    try:
        kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
        kernel32.OpenProcess.argtypes = [ctypes.c_uint32, ctypes.c_int, ctypes.c_uint32]
        kernel32.OpenProcess.restype = ctypes.c_void_p
        kernel32.GetExitCodeProcess.argtypes = [ctypes.c_void_p, ctypes.POINTER(ctypes.c_ulong)]
        kernel32.GetExitCodeProcess.restype = ctypes.c_int
        kernel32.TerminateProcess.argtypes = [ctypes.c_void_p, ctypes.c_uint32]
        kernel32.TerminateProcess.restype = ctypes.c_int
        kernel32.CloseHandle.argtypes = [ctypes.c_void_p]
        kernel32.CloseHandle.restype = ctypes.c_int
        return kernel32
    except Exception:
        return None


def _windows_pid_is_alive(pid: int) -> Optional[bool]:
    if os.name != "nt":
        return None
    target_pid = int(pid or 0)
    if target_pid <= 0:
        return False
    kernel32 = _windows_kernel32()
    if kernel32 is None:
        return None

    process_query_limited_information = 0x1000
    still_active = 259
    handle = kernel32.OpenProcess(process_query_limited_information, False, target_pid)
    if not handle:
        return False

    try:
        exit_code = ctypes.c_ulong()
        if not kernel32.GetExitCodeProcess(handle, ctypes.byref(exit_code)):
            return False
        return int(exit_code.value) == still_active
    finally:
        try:
            kernel32.CloseHandle(handle)
        except Exception:
            pass


def _windows_force_terminate_pid(pid: int) -> bool:
    if os.name != "nt":
        return False
    target_pid = int(pid or 0)
    if target_pid <= 0:
        return False
    kernel32 = _windows_kernel32()
    if kernel32 is None:
        return False

    process_terminate = 0x0001
    process_query_limited_information = 0x1000
    handle = kernel32.OpenProcess(process_terminate | process_query_limited_information, False, target_pid)
    if not handle:
        return False

    try:
        if not kernel32.TerminateProcess(handle, 1):
            return False
    finally:
        try:
            kernel32.CloseHandle(handle)
        except Exception:
            pass
    return not pid_is_alive(target_pid)


def _posix_pid_state(pid: int) -> str:
    if os.name == "nt":
        return ""
    target_pid = int(pid or 0)
    if target_pid <= 0:
        return ""

    proc_stat = Path("/proc") / str(target_pid) / "stat"
    try:
        raw = proc_stat.read_text(encoding="utf-8", errors="ignore").strip()
        marker = ") "
        if marker in raw:
            rest = raw.split(marker, 1)[1].strip()
            if rest:
                return rest[0].upper()
    except Exception:
        pass

    try:
        result = subprocess.run(
            ["ps", "-o", "stat=", "-p", str(target_pid)],
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
            check=False,
            text=True,
            timeout=0.5,
        )
    except Exception:
        return ""
    if int(getattr(result, "returncode", 1) or 0) != 0:
        return ""
    stat = str(getattr(result, "stdout", "") or "").strip()
    return stat[:1].upper() if stat else ""


def pid_is_alive(pid: int) -> bool:
    """Best-effort cross-platform liveness check."""
    target_pid = int(pid or 0)
    if target_pid <= 0:
        return False
    if os.name == "nt":
        windows_alive = _windows_pid_is_alive(target_pid)
        if windows_alive is not None:
            return bool(windows_alive)
    try:
        os.kill(target_pid, 0)
        return _posix_pid_state(target_pid) != "Z"
    except Exception:
        return False


def best_effort_signal_pid(pid: int, sig: SignalValue, *, include_group: bool = True) -> bool:
    """Best-effort signal delivery; prefer the process group on POSIX when requested."""
    target_pid = int(pid or 0)
    if target_pid <= 0:
        return False

    delivered = False
    if include_group and os.name != "nt":
        try:
            os.killpg(os.getpgid(target_pid), sig)
            delivered = True
        except Exception:
            pass

    if delivered:
        return True

    try:
        os.kill(target_pid, sig)
        return True
    except Exception:
        return False


def terminate_pid(
    pid: int,
    *,
    timeout_s: float = 1.0,
    include_group: bool = True,
    force: bool = False,
) -> bool:
    """Terminate a process with optional escalation to a hard kill."""
    target_pid = int(pid or 0)
    if target_pid <= 0:
        return True
    if not pid_is_alive(target_pid):
        return True

    if os.name == "nt" and include_group:
        _windows_taskkill_pid_tree(target_pid, force=False)
        deadline = time.time() + max(float(timeout_s or 0.0), 0.0)
        while time.time() < deadline:
            if not pid_is_alive(target_pid):
                return True
            time.sleep(0.05)

        if not force:
            return not pid_is_alive(target_pid)

        _windows_taskkill_pid_tree(target_pid, force=True)
        deadline = time.time() + max(float(timeout_s or 0.0), 0.0)
        while time.time() < deadline:
            if not pid_is_alive(target_pid):
                return True
            time.sleep(0.05)

        if _windows_force_terminate_pid(target_pid):
            deadline = time.time() + max(float(timeout_s or 0.0), 0.0)
            while time.time() < deadline:
                if not pid_is_alive(target_pid):
                    return True
                time.sleep(0.05)
        return not pid_is_alive(target_pid)

    best_effort_signal_pid(target_pid, SOFT_TERMINATE_SIGNAL, include_group=include_group)
    deadline = time.time() + max(float(timeout_s or 0.0), 0.0)
    while time.time() < deadline:
        if not pid_is_alive(target_pid):
            return True
        time.sleep(0.05)

    if not force:
        return not pid_is_alive(target_pid)

    best_effort_signal_pid(target_pid, HARD_TERMINATE_SIGNAL, include_group=include_group)
    deadline = time.time() + max(float(timeout_s or 0.0), 0.0)
    while time.time() < deadline:
        if not pid_is_alive(target_pid):
            return True
        time.sleep(0.05)
    return not pid_is_alive(target_pid)
