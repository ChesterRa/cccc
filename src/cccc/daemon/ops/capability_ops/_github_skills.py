"""GitHub SKILL.md repository discovery for capability imports."""

from __future__ import annotations

import re
import sys
from typing import Any, Dict, List, Optional, Tuple
from urllib.parse import quote, urlparse

from ....util.time import utc_now_iso

from ._common import _GITHUB_API_BASE, _RAW_GITHUB_BASE, _QUAL_BLOCKED, _QUAL_QUALIFIED
from ._install import _github_headers, _sanitize_skill_id_token
from ._remote import _extract_skill_capsule, _extract_skill_dependencies, _split_frontmatter, _validate_agentskill_frontmatter


_GITHUB_OWNER_REPO_RE = re.compile(r"^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+(?:[/#:@?].*)?$")


def _pkg():
    """Get parent package module for mock-compatible function lookups."""
    return sys.modules[__name__.rsplit(".", 1)[0]]


def _parse_github_owner_repo_ref(source_uri: str) -> Optional[Tuple[str, str, str]]:
    value = str(source_uri or "").strip()
    if not value:
        return None
    if _GITHUB_OWNER_REPO_RE.match(value):
        parts = value.split("?", 1)[0].split("#", 1)[0].split("@", 1)[0].rstrip("/").split("/")
        if len(parts) >= 2:
            return parts[0], parts[1].removesuffix(".git"), "main"
    if value.startswith("git@github.com:"):
        path = value.split(":", 1)[1].removesuffix(".git").strip("/")
        parts = path.split("/")
        if len(parts) >= 2:
            return parts[0], parts[1], "main"
    parsed = urlparse(value)
    if parsed.netloc.lower() != "github.com":
        return None
    parts = [part for part in parsed.path.strip("/").split("/") if part]
    if len(parts) < 2:
        return None
    owner = parts[0]
    repo = parts[1].removesuffix(".git")
    ref = "main"
    if len(parts) >= 4 and parts[2] in {"tree", "blob"}:
        ref = parts[3]
    if parsed.fragment.strip():
        ref = parsed.fragment.strip()
    return owner, repo, ref


def _github_skill_paths_from_tree(data: Dict[str, Any]) -> List[Tuple[str, str]]:
    tree = data.get("tree")
    if not isinstance(tree, list):
        return []
    rows: List[Tuple[str, str]] = []
    for item in tree:
        if not isinstance(item, dict):
            continue
        if str(item.get("type") or "").strip().lower() != "blob":
            continue
        path = str(item.get("path") or "").strip().replace("\\", "/")
        if path == "SKILL.md" or re.match(r"^skills/(?:[^/]+/)+SKILL\.md$", path):
            rows.append((path, str(item.get("sha") or "").strip()))
    return sorted(rows, key=lambda row: row[0])


def _github_skill_record_for_path(
    *,
    owner: str,
    repo: str,
    ref: str,
    path: str,
    blob_sha: str,
    now_iso: str,
) -> Optional[Dict[str, Any]]:
    safe_path = "/".join(part for part in str(path or "").split("/") if part and part not in {".", ".."})
    if not safe_path:
        return None
    raw_url = f"{_RAW_GITHUB_BASE}/{quote(owner)}/{quote(repo)}/{quote(ref)}/{quote(safe_path, safe='/')}"
    text = _pkg()._http_get_text(raw_url, headers=_github_headers(), timeout=12.0)
    frontmatter, body = _split_frontmatter(text)
    parts = safe_path.split("/")
    dir_name = parts[-2] if len(parts) >= 3 and parts[0] == "skills" else repo
    errors = _validate_agentskill_frontmatter(frontmatter, dir_name=dir_name)
    name = _sanitize_skill_id_token(str(frontmatter.get("name") or dir_name), default=dir_name)
    owner_token = _sanitize_skill_id_token(owner, default="github")
    path_tokens = [
        _sanitize_skill_id_token(part, default="")
        for part in (parts[1:-2] if len(parts) >= 4 and parts[0] == "skills" else [])
    ]
    path_tokens = [part for part in path_tokens if part]
    cap_name = _sanitize_skill_id_token("-".join([*path_tokens, name]), default=name)
    description = str(frontmatter.get("description") or "").strip()
    license_text = str(frontmatter.get("license") or "").strip()
    tags_raw = frontmatter.get("tags")
    tags = [str(x).strip() for x in tags_raw if str(x).strip()] if isinstance(tags_raw, list) else []
    source_uri = (
        f"https://github.com/{quote(owner)}/{quote(repo)}/tree/{quote(ref)}/{quote('/'.join(parts[:-1]), safe='/')}"
        if len(parts) > 1
        else f"https://github.com/{quote(owner)}/{quote(repo)}/tree/{quote(ref)}"
    )
    qualification = _QUAL_BLOCKED if errors else _QUAL_QUALIFIED
    return {
        "capability_id": f"skill:github:{owner_token}:{cap_name}",
        "kind": "skill",
        "name": name,
        "description_short": description or f"GitHub skill {name} from {owner}/{repo}",
        "tags": ["skill", "external", "github", owner_token, *tags],
        "source_id": "github_import",
        "source_tier": "tier2",
        "source_uri": source_uri,
        "source_record_id": f"{owner}/{repo}:{safe_path}",
        "source_record_version": blob_sha,
        "updated_at_source": now_iso,
        "last_synced_at": now_iso,
        "sync_state": "remote",
        "install_mode": "builtin",
        "install_spec": {},
        "requirements": {},
        "license": license_text,
        "trust_tier": "tier2",
        "qualification_status": qualification,
        "qualification_reasons": errors,
        "health_status": "remote",
        "enable_supported": qualification != _QUAL_BLOCKED,
        "capsule_text": _extract_skill_capsule(frontmatter, body),
        "requires_capabilities": _extract_skill_dependencies(frontmatter),
    }


def _discover_github_skill_repository_records(source_uri: str) -> List[Dict[str, Any]]:
    parsed = _parse_github_owner_repo_ref(source_uri)
    if parsed is None:
        return []
    owner, repo, ref = parsed
    tree_url = f"{_GITHUB_API_BASE}/repos/{quote(owner)}/{quote(repo)}/git/trees/{quote(ref)}?recursive=1"
    data = _pkg()._http_get_json_obj(tree_url, headers=_github_headers(), timeout=12.0)
    rows = _github_skill_paths_from_tree(data)
    now_iso = utc_now_iso()
    records: List[Dict[str, Any]] = []
    seen: set[str] = set()
    for path, blob_sha in rows:
        record = _github_skill_record_for_path(
            owner=owner,
            repo=repo,
            ref=ref,
            path=path,
            blob_sha=blob_sha,
            now_iso=now_iso,
        )
        if not record:
            continue
        cap_id = str(record.get("capability_id") or "").strip()
        if not cap_id or cap_id in seen:
            continue
        seen.add(cap_id)
        records.append(record)
    return records
