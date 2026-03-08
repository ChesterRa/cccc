"""Capability search, list, filter, overview, and state-query handlers."""

from __future__ import annotations

import re
import sys
from datetime import datetime, timedelta, timezone
from typing import Any, Dict, List, Optional, Tuple

from ....contracts.v1 import DaemonError, DaemonResponse
from ....kernel.actors import get_effective_role
from ....kernel.capabilities import (
    BUILTIN_CAPABILITY_PACKS,
    CORE_TOOL_NAMES,
    all_builtin_pack_ids,
    resolve_visible_tool_names,
)
from ....kernel.context import ContextStorage
from ....kernel.group import load_group
from ....util.time import parse_utc_iso, utc_now_iso

from ._common import (
    _SOURCE_IDS,
    _STATE_LOCK,
    _CATALOG_LOCK,
    _RUNTIME_LOCK,
    _POLICY_LOCK,
    _LEVEL_INDEXED,
    _LEVEL_MOUNTED,
    _LEVEL_PINNED,
    _QUAL_QUALIFIED,
    _QUAL_BLOCKED,
    _QUAL_UNAVAILABLE,
    _error,
    _ensure_group,
    _is_foreman,
    _env_int,
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
    _runtime_recent_success,
    _record_runtime_recent_success,
)
from ._policy import (
    _normalize_policy_level,
    _policy_level_visible,
    _allowlist_policy,
    _allowlist_effective_snapshot,
)
from ._remote import (
    _tokenize_search_text,
    _remote_search_mcp_registry_records,
    _remote_search_skill_records,
)
from ._install import (
    _catalog_staleness_seconds,
)


def _pkg():
    """Get parent package module for mock-compatible function lookups."""
    return sys.modules[__name__.rsplit(".", 1)[0]]


def _resolve_actor_role(group: Any, actor_id: str) -> str:
    aid = str(actor_id or "").strip()
    if not aid or aid == "user":
        return ""
    try:
        return str(get_effective_role(group, aid) or "").strip().lower()
    except Exception:
        return ""


def _effective_policy_level(
    policy: Dict[str, Any],
    *,
    capability_id: str,
    kind: str,
    source_id: str,
    actor_role: str = "",
) -> str:
    cid = str(capability_id or "").strip()
    source = str(source_id or "").strip()
    kind_norm = str(kind or "").strip().lower()
    role = str(actor_role or "").strip().lower()

    source_levels = policy.get("source_levels") if isinstance(policy.get("source_levels"), dict) else {}
    capability_levels = policy.get("capability_levels") if isinstance(policy.get("capability_levels"), dict) else {}
    skill_source_levels = (
        policy.get("skill_source_levels") if isinstance(policy.get("skill_source_levels"), dict) else {}
    )
    role_pinned = policy.get("role_pinned") if isinstance(policy.get("role_pinned"), dict) else {}

    level = _normalize_policy_level(source_levels.get(source), default=_LEVEL_MOUNTED if not source else _LEVEL_INDEXED)
    if kind_norm == "skill":
        level = _normalize_policy_level(skill_source_levels.get(source, level), default=level)
    if cid and cid in capability_levels:
        level = _normalize_policy_level(capability_levels.get(cid), default=level)
    if role:
        role_caps = role_pinned.get(role)
        if isinstance(role_caps, set) and cid in role_caps:
            level = _LEVEL_PINNED
        elif isinstance(role_caps, list) and cid in {str(x or "").strip() for x in role_caps}:
            level = _LEVEL_PINNED
    return level


def _display_name_from_capability_id(capability_id: str) -> str:
    cid = str(capability_id or "").strip()
    if not cid:
        return "capability"
    if cid.startswith("skill:"):
        token = cid.split(":")[-1]
    elif cid.startswith("mcp:"):
        token = cid.split(":", 1)[1]
        if "/" in token:
            token = token.rsplit("/", 1)[-1]
    else:
        token = cid
    token = token.replace("_", " ").replace("-", " ").strip()
    return token or cid


def _external_policy_evidence(*, policy: Dict[str, Any], source_id: str, policy_level: str) -> Dict[str, str]:
    if str(source_id or "").strip() not in _SOURCE_IDS:
        return {}
    if _normalize_policy_level(policy_level) != _LEVEL_INDEXED:
        return {}
    mode = _pkg()._external_capability_safety_mode_from_policy(policy)
    if mode != "conservative":
        return {}
    return {
        "policy_source": "external_capability_safety_mode",
        "policy_mode": mode,
        "next_step": "switch_external_capability_safety_mode_or_adjust_allowlist",
    }


def _build_readiness_preview(
    *,
    rec: Dict[str, Any],
    capability_id: str,
    policy: Dict[str, Any],
    policy_level: str,
    blocked_reason_code: str = "",
    qualification_status: str,
    enable_supported: bool,
    cached_install_state: str = "",
    install_error_code: str = "",
    recent_success: Optional[Dict[str, Any]] = None,
) -> Dict[str, Any]:
    source_id = str(rec.get("source_id") or "").strip()
    install_mode = str(rec.get("install_mode") or "").strip()
    preview_basis: List[str] = []
    required_env: List[str] = []
    missing_env: List[str] = []
    try:
        required_env = _pkg()._required_environment_names(rec)
    except Exception:
        required_env = []
    if required_env:
        preview_basis.append("catalog_record")
        try:
            missing_env = _pkg()._missing_required_environment_names(rec)
        except Exception:
            missing_env = []
        if missing_env:
            preview_basis.append("local_env")

    has_recent_success = isinstance(recent_success, dict) and bool(recent_success)
    if has_recent_success or cached_install_state or install_error_code:
        preview_basis.append("runtime_cache")
    preview_basis.append("policy")
    preview_basis = sorted({x for x in preview_basis if str(x).strip()})

    preview: Dict[str, Any] = {
        "preview_status": "needs_inspect",
        "next_step": "inspect",
        "preview_basis": preview_basis,
        "policy_level": _normalize_policy_level(policy_level),
        "enable_supported": bool(enable_supported),
        "install_mode": install_mode,
    }
    if required_env:
        preview["required_env"] = sorted({str(x).strip() for x in required_env if str(x).strip()})
    if missing_env:
        preview["missing_env"] = sorted({str(x).strip() for x in missing_env if str(x).strip()})
    if cached_install_state:
        preview["cached_install_state"] = cached_install_state
    if install_error_code:
        preview["install_error_code"] = install_error_code
    if has_recent_success:
        preview["recent_success"] = dict(recent_success or {})

    if str(blocked_reason_code or "").strip():
        code = str(blocked_reason_code or "").strip()
        preview["preview_status"] = "blocked"
        preview["enable_block_reason"] = code
        if code == "policy_level_indexed":
            preview.update(_external_policy_evidence(policy=policy, source_id=source_id, policy_level=policy_level))
        if "next_step" not in preview:
            preview["next_step"] = "fix_known_blocker"
        return preview

    if not _policy_level_visible(policy_level):
        preview["preview_status"] = "blocked"
        preview["enable_block_reason"] = "policy_level_indexed"
        preview.update(_external_policy_evidence(policy=policy, source_id=source_id, policy_level=policy_level))
        if "next_step" not in preview:
            preview["next_step"] = "fix_known_blocker"
        return preview

    if qualification_status == _QUAL_BLOCKED:
        preview["preview_status"] = "blocked"
        preview["enable_block_reason"] = "qualification_blocked"
        preview["next_step"] = "fix_known_blocker"
        return preview

    if missing_env:
        preview["preview_status"] = "blocked"
        preview["enable_block_reason"] = "missing_required_env"
        preview["next_step"] = "fix_known_blocker"
        return preview

    if enable_supported or has_recent_success or cached_install_state in {"installed", "installed_degraded"}:
        preview["preview_status"] = "enableable"
        preview["next_step"] = "enable"
        return preview

    return preview


