"""Source URI import orchestration for capability imports."""

from __future__ import annotations

import hashlib
from pathlib import Path
from typing import Any, Callable, Dict, List
from urllib.parse import urlparse

from ....contracts.v1 import DaemonError, DaemonResponse
from ....util.time import utc_now_iso
from ._common import _error
from ._github_skills import _discover_github_skill_repository_records
from ._install import _sanitize_skill_id_token
from ._remote import _extract_skill_capsule, _extract_skill_dependencies, _split_frontmatter, _validate_agentskill_frontmatter


def _source_record_version(text: str) -> str:
    return hashlib.sha256(str(text or "").encode("utf-8")).hexdigest()


def _build_skill_record_from_markdown(
    *,
    markdown: str,
    source_id: str,
    source_uri: str,
    namespace: str,
    dir_name: str,
) -> Dict[str, Any]:
    frontmatter, body = _split_frontmatter(markdown)
    name = _sanitize_skill_id_token(str(frontmatter.get("name") or dir_name), default=dir_name or namespace)
    validation_dir = "" if source_id == "url_import" else (dir_name or name)
    errors = _validate_agentskill_frontmatter(frontmatter, dir_name=validation_dir)
    description = str(frontmatter.get("description") or "").strip()
    tags_raw = frontmatter.get("tags")
    tags = [str(x).strip() for x in tags_raw if str(x).strip()] if isinstance(tags_raw, list) else []
    now_iso = utc_now_iso()
    return {
        "capability_id": f"skill:{namespace}:{name}",
        "kind": "skill",
        "name": name,
        "description_short": description or f"{namespace} skill {name}",
        "tags": ["skill", "external", namespace, *tags],
        "source_id": source_id,
        "source_tier": "tier2",
        "source_uri": source_uri,
        "source_record_id": source_uri,
        "source_record_version": _source_record_version(markdown),
        "updated_at_source": now_iso,
        "last_synced_at": now_iso,
        "sync_state": "imported",
        "install_mode": "builtin",
        "install_spec": {},
        "requirements": {},
        "trust_tier": "tier2",
        "qualification_status": "blocked" if errors else "qualified",
        "qualification_reasons": errors,
        "health_status": "local" if source_id == "local_import" else "remote",
        "enable_supported": not errors,
        "capsule_text": _extract_skill_capsule(frontmatter, body),
        "requires_capabilities": _extract_skill_dependencies(frontmatter),
    }


def _url_skill_dir_name(source_uri: str) -> str:
    parsed = urlparse(str(source_uri or "").strip())
    parts = [part for part in parsed.path.strip("/").split("/") if part]
    if parts and parts[-1].lower() == "skill.md" and len(parts) >= 2:
        return _sanitize_skill_id_token(parts[-2], default="url-skill")
    if parts:
        return _sanitize_skill_id_token(Path(parts[-1]).stem, default="url-skill")
    return "url-skill"


def _read_local_skill_markdown(target: str) -> tuple[Path, str]:
    value = str(target or "").strip()
    if value.startswith("file://"):
        value = urlparse(value).path
    path = Path(value).expanduser()
    if path.is_dir():
        path = path / "SKILL.md"
    if not path.is_file():
        raise ValueError("local path must be a SKILL.md file or directory containing SKILL.md")
    return path, path.read_text(encoding="utf-8")


def _source_uri_for_import_expansion(args: Dict[str, Any]) -> str:
    if bool(args.get("_skip_source_uri_expansion")):
        return ""
    raw_record = args.get("record")
    record_source_uri = (
        str(raw_record.get("source_uri") or "").strip()
        if isinstance(raw_record, dict)
        else ""
    )
    source_uri = str(args.get("source_uri") or args.get("url") or record_source_uri).strip()
    if not source_uri:
        return ""
    if not isinstance(raw_record, dict) or not raw_record:
        return source_uri
    if (
        str(raw_record.get("kind") or "").strip().lower() == "skill"
        and str(raw_record.get("capability_id") or "").strip().startswith("skill:github:")
    ):
        return source_uri
    return ""


def _handle_capability_source_uri_import(
    args: Dict[str, Any],
    *,
    action_id: str,
    group_id: str,
    actor_id: str,
    source_uri: str,
    dry_run: bool,
    enable_after_import: bool,
    scope: str,
    import_record: Callable[[Dict[str, Any]], DaemonResponse],
) -> DaemonResponse:
    records = _discover_github_skill_repository_records(source_uri)
    if not records:
        return _error(
            "capability_import_invalid",
            "source_uri did not resolve to any GitHub SKILL.md records",
            details={"action_id": action_id, "source_uri": source_uri},
        )

    imported: List[Dict[str, Any]] = []
    active_count = 0
    refresh_required = False
    source_id = str(args.get("source_id") or "github_import").strip() or "github_import"
    for record in records:
        record = dict(record)
        record["source_id"] = source_id
        child_args = dict(args)
        child_args["record"] = record
        child_args["_skip_source_uri_expansion"] = True
        child_args.pop("source_uri", None)
        child_args.pop("url", None)
        child_resp = import_record(child_args)
        child_result = child_resp.result if child_resp.ok and isinstance(child_resp.result, dict) else {}
        child_item = {
            "capability_id": str(record.get("capability_id") or ""),
            "ok": bool(child_resp.ok),
            "state": str(child_result.get("state") or ""),
            "import_action": str(child_result.get("import_action") or ""),
            "active_after_import": bool(child_result.get("active_after_import")),
            "record": child_result.get("record") if isinstance(child_result.get("record"), dict) else record,
        }
        if not child_resp.ok and child_resp.error:
            child_item["error"] = {
                "code": child_resp.error.code,
                "message": child_resp.error.message,
                "details": child_resp.error.details,
            }
        if child_item["active_after_import"]:
            active_count += 1
        refresh_required = refresh_required or bool(child_result.get("refresh_required"))
        imported.append(child_item)

    failed = [item for item in imported if not bool(item.get("ok"))]
    result = {
        "action_id": action_id,
        "group_id": group_id,
        "actor_id": actor_id,
        "kind": "skill_repository",
        "source_uri": source_uri,
        "dry_run": bool(dry_run),
        "imported": True,
        "scope": scope,
        "enable_after_import": bool(enable_after_import),
        "capability_count": len(imported),
        "active_count": active_count,
        "refresh_required": bool(refresh_required),
        "state": "blocked" if failed else ("activation_pending" if enable_after_import else "imported"),
        "imported_capabilities": imported,
    }
    if failed:
        failed_count = len(failed)
        return DaemonResponse(
            ok=False,
            result=result,
            error=DaemonError(
                code="capability_import_failed",
                message=f"{failed_count} of {len(imported)} GitHub skill imports failed",
                details={
                    "action_id": action_id,
                    "source_uri": source_uri,
                    "failed_count": failed_count,
                    "capability_count": len(imported),
                    "failed_capability_ids": [
                        str(item.get("capability_id") or "")
                        for item in failed
                        if isinstance(item, dict) and str(item.get("capability_id") or "").strip()
                    ],
                },
            ),
        )
    return DaemonResponse(ok=True, result=result)
