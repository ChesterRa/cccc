"""Remote fetch, download, and sync functions for capability registry sources."""

from __future__ import annotations

import ast
import hashlib
import json
import os
import re
import sys
import time
import warnings
from typing import Any, Dict, List, Optional, Tuple
from urllib.error import HTTPError, URLError
from urllib.parse import quote, urlencode
from urllib.request import Request, urlopen

import yaml

from ....util.time import utc_now_iso

from ._common import (
    _SOURCE_IDS,
    _MCP_REGISTRY_BASE,
    _MCP_REGISTRY_PAGE_LIMIT,
    _GITHUB_API_BASE,
    _RAW_GITHUB_BASE,
    _OPENCLAW_SKILLS_TREE_API,
    _OPENCLAW_SKILLS_BLOB_BASE,
    _CLAWSKILLS_DATA_URL_DEFAULT,
    _SKILL_NAME_RE,
    _CLAWSKILLS_ENTRY_RE,
    _REMOTE_SOURCE_CACHE_LOCK,
    _OPENCLAW_TREE_CACHE,
    _QUAL_QUALIFIED,
    _QUAL_BLOCKED,
    _QUAL_UNAVAILABLE,
    _env_int,
    _env_bool,
)
from ._documents import _source_state_template
from ._install import (
    _sanitize_skill_id_token,
    _github_headers,
    _catalog_staleness_seconds,
    _supported_external_install_record,
    _normalize_registry_argument_entries,
    _normalize_registry_env_names,
    _extract_required_env_from_runtime_arguments,
    _normalize_registry_type_token,
)


def _pkg():
    """Get parent package module for mock-compatible function lookups."""
    return sys.modules[__name__.rsplit(".", 1)[0]]


# ---------------------------------------------------------------------------
# Shared text utilities (used by remote search + later by _search.py)
# ---------------------------------------------------------------------------

def _tokenize_search_text(text: str) -> List[str]:
    raw = str(text or "").strip().lower()
    if not raw:
        return []
    return [tok for tok in re.findall(r"[a-z0-9]{3,}", raw) if tok]


def _query_tokens_match(tokens: List[str], text: str) -> bool:
    if not tokens:
        return True
    hay = str(text or "").lower()
    return all(tok in hay for tok in tokens)


# ---------------------------------------------------------------------------
# MCP Registry source sync + search
# ---------------------------------------------------------------------------

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
            data = _pkg()._http_get_json_obj(url, timeout=12.0)

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
    data = _pkg()._http_get_json_obj(url, timeout=8.0)
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


# ---------------------------------------------------------------------------
# SkillsMP source search
# ---------------------------------------------------------------------------

_SKILLSMP_SKILL_URL_RE = re.compile(r"https?://skillsmp\.com/skills/[^\s)\]]+")
_SKILLSMP_DATE_RE = re.compile(r"\s+\d{4}-\d{2}-\d{2}\s*$")


def _js_literal_to_text(raw: Any) -> str:
    s = str(raw or "").strip()
    if not s:
        return ""
    try:
        with warnings.catch_warnings():
            warnings.simplefilter("ignore", SyntaxWarning)
            parsed = ast.literal_eval(s)
        return str(parsed or "")
    except Exception:
        return s.strip("'\"")


def _skillsmp_proxy_search_url(query: str) -> str:
    base = str(os.environ.get("CCCC_CAPABILITY_SKILLSMP_PROXY_BASE") or "").strip()
    if not base:
        base = "https://r.jina.ai/http://skillsmp.com/search"
    token = quote(str(query or "").strip())
    if "{query}" in base:
        return base.replace("{query}", token)
    sep = "&" if "?" in base else "?"
    return f"{base}{sep}q={token}"


