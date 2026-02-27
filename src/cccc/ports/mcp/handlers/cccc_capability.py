"""MCP handler functions for capability tools."""

from __future__ import annotations

from typing import Any, Dict, Optional

from ....kernel.capabilities import BUILTIN_CAPABILITY_PACKS
from ..common import MCPError, _call_daemon_or_raise


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
    approve: bool = False,
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
                "approve": bool(approve),
                "reason": str(reason or ""),
                "ttl_seconds": int(ttl_seconds or 3600),
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
    approve: bool = False,
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

    if not cap_id:
        raise MCPError(
            code="missing_capability_id",
            message="missing capability_id (and could not infer from tool_name)",
            details={},
        )

    enable_result = capability_enable(
        group_id=group_id,
        by=by,
        actor_id=target_actor,
        capability_id=cap_id,
        scope=scope,
        enabled=True,
        approve=approve,
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

    # Read-only memory tools should not have actor_id auto-injected,
    # as it would unintentionally filter results to the caller's own memories.
    _MEMORY_READ_ONLY_TOOLS = frozenset({
        "cccc_memory_search", "cccc_memory_stats",
        "cccc_memory_decay", "cccc_memory_export",
    })

    if "group_id" not in tool_args:
        tool_args["group_id"] = group_id
    if "by" not in tool_args:
        tool_args["by"] = by
    if "actor_id" not in tool_args and call_tool not in _MEMORY_READ_ONLY_TOOLS:
        tool_args["actor_id"] = target_actor

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
