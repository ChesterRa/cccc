"""Policy compilation, allowlist CRUD, and role/level functions for capability_ops."""

from __future__ import annotations

import hashlib
import json
from importlib import resources as pkg_resources
from pathlib import Path
from typing import Any, Dict, List, Tuple

import yaml

from ....contracts.v1 import DaemonResponse
from ....paths import ensure_home
from ....util.fs import atomic_write_text

from ._common import (
    _LEVEL_INDEXED,
    _LEVEL_MOUNTED,
    _LEVELS,
    _POLICY_LOCK,
    _POLICY_CACHE,
    _QUAL_STATES,
    _error,
)


def _normalize_policy_level(raw: Any, *, default: str = _LEVEL_INDEXED) -> str:
    level = str(raw or "").strip().lower()
    if level == "enabled":
        return _LEVEL_MOUNTED  # backward compat: enabled → mounted
    if level not in _LEVELS:
        return default
    return level


def _policy_level_visible(level: str) -> bool:
    return _normalize_policy_level(level) != _LEVEL_INDEXED


def _policy_default_compiled() -> Dict[str, Any]:
    return {
        "source_levels": {
            "cccc_builtin": _LEVEL_MOUNTED,
            "manual_import": _LEVEL_MOUNTED,
            "anthropic_skills": _LEVEL_MOUNTED,
            "github_skills_curated": _LEVEL_MOUNTED,
            "skillsmp_remote": _LEVEL_MOUNTED,
            "clawhub_remote": _LEVEL_MOUNTED,
            "openclaw_skills_remote": _LEVEL_MOUNTED,
            "clawskills_remote": _LEVEL_MOUNTED,
            "mcp_registry_official": _LEVEL_MOUNTED,
        },
        "capability_levels": {},
        "skill_source_levels": {},
        "role_pinned": {},
        "curated_mcp_entries": [],
        "curated_skill_entries": [],
    }


def _allowlist_default_source_label() -> str:
    return "builtin:cccc.resources/capability-allowlist.default.yaml"


def _allowlist_user_overlay_path() -> Path:
    return ensure_home() / "config" / "capability-allowlist.user.yaml"


def _safe_load_yaml_mapping(text: str) -> Dict[str, Any]:
    raw = yaml.safe_load(str(text or ""))
    if raw is None:
        return {}
    if isinstance(raw, dict):
        return dict(raw)
    raise ValueError("allowlist YAML root must be a mapping")


def _load_allowlist_default_doc() -> Tuple[Dict[str, Any], str]:
    try:
        text = pkg_resources.files("cccc.resources").joinpath("capability-allowlist.default.yaml").read_text(
            encoding="utf-8"
        )
    except Exception:
        text = ""
    try:
        doc = _safe_load_yaml_mapping(text)
    except Exception:
        doc = {}
    return doc, text


def _load_allowlist_overlay_doc() -> Tuple[Dict[str, Any], str, str]:
    path = _allowlist_user_overlay_path()
    if not path.exists() or not path.is_file():
        return {}, "", ""
    try:
        text = path.read_text(encoding="utf-8")
    except Exception:
        return {}, "", "failed_to_read_overlay"
    try:
        return _safe_load_yaml_mapping(text), text, ""
    except Exception as e:
        return {}, text, f"invalid_overlay_yaml:{e}"


def _merge_allowlist_docs(base: Any, overlay: Any) -> Dict[str, Any]:
    def _merge(a: Any, b: Any) -> Any:
        if isinstance(a, dict) and isinstance(b, dict):
            out: Dict[str, Any] = {str(k): v for k, v in a.items()}
            for key, value in b.items():
                sk = str(key)
                if sk in out:
                    out[sk] = _merge(out.get(sk), value)
                else:
                    out[sk] = value
            return out
        # Non-mapping types (including lists) are replaced by overlay value.
        return b

    base_doc = dict(base) if isinstance(base, dict) else {}
    overlay_doc = dict(overlay) if isinstance(overlay, dict) else {}
    merged = _merge(base_doc, overlay_doc)
    return merged if isinstance(merged, dict) else {}


