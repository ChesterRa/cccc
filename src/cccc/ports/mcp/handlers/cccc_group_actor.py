"""MCP handler functions for group/actor/runtime tools."""

from __future__ import annotations

from typing import Any, Dict, List, Optional

from ....daemon.federation.route_lookup import resolve_remote_group_route_token
from ....util.conv import coerce_bool
from ..common import MCPError, _call_daemon_or_raise


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
            "capability_autoload": a.get("capability_autoload"),
            "unread_count": a.get("unread_count"),
            "updated_at": a.get("updated_at"),
            "created_at": a.get("created_at"),
        })
    return out


def _is_headless_runner(value: Any) -> bool:
    return str(value or "").strip().lower() == "headless"


def _caller_runner(*, group_id: str, by: str) -> str:
    gid = str(group_id or "").strip()
    aid = str(by or "").strip()
    if not gid or not aid or aid == "user":
        return ""
    try:
        resp = _call_daemon_or_raise({"op": "actor_list", "args": {"group_id": gid, "include_unread": False}})
    except Exception:
        return ""
    actors = resp.get("actors") if isinstance(resp, dict) else None
    if not isinstance(actors, list):
        return ""
    for actor in actors:
        if not isinstance(actor, dict):
            continue
        if str(actor.get("id") or "").strip() != aid:
            continue
        return str(actor.get("runner") or "").strip().lower()
    return ""


def _caller_allows_headless(*, group_id: str, by: str) -> bool:
    return _is_headless_runner(_caller_runner(group_id=group_id, by=by))


def _require_allowed_profile(profile_id: str, *, group_id: str, by: str) -> Dict[str, Any]:
    pid = str(profile_id or "").strip()
    if not pid:
        return {}
    try:
        resp = _call_daemon_or_raise({"op": "actor_profile_get", "args": {"profile_id": pid, "by": "user"}})
    except Exception:
        return {}
    profile = resp.get("profile") if isinstance(resp, dict) else None
    if not isinstance(profile, dict):
        return {}
    if _is_headless_runner(profile.get("runner")) and not _caller_allows_headless(group_id=group_id, by=by):
        raise ValueError("headless runner is internal-only; use a PTY actor/profile")
    return profile


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


def _normalize_group_token(value: Any) -> str:
    token = str(value or "").strip()
    while token.startswith("#"):
        token = token[1:].strip()
    return token.lower()


def _group_matches_token(group: Dict[str, Any], token: str) -> str:
    if not token:
        return ""
    for field in ("group_id", "title", "topic"):
        value = _normalize_group_token(group.get(field))
        if value and value == token:
            return field
    return ""


def _federation_route_resolution(*, group_id: str, token: str) -> Dict[str, Any]:
    route = resolve_remote_group_route_token(group_id=group_id, token=token)
    if route is None:
        return {}
    return {
        "group_id": route.remote_group_id,
        "title": route.remote_group_title or route.remote_group_id,
        "topic": "",
        "running": True,
        "state": "active",
        "matched_by": "federation_remote_group_title",
        "token": str(token or "").strip(),
        "federation": True,
        "registration_id": route.registration_id,
        "trust_id": route.trust_id,
    }


def group_resolve(*, token: str, group_id: str = "") -> Dict[str, Any]:
    """Resolve a natural #group/title token to one real group_id."""
    raw = str(token or "").strip()
    normalized = _normalize_group_token(raw)
    if not normalized:
        raise MCPError(code="missing_token", message="token is required for cccc_group resolve")

    groups = group_list().get("groups")
    if not isinstance(groups, list):
        groups = []

    matches: list[tuple[Dict[str, Any], str]] = []
    for item in groups:
        if not isinstance(item, dict):
            continue
        matched_by = _group_matches_token(item, normalized)
        if matched_by:
            matches.append((item, matched_by))

    if not matches:
        remote_match = _federation_route_resolution(group_id=group_id, token=raw)
        if remote_match:
            return remote_match
        raise MCPError(
            code="not_found",
            message=f'no group matches token: {raw}. Do not guess dst_group_id; inspect cccc_group(action="list") or ask the user.',
        )
    if len(matches) > 1:
        candidates = [{**group, "matched_by": matched_by} for group, matched_by in matches]
        raise MCPError(
            code="ambiguous",
            message=f"multiple groups match token: {raw}. Use one returned group_id explicitly; do not guess dst_group_id.",
            details={"candidates": candidates},
        )

    group, matched_by = matches[0]
    return {
        "group_id": group.get("group_id") or "",
        "title": group.get("title") or "",
        "topic": group.get("topic") or "",
        "running": coerce_bool(group.get("running"), default=False),
        "state": group.get("state") or "",
        "matched_by": matched_by,
        "token": raw,
    }


def actor_list(*, group_id: str) -> Dict[str, Any]:
    """Get actor list"""
    res = _call_daemon_or_raise({"op": "actor_list", "args": {"group_id": group_id, "include_unread": True}})
    actors = res.get("actors") if isinstance(res, dict) else None
    return {"actors": _sanitize_actors_for_agent(actors)}


def actor_profile_list(*, group_id: str, by: str) -> Dict[str, Any]:
    """List reusable Actor Profiles visible to the current caller."""
    resp = _call_daemon_or_raise({"op": "actor_profile_list", "args": {"by": by}})
    profiles = resp.get("profiles") if isinstance(resp, dict) else None
    if not isinstance(profiles, list):
        return {"profiles": []}
    allow_headless = _caller_allows_headless(group_id=group_id, by=by)
    filtered = [
        item
        for item in profiles
        if isinstance(item, dict) and (allow_headless or not _is_headless_runner(item.get("runner")))
    ]
    return {"profiles": filtered}


def actor_add(
    *, group_id: str, by: str, actor_id: str,
    runtime: str = "codex", runner: str = "pty", title: str = "",
    command: Optional[List[str]] = None,
    env: Optional[Dict[str, str]] = None,
    profile_id: str = "",
    capability_autoload: Optional[List[str]] = None,
) -> Dict[str, Any]:
    """Add a new actor (foreman only). Caller may only use allowed runner/profile types."""
    allow_headless = _caller_allows_headless(group_id=group_id, by=by)
    normalized_runner = str(runner or "pty").strip().lower() or "pty"
    if _is_headless_runner(normalized_runner) and not allow_headless:
        raise ValueError("headless runner is internal-only; use a PTY actor/profile")
    req_args: Dict[str, Any] = {
        "group_id": group_id,
        "actor_id": actor_id,
        "runtime": runtime,
        "runner": "headless" if _is_headless_runner(normalized_runner) else "pty",
        "title": title,
        "command": command or [],
        "env": env or {},
        "by": by,
    }
    pid = str(profile_id or "").strip()
    if pid:
        profile = _require_allowed_profile(pid, group_id=group_id, by=by)
        if _is_headless_runner(profile.get("runner")):
            req_args["runner"] = "headless"
        req_args["profile_id"] = pid
    if isinstance(capability_autoload, list):
        req_args["capability_autoload"] = [str(x).strip() for x in capability_autoload if str(x or "").strip()]
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
    """Restart an actor (stop + start, clears context). Foreman and peer can restart any actor."""
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
