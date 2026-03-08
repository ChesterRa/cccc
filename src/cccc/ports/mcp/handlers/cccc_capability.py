"""MCP handler functions for capability tools."""

from __future__ import annotations

import os
import re
from typing import Any, Dict, Optional

from ....kernel.capabilities import BUILTIN_CAPABILITY_PACKS, CORE_TOOL_NAMES
from ..common import MCPError, _call_daemon_or_raise

_CORE_TOOL_NAME_SET = set(CORE_TOOL_NAMES)
_EXT_TOOL_NAME_RE = re.compile(r"^cccc_ext_[a-f0-9]{8}_(.+)$")


def _skill_runtime_contract_fields(capability_id: str) -> Dict[str, Any]:
    cid = str(capability_id or "").strip()
    if not cid.startswith("skill:"):
        return {}
    return {
        "skill_mode": "capsule_runtime",
        "full_local_skill_equivalent": False,
        "dynamic_tools_expected": False,
        "runtime_visible_in": ["active_capsule_skills"],
        "runtime_activation_evidence": "state:runnable_or_verified + capability_state.active_capsule_skills",
        "next_step_hint": (
            "capsule-runtime skill activation is visible in capability_state.active_capsule_skills. "
            "Do not expect new dynamic_tools for skill capsules. If the task needs full local skill scripts/assets, "
            "install a full skill package into $CODEX_HOME/skills."
        ),
    }


def _tool_name_aliases(name: str) -> set[str]:
    raw = str(name or "").strip()
    if not raw:
        return set()
    out = {
        raw,
        raw.lower(),
        raw.replace("-", "_"),
        raw.replace("_", "-"),
        raw.lower().replace("-", "_"),
        raw.lower().replace("_", "-"),
    }
    m = _EXT_TOOL_NAME_RE.match(raw.lower())
    if m:
        tail = str(m.group(1) or "").strip()
        if tail:
            out.add(tail)
            out.add(tail.replace("-", "_"))
            out.add(tail.replace("_", "-"))
    return {s for s in out if s}


def _normalize_diagnostics(enable_result: Dict[str, Any]) -> list[Dict[str, Any]]:
    raw = enable_result.get("diagnostics") if isinstance(enable_result, dict) else None
    if not isinstance(raw, list):
        return []
    out: list[Dict[str, Any]] = []
    for item in raw:
        if not isinstance(item, dict):
            continue
        code = str(item.get("code") or "").strip()
        if not code:
            continue
        row: Dict[str, Any] = {"code": code}
        message = str(item.get("message") or "").strip()
        if message:
            row["message"] = message
        if "retryable" in item:
            row["retryable"] = bool(item.get("retryable"))
        required_env = item.get("required_env")
        if isinstance(required_env, list):
            names = [str(x).strip() for x in required_env if str(x).strip()]
            if names:
                row["required_env"] = names
        action_hints = item.get("action_hints")
        if isinstance(action_hints, list):
            hints = [str(x).strip() for x in action_hints if str(x).strip()]
            if hints:
                row["action_hints"] = hints
        out.append(row)
    return out


