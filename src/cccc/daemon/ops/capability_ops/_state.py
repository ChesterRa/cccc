"""Capability state management: enable/disable/block/unblock bindings."""

from __future__ import annotations

import sys
import uuid
from datetime import datetime, timedelta, timezone
from typing import Any, Dict, List, Optional, Tuple

from ....contracts.v1 import DaemonError, DaemonResponse
from ....kernel.capabilities import BUILTIN_CAPABILITY_PACKS
from ....util.time import parse_utc_iso, utc_now_iso

from ._common import (
    _STATE_LOCK,
    _CATALOG_LOCK,
    _RUNTIME_LOCK,
    _QUAL_QUALIFIED,
    _QUAL_BLOCKED,
    _QUAL_UNAVAILABLE,
    _error,
    _ensure_group,
    _is_foreman,
    _normalize_scope,
    _quota_limit,
    _env_bool,
)
from ._documents import (
    _load_state_doc,
    _save_state_doc,
    _load_runtime_doc,
    _save_runtime_doc,
    _source_state_template,
)
from ._runtime import (
    _runtime_artifacts,
    _runtime_capability_artifacts,
    _runtime_install_for_capability,
    _remove_runtime_capability_artifact,
    _remove_runtime_artifact_if_unreferenced,
    _set_runtime_capability_artifact,
    _set_runtime_actor_binding,
    _remove_runtime_actor_binding,
    _remove_runtime_group_capability_bindings,
    _remove_runtime_capability_bindings_all_groups,
    _record_runtime_recent_success,
    _append_audit_event,
)
from ._policy import (
    _policy_level_visible,
    _allowlist_policy,
)
from ._remote import (
    _fetch_mcp_registry_record_by_server_name,
)
from ._install import (
    _supported_external_install_record,
)


def _pkg():
    """Get parent package module for mock-compatible function lookups."""
    return sys.modules[__name__.rsplit(".", 1)[0]]


def _binding_state_allows_external_tool(state: Any) -> bool:
    token = str(state or "").strip().lower()
    if not token:
        return False
    return token in {"ready", "ready_cached", "tool_call_failed"}


def _install_state_allows_external_tool(state: Any) -> bool:
    token = str(state or "").strip().lower()
    if not token:
        return False
    return token in {"installed", "installed_degraded"}


def _collect_enabled_capabilities(state_doc: Dict[str, Any], *, group_id: str, actor_id: str) -> Tuple[List[str], bool]:
    gid = str(group_id or "").strip()
    aid = str(actor_id or "").strip()
    enabled: List[str] = []
    mutated = False

    group_enabled = state_doc.get("group_enabled") if isinstance(state_doc.get("group_enabled"), dict) else {}
    enabled.extend(list(group_enabled.get(gid) or []))

    actor_enabled = state_doc.get("actor_enabled") if isinstance(state_doc.get("actor_enabled"), dict) else {}
    if isinstance(actor_enabled.get(gid), dict):
        enabled.extend(list((actor_enabled.get(gid) or {}).get(aid) or []))

    now = datetime.now(timezone.utc)
    session_enabled = state_doc.get("session_enabled") if isinstance(state_doc.get("session_enabled"), dict) else {}
    group_sessions = session_enabled.get(gid) if isinstance(session_enabled.get(gid), dict) else {}
    entries = list(group_sessions.get(aid) or [])
    remaining: List[Dict[str, str]] = []
    for item in entries:
        if not isinstance(item, dict):
            mutated = True
            continue
        cap_id = str(item.get("capability_id") or "").strip()
        expires_at = str(item.get("expires_at") or "").strip()
        dt = parse_utc_iso(expires_at)
        if not cap_id or dt is None:
            mutated = True
            continue
        if dt <= now:
            mutated = True
            continue
        enabled.append(cap_id)
        remaining.append({"capability_id": cap_id, "expires_at": expires_at})

    if remaining:
        session_enabled.setdefault(gid, {})
        session_enabled[gid][aid] = remaining
    else:
        if isinstance(session_enabled.get(gid), dict) and aid in session_enabled.get(gid, {}):
            mutated = True
            session_enabled[gid].pop(aid, None)
        if isinstance(session_enabled.get(gid), dict) and not session_enabled.get(gid):
            session_enabled.pop(gid, None)
    state_doc["session_enabled"] = session_enabled

    # Deduplicate while preserving order.
    seen: set[str] = set()
    ordered: List[str] = []
    for cap_id in enabled:
        cid = str(cap_id or "").strip()
        if not cid or cid in seen:
            continue
        seen.add(cid)
        ordered.append(cid)
    return ordered, mutated


def _set_enabled_capability(
    state_doc: Dict[str, Any],
    *,
    group_id: str,
    actor_id: str,
    scope: str,
    capability_id: str,
    enabled: bool,
    ttl_seconds: int,
) -> None:
    gid = str(group_id or "").strip()
    aid = str(actor_id or "").strip()
    cap_id = str(capability_id or "").strip()
    if scope == "group":
        group_enabled = state_doc.setdefault("group_enabled", {})
        items = set(group_enabled.get(gid) or [])
        if enabled:
            items.add(cap_id)
        else:
            items.discard(cap_id)
        if items:
            group_enabled[gid] = sorted(items)
        else:
            group_enabled.pop(gid, None)
        return

    if scope == "actor":
        actor_enabled = state_doc.setdefault("actor_enabled", {})
        group_map = actor_enabled.setdefault(gid, {})
        items = set(group_map.get(aid) or [])
        if enabled:
            items.add(cap_id)
        else:
            items.discard(cap_id)
        if items:
            group_map[aid] = sorted(items)
        else:
            group_map.pop(aid, None)
        if not group_map:
            actor_enabled.pop(gid, None)
        return

    # session scope
    session_enabled = state_doc.setdefault("session_enabled", {})
    group_map = session_enabled.setdefault(gid, {})
    entries = [x for x in list(group_map.get(aid) or []) if isinstance(x, dict)]
    entries = [x for x in entries if str(x.get("capability_id") or "").strip() != cap_id]
    if enabled:
        expires_at = (
            datetime.now(timezone.utc) + timedelta(seconds=max(60, min(ttl_seconds, 24 * 3600)))
        ).isoformat().replace("+00:00", "Z")
        entries.append({"capability_id": cap_id, "expires_at": expires_at})
    if entries:
        group_map[aid] = entries
    else:
        group_map.pop(aid, None)
    if not group_map:
        session_enabled.pop(gid, None)


