"""Durable DeepSeek manual-restart gate shared by both daemon engines."""

from __future__ import annotations

import json
from pathlib import Path
from typing import Any

from ...kernel.actors import validate_actor_id
from ...util.file_lock import acquire_lockfile, release_lockfile
from ...util.fs import atomic_write_json
from ...util.time import utc_now_iso

_STATE_VERSION = 1
_STATE_FILENAME = "runtime-state.json"


def record_running_generation(
    *,
    group_path: Path,
    group_id: str,
    actor_id: str,
    actor_created_at: str,
    generation: str,
) -> None:
    clean_generation = str(generation or "").strip()
    if not clean_generation:
        raise ValueError("DeepSeek launch generation is required")
    state_path, lock_path = _paths(group_path, actor_id)
    lock = acquire_lockfile(lock_path, blocking=True)
    try:
        atomic_write_json(
            state_path,
            {
                "v": _STATE_VERSION,
                "group_id": str(group_id),
                "actor_id": str(actor_id),
                "actor_created_at": str(actor_created_at or "").strip(),
                "generation": clean_generation,
                "manual_restart_required": False,
                "reason_code": "",
                "updated_at": utc_now_iso(),
            },
            indent=2,
        )
    finally:
        release_lockfile(lock)


def require_manual_restart(
    *,
    group_path: Path,
    group_id: str,
    actor_id: str,
    actor_created_at: str,
    expected_generation: str,
    reason_code: str,
) -> bool:
    clean_generation = str(expected_generation or "").strip()
    if not clean_generation:
        raise ValueError("DeepSeek launch generation is required")
    state_path, lock_path = _paths(group_path, actor_id)
    lock = acquire_lockfile(lock_path, blocking=True)
    try:
        state = _read_optional(state_path)
        if state is None or not _matches_actor(
            state,
            group_id=group_id,
            actor_id=actor_id,
            actor_created_at=actor_created_at,
        ):
            return False
        if str(state.get("generation") or "").strip() != clean_generation:
            return False
        state["manual_restart_required"] = True
        state["reason_code"] = str(reason_code or "").strip()
        state["updated_at"] = utc_now_iso()
        atomic_write_json(state_path, state, indent=2)
        return True
    finally:
        release_lockfile(lock)


def manual_restart_required(
    *,
    group_path: Path,
    group_id: str,
    actor_id: str,
    actor_created_at: str,
) -> bool:
    state_path, lock_path = _paths(group_path, actor_id)
    lock = acquire_lockfile(lock_path, blocking=True)
    try:
        state = _read_optional(state_path)
        return bool(
            state is not None
            and _matches_actor(
                state,
                group_id=group_id,
                actor_id=actor_id,
                actor_created_at=actor_created_at,
            )
            and state.get("manual_restart_required") is True
        )
    finally:
        release_lockfile(lock)


def _matches_actor(
    state: dict[str, Any],
    *,
    group_id: str,
    actor_id: str,
    actor_created_at: str,
) -> bool:
    version = state.get("v")
    if version != _STATE_VERSION:
        raise ValueError(f"unsupported DeepSeek runtime state version {version!r}")
    return (
        str(state.get("group_id") or "") == str(group_id)
        and str(state.get("actor_id") or "") == str(actor_id)
        and str(state.get("actor_created_at") or "").strip()
        == str(actor_created_at or "").strip()
    )


def _read_optional(path: Path) -> dict[str, Any] | None:
    try:
        raw = json.loads(path.read_text(encoding="utf-8"))
    except FileNotFoundError:
        return None
    if not isinstance(raw, dict):
        raise ValueError("DeepSeek runtime state must be a JSON object")
    return raw


def _paths(group_path: Path, actor_id: str) -> tuple[Path, Path]:
    clean_actor_id = validate_actor_id(str(actor_id))
    directory = Path(group_path) / "state" / "deepseek" / clean_actor_id
    state_path = directory / _STATE_FILENAME
    return state_path, directory / f"{_STATE_FILENAME}.lock"