def _parse_skillsmp_proxy_search_markdown(markdown: str, *, limit: int) -> List[Dict[str, Any]]:
    text = str(markdown or "")
    if not text.strip():
        return []
    rows: List[Dict[str, Any]] = []
    seen_uri: set[str] = set()
    now_iso = utc_now_iso()
    for m in _SKILLSMP_SKILL_URL_RE.finditer(text):
        source_uri = str(m.group(0) or "").strip().rstrip(").,")
        if not source_uri:
            continue
        if source_uri in seen_uri:
            continue
        seen_uri.add(source_uri)
        source_record_id = source_uri
        rec_hash = hashlib.sha1(source_record_id.encode("utf-8")).hexdigest()[:8]
        slug = source_uri.rstrip("/").split("/")[-1]
        slug_token = _sanitize_skill_id_token(slug, default="skill")
        cap_id = f"skill:skillsmp:{slug_token}-{rec_hash}"
        context = text[max(0, m.start() - 1200) : m.start()]
        one_line = " ".join((context.splitlines()[-1] if context.splitlines() else context).split())
        export_match = re.search(r"###\s*export\s+([A-Za-z0-9._-]+)", context)
        skill_name = ""
        if export_match:
            skill_name = _sanitize_skill_id_token(str(export_match.group(1) or ""), default="skill")
        if not skill_name:
            slug_parts = [p for p in slug_token.split("-") if p]
            if len(slug_parts) >= 2:
                skill_name = _sanitize_skill_id_token("-".join(slug_parts[-2:]), default="skill")
            else:
                skill_name = _sanitize_skill_id_token(slug_token, default="skill")
        repo_match = re.search(r'from\s+"([^"]+)"', context)
        description = one_line
        if repo_match:
            description = context[repo_match.end() :].strip()
        description = re.sub(r"^.*###\s*export\s+[A-Za-z0-9._-]+\s*", "", description).strip()
        description = _SKILLSMP_DATE_RE.sub("", description).strip()
        description = " ".join(description.split())
        if not description:
            description = f"SkillsMP skill candidate ({skill_name})"
        rows.append(
            {
                "capability_id": cap_id,
                "kind": "skill",
                "name": skill_name,
                "description_short": description[:600],
                "tags": ["skill", "external", "skillsmp", "remote_search"],
                "source_id": "skillsmp_remote",
                "source_tier": "tier2",
                "source_uri": source_uri,
                "source_record_id": source_record_id,
                "source_record_version": "",
                "updated_at_source": now_iso,
                "last_synced_at": now_iso,
                "sync_state": "remote",
                "install_mode": "builtin",
                "install_spec": {},
                "requirements": {},
                "license": "",
                "trust_tier": "tier2",
                "qualification_status": _QUAL_QUALIFIED,
                "qualification_reasons": [],
                "health_status": "remote",
                "enable_supported": True,
                "capsule_text": f"Skill: {skill_name}\nSummary: {description[:1000]}\nSource: {source_uri}",
                "requires_capabilities": [],
            }
        )
        if len(rows) >= max(1, int(limit or 20)):
            break
    return rows


def _skillsmp_api_search_url(query: str, *, page: int, limit: int) -> str:
    base = str(os.environ.get("CCCC_CAPABILITY_SKILLSMP_API_BASE") or "").strip()
    if not base:
        base = "https://skillsmp.com/api/v1/skills/search"
    params = {
        "q": str(query or "").strip(),
        "page": str(max(1, int(page or 1))),
        "limit": str(max(1, min(int(limit or 20), 100))),
        "sortBy": "stars",
    }
    sep = "&" if "?" in base else "?"
    return f"{base}{sep}{urlencode(params)}"


