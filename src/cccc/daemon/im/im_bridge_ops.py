"""IM bridge process management helpers for daemon."""

from __future__ import annotations

import os
import signal
from pathlib import Path
from typing import Callable, Dict, Optional

from ...util.file_lock import LockUnavailableError, acquire_lockfile, release_lockfile
from ...util.process import pid_is_alive

_IM_BRIDGE_UNSAFE_CA_ENV = ("SSL_CERT_FILE", "REQUESTS_CA_BUNDLE", "CURL_CA_BUNDLE")


def sanitize_im_bridge_env(env: Dict[str, str]) -> Dict[str, str]:
    """Remove inherited CA bundle overrides that can break IM SDK TLS."""
    for key in _IM_BRIDGE_UNSAFE_CA_ENV:
        env.pop(key, None)
    return env


def _read_positive_pid(path: Path) -> Optional[int]:
    try:
        pid = int(path.read_text(encoding="utf-8").strip().splitlines()[0])
    except Exception:
        return None
    return pid if pid > 0 else None


def _remove_pidfile(pid_path: Path) -> None:
    try:
        pid_path.unlink(missing_ok=True)
    except Exception:
        pass


def _owned_im_bridge_pid(
    pid_path: Path,
    *,
    pid_alive: Callable[[int], bool] = pid_is_alive,
) -> Optional[int]:
    """Return the PID that currently owns this group's singleton lock.

    A bare pidfile is not process identity: after a crash its PID can be reused
    by an unrelated process. Every supported Python IM worker holds the sibling
    ``im_bridge.lock`` for its whole lifetime and writes its own PID there, so
    the held lock is the cross-platform ownership witness.
    """
    lock_path = pid_path.with_name("im_bridge.lock")
    if not lock_path.exists():
        _remove_pidfile(pid_path)
        return None

    try:
        probe = acquire_lockfile(lock_path, blocking=False)
    except LockUnavailableError:
        owner_pid = _read_positive_pid(lock_path)
        if owner_pid is not None and pid_alive(owner_pid):
            return owner_pid
        return None
    except Exception:
        # An unverifiable owner must fail closed: never guess which PID to kill.
        return None
    else:
        try:
            release_lockfile(probe)
        finally:
            _remove_pidfile(pid_path)
        return None


def read_live_im_bridge_pid(pid_path: Path) -> Optional[int]:
    """Return the current singleton-lock owner, removing stale pid metadata."""
    return _owned_im_bridge_pid(pid_path)


def _proc_cccc_home(pid: int) -> Optional[Path]:
    """Best-effort read CCCC_HOME for a pid (Linux /proc only)."""
    try:
        env_path = Path("/proc") / str(pid) / "environ"
        raw = env_path.read_bytes()
    except Exception:
        return None
    cccc_home = None
    try:
        for item in raw.split(b"\x00"):
            if item.startswith(b"CCCC_HOME="):
                cccc_home = item.split(b"=", 1)[1].decode("utf-8", "ignore").strip()
                break
    except Exception:
        cccc_home = None
    if cccc_home:
        try:
            return Path(cccc_home).expanduser().resolve()
        except Exception:
            return None
    try:
        return (Path.home() / ".cccc").resolve()
    except Exception:
        return None


def stop_im_bridges_for_group(
    home: Path,
    *,
    group_id: str,
    best_effort_killpg: Callable[[int, signal.Signals], None],
) -> int:
    gid = str(group_id or "").strip()
    if not gid:
        return 0

    killed: set[int] = set()
    pid_path = home / "groups" / gid / "state" / "im_bridge.pid"
    pid = read_live_im_bridge_pid(pid_path)
    if pid is not None:
        best_effort_killpg(pid, signal.SIGTERM)
        killed.add(pid)
        _remove_pidfile(pid_path)

    proc = Path("/proc")
    if proc.exists():
        for proc_dir in proc.iterdir():
            if not proc_dir.is_dir() or not proc_dir.name.isdigit():
                continue
            pid = int(proc_dir.name)
            if pid in killed:
                continue
            try:
                cmdline = (proc_dir / "cmdline").read_bytes().decode("utf-8", "ignore")
            except Exception:
                continue
            if "cccc.ports.im.bridge" not in cmdline or gid not in cmdline:
                continue
            proc_home = _proc_cccc_home(pid)
            if proc_home is None:
                continue
            try:
                if proc_home != home.resolve():
                    continue
            except Exception:
                continue
            best_effort_killpg(pid, signal.SIGTERM)
            killed.add(pid)

    return len(killed)


