"""Dispatch handlers, catalog sync, and profile/autoload for capability_ops."""

from __future__ import annotations

import json
import sys
import uuid
from typing import Any, Dict, List, Optional, Tuple

from ....contracts.v1 import DaemonResponse
from ....kernel.capabilities import BUILTIN_CAPABILITY_PACKS
from ....util.time import parse_utc_iso, utc_now_iso

from ._common import (
    _SOURCE_IDS,
    _CATALOG_LOCK,
    _STATE_LOCK,
    _RUNTIME_LOCK,
    _LEVEL_INDEXED,
    _LEVEL_MOUNTED,
    _QUAL_QUALIFIED,
    _QUAL_BLOCKED,
    _QUAL_UNAVAILABLE,
    _QUAL_STATES,
    _error,
    _ensure_group,
    _is_foreman,
    _env_bool,
    _quota_limit,
)
from ._documents import (
    _source_state_template,
    _load_state_doc,
    _save_state_doc,
    _load_runtime_doc,
    _save_runtime_doc,
)
from ._runtime import (
    _runtime_artifacts,
    _runtime_capability_artifacts,
    _runtime_actor_bindings,
    _record_runtime_recent_success,
    _set_runtime_actor_binding,
    _remove_runtime_group_capability_bindings,
    _remove_runtime_capability_artifact,
    _remove_runtime_artifact_if_unreferenced,
    _append_audit_event,
)
from ._install import (
    _catalog_staleness_seconds,
    _needs_registry_hydration,
    _supported_external_install_record,
    _tool_name_aliases,
    _build_synthetic_tool_name,
    _invoke_installed_external_tool_with_aliases,
)
from ._policy import _normalize_policy_level
from ._remote import _mark_source_disabled
from ._state import (
    _binding_state_allows_external_tool,
    _install_state_allows_external_tool,
    _collect_enabled_capabilities,
    _collect_blocked_capabilities,
    _remove_capability_bindings,
    _has_any_binding_for_capability,
    handle_capability_enable,
)
from ._search import (
    _display_name_from_capability_id,
    _render_source_states,
)


def _pkg():
    """Get parent package module for mock-compatible function lookups."""
    return sys.modules[__name__.rsplit(".", 1)[0]]


# ---------------------------------------------------------------------------
# Curated / policy-apply helpers
# ---------------------------------------------------------------------------

def _curated_install_metadata(install_mode_preference: str) -> Tuple[str, Dict[str, Any]]:
    pref = str(install_mode_preference or "").strip().lower()
    if pref == "remote_only":
        return "remote_only", {"transport": "http", "url": ""}
    if pref == "package:npm":
        return "package", {"registry_type": "npm", "runtime_hint": "npx", "identifier": "", "version": ""}
    if pref.startswith("package:"):
        return "package", {"registry_type": pref.split(":", 1)[1], "runtime_hint": "", "identifier": "", "version": ""}
    return "", {}


