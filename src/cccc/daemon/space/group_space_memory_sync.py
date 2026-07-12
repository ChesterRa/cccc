from __future__ import annotations

import hashlib
import os
import re
from pathlib import Path
from typing import Any, Dict, List, Optional, Tuple

from ...contracts.v1 import SpaceMemorySyncSummary
from ...kernel.group import load_group
from ...kernel.memory_reme.layout import resolve_memory_layout
from ...util.fs import atomic_write_json, atomic_write_text, read_json
from ...util.time import utc_now_iso
from .group_space_provider import SpaceProviderError, provider_add_file_source, provider_delete_source
from .group_space_store import enqueue_space_job, get_space_binding, list_space_bindings

_SYNC_FILENAME = "notebooklm_sync.json"
_STAGING_DIRNAME = ".notebooklm-sync"
_DATE_RE = re.compile(r"^(\d{4}-\d{2}-\d{2})__.+\.md$")
_ENTRY_RE = re.compile(r"(?m)^##\s+")
_DEFAULT_MAX_WORDS = 500_000
_MAX_PARTS = 64
_EMPTY_SKIP_STATE = "skipped_empty"
_BLOCKING_ERROR_CODES = {
    "space_binding_missing",
    "space_provider_auth_invalid",
    "space_provider_disabled",
    "space_memory_source_oversize",
    "space_memory_quota_exceeded",
    "space_memory_replace_cleanup_failed",
}


def _max_words_per_source() -> int:
    raw = str(os.environ.get("CCCC_SPACE_MEMORY_SOURCE_MAX_WORDS") or "").strip()
    try:
        value = int(raw) if raw else _DEFAULT_MAX_WORDS
    except Exception:
        value = _DEFAULT_MAX_WORDS
    return max(10, min(value, 2_000_000))


def _sha256(text: str) -> str:
    return hashlib.sha256(text.encode("utf-8")).hexdigest()


def _word_count(text: str) -> int:
    return len(re.findall(r"\S+", text or ""))


def _extract_date(path: Path) -> str:
    m = _DATE_RE.match(path.name)
    return str(m.group(1) if m else "")


def _manifest_path(group_id: str) -> Path:
    layout = resolve_memory_layout(group_id, ensure_files=True)
    return layout.memory_root / _SYNC_FILENAME


def _staging_root(group_id: str) -> Path:
    layout = resolve_memory_layout(group_id, ensure_files=True)
    root = layout.memory_root / _STAGING_DIRNAME
    root.mkdir(parents=True, exist_ok=True)
    return root


def _source_id(payload: Dict[str, Any]) -> str:
    if not isinstance(payload, dict):
        return ""
    return str(payload.get("source_id") or payload.get("id") or "").strip()


def _new_manifest(group_id: str, *, remote_space_id: str = "") -> Dict[str, Any]:
    layout = resolve_memory_layout(group_id, ensure_files=True)
    return {
        "v": 1,
        "provider": "notebooklm",
        "lane": "memory",
        "group_id": group_id,
        "group_label": layout.group_label,
        "remote_space_id": str(remote_space_id or ""),
        "manifest_path": str(layout.memory_root / _SYNC_FILENAME),
        "last_scan_at": None,
        "last_success_at": None,
        "files": {},
    }


def _normalize_file_entry(date: str, value: Any) -> Optional[Dict[str, Any]]:
    if not isinstance(value, dict):
        return None
    out = {
        "date": str(date or ""),
        "file_path": str(value.get("file_path") or ""),
        "relative_path": str(value.get("relative_path") or ""),
        "content_hash": str(value.get("content_hash") or ""),
        "entry_count": int(value.get("entry_count") or 0),
        "word_count": int(value.get("word_count") or 0),
        "source_strategy": str(value.get("source_strategy") or "single"),
        "source_ids": [str(x) for x in (value.get("source_ids") or []) if str(x).strip()],
        "part_count": int(value.get("part_count") or 0),
        "state": str(value.get("state") or "pending"),
        "attempt": int(value.get("attempt") or 0),
        "job_id": str(value.get("job_id") or ""),
        "next_retry_at": str(value.get("next_retry_at") or "") or None,
        "last_error": value.get("last_error") if isinstance(value.get("last_error"), dict) else None,
        "synced_at": str(value.get("synced_at") or "") or None,
        "updated_at": str(value.get("updated_at") or "") or None,
    }
    return out