def stop_all_im_bridges(
    home: Path,
    *,
    best_effort_killpg: Callable[[int, signal.Signals], None],
) -> int:
    killed: set[int] = set()

    base = home / "groups"
    if base.exists():
        for pid_path in base.glob("*/state/im_bridge.pid"):
            pid = read_live_im_bridge_pid(pid_path)
            if pid is not None:
                best_effort_killpg(pid, signal.SIGTERM)
                killed.add(pid)
                _remove_pidfile(pid_path)

    proc = Path("/proc")
    if proc.exists():
        for proc_dir in proc.iterdir():
            if not proc_dir.is_dir() or not proc_dir.name.isdigit():
                continue
            pid = int(proc_dir.name)
            if pid in killed:
                continue
            try:
                cmdline = (proc_dir / "cmdline").read_bytes().decode("utf-8", "ignore")
            except Exception:
                continue
            if "cccc.ports.im.bridge" not in cmdline:
                continue
            proc_home = _proc_cccc_home(pid)
            if proc_home is None:
                continue
            try:
                if proc_home != home.resolve():
                    continue
            except Exception:
                continue
            best_effort_killpg(pid, signal.SIGTERM)
            killed.add(pid)

    return len(killed)


def cleanup_invalid_im_bridges(
    home: Path,
    *,
    pid_alive: Callable[[int], bool],
    best_effort_killpg: Callable[[int, signal.Signals], None],
) -> Dict[str, int]:
    killed = 0
    stale_pidfiles = 0

    base = home / "groups"
    if base.exists():
        for pid_path in base.glob("*/state/im_bridge.pid"):
            gid = pid_path.parent.parent.name
            group_yaml = base / gid / "group.yaml"
            existed = pid_path.exists()
            pid = _owned_im_bridge_pid(pid_path, pid_alive=pid_alive)
            if pid is None:
                if existed and not pid_path.exists():
                    stale_pidfiles += 1
                continue

            if not group_yaml.exists():
                best_effort_killpg(pid, signal.SIGTERM)
                killed += 1
                _remove_pidfile(pid_path)
            elif not pid_alive(pid):
                stale_pidfiles += 1
                _remove_pidfile(pid_path)

    proc = Path("/proc")
    if proc.exists():
        for proc_dir in proc.iterdir():
            if not proc_dir.is_dir() or not proc_dir.name.isdigit():
                continue
            pid = int(proc_dir.name)
            try:
                cmdline = (proc_dir / "cmdline").read_bytes().decode("utf-8", "ignore")
            except Exception:
                continue
            if "cccc.ports.im.bridge" not in cmdline:
                continue

            proc_home = _proc_cccc_home(pid)
            if proc_home is None:
                continue
            try:
                if proc_home != home.resolve():
                    continue
            except Exception:
                continue

            argv = [a for a in cmdline.split("\x00") if a]
            try:
                index = argv.index("cccc.ports.im.bridge")
            except ValueError:
                continue
            if index + 1 >= len(argv):
                continue
            gid = str(argv[index + 1] or "").strip()
            if not gid.startswith("g_"):
                continue

            group_yaml = home / "groups" / gid / "group.yaml"
            if not group_yaml.exists():
                best_effort_killpg(pid, signal.SIGTERM)
                killed += 1

    return {"killed": killed, "stale_pidfiles": stale_pidfiles}
