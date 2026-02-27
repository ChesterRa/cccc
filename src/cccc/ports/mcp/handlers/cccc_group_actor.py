"""MCP handler functions for group/actor/runtime tools."""

from __future__ import annotations

from typing import Any, Dict, List, Optional

from ....util.conv import coerce_bool
from ..common import _call_daemon_or_raise


def _sanitize_group_doc_for_agent(doc: Any) -> Dict[str, Any]:
    """Return a minimal, non-secret group view for agents.

    This intentionally excludes sensitive fields such as IM tokens and actor env.
    """
    if not isinstance(doc, dict):
        return {}
    out: Dict[str, Any] = {}
    for k in ("group_id", "title", "topic", "created_at", "updated_at", "state", "active_scope_key"):
        if k in doc:
            out[k] = doc.get(k)
    out["running"] = coerce_bool(doc.get("running"), default=False)
    scopes = doc.get("scopes")
    if isinstance(scopes, list):
        safe_scopes: list[dict[str, Any]] = []
        for sc in scopes:
            if not isinstance(sc, dict):
                continue
            safe_scopes.append({
                "scope_key": sc.get("scope_key"),
                "url": sc.get("url"),
                "label": sc.get("label"),
                "git_remote": sc.get("git_remote"),
            })
        out["scopes"] = safe_scopes
    return out


def _sanitize_actors_for_agent(raw: Any) -> List[Dict[str, Any]]:
    """Return a minimal actor view for agents (no env/command)."""
    if not isinstance(raw, list):
        return []
    out: List[Dict[str, Any]] = []
    for a in raw:
        if not isinstance(a, dict):
            continue
        out.append({
            "id": a.get("id"),
            "role": a.get("role"),
            "title": a.get("title"),
            "enabled": coerce_bool(a.get("enabled"), default=True),
            "running": coerce_bool(a.get("running"), default=False),
            "runner": a.get("runner"),
            "runtime": a.get("runtime"),
            "submit": a.get("submit"),
            "unread_count": a.get("unread_count"),
            "updated_at": a.get("updated_at"),
            "created_at": a.get("created_at"),
        })
    return out


def group_info(*, group_id: str) -> Dict[str, Any]:
    """Get group information"""
    res = _call_daemon_or_raise({"op": "group_show", "args": {"group_id": group_id}})
    doc = res.get("group") if isinstance(res, dict) else None
    return {"group": _sanitize_group_doc_for_agent(doc)}


def group_list() -> Dict[str, Any]:
    """List working groups (metadata only)."""
    res = _call_daemon_or_raise({"op": "groups"})
    raw = res.get("groups") if isinstance(res, dict) else None
    if not isinstance(raw, list):
        raw = []
    out: List[Dict[str, Any]] = []
    for g in raw:
        if not isinstance(g, dict):
            continue
        gid = str(g.get("group_id") or "").strip()
        if not gid:
            continue
        out.append(
            {
                "group_id": gid,
                "title": g.get("title") or "",
                "topic": g.get("topic") or "",
                "running": coerce_bool(g.get("running"), default=False),
                "state": g.get("state") or "",
                "updated_at": g.get("updated_at") or "",
                "created_at": g.get("created_at") or "",
            }
        )
    return {"groups": out}


def actor_list(*, group_id: str) -> Dict[str, Any]:
    """Get actor list"""
    res = _call_daemon_or_raise({"op": "actor_list", "args": {"group_id": group_id, "include_unread": True}})
    actors = res.get("actors") if isinstance(res, dict) else None
    return {"actors": _sanitize_actors_for_agent(actors)}


def actor_profile_list(*, by: str) -> Dict[str, Any]:
    """List reusable Actor Profiles."""
    return _call_daemon_or_raise({"op": "actor_profile_list", "args": {"by": by}})


def actor_add(
    *, group_id: str, by: str, actor_id: str,
    runtime: str = "codex", runner: str = "pty", title: str = "",
    command: Optional[List[str]] = None, env: Optional[Dict[str, str]] = None, profile_id: str = ""
) -> Dict[str, Any]:
    """Add a new actor (foreman only). Role is auto-determined by position."""
    req_args: Dict[str, Any] = {
        "group_id": group_id,
        "actor_id": actor_id,
        "runtime": runtime,
        "runner": runner,
        "title": title,
        "command": command or [],
        "env": env or {},
        "by": by,
    }
    pid = str(profile_id or "").strip()
    if pid:
        req_args["profile_id"] = pid
    return _call_daemon_or_raise({
        "op": "actor_add",
        "args": req_args,
    })


def actor_remove(*, group_id: str, by: str, actor_id: str) -> Dict[str, Any]:
    """Remove an actor. Foreman/peer can only remove themselves."""
    return _call_daemon_or_raise({
        "op": "actor_remove",
        "args": {"group_id": group_id, "actor_id": actor_id, "by": by},
    })


def actor_start(*, group_id: str, by: str, actor_id: str) -> Dict[str, Any]:
    """Start an actor (set enabled=true). Foreman can start any; peer cannot start."""
    return _call_daemon_or_raise({
        "op": "actor_start",
        "args": {"group_id": group_id, "actor_id": actor_id, "by": by},
    })


def actor_stop(*, group_id: str, by: str, actor_id: str) -> Dict[str, Any]:
    """Stop an actor (set enabled=false). Foreman can stop any; peer can only stop self."""
    return _call_daemon_or_raise({
        "op": "actor_stop",
        "args": {"group_id": group_id, "actor_id": actor_id, "by": by},
    })


def actor_restart(*, group_id: str, by: str, actor_id: str) -> Dict[str, Any]:
    """Restart an actor (stop + start, clears context). Foreman can restart any; peer can only restart self."""
    return _call_daemon_or_raise({
        "op": "actor_restart",
        "args": {"group_id": group_id, "actor_id": actor_id, "by": by},
    })


def runtime_list() -> Dict[str, Any]:
    """List available agent runtimes on the system"""
    from ....kernel.runtime import detect_all_runtimes
    from ....kernel.settings import get_runtime_pool

    runtimes = detect_all_runtimes(primary_only=False)
    pool = get_runtime_pool()

    return {
        "runtimes": [
            {
                "name": rt.name,
                "display_name": rt.display_name,
                "command": rt.command,
                "available": rt.available,
                "path": rt.path,
                "capabilities": rt.capabilities,
            }
            for rt in runtimes
        ],
        "available": [rt.name for rt in runtimes if rt.available],
        "pool": [
            {
                "runtime": e.runtime,
                "priority": e.priority,
                "scenarios": e.scenarios,
                "notes": e.notes,
            }
            for e in pool
        ],
    }


def group_set_state(*, group_id: str, by: str, state: str) -> Dict[str, Any]:
    """Set group state (active/idle/paused/stopped)."""
    s = str(state or "").strip().lower()
    if s == "stopped":
        return _call_daemon_or_raise({
            "op": "group_stop",
            "args": {"group_id": group_id, "by": by},
        })
    return _call_daemon_or_raise({
        "op": "group_set_state",
        "args": {"group_id": group_id, "state": s, "by": by},
    })