def _build_curated_records_from_policy(policy: Dict[str, Any]) -> Dict[str, Dict[str, Any]]:
    now_iso = utc_now_iso()
    out: Dict[str, Dict[str, Any]] = {}
    mcp_rows = policy.get("curated_mcp_entries") if isinstance(policy.get("curated_mcp_entries"), list) else []
    for raw in mcp_rows:
        if not isinstance(raw, dict):
            continue
        cap_id = str(raw.get("capability_id") or "").strip()
        if not cap_id.startswith("mcp:"):
            continue
        level = _normalize_policy_level(raw.get("level"), default=_LEVEL_MOUNTED)
        trust = str(raw.get("trust") or "").strip().lower()
        notes = str(raw.get("notes") or "").strip()
        risk_tags_raw = raw.get("risk_tags")
        risk_tags = [str(x).strip() for x in risk_tags_raw if str(x).strip()] if isinstance(risk_tags_raw, list) else []
        required_secrets_raw = raw.get("required_secrets")
        required_secrets = (
            [str(x).strip() for x in required_secrets_raw if str(x).strip()]
            if isinstance(required_secrets_raw, list)
            else []
        )
        install_mode, install_spec = _curated_install_metadata(str(raw.get("install_mode_preference") or ""))
        supported, unsupported_reason = _supported_external_install_record(
            {"install_mode": install_mode, "install_spec": install_spec}
        )
        hydration_needed = _needs_registry_hydration(
            cap_id,
            {"capability_id": cap_id, "kind": "mcp_toolpack", "install_mode": install_mode, "install_spec": install_spec},
        )
        qualification = _QUAL_QUALIFIED if (supported or hydration_needed) else _QUAL_UNAVAILABLE
        reasons: List[str] = []
        if level == _LEVEL_INDEXED:
            reasons.append("curated_indexed_default")
        if risk_tags:
            reasons.append("risk_tags_present")
        if unsupported_reason:
            reasons.append(unsupported_reason)
        out[cap_id] = {
            "capability_id": cap_id,
            "kind": "mcp_toolpack",
            "name": _display_name_from_capability_id(cap_id),
            "description_short": notes or f"Curated MCP capability {cap_id}",
            "tags": ["mcp", "external", "curated", *risk_tags],
            "source_id": "mcp_registry_official",
            "source_tier": "tier1",
            "source_uri": "",
            "source_record_id": cap_id.split(":", 1)[1],
            "source_record_version": "",
            "updated_at_source": now_iso,
            "last_synced_at": now_iso,
            "sync_state": "curated",
            "install_mode": install_mode,
            "install_spec": install_spec,
            "requirements": {"required_secrets": required_secrets} if required_secrets else {},
            "license": "",
            "trust_tier": trust or "tier1",
            "qualification_status": qualification,
            "qualification_reasons": reasons,
            "health_status": "curated",
            "enable_supported": bool(supported or hydration_needed),
        }

    skill_rows = policy.get("curated_skill_entries") if isinstance(policy.get("curated_skill_entries"), list) else []
    for raw in skill_rows:
        if not isinstance(raw, dict):
            continue
        cap_id = str(raw.get("capability_id") or "").strip()
        if not cap_id.startswith("skill:"):
            continue
        name = str(raw.get("name") or "").strip() or _display_name_from_capability_id(cap_id).replace(" ", "-")
        level = _normalize_policy_level(raw.get("level"), default=_LEVEL_MOUNTED)
        trust = str(raw.get("trust") or "").strip().lower()
        notes = str(raw.get("notes") or "").strip()
        source_id = str(raw.get("source_id") or "anthropic_skills").strip() or "anthropic_skills"
        source_uri = str(raw.get("source_uri") or "").strip()
        description_short = str(raw.get("description_short") or "").strip()
        license_text = str(raw.get("license") or "").strip()
        tags_raw = raw.get("tags")
        tags = [str(x).strip() for x in tags_raw if str(x).strip()] if isinstance(tags_raw, list) else []
        if source_id == "anthropic_skills" and "anthropic" not in {t.lower() for t in tags}:
            tags.append("anthropic")
        requires_raw = raw.get("requires_capabilities")
        requires_capabilities = (
            [str(x).strip() for x in requires_raw if str(x).strip()]
            if isinstance(requires_raw, list)
            else []
        )
        capsule_text = str(raw.get("capsule_text") or "").strip()
        qualification = str(raw.get("qualification_status") or "").strip().lower()
        if qualification not in _QUAL_STATES:
            qualification = _QUAL_QUALIFIED
        reasons_raw = raw.get("qualification_reasons")
        reasons = (
            [str(x).strip() for x in reasons_raw if str(x).strip()]
            if isinstance(reasons_raw, list)
            else []
        )
        if not reasons and qualification != _QUAL_QUALIFIED:
            reasons = [f"curated_{qualification}"]
        if not source_uri and source_id == "anthropic_skills":
            source_uri = f"https://github.com/anthropics/skills/tree/main/skills/{name}"
        if not description_short:
            description_short = notes or f"Curated skill {name}"
        if not capsule_text:
            capsule_text = f"Skill: {name}\nSummary: {description_short}".strip()
        out[cap_id] = {
            "capability_id": cap_id,
            "kind": "skill",
            "name": name,
            "description_short": description_short,
            "tags": ["skill", "external", "curated", *tags],
            "source_id": source_id,
            "source_tier": "tier1" if source_id == "anthropic_skills" else "tier2",
            "source_uri": source_uri,
            "source_record_id": name,
            "source_record_version": "",
            "updated_at_source": now_iso,
            "last_synced_at": now_iso,
            "sync_state": "curated",
            "install_mode": "builtin",
            "install_spec": {},
            "requirements": {},
            "license": license_text,
            "trust_tier": trust or ("tier1" if source_id == "anthropic_skills" else "tier2"),
            "qualification_status": qualification,
            "qualification_reasons": reasons,
            "health_status": "curated",
            "enable_supported": qualification != _QUAL_BLOCKED,
            "capsule_text": capsule_text,
            "requires_capabilities": requires_capabilities,
        }
    return out