def _load_manifest(group_id: str, *, remote_space_id: str = "") -> Tuple[Path, Dict[str, Any]]:
    path = _manifest_path(group_id)
    raw = read_json(path)
    base = _new_manifest(group_id, remote_space_id=remote_space_id)
    requested_remote_id = str(remote_space_id or "").strip()
    if not requested_remote_id:
        return path, base
    if not isinstance(raw, dict):
        return path, base
    stored_remote_id = str(raw.get("remote_space_id") or "").strip()
    if stored_remote_id and stored_remote_id != requested_remote_id:
        return path, base
    doc = dict(base)
    doc["remote_space_id"] = str(stored_remote_id or requested_remote_id)
    doc["last_scan_at"] = str(raw.get("last_scan_at") or "") or None
    doc["last_success_at"] = str(raw.get("last_success_at") or "") or None
    files_raw = raw.get("files") if isinstance(raw.get("files"), dict) else {}
    files: Dict[str, Dict[str, Any]] = {}
    for date, value in files_raw.items():
        date_key = str(date or "")[:10]
        if not date_key:
            continue
        item = _normalize_file_entry(date_key, value)
        if item is not None:
            files[date_key] = item
    doc["files"] = files
    return path, doc


def _save_manifest(path: Path, doc: Dict[str, Any]) -> Dict[str, Any]:
    path.parent.mkdir(parents=True, exist_ok=True)
    doc["updated_at"] = utc_now_iso()
    atomic_write_json(path, doc, indent=2)
    return doc


def _update_manifest_entry(
    group_id: str,
    *,
    date: str,
    remote_space_id: str = "",
    updates: Dict[str, Any],
) -> Dict[str, Any]:
    path, doc = _load_manifest(group_id, remote_space_id=remote_space_id)
    files = doc.get("files") if isinstance(doc.get("files"), dict) else {}
    current = files.get(date) if isinstance(files.get(date), dict) else {"date": date}
    merged = {**current, **dict(updates)}
    merged["date"] = date
    merged["updated_at"] = utc_now_iso()
    files[date] = merged
    doc["files"] = files
    if remote_space_id:
        doc["remote_space_id"] = str(remote_space_id)
    _save_manifest(path, doc)
    return merged


def _clear_manifest_success(group_id: str, *, remote_space_id: str = "") -> Dict[str, Any]:
    path, doc = _load_manifest(group_id, remote_space_id=remote_space_id)
    doc["last_success_at"] = utc_now_iso()
    _save_manifest(path, doc)
    return doc


def _manifest_coverage(files: Any) -> Dict[str, Any]:
    rows = files if isinstance(files, dict) else {}
    counts = {"pending": 0, "running": 0, "failed": 0, "blocked": 0}
    eligible_daily_files = 0
    synced_daily_files = 0
    empty_daily_skipped = 0
    last_eligible_daily_date = ""
    last_synced_daily_date = ""
    for date, item in rows.items():
        if not isinstance(item, dict):
            continue
        date_key = str(date or "")[:10]
        state = str(item.get("state") or "").strip()
        if state in counts:
            counts[state] += 1
        entry_count = int(item.get("entry_count") or 0)
        if state == _EMPTY_SKIP_STATE:
            empty_daily_skipped += 1
        if entry_count > 0:
            eligible_daily_files += 1
            if date_key and date_key > last_eligible_daily_date:
                last_eligible_daily_date = date_key
        if state == "succeeded" and entry_count > 0:
            synced_daily_files += 1
            if date_key and date_key > last_synced_daily_date:
                last_synced_daily_date = date_key
    return {
        "pending_files": int(counts["pending"]),
        "running_files": int(counts["running"]),
        "failed_files": int(counts["failed"]),
        "blocked_files": int(counts["blocked"]),
        "eligible_daily_files": int(eligible_daily_files),
        "synced_daily_files": int(synced_daily_files),
        "empty_daily_skipped": int(empty_daily_skipped),
        "last_eligible_daily_date": (last_eligible_daily_date or None),
        "last_synced_daily_date": (last_synced_daily_date or None),
    }