def _parse_skillsmp_api_payload(data: Dict[str, Any], *, limit: int) -> List[Dict[str, Any]]:
    now_iso = utc_now_iso()
    rows: List[Dict[str, Any]] = []
    candidates: List[Any] = []
    for key in ("items", "skills", "results", "data"):
        value = data.get(key)
        if isinstance(value, list):
            candidates = value
            break
        if isinstance(value, dict):
            for nested_key in ("items", "skills", "results"):
                nested = value.get(nested_key)
                if isinstance(nested, list):
                    candidates = nested
                    break
            if candidates:
                break

    for item in candidates:
        if not isinstance(item, dict):
            continue
        slug = _sanitize_skill_id_token(str(item.get("slug") or item.get("id") or ""), default="")
        name = _sanitize_skill_id_token(str(item.get("name") or item.get("displayName") or slug), default="skill")
        if not slug:
            continue
        summary = str(
            item.get("summary")
            or item.get("description")
            or item.get("desc")
            or ""
        ).strip()
        if not summary:
            summary = f"SkillsMP skill candidate ({name})"
        source_uri = f"https://skillsmp.com/skills/{slug}"
        source_record_id = source_uri
        rec_hash = hashlib.sha1(source_record_id.encode("utf-8")).hexdigest()[:8]
        rows.append(
            {
                "capability_id": f"skill:skillsmp:{slug}-{rec_hash}",
                "kind": "skill",
                "name": name,
                "description_short": summary[:600],
                "tags": ["skill", "external", "skillsmp", "remote_search"],
                "source_id": "skillsmp_remote",
                "source_tier": "tier2",
                "source_uri": source_uri,
                "source_record_id": source_record_id,
                "source_record_version": str(item.get("version") or ""),
                "updated_at_source": now_iso,
                "last_synced_at": now_iso,
                "sync_state": "remote",
                "install_mode": "builtin",
                "install_spec": {},
                "requirements": {},
                "license": "",
                "trust_tier": "tier2",
                "qualification_status": _QUAL_QUALIFIED,
                "qualification_reasons": [],
                "health_status": "remote",
                "enable_supported": True,
                "capsule_text": f"Skill: {name}\nSummary: {summary[:1000]}\nSource: {source_uri}",
                "requires_capabilities": [],
            }
        )
        if len(rows) >= max(1, int(limit or 20)):
            break
    return rows


def _remote_search_skillsmp_records(*, query: str, limit: int) -> List[Dict[str, Any]]:
    if not _env_bool("CCCC_CAPABILITY_SOURCE_SKILLSMP_REMOTE_ENABLED", True):
        return []
    q = str(query or "").strip()
    if not q:
        return []
    timeout_s = float(max(3, min(_env_int("CCCC_CAPABILITY_SKILLSMP_REMOTE_TIMEOUT_SECONDS", 10), 25)))
    api_key = str(os.environ.get("CCCC_CAPABILITY_SKILLSMP_API_KEY") or "").strip()
    api_error = ""
    if api_key:
        try:
            api_url = _skillsmp_api_search_url(q, page=1, limit=limit)
            api_data = _pkg()._http_get_json_obj(
                api_url,
                headers={"Authorization": f"Bearer {api_key}", "User-Agent": "cccc-capability-sync/1.0"},
                timeout=timeout_s,
            )
            api_rows = _parse_skillsmp_api_payload(api_data, limit=limit)
            if api_rows:
                return api_rows
            api_error = "skillsmp_api_empty"
        except HTTPError as e:
            if int(getattr(e, "code", 0) or 0) == 401:
                api_error = "skillsmp_api_auth_failed"
            else:
                api_error = f"skillsmp_api_http_{int(getattr(e, 'code', 0) or 0)}"
        except Exception as e:
            api_error = f"skillsmp_api_failed:{e}"

    url = _skillsmp_proxy_search_url(q)
    text = _pkg()._http_get_text(url, headers={"User-Agent": "cccc-capability-sync/1.0"}, timeout=timeout_s)
    rows = _parse_skillsmp_proxy_search_markdown(text, limit=limit)
    if rows:
        return rows
    lowered = text.lower()
    if "missing_api_key" in lowered or "authorization header is required" in lowered:
        raise RuntimeError("skillsmp_api_key_required")
    if "cloudflare" in lowered and "blocked" in lowered:
        raise RuntimeError("skillsmp_blocked_by_cloudflare")
    if "loading skills" in lowered:
        if api_error:
            raise RuntimeError(f"{api_error};skillsmp_loading_only")
        raise RuntimeError("skillsmp_loading_only")
    if api_error:
        raise RuntimeError(f"{api_error};skillsmp_empty_or_unparsable")
    raise RuntimeError("skillsmp_empty_or_unparsable")


# ---------------------------------------------------------------------------
# ClawHub source search
# ---------------------------------------------------------------------------

def _clawhub_api_url(*, query: str, limit: int, cursor: str = "") -> str:
    base = str(os.environ.get("CCCC_CAPABILITY_CLAWHUB_API_BASE") or "").strip()
    if not base:
        base = "https://clawhub.ai/api/v1/skills"
    params: Dict[str, str] = {
        "limit": str(max(1, min(int(limit or 20), 100))),
    }
    q = str(query or "").strip()
    if q:
        params["q"] = q
    cur = str(cursor or "").strip()
    if cur:
        params["cursor"] = cur
    sep = "&" if "?" in base else "?"
    return f"{base}{sep}{urlencode(params)}"


