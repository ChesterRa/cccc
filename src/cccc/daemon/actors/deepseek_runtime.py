"""Process-local DeepSeek ACP supervisor registry for actor lifecycle."""
from __future__ import annotations

import logging
import os
import shutil
import threading
from pathlib import Path
from typing import Dict, List

from ...runners.deepseek import DeepSeekSupervisor
from . import deepseek_restart_gate
from .deepseek_setup import ensure_deepseek_setup

logger = logging.getLogger("cccc.daemon.deepseek_runtime")
_LOCK = threading.RLock()
_SUPERVISORS: Dict[tuple[str, str], DeepSeekSupervisor] = {}
_MANUAL_RESTART_REQUIRED: set[tuple[str, str]] = set()


def start(
    *,
    group_id: str,
    actor_id: str,
    actor_created_at: str,
    group_path: Path,
    cwd: Path,
    command: List[str],
    env: Dict[str, str],
) -> DeepSeekSupervisor:
    key = (str(group_id), str(actor_id))
    effective_env = dict(os.environ)
    effective_env.update(env)
    executable = Path(str(command[0] if command else "")).stem.lower()
    # Setup owns the pinned ACP app and composition. Resolve the explicit
    # config before taking the process registry lock because npm may be slow.
    if executable in {"dsh", "dsh-acp-demo"}:
        outcome = ensure_deepseek_setup(effective_env)
        app = shutil.which("dsh-acp-demo", path=effective_env.get("PATH")) or str(command[0])
        command = [app, "--config", str(outcome.profile / "cordis.yml")]
    cccc_home = str(effective_env.get("CCCC_HOME") or "").strip()
    if not cccc_home:
        raise RuntimeError("CCCC_HOME is required for DeepSeek session persistence")
    session_root = Path(group_path) / "state" / "deepseek" / str(actor_id) / "sessions"
    session_root.mkdir(parents=True, exist_ok=True)
    effective_env["CCCC_GROUP_ID"] = str(group_id)
    effective_env["CCCC_ACTOR_ID"] = str(actor_id)
    effective_env["CCCC_DEEPSEEK_SESSION_ROOT"] = str(session_root)
    with _LOCK:
        current = _SUPERVISORS.get(key)
        if current is not None:
            current.stop()
        supervisor = DeepSeekSupervisor(command, cwd=str(cwd), env=effective_env)
        supervisor.start()
        try:
            supervisor.handshake(timeout=5.0)
        except Exception:
            supervisor.stop()
            raise
        _SUPERVISORS[key] = supervisor
        try:
            deepseek_restart_gate.record_running_generation(
                group_path=group_path,
                group_id=group_id,
                actor_id=actor_id,
                actor_created_at=actor_created_at,
                generation=supervisor.generation,
            )
        except Exception:
            _SUPERVISORS.pop(key, None)
            supervisor.stop()
            raise
        _MANUAL_RESTART_REQUIRED.discard(key)
        return supervisor


def stop(*, group_id: str, actor_id: str) -> None:
    key = (str(group_id), str(actor_id))
    with _LOCK:
        supervisor = _SUPERVISORS.pop(key, None)
        _MANUAL_RESTART_REQUIRED.discard(key)
    if supervisor is not None:
        supervisor.stop()


def require_manual_restart(
    *,
    group_id: str,
    actor_id: str,
    actor_created_at: str,
    group_path: Path,
    expected_generation: str,
    reason_code: str,
) -> bool:
    """Fence one failed generation and stop it without poisoning a replacement."""
    key = (str(group_id), str(actor_id))
    with _LOCK:
        supervisor = _SUPERVISORS.get(key)
        if supervisor is None or supervisor.generation != str(expected_generation):
            return False
        try:
            gate_applied = deepseek_restart_gate.require_manual_restart(
                group_path=group_path,
                group_id=group_id,
                actor_id=actor_id,
                actor_created_at=actor_created_at,
                expected_generation=expected_generation,
                reason_code=reason_code,
            )
        except Exception as error:
            logger.error(
                "failed to persist DeepSeek manual restart gate for %s/%s: %s",
                group_id,
                actor_id,
                error,
            )
            # Keep the current daemon fail-closed even if durable storage is
            # unavailable. A later explicit start can repair the state file.
            gate_applied = True
        if gate_applied:
            _SUPERVISORS.pop(key, None)
        else:
            return False
        _MANUAL_RESTART_REQUIRED.add(key)
    supervisor.stop()
    return True


def manual_restart_required(
    *,
    group_id: str,
    actor_id: str,
    actor_created_at: str,
    group_path: Path,
) -> bool:
    key = (str(group_id), str(actor_id))
    with _LOCK:
        if key in _MANUAL_RESTART_REQUIRED:
            return True
        try:
            return deepseek_restart_gate.manual_restart_required(
                group_path=group_path,
                group_id=group_id,
                actor_id=actor_id,
                actor_created_at=actor_created_at,
            )
        except Exception as error:
            logger.error(
                "failed to read DeepSeek manual restart gate for %s/%s: %s",
                group_id,
                actor_id,
                error,
            )
            return True


def running(*, group_id: str, actor_id: str) -> bool:
    key = (str(group_id), str(actor_id))
    with _LOCK:
        supervisor = _SUPERVISORS.get(key)
        return bool(supervisor is not None and supervisor.is_running())


def get(*, group_id: str, actor_id: str) -> DeepSeekSupervisor | None:
    key = (str(group_id), str(actor_id))
    with _LOCK:
        supervisor = _SUPERVISORS.get(key)
        return supervisor if supervisor is not None and supervisor.is_running() else None


def stop_group(*, group_id: str) -> None:
    target = str(group_id)
    with _LOCK:
        actor_ids = {
            actor_id for (gid, actor_id) in _SUPERVISORS if gid == target
        } | {
            actor_id for (gid, actor_id) in _MANUAL_RESTART_REQUIRED if gid == target
        }
    for actor_id in actor_ids:
        stop(group_id=target, actor_id=actor_id)


def group_running(group_id: str) -> bool:
    target = str(group_id)
    with _LOCK:
        supervisors = [supervisor for (gid, _), supervisor in _SUPERVISORS.items() if gid == target]
    return any(supervisor.is_running() for supervisor in supervisors)


def stop_all() -> None:
    with _LOCK:
        keys = set(_SUPERVISORS) | set(_MANUAL_RESTART_REQUIRED)
    for group_id, actor_id in keys:
        stop(group_id=group_id, actor_id=actor_id)
