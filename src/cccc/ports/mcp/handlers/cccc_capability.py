"""MCP handler functions for capability tools."""

from __future__ import annotations

from typing import Any, Dict, Optional

from ....kernel.capabilities import BUILTIN_CAPABILITY_PACKS, CORE_TOOL_NAMES
from ..common import MCPError, _call_daemon_or_raise

_CORE_TOOL_NAME_SET = set(CORE_TOOL_NAMES)


def capability_search(
    *,
    group_id: str,
    actor_id: str,
    query: str = "",
    kind: str = "",
    source_id: str = "",
    trust_tier: str = "",
    qualification_status: str = "",
    limit: int = 30,
    include_external: bool = True,
) -> Dict[str, Any]:
    """Search capability registry (built-in packs + synced external catalogs)."""
    return _call_daemon_or_raise(
        {
            "op": "capability_search",
            "args": {
                "group_id": group_id,
                "actor_id": actor_id,
                "by": actor_id,
                "query": str(query or ""),
                "kind": str(kind or ""),
                "source_id": str(source_id or ""),
                "trust_tier": str(trust_tier or ""),
                "qualification_status": str(qualification_status or ""),
                "limit": int(limit or 30),
                "include_external": bool(include_external),
            },
        }
    )


def capability_enable(
    *,
    group_id: str,
    by: str,
    capability_id: str,
    scope: str = "session",
    enabled: bool = True,
    cleanup: bool = False,
    reason: str = "",
    ttl_seconds: int = 3600,
    actor_id: Optional[str] = None,
) -> Dict[str, Any]:
    """Enable/disable a capability for group/actor/session scope."""
    target_actor = str(actor_id or by).strip()
    return _call_daemon_or_raise(
        {
            "op": "capability_enable",
            "args": {
                "group_id": group_id,
                "by": str(by or ""),
                "actor_id": target_actor,
                "capability_id": str(capability_id or ""),
                "scope": str(scope or "session"),
                "enabled": bool(enabled),
                "cleanup": bool(cleanup),
                "reason": str(reason or ""),
                "ttl_seconds": int(ttl_seconds or 3600),
            },
        }
    )


def capability_block(
    *,
    group_id: str,
    by: str,
    capability_id: str,
    scope: str = "group",
    blocked: bool = True,
    reason: str = "",
    ttl_seconds: int = 0,
    actor_id: Optional[str] = None,
) -> Dict[str, Any]:
    """Block/unblock a capability at group/global scope."""
    target_actor = str(actor_id or by).strip()
    return _call_daemon_or_raise(
        {
            "op": "capability_block",
            "args": {
                "group_id": group_id,
                "by": str(by or ""),
                "actor_id": target_actor,
                "capability_id": str(capability_id or ""),
                "scope": str(scope or "group"),
                "blocked": bool(blocked),
                "reason": str(reason or ""),
                "ttl_seconds": int(ttl_seconds or 0),
            },
        }
    )


def capability_state(*, group_id: str, actor_id: str) -> Dict[str, Any]:
    """Return effective capability exposure and visible tool names for caller scope."""
    return _call_daemon_or_raise(
        {
            "op": "capability_state",
            "args": {
                "group_id": group_id,
                "actor_id": actor_id,
                "by": actor_id,
            },
        }
    )


def capability_uninstall(
    *,
    group_id: str,
    by: str,
    capability_id: str,
    reason: str = "",
) -> Dict[str, Any]:
    """Uninstall an external capability runtime cache entry and revoke bindings."""
    return _call_daemon_or_raise(
        {
            "op": "capability_uninstall",
            "args": {
                "group_id": group_id,
                "by": str(by or ""),
                "actor_id": str(by or ""),
                "capability_id": str(capability_id or ""),
                "reason": str(reason or ""),
            },
        }
    )