def _clawhub_item_to_record(item: Dict[str, Any], *, now_iso: str) -> Optional[Dict[str, Any]]:
    slug = _sanitize_skill_id_token(str(item.get("slug") or ""), default="")
    if not slug:
        return None
    source_uri = f"https://clawhub.ai/skills/{slug}"
    source_record_id = source_uri
    rec_hash = hashlib.sha1(source_record_id.encode("utf-8")).hexdigest()[:8]
    name = _sanitize_skill_id_token(str(item.get("displayName") or slug), default="skill")
    description = str(item.get("summary") or "").strip()
    if not description:
        description = f"ClawHub skill candidate ({name})"
    tags: List[str] = ["skill", "external", "clawhub", "remote_search"]
    stats = item.get("stats") if isinstance(item.get("stats"), dict) else {}
    if int(stats.get("stars") or 0) > 0:
        tags.append("starred")
    version = ""
    latest_version = item.get("latestVersion")
    if isinstance(latest_version, dict):
        version = str(latest_version.get("version") or "").strip()
    else:
        tags_obj = item.get("tags") if isinstance(item.get("tags"), dict) else {}
        version = str(tags_obj.get("latest") or "").strip()

    return {
        "capability_id": f"skill:clawhub:{slug}-{rec_hash}",
        "kind": "skill",
        "name": name,
        "description_short": description[:600],
        "tags": tags,
        "source_id": "clawhub_remote",
        "source_tier": "tier2",
        "source_uri": source_uri,
        "source_record_id": source_record_id,
        "source_record_version": version,
        "updated_at_source": now_iso,
        "last_synced_at": now_iso,
        "sync_state": "remote",
        "install_mode": "builtin",
        "install_spec": {},
        "requirements": {},
        "license": "",
        "trust_tier": "tier2",
        "qualification_status": _QUAL_QUALIFIED,
        "qualification_reasons": [],
        "health_status": "remote",
        "enable_supported": True,
        "capsule_text": f"Skill: {name}\nSummary: {description[:1000]}\nSource: {source_uri}",
        "requires_capabilities": [],
    }


def _remote_search_clawhub_records(*, query: str, limit: int) -> List[Dict[str, Any]]:
    if not _env_bool("CCCC_CAPABILITY_SOURCE_CLAWHUB_REMOTE_ENABLED", True):
        return []
    q = str(query or "").strip()
    if not q:
        return []
    query_tokens = _tokenize_search_text(q)
    timeout_s = float(max(3, min(_env_int("CCCC_CAPABILITY_CLAWHUB_REMOTE_TIMEOUT_SECONDS", 10), 25)))
    max_pages = max(1, min(_env_int("CCCC_CAPABILITY_CLAWHUB_REMOTE_MAX_PAGES", 5), 15))
    page_size = max(1, min(_env_int("CCCC_CAPABILITY_CLAWHUB_REMOTE_PAGE_SIZE", 50), 100))
    requested = max(1, min(int(limit or 20), 100))
    rows: List[Dict[str, Any]] = []
    seen_caps: set[str] = set()
    now_iso = utc_now_iso()
    cursor = ""
    page = 0
    saw_payload = False
    while len(rows) < requested and page < max_pages:
        url = _clawhub_api_url(query=q, limit=page_size, cursor=cursor)
        data = _pkg()._http_get_json_obj(url, timeout=timeout_s)
        items = data.get("items")
        if not isinstance(items, list):
            break
        saw_payload = True
        page += 1
        for item in items:
            if not isinstance(item, dict):
                continue
            rec = _clawhub_item_to_record(item, now_iso=now_iso)
            if not isinstance(rec, dict):
                continue
            search_text = " ".join(
                [
                    str(rec.get("name") or ""),
                    str(rec.get("description_short") or ""),
                    str(item.get("slug") or ""),
                    str(item.get("displayName") or ""),
                ]
            )
            if not _query_tokens_match(query_tokens, search_text):
                continue
            cap_id = str(rec.get("capability_id") or "")
            if not cap_id or cap_id in seen_caps:
                continue
            seen_caps.add(cap_id)
            rows.append(rec)
            if len(rows) >= requested:
                break
        cursor = str(data.get("nextCursor") or "").strip()
        if not cursor:
            break
    if rows:
        return rows[:requested]
    if saw_payload:
        return []
    raise RuntimeError("clawhub_empty_or_unparsable")