def _remove_capability_bindings(
    state_doc: Dict[str, Any],
    *,
    group_id: str,
    capability_id: str,
) -> int:
    gid = str(group_id or "").strip()
    cap_id = str(capability_id or "").strip()
    if not gid or not cap_id:
        return 0
    removed = 0

    group_enabled = state_doc.get("group_enabled") if isinstance(state_doc.get("group_enabled"), dict) else {}
    if isinstance(group_enabled.get(gid), list):
        before = set(group_enabled.get(gid) or [])
        if cap_id in before:
            before.discard(cap_id)
            removed += 1
            if before:
                group_enabled[gid] = sorted(before)
            else:
                group_enabled.pop(gid, None)
        state_doc["group_enabled"] = group_enabled

    actor_enabled = state_doc.get("actor_enabled") if isinstance(state_doc.get("actor_enabled"), dict) else {}
    per_group_actor = actor_enabled.get(gid) if isinstance(actor_enabled.get(gid), dict) else {}
    actor_keys = list(per_group_actor.keys()) if isinstance(per_group_actor, dict) else []
    for aid in actor_keys:
        items = set(per_group_actor.get(aid) or [])
        if cap_id in items:
            items.discard(cap_id)
            removed += 1
            if items:
                per_group_actor[aid] = sorted(items)
            else:
                per_group_actor.pop(aid, None)
    if isinstance(actor_enabled, dict):
        if per_group_actor:
            actor_enabled[gid] = per_group_actor
        else:
            actor_enabled.pop(gid, None)
        state_doc["actor_enabled"] = actor_enabled

    session_enabled = state_doc.get("session_enabled") if isinstance(state_doc.get("session_enabled"), dict) else {}
    per_group_session = session_enabled.get(gid) if isinstance(session_enabled.get(gid), dict) else {}
    session_keys = list(per_group_session.keys()) if isinstance(per_group_session, dict) else []
    for aid in session_keys:
        entries = per_group_session.get(aid) if isinstance(per_group_session.get(aid), list) else []
        keep: List[Dict[str, str]] = []
        hit = False
        for item in entries:
            if not isinstance(item, dict):
                continue
            if str(item.get("capability_id") or "").strip() == cap_id:
                hit = True
                continue
            keep.append(item)
        if hit:
            removed += 1
        if keep:
            per_group_session[aid] = keep
        else:
            per_group_session.pop(aid, None)
    if isinstance(session_enabled, dict):
        if per_group_session:
            session_enabled[gid] = per_group_session
        else:
            session_enabled.pop(gid, None)
        state_doc["session_enabled"] = session_enabled

    return removed


def _remove_capability_bindings_all_groups(
    state_doc: Dict[str, Any],
    *,
    capability_id: str,
) -> int:
    cap_id = str(capability_id or "").strip()
    if not cap_id:
        return 0
    group_ids: set[str] = set()
    group_enabled = state_doc.get("group_enabled") if isinstance(state_doc.get("group_enabled"), dict) else {}
    actor_enabled = state_doc.get("actor_enabled") if isinstance(state_doc.get("actor_enabled"), dict) else {}
    session_enabled = state_doc.get("session_enabled") if isinstance(state_doc.get("session_enabled"), dict) else {}
    group_ids.update(str(gid).strip() for gid in group_enabled.keys() if str(gid).strip())
    group_ids.update(str(gid).strip() for gid in actor_enabled.keys() if str(gid).strip())
    group_ids.update(str(gid).strip() for gid in session_enabled.keys() if str(gid).strip())
    removed = 0
    for gid in sorted(group_ids):
        removed += _remove_capability_bindings(state_doc, group_id=gid, capability_id=cap_id)
    return removed


def _set_blocked_capability(
    state_doc: Dict[str, Any],
    *,
    scope: str,
    group_id: str,
    capability_id: str,
    by: str,
    reason: str,
    ttl_seconds: int,
) -> Dict[str, str]:
    cap_id = str(capability_id or "").strip()
    gid = str(group_id or "").strip()
    actor = str(by or "").strip()
    ttl = max(0, min(int(ttl_seconds or 0), 30 * 24 * 3600))
    now_iso = utc_now_iso()
    expires_at = ""
    if ttl > 0:
        expires_at = (datetime.now(timezone.utc) + timedelta(seconds=ttl)).isoformat().replace("+00:00", "Z")
    entry = {
        "reason": str(reason or "").strip(),
        "by": actor,
        "blocked_at": now_iso,
        "expires_at": expires_at,
    }
    if scope == "global":
        global_blocked = state_doc.setdefault("global_blocked", {})
        if not isinstance(global_blocked, dict):
            global_blocked = {}
        global_blocked[cap_id] = entry
        state_doc["global_blocked"] = global_blocked
    else:
        group_blocked = state_doc.setdefault("group_blocked", {})
        if not isinstance(group_blocked, dict):
            group_blocked = {}
        per_group = group_blocked.setdefault(gid, {})
        if not isinstance(per_group, dict):
            per_group = {}
        per_group[cap_id] = entry
        group_blocked[gid] = per_group
        state_doc["group_blocked"] = group_blocked
    return entry


def _unset_blocked_capability(
    state_doc: Dict[str, Any],
    *,
    scope: str,
    group_id: str,
    capability_id: str,
) -> bool:
    cap_id = str(capability_id or "").strip()
    gid = str(group_id or "").strip()
    removed = False
    if scope == "global":
        global_blocked = state_doc.get("global_blocked") if isinstance(state_doc.get("global_blocked"), dict) else {}
        if cap_id in global_blocked:
            global_blocked.pop(cap_id, None)
            removed = True
        state_doc["global_blocked"] = global_blocked
        return removed

    group_blocked = state_doc.get("group_blocked") if isinstance(state_doc.get("group_blocked"), dict) else {}
    per_group = group_blocked.get(gid) if isinstance(group_blocked.get(gid), dict) else {}
    if cap_id in per_group:
        per_group.pop(cap_id, None)
        removed = True
    if per_group:
        group_blocked[gid] = per_group
    else:
        group_blocked.pop(gid, None)
    state_doc["group_blocked"] = group_blocked
    return removed


