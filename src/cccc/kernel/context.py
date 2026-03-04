"""
Context storage for CCCC groups (v2).

Context v2 core fields:
- Vision: project north star
- Overview: manual strategic coordination fields only
- Panorama: daemon-computed read-only projection
- Tasks: multi-level tree via parent_id (root tasks = phases/stages)
- Agents: flat per-agent working memory (short-term memory)

Storage: ~/.cccc/groups/<group_id>/context/
"""

from __future__ import annotations

import hashlib
import json
import re
from datetime import datetime, timezone
from enum import Enum
from pathlib import Path
from typing import Any, Dict, List, Optional, Tuple

import yaml

from .group import Group


# =============================================================================
# Enums
# =============================================================================


class TaskStatus(str, Enum):
    PLANNED = "planned"
    ACTIVE = "active"
    DONE = "done"
    ARCHIVED = "archived"


class StepStatus(str, Enum):
    PENDING = "pending"
    IN_PROGRESS = "in_progress"
    DONE = "done"


# =============================================================================
# Data Classes
# =============================================================================


class OverviewManual:
    def __init__(
        self,
        roles: Optional[List[str]] = None,
        collaboration_mode: str = "",
        current_focus: str = "",
        updated_by: str = "",
        updated_at: Optional[str] = None,
    ):
        self.roles = roles or []
        self.collaboration_mode = collaboration_mode
        self.current_focus = current_focus
        self.updated_by = updated_by
        self.updated_at = updated_at


class Overview:
    def __init__(self, manual: Optional[OverviewManual] = None):
        self.manual = manual or OverviewManual()


class Step:
    def __init__(
        self,
        id: str,
        name: str,
        acceptance: str = "",
        status: StepStatus = StepStatus.PENDING,
    ):
        self.id = id
        self.name = name
        self.acceptance = acceptance
        self.status = status


class Task:
    def __init__(
        self,
        id: str,
        name: str,
        goal: str = "",
        parent_id: Optional[str] = None,
        status: TaskStatus = TaskStatus.PLANNED,
        archived_from: Optional[str] = None,
        assignee: Optional[str] = None,
        created_at: Optional[str] = None,
        updated_at: Optional[str] = None,
        steps: Optional[List[Step]] = None,
    ):
        self.id = id
        self.name = name
        self.goal = goal
        self.parent_id = parent_id
        self.status = status
        self.archived_from = archived_from
        self.assignee = assignee
        self.created_at = created_at or _utc_now_iso()
        self.updated_at = updated_at
        self.steps = steps or []

    @property
    def is_root(self) -> bool:
        return self.parent_id is None

    @property
    def current_step(self) -> Optional[Step]:
        for step in self.steps:
            if step.status != StepStatus.DONE:
                return step
        return None

    @property
    def progress(self) -> float:
        if not self.steps:
            return 0.0
        done_count = sum(1 for s in self.steps if s.status == StepStatus.DONE)
        return done_count / len(self.steps)


class AgentState:
    """Flat agent working memory (short-term memory).

    Replaces the former AgentRuntime + AgentCapsule split.
    All fields are flat — no nested sub-objects.
    """

    def __init__(
        self,
        id: str,
        active_task_id: Optional[str] = None,
        focus: str = "",
        blockers: Optional[List[str]] = None,
        next_action: str = "",
        what_changed: str = "",
        decision_delta: str = "",
        environment: str = "",
        user_profile: str = "",
        notes: str = "",
        updated_at: Optional[str] = None,
    ):
        self.id = id
        # Task state
        self.active_task_id = active_task_id
        self.focus = focus
        self.blockers = blockers or []
        self.next_action = next_action
        # Episodic buffer
        self.what_changed = what_changed
        self.decision_delta = decision_delta
        # World model
        self.environment = environment
        self.user_profile = user_profile
        # Lessons & notes
        self.notes = notes
        self.updated_at = updated_at or _utc_now_iso()

class Context:
    def __init__(
        self,
        vision: Optional[str] = None,
        overview: Optional[Overview] = None,
        meta: Optional[Dict[str, Any]] = None,
    ):
        self.vision = vision
        self.overview = overview or Overview()
        self.meta = meta or {}


class AgentsData:
    def __init__(
        self,
        agents: Optional[List[AgentState]] = None,
    ):
        self.agents = agents or []


# =============================================================================
# Helpers
# =============================================================================