def _build_builtin_search_records() -> List[Dict[str, Any]]:
    out: List[Dict[str, Any]] = []
    for cap_id in all_builtin_pack_ids():
        pack = BUILTIN_CAPABILITY_PACKS.get(cap_id, {})
        out.append(
            {
                "capability_id": cap_id,
                "kind": "mcp_toolpack",
                "name": str(pack.get("title") or cap_id),
                "description_short": str(pack.get("description") or ""),
                "tags": list(pack.get("tags") or []),
                "source_id": "cccc_builtin",
                "source_tier": "builtin",
                "source_uri": "",
                "trust_tier": "builtin",
                "qualification_status": "qualified",
                "sync_state": "fresh",
                "enable_supported": True,
                "tool_count": len(tuple(pack.get("tool_names") or ())),
                "tool_names": [str(x).strip() for x in tuple(pack.get("tool_names") or ()) if str(x).strip()],
            }
        )
    return out


def _search_matches(query: str, item: Dict[str, Any]) -> bool:
    q = str(query or "").strip().lower()
    if not q:
        return True
    fields = [
        str(item.get("capability_id") or ""),
        str(item.get("name") or ""),
        str(item.get("description_short") or ""),
        " ".join(str(x) for x in (item.get("tags") or [])),
    ]
    haystack = " ".join(fields).lower()
    if q in haystack:
        return True
    tokens = _tokenize_search_text(q)
    if not tokens:
        return False
    return any(tok in haystack for tok in tokens)


def _score_item_tokens(item: Dict[str, Any], tokens: List[str]) -> int:
    if not tokens:
        return 0
    fields = [
        str(item.get("capability_id") or ""),
        str(item.get("name") or ""),
        str(item.get("description_short") or ""),
        " ".join(str(x) for x in (item.get("tags") or [])),
        " ".join(str(x) for x in (item.get("tool_names") or [])),
    ]
    haystack = " ".join(fields).lower()
    if not haystack:
        return 0
    score = 0
    for tok in tokens:
        if tok in haystack:
            score += 1
    return score


def _canonicalize_actor_hint(actor_id: str) -> str:
    s = str(actor_id or "").strip()
    if not s:
        return ""
    s = re.sub(r"(?<=[a-z0-9])([A-Z])", r"-\1", s)
    s = s.replace("_", "-").replace(" ", "-")
    s = re.sub(r"-{2,}", "-", s)
    s = s.strip("-")
    return s.lower()


def _context_search_tokens(*, group_id: str, actor_id: str) -> List[str]:
    gid = str(group_id or "").strip()
    aid = str(actor_id or "").strip()
    if not gid or not aid:
        return []
    group = load_group(gid)
    if group is None:
        return []
    try:
        storage = ContextStorage(group)
        tasks = storage.list_tasks()
        task_by_id = {str(t.id or "").strip(): t for t in tasks}
        agents_state = storage.load_agents()

        actor_norm = _canonicalize_actor_hint(aid)
        actor_states = [
            a
            for a in agents_state.agents
            if str(getattr(a, "id", "") or "").strip() in {aid, actor_norm}
        ]
        task_focus_text: List[str] = []
        for st in actor_states:
            active_task_id = str(getattr(st, "active_task_id", "") or "").strip()
            if active_task_id:
                task = task_by_id.get(active_task_id)
                if task is not None:
                    task_focus_text.append(str(getattr(task, "name", "") or ""))
                    task_focus_text.append(str(getattr(task, "goal", "") or ""))
            task_focus_text.append(str(getattr(st, "focus", "") or ""))

        if not task_focus_text:
            for t in tasks:
                if str(getattr(t, "assignee", "") or "").strip() in {aid, actor_norm}:
                    task_focus_text.append(str(getattr(t, "name", "") or ""))
                    task_focus_text.append(str(getattr(t, "goal", "") or ""))

        out: List[str] = []
        seen: set[str] = set()
        for txt in task_focus_text:
            for tok in _tokenize_search_text(txt):
                if tok in seen:
                    continue
                seen.add(tok)
                out.append(tok)
                if len(out) >= 20:
                    return out
        return out
    except Exception:
        return []


def _role_preferred_pack_ids(actor_role: str) -> List[str]:
    role = str(actor_role or "").strip().lower()
    if role == "foreman":
        return [
            "pack:group-runtime",
            "pack:automation",
            "pack:context-advanced",
        ]
    if role == "peer":
        return [
            "pack:space",
            "pack:file-im",
        ]
    return [
        "pack:group-runtime",
        "pack:space",
    ]


def _render_source_states(catalog_doc: Dict[str, Any]) -> Dict[str, Dict[str, Any]]:
    sources_raw = catalog_doc.get("sources")
    sources = sources_raw if isinstance(sources_raw, dict) else {}
    out: Dict[str, Dict[str, Any]] = {}
    for source_id in _SOURCE_IDS:
        state_raw = sources.get(source_id)
        state = dict(state_raw) if isinstance(state_raw, dict) else _source_state_template("never")
        state["staleness_seconds"] = _catalog_staleness_seconds(str(state.get("last_synced_at") or ""))
        out[source_id] = state
    return out