# ---------------------------------------------------------------------------
# OpenClaw skills source search
# ---------------------------------------------------------------------------

def _openclaw_tree_paths() -> List[str]:
    ttl_seconds = max(60, min(_env_int("CCCC_CAPABILITY_OPENCLAW_TREE_CACHE_TTL_SECONDS", 3600), 86_400))
    now = time.time()
    with _REMOTE_SOURCE_CACHE_LOCK:
        cache_paths = _OPENCLAW_TREE_CACHE.get("paths")
        fetched_at = float(_OPENCLAW_TREE_CACHE.get("fetched_at") or 0.0)
        if isinstance(cache_paths, list) and cache_paths and (now - fetched_at) < ttl_seconds:
            return [str(x) for x in cache_paths if str(x).strip()]

    data = _pkg()._http_get_json_obj(_OPENCLAW_SKILLS_TREE_API, headers=_github_headers(), timeout=12.0)
    tree = data.get("tree")
    if not isinstance(tree, list):
        return []
    paths: List[str] = []
    for item in tree:
        if not isinstance(item, dict):
            continue
        if str(item.get("type") or "").strip().lower() != "blob":
            continue
        path = str(item.get("path") or "").strip().replace("\\", "/")
        if not path.lower().endswith("skill.md"):
            continue
        paths.append(path)
    paths = sorted(set(paths))
    with _REMOTE_SOURCE_CACHE_LOCK:
        _OPENCLAW_TREE_CACHE["fetched_at"] = now
        _OPENCLAW_TREE_CACHE["paths"] = list(paths)
    return paths


def _openclaw_frontmatter_for_path(path: str) -> Tuple[Dict[str, Any], str]:
    safe_path = "/".join(part for part in str(path or "").split("/") if part and part not in {".", ".."})
    if not safe_path:
        return {}, ""
    url = f"{_OPENCLAW_SKILLS_BLOB_BASE}/{quote(safe_path, safe='/')}"
    text = _pkg()._http_get_text(url, headers=_github_headers(), timeout=8.0)
    try:
        frontmatter, body = _split_frontmatter(text)
        return frontmatter, body
    except Exception:
        return {}, text


