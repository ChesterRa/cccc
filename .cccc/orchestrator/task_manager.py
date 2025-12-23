# -*- coding: utf-8 -*-
"""
Task Manager - CRUD operations for tasks using ccontext directory structure.

This module handles all task file operations:
- Loading and saving tasks from context/tasks/
- Generating task summaries for TUI/IM

Directory structure (ccontext mode):
    context/
        context.yaml         # Execution context (milestones, notes, references)
        tasks/
            T001.yaml        # Task definitions
            T002.yaml
        archive/
            tasks/           # Completed/archived tasks

Two modes:
- ccontext mode: context/ directory exists, use context/tasks/
- init mode: context/ does not exist, wait for Agent/MCP to create it

Design principles:
- Single source of truth: Orchestrator manages all task files
- With MCP: Agents use ccontext MCP tools to update context/tasks
- Without MCP: Agents edit YAML files directly
- Atomic file operations to prevent corruption
"""
from __future__ import annotations

import json
import os
import re
import time
from datetime import datetime
from pathlib import Path
from typing import Any, Dict, List, Optional

try:
    import yaml
except ImportError:
    yaml = None  # type: ignore

from orchestrator.task_schema import (
    TaskDefinition, Step, Scope,
    TaskStatus, StepStatus,
    generate_next_task_id, validate_task_id
)


