"""SkillsMP-specific capability helpers."""

from __future__ import annotations

import hashlib
from typing import Any, Dict
from urllib.parse import urlparse

from ._install import _sanitize_skill_id_token


_SKILLSMP_GENERIC_NAME_TOKENS = {"md", "markdown", "skill", "skills"}
_SKILLSMP_GENERIC_NAMES = {"", "skill", "skills", "md", "skill-md", "skills-md"}


def _normalize_skillsmp_source_uri(uri: str) -> str:
    raw = str(uri or "").strip().rstrip(").,")
    if not raw:
        return ""
    parsed = urlparse(raw)
    host = str(parsed.netloc or "").lower()
    parts = [part for part in str(parsed.path or "").split("/") if part]
    if host in {"skillsmp.com", "www.skillsmp.com"} and len(parts) >= 2 and parts[0] == "skills":
        slug = _sanitize_skill_id_token(parts[1], default="")
        if slug:
            return f"https://skillsmp.com/skills/{slug}"
    return raw


def _skillsmp_record_key(row: Dict[str, Any]) -> str:
    return _normalize_skillsmp_source_uri(str(row.get("source_uri") or row.get("source_record_id") or "").strip())


def _skillsmp_capability_id_for_record_key(record_key: str) -> str:
    key = _normalize_skillsmp_source_uri(record_key)
    if not key:
        return ""
    slug = key.rstrip("/").split("/")[-1]
    slug_token = _sanitize_skill_id_token(slug, default="")
    if not slug_token:
        return ""
    rec_hash = hashlib.sha1(key.encode("utf-8")).hexdigest()[:8]
    return f"skill:skillsmp:{slug_token}-{rec_hash}"


def _skillsmp_record_is_canonical(row: Dict[str, Any]) -> bool:
    key = _skillsmp_record_key(row)
    expected = _skillsmp_capability_id_for_record_key(key)
    return bool(expected and str(row.get("capability_id") or "").strip() == expected)


def _skillsmp_skill_name_from_slug(slug: str) -> str:
    slug_token = _sanitize_skill_id_token(slug, default="skill")
    parts = [p for p in slug_token.split("-") if p]
    while parts and parts[-1] in _SKILLSMP_GENERIC_NAME_TOKENS:
        parts.pop()
    if not parts:
        return "skill"

    last_skills_index = -1
    for index, part in enumerate(parts):
        if part == "skills":
            last_skills_index = index
    if last_skills_index >= 0 and last_skills_index + 1 < len(parts):
        parts = parts[last_skills_index + 1 :]

    if len(parts) > 1 and parts[0] == "superpowers":
        parts = parts[1:]
    return _sanitize_skill_id_token("-".join(parts), default="skill")


def _skillsmp_record_display_name(row: Dict[str, Any]) -> str:
    name = _sanitize_skill_id_token(str(row.get("name") or ""), default="")
    if name and name not in _SKILLSMP_GENERIC_NAMES:
        return name
    source_uri = _skillsmp_record_key(row)
    slug = source_uri.rstrip("/").split("/")[-1] if source_uri else ""
    return _skillsmp_skill_name_from_slug(slug)