def _remote_search_openclaw_skill_records(*, query: str, limit: int) -> List[Dict[str, Any]]:
    if not _env_bool("CCCC_CAPABILITY_SOURCE_OPENCLAW_SKILLS_REMOTE_ENABLED", True):
        return []
    q = str(query or "").strip()
    if not q:
        return []
    query_tokens = _tokenize_search_text(q)
    requested = max(1, min(int(limit or 20), 100))
    paths = _openclaw_tree_paths()
    if not paths:
        return []

    candidates: List[str] = []
    for path in paths:
        hay = path.lower()
        if _query_tokens_match(query_tokens, hay):
            candidates.append(path)
    if not candidates:
        return []

    fetch_frontmatter_max = max(
        0,
        min(_env_int("CCCC_CAPABILITY_OPENCLAW_FRONTMATTER_FETCH_MAX", 10), 40),
    )
    now_iso = utc_now_iso()
    out: List[Dict[str, Any]] = []
    seen: set[str] = set()
    for idx, path in enumerate(candidates[: max(requested * 3, requested)]):
        source_record_id = f"openclaw/skills:{path}"
        rec_hash = hashlib.sha1(source_record_id.encode("utf-8")).hexdigest()[:8]
        tokens = [p for p in path.split("/") if p]
        slug_hint = tokens[-2] if len(tokens) >= 2 else tokens[-1]
        skill_name = _sanitize_skill_id_token(slug_hint, default="skill")
        cap_id = f"skill:openclaw:{skill_name}-{rec_hash}"
        if cap_id in seen:
            continue
        seen.add(cap_id)
        source_uri = f"https://github.com/openclaw/skills/blob/main/{quote(path, safe='/')}"
        frontmatter: Dict[str, Any] = {}
        body = ""
        if idx < fetch_frontmatter_max:
            try:
                frontmatter, body = _openclaw_frontmatter_for_path(path)
            except Exception:
                frontmatter = {}
                body = ""
        if frontmatter:
            maybe_name = _sanitize_skill_id_token(str(frontmatter.get("name") or ""), default=skill_name)
            if maybe_name:
                skill_name = maybe_name
            description = str(frontmatter.get("description") or "").strip()
            requires_capabilities = _extract_skill_dependencies(frontmatter)
            capsule_text = _extract_skill_capsule(frontmatter, body)
            license_text = str(frontmatter.get("license") or "").strip()
        else:
            description = ""
            requires_capabilities = []
            capsule_text = ""
            license_text = ""

        if not description:
            description = f"OpenClaw skill candidate ({skill_name}) from {path}"
        if not capsule_text:
            capsule_text = f"Skill: {skill_name}\nSummary: {description}\nSource: {source_uri}"
        out.append(
            {
                "capability_id": cap_id,
                "kind": "skill",
                "name": skill_name,
                "description_short": description[:600],
                "tags": ["skill", "external", "openclaw", "remote_search"],
                "source_id": "openclaw_skills_remote",
                "source_tier": "tier2",
                "source_uri": source_uri,
                "source_record_id": source_record_id,
                "source_record_version": "",
                "updated_at_source": now_iso,
                "last_synced_at": now_iso,
                "sync_state": "remote",
                "install_mode": "builtin",
                "install_spec": {},
                "requirements": {},
                "license": license_text,
                "trust_tier": "tier2",
                "qualification_status": _QUAL_QUALIFIED,
                "qualification_reasons": [],
                "health_status": "remote",
                "enable_supported": True,
                "capsule_text": capsule_text[:2400],
                "requires_capabilities": requires_capabilities[:32],
            }
        )
        if len(out) >= requested:
            break
    return out


# ---------------------------------------------------------------------------
# ClawSkills source search
# ---------------------------------------------------------------------------

def _parse_clawskills_data_js(*, script: str, query: str, limit: int) -> List[Dict[str, Any]]:
    text = str(script or "")
    if not text.strip():
        return []
    start = text.find("var SKILLS_DATA")
    if start < 0:
        return []
    list_start = text.find("[", start)
    list_end = text.rfind("]")
    if list_start < 0 or list_end < 0 or list_end <= list_start:
        return []
    payload = text[list_start : list_end + 1]
    tokens = _tokenize_search_text(query)
    requested = max(1, min(int(limit or 20), 100))
    rows: List[Dict[str, Any]] = []
    seen: set[str] = set()
    now_iso = utc_now_iso()
    for obj_match in _CLAWSKILLS_ENTRY_RE.finditer(payload):
        obj_text = str(obj_match.group(0) or "").strip()
        if not obj_text:
            continue
        fields: Dict[str, str] = {}
        for key_match in re.finditer(r"([a-zA-Z_][a-zA-Z0-9_]*)\s*:\s*('(?:\\\\'|[^'])*'|\"(?:\\\\\"|[^\"])*\")", obj_text):
            key = str(key_match.group(1) or "").strip().lower()
            value = _js_literal_to_text(key_match.group(2))
            if key:
                fields[key] = value
        slug = _sanitize_skill_id_token(fields.get("slug"), default="")
        if not slug:
            continue
        name = _sanitize_skill_id_token(fields.get("name"), default=slug)
        desc = str(fields.get("desc") or fields.get("description") or "").strip()
        category = str(fields.get("category") or "").strip()
        author = _sanitize_skill_id_token(fields.get("author"), default="")
        hay = " ".join([slug, name, desc, category, author]).lower()
        if not _query_tokens_match(tokens, hay):
            continue
        source_uri = f"https://clawskills.co/#skill-{slug}"
        source_record_id = f"clawskills:{slug}"
        rec_hash = hashlib.sha1(source_record_id.encode("utf-8")).hexdigest()[:8]
        cap_id = f"skill:clawskills:{slug}-{rec_hash}"
        if cap_id in seen:
            continue
        seen.add(cap_id)
        if not desc:
            desc = f"clawskills.co skill candidate ({name})"
        tags = ["skill", "external", "clawskills", "remote_search"]
        if category:
            tags.append(_sanitize_skill_id_token(category, default="category"))
        if author:
            tags.append(f"author:{author}")
        rows.append(
            {
                "capability_id": cap_id,
                "kind": "skill",
                "name": name,
                "description_short": desc[:600],
                "tags": tags,
                "source_id": "clawskills_remote",
                "source_tier": "tier2",
                "source_uri": source_uri,
                "source_record_id": source_record_id,
                "source_record_version": "",
                "updated_at_source": now_iso,
                "last_synced_at": now_iso,
                "sync_state": "remote",
                "install_mode": "builtin",
                "install_spec": {},
                "requirements": {},
                "license": "",
                "trust_tier": "tier2",
                "qualification_status": _QUAL_QUALIFIED,
                "qualification_reasons": [],
                "health_status": "remote",
                "enable_supported": True,
                "capsule_text": f"Skill: {name}\nSummary: {desc[:1000]}\nSource: {source_uri}",
                "requires_capabilities": [],
            }
        )
        if len(rows) >= requested:
            break
    return rows