def _allowlist_effective_snapshot() -> Dict[str, Any]:
    default_doc, default_text = _load_allowlist_default_doc()
    overlay_doc, overlay_text, overlay_error = _load_allowlist_overlay_doc()
    effective_doc = _merge_allowlist_docs(default_doc, overlay_doc)
    key_payload = json.dumps(
        {"default": default_doc, "overlay": overlay_doc},
        ensure_ascii=False,
        sort_keys=True,
    )
    revision = hashlib.sha1(key_payload.encode("utf-8")).hexdigest()
    return {
        "revision": revision,
        "default": default_doc,
        "overlay": overlay_doc,
        "effective": effective_doc,
        "default_source": _allowlist_default_source_label(),
        "overlay_source": str(_allowlist_user_overlay_path()) if overlay_text else "",
        "overlay_error": overlay_error,
        "default_text": default_text,
        "overlay_text": overlay_text,
    }


def _write_allowlist_overlay_doc(overlay_doc: Dict[str, Any]) -> None:
    path = _allowlist_user_overlay_path()
    root = path.parent
    root.mkdir(parents=True, exist_ok=True)
    doc = overlay_doc if isinstance(overlay_doc, dict) else {}
    if not doc:
        try:
            path.unlink()
        except FileNotFoundError:
            pass
        except Exception:
            pass
        return
    text = yaml.safe_dump(
        doc,
        allow_unicode=False,
        sort_keys=True,
    )
    atomic_write_text(path, text, encoding="utf-8")


def _clear_policy_cache() -> None:
    _POLICY_CACHE["key"] = ""
    _POLICY_CACHE["compiled"] = None
    _POLICY_CACHE["source"] = ""
    _POLICY_CACHE["error"] = ""