def _collect_blocked_capabilities(
    state_doc: Dict[str, Any],
    *,
    group_id: str,
) -> Tuple[Dict[str, Dict[str, str]], bool]:
    gid = str(group_id or "").strip()
    now = datetime.now(timezone.utc)
    blocked: Dict[str, Dict[str, str]] = {}
    mutated = False

    global_blocked = state_doc.get("global_blocked") if isinstance(state_doc.get("global_blocked"), dict) else {}
    group_blocked = state_doc.get("group_blocked") if isinstance(state_doc.get("group_blocked"), dict) else {}

    # Global baseline
    for cap_id, raw in list(global_blocked.items()):
        cid = str(cap_id or "").strip()
        if not cid or not isinstance(raw, dict):
            global_blocked.pop(cap_id, None)
            mutated = True
            continue
        expires_at = str(raw.get("expires_at") or "").strip()
        dt = parse_utc_iso(expires_at) if expires_at else None
        if dt is not None and dt <= now:
            global_blocked.pop(cap_id, None)
            mutated = True
            continue
        blocked[cid] = {
            "scope": "global",
            "reason": str(raw.get("reason") or "").strip(),
            "by": str(raw.get("by") or "").strip(),
            "blocked_at": str(raw.get("blocked_at") or "").strip(),
            "expires_at": expires_at,
        }

    # Group override / extension
    per_group = group_blocked.get(gid) if isinstance(group_blocked.get(gid), dict) else {}
    for cap_id, raw in list(per_group.items()):
        cid = str(cap_id or "").strip()
        if not cid or not isinstance(raw, dict):
            per_group.pop(cap_id, None)
            mutated = True
            continue
        expires_at = str(raw.get("expires_at") or "").strip()
        dt = parse_utc_iso(expires_at) if expires_at else None
        if dt is not None and dt <= now:
            per_group.pop(cap_id, None)
            mutated = True
            continue
        blocked[cid] = {
            "scope": "group",
            "reason": str(raw.get("reason") or "").strip(),
            "by": str(raw.get("by") or "").strip(),
            "blocked_at": str(raw.get("blocked_at") or "").strip(),
            "expires_at": expires_at,
        }
    if per_group:
        group_blocked[gid] = per_group
    elif gid in group_blocked:
        group_blocked.pop(gid, None)
        mutated = True

    if mutated:
        state_doc["global_blocked"] = global_blocked
        state_doc["group_blocked"] = group_blocked
    return blocked, mutated


def _has_any_binding_for_capability(
    state_doc: Dict[str, Any],
    *,
    capability_id: str,
) -> bool:
    cap_id = str(capability_id or "").strip()
    if not cap_id:
        return False
    group_enabled = state_doc.get("group_enabled") if isinstance(state_doc.get("group_enabled"), dict) else {}
    for items in group_enabled.values() if isinstance(group_enabled, dict) else []:
        if cap_id in set(items if isinstance(items, list) else []):
            return True
    actor_enabled = state_doc.get("actor_enabled") if isinstance(state_doc.get("actor_enabled"), dict) else {}
    for per_group in actor_enabled.values() if isinstance(actor_enabled, dict) else []:
        if not isinstance(per_group, dict):
            continue
        for items in per_group.values():
            if cap_id in set(items if isinstance(items, list) else []):
                return True
    session_enabled = state_doc.get("session_enabled") if isinstance(state_doc.get("session_enabled"), dict) else {}
    for per_group in session_enabled.values() if isinstance(session_enabled, dict) else []:
        if not isinstance(per_group, dict):
            continue
        for entries in per_group.values():
            if not isinstance(entries, list):
                continue
            for item in entries:
                if not isinstance(item, dict):
                    continue
                if str(item.get("capability_id") or "").strip() == cap_id:
                    return True
    return False