def _remote_search_clawskills_records(*, query: str, limit: int) -> List[Dict[str, Any]]:
    if not _env_bool("CCCC_CAPABILITY_SOURCE_CLAWSKILLS_REMOTE_ENABLED", True):
        return []
    q = str(query or "").strip()
    if not q:
        return []
    url = str(os.environ.get("CCCC_CAPABILITY_CLAWSKILLS_DATA_URL") or "").strip() or _CLAWSKILLS_DATA_URL_DEFAULT
    timeout_s = float(max(3, min(_env_int("CCCC_CAPABILITY_CLAWSKILLS_REMOTE_TIMEOUT_SECONDS", 12), 30)))
    text = _pkg()._http_get_text(url, headers={"User-Agent": "cccc-capability-sync/1.0"}, timeout=timeout_s)
    rows = _parse_clawskills_data_js(script=text, query=q, limit=limit)
    if rows:
        return rows
    if "var SKILLS_DATA" in text:
        return []
    raise RuntimeError("clawskills_empty_or_unparsable")


# ---------------------------------------------------------------------------
# Unified remote skill search
# ---------------------------------------------------------------------------

def _remote_search_skill_records(*, query: str, limit: int, source_filter: str = "") -> List[Dict[str, Any]]:
    requested = max(1, min(int(limit or 20), 100))
    out: List[Dict[str, Any]] = []
    seen: set[str] = set()
    errors: List[str] = []
    source_hint = str(source_filter or "").strip().lower()

    def _append_rows(rows: List[Dict[str, Any]]) -> None:
        for rec in rows:
            if not isinstance(rec, dict):
                continue
            cap_id = str(rec.get("capability_id") or "").strip()
            if not cap_id or cap_id in seen:
                continue
            seen.add(cap_id)
            out.append(rec)

    _parent = _pkg()
    adapters = [
        (
            "skillsmp",
            "skillsmp_remote",
            _parent._remote_search_skillsmp_records,
            max(1, min(_env_int("CCCC_CAPABILITY_SEARCH_REMOTE_SKILLSMP_LIMIT", requested), 100)),
        ),
        (
            "clawhub",
            "clawhub_remote",
            _parent._remote_search_clawhub_records,
            max(1, min(_env_int("CCCC_CAPABILITY_SEARCH_REMOTE_CLAWHUB_LIMIT", requested), 100)),
        ),
        (
            "openclaw",
            "openclaw_skills_remote",
            _parent._remote_search_openclaw_skill_records,
            max(1, min(_env_int("CCCC_CAPABILITY_SEARCH_REMOTE_OPENCLAW_LIMIT", requested), 100)),
        ),
        (
            "clawskills",
            "clawskills_remote",
            _parent._remote_search_clawskills_records,
            max(1, min(_env_int("CCCC_CAPABILITY_SEARCH_REMOTE_CLAWSKILLS_LIMIT", requested), 100)),
        ),
    ]
    if source_hint in {
        "skillsmp_remote",
        "clawhub_remote",
        "openclaw_skills_remote",
        "clawskills_remote",
    }:
        adapters = [item for item in adapters if str(item[1] or "").strip().lower() == source_hint]
    for source_name, _source_id, fn, source_limit in adapters:
        if len(out) >= requested:
            break
        needed = max(1, requested - len(out))
        try:
            rows = fn(query=query, limit=min(needed, source_limit))
            _append_rows(rows if isinstance(rows, list) else [])
        except Exception as e:
            errors.append(f"{source_name}:{e}")
            continue

    if out:
        return out[:requested]
    if errors:
        raise RuntimeError("; ".join(errors))
    return []


