from __future__ import annotations

import hashlib
import shutil
import uuid
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Dict, Optional

import yaml  # type: ignore

from ..paths import ensure_home
from ..util.fs import atomic_write_text
from ..util.time import utc_now_iso
from .git import git_branch_exists, git_remote_ref_exists, git_root, git_worktree_add
from .registry import Registry
from .scope import ScopeIdentity


def _new_group_id(seed: str) -> str:
    h = hashlib.sha256(seed.encode("utf-8")).hexdigest()
    return "g_" + h[:12]


def _random_group_id() -> str:
    return "g_" + uuid.uuid4().hex[:12]


@dataclass
class Group:
    group_id: str
    path: Path
    doc: Dict[str, Any]

    @property
    def ledger_path(self) -> Path:
        return self.path / "ledger.jsonl"

    def save(self) -> None:
        self.doc.setdefault("v", 1)
        self.doc["updated_at"] = utc_now_iso()
        atomic_write_text(self.path / "group.yaml", yaml.safe_dump(self.doc, allow_unicode=True, sort_keys=False))


def load_group(group_id: str) -> Optional[Group]:
    home = ensure_home()
    gp = home / "groups" / group_id
    p = gp / "group.yaml"
    if not p.exists():
        return None
    try:
        doc = yaml.safe_load(p.read_text(encoding="utf-8")) or {}
        if not isinstance(doc, dict):
            return None
        return Group(group_id=group_id, path=gp, doc=doc)
    except Exception:
        return None


def create_group(reg: Registry, *, title: str, topic: str = "") -> Group:
    home = ensure_home()
    groups_dir = home / "groups"
    groups_dir.mkdir(parents=True, exist_ok=True)

    now = utc_now_iso()
    group_id = _random_group_id()
    gp = groups_dir / group_id
    gp.mkdir(parents=True, exist_ok=True)
    (gp / "context").mkdir(parents=True, exist_ok=True)
    (gp / "scopes").mkdir(parents=True, exist_ok=True)
    (gp / "state").mkdir(parents=True, exist_ok=True)
    (gp / "ledger.jsonl").touch(exist_ok=True)

    group_doc: Dict[str, Any] = {
        "v": 1,
        "group_id": group_id,
        "title": title.strip() if title.strip() else "working-group",
        "topic": topic.strip(),
        "created_at": now,
        "updated_at": now,
        "running": False,
        "state": "active",  # active / idle / paused
        "active_scope_key": "",
        "scopes": [],
        "actors": [],
    }
    atomic_write_text(gp / "group.yaml", yaml.safe_dump(group_doc, allow_unicode=True, sort_keys=False))

    reg.groups[group_id] = {
        "group_id": group_id,
        "title": group_doc["title"],
        "topic": group_doc["topic"],
        "path": str(gp),
        "default_scope_key": "",
        "created_at": now,
        "updated_at": now,
    }
    reg.save()

    # Publish event for SSE subscribers
    from .events import publish_event
    publish_event("group.created", {"group_id": group_id, "title": group_doc["title"]})

    return Group(group_id=group_id, path=gp, doc=group_doc)


