"""
Context storage for CCCC groups (v3).

v3 truths:
- agent_states: per-actor short-term working memory (hot + warm)
- coordination: shared control plane (brief + tasks + recent decisions/handoffs)

Secondary views such as attention slices are computed projections, not
editable truths.

Storage: ~/.cccc/groups/<group_id>/context/
"""

from __future__ import annotations

import json
import re
from datetime import datetime, timezone
from enum import Enum
from pathlib import Path
from typing import Any, Dict, List, Optional

import yaml

from .group import Group
from .task_types import normalize_task_type_id, resolve_task_type_id
from ..util.fs import atomic_write_json, read_json


class TaskStatus(str, Enum):
    PLANNED = "planned"
    ACTIVE = "active"
    DONE = "done"
    ARCHIVED = "archived"


class ChecklistStatus(str, Enum):
    PENDING = "pending"
    IN_PROGRESS = "in_progress"
    DONE = "done"


class WaitingOn(str, Enum):
    NONE = "none"
    USER = "user"
    ACTOR = "actor"
    EXTERNAL = "external"


class CoordinationBrief:
    def __init__(
        self,
        objective: str = "",
        current_focus: str = "",
        constraints: Optional[List[str]] = None,
        project_brief: str = "",
        project_brief_stale: bool = False,
        updated_by: str = "",
        updated_at: Optional[str] = None,
    ):
        self.objective = objective
        self.current_focus = current_focus
        self.constraints = constraints or []
        self.project_brief = project_brief
        self.project_brief_stale = bool(project_brief_stale)
        self.updated_by = updated_by
        self.updated_at = updated_at


class CoordinationNote:
    def __init__(
        self,
        at: Optional[str] = None,
        by: str = "",
        summary: str = "",
        task_id: Optional[str] = None,
    ):
        self.at = at or _utc_now_iso()
        self.by = by
        self.summary = summary
        self.task_id = task_id


class Coordination:
    def __init__(
        self,
        brief: Optional[CoordinationBrief] = None,
        recent_decisions: Optional[List[CoordinationNote]] = None,
        recent_handoffs: Optional[List[CoordinationNote]] = None,
    ):
        self.brief = brief or CoordinationBrief()
        self.recent_decisions = recent_decisions or []
        self.recent_handoffs = recent_handoffs or []


class ChecklistItem:
    def __init__(
        self,
        id: str,
        text: str,
        status: ChecklistStatus = ChecklistStatus.PENDING,
    ):
        self.id = id
        self.text = text
        self.status = status


class Task:
    def __init__(
        self,
        id: str,
        title: str,
        outcome: str = "",
        parent_id: Optional[str] = None,
        status: TaskStatus = TaskStatus.PLANNED,
        archived_from: Optional[str] = None,
        assignee: Optional[str] = None,
        priority: str = "",
        blocked_by: Optional[List[str]] = None,
        waiting_on: WaitingOn = WaitingOn.NONE,
        handoff_to: Optional[str] = None,
        task_type: Optional[str] = None,
        client_request_id: Optional[str] = None,
        notes: str = "",
        created_at: Optional[str] = None,
        updated_at: Optional[str] = None,
        checklist: Optional[List[ChecklistItem]] = None,
    ):
        self.id = id
        self.title = title
        self.outcome = outcome
        self.parent_id = parent_id
        self.status = status
        self.archived_from = archived_from
        self.assignee = assignee
        self.priority = priority
        self.blocked_by = blocked_by or []
        self.waiting_on = waiting_on
        self.handoff_to = handoff_to
        self.task_type = resolve_task_type_id(task_type, parent_id)
        self.client_request_id = client_request_id
        self.notes = notes
        self.created_at = created_at or _utc_now_iso()
        self.updated_at = updated_at
        self.checklist = checklist or []

    @property
    def is_root(self) -> bool:
        return self.parent_id is None

    @property
    def current_checklist_item(self) -> Optional[ChecklistItem]:
        for item in self.checklist:
            if item.status != ChecklistStatus.DONE:
                return item
        return None

    @property
    def progress(self) -> float:
        if not self.checklist:
            return 0.0
        done_count = sum(1 for item in self.checklist if item.status == ChecklistStatus.DONE)
        return done_count / len(self.checklist)


