from __future__ import annotations

import signal
import time
from pathlib import Path
from typing import Callable

from ..paths import ensure_home
from ..util.fs import atomic_write_json, read_json
from ..util.time import utc_now_iso


def pty_state_path(group_id: str, actor_id: str) -> Path:
    home = ensure_home()
    return home / "groups" / str(group_id) / "state" / "runners" / "pty" / f"{actor_id}.json"


def write_pty_state(group_id: str, actor_id: str, *, pid: int) -> None:
    p = pty_state_path(group_id, actor_id)
    atomic_write_json(
        p,
        {
            "v": 1,
            "kind": "pty",
            "group_id": str(group_id),
            "actor_id": str(actor_id),
            "pid": int(pid),
            "started_at": utc_now_iso(),
        },
    )


def remove_pty_state_if_pid(group_id: str, actor_id: str, *, pid: int) -> None:
    p = pty_state_path(group_id, actor_id)
    if not p.exists():
        return
    doc = read_json(p)
    try:
        cur = int(doc.get("pid") or 0) if isinstance(doc, dict) else 0
    except Exception:
        cur = 0
    if cur and int(pid) and cur != int(pid):
        return
    try:
        p.unlink()
    except Exception:
        pass


def headless_state_path(group_id: str, actor_id: str) -> Path:
    home = ensure_home()
    return home / "groups" / str(group_id) / "state" / "runners" / "headless" / f"{actor_id}.json"


def write_headless_state(group_id: str, actor_id: str) -> None:
    p = headless_state_path(group_id, actor_id)
    atomic_write_json(
        p,
        {
            "v": 1,
            "kind": "headless",
            "group_id": str(group_id),
            "actor_id": str(actor_id),
            "started_at": utc_now_iso(),
        },
    )


def remove_headless_state(group_id: str, actor_id: str) -> None:
    p = headless_state_path(group_id, actor_id)
    try:
        if p.exists():
            p.unlink()
    except Exception:
        pass


def cleanup_stale_pty_state(
    home: Path,
    *,
    pid_alive: Callable[[int], bool],
    best_effort_killpg: Callable[[int, signal.Signals], None],
) -> None:
    base = home / "groups"
    if not base.exists():
        return
    for p in base.glob("*/state/runners/pty/*.json"):
        doc = read_json(p)
        if not isinstance(doc, dict) or str(doc.get("kind") or "") != "pty":
            try:
                p.unlink()
            except Exception:
                pass
            continue
        try:
            pid = int(doc.get("pid") or 0)
        except Exception:
            pid = 0
        if pid <= 0 or not pid_alive(pid):
            try:
                p.unlink()
            except Exception:
                pass
            continue
        best_effort_killpg(pid, signal.SIGTERM)
        deadline = time.time() + 1.0
        while time.time() < deadline and pid_alive(pid):
            time.sleep(0.05)
        if pid_alive(pid):
            best_effort_killpg(pid, signal.SIGKILL)
        try:
            p.unlink()
        except Exception:
            pass
