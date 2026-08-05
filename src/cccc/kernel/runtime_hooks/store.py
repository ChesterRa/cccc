from __future__ import annotations

import hashlib
import json
from pathlib import Path
from typing import Any, Callable, Mapping, Optional

from ...util.file_lock import acquire_lockfile, release_lockfile
from ...util.time import utc_now_iso
from .committed_io import write_json_committed
from .contracts import STATE_VERSION, HookState, validate_runtime
from .state_machine import reduce_hook_event, reduce_interrupt, reduce_terminal_input


def state_path(home: Path, runtime: str, group_id: str, actor_id: str) -> Path:
    runtime = validate_runtime(runtime)
    digest = hashlib.sha256(
        str(group_id).encode("utf-8") + b"\0" + str(actor_id).encode("utf-8")
    ).hexdigest()
    return Path(home) / "daemon" / f"{runtime}-hook-state" / f"{digest}.json"


def _lock_path(home: Path, runtime: str, group_id: str, actor_id: str) -> Path:
    return state_path(home, runtime, group_id, actor_id).with_suffix(".lock")


def _read_strict(path: Path) -> Any:
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except FileNotFoundError:
        return None
    except (OSError, UnicodeError, json.JSONDecodeError) as exc:
        raise ValueError(f"invalid hook state file: {path}") from exc


def _read_state_unlocked(
    home: Path,
    runtime: str,
    group_id: str,
    actor_id: str,
) -> Optional[HookState]:
    runtime = validate_runtime(runtime)
    raw = _read_strict(state_path(home, runtime, group_id, actor_id))
    if raw is None:
        return None
    state = HookState.from_dict(raw)
    if (
        state.runtime != runtime
        or state.group_id != group_id
        or state.actor_id != actor_id
    ):
        raise ValueError("hook state identity mismatch")
    return state


def read_state(
    home: Path,
    runtime: str,
    group_id: str,
    actor_id: str,
) -> Optional[HookState]:
    return _read_state_unlocked(Path(home), runtime, group_id, actor_id)


def _write_state(home: Path, state: HookState) -> None:
    write_json_committed(
        state_path(home, state.runtime, state.group_id, state.actor_id),
        state.to_dict(),
    )


def begin_launch(
    home: Path,
    runtime: str,
    group_id: str,
    actor_id: str,
    launch_token: str,
    event: str = "HookPending",
    observer: Callable[[HookState], None] | None = None,
) -> HookState:
    runtime = validate_runtime(runtime)
    if not str(group_id).strip() or not str(actor_id).strip():
        raise ValueError("missing group or actor identity")
    if not str(launch_token).strip():
        raise ValueError("launch token must be non-empty")
    state = HookState(
        v=STATE_VERSION,
        runtime=runtime,
        group_id=str(group_id),
        actor_id=str(actor_id),
        status="waiting",
        event=str(event),
        session_id="",
        turn_id=None,
        operation_id=None,
        interrupted=False,
        awaiting_session_start=True,
        turn_generation=0,
        launch_token=str(launch_token),
        seen_turn_ids=[],
        seen_operation_ids=[],
        turn_fence_exhausted=False,
        operation_fence_exhausted=False,
        diagnostic=None,
        session_closed=False,
        observation="pty_fail_closed" if runtime == "claude" else "full_fidelity",
        updated_at=utc_now_iso(),
    )
    lock = acquire_lockfile(_lock_path(home, runtime, group_id, actor_id))
    try:
        previous = _read_state_unlocked(
            Path(home), runtime, group_id, actor_id
        )
        _write_state(Path(home), state)
        try:
            if observer is not None:
                observer(state)
        except Exception:
            if previous is None:
                state_path(
                    home, runtime, group_id, actor_id
                ).unlink(missing_ok=True)
            else:
                _write_state(Path(home), previous)
            raise
    finally:
        release_lockfile(lock)
    return state


def record_hook_event(
    home: Path,
    runtime: str,
    group_id: str,
    actor_id: str,
    launch_token: str,
    payload: Mapping[str, Any],
) -> HookState:
    runtime = validate_runtime(runtime)
    lock = acquire_lockfile(_lock_path(home, runtime, group_id, actor_id))
    try:
        previous = _read_state_unlocked(Path(home), runtime, group_id, actor_id)
        if previous is None:
            raise ValueError("hook event received before launch configuration")
        state = reduce_hook_event(
            previous,
            launch_token,
            payload,
            updated_at=utc_now_iso(),
        )
        authorized = state != previous or _claude_observation_authorized(
            previous, launch_token, payload
        )
        if state != previous:
            _write_state(Path(home), state)
        if authorized:
            try:
                from . import activity

                activity.record_hook_activity(
                    Path(home), runtime, launch_token, payload, state
                )
            except Exception:
                if state != previous:
                    _write_state(Path(home), previous)
                raise
        return state
    finally:
        release_lockfile(lock)


def _claude_observation_authorized(
    state: HookState,
    launch_token: str,
    payload: Mapping[str, Any],
) -> bool:
    return (
        state.runtime == "claude"
        and state.v == STATE_VERSION
        and not state.awaiting_session_start
        and not state.session_closed
        and bool(str(launch_token).strip())
        and launch_token == state.launch_token
        and str(payload.get("session_id") or "").strip() == state.session_id
    )


def _mutate_with_activity_close(
    home: Path,
    runtime: str,
    group_id: str,
    actor_id: str,
    *,
    reducer: Any,
    event_type: str,
) -> Optional[HookState]:
    runtime = validate_runtime(runtime)
    lock = acquire_lockfile(_lock_path(home, runtime, group_id, actor_id))
    try:
        previous = _read_state_unlocked(Path(home), runtime, group_id, actor_id)
        if previous is None:
            return None
        state = reducer(previous, updated_at=utc_now_iso())
        if state == previous:
            return state
        _write_state(Path(home), state)
        try:
            from .activity import close_actor_activities

            close_actor_activities(Path(home), state, event_type)
        except Exception:
            _write_state(Path(home), previous)
            raise
        return state
    finally:
        release_lockfile(lock)


def record_interrupt(
    home: Path, runtime: str, group_id: str, actor_id: str
) -> Optional[HookState]:
    return _mutate_with_activity_close(
        Path(home),
        runtime,
        group_id,
        actor_id,
        reducer=reduce_interrupt,
        event_type="UserInterrupt",
    )


def record_terminal_input(
    home: Path, runtime: str, group_id: str, actor_id: str
) -> Optional[HookState]:
    return _mutate_with_activity_close(
        Path(home),
        runtime,
        group_id,
        actor_id,
        reducer=reduce_terminal_input,
        event_type="TurnSuperseded",
    )