def handle_capability_overview(args: Dict[str, Any]) -> DaemonResponse:
    query = str(args.get("query") or "").strip()
    limit = max(1, min(int(args.get("limit") or 400), 2000))
    include_indexed = bool(args.get("include_indexed", True))

    try:
        with _POLICY_LOCK:
            snapshot = _allowlist_effective_snapshot()
            policy = _allowlist_policy()
        effective_doc = snapshot.get("effective") if isinstance(snapshot.get("effective"), dict) else {}
        source_levels = policy.get("source_levels") if isinstance(policy.get("source_levels"), dict) else {}

        with _CATALOG_LOCK:
            catalog_path, catalog_doc = _pkg()._load_catalog_doc()
            if _pkg()._ensure_curated_catalog_records(catalog_doc, policy=policy):
                _pkg()._save_catalog_doc(catalog_path, catalog_doc)
            source_states = _render_source_states(catalog_doc)
            records = catalog_doc.get("records") if isinstance(catalog_doc.get("records"), dict) else {}
            external_rows = [dict(v) for v in records.values() if isinstance(v, dict)]

        with _STATE_LOCK:
            state_path, state_doc = _load_state_doc()
            blocked_caps_all, blocked_mutated = _pkg()._collect_blocked_capabilities(state_doc, group_id="")
            if blocked_mutated:
                _save_state_doc(state_path, state_doc)

        blocked_global: Dict[str, Dict[str, Any]] = {}
        for cap_id, row in blocked_caps_all.items():
            if not isinstance(row, dict):
                continue
            if str(row.get("scope") or "").strip().lower() != "global":
                continue
            blocked_global[str(cap_id)] = dict(row)

        with _RUNTIME_LOCK:
            _, runtime_doc = _load_runtime_doc()
            capability_artifacts = _runtime_capability_artifacts(runtime_doc)
            artifacts = _runtime_artifacts(runtime_doc)
            recent_success = _runtime_recent_success(runtime_doc)

        entries: Dict[str, Dict[str, Any]] = {}

        def _upsert_entry(row: Dict[str, Any]) -> None:
            cap_id = str(row.get("capability_id") or "").strip()
            if not cap_id:
                return
            current = entries.get(cap_id) if isinstance(entries.get(cap_id), dict) else {}
            merged = dict(current)
            merged.update({k: v for k, v in row.items() if v not in (None, "", [], {}) or k not in merged})
            merged["capability_id"] = cap_id
            entries[cap_id] = merged

        for row in _build_builtin_search_records():
            if isinstance(row, dict):
                _upsert_entry(row)
        for row in external_rows:
            if isinstance(row, dict):
                _upsert_entry(row)

        for cap_id, row in recent_success.items():
            cid = str(cap_id or "").strip()
            if not cid:
                continue
            if cid in entries:
                continue
            kind = "skill" if cid.startswith("skill:") else ("mcp_toolpack" if cid.startswith("mcp:") else "")
            _upsert_entry(
                {
                    "capability_id": cid,
                    "kind": kind,
                    "name": _display_name_from_capability_id(cid),
                    "description_short": "Recently successful capability",
                    "source_id": "runtime_recent_success",
                    "sync_state": "runtime",
                    "qualification_status": _QUAL_QUALIFIED,
                }
            )

        source_cfg_map: Dict[str, Dict[str, Any]] = {}
        effective_sources = effective_doc.get("sources") if isinstance(effective_doc.get("sources"), list) else []
        for row in effective_sources:
            if not isinstance(row, dict):
                continue
            sid = str(row.get("source_id") or "").strip()
            if not sid:
                continue
            source_cfg_map[sid] = {
                "enabled": bool(row.get("enabled", True)),
                "rationale": str(row.get("rationale") or "").strip(),
            }

        rows: List[Dict[str, Any]] = []
        for cap_id, rec in entries.items():
            kind = str(rec.get("kind") or "").strip()
            source_id = str(rec.get("source_id") or "").strip()
            policy_level = _effective_policy_level(
                policy,
                capability_id=cap_id,
                kind=kind,
                source_id=source_id,
                actor_role="",
            )
            if (not include_indexed) and (not _policy_level_visible(policy_level)):
                continue
            blocked = blocked_global.get(cap_id) if isinstance(blocked_global.get(cap_id), dict) else None
            enable_supported = _pkg()._record_enable_supported(rec, capability_id=cap_id)
            recent_row = recent_success.get(cap_id) if isinstance(recent_success.get(cap_id), dict) else {}
            artifact_id = str(capability_artifacts.get(cap_id) or "").strip()
            artifact = artifacts.get(artifact_id) if artifact_id and isinstance(artifacts.get(artifact_id), dict) else {}
            install_state = str(artifact.get("state") or "").strip()
            install_error_code = str(artifact.get("last_error_code") or "").strip()
            install_error = str(artifact.get("last_error") or "").strip()
            recent_payload: Dict[str, Any] = {}
            if recent_row:
                recent_payload = {
                    "success_count": int(recent_row.get("success_count") or 0),
                    "last_success_at": str(recent_row.get("last_success_at") or ""),
                    "last_group_id": str(recent_row.get("last_group_id") or ""),
                    "last_actor_id": str(recent_row.get("last_actor_id") or ""),
                    "last_action": str(recent_row.get("last_action") or ""),
                }
            blocked_reason = str((blocked or {}).get("reason") or "").strip() if isinstance(blocked, dict) else ""
            qualification_status = str(rec.get("qualification_status") or _QUAL_QUALIFIED)
            blocked_reason_code = "blocked_by_global_policy" if bool(blocked) else ""
            item: Dict[str, Any] = {
                "capability_id": cap_id,
                "kind": kind,
                "name": str(rec.get("name") or _display_name_from_capability_id(cap_id)),
                "description_short": str(rec.get("description_short") or ""),
                "source_id": source_id,
                "source_uri": str(rec.get("source_uri") or ""),
                "source_tier": str(rec.get("source_tier") or ""),
                "trust_tier": str(rec.get("trust_tier") or ""),
                "license": str(rec.get("license") or ""),
                "sync_state": str(rec.get("sync_state") or ""),
                "policy_level": policy_level,
                "enable_supported": bool(enable_supported),
                "qualification_status": qualification_status,
                "install_mode": str(rec.get("install_mode") or ""),
                "tags": [str(x).strip() for x in (rec.get("tags") if isinstance(rec.get("tags"), list) else []) if str(x).strip()],
                "blocked_global": bool(blocked),
                "autoload_candidate": bool(enable_supported and (not blocked) and (_policy_level_visible(policy_level) or bool(recent_payload))),
                "policy_visible": bool(_policy_level_visible(policy_level)),
            }
            if blocked_reason:
                item["blocked_reason"] = blocked_reason
            if recent_payload:
                item["recent_success"] = recent_payload
            if install_state:
                item["cached_install_state"] = install_state
            if install_error_code:
                item["cached_install_error_code"] = install_error_code
            if install_error:
                item["cached_install_error"] = install_error
            item["readiness_preview"] = _build_readiness_preview(
                rec=rec,
                capability_id=cap_id,
                policy=policy,
                policy_level=policy_level,
                blocked_reason_code=blocked_reason_code,
                qualification_status=str(qualification_status or "").strip().lower(),
                enable_supported=bool(enable_supported),
                cached_install_state=install_state,
                install_error_code=install_error_code,
                recent_success=recent_payload,
            )
            if cap_id.startswith("pack:"):
                item["tool_count"] = int(rec.get("tool_count") or 0)
                tool_names = rec.get("tool_names") if isinstance(rec.get("tool_names"), list) else []
                if tool_names:
                    item["tool_names"] = [str(x).strip() for x in tool_names if str(x).strip()]
            rows.append(item)

        if query:
            rows = [row for row in rows if _search_matches(query, row)]

        def _rank(row: Dict[str, Any]) -> Tuple[int, int, int, str]:
            blocked_penalty = 1 if bool(row.get("blocked_global")) else 0
            recent = row.get("recent_success") if isinstance(row.get("recent_success"), dict) else {}
            recent_count = int(recent.get("success_count") or 0)
            recent_ts = str(recent.get("last_success_at") or "")
            cap_id = str(row.get("capability_id") or "")
            builtin_bias = 0 if cap_id.startswith("pack:") else 1
            return (
                blocked_penalty,
                builtin_bias,
                -recent_count,
                f"{recent_ts}:{str(row.get('name') or cap_id).lower()}",
            )

        rows.sort(key=_rank)
        items = rows[:limit]

        source_ids = sorted(
            {
                *_SOURCE_IDS,
                *(source_states.keys() if isinstance(source_states, dict) else []),
                *(source_levels.keys() if isinstance(source_levels, dict) else []),
                *(source_cfg_map.keys() if isinstance(source_cfg_map, dict) else []),
            }
        )
        sources: Dict[str, Dict[str, Any]] = {}
        for source_id in source_ids:
            state = source_states.get(source_id) if isinstance(source_states.get(source_id), dict) else {}
            cfg = source_cfg_map.get(source_id) if isinstance(source_cfg_map.get(source_id), dict) else {}
            sources[source_id] = {
                "source_id": source_id,
                "enabled": bool(cfg.get("enabled", True)),
                "source_level": _normalize_policy_level(source_levels.get(source_id), default=_LEVEL_INDEXED),
                "rationale": str(cfg.get("rationale") or ""),
                "sync_state": str(state.get("sync_state") or "never"),
                "last_synced_at": str(state.get("last_synced_at") or ""),
                "staleness_seconds": int(state.get("staleness_seconds") or 0),
                "record_count": int(state.get("record_count") or 0),
                "error": str(state.get("error") or ""),
            }

        blocked_list: List[Dict[str, Any]] = []
        for cap_id, row in sorted(blocked_global.items(), key=lambda x: str(x[0])):
            blocked_list.append(
                {
                    "capability_id": str(cap_id),
                    "scope": "global",
                    "reason": str(row.get("reason") or ""),
                    "by": str(row.get("by") or ""),
                    "blocked_at": str(row.get("blocked_at") or ""),
                    "expires_at": str(row.get("expires_at") or ""),
                }
            )

        return DaemonResponse(
            ok=True,
            result={
                "items": items,
                "count": len(items),
                "query": query,
                "sources": sources,
                "blocked_capabilities": blocked_list,
                "allowlist_revision": str(snapshot.get("revision") or ""),
            },
        )
    except Exception as e:
        return _error("capability_overview_failed", str(e))