def _utc_now_iso() -> str:
    return datetime.now(timezone.utc).isoformat().replace("+00:00", "Z")


def _coerce_task_status(value: Any) -> TaskStatus:
    raw = str(value or "planned").strip().lower()
    try:
        return TaskStatus(raw)
    except Exception:
        return TaskStatus.PLANNED


def _coerce_step_status(value: Any) -> StepStatus:
    raw = str(value or "pending").strip().lower()
    try:
        return StepStatus(raw)
    except Exception:
        return StepStatus.PENDING


def _coerce_int(value: Any, default: int) -> int:
    try:
        return int(value)
    except Exception:
        return default


# =============================================================================
# Context Storage
# =============================================================================


class ContextStorage:
    """Context storage for a single group."""

    def __init__(self, group: Group):
        self.group = group
        self.context_dir = group.path / "context"
        self.tasks_dir = self.context_dir / "tasks"
        # Cache for raw data to avoid re-reading files in compute_version()
        self._context_raw: Optional[Dict[str, Any]] = None
        self._tasks_raw: Dict[str, Dict[str, Any]] = {}

    def _ensure_dirs(self) -> None:
        self.context_dir.mkdir(parents=True, exist_ok=True)
        self.tasks_dir.mkdir(parents=True, exist_ok=True)

    def _context_path(self) -> Path:
        return self.context_dir / "context.yaml"

    def _task_path(self, task_id: str) -> Path:
        return self.tasks_dir / f"{task_id}.yaml"

    def _agents_path(self) -> Path:
        return self.context_dir / "agents.yaml"

    # =========================================================================
    # Version Computation
    # =========================================================================

    def compute_version(self) -> str:
        """Compute a version hash for the current context state."""

        def _jsonable(obj: Any) -> Any:
            if obj is None or isinstance(obj, (str, int, float, bool)):
                return obj
            if isinstance(obj, datetime):
                return obj.isoformat()
            if isinstance(obj, list):
                return [_jsonable(x) for x in obj]
            if isinstance(obj, dict):
                return {str(k): _jsonable(v) for k, v in obj.items()}
            return str(obj)

        h = hashlib.sha256()

        # Use cached context data if available, otherwise read from file
        ctx_path = self._context_path()
        if self._context_raw is not None:
            data = self._context_raw
            payload = json.dumps(_jsonable(data), sort_keys=True).encode()
            h.update(payload)
        elif ctx_path.exists():
            try:
                data = yaml.safe_load(ctx_path.read_text(encoding="utf-8"))
            except Exception:
                data = None
            payload = json.dumps(_jsonable(data), sort_keys=True).encode()
            h.update(payload)

        # Use cached task data if available, otherwise read from file
        if self.tasks_dir.exists():
            for task_file in sorted(self.tasks_dir.glob("T*.yaml")):
                h.update(task_file.name.encode("utf-8"))
                if task_file.name in self._tasks_raw:
                    data = self._tasks_raw[task_file.name]
                else:
                    try:
                        data = yaml.safe_load(task_file.read_text(encoding="utf-8"))
                    except Exception:
                        data = None
                payload = json.dumps(_jsonable(data), sort_keys=True).encode()
                h.update(payload)

        return h.hexdigest()[:12]

    # =========================================================================
    # Context Operations
    # =========================================================================

    def _default_meta(self) -> Dict[str, Any]:
        return {
            "contract": {
                "vision": "One-sentence north star; update rarely.",
                "overview": "Structured project view. manual=human-maintained.",
                "panorama": "Read-only projection computed by daemon from tasks + agents.",
                "tasks": "Multi-level task tree. Root tasks = phases/stages. Child tasks = execution.",
                "agents": "Flat per-agent short-term working memory.",
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

            # Cache raw data for compute_version()
            self._context_raw = data

            vision = data.get("vision")

            # Parse overview.manual
            overview_raw = data.get("overview")
            manual = OverviewManual()
            if isinstance(overview_raw, dict):
                manual_raw = overview_raw.get("manual")
                if isinstance(manual_raw, dict):
                    roles_raw = manual_raw.get("roles")
                    manual = OverviewManual(
                        roles=list(roles_raw) if isinstance(roles_raw, list) else [],
                        collaboration_mode=str(manual_raw.get("collaboration_mode") or ""),
                        current_focus=str(manual_raw.get("current_focus") or ""),
                        updated_by=str(manual_raw.get("updated_by") or ""),
                        updated_at=manual_raw.get("updated_at"),
                    )
            overview = Overview(manual=manual)

            meta = data.get("meta")
            if not isinstance(meta, dict) or not meta:
                meta = self._default_meta()

            return Context(
                vision=vision,
                overview=overview,
                meta=meta,
            )
        except Exception:
            return Context(meta=self._default_meta())

    def save_context(self, context: Context) -> None:
        self._ensure_dirs()

        data: Dict[str, Any] = {}
        if context.vision is not None:
            data["vision"] = context.vision

        # Serialize overview.manual
        manual = context.overview.manual if context.overview else OverviewManual()
        manual_data: Dict[str, Any] = {}
        if manual.roles:
            manual_data["roles"] = manual.roles
        if manual.collaboration_mode:
            manual_data["collaboration_mode"] = manual.collaboration_mode
        if manual.current_focus:
            manual_data["current_focus"] = manual.current_focus
        if manual.updated_by:
            manual_data["updated_by"] = manual.updated_by
        if manual.updated_at:
            manual_data["updated_at"] = manual.updated_at
        if manual_data:
            data["overview"] = {"manual": manual_data}

        meta = context.meta if isinstance(context.meta, dict) else {}
        if not meta:
            meta = self._default_meta()
        data["meta"] = meta

        path = self._context_path()
        path.write_text(
            yaml.safe_dump(data, allow_unicode=True, sort_keys=False, default_flow_style=False),
            encoding="utf-8",
        )

    # =========================================================================
    # Task Operations
    # =========================================================================

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

            # Cache raw data for compute_version()
            self._tasks_raw[path.name] = data

            steps = []
            steps_raw = data.get("steps")
            if not isinstance(steps_raw, list):
                steps_raw = []
            for s in steps_raw:
                if not isinstance(s, dict):
                    continue
                steps.append(
                    Step(
                        id=s.get("id", ""),
                        name=s.get("name", ""),
                        acceptance=s.get("acceptance", ""),
                        status=_coerce_step_status(s.get("status")),
                    )
                )

            task_status = _coerce_task_status(data.get("status", "planned"))

            parent_id = data.get("parent_id")

            return Task(
                id=data.get("id", ""),
                name=data.get("name", ""),
                goal=data.get("goal", ""),
                parent_id=parent_id,
                status=task_status,
                archived_from=data.get("archived_from"),
                assignee=data.get("assignee"),
                created_at=data.get("created_at", ""),
                updated_at=data.get("updated_at"),
                steps=steps,
            )
        except Exception:
            return None

    def save_task(self, task: Task) -> None:
        self._ensure_dirs()

        data = {
            "id": task.id,
            "name": task.name,
            "goal": task.goal,
            "parent_id": task.parent_id,
            "status": task.status.value if isinstance(task.status, TaskStatus) else task.status,
            "archived_from": getattr(task, "archived_from", None),
            "assignee": task.assignee,
            "created_at": task.created_at,
            "updated_at": task.updated_at,
            "steps": [
                {
                    "id": s.id,
                    "name": s.name,
                    "acceptance": s.acceptance,
                    "status": s.status.value if isinstance(s.status, StepStatus) else s.status,
                }
                for s in task.steps
            ],
        }
        data = {k: v for k, v in data.items() if v is not None}

        path = self._task_path(task.id)
        path.write_text(
            yaml.safe_dump(data, allow_unicode=True, sort_keys=False, default_flow_style=False),
            encoding="utf-8",
        )

    def list_tasks(self) -> List[Task]:
        tasks = []
        if self.tasks_dir.exists():
            for path in self.tasks_dir.glob("T*.yaml"):
                task = self._parse_task(path)
                if task:
                    tasks.append(task)
        tasks.sort(key=lambda t: t.id)
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
        """Get direct children of a task."""
        if tasks is None:
            tasks = self.list_tasks()
        return [t for t in tasks if t.parent_id == task_id]

    def detect_cycle(self, task_id: str, new_parent_id: Optional[str], tasks: Optional[List[Task]] = None) -> bool:
        """Detect if moving task_id under new_parent_id would create a cycle.

        Returns True if a cycle would be created.
        """
        if new_parent_id is None:
            return False
        if new_parent_id == task_id:
            return True

        if tasks is None:
            tasks = self.list_tasks()
        tasks_by_id = {t.id: t for t in tasks}

        # Traverse from new_parent_id up to root; if we hit task_id, it's a cycle
        visited = set()
        current = new_parent_id
        while current is not None:
            if current == task_id:
                return True
            if current in visited:
                return True  # pre-existing cycle
            visited.add(current)
            parent_task = tasks_by_id.get(current)
            if parent_task is None:
                break
            current = parent_task.parent_id
        return False

    # =========================================================================
    # Agent-State Operations
    # =========================================================================

    def load_agents(self) -> AgentsData:
        path = self._agents_path()
        if not path.exists():
            return AgentsData()

        try:
            data = yaml.safe_load(path.read_text(encoding="utf-8"))
            if data is None:
                return AgentsData()

            agents: List[AgentState] = []
            agents_raw = data.get("agents")
            if not isinstance(agents_raw, list):
                agents_raw = []
            for a in agents_raw:
                if not isinstance(a, dict):
                    continue

                aid = str(a.get("id") or "").strip()
                if not aid:
                    continue
                blockers_raw = a.get("blockers")
                agents.append(AgentState(
                    id=aid,
                    active_task_id=str(a.get("active_task_id") or "").strip() or None,
                    focus=str(a.get("focus") or ""),
                    blockers=list(blockers_raw) if isinstance(blockers_raw, list) else [],
                    next_action=str(a.get("next_action") or ""),
                    what_changed=str(a.get("what_changed") or ""),
                    decision_delta=str(a.get("decision_delta") or ""),
                    environment=str(a.get("environment") or ""),
                    user_profile=str(a.get("user_profile") or ""),
                    notes=str(a.get("notes") or ""),
                    updated_at=str(a.get("updated_at") or "") or None,
                ))

            return AgentsData(agents=agents)
        except Exception:
            return AgentsData()

    def save_agents(self, agents_state: AgentsData) -> None:
        self._ensure_dirs()

        agents_data = []
        for a in agents_state.agents:
            entry: Dict[str, Any] = {"id": a.id}
            if a.active_task_id:
                entry["active_task_id"] = a.active_task_id
            if a.focus:
                entry["focus"] = a.focus
            if a.blockers:
                entry["blockers"] = a.blockers
            if a.next_action:
                entry["next_action"] = a.next_action
            if a.what_changed:
                entry["what_changed"] = a.what_changed
            if a.decision_delta:
                entry["decision_delta"] = a.decision_delta
            if a.environment:
                entry["environment"] = a.environment
            if a.user_profile:
                entry["user_profile"] = a.user_profile
            if a.notes:
                entry["notes"] = a.notes
            if a.updated_at:
                entry["updated_at"] = a.updated_at
            agents_data.append(entry)

        data: Dict[str, Any] = {"agents": agents_data}

        path = self._agents_path()
        path.write_text(
            yaml.safe_dump(data, allow_unicode=True, sort_keys=False, default_flow_style=False),
            encoding="utf-8",
        )

    def _canonicalize_agent_id(self, agent_id: str) -> str:
        s = str(agent_id or "").strip()
        if not s:
            return ""
        s = re.sub(r"(?<=[a-z0-9])([A-Z])", r"-\1", s)
        s = s.replace("_", "-").replace(" ", "-")
        s = re.sub(r"-{2,}", "-", s)
        s = s.strip("-")
        return s.lower()

    def _get_or_create_agent(self, agents_state: AgentsData, agent_id: str) -> AgentState:
        canonical_id = self._canonicalize_agent_id(agent_id)
        if not canonical_id:
            raise ValueError("agent_id must be a non-empty string")
        for a in agents_state.agents:
            if a.id == canonical_id:
                return a
        agent = AgentState(id=canonical_id)
        agents_state.agents.append(agent)
        return agent

    def update_agent_state(
        self, agent_id: str, status: str, active_task_id: Optional[str] = None
    ) -> AgentState:
        canonical_id = self._canonicalize_agent_id(agent_id)
        if not canonical_id:
            raise ValueError("agent_id must be a non-empty string")
        agents_state = self.load_agents()
        agent = self._get_or_create_agent(agents_state, canonical_id)
        agent.focus = re.sub(r"\s+", " ", str(status or "")).strip()
        agent.active_task_id = str(active_task_id or "").strip() or None
        agent.updated_at = _utc_now_iso()
        self.save_agents(agents_state)
        return agent

    def clear_agent_state(self, agent_id: str) -> AgentState:
        canonical_id = self._canonicalize_agent_id(agent_id)
        if not canonical_id:
            raise ValueError("agent_id must be a non-empty string")
        agents_state = self.load_agents()
        agent = self._get_or_create_agent(agents_state, canonical_id)
        agent.active_task_id = None
        agent.focus = ""
        agent.blockers = []
        agent.next_action = ""
        agent.what_changed = ""
        agent.decision_delta = ""
        agent.environment = ""
        agent.user_profile = ""
        agent.notes = ""
        agent.updated_at = _utc_now_iso()
        self.save_agents(agents_state)
        return agent

    def clear_agent_status(self, agent_id: str) -> AgentState:
        return self.clear_agent_state(agent_id=agent_id)

    def clear_agent_status_if_present(self, agent_id: str) -> bool:
        """Clear an agent status only if an entry already exists."""
        canonical_id = self._canonicalize_agent_id(agent_id)
        if not canonical_id:
            return False

        agents_state = self.load_agents()
        agent = None
        for a in agents_state.agents:
            if a.id == canonical_id:
                agent = a
                break
        if agent is None:
            return False
        agent.focus = ""
        agent.updated_at = _utc_now_iso()
        self.save_agents(agents_state)
        return True

    def delete_agent_state(self, agent_id: str) -> bool:
        """Delete an agent state entry entirely.

        Use this when an actor is removed from the group and should no longer be
        visible in agent-state lists.
        """
        canonical_id = self._canonicalize_agent_id(agent_id)
        if not canonical_id:
            return False

        agents_state = self.load_agents()
        before = len(agents_state.agents)
        agents_state.agents = [a for a in agents_state.agents if a.id != canonical_id]
        if len(agents_state.agents) == before:
            return False
        self.save_agents(agents_state)
        return True

    # =========================================================================
    # Panorama Projection
    # =========================================================================

    def compute_panorama_mermaid(
        self,
        tasks: Optional[List[Task]] = None,
        agents_state: Optional[AgentsData] = None,
        overview: Optional[Overview] = None,
    ) -> str:
        """Compute deterministic panorama mermaid projection from tasks + agents."""
        if tasks is None:
            tasks = self.list_tasks()
        if agents_state is None:
            agents_state = self.load_agents()
        if overview is None:
            context = self.load_context()
            overview = context.overview

        def _safe_node_id(prefix: str, raw: str) -> str:
            cleaned = re.sub(r"[^a-zA-Z0-9_]", "_", str(raw or ""))
            cleaned = re.sub(r"_+", "_", cleaned).strip("_")
            if not cleaned:
                cleaned = "unknown"
            return f"{prefix}_{cleaned}"

        def _safe_label(raw: str) -> str:
            txt = str(raw or "").replace('"', "'").replace("\n", " ").strip()
            return re.sub(r"\s+", " ", txt)[:120]

        lines: List[str] = ["graph TD"]
        lines.append('OVR["Group Overview"]')

        manual = overview.manual if isinstance(overview, Overview) else OverviewManual()
        if manual.current_focus:
            lines.append(f'FOCUS["focus: {_safe_label(manual.current_focus)}"]')
            lines.append("OVR --> FOCUS")

        if manual.roles:
            roles_label = _safe_label(", ".join(str(x) for x in manual.roles if str(x).strip()))
            if roles_label:
                lines.append(f'ROLES["roles: {roles_label}"]')
                lines.append("OVR --> ROLES")

        for task in tasks:
            status = task.status.value if isinstance(task.status, TaskStatus) else str(task.status)
            task_node = _safe_node_id("T", task.id)
            task_label = _safe_label(f"{task.id} {task.name} [{status}]")
            lines.append(f'{task_node}["{task_label}"]')
            if task.parent_id:
                parent_node = _safe_node_id("T", task.parent_id)
                lines.append(f"{parent_node} --> {task_node}")
            else:
                lines.append(f"OVR --> {task_node}")

        for agent in agents_state.agents:
            agent_node = _safe_node_id("A", agent.id)
            agent_focus = _safe_label(agent.focus or "")
            agent_label = _safe_label(f"{agent.id}: {agent_focus or 'idle'}")
            lines.append(f'{agent_node}["{agent_label}"]')
            lines.append(f"OVR -.-> {agent_node}")
            if agent.active_task_id:
                task_node = _safe_node_id("T", agent.active_task_id)
                lines.append(f"{agent_node} --> {task_node}")

        return "\n".join(lines)