def read_memory_notebooklm_sync_state(group_id: str, *, remote_space_id: str = "") -> Dict[str, Any]:
    path, doc = _load_manifest(group_id, remote_space_id=remote_space_id)
    doc["manifest_path"] = str(path)
    doc.update(_manifest_coverage(doc.get("files")))
    return doc


def summarize_memory_notebooklm_sync(group_id: str, *, remote_space_id: str = "") -> Dict[str, Any]:
    path, doc = _load_manifest(group_id, remote_space_id=remote_space_id)
    coverage = _manifest_coverage(doc.get("files"))
    return SpaceMemorySyncSummary(
        lane="memory",
        manifest_path=str(path),
        last_scan_at=str(doc.get("last_scan_at") or "") or None,
        last_success_at=str(doc.get("last_success_at") or "") or None,
        pending_files=int(coverage.get("pending_files") or 0),
        running_files=int(coverage.get("running_files") or 0),
        failed_files=int(coverage.get("failed_files") or 0),
        blocked_files=int(coverage.get("blocked_files") or 0),
        eligible_daily_files=int(coverage.get("eligible_daily_files") or 0),
        synced_daily_files=int(coverage.get("synced_daily_files") or 0),
        empty_daily_skipped=int(coverage.get("empty_daily_skipped") or 0),
        last_eligible_daily_date=str(coverage.get("last_eligible_daily_date") or "") or None,
        last_synced_daily_date=str(coverage.get("last_synced_daily_date") or "") or None,
    ).model_dump(exclude_none=True)


def _split_daily_parts(*, text: str, date: str, group_label: str) -> Tuple[List[str], str, int]:
    max_words = _max_words_per_source()
    total_words = _word_count(text)
    if total_words <= max_words:
        return [text], "single", total_words

    matches = list(_ENTRY_RE.finditer(text))
    if not matches:
        raise ValueError("daily file exceeds NotebookLM source limit and has no stable entry boundaries")
    preamble = text[: matches[0].start()]
    entries: List[str] = []
    for idx, match in enumerate(matches):
        start = match.start()
        end = matches[idx + 1].start() if idx + 1 < len(matches) else len(text)
        entries.append(text[start:end].rstrip() + "\n")

    parts: List[str] = []
    current_entries: List[str] = []
    for entry in entries:
        candidate_entries = current_entries + [entry]
        candidate_text = preamble.rstrip() + "\n\n" + "\n".join(candidate_entries)
        if _word_count(candidate_text) <= max_words:
            current_entries = candidate_entries
            continue
        if not current_entries:
            raise ValueError("single memory entry exceeds NotebookLM source limit")
        parts.append(preamble.rstrip() + "\n\n" + "\n".join(current_entries).rstrip() + "\n")
        current_entries = [entry]
        if _word_count((preamble.rstrip() + "\n\n" + entry).rstrip() + "\n") > max_words:
            raise ValueError("single memory entry exceeds NotebookLM source limit")
        if len(parts) >= _MAX_PARTS:
            raise ValueError("memory daily split would exceed max parts")
    if current_entries:
        parts.append(preamble.rstrip() + "\n\n" + "\n".join(current_entries).rstrip() + "\n")
    if len(parts) > _MAX_PARTS:
        raise ValueError("memory daily split would exceed max parts")
    return parts, "split", total_words