def handle_capability_enable(args: Dict[str, Any]) -> DaemonResponse:
    group_id = str(args.get("group_id") or "").strip()
    by = str(args.get("by") or args.get("actor_id") or "").strip()
    actor_id = str(args.get("actor_id") or by).strip()
    capability_id = str(args.get("capability_id") or "").strip()
    enabled = bool(args.get("enabled", True))
    cleanup = bool(args.get("cleanup", False))
    ttl_seconds = int(args.get("ttl_seconds") or 3600)
    reason = str(args.get("reason") or "").strip()
    if len(reason) > 280:
        reason = reason[:280]
    action_id = f"cact_{uuid.uuid4().hex[:16]}"
    scope_for_audit = str(args.get("scope") or "session").strip().lower() or "session"
    max_enabled_per_actor = _quota_limit("CCCC_CAPABILITY_MAX_ENABLED_PER_ACTOR", 12, minimum=1, maximum=500)
    max_enabled_per_group = _quota_limit("CCCC_CAPABILITY_MAX_ENABLED_PER_GROUP", 24, minimum=1, maximum=2000)
    max_installations_total = _quota_limit("CCCC_CAPABILITY_MAX_INSTALLATIONS_TOTAL", 128, minimum=1, maximum=5000)

    def _audit(
        outcome: str,
        *,
        state: str = "",
        error_code: str = "",
        details: Optional[Dict[str, Any]] = None,
    ) -> None:
        payload = dict(details) if isinstance(details, dict) else {}
        if state:
            payload["state"] = state
        if error_code:
            payload["error_code"] = error_code
        if reason:
            payload["reason"] = reason
        try:
            _append_audit_event(
                action_id=action_id,
                op="capability_enable",
                group_id=group_id,
                actor_id=actor_id,
                by=by,
                capability_id=capability_id,
                scope=scope_for_audit,
                enabled=enabled,
                outcome=outcome,
                details=payload,
            )
        except Exception:
            # Audit write failures should not block capability state mutation path.
            pass

    def _check_enable_quota(*, scope: str, group_id: str, actor_id: str, capability_id: str) -> str:
        with _STATE_LOCK:
            _, state_doc = _load_state_doc()
            if scope in {"actor", "session"}:
                enabled_caps, _ = _collect_enabled_capabilities(
                    state_doc,
                    group_id=group_id,
                    actor_id=actor_id or "user",
                )
                if capability_id not in set(enabled_caps) and len(enabled_caps) >= max_enabled_per_actor:
                    return f"quota_enabled_actor_exceeded:{max_enabled_per_actor}"

            if scope == "group":
                group_enabled = (
                    state_doc.get("group_enabled")
                    if isinstance(state_doc.get("group_enabled"), dict)
                    else {}
                )
                current = set(group_enabled.get(group_id) or [])
                if capability_id not in current and len(current) >= max_enabled_per_group:
                    return f"quota_enabled_group_exceeded:{max_enabled_per_group}"
        return ""

    try:
        group = _ensure_group(group_id)
        scope = _normalize_scope(args.get("scope"))
        scope_for_audit = scope
        policy = _allowlist_policy()
        actor_role = _pkg()._resolve_actor_role(group, actor_id)
        if not capability_id:
            _audit("denied", state="denied", error_code="missing_capability_id")
            return _error("missing_capability_id", "missing capability_id", details={"action_id": action_id})

        # Permission model:
        # - user can write all scopes
        # - actors can write actor/session for self
        # - only foreman can write group scope
        if by != "user":
            if not by:
                _audit("denied", state="denied", error_code="missing_actor_id")
                return _error("missing_actor_id", "missing actor identity (by)", details={"action_id": action_id})
            if scope == "group" and (not _is_foreman(group, by)):
                _audit("denied", state="denied", error_code="permission_denied")
                return _error(
                    "permission_denied",
                    "only foreman can enable group scope",
                    details={"action_id": action_id},
                )
            if scope in {"actor", "session"} and actor_id != by:
                _audit("denied", state="denied", error_code="permission_denied")
                return _error(
                    "permission_denied",
                    "actor can only mutate their own actor/session scope",
                    details={"action_id": action_id},
                )

        blocked_entry: Dict[str, str] = {}
        with _STATE_LOCK:
            state_path, state_doc = _load_state_doc()
            blocked_caps, blocked_mutated = _collect_blocked_capabilities(state_doc, group_id=group_id)
            if blocked_mutated:
                _save_state_doc(state_path, state_doc)
        maybe_block = blocked_caps.get(capability_id) if isinstance(blocked_caps.get(capability_id), dict) else None
        if isinstance(maybe_block, dict):
            blocked_entry = dict(maybe_block)
        if enabled and blocked_entry:
            scope_token = str(blocked_entry.get("scope") or "group").strip().lower()
            reason_code = "blocked_by_global_policy" if scope_token == "global" else "blocked_by_group_policy"
            reason_text = str(blocked_entry.get("reason") or "").strip()
            details = {"blocked_scope": scope_token}
            if reason_text:
                details["blocked_reason"] = reason_text
            _audit("failed", state="failed", error_code=reason_code, details=details)
            result = {
                "action_id": action_id,
                "group_id": group_id,
                "actor_id": actor_id,
                "capability_id": capability_id,
                "scope": scope,
                "enabled": False,
                "state": "failed",
                "refresh_required": False,
                "reason": reason_code,
                "policy_level": "blocked",
                "blocked_scope": scope_token,
            }
            if reason_text:
                result["blocked_reason"] = reason_text
            return DaemonResponse(ok=True, result=result)

        if enabled:
            quota_reason = _check_enable_quota(
                scope=scope,
                group_id=group_id,
                actor_id=actor_id,
                capability_id=capability_id,
            )
            if quota_reason:
                _audit("failed", state="failed", error_code=quota_reason)
                return DaemonResponse(
                    ok=True,
                    result={
                        "action_id": action_id,
                        "group_id": group_id,
                        "actor_id": actor_id,
                        "capability_id": capability_id,
                        "scope": scope,
                        "enabled": False,
                        "state": "failed",
                        "refresh_required": False,
                        "reason": quota_reason,
                    },
                )

        # Built-in packs are directly enable-able.
        if capability_id.startswith("pack:"):
            if capability_id not in BUILTIN_CAPABILITY_PACKS:
                _audit("denied", state="denied", error_code="capability_not_found")
                return _error(
                    "capability_not_found",
                    f"unknown built-in capability: {capability_id}",
                    details={"action_id": action_id},
                )
            policy_level = _pkg()._effective_policy_level(
                policy,
                capability_id=capability_id,
                kind="mcp_toolpack",
                source_id="cccc_builtin",
                actor_role=actor_role,
            )
            if enabled and (not _policy_level_visible(policy_level)):
                reason_code = "policy_level_indexed"
                _audit("failed", state="failed", error_code=reason_code, details={"policy_level": policy_level})
                return DaemonResponse(
                    ok=True,
                    result={
                        "action_id": action_id,
                        "group_id": group_id,
                        "actor_id": actor_id,
                        "capability_id": capability_id,
                        "scope": scope,
                        "enabled": False,
                        "state": "failed",
                        "refresh_required": False,
                        "reason": reason_code,
                        "policy_level": policy_level,
                    },
                )
            with _STATE_LOCK:
                state_path, state_doc = _load_state_doc()
                _set_enabled_capability(
                    state_doc,
                    group_id=group_id,
                    actor_id=actor_id,
                    scope=scope,
                    capability_id=capability_id,
                    enabled=enabled,
                    ttl_seconds=ttl_seconds,
                )
                _save_state_doc(state_path, state_doc)
            if enabled:
                with _RUNTIME_LOCK:
                    runtime_path, runtime_doc = _load_runtime_doc()
                    _record_runtime_recent_success(
                        runtime_doc,
                        capability_id=capability_id,
                        group_id=group_id,
                        actor_id=actor_id,
                        action="enable",
                    )
                    _save_runtime_doc(runtime_path, runtime_doc)
            _audit("ready", state="ready", details={"builtin": True})
            return DaemonResponse(
                ok=True,
                result={
                    "action_id": action_id,
                    "group_id": group_id,
                    "actor_id": actor_id,
                    "capability_id": capability_id,
                    "scope": scope,
                    "enabled": enabled,
                    "state": "ready",
                    "refresh_required": True,
                    "wait": "relist_or_reconnect",
                    "refresh_mode": "relist_or_reconnect",
                    "policy_level": policy_level,
                },
            )

        # External capability path (M2): supports remote_only + package (npm/pypi/oci).
        with _CATALOG_LOCK:
            catalog_path, catalog_doc = _pkg()._load_catalog_doc()
            if _pkg()._ensure_curated_catalog_records(catalog_doc, policy=policy):
                _pkg()._save_catalog_doc(catalog_path, catalog_doc)
            records = catalog_doc.get("records") if isinstance(catalog_doc.get("records"), dict) else {}
        rec = records.get(capability_id) if isinstance(records, dict) else None
        if not isinstance(rec, dict):
            if not enabled:
                removed_installation = False
                cleanup_skipped_reason = ""
                with _STATE_LOCK:
                    state_path, state_doc = _load_state_doc()
                    _set_enabled_capability(
                        state_doc,
                        group_id=group_id,
                        actor_id=actor_id,
                        scope=scope,
                        capability_id=capability_id,
                        enabled=False,
                        ttl_seconds=ttl_seconds,
                    )
                    has_remaining_binding = _has_any_binding_for_capability(state_doc, capability_id=capability_id)
                    _save_state_doc(state_path, state_doc)
                removed_binding_count = 0
                with _RUNTIME_LOCK:
                    runtime_path, runtime_doc = _load_runtime_doc()
                    if scope == "group":
                        removed_binding_count = _remove_runtime_group_capability_bindings(
                            runtime_doc,
                            group_id=group_id,
                            capability_id=capability_id,
                        )
                    else:
                        removed_binding_count = _remove_runtime_actor_binding(
                            runtime_doc,
                            group_id=group_id,
                            actor_id=actor_id,
                            capability_id=capability_id,
                        )
                    if cleanup:
                        if has_remaining_binding:
                            cleanup_skipped_reason = "cleanup_skipped_capability_still_bound"
                        else:
                            removed_artifact_id = _remove_runtime_capability_artifact(
                                runtime_doc,
                                capability_id=capability_id,
                            )
                            if removed_artifact_id:
                                removed_installation = _remove_runtime_artifact_if_unreferenced(
                                    runtime_doc,
                                    artifact_id=removed_artifact_id,
                                )
                    if removed_binding_count > 0:
                        _save_runtime_doc(runtime_path, runtime_doc)
                    elif cleanup and (removed_installation or cleanup_skipped_reason):
                        _save_runtime_doc(runtime_path, runtime_doc)
                _audit(
                    "ready",
                    state="ready",
                    details={
                        "external": True,
                        "disabled": True,
                        "record_missing": True,
                    },
                )
                result: Dict[str, Any] = {
                    "action_id": action_id,
                    "group_id": group_id,
                    "actor_id": actor_id,
                    "capability_id": capability_id,
                    "scope": scope,
                    "enabled": False,
                    "state": "ready",
                    "refresh_required": True,
                    "wait": "relist_or_reconnect",
                    "refresh_mode": "relist_or_reconnect",
                    "removed_binding_count": int(removed_binding_count),
                    "removed_installation": bool(removed_installation),
                    "record_missing": True,
                }
                if cleanup_skipped_reason:
                    result["cleanup_skipped_reason"] = cleanup_skipped_reason
                return DaemonResponse(ok=True, result=result)
            if capability_id.startswith("mcp:") and _env_bool("CCCC_CAPABILITY_SOURCE_MCP_REGISTRY_ENABLED", True):
                server_name = capability_id.split(":", 1)[1]
                fetched: Optional[Dict[str, Any]] = None
                try:
                    fetched = _fetch_mcp_registry_record_by_server_name(server_name)
                except Exception:
                    fetched = None
                if isinstance(fetched, dict):
                    with _CATALOG_LOCK:
                        cpath, cdoc = _pkg()._load_catalog_doc()
                        rows = cdoc.get("records") if isinstance(cdoc.get("records"), dict) else {}
                        rows[capability_id] = fetched
                        cdoc["records"] = rows
                        _pkg()._refresh_source_record_counts(cdoc)
                        _pkg()._save_catalog_doc(cpath, cdoc)
                    rec = fetched
            if not isinstance(rec, dict):
                _audit("denied", state="denied", error_code="capability_not_found")
                return _error(
                    "capability_not_found",
                    f"capability not found: {capability_id}",
                    details={"action_id": action_id},
                )

        policy_level = _pkg()._effective_policy_level(
            policy,
            capability_id=capability_id,
            kind=str(rec.get("kind") or ""),
            source_id=str(rec.get("source_id") or ""),
            actor_role=actor_role,
        )
        if enabled and (not _policy_level_visible(policy_level)):
            reason_code = "policy_level_indexed"
            _audit("failed", state="failed", error_code=reason_code, details={"policy_level": policy_level})
            return DaemonResponse(
                ok=True,
                result={
                    "action_id": action_id,
                    "group_id": group_id,
                    "actor_id": actor_id,
                    "capability_id": capability_id,
                    "scope": scope,
                    "enabled": False,
                    "state": "failed",
                    "refresh_required": False,
                    "reason": reason_code,
                    "policy_level": policy_level,
                },
            )

        if enabled and _pkg()._needs_registry_hydration(capability_id, rec) and _env_bool(
            "CCCC_CAPABILITY_SOURCE_MCP_REGISTRY_ENABLED", True
        ):
            server_name = capability_id.split(":", 1)[1]
            fetched: Optional[Dict[str, Any]] = None
            try:
                fetched = _fetch_mcp_registry_record_by_server_name(server_name)
            except Exception:
                fetched = None
            if isinstance(fetched, dict):
                rec = _pkg()._merge_registry_install_into_record(rec, fetched)
                with _CATALOG_LOCK:
                    cpath, cdoc = _pkg()._load_catalog_doc()
                    rows = cdoc.get("records") if isinstance(cdoc.get("records"), dict) else {}
                    rows[capability_id] = rec
                    cdoc["records"] = rows
                    _pkg()._refresh_source_record_counts(cdoc)
                    _pkg()._save_catalog_doc(cpath, cdoc)

        if not enabled:
            removed_installation = False
            cleanup_skipped_reason = ""
            with _STATE_LOCK:
                state_path, state_doc = _load_state_doc()
                _set_enabled_capability(
                    state_doc,
                    group_id=group_id,
                    actor_id=actor_id,
                    scope=scope,
                    capability_id=capability_id,
                    enabled=False,
                    ttl_seconds=ttl_seconds,
                )
                has_remaining_binding = _has_any_binding_for_capability(state_doc, capability_id=capability_id)
                _save_state_doc(state_path, state_doc)
            removed_binding_count = 0
            with _RUNTIME_LOCK:
                runtime_path, runtime_doc = _load_runtime_doc()
                if scope == "group":
                    removed_binding_count = _remove_runtime_group_capability_bindings(
                        runtime_doc,
                        group_id=group_id,
                        capability_id=capability_id,
                    )
                else:
                    removed_binding_count = _remove_runtime_actor_binding(
                        runtime_doc,
                        group_id=group_id,
                        actor_id=actor_id,
                        capability_id=capability_id,
                    )
                if cleanup:
                    if has_remaining_binding:
                        cleanup_skipped_reason = "cleanup_skipped_capability_still_bound"
                    else:
                        removed_artifact_id = _remove_runtime_capability_artifact(
                            runtime_doc,
                            capability_id=capability_id,
                        )
                        if removed_artifact_id:
                            removed_installation = _remove_runtime_artifact_if_unreferenced(
                                runtime_doc,
                                artifact_id=removed_artifact_id,
                            )
                if removed_binding_count > 0:
                    _save_runtime_doc(runtime_path, runtime_doc)
                elif cleanup and (removed_installation or cleanup_skipped_reason):
                    _save_runtime_doc(runtime_path, runtime_doc)
            _audit("ready", state="ready", details={"external": True, "disabled": True})
            result: Dict[str, Any] = {
                "action_id": action_id,
                "group_id": group_id,
                "actor_id": actor_id,
                "capability_id": capability_id,
                "scope": scope,
                "enabled": False,
                "state": "ready",
                "refresh_required": True,
                "wait": "relist_or_reconnect",
                "refresh_mode": "relist_or_reconnect",
                "removed_binding_count": int(removed_binding_count),
                "removed_installation": bool(removed_installation),
                "policy_level": policy_level,
            }
            if cleanup_skipped_reason:
                result["cleanup_skipped_reason"] = cleanup_skipped_reason
            return DaemonResponse(
                ok=True,
                result=result,
            )

        qualification = str(rec.get("qualification_status") or _QUAL_QUALIFIED).strip().lower()
        if qualification == _QUAL_BLOCKED:
            _audit("denied", state="denied", error_code="qualification_blocked")
            return _error(
                "qualification_blocked",
                f"capability blocked by policy: {capability_id}",
                details={
                    "action_id": action_id,
                    "qualification_reasons": rec.get("qualification_reasons") or [],
                },
            )
        if enabled and (not _pkg()._record_enable_supported(rec, capability_id=capability_id)):
            reason_code = "capability_unavailable"
            _audit("failed", state="failed", error_code=reason_code)
            return DaemonResponse(
                ok=True,
                result={
                    "action_id": action_id,
                    "group_id": group_id,
                    "actor_id": actor_id,
                    "capability_id": capability_id,
                    "scope": scope,
                    "enabled": False,
                    "state": "failed",
                    "refresh_required": False,
                    "reason": reason_code,
                    "policy_level": policy_level,
                    "qualification_status": qualification or _QUAL_UNAVAILABLE,
                },
            )

        rec_kind = str(rec.get("kind") or "").strip().lower()
        if rec_kind == "skill":
            requires_caps = rec.get("requires_capabilities")
            requires = [str(x).strip() for x in requires_caps if str(x).strip()] if isinstance(requires_caps, list) else []
            applied_dependencies: List[str] = []
            skipped_dependencies: List[Dict[str, str]] = []
            with _STATE_LOCK:
                state_path, state_doc = _load_state_doc()
                _set_enabled_capability(
                    state_doc,
                    group_id=group_id,
                    actor_id=actor_id,
                    scope=scope,
                    capability_id=capability_id,
                    enabled=True,
                    ttl_seconds=ttl_seconds,
                )
                for dep_cap_id in requires:
                    if dep_cap_id not in BUILTIN_CAPABILITY_PACKS:
                        skipped_dependencies.append(
                            {"capability_id": dep_cap_id, "reason": "unsupported_skill_dependency"}
                        )
                        continue
                    dep_quota_reason = _check_enable_quota(
                        scope=scope,
                        group_id=group_id,
                        actor_id=actor_id,
                        capability_id=dep_cap_id,
                    )
                    if dep_quota_reason:
                        skipped_dependencies.append({"capability_id": dep_cap_id, "reason": dep_quota_reason})
                        continue
                    _set_enabled_capability(
                        state_doc,
                        group_id=group_id,
                        actor_id=actor_id,
                        scope=scope,
                        capability_id=dep_cap_id,
                        enabled=True,
                        ttl_seconds=ttl_seconds,
                    )
                    applied_dependencies.append(dep_cap_id)
                _save_state_doc(state_path, state_doc)

            with _RUNTIME_LOCK:
                runtime_path, runtime_doc = _load_runtime_doc()
                _set_runtime_actor_binding(
                    runtime_doc,
                    group_id=group_id,
                    actor_id=actor_id,
                    capability_id=capability_id,
                    state="ready_skill",
                    last_error="",
                )
                _record_runtime_recent_success(
                    runtime_doc,
                    capability_id=capability_id,
                    group_id=group_id,
                    actor_id=actor_id,
                    action="enable",
                )
                _save_runtime_doc(runtime_path, runtime_doc)

            capsule = str(rec.get("capsule_text") or "").strip()
            if not capsule:
                capsule = str(rec.get("description_short") or "").strip()
            refresh_required = bool(applied_dependencies)
            _audit(
                "ready",
                state="ready",
                details={
                    "skill": True,
                    "applied_dependencies": applied_dependencies,
                    "skipped_dependencies": skipped_dependencies,
                },
            )
            result: Dict[str, Any] = {
                "action_id": action_id,
                "group_id": group_id,
                "actor_id": actor_id,
                "capability_id": capability_id,
                "scope": scope,
                "enabled": True,
                "state": "ready",
                "refresh_required": refresh_required,
                "policy_level": policy_level,
                "skill": {
                    "capability_id": capability_id,
                    "name": str(rec.get("name") or capability_id),
                    "description_short": str(rec.get("description_short") or ""),
                    "capsule": capsule,
                    "requires_capabilities": requires,
                    "applied_dependencies": applied_dependencies,
                    "skipped_dependencies": skipped_dependencies,
                    "source_id": str(rec.get("source_id") or ""),
                    "source_uri": str(rec.get("source_uri") or ""),
                },
            }
            if refresh_required:
                result["wait"] = "relist_or_reconnect"
                result["refresh_mode"] = "relist_or_reconnect"
            return DaemonResponse(ok=True, result=result)

        supported, unsupported_reason = _supported_external_install_record(rec)
        if not supported:
            reason_code = unsupported_reason or "unsupported_external_installer"
            diagnostics = [
                {
                    "code": str(reason_code),
                    "message": str(reason_code),
                    "retryable": False,
                    "action_hints": ["switch_to_supported_installer_or_runtime"],
                }
            ]
            _audit("failed", state="failed", error_code=unsupported_reason or "unsupported_external_installer")
            return DaemonResponse(
                ok=True,
                result={
                    "action_id": action_id,
                    "group_id": group_id,
                    "actor_id": actor_id,
                    "capability_id": capability_id,
                    "scope": scope,
                    "enabled": False,
                    "state": "failed",
                    "refresh_required": False,
                    "reason": reason_code,
                    "policy_level": policy_level,
                    "diagnostics": diagnostics,
                },
            )

        install: Dict[str, Any]
        reused_cached_install = False
        install_key = _pkg()._external_artifact_cache_key(rec, capability_id=capability_id)
        artifact_id = _pkg()._external_artifact_id(rec, capability_id=capability_id)
        with _RUNTIME_LOCK:
            _, runtime_doc_quota = _load_runtime_doc()
            artifacts_quota = _runtime_artifacts(runtime_doc_quota)
            mapped_artifact_id, existing_mapped = _runtime_install_for_capability(
                runtime_doc_quota,
                capability_id=capability_id,
            )
            if mapped_artifact_id:
                artifact_id = mapped_artifact_id
            existing = existing_mapped if isinstance(existing_mapped, dict) else artifacts_quota.get(artifact_id)
            if isinstance(existing, dict) and _install_state_allows_external_tool(existing.get("state")):
                install = dict(existing)
                reused_cached_install = True
            else:
                if artifact_id not in artifacts_quota and len(artifacts_quota) >= max_installations_total:
                    quota_reason = f"quota_installations_total_exceeded:{max_installations_total}"
                    _audit("failed", state="failed", error_code=quota_reason)
                    return DaemonResponse(
                        ok=True,
                        result={
                            "action_id": action_id,
                            "group_id": group_id,
                            "actor_id": actor_id,
                            "capability_id": capability_id,
                            "scope": scope,
                            "enabled": False,
                            "state": "failed",
                            "refresh_required": False,
                            "reason": quota_reason,
                            "policy_level": policy_level,
                        },
                    )
                install = {}

        if not reused_cached_install:
            try:
                install = _pkg()._install_external_capability(rec, capability_id=capability_id)
            except Exception as e:
                install_error = _pkg()._classify_external_install_error(e)
                error_code = str(install_error.get("code") or "install_failed")
                with _RUNTIME_LOCK:
                    runtime_path, runtime_doc = _load_runtime_doc()
                    failed_artifact = _pkg()._artifact_entry_from_install(
                        {
                            "state": "install_failed",
                            "installer": "",
                            "install_mode": str(rec.get("install_mode") or ""),
                            "invoker": {},
                            "tools": [],
                            "last_error": str(e),
                            "last_error_code": error_code,
                            "updated_at": utc_now_iso(),
                        },
                        artifact_id=artifact_id,
                        install_key=install_key,
                        capability_id=capability_id,
                    )
                    _pkg()._upsert_runtime_artifact_for_capability(
                        runtime_doc,
                        artifact_id=artifact_id,
                        capability_id=capability_id,
                        artifact_entry=failed_artifact,
                    )
                    _set_runtime_actor_binding(
                        runtime_doc,
                        group_id=group_id,
                        actor_id=actor_id,
                        capability_id=capability_id,
                        artifact_id=artifact_id,
                        state="install_failed",
                        last_error=str(e),
                    )
                    _save_runtime_doc(runtime_path, runtime_doc)
                _audit("failed", state="failed", error_code=error_code, details={"error": str(e)})
                reason = f"install_failed:{error_code}"
                result: Dict[str, Any] = {
                    "action_id": action_id,
                    "group_id": group_id,
                    "actor_id": actor_id,
                    "capability_id": capability_id,
                    "scope": scope,
                    "enabled": False,
                    "state": "failed",
                    "refresh_required": False,
                    "reason": reason,
                    "install_error_code": error_code,
                    "retryable": bool(install_error.get("retryable")),
                    "policy_level": policy_level,
                    "diagnostics": _pkg()._diagnostics_from_install_error(e),
                }
                required_env = install_error.get("required_env")
                if isinstance(required_env, list) and required_env:
                    result["required_env"] = [str(x).strip() for x in required_env if str(x).strip()]
                if str(e):
                    result["error"] = str(e)
                return DaemonResponse(
                    ok=True,
                    result=result,
                )

            with _RUNTIME_LOCK:
                runtime_path, runtime_doc = _load_runtime_doc()
                artifact = _pkg()._artifact_entry_from_install(
                    install,
                    artifact_id=artifact_id,
                    install_key=install_key,
                    capability_id=capability_id,
                )
                _pkg()._upsert_runtime_artifact_for_capability(
                    runtime_doc,
                    artifact_id=artifact_id,
                    capability_id=capability_id,
                    artifact_entry=artifact,
                )
                _set_runtime_actor_binding(
                    runtime_doc,
                    group_id=group_id,
                    actor_id=actor_id,
                    capability_id=capability_id,
                    artifact_id=artifact_id,
                    state="ready",
                    last_error="",
                )
                _save_runtime_doc(runtime_path, runtime_doc)
                install = artifact
        else:
            with _RUNTIME_LOCK:
                runtime_path, runtime_doc = _load_runtime_doc()
                _set_runtime_capability_artifact(
                    runtime_doc,
                    capability_id=capability_id,
                    artifact_id=artifact_id,
                )
                _set_runtime_actor_binding(
                    runtime_doc,
                    group_id=group_id,
                    actor_id=actor_id,
                    capability_id=capability_id,
                    artifact_id=artifact_id,
                    state="ready_cached",
                    last_error="",
                )
                _save_runtime_doc(runtime_path, runtime_doc)

        with _STATE_LOCK:
            state_path, state_doc = _load_state_doc()
            _set_enabled_capability(
                state_doc,
                group_id=group_id,
                actor_id=actor_id,
                scope=scope,
                capability_id=capability_id,
                enabled=True,
                ttl_seconds=ttl_seconds,
            )
            _save_state_doc(state_path, state_doc)
        with _RUNTIME_LOCK:
            runtime_path, runtime_doc = _load_runtime_doc()
            _record_runtime_recent_success(
                runtime_doc,
                capability_id=capability_id,
                group_id=group_id,
                actor_id=actor_id,
                action="enable",
            )
            _save_runtime_doc(runtime_path, runtime_doc)

        tools = install.get("tools") if isinstance(install.get("tools"), list) else []
        _audit("ready", state="ready", details={"installed_tool_count": len(tools)})
        install_state = str(install.get("state") or "").strip() or "installed"
        degraded = install_state == "installed_degraded"
        result: Dict[str, Any] = {
            "action_id": action_id,
            "group_id": group_id,
            "actor_id": actor_id,
            "capability_id": capability_id,
            "scope": scope,
            "enabled": True,
            "state": "ready",
            "refresh_required": True,
            "wait": "relist_or_reconnect",
            "refresh_mode": "relist_or_reconnect",
            "installed_tool_count": len(tools),
            "installer": str(install.get("installer") or ""),
            "reused_cached_install": bool(reused_cached_install),
            "policy_level": policy_level,
            "install_state": install_state,
            "degraded": degraded,
        }
        install_error_code = str(install.get("last_error_code") or "").strip()
        if install_error_code:
            result["install_error_code"] = install_error_code
        if degraded:
            degraded_reason = str(install.get("last_error") or "").strip()
            if degraded_reason:
                result["degraded_reason"] = degraded_reason
            result["degraded_call_hint"] = (
                "tools_not_listed_call_capability_use_with_capability_id_and_real_tool_name"
            )
        return DaemonResponse(
            ok=True,
            result=result,
        )
    except LookupError as e:
        _audit("failed", state="failed", error_code="group_not_found", details={"error": str(e)})
        return _error("group_not_found", str(e), details={"action_id": action_id})
    except ValueError as e:
        message = str(e)
        if message == "missing_group_id":
            _audit("failed", state="failed", error_code="missing_group_id")
            return _error("missing_group_id", "missing group_id", details={"action_id": action_id})
        _audit("failed", state="failed", error_code="capability_enable_invalid", details={"error": message})
        return _error("capability_enable_invalid", message, details={"action_id": action_id})
    except Exception as e:
        _audit("failed", state="failed", error_code="capability_enable_failed", details={"error": str(e)})
        return _error("capability_enable_failed", str(e), details={"action_id": action_id})