def _ensure_curated_catalog_records(catalog_doc: Dict[str, Any], *, policy: Dict[str, Any]) -> bool:
    records = catalog_doc.get("records") if isinstance(catalog_doc.get("records"), dict) else {}
    if not isinstance(records, dict):
        records = {}
    curated = _build_curated_records_from_policy(policy)
    if not curated:
        return False

    changed = False
    touched_sources: set[str] = set()
    for cap_id, rec in curated.items():
        existing = records.get(cap_id) if isinstance(records.get(cap_id), dict) else None
        if isinstance(existing, dict):
            if existing == rec:
                continue
            touched_sources.add(str(existing.get("source_id") or "").strip())
            records[cap_id] = rec
            touched_sources.add(str(rec.get("source_id") or "").strip())
            changed = True
            continue
        records[cap_id] = rec
        touched_sources.add(str(rec.get("source_id") or "").strip())
        changed = True
    if not changed:
        return False

    catalog_doc["records"] = records
    sources = catalog_doc.get("sources") if isinstance(catalog_doc.get("sources"), dict) else {}
    now_iso = utc_now_iso()
    for source_id in touched_sources:
        if not source_id:
            continue
        state = sources.get(source_id) if isinstance(sources.get(source_id), dict) else _source_state_template("never")
        if str(state.get("sync_state") or "").strip() in {"", "never", "disabled"}:
            state["sync_state"] = "curated"
        if not str(state.get("last_synced_at") or "").strip():
            state["last_synced_at"] = now_iso
        state["error"] = ""
        state["staleness_seconds"] = _catalog_staleness_seconds(str(state.get("last_synced_at") or ""))
        sources[source_id] = state
    catalog_doc["sources"] = sources
    _refresh_source_record_counts(catalog_doc)
    return True


# ---------------------------------------------------------------------------
# Catalog pruning / sorting / sync
# ---------------------------------------------------------------------------

def _sync_tier_rank(tier: str) -> int:
    t = str(tier or "").strip().lower()
    if t in {"builtin", "official", "tier1"}:
        return 0
    if t in {"tier2"}:
        return 1
    return 2


def _qualification_rank(q: str) -> int:
    status = str(q or "").strip().lower()
    if status == _QUAL_QUALIFIED:
        return 0
    if status == _QUAL_UNAVAILABLE:
        return 1
    if status == _QUAL_BLOCKED:
        return 2
    return 3


def _catalog_record_sort_key(item: Dict[str, Any]) -> Tuple[int, int, int, int, str]:
    source_tier = str(item.get("source_tier") or "")
    qual = str(item.get("qualification_status") or "")
    updated_at_source = str(item.get("updated_at_source") or "")
    last_synced_at = str(item.get("last_synced_at") or "")
    name = str(item.get("name") or item.get("capability_id") or "").lower()
    updated_dt = parse_utc_iso(updated_at_source)
    synced_dt = parse_utc_iso(last_synced_at)
    updated_rank = -int(updated_dt.timestamp()) if updated_dt is not None else 0
    synced_rank = -int(synced_dt.timestamp()) if synced_dt is not None else 0
    return (
        _sync_tier_rank(source_tier),
        _qualification_rank(qual),
        updated_rank,
        synced_rank,
        name,
    )


def _catalog_max_records() -> int:
    return _quota_limit(
        "CCCC_CAPABILITY_CATALOG_MAX_RECORDS",
        20_000,
        minimum=200,
        maximum=500_000,
    )


def _prune_catalog_records(catalog: Dict[str, Any]) -> int:
    records = catalog.get("records") if isinstance(catalog.get("records"), dict) else {}
    if not isinstance(records, dict):
        return 0
    max_records = _catalog_max_records()
    if len(records) <= max_records:
        return 0
    rows: List[Tuple[str, Dict[str, Any]]] = []
    for cap_id, rec in records.items():
        if not isinstance(rec, dict):
            continue
        rows.append((str(cap_id or ""), dict(rec)))
    rows.sort(key=lambda x: _catalog_record_sort_key(x[1]))
    # Keep best candidates, then stable-id for deterministic output.
    keep_rows = rows[:max_records]
    keep_rows.sort(key=lambda x: str(x[0] or ""))
    keep_ids = {str(cap_id or "") for cap_id, _ in keep_rows}
    pruned = max(0, len(records) - len(keep_ids))
    if pruned <= 0:
        return 0
    catalog["records"] = {cid: rec for cid, rec in records.items() if str(cid or "") in keep_ids}
    return pruned