class AgentStateHot:
    def __init__(
        self,
        active_task_id: Optional[str] = None,
        focus: str = "",
        next_action: str = "",
        blockers: Optional[List[str]] = None,
    ):
        self.active_task_id = active_task_id
        self.focus = focus
        self.next_action = next_action
        self.blockers = blockers or []


class AgentStateWarm:
    def __init__(
        self,
        what_changed: str = "",
        open_loops: Optional[List[str]] = None,
        commitments: Optional[List[str]] = None,
        environment_summary: str = "",
        user_model: str = "",
        persona_notes: str = "",
    ):
        self.what_changed = what_changed
        self.open_loops = open_loops or []
        self.commitments = commitments or []
        self.environment_summary = environment_summary
        self.user_model = user_model
        self.persona_notes = persona_notes


class AgentState:
    def __init__(
        self,
        id: str,
        hot: Optional[AgentStateHot] = None,
        warm: Optional[AgentStateWarm] = None,
        updated_at: Optional[str] = None,
    ):
        self.id = id
        self.hot = hot or AgentStateHot()
        self.warm = warm or AgentStateWarm()
        self.updated_at = updated_at or _utc_now_iso()


class Context:
    def __init__(
        self,
        coordination: Optional[Coordination] = None,
        meta: Optional[Dict[str, Any]] = None,
    ):
        self.coordination = coordination or Coordination()
        self.meta = meta or {}


class AgentsData:
    def __init__(self, agents: Optional[List[AgentState]] = None):
        self.agents = agents or []


def _utc_now_iso() -> str:
    return datetime.now(timezone.utc).isoformat().replace("+00:00", "Z")


def _coerce_task_status(value: Any) -> TaskStatus:
    raw = str(value or "planned").strip().lower()
    try:
        return TaskStatus(raw)
    except Exception:
        return TaskStatus.PLANNED


def _coerce_checklist_status(value: Any) -> ChecklistStatus:
    raw = str(value or "pending").strip().lower()
    try:
        return ChecklistStatus(raw)
    except Exception:
        return ChecklistStatus.PENDING


def _coerce_waiting_on(value: Any) -> WaitingOn:
    raw = str(value or "none").strip().lower()
    try:
        return WaitingOn(raw)
    except Exception:
        return WaitingOn.NONE


def normalize_task_type(value: Any) -> Optional[str]:
    return normalize_task_type_id(value)