def _build_resolution_plan(*, capability_id: str, diagnostics: list[Dict[str, Any]]) -> Dict[str, Any]:
    codes = {str(item.get("code") or "").strip() for item in diagnostics if isinstance(item, dict)}
    requires_user = set()
    user_requests: list[Dict[str, Any]] = []
    agent_actions: list[str] = []

    for item in diagnostics:
        if not isinstance(item, dict):
            continue
        code = str(item.get("code") or "").strip()
        if code == "missing_required_env":
            requires_user.add(code)
            required_env = item.get("required_env") if isinstance(item.get("required_env"), list) else []
            names = [str(x).strip() for x in required_env if str(x).strip()]
            user_requests.append(
                {
                    "kind": "provide_env",
                    "required_env": names,
                    "message": "please provide required environment variables for this capability",
                }
            )
            continue
        if code == "runtime_permission_denied":
            requires_user.add(code)
            user_requests.append(
                {
                    "kind": "grant_runtime_permission",
                    "message": "please grant runtime permission (e.g., docker socket access) and retry",
                }
            )
            continue
        if code in {"runtime_binary_missing"}:
            agent_actions.append("install_or_expose_runtime_binary_then_retry")
            continue
        if code in {"runtime_dependency_missing", "runtime_start_failed"}:
            agent_actions.append("retry_with_safe_runtime_flags_or_different_version")
            continue
        if code in {"probe_timeout", "network_dns_failure", "network_unreachable"}:
            agent_actions.append("retry_then_check_network")
            continue

    plan: Dict[str, Any] = {
        "status": "needs_agent_action",
        "capability_id": str(capability_id or ""),
        "codes": sorted(code for code in codes if code),
        "agent_actions": sorted(set(agent_actions)),
        "user_requests": user_requests,
    }
    if requires_user:
        plan["status"] = "needs_user_input"
    if not diagnostics:
        plan["status"] = "insufficient_diagnostics"
    return plan


def _should_retry_enable(*, diagnostics: list[Dict[str, Any]]) -> bool:
    if not diagnostics:
        return False
    blocking_codes = {"missing_required_env", "runtime_permission_denied"}
    retryable = False
    for item in diagnostics:
        if not isinstance(item, dict):
            continue
        code = str(item.get("code") or "").strip()
        if code in blocking_codes:
            return False
        if bool(item.get("retryable")):
            retryable = True
    return retryable


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