def _compile_allowlist_policy(raw: Any) -> Dict[str, Any]:
    compiled = _policy_default_compiled()
    doc = raw if isinstance(raw, dict) else {}

    defaults = doc.get("defaults") if isinstance(doc.get("defaults"), dict) else {}
    source_levels = defaults.get("source_level") if isinstance(defaults.get("source_level"), dict) else {}
    for source_id, level in source_levels.items():
        sid = str(source_id or "").strip()
        if not sid:
            continue
        compiled["source_levels"][sid] = _normalize_policy_level(level, default=_LEVEL_INDEXED)

    capability_levels: Dict[str, str] = {}
    curated_mcp_entries: List[Dict[str, Any]] = []
    for item in doc.get("mcp_overrides") if isinstance(doc.get("mcp_overrides"), list) else []:
        if not isinstance(item, dict):
            continue
        cid = str(item.get("capability_id") or "").strip()
        if not cid:
            continue
        level = _normalize_policy_level(item.get("level"), default=_LEVEL_MOUNTED)
        capability_levels[cid] = level
        curated_mcp_entries.append(
            {
                "capability_id": cid,
                "level": level,
                "trust": str(item.get("trust") or "").strip().lower(),
                "notes": str(item.get("notes") or "").strip(),
                "install_mode_preference": str(item.get("install_mode_preference") or "").strip(),
                "risk_tags": list(item.get("risk_tags") or []) if isinstance(item.get("risk_tags"), list) else [],
                "required_secrets": (
                    list(item.get("required_secrets") or []) if isinstance(item.get("required_secrets"), list) else []
                ),
            }
        )

    skills = doc.get("skills") if isinstance(doc.get("skills"), dict) else {}
    skill_source_levels: Dict[str, str] = {}
    for item in skills.get("source_overrides") if isinstance(skills.get("source_overrides"), list) else []:
        if not isinstance(item, dict):
            continue
        sid = str(item.get("source_id") or "").strip()
        if not sid:
            continue
        skill_source_levels[sid] = _normalize_policy_level(item.get("level"), default=_LEVEL_MOUNTED)

    role_pinned: Dict[str, set[str]] = {}
    curated_skill_entries: List[Dict[str, Any]] = []

    def _append_skill_entry(raw_item: Dict[str, Any], *, default_source_id: str) -> None:
        cid = str(raw_item.get("capability_id") or "").strip()
        if not cid:
            return
        level = _normalize_policy_level(raw_item.get("level"), default=_LEVEL_MOUNTED)
        capability_levels[cid] = level
        source_id = str(raw_item.get("source_id") or default_source_id).strip() or default_source_id
        trust = str(raw_item.get("trust") or "").strip().lower()
        notes = str(raw_item.get("notes") or "").strip()
        name = str(raw_item.get("name") or "").strip()
        source_uri = str(raw_item.get("source_uri") or "").strip()
        description_short = str(raw_item.get("description_short") or "").strip()
        capsule_text = str(raw_item.get("capsule_text") or "").strip()
        license_text = str(raw_item.get("license") or "").strip()
        qualification_status = str(raw_item.get("qualification_status") or "").strip().lower()
        if qualification_status not in _QUAL_STATES:
            qualification_status = ""
        tags = list(raw_item.get("tags") or []) if isinstance(raw_item.get("tags"), list) else []
        requires_caps = (
            list(raw_item.get("requires_capabilities") or [])
            if isinstance(raw_item.get("requires_capabilities"), list)
            else []
        )
        reasons = (
            [str(x).strip() for x in raw_item.get("qualification_reasons") if str(x).strip()]
            if isinstance(raw_item.get("qualification_reasons"), list)
            else []
        )
        curated_skill_entries.append(
            {
                "capability_id": cid,
                "level": level,
                "source_id": source_id,
                "trust": trust,
                "notes": notes,
                "name": name,
                "source_uri": source_uri,
                "description_short": description_short,
                "capsule_text": capsule_text,
                "license": license_text,
                "qualification_status": qualification_status,
                "qualification_reasons": reasons,
                "tags": tags,
                "requires_capabilities": requires_caps,
            }
        )
        for role_raw in raw_item.get("pinned_roles") if isinstance(raw_item.get("pinned_roles"), list) else []:
            role = str(role_raw or "").strip().lower()
            if not role:
                continue
            role_pinned.setdefault(role, set()).add(cid)

    for item in skills.get("official_anthropic") if isinstance(skills.get("official_anthropic"), list) else []:
        if isinstance(item, dict):
            _append_skill_entry(item, default_source_id="anthropic_skills")

    for item in skills.get("curated") if isinstance(skills.get("curated"), list) else []:
        if isinstance(item, dict):
            _append_skill_entry(item, default_source_id="github_skills_curated")

    role_defaults = doc.get("role_defaults") if isinstance(doc.get("role_defaults"), dict) else {}
    for role_name, role_cfg in role_defaults.items():
        role = str(role_name or "").strip().lower()
        if not role or not isinstance(role_cfg, dict):
            continue
        pinned = role_cfg.get("pinned") if isinstance(role_cfg.get("pinned"), list) else []
        for item in pinned:
            cid = str(item or "").strip()
            if cid:
                role_pinned.setdefault(role, set()).add(cid)

    compiled["capability_levels"] = capability_levels
    compiled["skill_source_levels"] = skill_source_levels
    compiled["role_pinned"] = role_pinned
    compiled["curated_mcp_entries"] = curated_mcp_entries
    compiled["curated_skill_entries"] = curated_skill_entries
    return compiled


