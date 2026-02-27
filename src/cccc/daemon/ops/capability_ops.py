"""Capability registry and progressive MCP disclosure operation handlers."""

from __future__ import annotations

import json
import os
import re
import hashlib
from importlib import resources as pkg_resources
import subprocess
import threading
import uuid
from datetime import datetime, timedelta, timezone
from pathlib import Path
from typing import Any, Dict, List, Optional, Tuple
from urllib.error import HTTPError, URLError
from urllib.parse import quote, urlencode
from urllib.request import Request, urlopen

import yaml

from ...contracts.v1 import DaemonError, DaemonResponse
from ...kernel.actors import get_effective_role
from ...kernel.capabilities import (
    BUILTIN_CAPABILITY_PACKS,
    CORE_TOOL_NAMES,
    all_builtin_pack_ids,
    resolve_visible_tool_names,
)
from ...kernel.group import load_group
from ...paths import ensure_home
from ...util.fs import atomic_write_json, read_json
from ...util.time import parse_utc_iso, utc_now_iso

_SOURCE_IDS = (
    "mcp_registry_official",
    "anthropic_skills",
    "agentskills_validator",
)

_MCP_REGISTRY_BASE = "https://registry.modelcontextprotocol.io"
_MCP_REGISTRY_PAGE_LIMIT = 100
_GITHUB_API_BASE = "https://api.github.com"
_RAW_GITHUB_BASE = "https://raw.githubusercontent.com"
_SKILL_NAME_RE = re.compile(r"^[a-z0-9]+(?:-[a-z0-9]+)*$")
_STATE_LOCK = threading.RLock()
_CATALOG_LOCK = threading.RLock()
_RUNTIME_LOCK = threading.RLock()
_AUDIT_LOCK = threading.RLock()
_POLICY_LOCK = threading.RLock()

_LEVEL_INDEXED = "indexed"
_LEVEL_MOUNTED = "mounted"
_LEVEL_ENABLED = "enabled"
_LEVEL_PINNED = "pinned"
_LEVELS = {_LEVEL_INDEXED, _LEVEL_MOUNTED, _LEVEL_ENABLED, _LEVEL_PINNED}
_POLICY_LEVEL_ORDER = {
    _LEVEL_INDEXED: 0,
    _LEVEL_MOUNTED: 1,
    _LEVEL_ENABLED: 2,
    _LEVEL_PINNED: 3,
}
_POLICY_CACHE: Dict[str, Any] = {
    "key": "",
    "compiled": None,
    "source": "",
    "error": "",
}


def _error(code: str, message: str, *, details: Optional[Dict[str, Any]] = None) -> DaemonResponse:
    return DaemonResponse(
        ok=False,
        error=DaemonError(code=code, message=message, details=(details or {})),
    )


def _capability_root() -> Path:
    return ensure_home() / "state" / "capabilities"


def _state_path() -> Path:
    return _capability_root() / "state.json"


def _catalog_path() -> Path:
    return _capability_root() / "catalog.json"


def _runtime_path() -> Path:
    return _capability_root() / "runtime.json"


def _audit_path() -> Path:
    return _capability_root() / "audit.jsonl"


def _source_state_template(sync_state: str = "never") -> Dict[str, Any]:
    return {
        "sync_state": sync_state,
        "last_synced_at": "",
        "staleness_seconds": None,
        "error": "",
        "record_count": 0,
        "next_cursor": "",
        "updated_since": "",
    }


def _new_state_doc() -> Dict[str, Any]:
    now = utc_now_iso()
    return {
        "v": 1,
        "created_at": now,
        "updated_at": now,
        "group_enabled": {},
        "actor_enabled": {},
        "session_enabled": {},
    }


def _new_catalog_doc() -> Dict[str, Any]:
    now = utc_now_iso()
    return {
        "v": 1,
        "created_at": now,
        "updated_at": now,
        "sources": {
            "mcp_registry_official": _source_state_template("never"),
            "anthropic_skills": _source_state_template("never"),
            "agentskills_validator": _source_state_template("never"),
        },
        "records": {},
    }


def _new_runtime_doc() -> Dict[str, Any]:
    now = utc_now_iso()
    return {
        "v": 1,
        "created_at": now,
        "updated_at": now,
        "installations": {},
        "actor_bindings": {},
    }


def _normalize_state_doc(raw: Any) -> Dict[str, Any]:
    if not isinstance(raw, dict):
        return _new_state_doc()
    doc = dict(raw)
    now = utc_now_iso()
    doc["v"] = 1
    if not str(doc.get("created_at") or "").strip():
        doc["created_at"] = now
    if not str(doc.get("updated_at") or "").strip():
        doc["updated_at"] = now

    group_enabled_raw = doc.get("group_enabled")
    group_enabled: Dict[str, List[str]] = {}
    if isinstance(group_enabled_raw, dict):
        for group_id, items in group_enabled_raw.items():
            gid = str(group_id or "").strip()
            if not gid or not isinstance(items, list):
                continue
            clean = sorted({str(x or "").strip() for x in items if str(x or "").strip()})
            if clean:
                group_enabled[gid] = clean
    doc["group_enabled"] = group_enabled

    actor_enabled_raw = doc.get("actor_enabled")
    actor_enabled: Dict[str, Dict[str, List[str]]] = {}
    if isinstance(actor_enabled_raw, dict):
        for group_id, per_group in actor_enabled_raw.items():
            gid = str(group_id or "").strip()
            if not gid or not isinstance(per_group, dict):
                continue
            clean_group: Dict[str, List[str]] = {}
            for actor_id, items in per_group.items():
                aid = str(actor_id or "").strip()
                if not aid or not isinstance(items, list):
                    continue
                clean = sorted({str(x or "").strip() for x in items if str(x or "").strip()})
                if clean:
                    clean_group[aid] = clean
            if clean_group:
                actor_enabled[gid] = clean_group
    doc["actor_enabled"] = actor_enabled

    session_enabled_raw = doc.get("session_enabled")
    session_enabled: Dict[str, Dict[str, List[Dict[str, str]]]] = {}
    if isinstance(session_enabled_raw, dict):
        for group_id, per_group in session_enabled_raw.items():
            gid = str(group_id or "").strip()
            if not gid or not isinstance(per_group, dict):
                continue
            clean_group: Dict[str, List[Dict[str, str]]] = {}
            for actor_id, entries in per_group.items():
                aid = str(actor_id or "").strip()
                if not aid or not isinstance(entries, list):
                    continue
                clean_entries: List[Dict[str, str]] = []
                for item in entries:
                    if not isinstance(item, dict):
                        continue
                    cap_id = str(item.get("capability_id") or "").strip()
                    expires_at = str(item.get("expires_at") or "").strip()
                    if not cap_id or not expires_at:
                        continue
                    if parse_utc_iso(expires_at) is None:
                        continue
                    clean_entries.append({"capability_id": cap_id, "expires_at": expires_at})
                if clean_entries:
                    clean_group[aid] = clean_entries
            if clean_group:
                session_enabled[gid] = clean_group
    doc["session_enabled"] = session_enabled
    return doc


def _normalize_catalog_doc(raw: Any) -> Dict[str, Any]:
    if not isinstance(raw, dict):
        return _new_catalog_doc()
    doc = dict(raw)
    now = utc_now_iso()
    doc["v"] = 1
    if not str(doc.get("created_at") or "").strip():
        doc["created_at"] = now
    if not str(doc.get("updated_at") or "").strip():
        doc["updated_at"] = now

    sources_raw = doc.get("sources")
    sources = sources_raw if isinstance(sources_raw, dict) else {}
    normalized_sources: Dict[str, Dict[str, Any]] = {}
    for source_id in _SOURCE_IDS:
        item = sources.get(source_id)
        normalized = _source_state_template("never")
        if isinstance(item, dict):
            normalized.update(
                {
                    "sync_state": str(item.get("sync_state") or "never"),
                    "last_synced_at": str(item.get("last_synced_at") or ""),
                    "staleness_seconds": item.get("staleness_seconds"),
                    "error": str(item.get("error") or ""),
                    "record_count": int(item.get("record_count") or 0),
                    "next_cursor": str(item.get("next_cursor") or ""),
                    "updated_since": str(item.get("updated_since") or ""),
                }
            )
        normalized_sources[source_id] = normalized
    doc["sources"] = normalized_sources

    records_raw = doc.get("records")
    records: Dict[str, Dict[str, Any]] = {}
    if isinstance(records_raw, dict):
        for capability_id, item in records_raw.items():
            cap_id = str(capability_id or "").strip()
            if not cap_id or not isinstance(item, dict):
                continue
            candidate = dict(item)
            candidate["capability_id"] = cap_id
            records[cap_id] = candidate
    doc["records"] = records
    return doc


