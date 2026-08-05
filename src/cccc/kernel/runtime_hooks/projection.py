from __future__ import annotations

import hashlib
import json
from dataclasses import replace
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Optional, Sequence

from .contracts import RuntimeActivityEvent
from .store import read_state, state_path

RECENT_COMPLETED_SECONDS = 15
STUCK_AFTER_SECONDS = 60


def launch_identity_path(home: Path, group_id: str, actor_id: str) -> Path:
    digest = hashlib.sha256(
        str(group_id).encode() + b"\0" + str(actor_id).encode()
    ).hexdigest()
    return Path(home) / "daemon" / "runtime-hook-launch" / f"{digest}.json"


def read_launch_identity(
    home: Path, group_id: str, actor_id: str
) -> dict[str, Any] | None:
    path = launch_identity_path(home, group_id, actor_id)
    try:
        raw = json.loads(path.read_text(encoding="utf-8"))
    except FileNotFoundError:
        return None
    except (OSError, UnicodeError, json.JSONDecodeError):
        return None
    if (
        not isinstance(raw, dict)
        or raw.get("v") != 1
        or raw.get("group_id") != group_id
        or raw.get("actor_id") != actor_id
        or not isinstance(raw.get("hook_enabled"), bool)
        or type(raw.get("pid")) is not int
        or raw["pid"] < 0
    ):
        return None
    return raw


def read_working_projection(
    home: Path,
    runtime: str,
    group_id: str,
    actor_id: str,
    *,
    launch_identity: dict[str, Any] | None = None,
) -> dict[str, Any] | None:
    runtime_name = str(runtime or "").strip().lower()
    if runtime_name not in {"codex", "claude"}:
        return None
    identity = launch_identity or read_launch_identity(
        home, group_id, actor_id
    )
    if (
        identity is None
        or identity.get("runtime") != runtime_name
        or identity.get("hook_enabled") is not True
    ):
        return None
    path = state_path(home, runtime_name, group_id, actor_id)
    if not path.exists():
        return None
    try:
        state = read_state(home, runtime_name, group_id, actor_id)
    except ValueError:
        return _projection(
            "waiting", f"{runtime_name}_hook_state_unavailable", None, None
        )
    if state is None:
        return _projection(
            "waiting", f"{runtime_name}_hook_state_unavailable", None, None
        )
    if state.launch_token != identity.get("launch_token"):
        return _projection(
            "waiting", f"{runtime_name}_hook_state_unavailable", None, None
        )
    if state.v == 2:
        reason = f"{state.runtime}_hook_legacy_unfenced_{state.event}"
    elif state.observation == "pty_fail_closed":
        reason = f"claude_pty_fail_closed_{state.event}"
    else:
        reason = f"{state.runtime}_hook_{state.event}"
    return _projection(state.status, reason, state.updated_at, state.turn_id)


def runtime_hook_working_projection(
    home: Path,
    *,
    running: bool,
    effective_runner: str,
    runtime: str,
    group_id: str,
    actor_id: str,
    session_capability: Any = None,
) -> dict[str, Any] | None:
    if not running or effective_runner != "pty":
        return None
    runtime_name = str(runtime or "").strip().lower()
    capability_runtime = _capability_value(
        session_capability, "runtime"
    )
    capability_token = _capability_value(
        session_capability, "launch_token"
    )
    capability_source = _capability_value(
        session_capability, "runtime_state_source"
    )
    try:
        capability_pid = int(
            _capability_value(session_capability, "pid") or 0
        )
    except (TypeError, ValueError):
        return None
    if (
        capability_runtime != runtime_name
        or capability_source != "terminal"
        or not capability_token
        or capability_pid <= 0
    ):
        return None
    identity = read_launch_identity(home, group_id, actor_id)
    if (
        identity is None
        or identity.get("runtime") != runtime_name
        or identity.get("launch_token") != capability_token
        or identity["pid"] != capability_pid
    ):
        return None
    return read_working_projection(
        home,
        runtime_name,
        group_id,
        actor_id,
        launch_identity=identity,
    )


def _capability_value(capability: Any, name: str) -> Any:
    if isinstance(capability, dict):
        return capability.get(name)
    return getattr(capability, name, None)


def _projection(
    status: str, reason: str, updated_at: str | None, active_task_id: str | None
) -> dict[str, Any]:
    return {
        "effective_working_state": status,
        "effective_working_reason": reason,
        "effective_working_updated_at": updated_at,
        "effective_active_task_id": active_task_id,
    }


def project_snapshot(
    events: Sequence[RuntimeActivityEvent],
    *,
    now: Optional[datetime] = None,
) -> list[RuntimeActivityEvent]:
    moment = now or datetime.now(timezone.utc)
    latest = {
        (event.actor_id, event.activity_id): event for event in events
    }
    projected = [
        event
        for event in latest.values()
        if _activity_active(event)
        or (
            (age := _age_seconds(event, moment)) is not None
            and age <= RECENT_COMPLETED_SECONDS
        )
    ]
    projected.extend(_stuck_events(latest, moment))
    return sorted(projected, key=lambda event: (event.ts, event.id))


def _age_seconds(
    event: RuntimeActivityEvent, now: datetime
) -> Optional[int]:
    try:
        parsed = datetime.fromisoformat(
            str(event.ts).replace("Z", "+00:00")
        )
        if parsed.tzinfo is None:
            parsed = parsed.replace(tzinfo=timezone.utc)
        return max(
            0,
            int(
                (
                    now - parsed.astimezone(timezone.utc)
                ).total_seconds()
            ),
        )
    except (TypeError, ValueError):
        return None


def _activity_active(event: RuntimeActivityEvent) -> bool:
    return event.status in {"started", "waiting", "stuck"}


def _stuck_events(
    latest: dict[tuple[str, str], RuntimeActivityEvent],
    now: datetime,
) -> list[RuntimeActivityEvent]:
    active_tool_actors = {
        event.actor_id
        for event in latest.values()
        if event.kind == "tool" and _activity_active(event)
    }
    stuck: list[RuntimeActivityEvent] = []
    for event in latest.values():
        age = _age_seconds(event, now)
        if (
            _activity_active(event)
            and event.kind in {"turn", "tool"}
            and not (
                event.kind == "turn"
                and event.actor_id in active_tool_actors
            )
            and age is not None
            and age >= STUCK_AFTER_SECONDS
        ):
            stuck.append(
                replace(
                    event,
                    id=f"stuck:{event.id}",
                    ts=now.isoformat().replace("+00:00", "Z"),
                    status="stuck",
                    event_type="StuckDetected",
                    duration_ms=age * 1000,
                )
            )
    return stuck