class ContextStorage:
    """Context storage for a single group."""

    def __init__(self, group: Group):
        self.group = group
        self.context_dir = group.path / "context"
        self.tasks_dir = self.context_dir / "tasks"
        self._context_raw: Optional[Dict[str, Any]] = None
        self._tasks_raw: Dict[str, Dict[str, Any]] = {}
        self._agents_raw: Optional[Dict[str, Any]] = None

    def _ensure_dirs(self) -> None:
        self.context_dir.mkdir(parents=True, exist_ok=True)
        self.tasks_dir.mkdir(parents=True, exist_ok=True)

    def _context_path(self) -> Path:
        return self.context_dir / "context.yaml"

    def _task_path(self, task_id: str) -> Path:
        return self.tasks_dir / f"{task_id}.yaml"

    def _agents_path(self) -> Path:
        return self.context_dir / "agents.yaml"

    def _version_state_path(self) -> Path:
        return self.context_dir / "version_state.json"

    def _summary_snapshot_path(self) -> Path:
        return self.context_dir / "summary_snapshot.json"

    def _default_version_state(self) -> Dict[str, int]:
        has_context = self._context_path().exists()
        has_agents = self._agents_path().exists()
        has_tasks = self.tasks_dir.exists() and any(self.tasks_dir.glob("T*.yaml"))
        # Bootstrap existing groups into a non-empty baseline so versioning starts stable.
        initial_rev = 1 if (has_context or has_agents or has_tasks) else 0
        return {
            "global_rev": initial_rev,
            "context_rev": 1 if has_context else 0,
            "tasks_rev": 1 if has_tasks else 0,
            "agents_rev": 1 if has_agents else 0,
            "actors_rev": 0,
        }

    def load_version_state(self) -> Dict[str, int]:
        path = self._version_state_path()
        raw = read_json(path)
        if not raw:
            return self._default_version_state()

        def _coerce_rev(key: str, fallback: int) -> int:
            try:
                value = int(raw.get(key, fallback))
            except Exception:
                value = fallback
            return max(0, value)

        fallback = self._default_version_state()
        return {
            "global_rev": _coerce_rev("global_rev", fallback["global_rev"]),
            "context_rev": _coerce_rev("context_rev", fallback["context_rev"]),
            "tasks_rev": _coerce_rev("tasks_rev", fallback["tasks_rev"]),
            "agents_rev": _coerce_rev("agents_rev", fallback["agents_rev"]),
            "actors_rev": _coerce_rev("actors_rev", fallback["actors_rev"]),
        }

    def save_version_state(self, state: Dict[str, Any]) -> Dict[str, int]:
        self._ensure_dirs()
        current = self.load_version_state()
        next_state = {
            "global_rev": max(0, int(state.get("global_rev", current["global_rev"]))),
            "context_rev": max(0, int(state.get("context_rev", current["context_rev"]))),
            "tasks_rev": max(0, int(state.get("tasks_rev", current["tasks_rev"]))),
            "agents_rev": max(0, int(state.get("agents_rev", current["agents_rev"]))),
            "actors_rev": max(0, int(state.get("actors_rev", current["actors_rev"]))),
        }
        atomic_write_json(self._version_state_path(), next_state, indent=2)
        return next_state

    def bump_version_state(
        self,
        *,
        context_changed: bool = False,
        tasks_changed: bool = False,
        agents_changed: bool = False,
        actors_changed: bool = False,
    ) -> Dict[str, int]:
        state = self.load_version_state()
        if not any((context_changed, tasks_changed, agents_changed, actors_changed)):
            return state
        if context_changed:
            state["context_rev"] += 1
        if tasks_changed:
            state["tasks_rev"] += 1
        if agents_changed:
            state["agents_rev"] += 1
        if actors_changed:
            state["actors_rev"] += 1
        state["global_rev"] += 1
        return self.save_version_state(state)

    def summary_basis(self) -> Dict[str, int]:
        state = self.load_version_state()
        return {
            "context_rev": state["context_rev"],
            "tasks_rev": state["tasks_rev"],
            "agents_rev": state["agents_rev"],
            "actors_rev": state["actors_rev"],
        }

    def load_summary_snapshot(self) -> Dict[str, Any]:
        path = self._summary_snapshot_path()
        raw = read_json(path)
        if not isinstance(raw, dict):
            return {}
        basis = raw.get("basis")
        result = raw.get("result")
        if not isinstance(basis, dict) or not isinstance(result, dict):
            return {}
        return {
            "schema": int(raw.get("schema", 1) or 1),
            "basis": {
                "context_rev": max(0, int(basis.get("context_rev", 0) or 0)),
                "tasks_rev": max(0, int(basis.get("tasks_rev", 0) or 0)),
                "agents_rev": max(0, int(basis.get("agents_rev", 0) or 0)),
                "actors_rev": max(0, int(basis.get("actors_rev", 0) or 0)),
            },
            "version": str(raw.get("version") or "").strip(),
            "result": result,
            "built_at": str(raw.get("built_at") or "").strip(),
        }

    def save_summary_snapshot(self, *, basis: Dict[str, Any], version: str, result: Dict[str, Any]) -> None:
        self._ensure_dirs()
        snapshot = {
            "schema": 1,
            "basis": {
                "context_rev": max(0, int(basis.get("context_rev", 0) or 0)),
                "tasks_rev": max(0, int(basis.get("tasks_rev", 0) or 0)),
                "agents_rev": max(0, int(basis.get("agents_rev", 0) or 0)),
                "actors_rev": max(0, int(basis.get("actors_rev", 0) or 0)),
            },
            "version": str(version or "").strip(),
            "result": result,
            "built_at": _utc_now_iso(),
        }
        atomic_write_json(self._summary_snapshot_path(), snapshot, indent=2)

    def compute_version(self) -> str:
        return f"ctxv:{self.load_version_state()['global_rev']}"

    def _default_meta(self) -> Dict[str, Any]:
        return {
            "contract": {
                "agent_states": "Per-actor working memory. hot=current execution, warm=recovery digest.",
                "coordination": "Shared control plane. brief=current objective/focus; tasks=dispatch truth.",
                "projections": "Read-only derived views such as board and attention.",
            }
        }

    def load_context(self) -> Context:
        path = self._context_path()
        if not path.exists():
            return Context(meta=self._default_meta())
        try:
            data = yaml.safe_load(path.read_text(encoding="utf-8"))
            if data is None:
                return Context(meta=self._default_meta())
            self._context_raw = data

            coordination_raw = data.get("coordination") if isinstance(data.get("coordination"), dict) else {}
            brief_raw = coordination_raw.get("brief") if isinstance(coordination_raw.get("brief"), dict) else {}
            brief = CoordinationBrief(
                objective=str(brief_raw.get("objective") or ""),
                current_focus=str(brief_raw.get("current_focus") or ""),
                constraints=list(brief_raw.get("constraints") or []) if isinstance(brief_raw.get("constraints"), list) else [],
                project_brief=str(brief_raw.get("project_brief") or ""),
                project_brief_stale=bool(brief_raw.get("project_brief_stale")),
                updated_by=str(brief_raw.get("updated_by") or ""),
                updated_at=str(brief_raw.get("updated_at") or "") or None,
            )

            def _parse_notes(raw_items: Any) -> List[CoordinationNote]:
                notes: List[CoordinationNote] = []
                if not isinstance(raw_items, list):
                    return notes
                for raw in raw_items:
                    if not isinstance(raw, dict):
                        continue
                    summary = str(raw.get("summary") or "").strip()
                    if not summary:
                        continue
                    notes.append(
                        CoordinationNote(
                            at=str(raw.get("at") or "") or None,
                            by=str(raw.get("by") or ""),
                            summary=summary,
                            task_id=str(raw.get("task_id") or "") or None,
                        )
                    )
                return notes

            coordination = Coordination(
                brief=brief,
                recent_decisions=_parse_notes(coordination_raw.get("recent_decisions")),
                recent_handoffs=_parse_notes(coordination_raw.get("recent_handoffs")),
            )
            meta = data.get("meta") if isinstance(data.get("meta"), dict) and data.get("meta") else self._default_meta()
            return Context(coordination=coordination, meta=meta)
        except Exception:
            return Context(meta=self._default_meta())

    def save_context(self, context: Context) -> None:
        self._ensure_dirs()
        brief = context.coordination.brief if context.coordination else CoordinationBrief()
        data: Dict[str, Any] = {
            "coordination": {
                "brief": {
                    "objective": brief.objective,
                    "current_focus": brief.current_focus,
                    "constraints": brief.constraints,
                    "project_brief": brief.project_brief,
                    "project_brief_stale": bool(brief.project_brief_stale),
                    "updated_by": brief.updated_by,
                    "updated_at": brief.updated_at,
                },
                "recent_decisions": [
                    {
                        "at": note.at,
                        "by": note.by,
                        "summary": note.summary,
                        "task_id": note.task_id,
                    }
                    for note in (context.coordination.recent_decisions if context.coordination else [])
                    if str(note.summary or "").strip()
                ],
                "recent_handoffs": [
                    {
                        "at": note.at,
                        "by": note.by,
                        "summary": note.summary,
                        "task_id": note.task_id,
                    }
                    for note in (context.coordination.recent_handoffs if context.coordination else [])
                    if str(note.summary or "").strip()
                ],
            },
            "meta": context.meta if isinstance(context.meta, dict) and context.meta else self._default_meta(),
        }
        path = self._context_path()
        path.write_text(
            yaml.safe_dump(data, allow_unicode=True, sort_keys=False, default_flow_style=False),
            encoding="utf-8",
        )

    def load_task(self, task_id: str) -> Optional[Task]:
        path = self._task_path(task_id)
        if path.exists():
            return self._parse_task(path)
        return None

    def _parse_task(self, path: Path) -> Optional[Task]:
        try:
            data = yaml.safe_load(path.read_text(encoding="utf-8"))
            if data is None:
                return None
            self._tasks_raw[path.name] = data
            checklist: List[ChecklistItem] = []
            checklist_raw = data.get("checklist")
            if not isinstance(checklist_raw, list):
                checklist_raw = []
            for raw in checklist_raw:
                if not isinstance(raw, dict):
                    continue
                checklist.append(
                    ChecklistItem(
                        id=str(raw.get("id") or ""),
                        text=str(raw.get("text") or ""),
                        status=_coerce_checklist_status(raw.get("status")),
                    )
                )
            return Task(
                id=str(data.get("id") or ""),
                title=str(data.get("title") or ""),
                outcome=str(data.get("outcome") or ""),
                parent_id=str(data.get("parent_id") or "") or None,
                status=_coerce_task_status(data.get("status")),
                archived_from=str(data.get("archived_from") or "") or None,
                assignee=str(data.get("assignee") or "") or None,
                priority=str(data.get("priority") or ""),
                blocked_by=list(data.get("blocked_by") or []) if isinstance(data.get("blocked_by"), list) else [],
                waiting_on=_coerce_waiting_on(data.get("waiting_on")),
                handoff_to=str(data.get("handoff_to") or "") or None,
                task_type=normalize_task_type(data.get("task_type")),
                client_request_id=str(data.get("client_request_id") or "") or None,
                notes=str(data.get("notes") or ""),
                created_at=str(data.get("created_at") or "") or _utc_now_iso(),
                updated_at=str(data.get("updated_at") or "") or None,
                checklist=checklist,
            )
        except Exception:
            return None

    def save_task(self, task: Task) -> None:
        self._ensure_dirs()
        data = {
            "id": task.id,
            "title": task.title,
            "outcome": task.outcome,
            "parent_id": task.parent_id,
            "status": task.status.value if isinstance(task.status, TaskStatus) else str(task.status),
            "archived_from": task.archived_from,
            "assignee": task.assignee,
            "priority": task.priority,
            "blocked_by": task.blocked_by,
            "waiting_on": task.waiting_on.value if isinstance(task.waiting_on, WaitingOn) else str(task.waiting_on),
            "handoff_to": task.handoff_to,
            "task_type": normalize_task_type(task.task_type),
            "client_request_id": task.client_request_id,
            "notes": task.notes,
            "created_at": task.created_at,
            "updated_at": task.updated_at,
            "checklist": [
                {
                    "id": item.id,
                    "text": item.text,
                    "status": item.status.value if isinstance(item.status, ChecklistStatus) else str(item.status),
                }
                for item in task.checklist
            ],
        }
        data = {k: v for k, v in data.items() if v is not None and v != []}
        path = self._task_path(task.id)
        path.write_text(
            yaml.safe_dump(data, allow_unicode=True, sort_keys=False, default_flow_style=False),
            encoding="utf-8",
        )

    def delete_task(self, task_id: str) -> bool:
        path = self._task_path(task_id)
        self._tasks_raw.pop(path.name, None)
        if not path.exists():
            return False
        path.unlink()
        return True

    def list_tasks(self) -> List[Task]:
        tasks: List[Task] = []
        if self.tasks_dir.exists():
            for path in self.tasks_dir.glob("T*.yaml"):
                task = self._parse_task(path)
                if task:
                    tasks.append(task)
        tasks.sort(key=lambda task: task.id)
        return tasks

    def generate_task_id(self) -> str:
        max_num = 0
        if self.tasks_dir.exists():
            for path in self.tasks_dir.glob("T*.yaml"):
                match = re.match(r"T(\d+)", path.stem)
                if match:
                    max_num = max(max_num, int(match.group(1)))
        return f"T{max_num + 1:03d}"

    def get_task_children(self, task_id: str, tasks: Optional[List[Task]] = None) -> List[Task]:
        if tasks is None:
            tasks = self.list_tasks()
        return [task for task in tasks if task.parent_id == task_id]

    def detect_cycle(self, task_id: str, new_parent_id: Optional[str], tasks: Optional[List[Task]] = None) -> bool:
        if new_parent_id is None:
            return False
        if new_parent_id == task_id:
            return True
        if tasks is None:
            tasks = self.list_tasks()
        tasks_by_id = {task.id: task for task in tasks}
        visited = set()
        current = new_parent_id
        while current is not None:
            if current == task_id:
                return True
            if current in visited:
                return True
            visited.add(current)
            parent_task = tasks_by_id.get(current)
            if parent_task is None:
                break
            current = parent_task.parent_id
        return False

    def load_agents(self) -> AgentsData:
        path = self._agents_path()
        if not path.exists():
            return AgentsData()
        try:
            data = yaml.safe_load(path.read_text(encoding="utf-8"))
            if data is None:
                return AgentsData()
            self._agents_raw = data
            agents: List[AgentState] = []
            agents_raw = data.get("agent_states")
            if not isinstance(agents_raw, list):
                agents_raw = []
            for raw in agents_raw:
                if not isinstance(raw, dict):
                    continue
                aid = str(raw.get("actor_id") or raw.get("id") or "").strip()
                if not aid:
                    continue
                hot_raw = raw.get("hot") if isinstance(raw.get("hot"), dict) else {}
                warm_raw = raw.get("warm") if isinstance(raw.get("warm"), dict) else {}
                agents.append(
                    AgentState(
                        id=aid,
                        hot=AgentStateHot(
                            active_task_id=str(hot_raw.get("active_task_id") or "") or None,
                            focus=str(hot_raw.get("focus") or ""),
                            next_action=str(hot_raw.get("next_action") or ""),
                            blockers=list(hot_raw.get("blockers") or []) if isinstance(hot_raw.get("blockers"), list) else [],
                        ),
                        warm=AgentStateWarm(
                            what_changed=str(warm_raw.get("what_changed") or ""),
                            open_loops=list(warm_raw.get("open_loops") or []) if isinstance(warm_raw.get("open_loops"), list) else [],
                            commitments=list(warm_raw.get("commitments") or []) if isinstance(warm_raw.get("commitments"), list) else [],
                            environment_summary=str(warm_raw.get("environment_summary") or ""),
                            user_model=str(warm_raw.get("user_model") or ""),
                            persona_notes=str(warm_raw.get("persona_notes") or ""),
                        ),
                        updated_at=str(raw.get("updated_at") or "") or None,
                    )
                )
            return AgentsData(agents=agents)
        except Exception:
            return AgentsData()

    def save_agents(self, agents_state: AgentsData) -> None:
        self._ensure_dirs()
        payload = {
            "agent_states": [
                {
                    "actor_id": agent.id,
                    "hot": {
                        "active_task_id": agent.hot.active_task_id,
                        "focus": agent.hot.focus,
                        "next_action": agent.hot.next_action,
                        "blockers": agent.hot.blockers,
                    },
                    "warm": {
                        "what_changed": agent.warm.what_changed,
                        "open_loops": agent.warm.open_loops,
                        "commitments": agent.warm.commitments,
                        "environment_summary": agent.warm.environment_summary,
                        "user_model": agent.warm.user_model,
                        "persona_notes": agent.warm.persona_notes,
                    },
                    "updated_at": agent.updated_at,
                }
                for agent in agents_state.agents
            ]
        }
        path = self._agents_path()
        path.write_text(
            yaml.safe_dump(payload, allow_unicode=True, sort_keys=False, default_flow_style=False),
            encoding="utf-8",
        )

    def _canonicalize_agent_id(self, agent_id: str) -> str:
        value = str(agent_id or "").strip()
        if not value:
            return ""
        value = re.sub(r"(?<=[a-z0-9])([A-Z])", r"-\1", value)
        value = value.replace("_", "-").replace(" ", "-")
        value = re.sub(r"-{2,}", "-", value)
        return value.strip("-").lower()

    def _get_or_create_agent(self, agents_state: AgentsData, agent_id: str) -> AgentState:
        canonical_id = self._canonicalize_agent_id(agent_id)
        if not canonical_id:
            raise ValueError("agent_id must be a non-empty string")
        for agent in agents_state.agents:
            if agent.id == canonical_id:
                return agent
        agent = AgentState(id=canonical_id)
        agents_state.agents.append(agent)
        return agent

    def update_agent_state(self, agent_id: str, status: str, active_task_id: Optional[str] = None) -> AgentState:
        canonical_id = self._canonicalize_agent_id(agent_id)
        if not canonical_id:
            raise ValueError("agent_id must be a non-empty string")
        agents_state = self.load_agents()
        agent = self._get_or_create_agent(agents_state, canonical_id)
        agent.hot.focus = re.sub(r"\s+", " ", str(status or "")).strip()
        agent.hot.active_task_id = str(active_task_id or "").strip() or None
        agent.updated_at = _utc_now_iso()
        self.save_agents(agents_state)
        return agent

    def clear_agent_state(self, agent_id: str) -> AgentState:
        canonical_id = self._canonicalize_agent_id(agent_id)
        if not canonical_id:
            raise ValueError("agent_id must be a non-empty string")
        agents_state = self.load_agents()
        agent = self._get_or_create_agent(agents_state, canonical_id)
        agent.hot = AgentStateHot()
        agent.warm = AgentStateWarm()
        agent.updated_at = _utc_now_iso()
        self.save_agents(agents_state)
        return agent

    def clear_agent_status(self, agent_id: str) -> AgentState:
        return self.clear_agent_state(agent_id=agent_id)

    def clear_agent_status_if_present(self, agent_id: str) -> bool:
        canonical_id = self._canonicalize_agent_id(agent_id)
        if not canonical_id:
            return False
        agents_state = self.load_agents()
        for agent in agents_state.agents:
            if agent.id != canonical_id:
                continue
            agent.hot.active_task_id = None
            agent.hot.focus = ""
            agent.hot.next_action = ""
            agent.hot.blockers = []
            agent.warm.what_changed = ""
            agent.updated_at = _utc_now_iso()
            self.save_agents(agents_state)
            self.bump_version_state(agents_changed=True)
            return True
        return False

    def delete_agent_state(self, agent_id: str) -> bool:
        canonical_id = self._canonicalize_agent_id(agent_id)
        if not canonical_id:
            return False
        agents_state = self.load_agents()
        before = len(agents_state.agents)
        agents_state.agents = [agent for agent in agents_state.agents if agent.id != canonical_id]
        if len(agents_state.agents) == before:
            return False
        self.save_agents(agents_state)
        self.bump_version_state(agents_changed=True)
        return True