# ---------------------------------------------------------------------------
# MCP Registry record fetch + normalize
# ---------------------------------------------------------------------------

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
        runtime_arguments = _normalize_registry_argument_entries(pkg.get("runtimeArguments"))
        package_arguments = _normalize_registry_argument_entries(pkg.get("packageArguments"))
        required_env = _normalize_registry_env_names(pkg.get("environmentVariables"), required_only=True)
        required_env_from_runtime = _extract_required_env_from_runtime_arguments(pkg.get("runtimeArguments"))
        for env_name in required_env_from_runtime:
            if env_name not in required_env:
                required_env.append(env_name)
        env_names = _normalize_registry_env_names(pkg.get("environmentVariables"), required_only=False)
        raw_registry_type = str(pkg.get("registryType") or "").strip()
        install_spec = {
            "registry_type": _normalize_registry_type_token(raw_registry_type),
            "registry_type_raw": raw_registry_type,
            "identifier": str(pkg.get("identifier") or "").strip(),
            "version": str(pkg.get("version") or server.get("version") or "").strip(),
            "runtime_hint": str(pkg.get("runtimeHint") or "").strip(),
            "transport": str((pkg.get("transport") or {}).get("type") or "").strip()
            if isinstance(pkg.get("transport"), dict)
            else "",
            "runtime_arguments": runtime_arguments,
            "package_arguments": package_arguments,
            "required_env": required_env,
            "env_names": env_names,
        }
    elif remotes:
        install_mode = "remote_only"
        remote = remotes[0] if isinstance(remotes[0], dict) else {}
        install_spec = {
            "transport": str(remote.get("type") or "").strip(),
            "url": str(remote.get("url") or "").strip(),
        }

    supported, _ = _supported_external_install_record(
        {
            "install_mode": install_mode,
            "install_spec": install_spec,
        }
    )
    qualification = _QUAL_UNAVAILABLE
    if status == "deleted":
        qualification = _QUAL_BLOCKED
    elif supported:
        qualification = _QUAL_QUALIFIED
    else:
        qualification = _QUAL_UNAVAILABLE

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
            ["registry_status_deleted"]
            if qualification == _QUAL_BLOCKED
            else (["external_install_supported"] if qualification == _QUAL_QUALIFIED else ["external_install_unavailable"])
        ),
        "health_status": "ok" if status == "active" else status,
        "enable_supported": bool(supported and status == "active"),
    }


# ---------------------------------------------------------------------------
# Skill frontmatter parsing
# ---------------------------------------------------------------------------

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


# ---------------------------------------------------------------------------
# Anthropic skills source sync
# ---------------------------------------------------------------------------

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
        # Use _pkg() for mock-compatible lookups: tests mock these on the
        # parent package namespace (cccc.daemon.ops.capability_ops).
        dirs_data = _pkg()._http_get_json(
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
            with _pkg().urlopen(req, timeout=12.0) as resp:
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
                qualification = _QUAL_BLOCKED
                reasons.extend(errors)
            else:
                qualification = _QUAL_QUALIFIED

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
                "enable_supported": qualification != _QUAL_BLOCKED,
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


def _mark_source_disabled(catalog: Dict[str, Any], source_id: str) -> None:
    sources = catalog.get("sources") if isinstance(catalog.get("sources"), dict) else {}
    state = sources.get(source_id) if isinstance(sources.get(source_id), dict) else _source_state_template("never")
    state["sync_state"] = "disabled"
    state["error"] = "source_disabled_by_policy"
    state["staleness_seconds"] = _catalog_staleness_seconds(str(state.get("last_synced_at") or ""))
    sources[source_id] = state
    catalog["sources"] = sources