def _allowlist_policy() -> Dict[str, Any]:
    with _POLICY_LOCK:
        snapshot = _allowlist_effective_snapshot()
        key = str(snapshot.get("revision") or "")
        cached = _POLICY_CACHE.get("compiled")
        if _POLICY_CACHE.get("key") == key and isinstance(cached, dict):
            return cached

        error = ""
        compiled = _policy_default_compiled()
        overlay_error = str(snapshot.get("overlay_error") or "").strip()
        if overlay_error:
            error = overlay_error
        try:
            compiled = _compile_allowlist_policy(snapshot.get("effective"))
        except Exception as e:
            error = str(e)
            compiled = _policy_default_compiled()

        _POLICY_CACHE["key"] = key
        _POLICY_CACHE["compiled"] = compiled
        _POLICY_CACHE["source"] = (
            f"{snapshot.get('default_source')};overlay={snapshot.get('overlay_source') or '<none>'}"
        )
        _POLICY_CACHE["error"] = error
        return compiled


def _allowlist_validate_overlay_doc(overlay_doc: Dict[str, Any]) -> Tuple[bool, str, Dict[str, Any], Dict[str, Any], str]:
    default_doc, _ = _load_allowlist_default_doc()
    effective_doc = _merge_allowlist_docs(default_doc, overlay_doc)
    try:
        _compile_allowlist_policy(effective_doc)
    except Exception as e:
        return False, str(e), default_doc, effective_doc, ""
    revision_payload = json.dumps(
        {"default": default_doc, "overlay": overlay_doc},
        ensure_ascii=False,
        sort_keys=True,
    )
    revision = hashlib.sha1(revision_payload.encode("utf-8")).hexdigest()
    return True, "", default_doc, effective_doc, revision


def handle_capability_allowlist_get(args: Dict[str, Any]) -> DaemonResponse:
    try:
        with _POLICY_LOCK:
            snapshot = _allowlist_effective_snapshot()
            _ = _allowlist_policy()
        return DaemonResponse(
            ok=True,
            result={
                "default": snapshot.get("default") if isinstance(snapshot.get("default"), dict) else {},
                "overlay": snapshot.get("overlay") if isinstance(snapshot.get("overlay"), dict) else {},
                "effective": snapshot.get("effective") if isinstance(snapshot.get("effective"), dict) else {},
                "revision": str(snapshot.get("revision") or ""),
                "default_source": str(snapshot.get("default_source") or ""),
                "overlay_source": str(snapshot.get("overlay_source") or ""),
                "overlay_error": str(snapshot.get("overlay_error") or ""),
                "policy_source": str(_POLICY_CACHE.get("source") or ""),
                "policy_error": str(_POLICY_CACHE.get("error") or ""),
            },
        )
    except Exception as e:
        return _error("capability_allowlist_get_failed", str(e))


def handle_capability_allowlist_validate(args: Dict[str, Any]) -> DaemonResponse:
    mode = str(args.get("mode") or "patch").strip().lower()
    overlay_arg = args.get("overlay")
    patch_arg = args.get("patch")
    if mode not in {"patch", "replace"}:
        return _error("invalid_request", "mode must be patch or replace")
    if mode == "replace":
        if not isinstance(overlay_arg, dict):
            return _error("invalid_request", "overlay must be an object when mode=replace")
        overlay_next = dict(overlay_arg)
    else:
        if not isinstance(patch_arg, dict):
            return _error("invalid_request", "patch must be an object when mode=patch")
        with _POLICY_LOCK:
            snapshot = _allowlist_effective_snapshot()
            overlay_cur = snapshot.get("overlay") if isinstance(snapshot.get("overlay"), dict) else {}
        overlay_next = _merge_allowlist_docs(overlay_cur, patch_arg)

    try:
        valid, reason, default_doc, effective_doc, revision = _allowlist_validate_overlay_doc(overlay_next)
    except Exception as e:
        return _error("capability_allowlist_validate_failed", str(e))
    return DaemonResponse(
        ok=True,
        result={
            "valid": bool(valid),
            "reason": str(reason or ""),
            "default": default_doc,
            "overlay": overlay_next,
            "effective": effective_doc,
            "revision": str(revision or ""),
        },
    )