def _normalize_runtime_doc(raw: Any) -> Dict[str, Any]:
    if not isinstance(raw, dict):
        return _new_runtime_doc()
    doc = dict(raw)
    now = utc_now_iso()
    doc["v"] = 1
    if not str(doc.get("created_at") or "").strip():
        doc["created_at"] = now
    if not str(doc.get("updated_at") or "").strip():
        doc["updated_at"] = now
    installs_raw = doc.get("installations")
    installs: Dict[str, Dict[str, Any]] = {}
    if isinstance(installs_raw, dict):
        for capability_id, item in installs_raw.items():
            cid = str(capability_id or "").strip()
            if not cid or not isinstance(item, dict):
                continue
            tools_raw = item.get("tools")
            tools: List[Dict[str, Any]] = []
            if isinstance(tools_raw, list):
                for tool in tools_raw:
                    if not isinstance(tool, dict):
                        continue
                    name = str(tool.get("name") or "").strip()
                    real_name = str(tool.get("real_tool_name") or "").strip()
                    if not name or not real_name:
                        continue
                    schema = tool.get("inputSchema")
                    if not isinstance(schema, dict):
                        schema = {"type": "object", "properties": {}, "required": []}
                    tools.append(
                        {
                            "name": name,
                            "real_tool_name": real_name,
                            "description": str(tool.get("description") or "").strip(),
                            "inputSchema": schema,
                        }
                    )
            installs[cid] = {
                "capability_id": cid,
                "state": str(item.get("state") or "").strip() or "unknown",
                "installer": str(item.get("installer") or "").strip(),
                "install_mode": str(item.get("install_mode") or "").strip(),
                "invoker": dict(item.get("invoker")) if isinstance(item.get("invoker"), dict) else {},
                "tools": tools,
                "last_error": str(item.get("last_error") or "").strip(),
                "updated_at": str(item.get("updated_at") or "").strip() or now,
            }
    doc["installations"] = installs
    actor_bindings_raw = doc.get("actor_bindings")
    actor_bindings: Dict[str, Dict[str, Dict[str, Dict[str, Any]]]] = {}
    if isinstance(actor_bindings_raw, dict):
        for group_id, per_group in actor_bindings_raw.items():
            gid = str(group_id or "").strip()
            if not gid or not isinstance(per_group, dict):
                continue
            clean_group: Dict[str, Dict[str, Dict[str, Any]]] = {}
            for actor_id, per_actor in per_group.items():
                aid = str(actor_id or "").strip()
                if not aid or not isinstance(per_actor, dict):
                    continue
                clean_actor: Dict[str, Dict[str, Any]] = {}
                for capability_id, item in per_actor.items():
                    cid = str(capability_id or "").strip()
                    if not cid or not isinstance(item, dict):
                        continue
                    clean_actor[cid] = {
                        "state": str(item.get("state") or "").strip() or "unknown",
                        "last_error": str(item.get("last_error") or "").strip(),
                        "updated_at": str(item.get("updated_at") or "").strip() or now,
                    }
                if clean_actor:
                    clean_group[aid] = clean_actor
            if clean_group:
                actor_bindings[gid] = clean_group
    doc["actor_bindings"] = actor_bindings
    return doc


def _load_state_doc() -> Tuple[Path, Dict[str, Any]]:
    path = _state_path()
    return path, _normalize_state_doc(read_json(path))