def capability_import(
    *,
    group_id: str,
    by: str,
    actor_id: Optional[str] = None,
    record: Optional[Dict[str, Any]] = None,
    dry_run: bool = False,
    probe: bool = True,
    enable_after_import: bool = False,
    scope: str = "session",
    ttl_seconds: int = 3600,
    reason: str = "",
) -> Dict[str, Any]:
    """Import a normalized external capability record (agent-prepared), optionally enable after import."""
    target_actor = str(actor_id or by).strip()
    return _call_daemon_or_raise(
        {
            "op": "capability_import",
            "args": {
                "group_id": str(group_id or ""),
                "by": str(by or ""),
                "actor_id": target_actor,
                "record": dict(record) if isinstance(record, dict) else {},
                "dry_run": bool(dry_run),
                "probe": bool(probe),
                "enable_after_import": bool(enable_after_import),
                "scope": str(scope or "session"),
                "ttl_seconds": int(ttl_seconds or 3600),
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
            call_aliases = _tool_name_aliases(call_tool)
            matched_cap_ids: set[str] = set()
            for item in dynamic:
                if not isinstance(item, dict):
                    continue
                dyn_cap_id = str(item.get("capability_id") or "").strip()
                if not dyn_cap_id:
                    continue
                dyn_name = str(item.get("name") or "").strip()
                dyn_real = str(item.get("real_tool_name") or "").strip()
                if call_aliases.intersection(_tool_name_aliases(dyn_name)) or call_aliases.intersection(
                    _tool_name_aliases(dyn_real)
                ):
                    matched_cap_ids.add(dyn_cap_id)
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
    reused_existing_binding = False
    if cap_id == "core":
        enable_result = {
            "state": "runnable",
            "enabled": True,
            "refresh_required": False,
            "scope": "core",
        }
    else:
        state_probe: Dict[str, Any] = {}
        if call_tool:
            try:
                state_probe = capability_state(group_id=group_id, actor_id=target_actor)
            except Exception:
                state_probe = {}
        enabled_caps = {
            str(x).strip()
            for x in (
                state_probe.get("enabled_capabilities")
                if isinstance(state_probe.get("enabled_capabilities"), list)
                else []
            )
            if str(x).strip()
        }
        if call_tool and cap_id in enabled_caps:
            reused_existing_binding = True
            enable_result = {
                "state": "runnable",
                "enabled": True,
                "refresh_required": False,
                "reused_existing_binding": True,
                "scope": str(scope or "session"),
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
        retry_trace: list[Dict[str, Any]] = []
        state = str(enable_result.get("state") or "").strip().lower()
        diagnostics = _normalize_diagnostics(enable_result)
        max_retries = 0 if reused_existing_binding else max(
            0, min(int(os.environ.get("CCCC_CAPABILITY_USE_AUTO_RETRY_MAX") or "1"), 3)
        )
        success_states = {"activation_pending", "runnable", "verified"}
        if state not in success_states and _should_retry_enable(diagnostics=diagnostics) and max_retries > 0:
            for idx in range(max_retries):
                retry_reason = str(reason or "").strip()
                if retry_reason:
                    retry_reason = f"{retry_reason};auto_retry_{idx + 1}"
                else:
                    retry_reason = f"auto_retry_{idx + 1}"
                retry_result = capability_enable(
                    group_id=group_id,
                    by=by,
                    actor_id=target_actor,
                    capability_id=cap_id,
                    scope=scope,
                    enabled=True,
                    ttl_seconds=ttl_seconds,
                    reason=retry_reason,
                )
                retry_state = str(retry_result.get("state") or "").strip().lower()
                retry_trace.append(
                    {
                        "attempt": idx + 1,
                        "state": retry_state,
                        "reason": str(retry_result.get("reason") or ""),
                        "install_error_code": str(retry_result.get("install_error_code") or ""),
                    }
                )
                enable_result = retry_result
                state = retry_state
                diagnostics = _normalize_diagnostics(enable_result)
                if state in success_states:
                    break
        if state not in success_states:
            resolution_plan = _build_resolution_plan(capability_id=cap_id, diagnostics=diagnostics)
            return {
                "group_id": group_id,
                "actor_id": target_actor,
                "capability_id": cap_id,
                "enabled": False,
                "enable_result": enable_result,
                "diagnostics": diagnostics,
                "resolution_plan": resolution_plan,
                "auto_retry": {
                    "attempted": bool(retry_trace),
                    "attempts": retry_trace,
                    "max_retries": max_retries,
                },
                "state": state,
                "tool_called": False,
                **_skill_runtime_contract_fields(cap_id),
            }

    state = str(enable_result.get("state") or "runnable").strip().lower() or "runnable"

    if not call_tool:
        out = {
            "group_id": group_id,
            "actor_id": target_actor,
            "capability_id": cap_id,
            "enabled": True,
            "state": state,
            "refresh_required": bool(enable_result.get("refresh_required")),
            "enable_result": enable_result,
            "tool_called": False,
        }
        skill_payload = enable_result.get("skill") if isinstance(enable_result, dict) else None
        if isinstance(skill_payload, dict):
            out["skill"] = skill_payload
        out.update(_skill_runtime_contract_fields(cap_id))
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
    # otherwise search/get-like operations are unintentionally narrowed to caller scope.
    skip_actor_injection = False
    if call_tool == "cccc_memory":
        mem_action = str(tool_args.get("action") or "search").strip().lower()
        if mem_action in {"layout_get", "search", "get"}:
            skip_actor_injection = True
    elif call_tool == "cccc_memory_admin":
        mem_admin_action = str(tool_args.get("action") or "index_sync").strip().lower()
        if mem_admin_action in {"index_sync", "context_check", "compact", "daily_flush"}:
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
    out = {
        "group_id": group_id,
        "actor_id": target_actor,
        "capability_id": cap_id,
        "enabled": True,
        "state": "verified",
        "refresh_required": False,
        "verification_source": "tool_call",
        "reused_existing_binding": bool(reused_existing_binding),
        "enable_result": enable_result,
        "tool_called": True,
        "tool_name": call_tool,
        "tool_result": tool_result,
    }
    out.update(_skill_runtime_contract_fields(cap_id))
    return out