def handle_capability_search(args: Dict[str, Any]) -> DaemonResponse:
    group_id = str(args.get("group_id") or "").strip()
    actor_id = str(args.get("actor_id") or args.get("by") or "").strip()
    query = str(args.get("query") or "").strip()
    limit = max(1, min(int(args.get("limit") or 30), 200))
    include_external = bool(args.get("include_external", True))
    kind_filter = str(args.get("kind") or "").strip().lower()
    source_filter = str(args.get("source_id") or "").strip()
    trust_filter = str(args.get("trust_tier") or "").strip().lower()
    qualification_filter = str(args.get("qualification_status") or "").strip().lower()

    try:
        group = _ensure_group(group_id)
        if actor_id and actor_id != "user":
            # If actor context is provided, ensure actor exists in this group.
            actors = group.doc.get("actors") if isinstance(group.doc.get("actors"), list) else []
            if actor_id not in {str(a.get("id") or "") for a in actors if isinstance(a, dict)}:
                return _error("actor_not_found", f"actor not found in group: {actor_id}")

        with _STATE_LOCK:
            state_path, state_doc = _load_state_doc()
            enabled_caps, mutated = _pkg()._collect_enabled_capabilities(
                state_doc, group_id=group_id, actor_id=actor_id or "user"
            )
            blocked_caps, blocked_mutated = _pkg()._collect_blocked_capabilities(state_doc, group_id=group_id)
            if mutated or blocked_mutated:
                _save_state_doc(state_path, state_doc)
        with _RUNTIME_LOCK:
            _, runtime_doc = _load_runtime_doc()
            capability_artifacts = _runtime_capability_artifacts(runtime_doc)
            artifacts = _runtime_artifacts(runtime_doc)
            recent_success = _runtime_recent_success(runtime_doc)
        enabled_set = set(enabled_caps)
        actor_role = _resolve_actor_role(group, actor_id)
        policy = _allowlist_policy()
        query_tokens = _tokenize_search_text(query)
        context_tokens = _context_search_tokens(group_id=group_id, actor_id=actor_id)
        preferred_packs = set(_role_preferred_pack_ids(actor_role))

        external_records: List[Dict[str, Any]] = []
        source_states: Dict[str, Dict[str, Any]] = {}
        remote_augmented = False
        remote_added = 0
        remote_error = ""
        with _CATALOG_LOCK:
            catalog_path, catalog_doc = _pkg()._load_catalog_doc()
            if include_external and _pkg()._ensure_curated_catalog_records(catalog_doc, policy=policy):
                _pkg()._save_catalog_doc(catalog_path, catalog_doc)
            if include_external:
                for item in (
                    catalog_doc.get("records")
                    if isinstance(catalog_doc.get("records"), dict)
                    else {}
                ).values():
                    if isinstance(item, dict):
                        external_records.append(dict(item))
            source_states = _render_source_states(catalog_doc)

        records: List[Dict[str, Any]] = _build_builtin_search_records()
        if include_external:
            records.extend(external_records)

        if kind_filter:
            records = [r for r in records if str(r.get("kind") or "").strip().lower() == kind_filter]
        if source_filter:
            records = [r for r in records if str(r.get("source_id") or "").strip() == source_filter]
        if trust_filter:
            records = [r for r in records if str(r.get("trust_tier") or "").strip().lower() == trust_filter]
        if qualification_filter:
            records = [
                r
                for r in records
                if str(r.get("qualification_status") or "").strip().lower() == qualification_filter
            ]
        policy_hidden_count = 0
        visible_records: List[Dict[str, Any]] = []
        for rec in records:
            cap_id = str(rec.get("capability_id") or "").strip()
            policy_level = _effective_policy_level(
                policy,
                capability_id=cap_id,
                kind=str(rec.get("kind") or ""),
                source_id=str(rec.get("source_id") or ""),
                actor_role=actor_role,
            )
            next_rec = dict(rec)
            next_rec["policy_level"] = policy_level
            block_entry = blocked_caps.get(cap_id) if isinstance(blocked_caps.get(cap_id), dict) else None
            if isinstance(block_entry, dict):
                next_rec["qualification_status"] = _QUAL_BLOCKED
                reasons = list(next_rec.get("qualification_reasons") or [])
                reason_text = str(block_entry.get("reason") or "").strip()
                scope_text = str(block_entry.get("scope") or "").strip().lower()
                if reason_text:
                    reasons = [f"runtime_block:{reason_text}"]
                else:
                    reasons = [f"runtime_block_{scope_text or 'group'}"]
                next_rec["qualification_reasons"] = reasons
                next_rec["blocked_scope"] = scope_text or "group"
            if (not _policy_level_visible(policy_level)) and (cap_id not in enabled_set):
                policy_hidden_count += 1
                continue
            visible_records.append(next_rec)
        records = visible_records
        records = [r for r in records if _search_matches(query, r)]

        remote_limit = max(1, min(_env_int("CCCC_CAPABILITY_SEARCH_REMOTE_FALLBACK_LIMIT", 40), 100))
        target_fill = max(1, min(limit, remote_limit))
        should_remote_augment = (
            include_external
            and bool(str(query or "").strip())
            and len(records) < target_fill
            and kind_filter in {"", "mcp_toolpack", "skill"}
            and _env_bool("CCCC_CAPABILITY_SEARCH_REMOTE_FALLBACK", True)
        )
        if should_remote_augment:
            remote_rows: List[Dict[str, Any]] = []
            remote_errors: List[str] = []
            needed = max(1, target_fill - len(records))
            try:
                if kind_filter in {"", "mcp_toolpack"} and _env_bool("CCCC_CAPABILITY_SOURCE_MCP_REGISTRY_ENABLED", True):
                    remote_rows.extend(_remote_search_mcp_registry_records(query=query, limit=needed))
            except Exception as e:
                remote_errors.append(f"mcp_registry:{e}")
            try:
                if kind_filter in {"", "skill"}:
                    skill_limit = max(
                        1,
                        min(_env_int("CCCC_CAPABILITY_SEARCH_REMOTE_SKILL_LIMIT", remote_limit), 100),
                    )
                    remote_rows.extend(
                        _remote_search_skill_records(
                            query=query,
                            limit=min(needed, skill_limit),
                            source_filter=source_filter,
                        )
                    )
            except Exception as e:
                remote_errors.append(f"skills:{e}")

            if remote_rows:
                existing = {str(r.get("capability_id") or "") for r in records if isinstance(r, dict)}
                accepted: List[Dict[str, Any]] = []
                for rec in remote_rows:
                    if kind_filter and str(rec.get("kind") or "").strip().lower() != kind_filter:
                        continue
                    if source_filter and str(rec.get("source_id") or "").strip() != source_filter:
                        continue
                    if trust_filter and str(rec.get("trust_tier") or "").strip().lower() != trust_filter:
                        continue
                    if qualification_filter and str(rec.get("qualification_status") or "").strip().lower() != qualification_filter:
                        continue
                    if not _search_matches(query, rec):
                        continue
                    cap_id = str(rec.get("capability_id") or "").strip()
                    if not cap_id or cap_id in existing:
                        continue
                    policy_level = _effective_policy_level(
                        policy,
                        capability_id=cap_id,
                        kind=str(rec.get("kind") or ""),
                        source_id=str(rec.get("source_id") or ""),
                        actor_role=actor_role,
                    )
                    if (not _policy_level_visible(policy_level)) and (cap_id not in enabled_set):
                        policy_hidden_count += 1
                        continue
                    existing.add(cap_id)
                    accepted_rec = dict(rec)
                    accepted_rec["policy_level"] = policy_level
                    block_entry = blocked_caps.get(cap_id) if isinstance(blocked_caps.get(cap_id), dict) else None
                    if isinstance(block_entry, dict):
                        accepted_rec["qualification_status"] = _QUAL_BLOCKED
                        reason_text = str(block_entry.get("reason") or "").strip()
                        scope_text = str(block_entry.get("scope") or "").strip().lower()
                        accepted_rec["qualification_reasons"] = (
                            [f"runtime_block:{reason_text}"]
                            if reason_text
                            else [f"runtime_block_{scope_text or 'group'}"]
                        )
                        accepted_rec["blocked_scope"] = scope_text or "group"
                    accepted.append(accepted_rec)
                if accepted:
                    records.extend(accepted)
                    remote_augmented = True
                    remote_added = len(accepted)
                    with _CATALOG_LOCK:
                        path, doc = _pkg()._load_catalog_doc()
                        rows = doc.get("records") if isinstance(doc.get("records"), dict) else {}
                        changed = False
                        touched_sources: set[str] = set()
                        for rec in accepted:
                            cap_id = str(rec.get("capability_id") or "").strip()
                            if not cap_id:
                                continue
                            store_rec = dict(rec)
                            store_rec.pop("blocked_scope", None)
                            rec_reasons = store_rec.get("qualification_reasons")
                            reasons = (
                                [str(x).strip() for x in rec_reasons if str(x).strip()]
                                if isinstance(rec_reasons, list)
                                else []
                            )
                            if (
                                str(store_rec.get("qualification_status") or "").strip().lower() == _QUAL_BLOCKED
                                and any(r.startswith("runtime_block") for r in reasons)
                            ):
                                store_rec["qualification_status"] = _QUAL_QUALIFIED
                                store_rec["qualification_reasons"] = []
                            source_id = str(store_rec.get("source_id") or "").strip()
                            if source_id:
                                touched_sources.add(source_id)
                            if rows.get(cap_id) != store_rec:
                                rows[cap_id] = store_rec
                                changed = True
                        if changed:
                            doc["records"] = rows
                            now_iso = utc_now_iso()
                            sources_doc = doc.get("sources") if isinstance(doc.get("sources"), dict) else {}
                            for source_id in touched_sources:
                                state = (
                                    sources_doc.get(source_id)
                                    if isinstance(sources_doc.get(source_id), dict)
                                    else _source_state_template("never")
                                )
                                state["sync_state"] = "remote_fallback"
                                state["last_synced_at"] = now_iso
                                state["staleness_seconds"] = 0
                                state["error"] = ""
                                sources_doc[source_id] = state
                            doc["sources"] = sources_doc
                            _pkg()._refresh_source_record_counts(doc)
                            _pkg()._save_catalog_doc(path, doc)
                        source_states = _render_source_states(doc)
            if remote_errors:
                remote_error = "; ".join(str(x) for x in remote_errors if str(x))

        def _rank(item: Dict[str, Any]) -> Tuple[int, int, int, int, int, str]:
            cap_id = str(item.get("capability_id") or "")
            is_builtin = 0 if cap_id.startswith("pack:") else 1
            name_key = str(item.get("name") or cap_id).lower()
            enabled_bias = 0 if cap_id in enabled_set else 1
            qualification = str(item.get("qualification_status") or "").strip().lower()
            if qualification == _QUAL_BLOCKED:
                qualification_penalty = 2
            elif qualification == _QUAL_UNAVAILABLE:
                qualification_penalty = 1
            else:
                qualification_penalty = 0

            # Query-first path: preserve deterministic behavior with stronger lexical score.
            if query_tokens:
                query_score = _score_item_tokens(item, query_tokens)
                return (
                    is_builtin,
                    enabled_bias,
                    -query_score,
                    qualification_penalty,
                    0,
                    name_key,
                )

            # Empty-query path: prioritize actionable + context-relevant packs.
            context_score = _score_item_tokens(item, context_tokens)
            preferred_bias = 0 if cap_id in preferred_packs else 1
            return (
                is_builtin,
                enabled_bias,
                -context_score,
                preferred_bias,
                qualification_penalty,
                name_key,
            )

        records.sort(key=_rank)
        sliced = records[:limit]
        items: List[Dict[str, Any]] = []
        for rec in sliced:
            cap_id = str(rec.get("capability_id") or "")
            qualification_status = str(rec.get("qualification_status") or _QUAL_QUALIFIED)
            enable_supported = _pkg()._record_enable_supported(rec, capability_id=cap_id)
            blocked_scope = str(rec.get("blocked_scope") or "").strip().lower()
            blocked_reason_code = ""
            if blocked_scope == "global":
                blocked_reason_code = "blocked_by_global_policy"
            elif blocked_scope == "group":
                blocked_reason_code = "blocked_by_group_policy"
            artifact_id = str(capability_artifacts.get(cap_id) or "").strip()
            artifact = artifacts.get(artifact_id) if artifact_id and isinstance(artifacts.get(artifact_id), dict) else {}
            cached_install_state = str(artifact.get("state") or "").strip()
            install_error_code = str(artifact.get("last_error_code") or "").strip()
            recent_row = recent_success.get(cap_id) if isinstance(recent_success.get(cap_id), dict) else {}
            recent_payload = {
                "success_count": int(recent_row.get("success_count") or 0),
                "last_success_at": str(recent_row.get("last_success_at") or ""),
                "last_group_id": str(recent_row.get("last_group_id") or ""),
                "last_actor_id": str(recent_row.get("last_actor_id") or ""),
                "last_action": str(recent_row.get("last_action") or ""),
            } if recent_row else {}
            item = {
                "capability_id": cap_id,
                "kind": str(rec.get("kind") or ""),
                "name": str(rec.get("name") or cap_id),
                "description_short": str(rec.get("description_short") or ""),
                "source_id": str(rec.get("source_id") or ""),
                "source_tier": str(rec.get("source_tier") or ""),
                "source_uri": str(rec.get("source_uri") or ""),
                "trust_tier": str(rec.get("trust_tier") or ""),
                "license": str(rec.get("license") or ""),
                "qualification_status": qualification_status,
                "sync_state": str(rec.get("sync_state") or ""),
                "enabled": cap_id in enabled_set,
                "enable_supported": enable_supported,
                "install_mode": str(rec.get("install_mode") or ""),
                "policy_level": str(rec.get("policy_level") or ""),
                "tags": list(rec.get("tags") or []),
            }
            if cached_install_state:
                item["cached_install_state"] = cached_install_state
            if install_error_code:
                item["cached_install_error_code"] = install_error_code
            if recent_payload:
                item["recent_success"] = recent_payload
            if cap_id.startswith("pack:"):
                item["tool_count"] = int(rec.get("tool_count") or 0)
                tool_names = rec.get("tool_names") if isinstance(rec.get("tool_names"), list) else []
                if tool_names:
                    item["tool_names"] = [str(x).strip() for x in tool_names if str(x).strip()]
            qualification = str(item.get("qualification_status") or "")
            if qualification == _QUAL_BLOCKED:
                item["enable_hint"] = "blocked"
                reasons = rec.get("qualification_reasons")
                if isinstance(reasons, list):
                    first = next((str(x).strip() for x in reasons if str(x).strip()), "")
                    if first:
                        item["blocked_reason"] = first
            elif bool(item.get("enable_supported")):
                item["enable_hint"] = "enable_now"
            else:
                item["enable_hint"] = "unsupported"
            if blocked_scope:
                item["blocked_scope"] = blocked_scope
            item["readiness_preview"] = _build_readiness_preview(
                rec=rec,
                capability_id=cap_id,
                policy=policy,
                policy_level=str(rec.get("policy_level") or ""),
                blocked_reason_code=blocked_reason_code,
                qualification_status=str(qualification_status or "").strip().lower(),
                enable_supported=bool(enable_supported),
                cached_install_state=cached_install_state,
                install_error_code=install_error_code,
                recent_success=recent_payload,
            )
            items.append(item)

        return DaemonResponse(
            ok=True,
            result={
                "group_id": group_id,
                "actor_id": actor_id,
                "default_profile": "core",
                "items": items,
                "count": len(items),
                "sources": source_states,
                "applied_filters": {
                    "kind": kind_filter,
                    "source_id": source_filter,
                    "trust_tier": trust_filter,
                    "qualification_status": qualification_filter,
                },
                "search_diagnostics": {
                    "remote_augmented": bool(remote_augmented),
                    "remote_added": int(remote_added),
                    "remote_error": str(remote_error or ""),
                    "policy_hidden_count": int(policy_hidden_count),
                },
            },
        )
    except LookupError as e:
        return _error("group_not_found", str(e))
    except Exception as e:
        return _error("capability_search_failed", str(e))


