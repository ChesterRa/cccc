from __future__ import annotations

from dataclasses import asdict, dataclass
from typing import Any, ClassVar, Optional


STATE_VERSION = 3
LEGACY_STATE_VERSION = 2
ACTIVITY_VERSION = 1
SUPPORTED_RUNTIMES = frozenset({"codex", "claude"})
MAX_SEEN_TURNS = 4096
MAX_SEEN_OPERATIONS = 4096
ACTIVITY_EVENT_LIMIT = 256
ACTIVITY_RETENTION_SECONDS = 300


def validate_runtime(runtime: str) -> str:
    value = str(runtime or "").strip().lower()
    if value not in SUPPORTED_RUNTIMES:
        raise ValueError(f"unsupported hook runtime: {runtime}")
    return value


def _required_text(raw: dict[str, Any], key: str, *, allow_empty: bool = False) -> str:
    value = raw.get(key)
    if not isinstance(value, str) or (not allow_empty and not value.strip()):
        raise ValueError(f"invalid hook field: {key}")
    return value


def _optional_text(raw: dict[str, Any], key: str) -> Optional[str]:
    value = raw.get(key)
    if value is None:
        return None
    if not isinstance(value, str):
        raise ValueError(f"invalid hook field: {key}")
    return value


def _text_list(raw: dict[str, Any], key: str) -> list[str]:
    value = raw.get(key, [])
    if not isinstance(value, list) or any(not isinstance(item, str) for item in value):
        raise ValueError(f"invalid hook field: {key}")
    return list(value)


@dataclass(frozen=True)
class HookState:
    v: int
    runtime: str
    group_id: str
    actor_id: str
    status: str
    event: str
    session_id: str
    turn_id: Optional[str]
    operation_id: Optional[str]
    interrupted: bool
    awaiting_session_start: bool
    turn_generation: int
    launch_token: str
    seen_turn_ids: list[str]
    seen_operation_ids: list[str]
    turn_fence_exhausted: bool
    operation_fence_exhausted: bool
    diagnostic: Optional[str]
    session_closed: bool
    observation: str
    updated_at: str

    _STATUSES: ClassVar[frozenset[str]] = frozenset({"stopped", "idle", "working", "waiting"})

    @classmethod
    def from_dict(cls, raw: Any) -> "HookState":
        if not isinstance(raw, dict):
            raise ValueError("hook state must be an object")
        version = raw.get("v")
        if type(version) is not int or version not in {LEGACY_STATE_VERSION, STATE_VERSION}:
            raise ValueError("unsupported hook state version")
        runtime = validate_runtime(str(raw.get("runtime") or "codex"))
        status = _required_text(raw, "status")
        if status not in cls._STATUSES:
            raise ValueError("invalid hook status")
        generation = raw.get("turn_generation", 0)
        if type(generation) is not int or generation < 0:
            raise ValueError("invalid turn_generation")
        bool_fields = (
            "interrupted",
            "awaiting_session_start",
            "turn_fence_exhausted",
            "operation_fence_exhausted",
            "session_closed",
        )
        for field in bool_fields:
            if field in raw and type(raw[field]) is not bool:
                raise ValueError(f"invalid hook field: {field}")
        return cls(
            v=version,
            runtime=runtime,
            group_id=_required_text(raw, "group_id"),
            actor_id=_required_text(raw, "actor_id"),
            status=status,
            event=_required_text(raw, "event"),
            session_id=_required_text(raw, "session_id", allow_empty=True),
            turn_id=_optional_text(raw, "turn_id"),
            operation_id=_optional_text(raw, "operation_id"),
            interrupted=bool(raw.get("interrupted", False)),
            awaiting_session_start=bool(raw.get("awaiting_session_start", False)),
            turn_generation=generation,
            launch_token=str(raw.get("launch_token") or ""),
            seen_turn_ids=_text_list(raw, "seen_turn_ids"),
            seen_operation_ids=_text_list(raw, "seen_operation_ids"),
            turn_fence_exhausted=bool(raw.get("turn_fence_exhausted", False)),
            operation_fence_exhausted=bool(raw.get("operation_fence_exhausted", False)),
            diagnostic=_optional_text(raw, "diagnostic"),
            session_closed=bool(raw.get("session_closed", False)),
            observation=str(raw.get("observation") or ""),
            updated_at=_required_text(raw, "updated_at"),
        )

    def to_dict(self) -> dict[str, Any]:
        return asdict(self)


@dataclass(frozen=True)
class RuntimeActivityEvent:
    v: int
    id: str
    ts: str
    group_id: str
    actor_id: str
    runtime: str
    activity_id: str
    kind: str
    status: str
    event_type: str
    session_id: str
    turn_id: Optional[str]
    operation_id: Optional[str]
    tool_name: Optional[str]
    duration_ms: Optional[int]

    @classmethod
    def from_dict(cls, raw: Any) -> "RuntimeActivityEvent":
        if not isinstance(raw, dict) or raw.get("v") != ACTIVITY_VERSION:
            raise ValueError("invalid runtime activity version")
        duration = raw.get("duration_ms")
        if duration is not None and (type(duration) is not int or duration < 0):
            raise ValueError("invalid runtime activity duration")
        runtime = validate_runtime(_required_text(raw, "runtime"))
        return cls(
            v=ACTIVITY_VERSION,
            id=_required_text(raw, "id"),
            ts=_required_text(raw, "ts"),
            group_id=_required_text(raw, "group_id"),
            actor_id=_required_text(raw, "actor_id"),
            runtime=runtime,
            activity_id=_required_text(raw, "activity_id"),
            kind=_required_text(raw, "kind"),
            status=_required_text(raw, "status"),
            event_type=_required_text(raw, "event_type"),
            session_id=_required_text(raw, "session_id"),
            turn_id=_optional_text(raw, "turn_id"),
            operation_id=_optional_text(raw, "operation_id"),
            tool_name=_optional_text(raw, "tool_name"),
            duration_ms=duration,
        )

    def to_dict(self) -> dict[str, Any]:
        return asdict(self)