def _refresh_source_record_counts(catalog: Dict[str, Any]) -> None:
    sources = catalog.get("sources") if isinstance(catalog.get("sources"), dict) else {}
    records = catalog.get("records") if isinstance(catalog.get("records"), dict) else {}
    counts = {source_id: 0 for source_id in _SOURCE_IDS}
    for rec in records.values() if isinstance(records, dict) else []:
        if not isinstance(rec, dict):
            continue
        sid = str(rec.get("source_id") or "").strip()
        if sid in counts:
            counts[sid] += 1
    for source_id in _SOURCE_IDS:
        state = sources.get(source_id) if isinstance(sources.get(source_id), dict) else _source_state_template("never")
        state["record_count"] = int(counts.get(source_id) or 0)
        sources[source_id] = state
    catalog["sources"] = sources


def _sync_catalog(catalog_doc: Dict[str, Any], *, force: bool) -> Dict[str, Any]:
    before = json.dumps(catalog_doc, ensure_ascii=False, sort_keys=True)
    upserted: Dict[str, int] = {}

    if _env_bool("CCCC_CAPABILITY_SOURCE_MCP_REGISTRY_ENABLED", True):
        upserted["mcp_registry_official"] = int(_pkg()._sync_mcp_registry_source(catalog_doc, force=force))
    else:
        _mark_source_disabled(catalog_doc, "mcp_registry_official")
        upserted["mcp_registry_official"] = 0

    if _env_bool("CCCC_CAPABILITY_SOURCE_ANTHROPIC_SKILLS_ENABLED", True):
        upserted["anthropic_skills"] = int(_pkg()._sync_anthropic_skills_source(catalog_doc, force=force))
    else:
        _mark_source_disabled(catalog_doc, "anthropic_skills")
        upserted["anthropic_skills"] = 0

    pruned = _prune_catalog_records(catalog_doc)
    _refresh_source_record_counts(catalog_doc)

    after = json.dumps(catalog_doc, ensure_ascii=False, sort_keys=True)
    return {
        "changed": (before != after),
        "upserted": upserted,
        "upserted_total": sum(max(0, int(v or 0)) for v in upserted.values()),
        "pruned": int(pruned),
    }


def _auto_sync_catalog(catalog_doc: Dict[str, Any]) -> bool:
    return bool(_sync_catalog(catalog_doc, force=False).get("changed"))


def sync_capability_catalog_once(*, force: bool = False) -> Dict[str, Any]:
    """Run one capability catalog sync pass and persist if changed.

    Intended for daemon-owned background sync loops. Does not raise.
    """
    try:
        with _CATALOG_LOCK:
            catalog_path, catalog_doc = _pkg()._load_catalog_doc()
            result = _pkg()._sync_catalog(catalog_doc, force=bool(force))
            if bool(result.get("changed")):
                _pkg()._save_catalog_doc(catalog_path, catalog_doc)
        return {
            "ok": True,
            "changed": bool(result.get("changed")),
            "upserted_total": int(result.get("upserted_total") or 0),
            "upserted": result.get("upserted") if isinstance(result.get("upserted"), dict) else {},
            "pruned": int(result.get("pruned") or 0),
            "source_states": _render_source_states(catalog_doc),
        }
    except Exception as e:
        return {
            "ok": False,
            "changed": False,
            "upserted_total": 0,
            "upserted": {},
            "pruned": 0,
            "error": str(e),
            "source_states": {},
        }


# ---------------------------------------------------------------------------
# Profile / autoload helpers
# ---------------------------------------------------------------------------

def _normalize_capability_id_list(raw: Any) -> List[str]:
    out: List[str] = []
    if not isinstance(raw, list):
        return out
    seen: set[str] = set()
    for item in raw:
        cap_id = str(item or "").strip()
        if not cap_id or cap_id in seen:
            continue
        seen.add(cap_id)
        out.append(cap_id)
    return out


def _normalize_profile_capability_defaults(raw: Any) -> Dict[str, Any]:
    if not isinstance(raw, dict):
        return {"autoload_capabilities": [], "default_scope": "actor", "session_ttl_seconds": 3600}
    scope = str(raw.get("default_scope") or "actor").strip().lower()
    if scope not in {"actor", "session"}:
        scope = "actor"
    try:
        ttl = int(raw.get("session_ttl_seconds") or 3600)
    except Exception:
        ttl = 3600
    ttl = max(60, min(ttl, 24 * 3600))
    autoload = _normalize_capability_id_list(raw.get("autoload_capabilities"))
    return {"autoload_capabilities": autoload, "default_scope": scope, "session_ttl_seconds": ttl}


