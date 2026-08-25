from __future__ import annotations

from typing import Any, Callable, Dict, Iterable

from ...kernel.context import CoordinationBrief


def normalize_context_detail(value: Any, *, default: str = "full") -> str:
    detail = str(value or default).strip().lower() or default
    if detail not in {"full", "summary", "overview"}:
        raise ValueError(f"Invalid context detail: {value}")
    return detail


def tasks_version(storage: Any) -> str:
    return f"tasksv:{storage.load_version_state()['tasks_rev']}"


def context_versions(storage: Any) -> tuple[str, str]:
    state = storage.load_version_state()
    return f"ctxv:{state['global_rev']}", f"tasksv:{state['tasks_rev']}"


def normalize_summary_snapshot(snapshot: Dict[str, Any]) -> tuple[Dict[str, Any], bool]:
    """Return a contract-complete result and whether the stored snapshot needs rebuilding."""
    result = snapshot.get("result")
    if not isinstance(result, dict):
        return {}, False
    basis = snapshot.get("basis") if isinstance(snapshot.get("basis"), dict) else {}
    try:
        revision = max(0, int(basis.get("tasks_rev", 0) or 0))
    except (TypeError, ValueError):
        revision = 0
    expected = f"tasksv:{revision}"
    if str(result.get("tasks_version") or "").strip() == expected:
        return result, False
    upgraded = dict(result)
    upgraded["tasks_version"] = expected
    return upgraded, True


def build_overview(
    storage: Any,
    context: Any,
    ordered_agents: Iterable[Any],
    *,
    serialize_brief: Callable[[Any], Dict[str, Any]],
    serialize_note: Callable[[Any], Dict[str, Any]],
    serialize_agent: Callable[[Any], Dict[str, Any]],
) -> Dict[str, Any]:
    return {
        "version": storage.compute_version(),
        "tasks_version": tasks_version(storage),
        "coordination": {
            "brief": serialize_brief(context.coordination.brief),
            "recent_decisions": [
                serialize_note(note) for note in context.coordination.recent_decisions
            ],
            "recent_handoffs": [
                serialize_note(note) for note in context.coordination.recent_handoffs
            ],
        },
        "agent_states": [serialize_agent(agent) for agent in ordered_agents],
        "actors_runtime": [],
        "meta": context.meta if isinstance(context.meta, dict) else {},
    }


def build_empty_summary(
    storage: Any,
    serialize_brief: Callable[[Any], Dict[str, Any]],
    summarize_tasks: Callable[..., Dict[str, Any]],
) -> Dict[str, Any]:
    version, task_version = context_versions(storage)
    return {
        "version": version,
        "tasks_version": task_version,
        "coordination": {
            "brief": serialize_brief(CoordinationBrief()),
            "tasks": [],
        },
        "agent_states": [],
        "actors_runtime": [],
        "attention": {},
        "tasks_summary": summarize_tasks([], attention={}),
        "meta": {},
    }
