"""
Context storage for CCCC groups.

Inspired by the "ccontext" pattern, each group has a small, shared working context:
- Vision/Sketch: project vision and high-level blueprint
- Milestones: 2–6 coarse phases
- Tasks: deliverable work items with 3–7 steps
- Notes: short notes (manually managed)
- References: file/URL references (manually managed)
- Presence: what each agent is doing

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


class MilestoneStatus(str, Enum):
    DONE = "done"
    ACTIVE = "active"
    PLANNED = "planned"
    ARCHIVED = "archived"


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


class Milestone:
    def __init__(
        self,
        id: str,
        name: str,
        description: str = "",
        status: MilestoneStatus = MilestoneStatus.PLANNED,
        archived_from: Optional[str] = None,
        started: Optional[str] = None,
        completed: Optional[str] = None,
        outcomes: Optional[str] = None,
        updated_at: Optional[str] = None,
    ):
        self.id = id
        self.name = name
        self.description = description
        self.status = status
        self.archived_from = archived_from
        self.started = started
        self.completed = completed
        self.outcomes = outcomes
        self.updated_at = updated_at


class Note:
    def __init__(self, id: str, content: str):
        self.id = id
        self.content = content


class Reference:
    def __init__(self, id: str, url: str, note: str):
        self.id = id
        self.url = url
        self.note = note


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
        status: TaskStatus = TaskStatus.PLANNED,
        archived_from: Optional[str] = None,
        milestone: Optional[str] = None,
        assignee: Optional[str] = None,
        created_at: Optional[str] = None,
        updated_at: Optional[str] = None,
        steps: Optional[List[Step]] = None,
    ):
        self.id = id
        self.name = name
        self.goal = goal
        self.status = status
        self.archived_from = archived_from
        self.milestone = milestone
        self.assignee = assignee
        self.created_at = created_at or _utc_now_iso()
        self.updated_at = updated_at
        self.steps = steps or []

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


class AgentPresence:
    def __init__(self, id: str, status: str = "", updated_at: Optional[str] = None):
        self.id = id
        self.status = status
        self.updated_at = updated_at or _utc_now_iso()


class Context:
    def __init__(
        self,
        vision: Optional[str] = None,
        sketch: Optional[str] = None,
        milestones: Optional[List[Milestone]] = None,
        notes: Optional[List[Note]] = None,
        references: Optional[List[Reference]] = None,
        meta: Optional[Dict[str, Any]] = None,
    ):
        self.vision = vision
        self.sketch = sketch
        self.milestones = milestones or []
        self.notes = notes or []
        self.references = references or []
        self.meta = meta or {}


class PresenceData:
    def __init__(
        self,
        agents: Optional[List[AgentPresence]] = None,
        heartbeat_timeout_seconds: int = 300,
    ):
        self.agents = agents or []
        self.heartbeat_timeout_seconds = heartbeat_timeout_seconds


# =============================================================================
# Helpers
# =============================================================================


def _utc_now_iso() -> str:
    return datetime.now(timezone.utc).isoformat().replace("+00:00", "Z")


def _coerce_milestone_status(value: Any) -> MilestoneStatus:
    raw = str(value or "planned").strip().lower()
    if raw == "pending":
        raw = "planned"
    try:
        return MilestoneStatus(raw)
    except Exception:
        return MilestoneStatus.PLANNED


def _coerce_task_status(value: Any) -> TaskStatus:
    raw = str(value or "planned").strip().lower()
    if raw == "pending":
        raw = "planned"
    try:
        return TaskStatus(raw)
    except Exception:
        return TaskStatus.PLANNED


def _coerce_step_status(value: Any) -> StepStatus:
    raw = str(value or "pending").strip().lower()
    if raw in ("planned",):
        raw = "pending"
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

    def _presence_path(self) -> Path:
        return self.context_dir / "presence.yaml"

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
                "sketch": "Static blueprint only (architecture/strategy). No TODO/progress/tasks.",
                "milestones": "Coarse phase timeline (2-6). Exactly one active.",
                "tasks": "Deliverable work items with 3-7 steps.",
                "linking": "Each task should set milestone: Mx to form Vision→Milestones→Tasks tree.",
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
            sketch = data.get("sketch")
            meta = data.get("meta")
            if not isinstance(meta, dict) or not meta:
                meta = self._default_meta()

            milestones = []
            milestones_raw = data.get("milestones")
            if not isinstance(milestones_raw, list):
                milestones_raw = []
            for m in milestones_raw:
                if not isinstance(m, dict):
                    continue
                milestones.append(
                    Milestone(
                        id=m.get("id", ""),
                        name=m.get("name", ""),
                        description=m.get("description", ""),
                        status=_coerce_milestone_status(m.get("status")),
                        archived_from=m.get("archived_from"),
                        started=m.get("started"),
                        completed=m.get("completed"),
                        outcomes=m.get("outcomes"),
                        updated_at=m.get("updated_at"),
                    )
                )

            notes = []
            for n in data.get("notes", []):
                if not isinstance(n, dict):
                    continue
                notes.append(
                    Note(
                        id=n.get("id", ""),
                        content=n.get("content", ""),
                    )
                )

            refs = []
            for r in data.get("references", []):
                if not isinstance(r, dict):
                    continue
                refs.append(
                    Reference(
                        id=r.get("id", ""),
                        url=r.get("url", ""),
                        note=r.get("note", ""),
                    )
                )

            return Context(
                vision=vision,
                sketch=sketch,
                milestones=milestones,
                notes=notes,
                references=refs,
                meta=meta,
            )
        except Exception:
            return Context(meta=self._default_meta())

    def save_context(self, context: Context) -> None:
        self._ensure_dirs()

        data: Dict[str, Any] = {}
        if context.vision is not None:
            data["vision"] = context.vision
        if context.sketch is not None:
            data["sketch"] = context.sketch

        meta = context.meta if isinstance(context.meta, dict) else {}
        if not meta:
            meta = self._default_meta()
        data["meta"] = meta

        data["milestones"] = [
            {
                k: v
                for k, v in {
                    "id": m.id,
                    "name": m.name,
                    "description": m.description,
                    "status": m.status.value if isinstance(m.status, MilestoneStatus) else m.status,
                    "archived_from": getattr(m, "archived_from", None),
                    "started": m.started,
                    "completed": m.completed,
                    "outcomes": m.outcomes,
                    "updated_at": m.updated_at,
                }.items()
                if v is not None
            }
            for m in context.milestones
        ]

        data["notes"] = [{"id": n.id, "content": n.content} for n in context.notes]
        data["references"] = [
            {"id": r.id, "url": r.url, "note": r.note} for r in context.references
        ]

        path = self._context_path()
        path.write_text(
            yaml.safe_dump(data, allow_unicode=True, sort_keys=False, default_flow_style=False),
            encoding="utf-8",
        )

    # =========================================================================
    # Milestone Operations
    # =========================================================================

    def generate_milestone_id(self, context: Context) -> str:
        max_num = 0
        for m in context.milestones:
            match = re.match(r"M(\d+)", m.id)
            if match:
                max_num = max(max_num, int(match.group(1)))
        return f"M{max_num + 1}"

    def get_milestone(self, context: Context, milestone_id: str) -> Optional[Milestone]:
        for m in context.milestones:
            if m.id == milestone_id:
                return m
        return None

    # =========================================================================
    # Note Operations
    # =========================================================================

    def generate_note_id(self, context: Context) -> str:
        max_num = 0
        for note in context.notes:
            match = re.match(r"N(\d+)", note.id)
            if match:
                max_num = max(max_num, int(match.group(1)))
        return f"N{max_num + 1:03d}"

    def get_note_by_id(self, context: Context, note_id: str) -> Optional[Note]:
        for n in context.notes:
            if n.id == note_id:
                return n
        return None

    # =========================================================================
    # Reference Operations
    # =========================================================================

    def generate_reference_id(self, context: Context) -> str:
        max_num = 0
        for ref in context.references:
            match = re.match(r"R(\d+)", ref.id)
            if match:
                max_num = max(max_num, int(match.group(1)))
        return f"R{max_num + 1:03d}"

    def get_reference_by_id(self, context: Context, ref_id: str) -> Optional[Reference]:
        for r in context.references:
            if r.id == ref_id:
                return r
        return None

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

            return Task(
                id=data.get("id", ""),
                name=data.get("name", ""),
                goal=data.get("goal", ""),
                status=task_status,
                archived_from=data.get("archived_from"),
                milestone=data.get("milestone") or data.get("milestone_id"),
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
            "status": task.status.value if isinstance(task.status, TaskStatus) else task.status,
            "archived_from": getattr(task, "archived_from", None),
            "milestone": task.milestone,
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

    # =========================================================================
    # Presence Operations
    # =========================================================================

    def load_presence(self) -> PresenceData:
        path = self._presence_path()
        if not path.exists():
            return PresenceData()

        try:
            data = yaml.safe_load(path.read_text(encoding="utf-8"))
            if data is None:
                return PresenceData()

            agents = []
            agents_raw = data.get("agents")
            if not isinstance(agents_raw, list):
                agents_raw = []
            for a in agents_raw:
                if not isinstance(a, dict):
                    continue
                agents.append(
                    AgentPresence(
                        id=a.get("id", ""),
                        status=a.get("status", ""),
                        updated_at=a.get("updated_at", ""),
                    )
                )

            return PresenceData(
                agents=agents,
                heartbeat_timeout_seconds=max(0, _coerce_int(data.get("heartbeat_timeout_seconds", 300), 300)),
            )
        except Exception:
            return PresenceData()

    def save_presence(self, presence: PresenceData) -> None:
        self._ensure_dirs()

        data = {
            "agents": [
                {k: v for k, v in {"id": a.id, "status": a.status, "updated_at": a.updated_at}.items() if v}
                for a in presence.agents
            ],
            "heartbeat_timeout_seconds": presence.heartbeat_timeout_seconds,
        }

        path = self._presence_path()
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

    def update_agent_presence(self, agent_id: str, status: str) -> AgentPresence:
        canonical_id = self._canonicalize_agent_id(agent_id)
        if not canonical_id:
            raise ValueError("agent_id must be a non-empty string")

        status_norm = re.sub(r"\s+", " ", str(status or "")).strip()
        presence = self.load_presence()

        agent = None
        for a in presence.agents:
            if a.id == canonical_id:
                agent = a
                break

        if agent is None:
            agent = AgentPresence(id=canonical_id)
            presence.agents.append(agent)

        agent.status = status_norm
        agent.updated_at = _utc_now_iso()

        self.save_presence(presence)
        return agent

    def clear_agent_status(self, agent_id: str) -> AgentPresence:
        return self.update_agent_presence(agent_id=agent_id, status="")

    def clear_agent_status_if_present(self, agent_id: str) -> bool:
        """Clear an agent status only if an entry already exists."""
        canonical_id = self._canonicalize_agent_id(agent_id)
        if not canonical_id:
            return False

        presence = self.load_presence()
        agent = None
        for a in presence.agents:
            if a.id == canonical_id:
                agent = a
                break
        if agent is None:
            return False

        agent.status = ""
        agent.updated_at = _utc_now_iso()
        self.save_presence(presence)
        return True

    def delete_agent_presence(self, agent_id: str) -> bool:
        """Delete an agent presence entry entirely.

        Use this when an actor is removed from the group and should no longer be
        visible in presence lists.
        """
        canonical_id = self._canonicalize_agent_id(agent_id)
        if not canonical_id:
            return False

        presence = self.load_presence()
        before = len(presence.agents)
        presence.agents = [a for a in presence.agents if a.id != canonical_id]
        if len(presence.agents) == before:
            return False
        self.save_presence(presence)
        return True