def _stage_part_files(group_id: str, *, date: str, group_label: str, parts: List[str]) -> List[str]:
    root = _staging_root(group_id)
    out: List[str] = []
    total = len(parts)
    for idx, part in enumerate(parts, start=1):
        name = f"{date}__{group_label}__part{idx:02d}of{total:02d}.md"
        path = root / name
        atomic_write_text(path, part, encoding="utf-8")
        out.append(str(path))
    return out


def _plan_daily_upload(group_id: str, *, file_path: str, date: str, group_label: str) -> Dict[str, Any]:
    path = Path(file_path)
    text = path.read_text(encoding="utf-8", errors="replace")
    matches = list(_ENTRY_RE.finditer(text))
    entry_count = len(matches)
    parts, strategy, total_words = _split_daily_parts(text=text, date=date, group_label=group_label)
    if strategy == "single":
        staged_paths = [str(path)]
    else:
        staged_paths = _stage_part_files(group_id, date=date, group_label=group_label, parts=parts)
    return {
        "content_hash": _sha256(text),
        "entry_count": entry_count,
        "word_count": total_words,
        "source_strategy": strategy,
        "part_count": len(parts),
        "upload_paths": staged_paths,
    }


def _memory_daily_idempotency_key(*, provider: str, group_label: str, date: str, content_hash: str) -> str:
    return f"{provider}:memory:{group_label}:{date}:{content_hash}"