def apply_actor_profile_capability_defaults(
    *,
    group_id: str,
    actor_id: str,
    profile_id: str,
    capability_defaults: Any,
) -> Dict[str, Any]:
    cfg = _normalize_profile_capability_defaults(capability_defaults)
    requested = list(cfg.get("autoload_capabilities") or [])
    scope = str(cfg.get("default_scope") or "actor")
    ttl_seconds = int(cfg.get("session_ttl_seconds") or 3600)
    if not requested:
        return {
            "requested_count": 0,
            "applied": [],
            "skipped": [],
            "scope": scope,
            "ttl_seconds": ttl_seconds,
        }

    applied: List[str] = []
    skipped: List[Dict[str, str]] = []
    for cap_id in requested:
        resp = handle_capability_enable(
            {
                "group_id": group_id,
                "by": "user",
                "actor_id": actor_id,
                "capability_id": cap_id,
                "scope": scope,
                "enabled": True,
                "ttl_seconds": ttl_seconds,
                "reason": f"profile_default:{profile_id}",
            }
        )
        if not bool(resp.ok):
            code = str((resp.error.code if resp.error else "") or "enable_failed")
            skipped.append({"capability_id": cap_id, "reason": code})
            continue
        result = resp.result if isinstance(resp.result, dict) else {}
        state = str(result.get("state") or "").strip().lower()
        if state == "ready" and bool(result.get("enabled", True)):
            applied.append(cap_id)
            continue
        reason = str(result.get("reason") or state or "not_ready")
        skipped.append({"capability_id": cap_id, "reason": reason})
    return {
        "requested_count": len(requested),
        "applied": applied,
        "skipped": skipped,
        "scope": scope,
        "ttl_seconds": ttl_seconds,
    }


def apply_actor_capability_autoload(
    *,
    group_id: str,
    actor_id: str,
    autoload_capabilities: Any,
    scope: str = "actor",
    ttl_seconds: int = 3600,
    reason: str = "actor_autoload",
) -> Dict[str, Any]:
    requested = _normalize_capability_id_list(autoload_capabilities)
    eff_scope = str(scope or "actor").strip().lower()
    if eff_scope not in {"actor", "session"}:
        eff_scope = "actor"
    try:
        eff_ttl = int(ttl_seconds or 3600)
    except Exception:
        eff_ttl = 3600
    eff_ttl = max(60, min(eff_ttl, 24 * 3600))
    if not requested:
        return {
            "requested_count": 0,
            "applied": [],
            "skipped": [],
            "scope": eff_scope,
            "ttl_seconds": eff_ttl,
        }

    applied: List[str] = []
    skipped: List[Dict[str, str]] = []
    for cap_id in requested:
        resp = handle_capability_enable(
            {
                "group_id": group_id,
                "by": "user",
                "actor_id": actor_id,
                "capability_id": cap_id,
                "scope": eff_scope,
                "enabled": True,
                "ttl_seconds": eff_ttl,
                "reason": reason,
            }
        )
        if not bool(resp.ok):
            code = str((resp.error.code if resp.error else "") or "enable_failed")
            skipped.append({"capability_id": cap_id, "reason": code})
            continue
        result = resp.result if isinstance(resp.result, dict) else {}
        state = str(result.get("state") or "").strip().lower()
        if state == "ready" and bool(result.get("enabled", True)):
            applied.append(cap_id)
            continue
        reason_code = str(result.get("reason") or state or "not_ready")
        skipped.append({"capability_id": cap_id, "reason": reason_code})
    return {
        "requested_count": len(requested),
        "applied": applied,
        "skipped": skipped,
        "scope": eff_scope,
        "ttl_seconds": eff_ttl,
    }


# ---------------------------------------------------------------------------
# handle_capability_uninstall
# ---------------------------------------------------------------------------