def handle_capability_state(args: Dict[str, Any]) -> DaemonResponse:
    group_id = str(args.get("group_id") or "").strip()
    by = str(args.get("by") or args.get("actor_id") or "").strip()
    actor_id = str(args.get("actor_id") or by).strip()

    try:
        group = _ensure_group(group_id)
        if by and by != "user" and actor_id and actor_id != by:
            return _error("permission_denied", "actor can only inspect their own scope")
        if not actor_id:
            actor_id = "user"
        actor_role = _resolve_actor_role(group, actor_id)
        policy = _allowlist_policy()
        max_dynamic_tools_visible = _quota_limit(
            "CCCC_CAPABILITY_MAX_DYNAMIC_TOOLS_VISIBLE",
            32,
            minimum=1,
            maximum=500,
        )

        with _STATE_LOCK:
            state_path, state_doc = _load_state_doc()
            enabled_caps, mutated = _pkg()._collect_enabled_capabilities(state_doc, group_id=group_id, actor_id=actor_id)
            blocked_caps, blocked_mutated = _pkg()._collect_blocked_capabilities(state_doc, group_id=group_id)
            if mutated or blocked_mutated:
                _save_state_doc(state_path, state_doc)
        enabled_caps_effective = [cap for cap in enabled_caps if cap not in set(blocked_caps.keys())]
        builtin_enabled = [cap for cap in enabled_caps_effective if cap in BUILTIN_CAPABILITY_PACKS]
        external_enabled = [cap for cap in enabled_caps_effective if cap not in BUILTIN_CAPABILITY_PACKS]

        dynamic_tools: List[Dict[str, Any]] = []
        install_state_by_cap: Dict[str, str] = {}
        install_artifact_by_cap: Dict[str, str] = {}
        install_error_by_cap: Dict[str, str] = {}
        install_error_code_by_cap: Dict[str, str] = {}
        actor_binding_state_by_cap: Dict[str, Dict[str, str]] = {}
        with _RUNTIME_LOCK:
            _, runtime_doc = _load_runtime_doc()
            artifacts = _runtime_artifacts(runtime_doc)
            capability_artifacts = _runtime_capability_artifacts(runtime_doc)
            actor_bindings = _runtime_actor_bindings(runtime_doc)
            recent_success = _runtime_recent_success(runtime_doc)
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
            if isinstance(capability_artifacts, dict):
                for cap_id, artifact_id in capability_artifacts.items():
                    cid = str(cap_id or "").strip()
                    aid = str(artifact_id or "").strip()
                    install = artifacts.get(aid) if isinstance(artifacts.get(aid), dict) else None
                    if not cid or not aid or not isinstance(install, dict):
                        continue
                    install_state_by_cap[cid] = str(install.get("state") or "").strip()
                    install_artifact_by_cap[cid] = aid
                    install_error_by_cap[cid] = str(install.get("last_error") or "").strip()
                    install_error_code_by_cap[cid] = str(install.get("last_error_code") or "").strip()
            if isinstance(per_actor_bindings, dict):
                for cap_id, item in per_actor_bindings.items():
                    if not isinstance(item, dict):
                        continue
                    actor_binding_state_by_cap[str(cap_id)] = {
                        "artifact_id": str(item.get("artifact_id") or "").strip(),
                        "state": str(item.get("state") or "").strip() or "unknown",
                        "last_error": str(item.get("last_error") or "").strip(),
                    }
            for capability_id in external_enabled:
                binding = per_actor_bindings.get(capability_id) if isinstance(per_actor_bindings, dict) else None
                binding_artifact_id = (
                    str((binding or {}).get("artifact_id") or "").strip()
                    if isinstance(binding, dict)
                    else ""
                )
                artifact_id = binding_artifact_id or str(install_artifact_by_cap.get(capability_id) or "").strip()
                install = artifacts.get(artifact_id) if isinstance(artifacts, dict) else None
                if not isinstance(install, dict):
                    continue
                if not _pkg()._install_state_allows_external_tool(install.get("state")):
                    continue
                binding_state = str((binding or {}).get("state") or "").strip() if isinstance(binding, dict) else ""
                if not _pkg()._binding_state_allows_external_tool(binding_state):
                    continue
                tools = install.get("tools") if isinstance(install.get("tools"), list) else []
                for tool in tools:
                    if not isinstance(tool, dict):
                        continue
                    name = str(tool.get("name") or "").strip()
                    real_name = str(tool.get("real_tool_name") or "").strip()
                    if not name or not real_name:
                        continue
                    schema = _pkg()._normalize_mcp_input_schema(tool.get("inputSchema"))
                    dynamic_tools.append(
                        {
                            "name": name,
                            "description": str(tool.get("description") or "").strip(),
                            "inputSchema": schema,
                            "capability_id": capability_id,
                            "real_tool_name": real_name,
                        }
                    )
        dynamic_tools.sort(key=lambda x: str(x.get("name") or ""))
        dynamic_tool_dropped = 0
        if len(dynamic_tools) > max_dynamic_tools_visible:
            dynamic_tool_dropped = len(dynamic_tools) - max_dynamic_tools_visible
            dynamic_tools = dynamic_tools[:max_dynamic_tools_visible]

        visible_tools = sorted(
            set(resolve_visible_tool_names(builtin_enabled, actor_role=actor_role))
            | {str(x.get("name") or "").strip() for x in dynamic_tools if isinstance(x, dict)}
        )

        hidden_capabilities: List[Dict[str, Any]] = []
        for cap_id in all_builtin_pack_ids():
            if cap_id in builtin_enabled:
                continue
            block_entry = blocked_caps.get(cap_id) if isinstance(blocked_caps.get(cap_id), dict) else None
            if isinstance(block_entry, dict):
                scope_token = str(block_entry.get("scope") or "group").strip().lower()
                reason_token = "blocked_by_global_policy" if scope_token == "global" else "blocked_by_group_policy"
                row = {"capability_id": cap_id, "reason": reason_token, "policy_level": "blocked"}
                reason_text = str(block_entry.get("reason") or "").strip()
                if reason_text:
                    row["blocked_reason"] = reason_text
                hidden_capabilities.append(row)
                continue
            level = _effective_policy_level(
                policy,
                capability_id=cap_id,
                kind="mcp_toolpack",
                source_id="cccc_builtin",
                actor_role=actor_role,
            )
            hidden_capabilities.append(
                {
                    "capability_id": cap_id,
                    "reason": "policy_indexed" if (not _policy_level_visible(level)) else "not_enabled",
                    "policy_level": level,
                }
            )

        now = datetime.now(timezone.utc)
        group_enabled_map = state_doc.get("group_enabled") if isinstance(state_doc.get("group_enabled"), dict) else {}
        actor_enabled_map = state_doc.get("actor_enabled") if isinstance(state_doc.get("actor_enabled"), dict) else {}
        session_enabled_map = (
            state_doc.get("session_enabled") if isinstance(state_doc.get("session_enabled"), dict) else {}
        )

        per_group_actor = actor_enabled_map.get(group_id) if isinstance(actor_enabled_map.get(group_id), dict) else {}
        per_group_session = (
            session_enabled_map.get(group_id) if isinstance(session_enabled_map.get(group_id), dict) else {}
        )

        def _scope_mismatch_for_capability(capability_id: str) -> bool:
            cap = str(capability_id or "").strip()
            if not cap:
                return False
            if cap in set(group_enabled_map.get(group_id) or []):
                return False
            for aid, items in per_group_actor.items():
                if str(aid) == actor_id:
                    continue
                if cap in set(items if isinstance(items, list) else []):
                    return True
            for aid, entries in per_group_session.items():
                if str(aid) == actor_id or not isinstance(entries, list):
                    continue
                for item in entries:
                    if not isinstance(item, dict):
                        continue
                    if str(item.get("capability_id") or "").strip() != cap:
                        continue
                    dt = parse_utc_iso(str(item.get("expires_at") or ""))
                    if dt is not None and dt > now:
                        return True
            return False

        session_bindings: List[Dict[str, Any]] = []
        own_session_entries = per_group_session.get(actor_id) if isinstance(per_group_session.get(actor_id), list) else []
        for entry in own_session_entries:
            if not isinstance(entry, dict):
                continue
            cap_id = str(entry.get("capability_id") or "").strip()
            expires_at = str(entry.get("expires_at") or "").strip()
            if not cap_id or not expires_at:
                continue
            dt = parse_utc_iso(expires_at)
            if dt is None:
                continue
            session_bindings.append(
                {
                    "capability_id": cap_id,
                    "expires_at": expires_at,
                    "ttl_seconds": max(0, int((dt - now).total_seconds())),
                }
            )

        with _CATALOG_LOCK:
            catalog_path, catalog_doc = _pkg()._load_catalog_doc()
            if _pkg()._ensure_curated_catalog_records(catalog_doc, policy=policy):
                _pkg()._save_catalog_doc(catalog_path, catalog_doc)
            source_states = _render_source_states(catalog_doc)
            records_raw = catalog_doc.get("records") if isinstance(catalog_doc.get("records"), dict) else {}
            external_records = {
                str(cid): dict(rec)
                for cid, rec in records_raw.items()
                if isinstance(rec, dict) and str(cid) and (not str(cid).startswith("pack:"))
            }

        actors = group.doc.get("actors") if isinstance(group.doc.get("actors"), list) else []
        actor_record = next(
            (
                a
                for a in actors
                if isinstance(a, dict) and str(a.get("id") or "").strip() == actor_id
            ),
            {},
        )
        actor_autoload_capabilities = _pkg()._normalize_capability_id_list(
            actor_record.get("capability_autoload") if isinstance(actor_record, dict) else []
        )
        profile_id = str(actor_record.get("profile_id") or "").strip() if isinstance(actor_record, dict) else ""
        profile_autoload_capabilities: List[str] = []
        if profile_id:
            try:
                from ..actors.actor_profile_store import get_actor_profile as _get_actor_profile

                profile_doc = _get_actor_profile(profile_id)
                if isinstance(profile_doc, dict):
                    defaults_cfg = _pkg()._normalize_profile_capability_defaults(profile_doc.get("capability_defaults"))
                    profile_autoload_capabilities = list(defaults_cfg.get("autoload_capabilities") or [])
            except Exception:
                profile_autoload_capabilities = []
        effective_autoload_capabilities = _pkg()._normalize_capability_id_list(
            [*profile_autoload_capabilities, *actor_autoload_capabilities]
        )

        active_capsule_skills: List[Dict[str, Any]] = []
        for cap_id in enabled_caps_effective:
            rec = external_records.get(cap_id)
            if not isinstance(rec, dict):
                continue
            if str(rec.get("kind") or "").strip().lower() != "skill":
                continue
            active_capsule_skills.append(
                {
                    "capability_id": cap_id,
                    "name": str(rec.get("name") or cap_id),
                    "description_short": str(rec.get("description_short") or ""),
                    "source_id": str(rec.get("source_id") or ""),
                    "source_uri": str(rec.get("source_uri") or ""),
                    "policy_level": _effective_policy_level(
                        policy,
                        capability_id=cap_id,
                        kind=str(rec.get("kind") or ""),
                        source_id=str(rec.get("source_id") or ""),
                        actor_role=actor_role,
                    ),
                }
            )

        autoload_skills: List[Dict[str, Any]] = []
        for cap_id in effective_autoload_capabilities:
            rec = external_records.get(cap_id)
            if not isinstance(rec, dict):
                continue
            if str(rec.get("kind") or "").strip().lower() != "skill":
                continue
            autoload_skills.append(
                {
                    "capability_id": cap_id,
                    "name": str(rec.get("name") or cap_id),
                    "description_short": str(rec.get("description_short") or ""),
                    "source_id": str(rec.get("source_id") or ""),
                    "policy_level": _effective_policy_level(
                        policy,
                        capability_id=cap_id,
                        kind=str(rec.get("kind") or ""),
                        source_id=str(rec.get("source_id") or ""),
                        actor_role=actor_role,
                    ),
                }
            )

        for cap_id, rec in external_records.items():
            block_entry = blocked_caps.get(cap_id) if isinstance(blocked_caps.get(cap_id), dict) else None
            if isinstance(block_entry, dict):
                scope_token = str(block_entry.get("scope") or "group").strip().lower()
                reason_token = "blocked_by_global_policy" if scope_token == "global" else "blocked_by_group_policy"
                row: Dict[str, Any] = {"capability_id": cap_id, "reason": reason_token, "policy_level": "blocked"}
                reason_text = str(block_entry.get("reason") or "").strip()
                if reason_text:
                    row["blocked_reason"] = reason_text
                hidden_capabilities.append(row)
                continue
            policy_level = _effective_policy_level(
                policy,
                capability_id=cap_id,
                kind=str(rec.get("kind") or ""),
                source_id=str(rec.get("source_id") or ""),
                actor_role=actor_role,
            )
            if cap_id in external_enabled:
                state = str(install_state_by_cap.get(cap_id) or "").strip()
                binding = actor_binding_state_by_cap.get(cap_id) if isinstance(actor_binding_state_by_cap.get(cap_id), dict) else {}
                binding_state = str(binding.get("state") or "").strip()
                if not binding_state:
                    hidden_capabilities.append(
                        {
                            "capability_id": cap_id,
                            "reason": "binding_missing",
                            "policy_level": policy_level,
                        }
                    )
                    continue
                if state and (not _pkg()._install_state_allows_external_tool(state)):
                    install_error = str(install_error_by_cap.get(cap_id) or "").strip()
                    install_error_code = str(install_error_code_by_cap.get(cap_id) or "").strip()
                    hidden_capabilities.append(
                        {
                            "capability_id": cap_id,
                            "reason": "install_failed",
                            "state": state,
                            "policy_level": policy_level,
                            **({"install_error_code": install_error_code} if install_error_code else {}),
                            **({"install_error": install_error} if install_error else {}),
                        }
                    )
                    continue
                if not _pkg()._binding_state_allows_external_tool(binding_state):
                    hidden_capabilities.append(
                        {
                            "capability_id": cap_id,
                            "reason": "binding_not_ready",
                            "state": binding_state,
                            "policy_level": policy_level,
                        }
                    )
                continue

            if not _policy_level_visible(policy_level):
                hidden_capabilities.append(
                    {
                        "capability_id": cap_id,
                        "reason": "policy_indexed",
                        "policy_level": policy_level,
                    }
                )
                continue

            qualification = str(rec.get("qualification_status") or "").strip().lower()
            if qualification == _QUAL_BLOCKED:
                hidden_capabilities.append(
                    {"capability_id": cap_id, "reason": "policy_blocked", "policy_level": policy_level}
                )
            elif not _pkg()._record_enable_supported(rec, capability_id=cap_id):
                hidden_capabilities.append(
                    {"capability_id": cap_id, "reason": "unavailable", "policy_level": policy_level}
                )
            elif _scope_mismatch_for_capability(cap_id):
                hidden_capabilities.append(
                    {"capability_id": cap_id, "reason": "scope_mismatch", "policy_level": policy_level}
                )
            else:
                hidden_capabilities.append(
                    {"capability_id": cap_id, "reason": "not_enabled", "policy_level": policy_level}
                )

        external_binding_states: Dict[str, Dict[str, str]] = {}
        for cap_id in external_enabled:
            rec = external_records.get(cap_id)
            kind = str((rec or {}).get("kind") or "").strip().lower() if isinstance(rec, dict) else ""
            install_state = str(install_state_by_cap.get(cap_id) or "").strip()
            binding = actor_binding_state_by_cap.get(cap_id) if isinstance(actor_binding_state_by_cap.get(cap_id), dict) else {}
            binding_state = str(binding.get("state") or "").strip().lower()
            recent_row = recent_success.get(cap_id) if isinstance(recent_success.get(cap_id), dict) else {}
            last_action = str(recent_row.get("last_action") or "").strip().lower()
            entry: Dict[str, str] = {}
            if kind == "skill":
                entry["mode"] = "skill"
                if str(binding.get("last_error") or "").strip():
                    entry["state"] = "blocked"
                    entry["last_error"] = str(binding.get("last_error") or "").strip()
                else:
                    entry["state"] = str(binding_state or "runnable")
            else:
                entry["mode"] = "mcp"
                entry["install_state"] = install_state or "unknown"
                artifact_id = str(binding.get("artifact_id") or install_artifact_by_cap.get(cap_id) or "").strip()
                if artifact_id:
                    entry["artifact_id"] = artifact_id
                install_error = str(install_error_by_cap.get(cap_id) or "").strip()
                install_error_code = str(install_error_code_by_cap.get(cap_id) or "").strip()
                if str(binding.get("last_error") or "").strip():
                    entry["state"] = "blocked"
                    entry["last_error"] = str(binding.get("last_error") or "").strip()
                elif last_action == "tool_call":
                    entry["state"] = "verified"
                elif binding_state in {"activation_pending", "runnable", "verified"}:
                    entry["state"] = binding_state
                elif install_state and _pkg()._install_state_allows_external_tool(install_state):
                    entry["state"] = "activation_pending"
                else:
                    entry["state"] = "blocked"
                if install_state == "installed_degraded" and entry.get("state") != "verified":
                    entry["last_error"] = install_error or "probe_timeout"
                    if install_error_code:
                        entry["last_error_code"] = install_error_code
                elif install_state and install_state not in {"installed", "installed_degraded"}:
                    entry["last_error"] = install_error or "install_not_ready"
                    if install_error_code:
                        entry["last_error_code"] = install_error_code
            external_binding_states[cap_id] = entry

        blocked_capabilities: List[Dict[str, str]] = []
        for cap_id in sorted(blocked_caps.keys()):
            entry = blocked_caps.get(cap_id) if isinstance(blocked_caps.get(cap_id), dict) else {}
            item = {
                "capability_id": cap_id,
                "scope": str(entry.get("scope") or "group"),
                "reason": str(entry.get("reason") or ""),
                "by": str(entry.get("by") or ""),
                "blocked_at": str(entry.get("blocked_at") or ""),
                "expires_at": str(entry.get("expires_at") or ""),
            }
            blocked_capabilities.append(item)

        return DaemonResponse(
            ok=True,
            result={
                "group_id": group_id,
                "actor_id": actor_id,
                "default_profile": "core",
                "core_tool_count": len(CORE_TOOL_NAMES),
                "visible_tool_count": len(visible_tools),
                "visible_tools": visible_tools,
                "dynamic_tools": dynamic_tools,
                "dynamic_tool_limit": max_dynamic_tools_visible,
                "dynamic_tool_dropped": dynamic_tool_dropped,
                "enabled_capabilities": enabled_caps_effective,
                "active_capsule_skills": active_capsule_skills,
                "autoload_skills": autoload_skills,
                "autoload_capabilities": effective_autoload_capabilities,
                "actor_autoload_capabilities": actor_autoload_capabilities,
                "profile_autoload_capabilities": profile_autoload_capabilities,
                "hidden_capabilities": hidden_capabilities,
                "external_binding_states": external_binding_states,
                "precedence_chain": ["session", "actor", "group"],
                "session_bindings": session_bindings,
                "source_states": source_states,
                "blocked_capabilities": blocked_capabilities,
                "is_foreman": bool(_is_foreman(group, actor_id)),
            },
        )
    except LookupError as e:
        return _error("group_not_found", str(e))
    except ValueError as e:
        message = str(e)
        if message == "missing_group_id":
            return _error("missing_group_id", "missing group_id")
        return _error("capability_state_invalid", message)
    except Exception as e:
        return _error("capability_state_failed", str(e))