def _save_state_doc(path: Path, doc: Dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    doc["updated_at"] = utc_now_iso()
    atomic_write_json(path, doc, indent=2)


def _load_catalog_doc() -> Tuple[Path, Dict[str, Any]]:
    path = _catalog_path()
    return path, _normalize_catalog_doc(read_json(path))


def _save_catalog_doc(path: Path, doc: Dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    doc["updated_at"] = utc_now_iso()
    atomic_write_json(path, doc, indent=2)


def _load_runtime_doc() -> Tuple[Path, Dict[str, Any]]:
    path = _runtime_path()
    return path, _normalize_runtime_doc(read_json(path))


def _save_runtime_doc(path: Path, doc: Dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    doc["updated_at"] = utc_now_iso()
    atomic_write_json(path, doc, indent=2)


def _runtime_actor_bindings(runtime_doc: Dict[str, Any]) -> Dict[str, Dict[str, Dict[str, Dict[str, Any]]]]:
    raw = runtime_doc.get("actor_bindings")
    return raw if isinstance(raw, dict) else {}


def _set_runtime_actor_binding(
    runtime_doc: Dict[str, Any],
    *,
    group_id: str,
    actor_id: str,
    capability_id: str,
    state: str,
    last_error: str = "",
) -> None:
    gid = str(group_id or "").strip()
    aid = str(actor_id or "").strip()
    cid = str(capability_id or "").strip()
    if not gid or not aid or not cid:
        return
    bindings = _runtime_actor_bindings(runtime_doc)
    per_group = bindings.setdefault(gid, {})
    per_actor = per_group.setdefault(aid, {})
    per_actor[cid] = {
        "state": str(state or "").strip() or "unknown",
        "last_error": str(last_error or "").strip(),
        "updated_at": utc_now_iso(),
    }
    runtime_doc["actor_bindings"] = bindings


def _remove_runtime_actor_binding(
    runtime_doc: Dict[str, Any],
    *,
    group_id: str,
    actor_id: str,
    capability_id: str,
) -> int:
    gid = str(group_id or "").strip()
    aid = str(actor_id or "").strip()
    cid = str(capability_id or "").strip()
    if not gid or not aid or not cid:
        return 0
    bindings = _runtime_actor_bindings(runtime_doc)
    per_group = bindings.get(gid) if isinstance(bindings.get(gid), dict) else {}
    per_actor = per_group.get(aid) if isinstance(per_group.get(aid), dict) else {}
    if cid not in per_actor:
        return 0
    per_actor.pop(cid, None)
    if per_actor:
        per_group[aid] = per_actor
    else:
        per_group.pop(aid, None)
    if per_group:
        bindings[gid] = per_group
    else:
        bindings.pop(gid, None)
    runtime_doc["actor_bindings"] = bindings
    return 1


def _remove_runtime_group_capability_bindings(
    runtime_doc: Dict[str, Any],
    *,
    group_id: str,
    capability_id: str,
) -> int:
    gid = str(group_id or "").strip()
    cid = str(capability_id or "").strip()
    if not gid or not cid:
        return 0
    bindings = _runtime_actor_bindings(runtime_doc)
    per_group = bindings.get(gid) if isinstance(bindings.get(gid), dict) else {}
    removed = 0
    actor_ids = list(per_group.keys()) if isinstance(per_group, dict) else []
    for aid in actor_ids:
        per_actor = per_group.get(aid) if isinstance(per_group.get(aid), dict) else {}
        if cid in per_actor:
            per_actor.pop(cid, None)
            removed += 1
        if per_actor:
            per_group[aid] = per_actor
        else:
            per_group.pop(aid, None)
    if per_group:
        bindings[gid] = per_group
    else:
        bindings.pop(gid, None)
    runtime_doc["actor_bindings"] = bindings
    return removed


def _remove_runtime_capability_bindings_all_groups(
    runtime_doc: Dict[str, Any],
    *,
    capability_id: str,
) -> int:
    cid = str(capability_id or "").strip()
    if not cid:
        return 0
    bindings = _runtime_actor_bindings(runtime_doc)
    removed = 0
    group_ids = list(bindings.keys()) if isinstance(bindings, dict) else []
    for gid in group_ids:
        per_group = bindings.get(gid) if isinstance(bindings.get(gid), dict) else {}
        actor_ids = list(per_group.keys()) if isinstance(per_group, dict) else []
        for aid in actor_ids:
            per_actor = per_group.get(aid) if isinstance(per_group.get(aid), dict) else {}
            if cid in per_actor:
                per_actor.pop(cid, None)
                removed += 1
            if per_actor:
                per_group[aid] = per_actor
            else:
                per_group.pop(aid, None)
        if per_group:
            bindings[gid] = per_group
        else:
            bindings.pop(gid, None)
    runtime_doc["actor_bindings"] = bindings
    return removed


def _append_audit_event(
    *,
    action_id: str,
    op: str,
    group_id: str,
    actor_id: str,
    by: str,
    capability_id: str,
    scope: str,
    enabled: bool,
    outcome: str,
    details: Optional[Dict[str, Any]] = None,
) -> None:
    event = {
        "v": 1,
        "event_id": f"cae_{uuid.uuid4().hex}",
        "at": utc_now_iso(),
        "action_id": str(action_id or "").strip(),
        "op": str(op or "").strip(),
        "group_id": str(group_id or "").strip(),
        "actor_id": str(actor_id or "").strip(),
        "by": str(by or "").strip(),
        "capability_id": str(capability_id or "").strip(),
        "scope": str(scope or "").strip(),
        "enabled": bool(enabled),
        "outcome": str(outcome or "").strip(),
        "details": details if isinstance(details, dict) else {},
    }
    line = json.dumps(event, ensure_ascii=False, default=str)
    with _AUDIT_LOCK:
        path = _audit_path()
        path.parent.mkdir(parents=True, exist_ok=True)
        with path.open("a", encoding="utf-8") as f:
            f.write(line + "\n")


def _ensure_group(group_id: str):
    gid = str(group_id or "").strip()
    if not gid:
        raise ValueError("missing_group_id")
    group = load_group(gid)
    if group is None:
        raise LookupError(f"group not found: {gid}")
    return group


def _is_foreman(group: Any, actor_id: str) -> bool:
    aid = str(actor_id or "").strip()
    if not aid:
        return False
    try:
        return str(get_effective_role(group, aid) or "") == "foreman"
    except Exception:
        return False


def _normalize_scope(raw_scope: Any) -> str:
    scope = str(raw_scope or "session").strip().lower()
    if scope not in {"group", "actor", "session"}:
        raise ValueError(f"invalid scope: {scope}")
    return scope


def _http_get_json(url: str, *, headers: Optional[Dict[str, str]] = None, timeout: float = 10.0) -> Any:
    req = Request(url, method="GET")
    all_headers = {"Accept": "application/json"}
    if isinstance(headers, dict):
        all_headers.update({str(k): str(v) for k, v in headers.items()})
    for k, v in all_headers.items():
        req.add_header(k, v)
    with urlopen(req, timeout=timeout) as resp:
        payload = resp.read().decode("utf-8", errors="replace")
    return json.loads(payload)


def _http_get_json_obj(url: str, *, headers: Optional[Dict[str, str]] = None, timeout: float = 10.0) -> Dict[str, Any]:
    data = _http_get_json(url, headers=headers, timeout=timeout)
    if not isinstance(data, dict):
        raise ValueError("response is not a JSON object")
    return data


def _env_int(name: str, default: int) -> int:
    raw = str(os.environ.get(name) or "").strip()
    if not raw:
        return default
    try:
        return int(raw)
    except Exception:
        return default


def _env_bool(name: str, default: bool) -> bool:
    raw = str(os.environ.get(name) or "").strip().lower()
    if not raw:
        return default
    if raw in {"1", "true", "yes", "on"}:
        return True
    if raw in {"0", "false", "no", "off"}:
        return False
    return default


def _quota_limit(name: str, default: int, *, minimum: int = 1, maximum: int = 10_000) -> int:
    raw = _env_int(name, default)
    return max(minimum, min(int(raw or default), maximum))


def _normalize_policy_level(raw: Any, *, default: str = _LEVEL_INDEXED) -> str:
    level = str(raw or "").strip().lower()
    if level not in _LEVELS:
        return default
    return level


def _policy_level_visible(level: str) -> bool:
    return _normalize_policy_level(level) != _LEVEL_INDEXED


def _policy_default_compiled() -> Dict[str, Any]:
    return {
        "source_levels": {
            "cccc_builtin": _LEVEL_ENABLED,
            "anthropic_skills": _LEVEL_MOUNTED,
            "mcp_registry_official": _LEVEL_INDEXED,
            "agentskills_validator": _LEVEL_INDEXED,
        },
        "capability_levels": {},
        "skill_source_levels": {},
        "role_pinned": {},
    }


def _resolve_allowlist_override_path() -> Optional[Path]:
    env_path = str(os.environ.get("CCCC_CAPABILITY_ALLOWLIST_PATH") or "").strip()
    if env_path:
        p = Path(env_path).expanduser()
        if p.exists() and p.is_file():
            return p
        return None
    default_path = ensure_home() / "config" / "capability-allowlist.yaml"
    if default_path.exists() and default_path.is_file():
        return default_path
    return None


def _load_allowlist_text() -> Tuple[str, str, str]:
    override = _resolve_allowlist_override_path()
    if override is not None:
        try:
            text = override.read_text(encoding="utf-8")
        except Exception:
            text = ""
        mtime_ns = 0
        try:
            mtime_ns = int(override.stat().st_mtime_ns)
        except Exception:
            mtime_ns = 0
        key = f"file:{str(override)}:{mtime_ns}"
        return key, str(override), text
    try:
        text = pkg_resources.files("cccc.resources").joinpath("capability-allowlist.default.yaml").read_text(
            encoding="utf-8"
        )
    except Exception:
        text = ""
    key = f"builtin:{hashlib.sha1(text.encode('utf-8')).hexdigest()}"
    return key, "builtin:cccc.resources/capability-allowlist.default.yaml", text


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
    for item in doc.get("mcp_overrides") if isinstance(doc.get("mcp_overrides"), list) else []:
        if not isinstance(item, dict):
            continue
        cid = str(item.get("capability_id") or "").strip()
        if not cid:
            continue
        capability_levels[cid] = _normalize_policy_level(item.get("level"), default=_LEVEL_MOUNTED)

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
    for item in skills.get("official_anthropic") if isinstance(skills.get("official_anthropic"), list) else []:
        if not isinstance(item, dict):
            continue
        cid = str(item.get("capability_id") or "").strip()
        if not cid:
            continue
        capability_levels[cid] = _normalize_policy_level(item.get("level"), default=_LEVEL_MOUNTED)
        for role_raw in item.get("pinned_roles") if isinstance(item.get("pinned_roles"), list) else []:
            role = str(role_raw or "").strip().lower()
            if not role:
                continue
            role_pinned.setdefault(role, set()).add(cid)

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
    return compiled


def _allowlist_policy() -> Dict[str, Any]:
    with _POLICY_LOCK:
        key, source, text = _load_allowlist_text()
        cached = _POLICY_CACHE.get("compiled")
        if _POLICY_CACHE.get("key") == key and isinstance(cached, dict):
            return cached

        error = ""
        compiled = _policy_default_compiled()
        if text.strip():
            try:
                parsed = yaml.safe_load(text)
                compiled = _compile_allowlist_policy(parsed)
            except Exception as e:
                error = str(e)
                compiled = _policy_default_compiled()

        _POLICY_CACHE["key"] = key
        _POLICY_CACHE["compiled"] = compiled
        _POLICY_CACHE["source"] = source
        _POLICY_CACHE["error"] = error
        return compiled


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


def _github_headers() -> Dict[str, str]:
    headers = {
        "User-Agent": "cccc-capability-sync/1.0",
        "Accept": "application/vnd.github+json",
    }
    token = str(os.environ.get("CCCC_CAPABILITY_GITHUB_TOKEN") or "").strip()
    if token:
        headers["Authorization"] = f"Bearer {token}"
    return headers


def _catalog_staleness_seconds(last_synced_at: str) -> Optional[int]:
    dt = parse_utc_iso(str(last_synced_at or ""))
    if dt is None:
        return None
    now = datetime.now(timezone.utc)
    return max(0, int((now - dt).total_seconds()))


def _sanitize_tool_token(raw: str) -> str:
    token = re.sub(r"[^a-zA-Z0-9_]+", "_", str(raw or "").strip().lower()).strip("_")
    return token or "tool"


def _build_synthetic_tool_name(capability_id: str, real_tool_name: str, *, used: set[str]) -> str:
    cap_hash = hashlib.sha1(str(capability_id or "").encode("utf-8")).hexdigest()[:8]
    base = f"cccc_ext_{cap_hash}_{_sanitize_tool_token(real_tool_name)}"
    name = base
    i = 2
    while name in used:
        name = f"{base}_{i}"
        i += 1
    used.add(name)
    return name


def _normalize_mcp_input_schema(schema: Any) -> Dict[str, Any]:
    if isinstance(schema, dict) and str(schema.get("type") or "").strip():
        return dict(schema)
    return {"type": "object", "properties": {}, "required": []}


def _normalize_discovered_tools(capability_id: str, tools: Any) -> List[Dict[str, Any]]:
    if not isinstance(tools, list):
        return []
    out: List[Dict[str, Any]] = []
    used: set[str] = set()
    for item in tools:
        if not isinstance(item, dict):
            continue
        real_name = str(item.get("name") or "").strip()
        if not real_name:
            continue
        out.append(
            {
                "name": _build_synthetic_tool_name(capability_id, real_name, used=used),
                "real_tool_name": real_name,
                "description": str(item.get("description") or "").strip(),
                "inputSchema": _normalize_mcp_input_schema(item.get("inputSchema")),
            }
        )
    return out


def _npx_package_command(rec: Dict[str, Any]) -> Optional[List[str]]:
    install_spec = rec.get("install_spec") if isinstance(rec.get("install_spec"), dict) else {}
    identifier = str(install_spec.get("identifier") or "").strip()
    if not identifier:
        return None
    version = str(install_spec.get("version") or "").strip()
    pkg = identifier
    if version and "@" in identifier[1:]:
        pkg = identifier
    elif version:
        pkg = f"{identifier}@{version}"
    runtime_hint = str(install_spec.get("runtime_hint") or "").strip().lower()
    if runtime_hint and runtime_hint not in {"npx"}:
        return None
    return ["npx", "-y", pkg]


def _stdio_mcp_roundtrip(command: List[str], requests: List[Dict[str, Any]], *, timeout_s: float) -> List[Dict[str, Any]]:
    proc = subprocess.Popen(
        command,
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        encoding="utf-8",
        errors="replace",
    )
    input_blob = "\n".join(json.dumps(req, ensure_ascii=False) for req in requests) + "\n"
    try:
        out, err = proc.communicate(input=input_blob, timeout=max(2.0, float(timeout_s)))
    except subprocess.TimeoutExpired:
        proc.kill()
        proc.communicate()
        raise TimeoutError("stdio mcp request timed out")
    if proc.returncode not in {0, None} and not out.strip():
        raise RuntimeError(f"stdio mcp exited with code {proc.returncode}: {err.strip()}")
    responses: List[Dict[str, Any]] = []
    for line in str(out or "").splitlines():
        raw = line.strip()
        if not raw:
            continue
        try:
            item = json.loads(raw)
        except Exception:
            continue
        if isinstance(item, dict):
            responses.append(item)
    return responses


def _extract_jsonrpc_result(
    responses: List[Dict[str, Any]],
    *,
    req_id: int,
    operation: str,
) -> Dict[str, Any]:
    for item in responses:
        if int(item.get("id") or -1) != int(req_id):
            continue
        err = item.get("error")
        if isinstance(err, dict):
            raise RuntimeError(f"{operation} failed: {str(err.get('message') or 'unknown error')}")
        result = item.get("result")
        return result if isinstance(result, dict) else {}
    raise RuntimeError(f"{operation} failed: missing response")


def _http_jsonrpc_request(
    url: str,
    payload: Dict[str, Any],
    *,
    timeout_s: float,
    session_id: str = "",
) -> Tuple[Dict[str, Any], str]:
    body = json.dumps(payload, ensure_ascii=False).encode("utf-8")
    req = Request(url, data=body, method="POST")
    req.add_header("Content-Type", "application/json")
    req.add_header("Accept", "application/json")
    if session_id:
        req.add_header("Mcp-Session-Id", session_id)
    with urlopen(req, timeout=max(2.0, float(timeout_s))) as resp:
        payload_text = resp.read().decode("utf-8", errors="replace")
        out_session = str(resp.headers.get("Mcp-Session-Id") or session_id or "").strip()
    data = json.loads(payload_text) if payload_text.strip() else {}
    if not isinstance(data, dict):
        raise ValueError("remote mcp response is not JSON object")
    return data, out_session


def _remote_mcp_call(url: str, method: str, params: Dict[str, Any], *, timeout_s: float) -> Dict[str, Any]:
    init_req = {
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {"name": "cccc-capability-runtime", "version": "1.0"},
        },
    }
    init_resp, session_id = _http_jsonrpc_request(url, init_req, timeout_s=timeout_s)
    if isinstance(init_resp.get("error"), dict):
        raise RuntimeError(str((init_resp.get("error") or {}).get("message") or "remote initialize failed"))
    call_req = {
        "jsonrpc": "2.0",
        "id": 2,
        "method": str(method or ""),
        "params": params if isinstance(params, dict) else {},
    }
    call_resp, _ = _http_jsonrpc_request(url, call_req, timeout_s=timeout_s, session_id=session_id)
    if isinstance(call_resp.get("error"), dict):
        raise RuntimeError(str((call_resp.get("error") or {}).get("message") or f"remote {method} failed"))
    result = call_resp.get("result")
    return result if isinstance(result, dict) else {}


def _supported_external_install_record(rec: Dict[str, Any]) -> Tuple[bool, str]:
    install_mode = str(rec.get("install_mode") or "").strip()
    spec = rec.get("install_spec") if isinstance(rec.get("install_spec"), dict) else {}
    if install_mode == "remote_only":
        transport = str(spec.get("transport") or "").strip().lower()
        if transport in {"", "streamable-http", "http", "sse"}:
            if str(spec.get("url") or "").strip():
                return True, ""
            return False, "missing_remote_url"
        return False, f"unsupported_remote_transport:{transport or 'unknown'}"
    if install_mode == "package":
        registry_type = str(spec.get("registry_type") or "").strip().lower()
        if registry_type and registry_type != "npm":
            return False, f"unsupported_registry_type:{registry_type}"
        if _npx_package_command(rec) is None:
            return False, "unsupported_runtime_hint"
        return True, ""
    return False, f"unsupported_install_mode:{install_mode or 'unknown'}"


def _install_external_capability(rec: Dict[str, Any], *, capability_id: str) -> Dict[str, Any]:
    install_mode = str(rec.get("install_mode") or "").strip()
    if install_mode == "remote_only":
        spec = rec.get("install_spec") if isinstance(rec.get("install_spec"), dict) else {}
        url = str(spec.get("url") or "").strip()
        if not url:
            raise ValueError("missing_remote_url")
        tools_result = _remote_mcp_call(url, "tools/list", {}, timeout_s=12.0)
        tools = _normalize_discovered_tools(capability_id, tools_result.get("tools"))
        if not tools:
            raise RuntimeError("remote tools/list returned no tools")
        return {
            "state": "installed",
            "installer": "remote_http",
            "install_mode": "remote_only",
            "invoker": {"type": "remote_http", "url": url},
            "tools": tools,
            "last_error": "",
            "updated_at": utc_now_iso(),
        }

    if install_mode == "package":
        command = _npx_package_command(rec)
        if not command:
            raise ValueError("unsupported_runtime_hint")
        requests = [
            {
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": {
                    "protocolVersion": "2024-11-05",
                    "capabilities": {},
                    "clientInfo": {"name": "cccc-capability-runtime", "version": "1.0"},
                },
            },
            {"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}},
        ]
        responses = _stdio_mcp_roundtrip(command, requests, timeout_s=15.0)
        tools_result = _extract_jsonrpc_result(responses, req_id=2, operation="tools/list")
        tools = _normalize_discovered_tools(capability_id, tools_result.get("tools"))
        if not tools:
            raise RuntimeError("package tools/list returned no tools")
        return {
            "state": "installed",
            "installer": "npm_npx",
            "install_mode": "package",
            "invoker": {"type": "npm_stdio", "command": command},
            "tools": tools,
            "last_error": "",
            "updated_at": utc_now_iso(),
        }

    raise ValueError(f"unsupported_install_mode:{install_mode or 'unknown'}")


def _invoke_installed_external_tool(
    install: Dict[str, Any],
    *,
    real_tool_name: str,
    arguments: Dict[str, Any],
) -> Dict[str, Any]:
    invoker = install.get("invoker") if isinstance(install.get("invoker"), dict) else {}
    invoker_type = str(invoker.get("type") or "").strip()
    if invoker_type == "remote_http":
        url = str(invoker.get("url") or "").strip()
        if not url:
            raise ValueError("missing_remote_url")
        return _remote_mcp_call(
            url,
            "tools/call",
            {"name": real_tool_name, "arguments": arguments if isinstance(arguments, dict) else {}},
            timeout_s=30.0,
        )
    if invoker_type == "npm_stdio":
        command = invoker.get("command")
        cmd = [str(x) for x in command] if isinstance(command, list) else []
        if not cmd:
            raise ValueError("missing_npm_command")
        requests = [
            {
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": {
                    "protocolVersion": "2024-11-05",
                    "capabilities": {},
                    "clientInfo": {"name": "cccc-capability-runtime", "version": "1.0"},
                },
            },
            {
                "jsonrpc": "2.0",
                "id": 2,
                "method": "tools/call",
                "params": {
                    "name": real_tool_name,
                    "arguments": arguments if isinstance(arguments, dict) else {},
                },
            },
        ]
        responses = _stdio_mcp_roundtrip(cmd, requests, timeout_s=45.0)
        return _extract_jsonrpc_result(responses, req_id=2, operation="tools/call")
    raise ValueError(f"unsupported_invoker:{invoker_type or 'unknown'}")


def _sync_mcp_registry_source(catalog: Dict[str, Any], *, force: bool = False) -> int:
    sources = catalog["sources"]
    state = sources["mcp_registry_official"]
    interval_s = max(60, _env_int("CCCC_CAPABILITY_MCP_SYNC_INTERVAL_SECONDS", 6 * 3600))
    stale = _catalog_staleness_seconds(str(state.get("last_synced_at") or ""))
    if (not force) and stale is not None and stale < interval_s:
        return 0

    max_pages = max(1, min(_env_int("CCCC_CAPABILITY_MCP_MAX_PAGES", 5), 50))
    records = catalog["records"]
    updated_since = str(state.get("updated_since") or "").strip()
    cursor = str(state.get("next_cursor") or "").strip()
    page = 0
    upserted = 0
    now_iso = utc_now_iso()

    try:
        while page < max_pages:
            params: Dict[str, str] = {"limit": str(_MCP_REGISTRY_PAGE_LIMIT)}
            if updated_since:
                params["updated_since"] = updated_since
            if cursor:
                params["cursor"] = cursor
            url = f"{_MCP_REGISTRY_BASE}/v0.1/servers?{urlencode(params)}"
            data = _http_get_json_obj(url, timeout=12.0)

            servers = data.get("servers")
            if not isinstance(servers, list):
                servers = []
            for item in servers:
                record = _normalize_mcp_registry_record(item, synced_at=now_iso)
                if record is None:
                    continue
                records[str(record["capability_id"])] = record
                upserted += 1

            metadata = data.get("metadata") if isinstance(data.get("metadata"), dict) else {}
            next_cursor = str(metadata.get("nextCursor") or "").strip()
            cursor = next_cursor
            page += 1
            if not cursor:
                break

        state["last_synced_at"] = now_iso
        state["staleness_seconds"] = 0
        state["sync_state"] = "fresh" if not cursor else "stale"
        state["error"] = ""
        state["record_count"] = sum(
            1
            for item in records.values()
            if isinstance(item, dict) and str(item.get("source_id") or "") == "mcp_registry_official"
        )
        state["next_cursor"] = cursor
        if not cursor:
            state["updated_since"] = now_iso
        return upserted
    except (HTTPError, URLError, TimeoutError, ValueError, json.JSONDecodeError) as e:
        state["sync_state"] = "degraded"
        state["error"] = str(e)
        state["staleness_seconds"] = _catalog_staleness_seconds(str(state.get("last_synced_at") or ""))
        return 0


def _mcp_registry_search_servers(*, query: str, limit: int) -> List[Dict[str, Any]]:
    q = str(query or "").strip()
    if not q:
        return []
    lim = max(1, min(int(limit or 20), _MCP_REGISTRY_PAGE_LIMIT))
    params: Dict[str, str] = {
        "limit": str(lim),
        "search": q,
        "version": "latest",
    }
    url = f"{_MCP_REGISTRY_BASE}/v0.1/servers?{urlencode(params)}"
    data = _http_get_json_obj(url, timeout=8.0)
    servers = data.get("servers")
    if not isinstance(servers, list):
        return []
    out: List[Dict[str, Any]] = []
    for item in servers:
        if isinstance(item, dict):
            out.append(item)
    return out


def _remote_search_mcp_registry_records(*, query: str, limit: int) -> List[Dict[str, Any]]:
    now_iso = utc_now_iso()
    rows = _mcp_registry_search_servers(query=query, limit=limit)
    out: List[Dict[str, Any]] = []
    seen: set[str] = set()
    for item in rows:
        rec = _normalize_mcp_registry_record(item, synced_at=now_iso)
        if rec is None:
            continue
        cap_id = str(rec.get("capability_id") or "").strip()
        if not cap_id or cap_id in seen:
            continue
        seen.add(cap_id)
        out.append(rec)
    return out


def _fetch_mcp_registry_record_by_server_name(server_name: str) -> Optional[Dict[str, Any]]:
    name = str(server_name or "").strip()
    if not name:
        return None
    rows = _remote_search_mcp_registry_records(query=name, limit=20)
    for rec in rows:
        cap_id = str(rec.get("capability_id") or "").strip()
        if cap_id == f"mcp:{name}":
            return rec
    return None


def _normalize_mcp_registry_record(raw: Any, *, synced_at: str) -> Optional[Dict[str, Any]]:
    if not isinstance(raw, dict):
        return None
    server = raw.get("server") if isinstance(raw.get("server"), dict) else raw
    if not isinstance(server, dict):
        return None
    name = str(server.get("name") or "").strip()
    if not name:
        return None

    meta = raw.get("_meta") if isinstance(raw.get("_meta"), dict) else {}
    official = meta.get("io.modelcontextprotocol.registry/official")
    official = official if isinstance(official, dict) else {}
    status = str(official.get("status") or "active").strip().lower()
    updated_at_source = str(official.get("updatedAt") or official.get("publishedAt") or "").strip()
    if not updated_at_source:
        updated_at_source = synced_at

    packages = server.get("packages") if isinstance(server.get("packages"), list) else []
    remotes = server.get("remotes") if isinstance(server.get("remotes"), list) else []
    install_mode = "unknown"
    install_spec: Dict[str, Any] = {}
    if packages:
        install_mode = "package"
        pkg = packages[0] if isinstance(packages[0], dict) else {}
        install_spec = {
            "registry_type": str(pkg.get("registryType") or "").strip(),
            "identifier": str(pkg.get("identifier") or "").strip(),
            "version": str(pkg.get("version") or server.get("version") or "").strip(),
            "runtime_hint": str(pkg.get("runtimeHint") or "").strip(),
            "transport": str((pkg.get("transport") or {}).get("type") or "").strip()
            if isinstance(pkg.get("transport"), dict)
            else "",
        }
    elif remotes:
        install_mode = "remote_only"
        remote = remotes[0] if isinstance(remotes[0], dict) else {}
        install_spec = {
            "transport": str(remote.get("type") or "").strip(),
            "url": str(remote.get("url") or "").strip(),
        }

    qualification = "manual_review"
    if status == "deleted":
        qualification = "blocked"
    elif install_mode in {"package", "remote_only"}:
        qualification = "manual_review"

    supported, _ = _supported_external_install_record(
        {
            "install_mode": install_mode,
            "install_spec": install_spec,
        }
    )

    source_uri = ""
    repository = server.get("repository")
    if isinstance(repository, dict):
        source_uri = str(repository.get("url") or "").strip()
    if not source_uri:
        source_uri = str(server.get("websiteUrl") or "").strip()
    if not source_uri:
        source_uri = f"{_MCP_REGISTRY_BASE}/v0.1/servers/{quote(name, safe='')}/versions/{quote(str(server.get('version') or 'latest'), safe='')}"

    return {
        "capability_id": f"mcp:{name}",
        "kind": "mcp_toolpack",
        "name": name,
        "description_short": str(server.get("description") or server.get("title") or "").strip(),
        "tags": ["mcp", "external", "registry"],
        "source_id": "mcp_registry_official",
        "source_tier": "tier1",
        "source_uri": source_uri,
        "source_record_id": name,
        "source_record_version": str(server.get("version") or "").strip(),
        "updated_at_source": updated_at_source,
        "last_synced_at": synced_at,
        "sync_state": "fresh",
        "install_mode": install_mode,
        "install_spec": install_spec,
        "requirements": {},
        "license": "",
        "trust_tier": "tier1",
        "qualification_status": qualification,
        "qualification_reasons": (
            ["registry_status_deleted"] if qualification == "blocked" else ["external_install_requires_review"]
        ),
        "health_status": "ok" if status == "active" else status,
        "enable_supported": bool(supported and status == "active"),
    }


def _split_frontmatter(markdown: str) -> Tuple[Dict[str, Any], str]:
    raw = str(markdown or "")
    if not raw.startswith("---"):
        raise ValueError("missing YAML frontmatter")
    parts = raw.split("---", 2)
    if len(parts) < 3:
        raise ValueError("frontmatter not closed")
    doc = yaml.safe_load(parts[1])
    if not isinstance(doc, dict):
        raise ValueError("frontmatter must be a mapping")
    body = str(parts[2] or "")
    return doc, body


def _parse_frontmatter(markdown: str) -> Dict[str, Any]:
    doc, _ = _split_frontmatter(markdown)
    return doc


def _extract_skill_capsule(frontmatter: Dict[str, Any], body: str) -> str:
    """Build a short deterministic skill capsule for runtime use."""
    lines: List[str] = []
    name = str(frontmatter.get("name") or "").strip()
    desc = str(frontmatter.get("description") or "").strip()
    if name:
        lines.append(f"Skill: {name}")
    if desc:
        lines.append(f"Summary: {desc}")
    raw_body = str(body or "").strip()
    if raw_body:
        snippet = raw_body[:1600].strip()
        if snippet:
            lines.append("")
            lines.append("Notes:")
            lines.append(snippet)
    out = "\n".join(lines).strip()
    return out[:2400]


def _extract_skill_dependencies(frontmatter: Dict[str, Any]) -> List[str]:
    """Extract deterministic capability dependency list from frontmatter."""
    out: List[str] = []
    for key in ("requires_capabilities", "capabilities", "requires"):
        raw = frontmatter.get(key)
        if not isinstance(raw, list):
            continue
        for item in raw:
            cid = str(item or "").strip()
            if cid and cid not in out:
                out.append(cid)
    return out[:32]


def _validate_agentskill_frontmatter(frontmatter: Dict[str, Any], *, dir_name: str) -> List[str]:
    errors: List[str] = []
    name = str(frontmatter.get("name") or "").strip()
    desc = str(frontmatter.get("description") or "").strip()
    if not name:
        errors.append("missing name")
    else:
        if len(name) > 64:
            errors.append("name too long")
        if not _SKILL_NAME_RE.fullmatch(name):
            errors.append("invalid name format")
        if str(dir_name or "").strip() and name != str(dir_name or "").strip():
            errors.append("name does not match directory")
    if not desc:
        errors.append("missing description")
    elif len(desc) > 1024:
        errors.append("description too long")
    return errors


def _sync_anthropic_skills_source(catalog: Dict[str, Any], *, force: bool = False) -> int:
    sources = catalog["sources"]
    state = sources["anthropic_skills"]
    interval_s = max(60, _env_int("CCCC_CAPABILITY_SKILL_SYNC_INTERVAL_SECONDS", 12 * 3600))
    stale = _catalog_staleness_seconds(str(state.get("last_synced_at") or ""))
    if (not force) and stale is not None and stale < interval_s:
        return 0

    records = catalog["records"]
    now_iso = utc_now_iso()
    try:
        dirs_data = _http_get_json(
            f"{_GITHUB_API_BASE}/repos/anthropics/skills/contents/skills?per_page=200",
            headers=_github_headers(),
            timeout=12.0,
        )
        if not isinstance(dirs_data, list):
            raise ValueError("unexpected GitHub response shape")
        upserted = 0
        for item in dirs_data:
            if not isinstance(item, dict):
                continue
            if str(item.get("type") or "").strip() != "dir":
                continue
            dir_name = str(item.get("name") or "").strip()
            if not dir_name:
                continue
            raw_url = f"{_RAW_GITHUB_BASE}/anthropics/skills/main/skills/{dir_name}/SKILL.md"
            req = Request(raw_url, method="GET")
            for k, v in _github_headers().items():
                req.add_header(k, v)
            with urlopen(req, timeout=12.0) as resp:
                md = resp.read().decode("utf-8", errors="replace")
            frontmatter, body = _split_frontmatter(md)
            errors = _validate_agentskill_frontmatter(frontmatter, dir_name=dir_name)
            name = str(frontmatter.get("name") or dir_name).strip()
            description = str(frontmatter.get("description") or "").strip()
            license_text = str(frontmatter.get("license") or "").strip()
            tags_raw = frontmatter.get("tags")
            tags = (
                [str(x).strip() for x in tags_raw if str(x).strip()]
                if isinstance(tags_raw, list)
                else []
            )
            capsule_text = _extract_skill_capsule(frontmatter, body)
            requires_capabilities = _extract_skill_dependencies(frontmatter)
            qualification = "qualified"
            reasons: List[str] = []
            if errors:
                qualification = "blocked"
                reasons.extend(errors)
            elif ("proprietary" in license_text.lower()) or ("source-available" in license_text.lower()):
                qualification = "manual_review"
                reasons.append("license_requires_review")

            record = {
                "capability_id": f"skill:anthropic:{name}",
                "kind": "skill",
                "name": name,
                "description_short": description,
                "tags": ["skill", "external", "anthropic", *tags],
                "source_id": "anthropic_skills",
                "source_tier": "tier1",
                "source_uri": f"https://github.com/anthropics/skills/tree/main/skills/{dir_name}",
                "source_record_id": dir_name,
                "source_record_version": str(item.get("sha") or "").strip(),
                "updated_at_source": now_iso,
                "last_synced_at": now_iso,
                "sync_state": "fresh",
                "install_mode": "builtin",
                "install_spec": {},
                "requirements": {},
                "license": license_text,
                "trust_tier": "tier1",
                "qualification_status": qualification,
                "qualification_reasons": reasons,
                "health_status": "ok",
                "enable_supported": qualification != "blocked",
                "capsule_text": capsule_text,
                "requires_capabilities": requires_capabilities,
            }
            records[str(record["capability_id"])] = record
            upserted += 1

        state["last_synced_at"] = now_iso
        state["staleness_seconds"] = 0
        state["sync_state"] = "fresh"
        state["error"] = ""
        state["record_count"] = sum(
            1
            for item in records.values()
            if isinstance(item, dict) and str(item.get("source_id") or "") == "anthropic_skills"
        )
        return upserted
    except (HTTPError, URLError, TimeoutError, ValueError, json.JSONDecodeError) as e:
        state["sync_state"] = "degraded"
        state["error"] = str(e)
        state["staleness_seconds"] = _catalog_staleness_seconds(str(state.get("last_synced_at") or ""))
        return 0


def _mark_agentskills_validator_state(catalog: Dict[str, Any], *, force: bool = False) -> None:
    state = catalog["sources"]["agentskills_validator"]
    interval_s = max(60, _env_int("CCCC_CAPABILITY_AGENTSKILLS_SYNC_INTERVAL_SECONDS", 24 * 3600))
    stale = _catalog_staleness_seconds(str(state.get("last_synced_at") or ""))
    if (not force) and stale is not None and stale < interval_s and str(state.get("sync_state") or "") == "fresh":
        return
    state["sync_state"] = "fresh"
    state["last_synced_at"] = utc_now_iso()
    state["staleness_seconds"] = 0
    state["error"] = ""
    state["record_count"] = 0


def _mark_source_disabled(catalog: Dict[str, Any], source_id: str) -> None:
    sources = catalog.get("sources") if isinstance(catalog.get("sources"), dict) else {}
    state = sources.get(source_id) if isinstance(sources.get(source_id), dict) else _source_state_template("never")
    state["sync_state"] = "disabled"
    state["error"] = "source_disabled_by_policy"
    state["staleness_seconds"] = _catalog_staleness_seconds(str(state.get("last_synced_at") or ""))
    sources[source_id] = state
    catalog["sources"] = sources


def _sync_tier_rank(tier: str) -> int:
    t = str(tier or "").strip().lower()
    if t in {"builtin", "official", "tier1"}:
        return 0
    if t in {"tier2"}:
        return 1
    return 2


def _qualification_rank(q: str) -> int:
    status = str(q or "").strip().lower()
    if status == "qualified":
        return 0
    if status == "manual_review":
        return 1
    if status == "blocked":
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
        upserted["mcp_registry_official"] = int(_sync_mcp_registry_source(catalog_doc, force=force))
    else:
        _mark_source_disabled(catalog_doc, "mcp_registry_official")
        upserted["mcp_registry_official"] = 0

    if _env_bool("CCCC_CAPABILITY_SOURCE_ANTHROPIC_SKILLS_ENABLED", True):
        upserted["anthropic_skills"] = int(_sync_anthropic_skills_source(catalog_doc, force=force))
    else:
        _mark_source_disabled(catalog_doc, "anthropic_skills")
        upserted["anthropic_skills"] = 0

    if _env_bool("CCCC_CAPABILITY_SOURCE_AGENTSKILLS_VALIDATOR_ENABLED", True):
        _mark_agentskills_validator_state(catalog_doc, force=force)
        upserted["agentskills_validator"] = 0
    else:
        _mark_source_disabled(catalog_doc, "agentskills_validator")
        upserted["agentskills_validator"] = 0

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
            catalog_path, catalog_doc = _load_catalog_doc()
            result = _sync_catalog(catalog_doc, force=bool(force))
            if bool(result.get("changed")):
                _save_catalog_doc(catalog_path, catalog_doc)
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
    return q in haystack


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
            enabled_caps, mutated = _collect_enabled_capabilities(
                state_doc, group_id=group_id, actor_id=actor_id or "user"
            )
            if mutated:
                _save_state_doc(state_path, state_doc)
        enabled_set = set(enabled_caps)
        actor_role = _resolve_actor_role(group, actor_id)
        policy = _allowlist_policy()

        external_records: List[Dict[str, Any]] = []
        source_states: Dict[str, Dict[str, Any]] = {}
        remote_augmented = False
        remote_added = 0
        remote_error = ""
        with _CATALOG_LOCK:
            catalog_path, catalog_doc = _load_catalog_doc()
            search_auto_sync = _env_bool("CCCC_CAPABILITY_SEARCH_AUTO_SYNC", False)
            if include_external and search_auto_sync and _auto_sync_catalog(catalog_doc):
                _save_catalog_doc(catalog_path, catalog_doc)
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
            if (not _policy_level_visible(policy_level)) and (cap_id not in enabled_set):
                policy_hidden_count += 1
                continue
            visible_records.append(next_rec)
        records = visible_records
        records = [r for r in records if _search_matches(query, r)]

        should_remote_augment = (
            include_external
            and bool(str(query or "").strip())
            and len(records) == 0
            and kind_filter in {"", "mcp_toolpack"}
            and _env_bool("CCCC_CAPABILITY_SEARCH_REMOTE_FALLBACK", True)
            and _env_bool("CCCC_CAPABILITY_SOURCE_MCP_REGISTRY_ENABLED", True)
        )
        if should_remote_augment:
            remote_limit = max(1, min(_env_int("CCCC_CAPABILITY_SEARCH_REMOTE_FALLBACK_LIMIT", 40), 100))
            try:
                remote_rows = _remote_search_mcp_registry_records(query=query, limit=min(limit, remote_limit))
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
                        accepted.append(accepted_rec)
                    if accepted:
                        records.extend(accepted)
                        remote_augmented = True
                        with _CATALOG_LOCK:
                            path, doc = _load_catalog_doc()
                            rows = doc.get("records") if isinstance(doc.get("records"), dict) else {}
                            changed = False
                            for rec in accepted:
                                cap_id = str(rec.get("capability_id") or "").strip()
                                if not cap_id:
                                    continue
                                if rows.get(cap_id) != rec:
                                    rows[cap_id] = rec
                                    changed = True
                            if changed:
                                doc["records"] = rows
                                _refresh_source_record_counts(doc)
                                _save_catalog_doc(path, doc)
                                remote_added = len(accepted)
                            source_states = _render_source_states(doc)
            except Exception as e:
                remote_error = str(e)

        def _rank(item: Dict[str, Any]) -> Tuple[int, str]:
            cap_id = str(item.get("capability_id") or "")
            is_builtin = 0 if cap_id.startswith("pack:") else 1
            return (is_builtin, str(item.get("name") or cap_id).lower())

        records.sort(key=_rank)
        sliced = records[:limit]
        items: List[Dict[str, Any]] = []
        for rec in sliced:
            cap_id = str(rec.get("capability_id") or "")
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
                "qualification_status": str(rec.get("qualification_status") or "manual_review"),
                "sync_state": str(rec.get("sync_state") or ""),
                "enabled": cap_id in enabled_set,
                "enable_supported": bool(rec.get("enable_supported", cap_id.startswith("pack:"))),
                "install_mode": str(rec.get("install_mode") or ""),
                "policy_level": str(rec.get("policy_level") or ""),
                "tags": list(rec.get("tags") or []),
            }
            if cap_id.startswith("pack:"):
                item["tool_count"] = int(rec.get("tool_count") or 0)
            qualification = str(item.get("qualification_status") or "")
            if qualification == "blocked":
                item["enable_hint"] = "blocked"
                reasons = rec.get("qualification_reasons")
                if isinstance(reasons, list):
                    first = next((str(x).strip() for x in reasons if str(x).strip()), "")
                    if first:
                        item["blocked_reason"] = first
            elif qualification == "manual_review":
                item["enable_hint"] = "requires_approval"
            elif bool(item.get("enable_supported")):
                item["enable_hint"] = "enable_now"
            else:
                item["enable_hint"] = "unsupported"
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