def sync_memory_daily_files(
    group_id: str,
    *,
    provider: str = "notebooklm",
    force: bool = False,
    by: str = "user",
) -> Dict[str, Any]:
    binding = get_space_binding(group_id, provider=provider, lane="memory") or {}
    if str(binding.get("status") or "") != "bound":
        return {"ok": False, "code": "space_binding_missing", "message": "memory lane is not bound"}
    remote_space_id = str(binding.get("remote_space_id") or "").strip()
    if not remote_space_id:
        return {"ok": False, "code": "space_binding_missing", "message": "memory lane has no remote_space_id"}

    layout = resolve_memory_layout(group_id, ensure_files=True)
    manifest_path, manifest = _load_manifest(group_id, remote_space_id=remote_space_id)
    manifest["remote_space_id"] = remote_space_id
    manifest["last_scan_at"] = utc_now_iso()
    files = manifest.get("files") if isinstance(manifest.get("files"), dict) else {}
    today = utc_now_iso()[:10]
    scanned = 0
    queued = 0
    skipped = 0
    blocked = 0
    empty_daily_skipped = 0
    for daily_path in sorted(layout.daily_dir.glob("*.md")):
        date = _extract_date(daily_path)
        if not date or date >= today:
            continue
        scanned += 1
        file_text = daily_path.read_text(encoding="utf-8", errors="replace")
        content_hash = _sha256(file_text)
        current = files.get(date) if isinstance(files.get(date), dict) else {}
        current_source_ids = list(current.get("source_ids") or []) if isinstance(current, dict) else []
        same_hash = str(current.get("content_hash") or "") == content_hash
        state = str(current.get("state") or "")
        if not force and same_hash and state in {"pending", "running", "succeeded", "blocked", "failed", _EMPTY_SKIP_STATE}:
            if state == _EMPTY_SKIP_STATE:
                empty_daily_skipped += 1
            else:
                skipped += 1
            continue
        try:
            plan = _plan_daily_upload(
                group_id,
                file_path=str(daily_path),
                date=date,
                group_label=layout.group_label,
            )
        except Exception as exc:
            blocked += 1
            files[date] = {
                **(current if isinstance(current, dict) else {}),
                "date": date,
                "file_path": str(daily_path),
                "relative_path": f"daily/{daily_path.name}",
                "content_hash": content_hash,
                "source_ids": current_source_ids,
                "part_count": int(current.get("part_count") or 0) if isinstance(current, dict) else 0,
                "state": "blocked",
                "attempt": int(current.get("attempt") or 0) if isinstance(current, dict) else 0,
                "job_id": "",
                "last_error": {
                    "code": "space_memory_source_oversize",
                    "message": str(exc),
                    "retryable": False,
                },
                "updated_at": utc_now_iso(),
            }
            continue
        if int(plan.get("entry_count") or 0) <= 0:
            empty_daily_skipped += 1
            files[date] = {
                **(current if isinstance(current, dict) else {}),
                "date": date,
                "file_path": str(daily_path),
                "relative_path": f"daily/{daily_path.name}",
                "content_hash": str(plan.get("content_hash") or content_hash),
                "entry_count": 0,
                "word_count": int(plan.get("word_count") or 0),
                "source_strategy": str(plan.get("source_strategy") or "single"),
                "part_count": 0,
                "source_ids": current_source_ids,
                "state": _EMPTY_SKIP_STATE,
                "attempt": int(current.get("attempt") or 0) if isinstance(current, dict) else 0,
                "job_id": "",
                "next_retry_at": None,
                "last_error": None,
                "updated_at": utc_now_iso(),
            }
            continue
        payload = {
            "date": date,
            "group_label": layout.group_label,
            "file_path": str(daily_path),
            "relative_path": f"daily/{daily_path.name}",
            "content_hash": str(plan.get("content_hash") or content_hash),
            "entry_count": int(plan.get("entry_count") or 0),
            "word_count": int(plan.get("word_count") or 0),
            "source_strategy": str(plan.get("source_strategy") or "single"),
            "part_count": int(plan.get("part_count") or 1),
            "force": bool(force),
            "by": str(by or "user"),
        }
        job, dedup = enqueue_space_job(
            group_id=group_id,
            provider=provider,
            lane="memory",
            remote_space_id=remote_space_id,
            kind="memory_daily_sync",
            payload=payload,
            idempotency_key=_memory_daily_idempotency_key(
                provider=provider,
                group_label=layout.group_label,
                date=date,
                content_hash=str(payload.get("content_hash") or ""),
            ),
            max_attempts=4,
        )
        files[date] = {
            **(current if isinstance(current, dict) else {}),
            "date": date,
            "file_path": str(daily_path),
            "relative_path": f"daily/{daily_path.name}",
            "content_hash": str(payload.get("content_hash") or ""),
            "entry_count": int(payload.get("entry_count") or 0),
            "word_count": int(payload.get("word_count") or 0),
            "source_strategy": str(payload.get("source_strategy") or "single"),
            "part_count": int(payload.get("part_count") or 1),
            "source_ids": current_source_ids if same_hash and isinstance(current, dict) else current_source_ids,
            "state": str(job.get("state") or "pending"),
            "attempt": int(job.get("attempt") or 0),
            "job_id": str(job.get("job_id") or ""),
            "next_retry_at": str(job.get("next_run_at") or "") or None,
            "last_error": job.get("last_error") if isinstance(job.get("last_error"), dict) else None,
            "updated_at": utc_now_iso(),
        }
        if not dedup:
            queued += 1
        else:
            skipped += 1
    manifest["files"] = files
    _save_manifest(manifest_path, manifest)
    return {
        "ok": True,
        "provider": provider,
        "lane": "memory",
        "group_id": group_id,
        "remote_space_id": remote_space_id,
        "manifest_path": str(manifest_path),
        "scanned": int(scanned),
        "queued": int(queued),
        "skipped": int(skipped),
        "blocked": int(blocked),
        "empty_daily_skipped": int(empty_daily_skipped),
        "summary": summarize_memory_notebooklm_sync(group_id, remote_space_id=remote_space_id),
    }


def mark_memory_sync_job_running(job_doc: Dict[str, Any]) -> None:
    payload = job_doc.get("payload") if isinstance(job_doc.get("payload"), dict) else {}
    group_id = str(job_doc.get("group_id") or "").strip()
    date = str(payload.get("date") or "")[:10]
    if not group_id or not date:
        return
    _update_manifest_entry(
        group_id,
        date=date,
        remote_space_id=str(job_doc.get("remote_space_id") or ""),
        updates={
            "state": "running",
            "attempt": int(job_doc.get("attempt") or 0),
            "job_id": str(job_doc.get("job_id") or ""),
            "next_retry_at": None,
            "last_error": None,
        },
    )


