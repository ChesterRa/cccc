from __future__ import annotations

from ..kernel.actors import find_actor
from ..kernel.group import load_group
from ..kernel.runtime_state_source import actor_uses_codex_app_server_state
from .codex_app_sessions import SUPERVISOR as codex_app_supervisor


def stop_codex_app_server_for_pty_actor_if_needed(*, group_id: str, actor_id: str) -> bool:
    group = load_group(group_id)
    actor = find_actor(group, actor_id) if group is not None else None
    if not isinstance(actor, dict) or not actor_uses_codex_app_server_state(actor):
        return False
    codex_app_supervisor.stop_actor(group_id=group_id, actor_id=actor_id)
    return True