def handle_capability_enable(args: Dict[str, Any]) -> DaemonResponse:
    group_id = str(args.get("group_id") or "").strip()
    by = str(args.get("by") or args.get("actor_id") or "").strip()
    actor_id = str(args.get("actor_id") or by).strip()
    capability_id = str(args.get("capability_id") or "").strip()
    enabled = bool(args.get("enabled", True))
    cleanup = bool(args.get("cleanup", False))
    ttl_seconds = int(args.get("ttl_seconds") or 3600)
    approve = bool(args.get("approve", False))
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
        actor_role = _resolve_actor_role(group, actor_id)
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
            policy_level = _effective_policy_level(
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

        # External capability path (M2): limited to remote_only + npm package via npx.
        with _CATALOG_LOCK:
            _, catalog_doc = _load_catalog_doc()
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
                            installs = (
                                runtime_doc.get("installations")
                                if isinstance(runtime_doc.get("installations"), dict)
                                else {}
                            )
                            if capability_id in installs:
                                installs.pop(capability_id, None)
                                runtime_doc["installations"] = installs
                                removed_installation = True
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
                        cpath, cdoc = _load_catalog_doc()
                        rows = cdoc.get("records") if isinstance(cdoc.get("records"), dict) else {}
                        rows[capability_id] = fetched
                        cdoc["records"] = rows
                        _refresh_source_record_counts(cdoc)
                        _save_catalog_doc(cpath, cdoc)
                    rec = fetched
            if not isinstance(rec, dict):
                _audit("denied", state="denied", error_code="capability_not_found")
                return _error(
                    "capability_not_found",
                    f"capability not found: {capability_id}",
                    details={"action_id": action_id},
                )

        policy_level = _effective_policy_level(
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
                        installs = (
                            runtime_doc.get("installations")
                            if isinstance(runtime_doc.get("installations"), dict)
                            else {}
                        )
                        if capability_id in installs:
                            installs.pop(capability_id, None)
                            runtime_doc["installations"] = installs
                            removed_installation = True
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

        qualification = str(rec.get("qualification_status") or "manual_review")
        if qualification == "blocked":
            _audit("denied", state="denied", error_code="qualification_blocked")
            return _error(
                "qualification_blocked",
                f"capability blocked by policy: {capability_id}",
                details={
                    "action_id": action_id,
                    "qualification_reasons": rec.get("qualification_reasons") or [],
                },
            )
        if qualification == "manual_review" and not approve:
            _audit("pending_approval", state="pending_approval", error_code="approval_required")
            return DaemonResponse(
                ok=True,
                result={
                    "action_id": action_id,
                    "group_id": group_id,
                    "actor_id": actor_id,
                    "capability_id": capability_id,
                    "scope": scope,
                    "enabled": False,
                    "state": "pending_approval",
                    "refresh_required": False,
                    "reason": "manual_review_required",
                    "policy_level": policy_level,
                },
            )
        if qualification == "manual_review" and approve and by != "user" and (not _is_foreman(group, by)):
            _audit("denied", state="denied", error_code="permission_denied")
            return _error(
                "permission_denied",
                "manual-review approval requires user or foreman",
                details={"action_id": action_id},
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
                    "reason": unsupported_reason or "unsupported_external_installer",
                    "policy_level": policy_level,
                },
            )

        install: Dict[str, Any]
        reused_cached_install = False
        with _RUNTIME_LOCK:
            _, runtime_doc_quota = _load_runtime_doc()
            installs_quota = (
                runtime_doc_quota.get("installations")
                if isinstance(runtime_doc_quota.get("installations"), dict)
                else {}
            )
            existing = installs_quota.get(capability_id) if isinstance(installs_quota, dict) else None
            if isinstance(existing, dict) and str(existing.get("state") or "").strip() == "installed":
                install = dict(existing)
                reused_cached_install = True
            else:
                if capability_id not in installs_quota and len(installs_quota) >= max_installations_total:
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
                install = _install_external_capability(rec, capability_id=capability_id)
            except Exception as e:
                with _RUNTIME_LOCK:
                    runtime_path, runtime_doc = _load_runtime_doc()
                    installs = (
                        runtime_doc.get("installations")
                        if isinstance(runtime_doc.get("installations"), dict)
                        else {}
                    )
                    installs[capability_id] = {
                        "capability_id": capability_id,
                        "state": "install_failed",
                        "installer": "",
                        "install_mode": str(rec.get("install_mode") or ""),
                        "invoker": {},
                        "tools": [],
                        "last_error": str(e),
                        "updated_at": utc_now_iso(),
                    }
                    runtime_doc["installations"] = installs
                    _set_runtime_actor_binding(
                        runtime_doc,
                        group_id=group_id,
                        actor_id=actor_id,
                        capability_id=capability_id,
                        state="install_failed",
                        last_error=str(e),
                    )
                    _save_runtime_doc(runtime_path, runtime_doc)
                _audit("failed", state="failed", error_code="install_failed", details={"error": str(e)})
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
                        "reason": f"install_failed:{str(e)}",
                        "policy_level": policy_level,
                    },
                )

            with _RUNTIME_LOCK:
                runtime_path, runtime_doc = _load_runtime_doc()
                installs = (
                    runtime_doc.get("installations")
                    if isinstance(runtime_doc.get("installations"), dict)
                    else {}
                )
                install["capability_id"] = capability_id
                installs[capability_id] = install
                runtime_doc["installations"] = installs
                _set_runtime_actor_binding(
                    runtime_doc,
                    group_id=group_id,
                    actor_id=actor_id,
                    capability_id=capability_id,
                    state="ready",
                    last_error="",
                )
                _save_runtime_doc(runtime_path, runtime_doc)
        else:
            with _RUNTIME_LOCK:
                runtime_path, runtime_doc = _load_runtime_doc()
                _set_runtime_actor_binding(
                    runtime_doc,
                    group_id=group_id,
                    actor_id=actor_id,
                    capability_id=capability_id,
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

        tools = install.get("tools") if isinstance(install.get("tools"), list) else []
        _audit("ready", state="ready", details={"installed_tool_count": len(tools)})
        return DaemonResponse(
            ok=True,
            result={
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
            },
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
            enabled_caps, mutated = _collect_enabled_capabilities(state_doc, group_id=group_id, actor_id=actor_id)
            if mutated:
                _save_state_doc(state_path, state_doc)

        builtin_enabled = [cap for cap in enabled_caps if cap in BUILTIN_CAPABILITY_PACKS]
        external_enabled = [cap for cap in enabled_caps if cap not in BUILTIN_CAPABILITY_PACKS]

        dynamic_tools: List[Dict[str, Any]] = []
        install_state_by_cap: Dict[str, str] = {}
        actor_binding_state_by_cap: Dict[str, Dict[str, str]] = {}
        with _RUNTIME_LOCK:
            _, runtime_doc = _load_runtime_doc()
            installs = (
                runtime_doc.get("installations")
                if isinstance(runtime_doc.get("installations"), dict)
                else {}
            )
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
            if isinstance(installs, dict):
                for cap_id, install in installs.items():
                    if not isinstance(install, dict):
                        continue
                    install_state_by_cap[str(cap_id)] = str(install.get("state") or "").strip()
            if isinstance(per_actor_bindings, dict):
                for cap_id, item in per_actor_bindings.items():
                    if not isinstance(item, dict):
                        continue
                    actor_binding_state_by_cap[str(cap_id)] = {
                        "state": str(item.get("state") or "").strip() or "unknown",
                        "last_error": str(item.get("last_error") or "").strip(),
                    }
            for capability_id in external_enabled:
                install = installs.get(capability_id) if isinstance(installs, dict) else None
                if not isinstance(install, dict):
                    continue
                if str(install.get("state") or "") != "installed":
                    continue
                tools = install.get("tools") if isinstance(install.get("tools"), list) else []
                for tool in tools:
                    if not isinstance(tool, dict):
                        continue
                    name = str(tool.get("name") or "").strip()
                    real_name = str(tool.get("real_tool_name") or "").strip()
                    if not name or not real_name:
                        continue
                    schema = _normalize_mcp_input_schema(tool.get("inputSchema"))
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
            set(resolve_visible_tool_names(builtin_enabled))
            | {str(x.get("name") or "").strip() for x in dynamic_tools if isinstance(x, dict)}
        )

        hidden_capabilities: List[Dict[str, Any]] = []
        for cap_id in all_builtin_pack_ids():
            if cap_id in builtin_enabled:
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
            _, catalog_doc = _load_catalog_doc()
            source_states = _render_source_states(catalog_doc)
            records_raw = catalog_doc.get("records") if isinstance(catalog_doc.get("records"), dict) else {}
            external_records = {
                str(cid): dict(rec)
                for cid, rec in records_raw.items()
                if isinstance(rec, dict) and str(cid) and (not str(cid).startswith("pack:"))
            }

        active_skills: List[Dict[str, Any]] = []
        for cap_id in enabled_caps:
            rec = external_records.get(cap_id)
            if not isinstance(rec, dict):
                continue
            if str(rec.get("kind") or "").strip().lower() != "skill":
                continue
            active_skills.append(
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

        pinned_skill_ids = set(group_enabled_map.get(group_id) or [])
        pinned_skill_ids.update(set(per_group_actor.get(actor_id) or []))
        pinned_skills: List[Dict[str, Any]] = []
        for cap_id in sorted(pinned_skill_ids):
            rec = external_records.get(cap_id)
            if not isinstance(rec, dict):
                continue
            if str(rec.get("kind") or "").strip().lower() != "skill":
                continue
            pinned_skills.append(
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
            policy_level = _effective_policy_level(
                policy,
                capability_id=cap_id,
                kind=str(rec.get("kind") or ""),
                source_id=str(rec.get("source_id") or ""),
                actor_role=actor_role,
            )
            if cap_id in external_enabled:
                state = str(install_state_by_cap.get(cap_id) or "").strip()
                if state and state != "installed":
                    hidden_capabilities.append(
                        {
                            "capability_id": cap_id,
                            "reason": "install_failed",
                            "state": state,
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
            if qualification == "blocked":
                hidden_capabilities.append(
                    {"capability_id": cap_id, "reason": "policy_blocked", "policy_level": policy_level}
                )
            elif qualification == "manual_review":
                hidden_capabilities.append(
                    {"capability_id": cap_id, "reason": "manual_review_required", "policy_level": policy_level}
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
            entry: Dict[str, str] = {}
            if kind == "skill":
                entry["mode"] = "skill"
                entry["state"] = str(binding.get("state") or "ready_skill")
                if str(binding.get("last_error") or "").strip():
                    entry["last_error"] = str(binding.get("last_error") or "").strip()
            else:
                entry["mode"] = "mcp"
                entry["state"] = str(binding.get("state") or (install_state or "unknown"))
                if str(binding.get("last_error") or "").strip():
                    entry["last_error"] = str(binding.get("last_error") or "").strip()
                elif install_state and install_state != "installed":
                    entry["last_error"] = "install_not_ready"
            external_binding_states[cap_id] = entry

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
                "enabled_capabilities": enabled_caps,
                "active_skills": active_skills,
                "pinned_skills": pinned_skills,
                "hidden_capabilities": hidden_capabilities,
                "external_binding_states": external_binding_states,
                "precedence_chain": ["session", "actor", "group"],
                "session_bindings": session_bindings,
                "source_states": source_states,
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
        with _STATE_LOCK:
            state_path, state_doc = _load_state_doc()
            removed_bindings = _remove_capability_bindings(
                state_doc,
                group_id=group_id,
                capability_id=capability_id,
            )
            if removed_bindings > 0:
                _save_state_doc(state_path, state_doc)

        removed_installation = False
        removed_runtime_bindings = 0
        with _RUNTIME_LOCK:
            runtime_path, runtime_doc = _load_runtime_doc()
            installs = (
                runtime_doc.get("installations")
                if isinstance(runtime_doc.get("installations"), dict)
                else {}
            )
            if capability_id in installs:
                installs.pop(capability_id, None)
                runtime_doc["installations"] = installs
                removed_installation = True
            removed_runtime_bindings = _remove_runtime_capability_bindings_all_groups(
                runtime_doc,
                capability_id=capability_id,
            )
            if removed_installation or removed_runtime_bindings > 0:
                _save_runtime_doc(runtime_path, runtime_doc)

        refresh_required = bool(removed_bindings > 0 or removed_installation or removed_runtime_bindings > 0)
        _audit(
            "ready",
            state="ready",
            details={
                "removed_bindings": int(removed_bindings),
                "removed_installation": bool(removed_installation),
                "removed_runtime_bindings": int(removed_runtime_bindings),
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


def handle_capability_tool_call(args: Dict[str, Any]) -> DaemonResponse:
    group_id = str(args.get("group_id") or "").strip()
    by = str(args.get("by") or args.get("actor_id") or "").strip()
    actor_id = str(args.get("actor_id") or by).strip()
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
            if mutated:
                _save_state_doc(state_path, state_doc)

        with _RUNTIME_LOCK:
            runtime_path, runtime_doc = _load_runtime_doc()
            installs = (
                runtime_doc.get("installations")
                if isinstance(runtime_doc.get("installations"), dict)
                else {}
            )
            target_capability_id = ""
            target_install: Optional[Dict[str, Any]] = None
            real_tool_name = ""
            for capability_id in enabled_caps:
                if capability_id in BUILTIN_CAPABILITY_PACKS:
                    continue
                install = installs.get(capability_id) if isinstance(installs, dict) else None
                if not isinstance(install, dict):
                    continue
                if str(install.get("state") or "") != "installed":
                    continue
                tools = install.get("tools") if isinstance(install.get("tools"), list) else []
                for tool in tools:
                    if not isinstance(tool, dict):
                        continue
                    name = str(tool.get("name") or "").strip()
                    if name != tool_name:
                        continue
                    target_capability_id = capability_id
                    target_install = install
                    real_tool_name = str(tool.get("real_tool_name") or "").strip()
                    break
                if target_install is not None:
                    break

            if target_install is None or not real_tool_name:
                return _error("capability_tool_not_found", f"tool not found or not enabled: {tool_name}")

        try:
            result = _invoke_installed_external_tool(
                target_install,
                real_tool_name=real_tool_name,
                arguments=arguments,
            )
        except Exception as e:
            with _RUNTIME_LOCK:
                # Update last error for observability; do not change enabled scope on transient failures.
                path, doc = _load_runtime_doc()
                installs2 = doc.get("installations") if isinstance(doc.get("installations"), dict) else {}
                install2 = installs2.get(target_capability_id) if isinstance(installs2, dict) else None
                if isinstance(install2, dict):
                    install2["last_error"] = str(e)
                    install2["updated_at"] = utc_now_iso()
                    installs2[target_capability_id] = install2
                    doc["installations"] = installs2
                    _set_runtime_actor_binding(
                        doc,
                        group_id=group_id,
                        actor_id=actor_id,
                        capability_id=target_capability_id,
                        state="tool_call_failed",
                        last_error=str(e),
                    )
                    _save_runtime_doc(path, doc)
            return _error("capability_tool_call_failed", str(e))

        with _RUNTIME_LOCK:
            path, doc = _load_runtime_doc()
            _set_runtime_actor_binding(
                doc,
                group_id=group_id,
                actor_id=actor_id,
                capability_id=target_capability_id,
                state="ready",
                last_error="",
            )
            _save_runtime_doc(path, doc)

        return DaemonResponse(
            ok=True,
            result={
                "tool_name": tool_name,
                "capability_id": target_capability_id,
                "result": result,
            },
        )
    except LookupError as e:
        return _error("group_not_found", str(e))
    except Exception as e:
        return _error("capability_tool_call_failed", str(e))


def try_handle_capability_op(op: str, args: Dict[str, Any]) -> Optional[DaemonResponse]:
    if op == "capability_search":
        return handle_capability_search(args)
    if op == "capability_enable":
        return handle_capability_enable(args)
    if op == "capability_state":
        return handle_capability_state(args)
    if op == "capability_uninstall":
        return handle_capability_uninstall(args)
    if op == "capability_tool_call":
        return handle_capability_tool_call(args)
    return None
