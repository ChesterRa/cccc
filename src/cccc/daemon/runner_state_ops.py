from __future__ import annotations

import signal
import time
from pathlib import Path
from typing import Any, Callable, Dict

from ..kernel.group import load_group
from ..paths import ensure_home
from ..util.conv import coerce_bool
from ..util.fs import atomic_write_json, read_json
from ..util.process import HARD_TERMINATE_SIGNAL
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


def remove_pty_state_if_pid(group_id: str, actor_id: str, *, pid: int) -> bool:
    p = pty_state_path(group_id, actor_id)
    if not p.exists():
        return False
    doc = read_json(p)
    try:
        cur = int(doc.get("pid") or 0) if isinstance(doc, dict) else 0
    except Exception:
        cur = 0
    if cur and int(pid) and cur != int(pid):
        return False
    try:
        p.unlink()
        return True
    except Exception:
        return False


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
            "status": "waiting",
            "started_at": utc_now_iso(),
            "updated_at": utc_now_iso(),
        },
    )


def update_headless_state(group_id: str, actor_id: str, *, status: str = "", **updates: Any) -> bool:
    p = headless_state_path(group_id, actor_id)
    doc = read_json(p)
    if not isinstance(doc, dict) or str(doc.get("kind") or "") != "headless":
        return False
    if str(doc.get("group_id") or "") != str(group_id) or str(doc.get("actor_id") or "") != str(actor_id):
        return False
    normalized_status = str(status or "").strip().lower()
    if normalized_status:
        if normalized_status not in {"idle", "working", "waiting", "stopped"}:
            return False
        doc["status"] = normalized_status
    for key, value in updates.items():
        if value is not None:
            doc[str(key)] = value
    doc["updated_at"] = utc_now_iso()
    atomic_write_json(p, doc)
    return True


def read_headless_state(group_id: str, actor_id: str) -> Dict[str, Any]:
    doc = read_json(headless_state_path(group_id, actor_id))
    return doc if isinstance(doc, dict) else {}


def headless_state_running(group_id: str, actor_id: str) -> bool:
    doc = read_headless_state(group_id, actor_id)
    return bool(
        str(doc.get("kind") or "") == "headless"
        and str(doc.get("group_id") or "") == str(group_id)
        and str(doc.get("actor_id") or "") == str(actor_id)
        and str(doc.get("status") or "").strip().lower() != "stopped"
    )


def web_model_actor_running(group_id: str, actor: Dict[str, Any]) -> bool:
    if not isinstance(actor, dict):
        return False
    actor_id = str(actor.get("id") or "").strip()
    if not actor_id:
        return False
    runtime = str(actor.get("runtime") or "").strip().lower()
    runner = str(actor.get("runner") or "headless").strip().lower() or "headless"
    if runtime != "web_model" or runner != "headless":
        return False
    if not coerce_bool(actor.get("enabled"), default=True):
        return False
    return headless_state_running(group_id, actor_id)


def web_model_group_running(group_id: str) -> bool:
    group = load_group(str(group_id or "").strip())
    if group is None:
        return False
    actors = group.doc.get("actors") if isinstance(group.doc.get("actors"), list) else []
    return any(web_model_actor_running(group.group_id, actor) for actor in actors if isinstance(actor, dict))


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
            best_effort_killpg(pid, HARD_TERMINATE_SIGNAL)
        try:
            p.unlink()
        except Exception:
            pass