def attach_scope_to_group(
    reg: Registry,
    group: Group,
    scope: ScopeIdentity,
    *,
    set_active: bool = True,
    create_worktree: bool = False,
    worktree_branch: str = "",
    base_branch: str = "",
) -> Group:
    """Attach a scope (directory) to a group.

    Args:
        reg: Registry instance
        group: Group to attach the scope to
        scope: ScopeIdentity of the directory
        set_active: Whether to set this scope as the active scope
        create_worktree: If True, create a git worktree for this scope
        worktree_branch: Branch name for the worktree (used if base_branch is not provided)
        base_branch: Base branch to create a new auto-named branch from (e.g., "main").
                     If provided, auto-generates branch name as cccc/<group_id>.
                     Takes precedence over worktree_branch.

    Returns:
        Updated Group
    """
    now = utc_now_iso()

    # Handle worktree creation if requested
    worktree_path = ""
    actual_scope_url = scope.url

    if create_worktree:
        source_path = Path(scope.url)
        repo_root = git_root(source_path)
        if not repo_root:
            raise ValueError(f"Not a git repository: {scope.url}")

        if base_branch:
            # Auto-branch mode: create new branch cccc/<group_id> based on base_branch
            auto_branch_name = f"cccc/{group.group_id}"

            # Determine start_point based on whether base_branch is already a remote ref
            if base_branch.startswith("origin/"):
                # User selected a remote branch directly (e.g., "origin/main")
                if git_remote_ref_exists(repo_root, base_branch):
                    start_point = base_branch
                else:
                    raise ValueError(f"Remote branch '{base_branch}' not found")
            else:
                # User selected a local branch name, prefer origin/<branch> if exists
                remote_ref = f"origin/{base_branch}"
                if git_remote_ref_exists(repo_root, remote_ref):
                    start_point = remote_ref
                elif git_branch_exists(repo_root, base_branch):
                    start_point = base_branch
                else:
                    raise ValueError(f"Base branch '{base_branch}' not found (checked origin/{base_branch} and local)")

            # Create worktree with auto-generated branch name
            worktree_dir = repo_root.parent / f"{group.group_id}-{auto_branch_name.replace('/', '-')}"
            success, msg = git_worktree_add(
                repo_root, worktree_dir, auto_branch_name, create_branch=True, start_point=start_point
            )
            if not success:
                raise ValueError(f"Failed to create worktree: {msg}")

            worktree_path = str(worktree_dir)
            actual_scope_url = worktree_path
        elif worktree_branch:
            # Legacy mode: use provided branch name
            # Create worktree in sibling directory: ../group-<branch>
            worktree_dir = repo_root.parent / f"{group.group_id}-{worktree_branch}"
            # Check if branch already exists to decide whether to create it
            branch_exists = git_branch_exists(repo_root, worktree_branch)
            success, msg = git_worktree_add(repo_root, worktree_dir, worktree_branch, create_branch=not branch_exists)
            if not success:
                raise ValueError(f"Failed to create worktree: {msg}")

            worktree_path = str(worktree_dir)
            actual_scope_url = worktree_path  # Update scope URL to point to worktree
        else:
            raise ValueError("Either worktree_branch or base_branch is required when create_worktree=True")

    scopes = group.doc.get("scopes")
    if not isinstance(scopes, list):
        scopes = []
        group.doc["scopes"] = scopes

    existing: Optional[Dict[str, Any]] = None
    for item in scopes:
        if isinstance(item, dict) and item.get("scope_key") == scope.scope_key:
            existing = item
            break

    scope_entry = existing if existing is not None else {}
    scope_entry.update(
        {
            "scope_key": scope.scope_key,
            "url": actual_scope_url,
            "label": scope.label,
            "git_remote": scope.git_remote,
            "worktree_path": worktree_path,
        }
    )
    if existing is None:
        scopes.append(scope_entry)

    scope_dir = group.path / "scopes" / scope.scope_key
    scope_dir.mkdir(parents=True, exist_ok=True)
    scope_yaml = scope_dir / "scope.yaml"
    created_at = now
    if scope_yaml.exists():
        try:
            prior = yaml.safe_load(scope_yaml.read_text(encoding="utf-8")) or {}
            if isinstance(prior, dict) and isinstance(prior.get("created_at"), str) and prior.get("created_at"):
                created_at = prior["created_at"]
        except Exception:
            pass
    scope_doc: Dict[str, Any] = {
        "v": 1,
        "scope_key": scope.scope_key,
        "url": actual_scope_url,
        "label": scope.label,
        "git_remote": scope.git_remote,
        "worktree_path": worktree_path,
        "created_at": created_at,
        "updated_at": now,
    }
    atomic_write_text(scope_yaml, yaml.safe_dump(scope_doc, allow_unicode=True, sort_keys=False))

    if set_active or not str(group.doc.get("active_scope_key") or "").strip():
        group.doc["active_scope_key"] = scope.scope_key

    group.save()

    reg.defaults[scope.scope_key] = group.group_id
    meta = reg.groups.get(group.group_id)
    if isinstance(meta, dict):
        meta["title"] = group.doc.get("title") or meta.get("title") or ""
        meta["default_scope_key"] = group.doc.get("active_scope_key") or meta.get("default_scope_key") or ""
        meta["updated_at"] = now
    reg.save()
    return group


def set_active_scope(reg: Registry, group: Group, *, scope_key: str) -> Group:
    wanted = scope_key.strip()
    if not wanted:
        raise ValueError("missing scope_key")

    scopes = group.doc.get("scopes")
    if not isinstance(scopes, list):
        scopes = []

    ok = any(isinstance(item, dict) and item.get("scope_key") == wanted for item in scopes)
    if not ok:
        raise ValueError(f"scope not attached: {wanted}")

    group.doc["active_scope_key"] = wanted
    group.save()

    meta = reg.groups.get(group.group_id)
    if isinstance(meta, dict):
        meta["default_scope_key"] = wanted
        meta["updated_at"] = group.doc.get("updated_at") or utc_now_iso()
    reg.save()
    return group


