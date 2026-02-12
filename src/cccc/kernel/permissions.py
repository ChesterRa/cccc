from __future__ import annotations

from typing import Literal, Optional

from .actors import find_actor, get_effective_role
from .group import Group


ActorAction = Literal[
    "actor.list",
    "actor.add",
    "actor.remove",
    "actor.update",
    "actor.start",
    "actor.stop",
    "actor.restart",
]

GroupAction = Literal[
    "group.start",
    "group.stop",
    "group.update",
    "group.settings_update",
    "group.detach_scope",
    "group.delete",
    "group.set_state",
]


def actor_role(group: Group, actor_id: str) -> Optional[str]:
    """Get the effective role of an actor.
    
    Role is auto-determined by position:
    - First enabled actor = foreman
    - All others = peer
    """
    item = find_actor(group, actor_id)
    if item is None:
        return None
    return get_effective_role(group, actor_id)


def require_actor_permission(
    group: Group,
    *,
    by: str,
    action: ActorAction,
    target_actor_id: str = "",
) -> None:
    """Check actor permission for an action.
    
    Permission matrix:
    | Action        | user | foreman      | peer        |
    |---------------|------|--------------|-------------|
    | actor.list    | ✓    | ✓            | ✓           |
    | actor.add     | ✓    | ✓            | ✗           |
    | actor.start   | ✓    | ✓ (any)      | ✗           |
    | actor.stop    | ✓    | ✓ (any)      | ✓ (self)    |
    | actor.restart | ✓    | ✓ (any)      | ✓ (self)    |
    | actor.remove  | ✓    | ✓ (self)     | ✓ (self)    |
    | actor.update  | ✓    | ✗            | ✗           |
    """
    who = (by or "").strip()
    target = (target_actor_id or "").strip()

    # User has full access
    if not who or who == "user":
        return

    role = actor_role(group, who)
    
    if role == "foreman":
        # Foreman can: list, add, start/stop/restart any, remove self
        if action in ("actor.list", "actor.add", "actor.start", "actor.stop", "actor.restart"):
            return
        if action == "actor.remove":
            # Foreman can only remove self
            if target and target == who:
                return
            raise ValueError(f"permission denied: foreman can only remove self, not {target}")
        if action == "actor.update":
            raise ValueError("permission denied: actor.update is only available via CLI/Web UI")
        raise ValueError(f"permission denied: {who} cannot {action}")

    if role == "peer":
        # Peer can: list, stop/restart/remove self
        if action == "actor.list":
            return
        if action in ("actor.stop", "actor.restart", "actor.remove"):
            if target and target == who:
                return
            raise ValueError(f"permission denied: peer can only {action} self, not {target}")
        if action == "actor.add":
            raise ValueError("permission denied: only foreman can add actors")
        if action == "actor.start":
            raise ValueError("permission denied: only foreman can start actors")
        if action == "actor.update":
            raise ValueError("permission denied: actor.update is only available via CLI/Web UI")
        raise ValueError(f"permission denied: {who} cannot {action}")

    raise ValueError(f"unknown actor: {who}")


def require_inbox_permission(group: Group, *, by: str, target_actor_id: str) -> None:
    who = (by or "").strip()
    target = (target_actor_id or "").strip()

    if not who or who == "user":
        return

    role = actor_role(group, who)
    if role == "foreman":
        return

    if role == "peer":
        if target and target == who:
            return
        raise ValueError(f"permission denied: {who} cannot access inbox of {target or ''}".strip())

    raise ValueError(f"unknown actor: {who}")


def require_group_permission(group: Group, *, by: str, action: GroupAction) -> None:
    who = (by or "").strip()
    if not who or who == "user":
        return
    role = actor_role(group, who)
    if role == "foreman":
        return
    if role == "peer":
        raise ValueError(f"permission denied: {who} cannot {action}")
    raise ValueError(f"unknown actor: {who}")