def handle_capability_allowlist_update(args: Dict[str, Any]) -> DaemonResponse:
    by = str(args.get("by") or "user").strip() or "user"
    mode = str(args.get("mode") or "patch").strip().lower()
    expected_revision = str(args.get("expected_revision") or "").strip()
    overlay_arg = args.get("overlay")
    patch_arg = args.get("patch")
    if by != "user":
        return _error("permission_denied", "only user can update capability allowlist overlay")
    if mode not in {"patch", "replace"}:
        return _error("invalid_request", "mode must be patch or replace")

    try:
        with _POLICY_LOCK:
            snapshot = _allowlist_effective_snapshot()
            current_revision = str(snapshot.get("revision") or "")
            if expected_revision and expected_revision != current_revision:
                return _error(
                    "allowlist_revision_mismatch",
                    "expected_revision does not match current revision",
                    details={"expected_revision": expected_revision, "current_revision": current_revision},
                )
            overlay_cur = snapshot.get("overlay") if isinstance(snapshot.get("overlay"), dict) else {}
            if mode == "replace":
                if not isinstance(overlay_arg, dict):
                    return _error("invalid_request", "overlay must be an object when mode=replace")
                overlay_next = dict(overlay_arg)
            else:
                if not isinstance(patch_arg, dict):
                    return _error("invalid_request", "patch must be an object when mode=patch")
                overlay_next = _merge_allowlist_docs(overlay_cur, patch_arg)

            valid, reason, default_doc, effective_doc, revision = _allowlist_validate_overlay_doc(overlay_next)
            if not valid:
                return _error(
                    "allowlist_validation_failed",
                    reason or "overlay validation failed",
                    details={"current_revision": current_revision},
                )
            _write_allowlist_overlay_doc(overlay_next)
            _clear_policy_cache()
            policy_compiled = _allowlist_policy()
            _ = policy_compiled  # cache warm-up for deterministic post-update behavior
    except Exception as e:
        return _error("capability_allowlist_update_failed", str(e))

    return DaemonResponse(
        ok=True,
        result={
            "updated": True,
            "revision": str(revision or ""),
            "default": default_doc,
            "overlay": overlay_next,
            "effective": effective_doc,
            "policy_source": str(_POLICY_CACHE.get("source") or ""),
            "policy_error": str(_POLICY_CACHE.get("error") or ""),
        },
    )


def handle_capability_allowlist_reset(args: Dict[str, Any]) -> DaemonResponse:
    by = str(args.get("by") or "user").strip() or "user"
    if by != "user":
        return _error("permission_denied", "only user can reset capability allowlist overlay")
    path = _allowlist_user_overlay_path()
    removed = False
    try:
        with _POLICY_LOCK:
            try:
                if path.exists() and path.is_file():
                    path.unlink()
                    removed = True
            except Exception:
                removed = False
            _clear_policy_cache()
            snapshot = _allowlist_effective_snapshot()
            _ = _allowlist_policy()
    except Exception as e:
        return _error("capability_allowlist_reset_failed", str(e))
    return DaemonResponse(
        ok=True,
        result={
            "reset": True,
            "removed_overlay_file": bool(removed),
            "revision": str(snapshot.get("revision") or ""),
            "default": snapshot.get("default") if isinstance(snapshot.get("default"), dict) else {},
            "overlay": snapshot.get("overlay") if isinstance(snapshot.get("overlay"), dict) else {},
            "effective": snapshot.get("effective") if isinstance(snapshot.get("effective"), dict) else {},
            "default_source": str(snapshot.get("default_source") or ""),
            "overlay_source": str(snapshot.get("overlay_source") or ""),
            "overlay_error": str(snapshot.get("overlay_error") or ""),
            "policy_source": str(_POLICY_CACHE.get("source") or ""),
            "policy_error": str(_POLICY_CACHE.get("error") or ""),
        },
    )