def handle_capability_uninstall(args: Dict[str, Any]) -> DaemonResponse:
    group_id = str(args.get("group_id") or "").strip()
    by = str(args.get("by") or args.get("actor_id") or "").strip()
    actor_id = str(args.get("actor_id") or by).strip()
    capability_id = str(args.get("capability_id") or "").strip()
    reason = str(args.get("reason") or "").strip()
    if len(reason) > 280:
        reason = reason[:280]
    action_id = f"cact_{uuid.uuid4().hex[:16]}"

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
                op="capability_uninstall",
                group_id=group_id,
                actor_id=actor_id,
                by=by,
                capability_id=capability_id,
                scope="group",
                enabled=False,
                outcome=outcome,
                details=payload,
            )
        except Exception:
            pass

    try:
        group = _ensure_group(group_id)
        if not capability_id:
            _audit("denied", state="denied", error_code="missing_capability_id")
            return _error("missing_capability_id", "missing capability_id", details={"action_id": action_id})
        if by != "user" and (not _is_foreman(group, by)):
            _audit("denied", state="denied", error_code="permission_denied")
            return _error(
                "permission_denied",
                "only user or foreman can uninstall capability",
                details={"action_id": action_id},
            )

        removed_bindings = 0
        has_remaining_binding = False
        with _STATE_LOCK:
            state_path, state_doc = _load_state_doc()
            removed_bindings = _remove_capability_bindings(
                state_doc,
                group_id=group_id,
                capability_id=capability_id,
            )
            has_remaining_binding = _has_any_binding_for_capability(state_doc, capability_id=capability_id)
            if removed_bindings > 0:
                _save_state_doc(state_path, state_doc)

        removed_installation = False
        removed_runtime_bindings = 0
        cleanup_skipped_reason = ""
        with _RUNTIME_LOCK:
            runtime_path, runtime_doc = _load_runtime_doc()
            removed_runtime_bindings = _remove_runtime_group_capability_bindings(
                runtime_doc,
                group_id=group_id,
                capability_id=capability_id,
            )
            runtime_changed = bool(removed_runtime_bindings > 0)
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
                    runtime_changed = True
            if runtime_changed:
                _save_runtime_doc(runtime_path, runtime_doc)

        refresh_required = bool(removed_bindings > 0 or removed_installation or removed_runtime_bindings > 0)
        _audit(
            "ready",
            state="ready",
            details={
                "removed_bindings": int(removed_bindings),
                "removed_installation": bool(removed_installation),
                "removed_runtime_bindings": int(removed_runtime_bindings),
                "cleanup_skipped_reason": cleanup_skipped_reason,
            },
        )
        result: Dict[str, Any] = {
            "action_id": action_id,
            "group_id": group_id,
            "actor_id": actor_id,
            "capability_id": capability_id,
            "state": "ready",
            "removed_bindings": int(removed_bindings),
            "removed_installation": bool(removed_installation),
            "removed_runtime_bindings": int(removed_runtime_bindings),
            "refresh_required": refresh_required,
        }
        if cleanup_skipped_reason:
            result["cleanup_skipped_reason"] = cleanup_skipped_reason
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
        _audit("failed", state="failed", error_code="capability_uninstall_invalid", details={"error": message})
        return _error("capability_uninstall_invalid", message, details={"action_id": action_id})
    except Exception as e:
        _audit("failed", state="failed", error_code="capability_uninstall_failed", details={"error": str(e)})
        return _error("capability_uninstall_failed", str(e), details={"action_id": action_id})


# ---------------------------------------------------------------------------
# handle_capability_tool_call
# ---------------------------------------------------------------------------

