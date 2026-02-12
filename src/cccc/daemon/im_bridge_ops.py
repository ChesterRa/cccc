"""IM bridge process management helpers for daemon."""

from __future__ import annotations

import signal
from pathlib import Path
from typing import Callable, Dict, Optional


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
    if pid_path.exists():
        try:
            pid = int(pid_path.read_text(encoding="utf-8").strip())
            if pid > 0:
                best_effort_killpg(pid, signal.SIGTERM)
                killed.add(pid)
        except Exception:
            pass
        try:
            pid_path.unlink(missing_ok=True)
        except Exception:
            pass

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
            try:
                pid = int(pid_path.read_text(encoding="utf-8").strip())
                if pid > 0:
                    best_effort_killpg(pid, signal.SIGTERM)
                    killed.add(pid)
            except Exception:
                pass
            try:
                pid_path.unlink(missing_ok=True)
            except Exception:
                pass

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
            try:
                pid = int(pid_path.read_text(encoding="utf-8").strip())
            except Exception:
                pid = 0

            if pid <= 0 or not pid_alive(pid):
                stale_pidfiles += 1
                try:
                    pid_path.unlink(missing_ok=True)
                except Exception:
                    pass
                continue

            if not group_yaml.exists():
                best_effort_killpg(pid, signal.SIGTERM)
                killed += 1
                try:
                    pid_path.unlink(missing_ok=True)
                except Exception:
                    pass

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