def mark_memory_sync_job_retry(job_doc: Dict[str, Any], *, code: str, message: str, next_run_at: str) -> None:
    payload = job_doc.get("payload") if isinstance(job_doc.get("payload"), dict) else {}
    group_id = str(job_doc.get("group_id") or "").strip()
    date = str(payload.get("date") or "")[:10]
    if not group_id or not date:
        return
    _update_manifest_entry(
        group_id,
        date=date,
        remote_space_id=str(job_doc.get("remote_space_id") or ""),
        updates={
            "state": "pending",
            "attempt": int(job_doc.get("attempt") or 0),
            "job_id": str(job_doc.get("job_id") or ""),
            "next_retry_at": str(next_run_at or "") or None,
            "last_error": {"code": str(code or ""), "message": str(message or ""), "retryable": True},
        },
    )


def mark_memory_sync_job_failed(job_doc: Dict[str, Any], *, code: str, message: str) -> None:
    payload = job_doc.get("payload") if isinstance(job_doc.get("payload"), dict) else {}
    group_id = str(job_doc.get("group_id") or "").strip()
    date = str(payload.get("date") or "")[:10]
    if not group_id or not date:
        return
    state = "blocked" if str(code or "") in _BLOCKING_ERROR_CODES else "failed"
    _update_manifest_entry(
        group_id,
        date=date,
        remote_space_id=str(job_doc.get("remote_space_id") or ""),
        updates={
            "state": state,
            "attempt": int(job_doc.get("attempt") or 0),
            "job_id": str(job_doc.get("job_id") or ""),
            "next_retry_at": None,
            "last_error": {"code": str(code or ""), "message": str(message or ""), "retryable": False},
        },
    )


def _rollback_sources(provider: str, remote_space_id: str, source_ids: List[str]) -> None:
    for source_id in source_ids:
        try:
            provider_delete_source(provider, remote_space_id=remote_space_id, source_id=source_id)
        except Exception:
            pass