def handle_capability_block(args: Dict[str, Any]) -> DaemonResponse:
    group_id = str(args.get("group_id") or "").strip()
    by = str(args.get("by") or args.get("actor_id") or "").strip()
    actor_id = str(args.get("actor_id") or by).strip()
    capability_id = str(args.get("capability_id") or "").strip()
    scope = str(args.get("scope") or "group").strip().lower() or "group"
    blocked = bool(args.get("blocked", True))
    ttl_seconds = int(args.get("ttl_seconds") or 0)
    reason = str(args.get("reason") or "").strip()
    if len(reason) > 280:
        reason = reason[:280]
    action_id = f"cblk_{uuid.uuid4().hex[:16]}"

    def _audit(outcome: str, *, state: str = "", error_code: str = "", details: Optional[Dict[str, Any]] = None) -> None:
        payload = dict(details) if isinstance(details, dict) else {}
        if state:
            payload["state"] = state
        if error_code:
            payload["error_code"] = error_code
        if reason:
            payload["reason"] = reason
        try:
            _append_audit_event(
                action_id=action_id,
                op="capability_block",
                group_id=group_id,
                actor_id=actor_id,
                by=by,
                capability_id=capability_id,
                scope=scope,
                enabled=not blocked,
                outcome=outcome,
                details=payload,
            )
        except Exception:
            pass

    try:
        if scope not in {"group", "global"}:
            _audit("failed", state="failed", error_code="invalid_scope")
            return _error("invalid_scope", f"invalid scope: {scope}", details={"action_id": action_id})
        if not capability_id:
            _audit("denied", state="denied", error_code="missing_capability_id")
            return _error("missing_capability_id", "missing capability_id", details={"action_id": action_id})

        group = _ensure_group(group_id) if scope == "group" else None
        if by != "user":
            if not by:
                _audit("denied", state="denied", error_code="missing_actor_id")
                return _error("missing_actor_id", "missing actor identity (by)", details={"action_id": action_id})
            if actor_id != by:
                _audit("denied", state="denied", error_code="permission_denied")
                return _error("permission_denied", "actor can only mutate own block state", details={"action_id": action_id})
            if scope == "global":
                _audit("denied", state="denied", error_code="permission_denied")
                return _error("permission_denied", "only user can mutate global blocklist", details={"action_id": action_id})
            if group is not None and (not _is_foreman(group, by)):
                _audit("denied", state="denied", error_code="permission_denied")
                return _error("permission_denied", "only foreman can mutate group blocklist", details={"action_id": action_id})

        removed_bindings = 0
        block_entry: Dict[str, str] = {}
        with _STATE_LOCK:
            state_path, state_doc = _load_state_doc()
            if blocked:
                block_entry = _set_blocked_capability(
                    state_doc,
                    scope=scope,
                    group_id=group_id,
                    capability_id=capability_id,
                    by=by,
                    reason=reason,
                    ttl_seconds=ttl_seconds,
                )
                if scope == "global":
                    removed_bindings = _remove_capability_bindings_all_groups(state_doc, capability_id=capability_id)
                else:
                    removed_bindings = _remove_capability_bindings(
                        state_doc,
                        group_id=group_id,
                        capability_id=capability_id,
                    )
            else:
                _unset_blocked_capability(
                    state_doc,
                    scope=scope,
                    group_id=group_id,
                    capability_id=capability_id,
                )
            _save_state_doc(state_path, state_doc)

        removed_runtime_bindings = 0
        if blocked:
            with _RUNTIME_LOCK:
                runtime_path, runtime_doc = _load_runtime_doc()
                if scope == "global":
                    removed_runtime_bindings = _remove_runtime_capability_bindings_all_groups(
                        runtime_doc,
                        capability_id=capability_id,
                    )
                else:
                    removed_runtime_bindings = _remove_runtime_group_capability_bindings(
                        runtime_doc,
                        group_id=group_id,
                        capability_id=capability_id,
                    )
                if removed_runtime_bindings > 0:
                    _save_runtime_doc(runtime_path, runtime_doc)

        refresh_required = bool(removed_bindings > 0 or removed_runtime_bindings > 0)
        _audit(
            "ready",
            state="ready",
            details={
                "blocked": bool(blocked),
                "removed_bindings": int(removed_bindings),
                "removed_runtime_bindings": int(removed_runtime_bindings),
            },
        )
        result: Dict[str, Any] = {
            "action_id": action_id,
            "group_id": group_id,
            "actor_id": actor_id,
            "capability_id": capability_id,
            "scope": scope,
            "blocked": bool(blocked),
            "state": "ready",
            "removed_bindings": int(removed_bindings),
            "removed_runtime_bindings": int(removed_runtime_bindings),
            "refresh_required": refresh_required,
        }
        if blocked:
            result["block"] = {
                "reason": str(block_entry.get("reason") or ""),
                "by": str(block_entry.get("by") or by),
                "blocked_at": str(block_entry.get("blocked_at") or utc_now_iso()),
                "expires_at": str(block_entry.get("expires_at") or ""),
            }
        if refresh_required:
            result["wait"] = "relist_or_reconnect"
            result["refresh_mode"] = "relist_or_reconnect"
        return DaemonResponse(ok=True, result=result)
    except LookupError as e:
        _audit("failed", state="failed", error_code="group_not_found", details={"error": str(e)})
        return _error("group_not_found", str(e), details={"action_id": action_id})
    except ValueError as e:
        message = str(e)
        if message == "missing_group_id":
            _audit("failed", state="failed", error_code="missing_group_id")
            return _error("missing_group_id", "missing group_id", details={"action_id": action_id})
        _audit("failed", state="failed", error_code="capability_block_invalid", details={"error": message})
        return _error("capability_block_invalid", message, details={"action_id": action_id})
    except Exception as e:
        _audit("failed", state="failed", error_code="capability_block_failed", details={"error": str(e)})
        return _error("capability_block_failed", str(e), details={"action_id": action_id})
