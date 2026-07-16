"""Resolve the target-group handler for a delegated task (T407, T406 v3).

Rules:
- No explicit ``@target-agent`` -> default to the target group's foreman.
- An explicit ``@target-agent`` -> that exact agent (must be resolvable,
  non-internal, not ``user``).
- Never fall back to an arbitrary peer; never broadcast ``@all``.
- No explicit agent and no usable foreman -> ``no_target_foreman``.
- Explicit agent missing -> ``target_agent_not_found``; present but unusable
  (internal / user) -> ``target_agent_unavailable``. Never silently re-route to
  foreman or ``@all``.
"""

from __future__ import annotations

from typing import Any, Tuple

from ...kernel.actors import find_actor, find_foreman, is_internal_actor


def resolve_target_delegatee(group: Any, requested_actor: str = "") -> Tuple[str, str]:
    """Return ``(actor_id, error_code)``. On success error_code is ""."""
    requested = str(requested_actor or "").strip().lstrip("@").strip()

    if requested:
        if requested == "user":
            return "", "target_agent_unavailable"
        actor = find_actor(group, requested)
        if not isinstance(actor, dict):
            return "", "target_agent_not_found"
        if is_internal_actor(actor):
            return "", "target_agent_unavailable"
        aid = str(actor.get("id") or "").strip()
        if not aid:
            return "", "target_agent_not_found"
        return aid, ""

    foreman = find_foreman(group)
    if not isinstance(foreman, dict) or is_internal_actor(foreman):
        return "", "no_target_foreman"
    fid = str(foreman.get("id") or "").strip()
    if not fid or fid == "user":
        return "", "no_target_foreman"
    return fid, ""