def execute_memory_daily_sync_job(job_doc: Dict[str, Any]) -> Dict[str, Any]:
    payload = job_doc.get("payload") if isinstance(job_doc.get("payload"), dict) else {}
    group_id = str(job_doc.get("group_id") or "").strip()
    provider = str(job_doc.get("provider") or "notebooklm").strip() or "notebooklm"
    remote_space_id = str(job_doc.get("remote_space_id") or "").strip()
    date = str(payload.get("date") or "")[:10]
    file_path = str(payload.get("file_path") or "").strip()
    expected_hash = str(payload.get("content_hash") or "").strip()
    if not group_id or not date or not file_path or not remote_space_id:
        raise SpaceProviderError("space_job_invalid", "memory_daily_sync payload is incomplete", transient=False)
    binding = get_space_binding(group_id, provider=provider, lane="memory") or {}
    if str(binding.get("status") or "") != "bound" or str(binding.get("remote_space_id") or "").strip() != remote_space_id:
        raise SpaceProviderError("space_binding_missing", "memory lane binding is missing or changed", transient=False)
    source_file = Path(file_path)
    if not source_file.exists() or not source_file.is_file():
        raise SpaceProviderError("space_job_invalid", f"daily file missing: {file_path}", transient=False)

    layout = resolve_memory_layout(group_id, ensure_files=True)
    current_text = source_file.read_text(encoding="utf-8", errors="replace")
    current_hash = _sha256(current_text)
    if expected_hash and expected_hash != current_hash:
        raise SpaceProviderError("space_memory_source_changed", "daily file changed after enqueue; rescan required", transient=False)

    plan = _plan_daily_upload(group_id, file_path=file_path, date=date, group_label=layout.group_label)
    if int(plan.get("entry_count") or 0) <= 0:
        raise SpaceProviderError("space_job_invalid", "header-only daily file is not sync-eligible", transient=False)
    manifest = read_memory_notebooklm_sync_state(group_id, remote_space_id=remote_space_id)
    files = manifest.get("files") if isinstance(manifest.get("files"), dict) else {}
    current = files.get(date) if isinstance(files.get(date), dict) else {}
    previous_source_ids = [str(x) for x in (current.get("source_ids") or []) if str(x).strip()]

    uploaded_source_ids: List[str] = []
    for upload_path in list(plan.get("upload_paths") or []):
        try:
            added = provider_add_file_source(provider, remote_space_id=remote_space_id, file_path=str(upload_path))
        except Exception:
            _rollback_sources(provider, remote_space_id, uploaded_source_ids)
            raise
        new_sid = _source_id(added)
        if not new_sid:
            _rollback_sources(provider, remote_space_id, uploaded_source_ids)
            raise SpaceProviderError(
                "space_provider_upstream_error",
                f"provider add_file returned empty source_id for {upload_path}",
                transient=False,
                degrade_provider=False,
            )
        uploaded_source_ids.append(new_sid)

    deleted_old: List[str] = []
    try:
        for old_sid in previous_source_ids:
            if old_sid in uploaded_source_ids:
                continue
            provider_delete_source(provider, remote_space_id=remote_space_id, source_id=old_sid)
            deleted_old.append(old_sid)
    except Exception as exc:
        _rollback_sources(provider, remote_space_id, uploaded_source_ids)
        raise SpaceProviderError(
            "space_memory_replace_cleanup_failed",
            f"failed to replace old memory sources for {date}: {exc}",
            transient=False,
            degrade_provider=False,
        ) from exc

    synced_at = utc_now_iso()
    _update_manifest_entry(
        group_id,
        date=date,
        remote_space_id=remote_space_id,
        updates={
            "file_path": file_path,
            "relative_path": str(payload.get("relative_path") or f"daily/{Path(file_path).name}"),
            "content_hash": current_hash,
            "entry_count": int(plan.get("entry_count") or 0),
            "word_count": int(plan.get("word_count") or 0),
            "source_strategy": str(plan.get("source_strategy") or "single"),
            "source_ids": uploaded_source_ids,
            "part_count": int(plan.get("part_count") or len(uploaded_source_ids) or 1),
            "state": "succeeded",
            "attempt": int(job_doc.get("attempt") or 0),
            "job_id": str(job_doc.get("job_id") or ""),
            "next_retry_at": None,
            "last_error": None,
            "synced_at": synced_at,
        },
    )
    _clear_manifest_success(group_id, remote_space_id=remote_space_id)
    return {
        "provider": provider,
        "lane": "memory",
        "group_id": group_id,
        "date": date,
        "content_hash": current_hash,
        "source_ids": uploaded_source_ids,
        "deleted_source_ids": deleted_old,
        "source_strategy": str(plan.get("source_strategy") or "single"),
        "part_count": int(plan.get("part_count") or len(uploaded_source_ids) or 1),
        "synced_at": synced_at,
    }


def process_due_memory_space_syncs(*, provider: str = "notebooklm", limit: int = 20) -> Dict[str, Any]:
    max_items = max(1, min(int(limit or 20), 200))
    bindings = list_space_bindings(provider, lane="memory")
    processed = 0
    queued = 0
    blocked = 0
    missing_groups = 0
    for item in bindings[:max_items]:
        if not isinstance(item, dict):
            continue
        if str(item.get("status") or "") != "bound":
            continue
        group_id = str(item.get("group_id") or "").strip()
        if not group_id:
            continue
        if load_group(group_id) is None:
            missing_groups += 1
            continue
        processed += 1
        result = sync_memory_daily_files(group_id, provider=provider, force=False)
        if bool(result.get("ok")):
            queued += int(result.get("queued") or 0)
        else:
            blocked += 1
    return {
        "processed": processed,
        "queued": queued,
        "blocked": blocked,
        "missing_groups": missing_groups,
    }