def ensure_group_for_scope(reg: Registry, scope: ScopeIdentity) -> Group:
    home = ensure_home()
    groups_dir = home / "groups"
    groups_dir.mkdir(parents=True, exist_ok=True)

    existing_id = (reg.defaults.get(scope.scope_key) or "").strip()
    if existing_id:
        g = load_group(existing_id)
        if g is not None:
            return attach_scope_to_group(reg, g, scope, set_active=True)

    seed = scope.git_remote or scope.url
    group_id = _new_group_id(seed)
    g = load_group(group_id)
    if g is not None:
        return attach_scope_to_group(reg, g, scope, set_active=True)

    now = utc_now_iso()
    gp = groups_dir / group_id
    gp.mkdir(parents=True, exist_ok=True)
    (gp / "context").mkdir(parents=True, exist_ok=True)
    (gp / "scopes").mkdir(parents=True, exist_ok=True)
    (gp / "state").mkdir(parents=True, exist_ok=True)
    (gp / "ledger.jsonl").touch(exist_ok=True)

    group_doc: Dict[str, Any] = {
        "v": 1,
        "group_id": group_id,
        "title": scope.label,
        "topic": "",
        "created_at": now,
        "updated_at": now,
        "running": False,
        "state": "active",  # active / idle / paused
        "active_scope_key": "",
        "scopes": [],
        "actors": [],
    }
    atomic_write_text(gp / "group.yaml", yaml.safe_dump(group_doc, allow_unicode=True, sort_keys=False))

    reg.groups[group_id] = {
        "group_id": group_id,
        "title": scope.label,
        "topic": "",
        "path": str(gp),
        "default_scope_key": "",
        "created_at": now,
        "updated_at": now,
    }
    reg.save()

    return attach_scope_to_group(reg, Group(group_id=group_id, path=gp, doc=group_doc), scope, set_active=True)


def update_group(reg: Registry, group: Group, *, patch: Dict[str, Any]) -> Group:
    if "title" in patch:
        title = str(patch.get("title") or "").strip()
        if title:
            group.doc["title"] = title

    if "topic" in patch:
        topic = str(patch.get("topic") or "").strip()
        group.doc["topic"] = topic

    group.save()

    meta = reg.groups.get(group.group_id)
    if isinstance(meta, dict):
        meta["title"] = str(group.doc.get("title") or meta.get("title") or "")
        meta["topic"] = str(group.doc.get("topic") or "")
        meta["updated_at"] = str(group.doc.get("updated_at") or utc_now_iso())
    reg.save()
    # Publish event for SSE subscribers (best-effort invalidation for group list).
    from .events import publish_event

    publish_event(
        "group.updated",
        {
            "group_id": group.group_id,
            "title": str(group.doc.get("title") or ""),
            "topic": str(group.doc.get("topic") or ""),
        },
    )
    return group


def detach_scope_from_group(reg: Registry, group: Group, *, scope_key: str) -> Group:
    wanted = scope_key.strip()
    if not wanted:
        raise ValueError("missing scope_key")

    scopes = group.doc.get("scopes")
    if not isinstance(scopes, list):
        scopes = []

    before = len(scopes)
    scopes = [s for s in scopes if not (isinstance(s, dict) and str(s.get("scope_key") or "") == wanted)]
    if len(scopes) == before:
        raise ValueError(f"scope not attached: {wanted}")
    group.doc["scopes"] = scopes

    if str(group.doc.get("active_scope_key") or "") == wanted:
        new_active = ""
        for sc in scopes:
            if not isinstance(sc, dict):
                continue
            k = str(sc.get("scope_key") or "").strip()
            if k:
                new_active = k
                break
        group.doc["active_scope_key"] = new_active

    try:
        shutil.rmtree(group.path / "scopes" / wanted)
    except Exception:
        pass

    if reg.defaults.get(wanted) == group.group_id:
        reg.defaults.pop(wanted, None)

    group.save()

    meta = reg.groups.get(group.group_id)
    if isinstance(meta, dict):
        meta["default_scope_key"] = str(group.doc.get("active_scope_key") or "")
        meta["updated_at"] = str(group.doc.get("updated_at") or utc_now_iso())
    reg.save()
    return group


def delete_group(reg: Registry, *, group_id: str) -> None:
    gid = group_id.strip()
    if not gid:
        raise ValueError("missing group_id")

    home = ensure_home()
    gp = home / "groups" / gid
    if gp.exists():
        shutil.rmtree(gp)

    reg.groups.pop(gid, None)
    for k, v in list(reg.defaults.items()):
        if v == gid:
            reg.defaults.pop(k, None)
    reg.save()

    # Publish event for SSE subscribers
    from .events import publish_event
    publish_event("group.deleted", {"group_id": gid})


def get_group_state(group: Group) -> str:
    """Get the current group state (active/idle/paused)."""
    state = str(group.doc.get("state") or "active").strip()
    if state not in ("active", "idle", "paused"):
        return "active"
    return state


def set_group_state(group: Group, *, state: str) -> Group:
    """Set the group state.
    
    States:
    - active: Normal operation, all automation enabled
    - idle: Task completed, automation disabled, actors stay running but don't work proactively
    - paused: User paused, all automation disabled
    
    State transitions:
    - active -> idle: Foreman judges task complete
    - idle -> active: New message or task arrives
    - active -> paused: User manually pauses
    - paused -> active: User manually resumes
    """
    s = state.strip().lower()
    if s not in ("active", "idle", "paused"):
        raise ValueError(f"invalid state: {state} (must be active/idle/paused)")
    
    group.doc["state"] = s
    group.save()
    # Publish event for SSE subscribers (best-effort invalidation for group list).
    from .events import publish_event

    publish_event("group.state_changed", {"group_id": group.group_id, "state": s})
    return group