class TaskManager:
    """
    Manages tasks using ccontext directory structure.

    Directory structure (ccontext mode):
        context/
            context.yaml        # Execution context
            tasks/
                T001.yaml       # Task definition
                T002.yaml
            archive/
                tasks/          # Archived tasks
    
    Two modes:
        - ccontext mode: context/ exists, fully operational
        - init mode: context/ does not exist, waiting for initialization
    
    Error Handling:
        - Parse errors are captured in self.parse_errors (dict: task_id -> error_msg)
        - Errors are NOT printed to stderr to avoid screen flooding
        - TUI/IM can query parse_errors for display
    """

    def __init__(self, root: Path):
        """
        Initialize TaskManager.

        Args:
            root: Project root directory
        """
        self.root = Path(root)
        self.context_dir = self.root / "context"
        self.tasks_dir = self.context_dir / "tasks"
        self.archive_dir = self.context_dir / "archive" / "tasks"
        # Error tracking: task_id -> error message (cleared on successful load)
        self.parse_errors: Dict[str, str] = {}
        # Mode detection
        self._ready = self.context_dir.exists()

    def _ensure_dirs(self) -> None:
        """Ensure context directories exist (only when ready)."""
        if self._ready:
            self.tasks_dir.mkdir(parents=True, exist_ok=True)
            self.archive_dir.mkdir(parents=True, exist_ok=True)
    
    def is_ready(self) -> bool:
        """Check if context directory is initialized."""
        return self._ready
    
    def refresh(self) -> None:
        """
        Refresh task state by re-scanning the context directory.
        
        This clears existing parse_errors and reloads all tasks,
        capturing any new parse errors in the process.
        Also re-checks if context/ directory exists.
        """
        # Re-check mode
        self._ready = self.context_dir.exists()
        
        if not self._ready:
            self.parse_errors.clear()
            return
        
        # Clear stale errors before re-scan
        self.parse_errors.clear()
        # Load all tasks to catch validation errors
        for task in self.list_tasks():
            pass  # list_tasks already captures parse errors
    
    def get_parse_errors(self) -> Dict[str, str]:
        """Get current parse errors for display in TUI/IM."""
        return self.parse_errors.copy()
    
    def clear_error(self, task_id: str) -> None:
        """Clear error for a specific task (called when task is successfully loaded)."""
        self.parse_errors.pop(task_id, None)

    # =========================================================================
    # File I/O Utilities
    # =========================================================================

    def _read_yaml(self, path: Path, task_id: Optional[str] = None) -> Dict[str, Any]:
        """
        Read YAML file with robust error handling.
        
        Features:
        - Handles multi-document YAML (takes first document only)
        - Stores errors in self.parse_errors for TUI/IM display
        - No stderr output to avoid screen flooding
        
        Args:
            path: Path to YAML file
            task_id: Optional task ID for error tracking
        """
        if not path.exists():
            return {}
        try:
            raw_text = path.read_text(encoding='utf-8')
            
            if yaml:
                # Use safe_load_all to handle multi-document YAML gracefully
                # Only use the first document (common agent mistake: appending with ---)
                try:
                    docs = list(yaml.safe_load_all(raw_text))
                    content = docs[0] if docs else {}
                except yaml.YAMLError:
                    # Fallback: try safe_load for better error messages
                    content = yaml.safe_load(raw_text)
            else:
                # Fallback to JSON-like parsing
                content = json.loads(raw_text)
            
            # Clear error on successful parse
            if task_id:
                self.clear_error(task_id)
            return content if isinstance(content, dict) else {}
            
        except Exception as e:
            # Store error for TUI/IM display (no stderr print to avoid flood)
            if task_id:
                # Extract concise, actionable error message
                err_str = str(e)
                # Extract line/column info if available
                if 'line' in err_str and 'column' in err_str:
                    # Keep YAML error context
                    if len(err_str) > 150:
                        err_str = err_str[:150] + "..."
                else:
                    if len(err_str) > 100:
                        err_str = err_str[:100] + "..."
                self.parse_errors[task_id] = f"YAML: {err_str}"
            return {}

    def _write_yaml(self, path: Path, data: Dict[str, Any]) -> bool:
        """
        Write YAML file atomically.

        Returns:
            True on success, False on error
        """
        try:
            path.parent.mkdir(parents=True, exist_ok=True)
            tmp = path.with_suffix('.tmp')
            if yaml:
                content = yaml.safe_dump(data, allow_unicode=True, sort_keys=False, default_flow_style=False)
            else:
                content = json.dumps(data, indent=2, ensure_ascii=False)
            tmp.write_text(content, encoding='utf-8')
            tmp.replace(path)
            return True
        except Exception:
            return False

    # =========================================================================
    # Scope Management (simplified for ccontext mode)
    # =========================================================================

    def get_scope(self) -> Scope:
        """
        Get task scope by scanning context/tasks/ directory.
        
        In ccontext mode, there is no scope.yaml file.
        We build the scope dynamically from existing task files.
        """
        if not self._ready:
            return Scope()
        
        scope = Scope()
        # Scan tasks directory for T###.yaml files
        if self.tasks_dir.exists():
            for item in sorted(self.tasks_dir.glob("T*.yaml")):
                # Extract task ID from filename (T001.yaml -> T001)
                task_id = item.stem
                if validate_task_id(task_id):
                    scope.add_task(task_id)
        return scope

    def save_scope(self, scope: Scope) -> bool:
        """
        No-op in ccontext mode.
        
        In ccontext mode, scope is derived from task files, not stored separately.
        This method is kept for API compatibility but does nothing.
        """
        return True

    def _get_task_ids(self) -> List[str]:
        """
        Get all task IDs from context/tasks/ directory.
        
        Returns sorted list of task IDs (T001, T002, etc.)
        """
        if not self._ready or not self.tasks_dir.exists():
            return []
        
        task_ids = []
        for item in sorted(self.tasks_dir.glob("T*.yaml")):
            task_id = item.stem
            if validate_task_id(task_id):
                task_ids.append(task_id)
        return task_ids

    # =========================================================================
    # Task CRUD Operations
    # =========================================================================

    def _get_task_path(self, task_id: str) -> Optional[Path]:
        """
        Get path to task file.

        File format: context/tasks/T001.yaml
        """
        if not validate_task_id(task_id):
            return None
        if not self._ready:
            return None
        return self.tasks_dir / f"{task_id}.yaml"

    def _ensure_tasks_dir(self) -> bool:
        """
        Ensure tasks directory exists.
        
        Returns True if directory exists or was created.
        """
        if not self._ready:
            return False
        self.tasks_dir.mkdir(parents=True, exist_ok=True)
        return True

    def _normalize_task_data(self, data: Dict[str, Any], source: str = "") -> Dict[str, Any]:
        """
        Normalize and auto-fix common task data issues.
        
        This provides resilience against agent format variations:
        - Status values: 'pending' -> 'planned', unknown -> 'planned'
        - Step IDs: 'S01' -> 'S1', 'step1' -> 'S1', 's1' -> 'S1'
        - Step status: unknown -> 'pending'
        
        Note: Warnings are silently handled (no stderr print) to avoid screen flood.
        
        Args:
            data: Raw task data from YAML
            source: Source file path for logging
            
        Returns:
            Normalized data dict
        """
        # === Task Status Normalization ===
        if 'status' in data:
            raw_status = str(data['status']).lower().strip()
            # Map common variations to valid values
            status_map = {
                'planned': 'planned',
                'active': 'active', 
                'done': 'done',
            }
            if raw_status in status_map:
                data['status'] = status_map[raw_status]
            else:
                # Unknown status -> fallback to 'planned' (silent)
                data['status'] = 'planned'
        
        # === Steps Normalization ===
        if 'steps' in data and isinstance(data['steps'], list):
            normalized_steps = []
            for i, step in enumerate(data['steps']):
                if not isinstance(step, dict):
                    continue
                    
                # Step ID normalization
                if 'id' in step:
                    raw_id = str(step['id']).strip()
                    step['id'] = self._normalize_step_id(raw_id, i + 1)
                else:
                    # No ID -> generate one
                    step['id'] = f"S{i + 1}"
                
                # Step Status normalization
                if 'status' in step:
                    raw_step_status = str(step['status']).lower().strip()
                    step_status_map = {
                        'pending': 'pending',
                        'in_progress': 'in_progress',
                        'done': 'done',
                    }
                    if raw_step_status in step_status_map:
                        step['status'] = step_status_map[raw_step_status]
                    else:
                        # Unknown step status -> fallback to 'pending' (silent)
                        step['status'] = 'pending'
                else:
                    # Missing status -> default to pending
                    step['status'] = 'pending'
                
                # Ensure required fields
                if 'name' not in step or not step['name']:
                    step['name'] = f"Step {i + 1}"
                if 'acceptance' not in step:
                    step['acceptance'] = ''
                    
                normalized_steps.append(step)
            data['steps'] = normalized_steps
        
        return data
    
    def _normalize_step_id(self, raw_id: str, fallback_num: int) -> str:
        """
        Normalize step ID to standard format (S1, S2, S1.1, etc.)
        
        Handles variations:
        - S1, S01, S001 -> S1
        - s1, s01 -> S1
        - Step1, step1, Step-1 -> S1
        - 1, 01 -> S1
        - S1.1, S1.2 -> S1.1, S1.2 (preserved)
        
        Args:
            raw_id: Raw step ID string
            fallback_num: Fallback number if parsing fails
            
        Returns:
            Normalized step ID (S1, S2, S1.1, etc.)
        """
        import re
        
        # Already valid format
        if re.match(r'^S\d+(\.\d+)?$', raw_id):
            # Normalize leading zeros: S01 -> S1
            match = re.match(r'^S0*(\d+)(\.(\d+))?$', raw_id)
            if match:
                main = int(match.group(1))
                sub = match.group(3)
                if sub:
                    return f"S{main}.{int(sub)}"
                return f"S{main}"
            return raw_id
        
        # Lowercase s: s1, s01
        if re.match(r'^s\d+(\.\d+)?$', raw_id, re.IGNORECASE):
            return self._normalize_step_id(raw_id.upper(), fallback_num)
        
        # "Step" prefix: Step1, step-1, Step 1
        match = re.match(r'^step[-\s]?0*(\d+)$', raw_id, re.IGNORECASE)
        if match:
            return f"S{int(match.group(1))}"
        
        # Just a number: 1, 01, 001
        match = re.match(r'^0*(\d+)$', raw_id)
        if match:
            return f"S{int(match.group(1))}"
        
        # Fallback
        return f"S{fallback_num}"

    def get_task(self, task_id: str) -> Optional[TaskDefinition]:
        """
        Load task by ID.

        Args:
            task_id: Task ID (T001)

        Returns:
            TaskDefinition or None if not found
        """
        task_path = self._get_task_path(task_id)
        if not task_path or not task_path.exists():
            return None

        # Pass task_id for error tracking
        data = self._read_yaml(task_path, task_id=task_id)
        if not data:
            # If no data but file exists, error was already recorded in _read_yaml
            return None

        # === Detect non-standard formats and record as errors ===
        # Check for 'phases' instead of 'steps' (common mistake)
        has_phases = 'phases' in data and isinstance(data.get('phases'), list) and len(data['phases']) > 0
        has_steps = 'steps' in data and isinstance(data.get('steps'), list) and len(data['steps']) > 0
        
        if has_phases and not has_steps:
            # Task uses 'phases' instead of 'steps' - this is a format error
            self.parse_errors[task_id] = "Format: uses 'phases' instead of 'steps'. Rename 'phases' to 'steps' and ensure each step has id/name/status."
            return None
        
        # Check for completely missing steps (and no phases either)
        if not has_steps and not has_phases:
            # No steps defined at all - record as warning but still parse
            # (task without steps is technically valid but unusual)
            pass

        try:
            # Normalize data before validation
            data = self._normalize_task_data(data, str(task_path))
            task = TaskDefinition(**data)
            # Clear any previous error on successful load
            self.clear_error(task_id)
            return task
        except Exception as e:
            # Store validation error (don't print to stderr)
            err_str = str(e)
            if len(err_str) > 200:
                err_str = err_str[:200] + "..."
            self.parse_errors[task_id] = f"Validation: {err_str}"
            return None

    def save_task(self, task: TaskDefinition) -> bool:
        """
        Save task to disk.

        Creates tasks directory if needed.

        Args:
            task: TaskDefinition to save

        Returns:
            True on success
        """
        if not self._ensure_tasks_dir():
            return False
        
        task_path = self.tasks_dir / f"{task.id}.yaml"
        return self._write_yaml(task_path, task.model_dump())

    def delete_task(self, task_id: str) -> bool:
        """
        Delete task file.

        Args:
            task_id: Task ID to delete

        Returns:
            True on success
        """
        task_path = self._get_task_path(task_id)
        if not task_path or not task_path.exists():
            return False

        try:
            task_path.unlink()
            self.clear_error(task_id)
            return True
        except Exception:
            return False

    def list_tasks(self) -> List[TaskDefinition]:
        """
        List all tasks in ID order.

        Returns:
            List of TaskDefinition objects
        """
        if not self._ready:
            return []
        
        tasks = []
        for task_id in self._get_task_ids():
            task = self.get_task(task_id)
            if task:
                tasks.append(task)
        return tasks

    def create_task(self, name: str, goal: str, steps: List[Dict[str, str]]) -> Optional[TaskDefinition]:
        """
        Create a new task.

        Args:
            name: Task name
            goal: User-visible outcome
            steps: List of step dicts with 'name' and 'done' keys

        Returns:
            Created TaskDefinition or None on error
        """
        if not self._ready:
            return None
        
        # Generate next task ID
        existing_ids = self._get_task_ids()
        task_id = generate_next_task_id(existing_ids)

        # Build steps
        task_steps = []
        for i, s in enumerate(steps, 1):
            task_steps.append(Step(
                id=f"S{i}",
                name=s.get('name', f'Step {i}'),
                acceptance=s.get('acceptance', 'Done'),
                status=StepStatus.PENDING
            ))

        # Create task
        task = TaskDefinition(
            id=task_id,
            name=name,
            goal=goal,
            status=TaskStatus.PLANNED,
            steps=task_steps
        )

        if self.save_task(task):
            return task
        return None

    # =========================================================================
    # Context Operations (milestones, notes, references)
    # =========================================================================

    def _context_path(self) -> Path:
        """Get path to context.yaml."""
        return self.context_dir / "context.yaml"

    def _presence_path(self) -> Path:
        """Get path to presence.yaml (gitignored runtime state)."""
        return self.context_dir / "presence.yaml"

    def load_context(self) -> Dict[str, Any]:
        """
        Load context from context.yaml.

        Returns:
            Dict with vision, sketch, milestones, notes, references
        """
        if not self._ready:
            return {'vision': None, 'sketch': None, 'milestones': [], 'notes': [], 'references': []}

        path = self._context_path()
        if not path.exists():
            return {'vision': None, 'sketch': None, 'milestones': [], 'notes': [], 'references': []}

        try:
            data = self._read_yaml(path)
            return {
                'vision': data.get('vision'),
                'sketch': data.get('sketch'),
                'milestones': data.get('milestones', []),
                'notes': data.get('notes', []),
                'references': data.get('references', []),
            }
        except Exception:
            return {'vision': None, 'sketch': None, 'milestones': [], 'notes': [], 'references': []}

    def get_active_milestone(self) -> Optional[Dict[str, Any]]:
        """
        Get the currently active milestone.
        
        Returns:
            Active milestone dict or None
        """
        context = self.load_context()
        for m in context.get('milestones', []):
            if m.get('status') == 'active':
                return m
        return None

    def get_milestones_for_display(self) -> List[Dict[str, Any]]:
        """
        Get milestones for display (last 3 done + all active/pending).
        
        Returns:
            List of milestone dicts
        """
        context = self.load_context()
        milestones = context.get('milestones', [])
        
        done = [m for m in milestones if m.get('status') == 'done']
        active_pending = [m for m in milestones if m.get('status') != 'done']
        
        # Take last 3 done milestones
        recent_done = done[-3:] if done else []
        
        return recent_done + active_pending

    def get_notes(self) -> List[Dict[str, Any]]:
        """
        Get notes sorted by ttl (descending).
        
        Returns:
            List of note dicts
        """
        context = self.load_context()
        notes = context.get('notes', [])
        return sorted(notes, key=lambda n: n.get('ttl', n.get('score', 0)), reverse=True)

    def get_references(self) -> List[Dict[str, Any]]:
        """
        Get references sorted by ttl (descending).

        Returns:
            List of reference dicts
        """
        context = self.load_context()
        refs = context.get('references', [])
        return sorted(refs, key=lambda r: r.get('ttl', r.get('score', 0)), reverse=True)

    # =========================================================================
    # Vision/Sketch Operations
    # =========================================================================

    def get_vision(self) -> Optional[str]:
        """
        Get the project vision.

        Returns:
            Vision string or None
        """
        context = self.load_context()
        return context.get('vision')

    def get_sketch(self) -> Optional[str]:
        """
        Get the execution blueprint (sketch).

        Returns:
            Sketch markdown string or None
        """
        context = self.load_context()
        sketch = context.get('sketch')
        if sketch is None:
            return None
        if isinstance(sketch, str):
            return sketch
        # Handle dict/list from YAML - convert to readable string
        if isinstance(sketch, dict):
            import json
            return json.dumps(sketch, indent=2, ensure_ascii=False)
        return str(sketch)

    # =========================================================================
    # Presence Operations
    # =========================================================================

    def get_presence(self) -> List[Dict[str, Any]]:
        """
        Get all agents' presence status from presence.yaml.

        Returns:
            List of agent presence dicts
        """
        if not self._ready:
            return []

        path = self._presence_path()
        if not path.exists():
            return []

        try:
            data = self._read_yaml(path)
            return data.get('agents', [])
        except Exception:
            return []

    def _normalize_agent_id(self, agent_id: str) -> str:
        """
        Normalize agent ID to canonical format (peer-a, peer-b).

        Accepts various formats:
        - peerA, PeerA, peera, peer-a, peer_a -> peer-a
        - peerB, PeerB, peerb, peer-b, peer_b -> peer-b

        Returns:
            Normalized agent ID (peer-a or peer-b) or original if unrecognized
        """
        if not agent_id:
            return agent_id
        normalized = agent_id.lower().replace('_', '-').replace(' ', '-')
        # Map various formats to canonical form
        if normalized in ('peera', 'peer-a', 'a'):
            return 'peer-a'
        if normalized in ('peerb', 'peer-b', 'b'):
            return 'peer-b'
        return agent_id  # Return original for unrecognized IDs

    def update_presence(
        self,
        agent_id: str,
        status: str,
    ) -> bool:
        """
        Update an agent's presence status.

        Args:
            agent_id: Agent ID (any format - will be normalized to peer-a/peer-b)
            status: Natural language description of what the agent is doing/thinking

        Returns:
            True on success
        """
        if not self._ready:
            return False

        # Normalize agent_id to canonical format
        agent_id = self._normalize_agent_id(agent_id)

        path = self._presence_path()

        # Load existing data
        if path.exists():
            data = self._read_yaml(path)
        else:
            data = {'agents': [], 'heartbeat_timeout_seconds': 300}

        agents = data.get('agents', [])

        # Clean up: remove any duplicate entries with non-canonical IDs
        # Keep only entries with canonical IDs or unrecognized IDs
        seen_canonical = set()
        cleaned_agents = []
        for a in agents:
            aid = a.get('id', '')
            normalized_aid = self._normalize_agent_id(aid)
            if normalized_aid in ('peer-a', 'peer-b'):
                if normalized_aid not in seen_canonical:
                    # Update the ID to canonical form
                    a['id'] = normalized_aid
                    cleaned_agents.append(a)
                    seen_canonical.add(normalized_aid)
                # Skip duplicates
            else:
                # Keep unrecognized IDs as-is
                cleaned_agents.append(a)
        agents = cleaned_agents

        # Find or create agent entry
        agent_entry = None
        for a in agents:
            if a.get('id') == agent_id:
                agent_entry = a
                break

        if agent_entry is None:
            agent_entry = {'id': agent_id}
            agents.append(agent_entry)

        # Update fields
        agent_entry['status'] = status
        agent_entry['updated_at'] = datetime.utcnow().isoformat() + 'Z'

        # Remove empty status
        if not agent_entry.get('status'):
            agent_entry.pop('status', None)

        # Re-find and update in list
        for i, a in enumerate(agents):
            if a.get('id') == agent_id:
                agents[i] = agent_entry
                break

        data['agents'] = agents

        return self._write_yaml(path, data)

    def set_agent_idle(self, agent_id: str) -> bool:
        """
        Clear an agent's status.

        Args:
            agent_id: Agent ID

        Returns:
            True on success
        """
        return self.update_presence(
            agent_id=agent_id,
            status='',
        )

    # =========================================================================
    # IM Formatters (for /context subcommands)
    # =========================================================================

    def format_sketch_for_im(self) -> str:
        """
        Format sketch/vision for IM display.

        Format:
        ━━━ Vision ━━━
        [vision statement]

        ━━━ Sketch ━━━
        [sketch markdown]
        """
        lines = []

        vision = self.get_vision()
        if vision:
            lines.append("━━━ Vision ━━━")
            lines.append(vision)
            lines.append("")

        sketch = self.get_sketch()
        if sketch:
            lines.append("━━━ Sketch ━━━")
            lines.append(sketch)
        else:
            lines.append("━━━ Sketch ━━━")
            lines.append("No sketch defined.")

        return '\n'.join(lines)

    def format_presence_for_im(self) -> str:
        """
        Format presence for IM display.

        Format:
        ━━━ Team Status ━━━
        peer-a: Debugging JWT edge case, found timezone issue
        peer-b: (no status)
        """
        presence = self.get_presence()

        if not presence:
            return "━━━ Team Status ━━━\nNo presence data."

        lines = ["━━━ Team Status ━━━"]

        for agent in presence:
            agent_id = agent.get('id', 'unknown')
            status = agent.get('status', '')

            if status:
                lines.append(f"{agent_id}: {status}")
            else:
                lines.append(f"{agent_id}: (no status)")

        return '\n'.join(lines)

    # =========================================================================
    # Summary Generation
    # =========================================================================

    def get_summary(self) -> Dict[str, Any]:
        """
        Get overall blueprint summary for TUI/IM.

        Returns:
            Dict with summary statistics, task list, and any parse errors
        """
        tasks = self.list_tasks()

        # Calculate overall stats
        total = len(tasks)
        completed = sum(1 for t in tasks if t.status == TaskStatus.DONE)
        active = sum(1 for t in tasks if t.status == TaskStatus.ACTIVE)
        planned = sum(1 for t in tasks if t.status == TaskStatus.PLANNED)

        # Find current task (first active)
        current_task = None
        current_step = None
        for t in tasks:
            if t.status == TaskStatus.ACTIVE:
                current_task = t.id
                current_step = t.current_step
                break

        # Overall step progress
        total_steps = sum(t.total_steps for t in tasks)
        completed_steps = sum(t.completed_steps for t in tasks)
        
        # Include parse errors for TUI/IM display
        errors = self.get_parse_errors()

        return {
            'total_tasks': total,
            'completed_tasks': completed,
            'active_tasks': active,
            'planned_tasks': planned,
            'current_task': current_task,
            'current_step': current_step,
            'total_steps': total_steps,
            'completed_steps': completed_steps,
            'progress_percent': int(completed_steps / total_steps * 100) if total_steps else 0,
            'tasks': [
                {
                    'id': t.id,
                    'name': t.name,
                    'status': t.status,
                    'progress': t.progress,
                    'current_step': t.current_step,
                }
                for t in tasks
            ],
            'parse_errors': errors,  # Dict of task_id -> error_msg
            'error_count': len(errors),
        }

    def format_status_line(self) -> str:
        """
        Format single-line status for TUI status bar.

        Returns:
            String like "2/5 → T003 [S2]"
        """
        summary = self.get_summary()
        if summary['total_tasks'] == 0:
            return "No tasks"

        current = summary['current_task'] or '-'
        step = f"[{summary['current_step']}]" if summary['current_step'] else ''
        return f"{summary['completed_tasks']}/{summary['total_tasks']} → {current} {step}".strip()

    def format_task_list(self, expanded: bool = False) -> str:
        """
        Format task list for TUI panel display.

        Args:
            expanded: Whether to show expanded view

        Returns:
            Formatted string for display
        """
        tasks = self.list_tasks()
        if not tasks:
            return "No tasks defined"

        lines = []

        for task in tasks:
            # Status icon
            if task.status == TaskStatus.DONE:
                icon = '✓'
            elif task.status == TaskStatus.ACTIVE:
                icon = '→'
            else:
                icon = '○'

            if expanded:
                # Full task details
                lines.append(f"{icon} {task.id} {task.name}")
                lines.append(f"    Goal: {task.goal}")
                lines.append(f"    Progress: {task.progress} ({task.progress_percent}%)")
                for step in task.steps:
                    step_icon = '✓' if step.status == StepStatus.DONE else ('→' if step.status == StepStatus.IN_PROGRESS else '○')
                    lines.append(f"      {step_icon} {step.id}: {step.name}")
                lines.append('')
            else:
                # Compact view
                lines.append(f"{icon} {task.id} {task.name:<20} {task.progress}")

        return '\n'.join(lines)

    def format_for_im(self) -> str:
        """
        Format task status for IM display.

        Returns:
            IM-friendly formatted string
        """
        summary = self.get_summary()
        if summary['total_tasks'] == 0:
            return "━━━ Tasks ━━━\nNo tasks defined"

        lines = [
            "━━━ Tasks ━━━",
            f"▸ Progress: {summary['completed_tasks']}/{summary['total_tasks']} tasks ({summary['progress_percent']}%)",
        ]

        if summary['current_task']:
            lines.append(f"▸ Current: {summary['current_task']} [{summary['current_step'] or '-'}]")

        lines.append("")

        # Task list (compact)
        for t in summary['tasks'][:5]:  # Limit to 5 for IM
            if t['status'] == 'done':
                icon = '✓'
            elif t['status'] == 'active':
                icon = '→'
            else:
                icon = '○'
            lines.append(f"{icon} {t['id']} {t['name'][:15]} {t['progress']}")

        if len(summary['tasks']) > 5:
            lines.append(f"... and {len(summary['tasks']) - 5} more")

        return '\n'.join(lines)


# Module-level singleton instance
_manager: Optional[TaskManager] = None


def get_task_manager(root: Optional[Path] = None) -> TaskManager:
    """
    Get or create TaskManager singleton.

    Args:
        root: Project root directory (required on first call)

    Returns:
        TaskManager instance
    """
    global _manager
    if _manager is None:
        if root is None:
            root = Path.cwd()
        _manager = TaskManager(root)
    return _manager