def handle_capability_tool_call(args: Dict[str, Any]) -> DaemonResponse:
    group_id = str(args.get("group_id") or "").strip()
    by = str(args.get("by") or args.get("actor_id") or "").strip()
    actor_id = str(args.get("actor_id") or by).strip()
    capability_id_hint = str(args.get("capability_id") or "").strip()
    tool_name = str(args.get("tool_name") or "").strip()
    arguments = args.get("arguments") if isinstance(args.get("arguments"), dict) else {}

    try:
        group = _ensure_group(group_id)
        if by and by != "user" and actor_id and actor_id != by:
            return _error("permission_denied", "actor can only call tools as self")
        if not actor_id:
            return _error("missing_actor_id", "missing actor_id")
        if not tool_name:
            return _error("missing_tool_name", "missing tool_name")
        if actor_id != "user":
            actors = group.doc.get("actors") if isinstance(group.doc.get("actors"), list) else []
            if actor_id not in {str(a.get("id") or "") for a in actors if isinstance(a, dict)}:
                return _error("actor_not_found", f"actor not found in group: {actor_id}")

        with _STATE_LOCK:
            state_path, state_doc = _load_state_doc()
            enabled_caps, mutated = _collect_enabled_capabilities(state_doc, group_id=group_id, actor_id=actor_id)
            blocked_caps, blocked_mutated = _collect_blocked_capabilities(state_doc, group_id=group_id)
            if mutated or blocked_mutated:
                _save_state_doc(state_path, state_doc)

        enabled_external = [cid for cid in enabled_caps if cid not in BUILTIN_CAPABILITY_PACKS]
        if capability_id_hint and capability_id_hint in BUILTIN_CAPABILITY_PACKS:
            return _error("capability_tool_not_found", f"capability is not external: {capability_id_hint}")
        if capability_id_hint and isinstance(blocked_caps.get(capability_id_hint), dict):
            block_entry = blocked_caps.get(capability_id_hint) if isinstance(blocked_caps.get(capability_id_hint), dict) else {}
            scope_token = str(block_entry.get("scope") or "group").strip().lower()
            reason_code = "blocked_by_global_policy" if scope_token == "global" else "blocked_by_group_policy"
            details = {"capability_id": capability_id_hint, "tool_name": tool_name, "blocked_scope": scope_token}
            reason_text = str(block_entry.get("reason") or "").strip()
            if reason_text:
                details["blocked_reason"] = reason_text
            return _error(reason_code, f"capability blocked: {capability_id_hint}", details=details)
        if capability_id_hint and capability_id_hint not in set(enabled_external):
            return _error(
                "capability_tool_not_found",
                f"capability not enabled for actor: {capability_id_hint}",
                details={"capability_id": capability_id_hint, "tool_name": tool_name},
            )
        candidate_caps = [
            cid
            for cid in ([capability_id_hint] if capability_id_hint else list(enabled_external))
            if not isinstance(blocked_caps.get(cid), dict)
        ]
        if not candidate_caps:
            blocked_ids = [cid for cid in enabled_external if isinstance(blocked_caps.get(cid), dict)]
            if blocked_ids:
                return _error(
                    "blocked_by_group_policy",
                    "all enabled external capabilities are blocked",
                    details={"tool_name": tool_name, "blocked_capabilities": blocked_ids},
                )

        target_capability_id = ""
        target_artifact_id = ""
        target_install: Dict[str, Any] = {}
        real_tool_name = ""
        resolved_tool_name = tool_name
        tool_aliases = set(_tool_name_aliases(tool_name))
        if not tool_aliases:
            tool_aliases = {tool_name}

        with _RUNTIME_LOCK:
            _, runtime_doc = _load_runtime_doc()
            artifacts = _runtime_artifacts(runtime_doc)
            capability_artifacts = _runtime_capability_artifacts(runtime_doc)
            actor_bindings = _runtime_actor_bindings(runtime_doc)
            per_group_bindings = (
                actor_bindings.get(group_id)
                if isinstance(actor_bindings.get(group_id), dict)
                else {}
            )
            per_actor_bindings = (
                per_group_bindings.get(actor_id)
                if isinstance(per_group_bindings.get(actor_id), dict)
                else {}
            )
            matches: List[Tuple[str, str, Dict[str, Any], str, str]] = []
            available_by_cap: Dict[str, set[str]] = {}
            direct_fallback: Optional[Tuple[str, str, Dict[str, Any]]] = None
            for capability_id in candidate_caps:
                binding = per_actor_bindings.get(capability_id) if isinstance(per_actor_bindings, dict) else None
                binding_state = str((binding or {}).get("state") or "").strip() if isinstance(binding, dict) else ""
                if not _binding_state_allows_external_tool(binding_state):
                    continue
                artifact_id = str((binding or {}).get("artifact_id") or "").strip() if isinstance(binding, dict) else ""
                if not artifact_id:
                    artifact_id = str(capability_artifacts.get(capability_id) or "").strip()
                install = artifacts.get(artifact_id) if isinstance(artifacts, dict) else None
                if not isinstance(install, dict):
                    continue
                if not _install_state_allows_external_tool(install.get("state")):
                    continue
                tools = install.get("tools") if isinstance(install.get("tools"), list) else []
                if not tools:
                    if capability_id_hint and capability_id == capability_id_hint:
                        direct_fallback = (capability_id, artifact_id, install)
                    continue
                for tool in tools:
                    if not isinstance(tool, dict):
                        continue
                    synthetic_name = str(tool.get("name") or "").strip()
                    real_name = str(tool.get("real_tool_name") or "").strip()
                    if not synthetic_name or not real_name:
                        continue
                    names = available_by_cap.setdefault(capability_id, set())
                    names.add(synthetic_name)
                    names.add(real_name)
                    if (
                        tool_name != synthetic_name
                        and tool_name != real_name
                        and synthetic_name not in tool_aliases
                        and real_name not in tool_aliases
                    ):
                        continue
                    matches.append((capability_id, artifact_id, install, real_name, synthetic_name))

            if not matches:
                if capability_id_hint and isinstance(direct_fallback, tuple):
                    target_capability_id, target_artifact_id, target_install = direct_fallback
                    real_tool_name = tool_name
                    resolved_tool_name = tool_name
                else:
                    details: Dict[str, Any] = {}
                    if capability_id_hint:
                        available = sorted(available_by_cap.get(capability_id_hint) or [])
                        if available:
                            details["available_tools"] = available[:64]
                        details["capability_id"] = capability_id_hint
                    return _error("capability_tool_not_found", f"tool not found or not enabled: {tool_name}", details=details)
            else:
                if len(matches) > 1:
                    candidates = [
                        {
                            "capability_id": cap_id,
                            "tool_name": synthetic,
                            "real_tool_name": real,
                        }
                        for cap_id, _, _, real, synthetic in matches
                    ]
                    return _error(
                        "capability_tool_ambiguous",
                        f"tool resolves to multiple enabled capabilities: {tool_name}",
                        details={"tool_name": tool_name, "candidates": candidates},
                    )
                target_capability_id, target_artifact_id, target_install, real_tool_name, resolved_tool_name = matches[0]

        try:
            result, resolved_real_tool_name = _pkg()._invoke_installed_external_tool_with_aliases(
                target_install,
                requested_tool_name=real_tool_name,
                arguments=arguments,
            )
        except Exception as e:
            with _RUNTIME_LOCK:
                # Update last error for observability; do not change enabled scope on transient failures.
                path, doc = _load_runtime_doc()
                artifacts2 = _runtime_artifacts(doc)
                install2 = artifacts2.get(target_artifact_id) if isinstance(artifacts2, dict) else None
                if isinstance(install2, dict):
                    install2["last_error"] = str(e)
                    install2["updated_at"] = utc_now_iso()
                    artifacts2[target_artifact_id] = install2
                    doc["artifacts"] = artifacts2
                    _set_runtime_actor_binding(
                        doc,
                        group_id=group_id,
                        actor_id=actor_id,
                        capability_id=target_capability_id,
                        artifact_id=target_artifact_id,
                        state="tool_call_failed",
                        last_error=str(e),
                    )
                    _save_runtime_doc(path, doc)
            return _error("capability_tool_call_failed", str(e))

        with _RUNTIME_LOCK:
            path, doc = _load_runtime_doc()
            artifacts2 = _runtime_artifacts(doc)
            install2 = artifacts2.get(target_artifact_id) if isinstance(artifacts2, dict) else None
            if isinstance(install2, dict):
                tools2 = install2.get("tools") if isinstance(install2.get("tools"), list) else []
                has_tool = any(
                    isinstance(item, dict)
                    and str(item.get("real_tool_name") or "").strip() == str(resolved_real_tool_name or "").strip()
                    for item in tools2
                )
                if (not has_tool) and str(resolved_real_tool_name or "").strip():
                    used_names = {
                        str(item.get("name") or "").strip()
                        for item in tools2
                        if isinstance(item, dict)
                    }
                    tools2.append(
                        {
                            "name": _build_synthetic_tool_name(
                                target_capability_id,
                                str(resolved_real_tool_name or ""),
                                used=used_names,
                            ),
                            "real_tool_name": str(resolved_real_tool_name or ""),
                            "description": f"{resolved_real_tool_name} tool",
                            "inputSchema": {"type": "object", "properties": {}, "required": []},
                        }
                    )
                install2["tools"] = tools2
                install2["last_error"] = ""
                if str(install2.get("state") or "").strip().lower() == "installed_degraded" and tools2:
                    install2["state"] = "installed"
                install2["updated_at"] = utc_now_iso()
                artifacts2[target_artifact_id] = install2
                doc["artifacts"] = artifacts2
            _set_runtime_actor_binding(
                doc,
                group_id=group_id,
                actor_id=actor_id,
                capability_id=target_capability_id,
                artifact_id=target_artifact_id,
                state="ready",
                last_error="",
            )
            _record_runtime_recent_success(
                doc,
                capability_id=target_capability_id,
                group_id=group_id,
                actor_id=actor_id,
                action="tool_call",
            )
            _save_runtime_doc(path, doc)

        return DaemonResponse(
            ok=True,
            result={
                "tool_name": tool_name,
                "resolved_tool_name": resolved_tool_name,
                "real_tool_name": resolved_real_tool_name,
                "capability_id": target_capability_id,
                "result": result,
            },
        )
    except LookupError as e:
        return _error("group_not_found", str(e))
    except Exception as e:
        return _error("capability_tool_call_failed", str(e))