def capability_use(
    *,
    group_id: str,
    by: str,
    actor_id: Optional[str] = None,
    capability_id: str = "",
    tool_name: str = "",
    tool_arguments: Optional[Dict[str, Any]] = None,
    scope: str = "session",
    ttl_seconds: int = 3600,
    reason: str = "",
) -> Dict[str, Any]:
    """One-step capability use: enable then optionally call tool."""
    target_actor = str(actor_id or by).strip()
    cap_id = str(capability_id or "").strip()
    call_tool = str(tool_name or "").strip()
    tool_args = dict(tool_arguments) if isinstance(tool_arguments, dict) else {}

    if not cap_id and call_tool:
        candidates = [
            pack_id
            for pack_id, pack in BUILTIN_CAPABILITY_PACKS.items()
            if isinstance(pack, dict) and call_tool in set(pack.get("tool_names") or ())
        ]
        if len(candidates) == 1:
            cap_id = str(candidates[0])
        elif len(candidates) > 1:
            raise MCPError(
                code="capability_use_ambiguous_tool",
                message=f"tool maps to multiple capabilities: {call_tool}",
                details={"candidates": candidates},
            )
        else:
            try:
                state = capability_state(group_id=group_id, actor_id=target_actor)
            except Exception:
                state = {}
            dynamic = state.get("dynamic_tools") if isinstance(state.get("dynamic_tools"), list) else []
            matched_cap_ids = {
                str(item.get("capability_id") or "").strip()
                for item in dynamic
                if isinstance(item, dict)
                and str(item.get("capability_id") or "").strip()
                and (
                    str(item.get("name") or "").strip() == call_tool
                    or str(item.get("real_tool_name") or "").strip() == call_tool
                )
            }
            if len(matched_cap_ids) == 1:
                cap_id = next(iter(matched_cap_ids))
            elif len(matched_cap_ids) > 1:
                raise MCPError(
                    code="capability_use_ambiguous_tool",
                    message=f"tool maps to multiple capabilities: {call_tool}",
                    details={"candidates": sorted(matched_cap_ids)},
                )
            elif call_tool in _CORE_TOOL_NAME_SET:
                cap_id = "core"

    if not cap_id:
        raise MCPError(
            code="missing_capability_id",
            message="missing capability_id (and could not infer from tool_name); pass capability_id for external MCP tools",
            details={},
        )

    enable_result: Dict[str, Any]
    if cap_id == "core":
        enable_result = {
            "state": "ready",
            "enabled": True,
            "refresh_required": False,
            "scope": "core",
        }
    else:
        enable_result = capability_enable(
            group_id=group_id,
            by=by,
            actor_id=target_actor,
            capability_id=cap_id,
            scope=scope,
            enabled=True,
            ttl_seconds=ttl_seconds,
            reason=reason,
        )
        state = str(enable_result.get("state") or "").strip().lower()
        if state != "ready":
            return {
                "group_id": group_id,
                "actor_id": target_actor,
                "capability_id": cap_id,
                "enabled": False,
                "enable_result": enable_result,
                "tool_called": False,
            }

    if not call_tool:
        out = {
            "group_id": group_id,
            "actor_id": target_actor,
            "capability_id": cap_id,
            "enabled": True,
            "enable_result": enable_result,
            "tool_called": False,
        }
        skill_payload = enable_result.get("skill") if isinstance(enable_result, dict) else None
        if isinstance(skill_payload, dict):
            out["skill"] = skill_payload
        return out
    if call_tool == "cccc_capability_use":
        raise MCPError(
            code="capability_use_invalid_tool",
            message="cccc_capability_use cannot recursively call itself",
            details={},
        )

    if "group_id" not in tool_args:
        tool_args["group_id"] = group_id
    if "by" not in tool_args:
        tool_args["by"] = by

    # Read-only memory actions should not have actor_id auto-injected,
    # otherwise search-like operations are unintentionally narrowed to caller scope.
    skip_actor_injection = False
    if call_tool == "cccc_memory":
        mem_action = str(tool_args.get("action") or "search").strip().lower()
        if mem_action in {"guide", "search", "stats"}:
            skip_actor_injection = True
    elif call_tool == "cccc_memory_admin":
        mem_admin_action = str(tool_args.get("action") or "ingest").strip().lower()
        if mem_admin_action in {"export", "decay"}:
            skip_actor_injection = True

    if "actor_id" not in tool_args and not skip_actor_injection:
        tool_args["actor_id"] = target_actor

    # External MCP tools should be called via daemon capability_tool_call with explicit
    # capability_id + actor scope to avoid runtime-env mismatch during one-step calls.
    if cap_id.startswith("mcp:"):
        tool_result = _call_daemon_or_raise(
            {
                "op": "capability_tool_call",
                "args": {
                    "group_id": group_id,
                    "actor_id": target_actor,
                    "by": by,
                    "capability_id": cap_id,
                    "tool_name": call_tool,
                    "arguments": tool_args,
                },
            },
            timeout_s=120.0,
        )
    else:
        from ..server import handle_tool_call

        tool_result = handle_tool_call(call_tool, tool_args)
    return {
        "group_id": group_id,
        "actor_id": target_actor,
        "capability_id": cap_id,
        "enabled": True,
        "enable_result": enable_result,
        "tool_called": True,
        "tool_name": call_tool,
        "tool_result": tool_result,
    }
