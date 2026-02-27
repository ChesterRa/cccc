from __future__ import annotations

import hashlib
import os
from datetime import datetime, timezone
from pathlib import Path
import uuid
from typing import Any, Dict, Iterable, List, Optional, Tuple
from urllib.parse import urlparse

from ...kernel.actors import find_actor, find_foreman
from ...kernel.group import load_group
from ...kernel.ledger import append_event
from ...util.fs import atomic_write_json, read_json
from .group_space_paths import (
    resolve_space_root_from_group,
    resolve_space_root,
    space_index_path,
    space_state_path,
)
from .group_space_provider import (
    SpaceProviderError,
    provider_add_file_source,
    provider_delete_source,
    provider_download_artifact,
    provider_get_source_fulltext,
    provider_list_artifacts,
    provider_list_sources,
    provider_rename_source,
)
from .group_space_runtime import acquire_space_provider_write
from .group_space_store import get_space_binding, get_space_provider_state, list_space_bindings

_SYNC_INTERNAL_FILES = {".space-index.json", ".space-sync-state.json", ".space-status.json"}
_SYNC_EXCLUDED_TOP_DIRS = {"artifacts", "remote_sources"}
_MARKER_PREFIX = "CCCC::space::"
_PATH_HASH_LEN = 24
_REMOTE_SYNC_DIR = ".sync"
_REMOTE_SOURCES_DIR = "remote-sources"
_REMOTE_ARTIFACTS_MANIFEST = "remote-artifacts.json"
_REMOTE_SOURCE_TEXT_ROOT = "sources"
_REMOTE_SOURCE_PREVIEW_DIR = "source-text"
_LEGACY_REMOTE_SOURCE_SUBDIR = "notebooklm"
_MAX_REMOTE_ERRORS = 50
_FAILED_ITEMS_LIMIT = 20
_LOCAL_SOURCE_STABLE_EXTENSIONS = frozenset(
    {
        ".txt",
        ".md",
        ".markdown",
        ".pdf",
        ".docx",
        ".csv",
        ".tsv",
    }
)
_LOCAL_SOURCE_CONDITIONAL_EXTENSIONS = frozenset(
    {
        ".doc",
        ".ppt",
        ".pptx",
        ".xls",
        ".xlsx",
        ".odt",
        ".ods",
        ".rtf",
        ".png",
        ".jpg",
        ".jpeg",
        ".webp",
        ".gif",
        ".bmp",
        ".tif",
        ".tiff",
        ".heic",
        ".heif",
        ".mp3",
        ".wav",
        ".m4a",
        ".aac",
        ".flac",
        ".ogg",
        ".oga",
        ".mp4",
        ".m4v",
        ".mov",
        ".avi",
        ".mkv",
        ".webm",
    }
)
_LOCAL_SOURCE_ALLOWED_EXTENSIONS = frozenset(_LOCAL_SOURCE_STABLE_EXTENSIONS | _LOCAL_SOURCE_CONDITIONAL_EXTENSIONS)
_NOTEBOOKLM_MAX_FILE_SIZE_BYTES = 200 * 1024 * 1024
_RESERVED_NOTIFY_TARGETS = {
    "",
    "user",
    "all",
    "system",
    "foreman",
    "peers",
    "admin",
    "root",
    "cccc",
    "@all",
    "@foreman",
    "@peers",
    "@user",
}


def _now_iso() -> str:
    return datetime.now(timezone.utc).isoformat().replace("+00:00", "Z")


def _parse_utc_ts(raw: Any) -> Optional[datetime]:
    text = str(raw or "").strip()
    if not text:
        return None
    try:
        if text.endswith("Z"):
            text = text[:-1] + "+00:00"
        dt = datetime.fromisoformat(text)
        if dt.tzinfo is None:
            dt = dt.replace(tzinfo=timezone.utc)
        return dt.astimezone(timezone.utc)
    except Exception:
        return None


def _int_env(name: str, default: int, *, lo: int, hi: int) -> int:
    raw = str(os.environ.get(name) or "").strip()
    if not raw:
        return default
    try:
        value = int(raw)
    except Exception:
        return default
    return max(lo, min(hi, value))


def _reconcile_stale_seconds() -> int:
    return _int_env("CCCC_SPACE_RECONCILE_MAX_STALE_SECONDS", 600, lo=30, hi=86400)


def _sync_run_id() -> str:
    return f"ss_{uuid.uuid4().hex[:12]}"


def _path_hash(rel_path: str) -> str:
    return hashlib.sha256(rel_path.encode("utf-8")).hexdigest()[:_PATH_HASH_LEN]


def _marker_title(path_hash: str, rel_path: str) -> str:
    base = Path(rel_path).name or "file"
    prefix = f"{_MARKER_PREFIX}{path_hash}::"
    max_title = 180
    available = max(8, max_title - len(prefix))
    if len(base) > available:
        base = base[:available]
    return f"{prefix}{base}"


def _safe_file_component(raw: Any, *, fallback: str) -> str:
    text = str(raw or "").strip()
    text = "".join(ch if ch.isalnum() or ch in {"_", "-", "."} else "_" for ch in text)
    text = text.strip("._")
    return text[:96] if text else fallback


def _is_source_descriptor_path(rel_path: str) -> bool:
    return str(rel_path or "").strip().lower().endswith(".source.json")


def _is_uuid_like(raw: Any) -> bool:
    text = str(raw or "").strip()
    if not text:
        return False
    try:
        _ = uuid.UUID(text)
        return True
    except Exception:
        return False


def _source_label_from_title(raw: Any) -> str:
    text = str(raw or "").strip()
    if not text:
        return ""
    base = Path(text).stem or text
    base = base.strip()
    if not base:
        return ""
    if _is_uuid_like(base):
        return ""
    return base


def _source_label_from_url(raw: Any) -> str:
    text = str(raw or "").strip()
    if not text:
        return ""
    try:
        parsed = urlparse(text)
    except Exception:
        return ""
    host = str(parsed.hostname or "").strip().lower()
    path = str(parsed.path or "").strip("/")
    tail = path.split("/")[-1] if path else ""
    if tail:
        tail = Path(tail).stem or tail
    label = "-".join([part for part in [host, tail] if part])
    if not label:
        label = host
    return label


def _source_label_from_content(raw: Any) -> str:
    text = str(raw or "")
    if not text.strip():
        return ""
    for line in text.splitlines():
        s = str(line).strip()
        if not s:
            continue
        if len(s) > 64:
            s = s[:64]
        return s
    return ""


def _source_label(kind: str, *, title: str, url: str, content: str) -> str:
    k = _normalize_source_kind(kind)
    label = _source_label_from_title(title)
    if label:
        return label
    if k in {"web_page", "youtube"}:
        label = _source_label_from_url(url)
        if label:
            return label
    if k in {"pasted_text", "markdown", "pdf", "docx", "csv", "image", "media"}:
        label = _source_label_from_content(content)
        if label:
            return label
    return k or "source"


def _remote_source_file_stem(source_id: str, *, kind: str, title: str, url: str, content: str) -> str:
    sid = _safe_file_component(source_id, fallback="source")
    sid_short = sid[:8] if sid else "source"
    label = _source_label(kind, title=title, url=url, content=content)
    k = _safe_file_component(_normalize_source_kind(kind) or "source", fallback="source")
    l = _safe_file_component(label, fallback=k)
    stem = f"{k}-{l}-{sid_short}"
    if len(stem) > 120:
        stem = stem[:120]
    return stem.strip("._-") or f"{k}-{sid_short}"


def _is_legacy_remote_descriptor_rel_path(rel_path: str, *, source_id: str) -> bool:
    rel = str(rel_path or "").strip()
    if not _is_source_descriptor_path(rel):
        return False
    expected = f"{_REMOTE_SOURCE_TEXT_ROOT}/{_safe_file_component(source_id, fallback='source')}.source.json"
    return rel == expected


def _descriptor_stem(rel_path: str) -> str:
    rel = str(rel_path or "").strip()
    if not _is_source_descriptor_path(rel):
        return ""
    name = Path(rel).name
    suffix = ".source.json"
    if not name.endswith(suffix):
        return ""
    return name[: -len(suffix)].strip()


def _descriptor_name_is_unhelpful(rel_path: str, *, source_id: str) -> bool:
    if _is_legacy_remote_descriptor_rel_path(rel_path, source_id=source_id):
        return True
    stem = _descriptor_stem(rel_path)
    if not stem:
        return True
    if _is_uuid_like(stem):
        return True
    if stem == _safe_file_component(source_id, fallback="source"):
        return True
    return False


def _remote_source_descriptor_rel_path(
    source_id: str,
    *,
    kind: str,
    title: str,
    url: str,
    content: str,
) -> str:
    stem = _remote_source_file_stem(source_id, kind=kind, title=title, url=url, content=content)
    return f"{_REMOTE_SOURCE_TEXT_ROOT}/{stem}.source.json"


def _remote_source_preview_rel_path(
    source_id: str,
    *,
    kind: str,
    title: str = "",
    url: str = "",
    content: str = "",
) -> str:
    ext = _source_content_extension(kind)
    stem = _remote_source_file_stem(source_id, kind=kind, title=title, url=url, content=content)
    return f"{_REMOTE_SYNC_DIR}/{_REMOTE_SOURCE_PREVIEW_DIR}/{stem}{ext}"


def _remote_source_preview_rel_from_descriptor(rel_path: str, *, kind: str) -> str:
    ext = _source_content_extension(kind)
    stem = _descriptor_stem(rel_path) or _safe_file_component(Path(rel_path).stem, fallback="source")
    return f"{_REMOTE_SYNC_DIR}/{_REMOTE_SOURCE_PREVIEW_DIR}/{stem}{ext}"


def _parse_marker_hash(title: Any) -> str:
    text = str(title or "").strip()
    if not text.startswith(_MARKER_PREFIX):
        return ""
    suffix = text[len(_MARKER_PREFIX) :]
    try:
        token, _ = suffix.split("::", 1)
    except ValueError:
        return ""
    token = token.strip().lower()
    if len(token) != _PATH_HASH_LEN:
        return ""
    if any(ch not in "0123456789abcdef" for ch in token):
        return ""
    return token


def _normalize_source_kind(raw: Any) -> str:
    text = str(raw or "").strip().lower()
    if not text:
        return ""
    if "." in text:
        text = text.split(".")[-1].strip()
    text = text.replace("-", "_")
    alias = {
        "googledocs": "google_docs",
        "googleslides": "google_slides",
        "googlespreadsheet": "google_spreadsheet",
        "webpage": "web_page",
        "pastedtext": "pasted_text",
    }
    return alias.get(text, text)


def _source_is_ready(raw: Any) -> bool:
    status = str(raw or "").strip().lower()
    return status in {"2", "ready", "succeeded", "done"}


def _source_content_extension(kind: str) -> str:
    k = _normalize_source_kind(kind)
    if k == "csv":
        return ".csv"
    if k in {"markdown"}:
        return ".md"
    return ".txt"


def _local_source_extension(rel_path: str) -> str:
    return Path(str(rel_path or "")).suffix.lower()


def _local_source_policy(rel_path: str) -> str:
    ext = _local_source_extension(rel_path)
    if ext in _LOCAL_SOURCE_STABLE_EXTENSIONS:
        return "stable"
    if ext in _LOCAL_SOURCE_CONDITIONAL_EXTENSIONS:
        return "conditional"
    return "unsupported"


def _local_source_size_limit_bytes() -> int:
    # NotebookLM source upload limit: up to 200 MB per uploaded file.
    return _int_env(
        "CCCC_SPACE_LOCAL_FILE_MAX_BYTES",
        _NOTEBOOKLM_MAX_FILE_SIZE_BYTES,
        lo=1,
        hi=_NOTEBOOKLM_MAX_FILE_SIZE_BYTES,
    )


def _format_size_mib(size_bytes: int) -> str:
    return f"{(float(max(0, int(size_bytes))) / (1024.0 * 1024.0)):.1f} MiB"


def _local_source_format_error_message(rel_path: str) -> str:
    ext = _local_source_extension(rel_path)
    ext_label = ext or "(no extension)"
    allowed = ", ".join(sorted(_LOCAL_SOURCE_ALLOWED_EXTENSIONS))
    return (
        f"unsupported local source format {ext_label}; "
        f"allowed extensions: {allowed}. "
        "For web/youtube/google docs sources, use kind=resource_ingest."
    )


def _local_source_size_error_message(rel_path: str, *, size_bytes: int, limit_bytes: int) -> str:
    return (
        f"local file exceeds NotebookLM per-file size limit for sync upload "
        f"({_format_size_mib(size_bytes)} > {_format_size_mib(limit_bytes)}): {rel_path}"
    )


def group_space_local_file_policy() -> Dict[str, Any]:
    size_limit = _local_source_size_limit_bytes()
    return {
        "mode": "extension_whitelist",
        "stable_extensions": sorted(_LOCAL_SOURCE_STABLE_EXTENSIONS),
        "conditional_extensions": sorted(_LOCAL_SOURCE_CONDITIONAL_EXTENSIONS),
        "allowed_extensions": sorted(_LOCAL_SOURCE_ALLOWED_EXTENSIONS),
        "unsupported_error_code": "space_source_unsupported_format",
        "max_file_size_bytes": int(size_limit),
        "max_file_size_human": _format_size_mib(size_limit),
        "oversize_error_code": "space_source_file_too_large",
        "notes": [
            "Stable = frequently tested and expected to work for local file upload.",
            "Conditional = may work but can still fail upstream due to provider parsing/limits.",
            "NotebookLM uploaded-file size is capped at 200 MB per source.",
            "Use resource_ingest for web/youtube/google drive style sources.",
        ],
    }


def _render_source_content_text(
    *,
    source_id: str,
    title: str,
    kind: str,
    url: str,
    content: str,
) -> str:
    _ = source_id, title, kind, url
    return content.rstrip() + "\n"


def _normalize_artifact_kind(raw: Any) -> str:
    text = str(raw or "").strip().lower()
    if not text:
        return ""
    if "." in text:
        text = text.split(".")[-1].strip()
    text = text.replace("-", "_")
    alias = {
        "studyguide": "study_guide",
        "study": "study_guide",
        "datatable": "data_table",
        "table": "data_table",
        "slidedeck": "slide_deck",
        "slides": "slide_deck",
        "slide": "slide_deck",
        "deck": "slide_deck",
        "mindmap": "mind_map",
        "overview": "report",
        "summary": "report",
        "briefing": "report",
    }
    return alias.get(text, text)


def _artifact_is_completed(raw: Any) -> bool:
    status = str(raw or "").strip().lower()
    return status in {"completed", "succeeded", "ready", "done"}


def _artifact_extension(kind: str, *, output_format: str = "") -> str:
    k = _normalize_artifact_kind(kind)
    fmt = str(output_format or "").strip().lower()
    if k == "audio":
        return ".mp3"
    if k == "video":
        return ".mp4"
    if k in {"report", "study_guide"}:
        return ".md"
    if k in {"quiz", "flashcards"}:
        if fmt == "json":
            return ".json"
        if fmt == "html":
            return ".html"
        return ".md"
    if k == "infographic":
        return ".png"
    if k == "slide_deck":
        return ".pdf"
    if k == "data_table":
        return ".csv"
    if k == "mind_map":
        return ".json"
    return ".bin"


def _iter_space_files(space_root: Path) -> Iterable[Tuple[Path, str]]:
    try:
        roots = [Path(space_root).resolve()]
    except Exception:
        roots = [space_root]
    if not roots[0].exists():
        return []

    out: List[Tuple[Path, str]] = []
    for current, dirs, files in os.walk(roots[0]):
        current_path = Path(current)
        try:
            rel_current = current_path.relative_to(roots[0])
        except Exception:
            rel_current = Path(".")
        if rel_current == Path("."):
            dirs[:] = sorted(
                [
                    d
                    for d in dirs
                    if (not str(d).startswith(".")) and (str(d) not in _SYNC_EXCLUDED_TOP_DIRS)
                ]
            )
        else:
            dirs[:] = sorted([d for d in dirs if not str(d).startswith(".")])
        for name in sorted(files):
            if name in _SYNC_INTERNAL_FILES:
                continue
            if str(name).startswith("."):
                continue
            abs_path = current_path / name
            if not abs_path.is_file():
                continue
            try:
                rel = abs_path.relative_to(roots[0]).as_posix()
            except Exception:
                continue
            top = rel.split("/", 1)[0]
            if top in _SYNC_EXCLUDED_TOP_DIRS:
                continue
            out.append((abs_path, rel))
    return out


def _file_sha256(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        while True:
            chunk = f.read(1024 * 1024)
            if not chunk:
                break
            h.update(chunk)
    return h.hexdigest()


def _space_fingerprint(space_root: Path) -> Dict[str, Any]:
    digest = hashlib.sha256()
    files = 0
    total_bytes = 0
    latest_mtime_ns = 0
    for abs_path, rel_path in _iter_space_files(space_root):
        try:
            st = abs_path.stat()
        except Exception:
            continue
        size = int(st.st_size)
        mtime_ns = int(getattr(st, "st_mtime_ns", int(st.st_mtime * 1_000_000_000)))
        files += 1
        total_bytes += max(0, size)
        latest_mtime_ns = max(latest_mtime_ns, mtime_ns)
        digest.update(rel_path.encode("utf-8"))
        digest.update(b"\0")
        digest.update(str(size).encode("utf-8"))
        digest.update(b"\0")
        digest.update(str(mtime_ns).encode("utf-8"))
        digest.update(b"\n")
    return {
        "files": files,
        "total_bytes": total_bytes,
        "latest_mtime_ns": latest_mtime_ns,
        "digest": digest.hexdigest(),
    }


def _normalize_index_doc(raw: Any) -> Dict[str, Any]:
    if not isinstance(raw, dict):
        raw = {}
    entries_raw = raw.get("entries")
    entries = entries_raw if isinstance(entries_raw, dict) else {}
    norm_entries: Dict[str, Dict[str, Any]] = {}
    for rel_path, item in entries.items():
        rel = str(rel_path or "").strip()
        if not rel:
            continue
        if not isinstance(item, dict):
            continue
        norm_entries[rel] = {
            "rel_path": rel,
            "path_hash": str(item.get("path_hash") or _path_hash(rel)),
            "sha256": str(item.get("sha256") or ""),
            "size": int(item.get("size") or 0),
            "mtime_ns": int(item.get("mtime_ns") or 0),
            "source_id": str(item.get("source_id") or ""),
            "remote_title": str(item.get("remote_title") or ""),
            "last_synced_at": str(item.get("last_synced_at") or ""),
        }
    return {
        "v": 1,
        "group_id": str(raw.get("group_id") or ""),
        "provider": str(raw.get("provider") or "notebooklm"),
        "remote_space_id": str(raw.get("remote_space_id") or ""),
        "updated_at": str(raw.get("updated_at") or ""),
        "entries": norm_entries,
    }


def _normalize_state_doc(raw: Any) -> Dict[str, Any]:
    if not isinstance(raw, dict):
        raw = {}
    failed_raw = raw.get("failed_items")
    failed_items: List[Dict[str, str]] = []
    if isinstance(failed_raw, list):
        for item in failed_raw:
            if not isinstance(item, dict):
                continue
            rel_path = str(item.get("rel_path") or "").strip()
            code = str(item.get("code") or "space_sync_error").strip() or "space_sync_error"
            message = str(item.get("message") or "").strip()
            failed_items.append(
                {
                    "rel_path": rel_path,
                    "code": code,
                    "message": message,
                }
            )
            if len(failed_items) >= _FAILED_ITEMS_LIMIT:
                break
    errors_raw = raw.get("errors")
    errors: List[Dict[str, str]] = []
    if isinstance(errors_raw, list):
        for item in errors_raw:
            if not isinstance(item, dict):
                continue
            errors.append(
                {
                    "code": str(item.get("code") or "space_sync_error").strip() or "space_sync_error",
                    "message": str(item.get("message") or "").strip(),
                    "rel_path": str(item.get("rel_path") or "").strip(),
                }
            )
            if len(errors) >= _MAX_REMOTE_ERRORS:
                break
    state_value = str(raw.get("state") or "").strip().lower()
    if state_value not in {"ok", "error"}:
        state_value = "error" if int(raw.get("unsynced_count") or 0) > 0 else "ok"
    return {
        "v": 1,
        "group_id": str(raw.get("group_id") or ""),
        "provider": str(raw.get("provider") or "notebooklm"),
        "remote_space_id": str(raw.get("remote_space_id") or ""),
        "run_id": str(raw.get("run_id") or ""),
        "last_run_at": str(raw.get("last_run_at") or ""),
        "state": state_value,
        "converged": bool(raw.get("converged")),
        "unsynced_count": int(raw.get("unsynced_count") or 0),
        "failed_count": int(raw.get("failed_count") or 0),
        "failed_items": failed_items,
        "uploaded": int(raw.get("uploaded") or 0),
        "updated": int(raw.get("updated") or 0),
        "deleted": int(raw.get("deleted") or 0),
        "reused": int(raw.get("reused") or 0),
        "remote_sources": int(raw.get("remote_sources") or 0),
        "materialized_sources": int(raw.get("materialized_sources") or 0),
        "remote_artifacts": int(raw.get("remote_artifacts") or 0),
        "downloaded_artifacts": int(raw.get("downloaded_artifacts") or 0),
        "pruned_artifacts": int(raw.get("pruned_artifacts") or 0),
        "last_error": str(raw.get("last_error") or ""),
        "failure_signature": str(raw.get("failure_signature") or ""),
        "last_fingerprint": raw.get("last_fingerprint") if isinstance(raw.get("last_fingerprint"), dict) else {},
        "errors": errors,
    }


def _build_failed_summary(
    *,
    local_files: Dict[str, Dict[str, Any]],
    new_entries: Dict[str, Dict[str, Any]],
    errors: List[Dict[str, str]],
) -> Tuple[int, List[Dict[str, str]]]:
    out: List[Dict[str, str]] = []
    seen: set[Tuple[str, str, str]] = set()
    errored_rel_paths: set[str] = {
        str(item.get("rel_path") or "").strip()
        for item in errors
        if isinstance(item, dict) and str(item.get("rel_path") or "").strip()
    }
    total = 0

    def _push(*, rel_path: str, code: str, message: str) -> None:
        nonlocal total
        row = {
            "rel_path": rel_path,
            "code": code,
            "message": message,
        }
        key = (row["rel_path"], row["code"], row["message"])
        if key in seen:
            return
        seen.add(key)
        total += 1
        if len(out) < _FAILED_ITEMS_LIMIT:
            out.append(row)

    for rel_path in sorted(local_files.keys()):
        if rel_path in errored_rel_paths:
            continue
        item = new_entries.get(rel_path) if isinstance(new_entries.get(rel_path), dict) else {}
        if str(item.get("source_id") or "").strip():
            continue
        _push(
            rel_path=rel_path,
            code="space_source_unsynced",
            message="local resource is not mapped to any remote source",
        )

    for err in errors:
        if not isinstance(err, dict):
            continue
        rel_path = str(err.get("rel_path") or "").strip()
        code = str(err.get("code") or "space_sync_error").strip() or "space_sync_error"
        message = str(err.get("message") or "").strip()
        _push(rel_path=rel_path, code=code, message=message)
    return total, out


def _failure_signature(failed_items: List[Dict[str, str]]) -> str:
    if not failed_items:
        return ""
    digest = hashlib.sha256()
    for item in failed_items[:_FAILED_ITEMS_LIMIT]:
        digest.update(str(item.get("rel_path") or "").encode("utf-8"))
        digest.update(b"\0")
        digest.update(str(item.get("code") or "").encode("utf-8"))
        digest.update(b"\0")
        digest.update(str(item.get("message") or "").encode("utf-8"))
        digest.update(b"\n")
    return digest.hexdigest()


def _sync_notify_targets(group: Any, *, by: str) -> List[str]:
    targets: List[str] = []
    foreman = find_foreman(group)
    foreman_id = str((foreman or {}).get("id") or "").strip() if isinstance(foreman, dict) else ""
    if foreman_id:
        targets.append(foreman_id)

    actor_id = str(by or "").strip()
    if actor_id and actor_id not in _RESERVED_NOTIFY_TARGETS and isinstance(find_actor(group, actor_id), dict):
        if actor_id not in targets:
            targets.insert(0, actor_id)
    return targets


def _emit_sync_notification(
    *,
    group: Any,
    by: str,
    provider: str,
    remote_space_id: str,
    run_id: str,
    converged: bool,
    unsynced_count: int,
    failed_count: int,
    failed_items: List[Dict[str, str]],
) -> None:
    targets = _sync_notify_targets(group, by=by)
    if not targets:
        return
    notify_by = str(by or "").strip() or "system"
    if notify_by in _RESERVED_NOTIFY_TARGETS:
        notify_by = "system"

    if converged:
        title = "Group Space sync recovered"
        message = (
            f"NotebookLM sync is healthy again.\n"
            f"run_id={run_id}\n"
            f"unsynced={unsynced_count}"
        )
        kind = "status_change"
        priority = "normal"
    else:
        first = failed_items[0] if failed_items else {}
        rel = str(first.get("rel_path") or "").strip()
        code = str(first.get("code") or "space_sync_error")
        label = rel or "(global)"
        title = "Group Space sync failed"
        message = (
            f"NotebookLM sync has failures.\n"
            f"run_id={run_id}\n"
            f"failed={failed_count} unsynced={unsynced_count}\n"
            f"first={label} [{code}]"
        )
        kind = "error"
        priority = "high"

    context = {
        "provider": provider,
        "remote_space_id": remote_space_id,
        "run_id": run_id,
        "converged": bool(converged),
        "unsynced_count": int(unsynced_count),
        "failed_count": int(failed_count),
        "failed_items": failed_items[:_FAILED_ITEMS_LIMIT],
    }
    for actor_id in targets:
        try:
            append_event(
                group.ledger_path,
                kind="system.notify",
                group_id=str(group.group_id),
                scope_key="",
                by=notify_by,
                data={
                    "kind": kind,
                    "priority": priority,
                    "title": title,
                    "message": message,
                    "target_actor_id": actor_id,
                    "requires_ack": False,
                    "context": context,
                },
            )
        except Exception:
            continue


def _load_index(space_root: Path) -> Dict[str, Any]:
    return _normalize_index_doc(read_json(space_index_path(space_root)))


def _save_index(space_root: Path, doc: Dict[str, Any]) -> None:
    out = _normalize_index_doc(doc)
    out["updated_at"] = _now_iso()
    atomic_write_json(space_index_path(space_root), out, indent=2)


def _load_state(space_root: Path) -> Dict[str, Any]:
    return _normalize_state_doc(read_json(space_state_path(space_root)))


def _save_state(space_root: Path, doc: Dict[str, Any]) -> None:
    out = _normalize_state_doc(doc)
    atomic_write_json(space_state_path(space_root), out, indent=2)


def _scan_local(space_root: Path, previous_entries: Dict[str, Dict[str, Any]]) -> Dict[str, Dict[str, Any]]:
    out: Dict[str, Dict[str, Any]] = {}
    for abs_path, rel_path in _iter_space_files(space_root):
        try:
            st = abs_path.stat()
        except Exception:
            continue
        size = int(st.st_size)
        mtime_ns = int(getattr(st, "st_mtime_ns", int(st.st_mtime * 1_000_000_000)))
        prev = previous_entries.get(rel_path) if isinstance(previous_entries.get(rel_path), dict) else {}
        sha = ""
        if prev and int(prev.get("size") or 0) == size and int(prev.get("mtime_ns") or 0) == mtime_ns:
            sha = str(prev.get("sha256") or "").strip()
        if not sha:
            try:
                sha = _file_sha256(abs_path)
            except Exception:
                sha = ""
        out[rel_path] = {
            "rel_path": rel_path,
            "abs_path": str(abs_path),
            "path_hash": _path_hash(rel_path),
            "sha256": sha,
            "size": size,
            "mtime_ns": mtime_ns,
        }
    return out


def _migrate_legacy_source_layout(
    space_root: Path,
    *,
    provider: str,
    index_doc: Dict[str, Any],
) -> bool:
    changed = False
    sources_root = space_root / _REMOTE_SOURCE_TEXT_ROOT
    legacy_dir = sources_root / _safe_file_component(provider, fallback=_LEGACY_REMOTE_SOURCE_SUBDIR)
    if legacy_dir.exists() and legacy_dir.is_dir():
        for path in sorted(legacy_dir.rglob("*")):
            if not path.is_file():
                continue
            rel_in_legacy = path.relative_to(legacy_dir).as_posix()
            target = sources_root / rel_in_legacy
            target.parent.mkdir(parents=True, exist_ok=True)
            if target.exists():
                try:
                    if _file_sha256(path) == _file_sha256(target):
                        path.unlink(missing_ok=True)
                        changed = True
                        continue
                except Exception:
                    pass
                stem = target.stem
                suffix = target.suffix
                alt = target.with_name(f"{stem}_legacy{suffix}")
                n = 2
                while alt.exists():
                    alt = target.with_name(f"{stem}_legacy_{n}{suffix}")
                    n += 1
                target = alt
            try:
                path.rename(target)
                changed = True
            except Exception:
                continue
        for path in sorted(legacy_dir.rglob("*"), reverse=True):
            if path.is_dir():
                try:
                    path.rmdir()
                except Exception:
                    pass
        try:
            legacy_dir.rmdir()
        except Exception:
            pass

    entries = index_doc.get("entries") if isinstance(index_doc.get("entries"), dict) else {}
    if entries:
        prefix = f"{_REMOTE_SOURCE_TEXT_ROOT}/{_safe_file_component(provider, fallback=_LEGACY_REMOTE_SOURCE_SUBDIR)}/"
        new_entries: Dict[str, Dict[str, Any]] = {}
        for rel_path, item in entries.items():
            rel = str(rel_path or "").strip()
            if not rel:
                continue
            row = dict(item) if isinstance(item, dict) else {}
            moved = rel.startswith(prefix)
            rel_new = f"{_REMOTE_SOURCE_TEXT_ROOT}/{rel[len(prefix):]}" if moved else rel
            row["rel_path"] = rel_new
            row["path_hash"] = _path_hash(rel_new)
            if rel_new in new_entries:
                old_sid = str((new_entries.get(rel_new) or {}).get("source_id") or "").strip()
                new_sid = str(row.get("source_id") or "").strip()
                if old_sid and not new_sid:
                    continue
            new_entries[rel_new] = row
            if moved:
                changed = True
        if changed:
            index_doc["entries"] = new_entries
    return changed


def _write_remote_source_snapshots(space_root: Path, *, provider: str, remote_space_id: str, sources: List[Dict[str, Any]]) -> int:
    sync_dir = space_root / _REMOTE_SYNC_DIR
    source_dir = sync_dir / _REMOTE_SOURCES_DIR
    source_dir.mkdir(parents=True, exist_ok=True)

    seen: set[str] = set()
    changed = 0
    for row in sources:
        if not isinstance(row, dict):
            continue
        sid = _source_id(row)
        if not sid:
            continue
        name = f"{_safe_file_component(sid, fallback='source')}.json"
        path = source_dir / name
        seen.add(name)
        payload = {
            "v": 1,
            "provider": provider,
            "remote_space_id": remote_space_id,
            "source_id": sid,
            "title": str(row.get("title") or ""),
            "kind": str(row.get("kind") or ""),
            "status": row.get("status"),
            "url": str(row.get("url") or ""),
            "synced_at": _now_iso(),
        }
        prev = read_json(path)
        if not isinstance(prev, dict) or prev.get("source_id") != sid or prev.get("title") != payload["title"] or prev.get("kind") != payload["kind"] or prev.get("status") != payload["status"] or prev.get("url") != payload["url"]:
            atomic_write_json(path, payload, indent=2)
            changed += 1

    for file in source_dir.glob("*.json"):
        if file.name in seen:
            continue
        try:
            file.unlink(missing_ok=True)
            changed += 1
        except Exception:
            pass
    return changed


def _materialize_remote_source_texts(
    space_root: Path,
    *,
    provider: str,
    remote_space_id: str,
    sources: List[Dict[str, Any]],
    previous_entries: Dict[str, Dict[str, Any]],
    local_files: Dict[str, Dict[str, Any]],
    mapped_entries: Dict[str, Dict[str, Any]],
) -> Tuple[int, Dict[str, Dict[str, Any]], List[Dict[str, str]]]:
    root = space_root / _REMOTE_SOURCE_TEXT_ROOT
    root.mkdir(parents=True, exist_ok=True)
    preview_root = space_root / _REMOTE_SYNC_DIR / _REMOTE_SOURCE_PREVIEW_DIR
    preview_root.mkdir(parents=True, exist_ok=True)
    changed = 0
    out_entries: Dict[str, Dict[str, Any]] = {}
    out_errors: List[Dict[str, str]] = []

    def _append_materialize_error(code: str, message: str, *, rel_path: str = "") -> None:
        if len(out_errors) >= _MAX_REMOTE_ERRORS:
            return
        out_errors.append(
            {
                "code": str(code or "space_sync_error"),
                "message": str(message or "materialize error"),
                "rel_path": str(rel_path or "").strip(),
            }
        )

    source_to_path: Dict[str, str] = {}
    for rel_path, item in mapped_entries.items():
        if not isinstance(item, dict):
            continue
        sid = str(item.get("source_id") or "").strip()
        rel = str(rel_path or "").strip()
        if sid and rel and sid not in source_to_path:
            source_to_path[sid] = rel
    local_hashes: set[str] = {
        str(meta.get("path_hash") or "").strip()
        for meta in local_files.values()
        if isinstance(meta, dict) and str(meta.get("path_hash") or "").strip()
    }

    for row in sources:
        if not isinstance(row, dict):
            continue
        source_id = _source_id(row)
        if not source_id:
            continue
        title = str(row.get("title") or "")
        marker_hash = _parse_marker_hash(title)
        if marker_hash:
            mapped_rel_existing = str(source_to_path.get(source_id) or "").strip()
            local_projection_exists = bool(
                (marker_hash in local_hashes)
                or (mapped_rel_existing and mapped_rel_existing in local_files)
            )
            if local_projection_exists:
                # Local-managed source with an existing local projection.
                continue

        kind = _normalize_source_kind(row.get("kind"))
        status_raw = row.get("status")
        full_title = title or source_id
        full_url = str(row.get("url") or "")
        content = ""
        if _source_is_ready(status_raw):
            try:
                full = provider_get_source_fulltext(
                    provider,
                    remote_space_id=remote_space_id,
                    source_id=source_id,
                )
                full_title = str(full.get("title") or full_title)
                full_url = str(full.get("url") or full_url)
                kind = _normalize_source_kind(full.get("kind") or kind)
                content = str(full.get("content") or "")
            except Exception as e:
                code = "space_provider_upstream_error"
                if isinstance(e, SpaceProviderError):
                    code = str(e.code or code)
                _append_materialize_error(
                    code,
                    f"materialize remote source failed ({source_id}): {e}",
                    rel_path=_remote_source_descriptor_rel_path(
                        source_id,
                        kind=kind or "unknown",
                        title=full_title,
                        url=full_url,
                        content=content,
                    ),
                )

        mapped_rel = str(source_to_path.get(source_id) or "").strip()
        preferred_rel = _remote_source_descriptor_rel_path(
            source_id,
            kind=kind or "unknown",
            title=full_title,
            url=full_url,
            content=content,
        )
        rel_norm = preferred_rel
        if _is_source_descriptor_path(mapped_rel) and not _descriptor_name_is_unhelpful(mapped_rel, source_id=source_id):
            rel_norm = mapped_rel
        descriptor_path = (space_root / rel_norm).resolve()
        descriptor_path.parent.mkdir(parents=True, exist_ok=True)
        display_name = _source_label(kind or "unknown", title=full_title, url=full_url, content=content)

        descriptor_doc = {
            "v": 1,
            "provider": provider,
            "remote_space_id": remote_space_id,
            "source_id": source_id,
            "type": kind or "unknown",
            "kind": kind or "unknown",
            "title": full_title,
            "url": full_url,
            "status": str(status_raw if status_raw is not None else ""),
            "display_name": display_name,
            "mode": "remote_mirror",
            "read_only": True,
        }
        try:
            prev_doc = read_json(descriptor_path)
            if not isinstance(prev_doc, dict) or prev_doc != descriptor_doc:
                atomic_write_json(descriptor_path, descriptor_doc, indent=2)
                changed += 1
        except Exception as e:
            _append_materialize_error(
                "space_local_materialize_failed",
                f"write source descriptor failed ({rel_norm}): {e}",
                rel_path=rel_norm,
            )
            continue

        preview_rel = _remote_source_preview_rel_from_descriptor(rel_norm, kind=kind or "unknown")
        preview_path = (space_root / preview_rel).resolve()
        preview_path.parent.mkdir(parents=True, exist_ok=True)
        rendered = ""
        if content.strip():
            rendered = _render_source_content_text(
                source_id=source_id,
                title=full_title,
                kind=kind or "unknown",
                url=full_url,
                content=content,
            )
        else:
            rendered = (
                ("[Source still processing]\n" if not _source_is_ready(status_raw) else "[No extractable text available]\n")
                + f"source_id={source_id}\n"
                + f"title={full_title}\n"
                + f"kind={kind or 'unknown'}\n"
                + f"url={full_url}\n"
            )
        try:
            prev_preview = preview_path.read_text(encoding="utf-8") if preview_path.exists() else ""
        except Exception:
            prev_preview = ""
        if prev_preview != rendered:
            try:
                preview_path.write_text(rendered, encoding="utf-8")
                changed += 1
            except Exception as e:
                _append_materialize_error(
                    "space_local_materialize_failed",
                    f"write source preview failed ({preview_rel}): {e}",
                    rel_path=preview_rel,
                )

        # Migrate legacy remote mirror files (e.g. old sources/<id>.txt) out of active source tree.
        if mapped_rel and (not _is_source_descriptor_path(mapped_rel)) and mapped_rel.startswith(f"{_REMOTE_SOURCE_TEXT_ROOT}/"):
            legacy_path = (space_root / mapped_rel).resolve()
            if legacy_path.exists() and legacy_path.is_file():
                archive_dir = (space_root / _REMOTE_SYNC_DIR / "legacy-remote-mirrors").resolve()
                archive_dir.mkdir(parents=True, exist_ok=True)
                archive_name = f"{_safe_file_component(source_id, fallback='source')}-{_safe_file_component(legacy_path.name, fallback='legacy')}"
                archive_path = archive_dir / archive_name
                idx = 2
                while archive_path.exists():
                    archive_path = archive_dir / f"{archive_name}.{idx}"
                    idx += 1
                try:
                    legacy_path.replace(archive_path)
                    changed += 1
                except Exception as e:
                    _append_materialize_error(
                        "space_local_materialize_failed",
                        f"archive legacy source mirror failed ({mapped_rel}): {e}",
                        rel_path=mapped_rel,
                    )
        if mapped_rel and _is_source_descriptor_path(mapped_rel) and mapped_rel != rel_norm:
            old_descriptor_path = (space_root / mapped_rel).resolve()
            if old_descriptor_path.exists() and old_descriptor_path.is_file():
                archive_dir = (space_root / _REMOTE_SYNC_DIR / "legacy-remote-mirrors").resolve()
                archive_dir.mkdir(parents=True, exist_ok=True)
                archive_name = f"{_safe_file_component(source_id, fallback='source')}-{_safe_file_component(old_descriptor_path.name, fallback='legacy')}"
                archive_path = archive_dir / archive_name
                idx = 2
                while archive_path.exists():
                    archive_path = archive_dir / f"{archive_name}.{idx}"
                    idx += 1
                try:
                    old_descriptor_path.replace(archive_path)
                    changed += 1
                except Exception as e:
                    _append_materialize_error(
                        "space_local_materialize_failed",
                        f"archive legacy source descriptor failed ({mapped_rel}): {e}",
                        rel_path=mapped_rel,
                    )

        try:
            st = descriptor_path.stat()
            file_sha = _file_sha256(descriptor_path)
            out_entries[rel_norm] = {
                "rel_path": rel_norm,
                "path_hash": _path_hash(rel_norm),
                "sha256": file_sha,
                "size": int(st.st_size),
                "mtime_ns": int(getattr(st, "st_mtime_ns", int(st.st_mtime * 1_000_000_000))),
                "source_id": source_id,
                "remote_title": full_title,
                "last_synced_at": _now_iso(),
            }
        except Exception as e:
            _append_materialize_error(
                "space_local_materialize_failed",
                f"snapshot mirrored source failed ({rel_norm}): {e}",
                rel_path=rel_norm,
            )
            continue

        source_to_path[source_id] = rel_norm

    return changed, out_entries, out_errors


def _load_remote_artifacts_manifest(space_root: Path) -> Dict[str, Dict[str, Any]]:
    manifest_path = space_root / _REMOTE_SYNC_DIR / _REMOTE_ARTIFACTS_MANIFEST
    raw = read_json(manifest_path)
    if not isinstance(raw, dict):
        return {}
    entries_raw = raw.get("entries")
    entries = entries_raw if isinstance(entries_raw, dict) else {}
    out: Dict[str, Dict[str, Any]] = {}
    for key, item in entries.items():
        k = str(key or "").strip()
        if not k or not isinstance(item, dict):
            continue
        out[k] = dict(item)
    return out


def _save_remote_artifacts_manifest(
    space_root: Path,
    *,
    provider: str,
    remote_space_id: str,
    entries: Dict[str, Dict[str, Any]],
) -> None:
    manifest_path = space_root / _REMOTE_SYNC_DIR / _REMOTE_ARTIFACTS_MANIFEST
    manifest_path.parent.mkdir(parents=True, exist_ok=True)
    atomic_write_json(
        manifest_path,
        {
            "v": 1,
            "provider": provider,
            "remote_space_id": remote_space_id,
            "updated_at": _now_iso(),
            "entries": entries,
        },
        indent=2,
    )


def _source_id(raw: Dict[str, Any]) -> str:
    return str(raw.get("source_id") or "").strip()


def read_group_space_sync_state(group_id: str) -> Dict[str, Any]:
    space_root = resolve_space_root(group_id, create=False)
    if space_root is None:
        return {"available": False, "reason": "no_local_scope"}
    state = _load_state(space_root)
    state["available"] = True
    state["space_root"] = str(space_root)
    return state


def sync_group_space_files(
    group_id: str,
    *,
    provider: str = "notebooklm",
    force: bool = False,
    by: str = "",
) -> Dict[str, Any]:
    gid = str(group_id or "").strip()
    if not gid:
        return {"ok": False, "code": "missing_group_id", "message": "missing group_id"}
    group = load_group(gid)
    if group is None:
        return {"ok": False, "code": "group_not_found", "message": f"group not found: {gid}"}

    binding = get_space_binding(gid, provider=provider)
    if not isinstance(binding, dict):
        return {"ok": False, "code": "space_binding_missing", "message": "group is not bound to provider"}
    if str(binding.get("status") or "") != "bound":
        return {"ok": False, "code": "space_binding_missing", "message": "group space binding is not active"}
    remote_space_id = str(binding.get("remote_space_id") or "").strip()
    if not remote_space_id:
        return {"ok": False, "code": "space_binding_missing", "message": "binding has no remote_space_id"}

    provider_state = get_space_provider_state(provider)
    if not bool(provider_state.get("enabled")) or str(provider_state.get("mode") or "") == "disabled":
        return {"ok": False, "code": "space_provider_disabled", "message": "provider is disabled"}

    space_root = resolve_space_root_from_group(group, create=True)
    if space_root is None:
        return {"ok": False, "code": "no_local_scope", "message": "group has no local scope"}

    index_path = space_index_path(space_root)
    index_existed = bool(index_path.exists())
    index_doc = _load_index(space_root)
    state_doc = _load_state(space_root)
    if _migrate_legacy_source_layout(space_root, provider=provider, index_doc=index_doc):
        _save_index(space_root, index_doc)

    prev_converged = bool(state_doc.get("converged"))
    prev_failure_signature = str(state_doc.get("failure_signature") or "").strip()
    fingerprint = _space_fingerprint(space_root)
    stale_seconds = _reconcile_stale_seconds()
    last_run = _parse_utc_ts(state_doc.get("last_run_at"))
    age_seconds = None
    if last_run is not None:
        age_seconds = (datetime.now(timezone.utc) - last_run).total_seconds()

    can_skip = (
        (not force)
        and str(state_doc.get("remote_space_id") or "") == remote_space_id
        and bool(state_doc.get("converged"))
        and isinstance(state_doc.get("last_fingerprint"), dict)
        and state_doc.get("last_fingerprint") == fingerprint
        and (age_seconds is not None and age_seconds < float(stale_seconds))
    )
    if can_skip:
        return {
            "ok": True,
            "group_id": gid,
            "provider": provider,
            "remote_space_id": remote_space_id,
            "space_root": str(space_root),
            "skipped": True,
            "reason": "no_local_change",
            "run_id": str(state_doc.get("run_id") or ""),
            "state": state_doc,
        }

    entries = index_doc.get("entries") if isinstance(index_doc.get("entries"), dict) else {}
    local_files = _scan_local(space_root, entries)
    local_by_hash = {str(meta.get("path_hash") or ""): rel for rel, meta in local_files.items()}

    run_id = _sync_run_id()
    errors: List[Dict[str, str]] = []
    new_entries: Dict[str, Dict[str, Any]] = {}
    keep_source_ids: set[str] = set()
    claimed_remote_ids: set[str] = set()
    deleted_source_ids: set[str] = set()
    uploaded = 0
    updated = 0
    deleted = 0
    reused = 0
    remote_source_count = 0
    materialized_sources = 0
    remote_artifact_count = 0
    downloaded_artifacts = 0
    pruned_artifacts = 0
    local_size_limit_bytes = _local_source_size_limit_bytes()

    def _append_error(code: str, message: str, *, rel_path: str = "") -> None:
        if len(errors) >= _MAX_REMOTE_ERRORS:
            return
        errors.append(
            {
                "code": str(code or "space_upstream_error"),
                "message": str(message or "provider error"),
                "rel_path": str(rel_path or "").strip(),
            }
        )

    with acquire_space_provider_write(provider, remote_space_id):
        try:
            remote_list = provider_list_sources(provider, remote_space_id=remote_space_id)
        except SpaceProviderError as e:
            err_code = str(e.code or "space_provider_upstream_error")
            message = str(e)
            failed_items = [
                {
                    "rel_path": "",
                    "code": err_code,
                    "message": message,
                }
            ]
            failure_signature = _failure_signature(failed_items)
            state_doc.update(
                {
                    "group_id": gid,
                    "provider": provider,
                    "remote_space_id": remote_space_id,
                    "run_id": run_id,
                    "last_run_at": _now_iso(),
                    "state": "error",
                    "converged": False,
                    "unsynced_count": len(local_files),
                    "failed_count": len(failed_items),
                    "failed_items": failed_items,
                    "last_error": message,
                    "failure_signature": failure_signature,
                    "last_fingerprint": fingerprint,
                    "errors": [{"code": err_code, "message": message, "rel_path": ""}],
                }
            )
            _save_state(space_root, state_doc)
            if prev_converged or prev_failure_signature != failure_signature:
                _emit_sync_notification(
                    group=group,
                    by=by,
                    provider=provider,
                    remote_space_id=remote_space_id,
                    run_id=run_id,
                    converged=False,
                    unsynced_count=int(len(local_files)),
                    failed_count=int(len(failed_items)),
                    failed_items=failed_items,
                )
            return {
                "ok": False,
                "code": err_code,
                "message": message,
                "group_id": gid,
                "provider": provider,
                "remote_space_id": remote_space_id,
                "space_root": str(space_root),
                "run_id": run_id,
                "failed_items": failed_items,
            }

        sources = remote_list.get("sources") if isinstance(remote_list.get("sources"), list) else []
        remote_sources: List[Dict[str, Any]] = [dict(item) for item in sources if isinstance(item, dict)]
        remote_by_id: Dict[str, Dict[str, Any]] = {}
        managed_by_hash: Dict[str, List[Dict[str, Any]]] = {}
        for item in remote_sources:
            sid = _source_id(item)
            if sid:
                remote_by_id[sid] = item
            marker_hash = _parse_marker_hash(item.get("title"))
            if marker_hash:
                managed_by_hash.setdefault(marker_hash, []).append(item)

        # Safety guard:
        # When local space was recreated (index missing) and no local files exist yet,
        # treat this run as a remote->local rehydrate pass and never delete remote sources.
        # This prevents accidental data loss after users remove `space/` and sync again.
        rehydrate_only = bool(
            (not index_existed)
            and (not entries)
            and (not local_files)
            and bool(remote_sources)
        )

        for rel_path in sorted(local_files.keys()):
            meta = local_files.get(rel_path) or {}
            prev = entries.get(rel_path) if isinstance(entries.get(rel_path), dict) else {}
            path_hash = str(meta.get("path_hash") or "")
            prev_sid = str(prev.get("source_id") or "").strip()
            prev_remote_title = str(prev.get("remote_title") or "").strip()
            prev_marker_hash = _parse_marker_hash(prev_remote_title)

            if _is_source_descriptor_path(rel_path):
                source = remote_by_id.get(prev_sid) if prev_sid else None
                source_title = str((source or {}).get("title") or prev_remote_title).strip()
                source_marker_hash = _parse_marker_hash(source_title)
                remote_managed_prev = bool(prev_sid and prev_remote_title and not prev_marker_hash)
                remote_missing_for_prev = bool(prev_sid and prev_sid not in remote_by_id and source is None)
                local_unchanged = bool(
                    str(meta.get("sha256") or "").strip()
                    and str(prev.get("sha256") or "").strip()
                    and str(meta.get("sha256") or "").strip() == str(prev.get("sha256") or "").strip()
                )
                if remote_missing_for_prev and remote_managed_prev:
                    abs_path_raw = str(meta.get("abs_path") or "").strip()
                    removed_local = False
                    if abs_path_raw:
                        path_obj = Path(abs_path_raw)
                        try:
                            path_obj.unlink(missing_ok=True)
                        except Exception:
                            pass
                        removed_local = not path_obj.exists()
                    if removed_local:
                        local_files.pop(rel_path, None)
                        if local_by_hash.get(path_hash) == rel_path:
                            local_by_hash.pop(path_hash, None)
                        continue
                    _append_error(
                        "space_remote_source_removed",
                        (
                            f"remote-managed source removed upstream; local mirror cleanup failed ({rel_path})"
                            if local_unchanged
                            else f"remote-managed source removed upstream; local edited mirror ignored ({rel_path})"
                        ),
                        rel_path=rel_path,
                    )
                    continue

                remote_managed_bound = bool(source and prev_sid and not source_marker_hash)
                if remote_managed_bound and prev_sid:
                    keep_source_ids.add(prev_sid)
                    if not local_unchanged:
                        _append_error(
                            "space_remote_source_read_only",
                            (
                                f"local edits ignored for remote-managed source descriptor ({rel_path}); "
                                "edit in provider origin instead"
                            ),
                            rel_path=rel_path,
                        )
                    else:
                        reused += 1
                    new_entries[rel_path] = {
                        "rel_path": rel_path,
                        "path_hash": path_hash,
                        "sha256": str(meta.get("sha256") or ""),
                        "size": int(meta.get("size") or 0),
                        "mtime_ns": int(meta.get("mtime_ns") or 0),
                        "source_id": prev_sid,
                        "remote_title": source_title or prev_remote_title,
                        "last_synced_at": _now_iso(),
                    }
                    continue

                _append_error(
                    "space_source_descriptor_read_only",
                    (
                        f"descriptor source is read-only in local sync path ({rel_path}); "
                        "use Group Space Add Source (resource_ingest) to create or modify remote URL/Docs/Youtube sources"
                    ),
                    rel_path=rel_path,
                )
                if prev and isinstance(prev, dict):
                    new_entries[rel_path] = dict(prev)
                continue

            if _local_source_policy(rel_path) == "unsupported":
                _append_error(
                    "space_source_unsupported_format",
                    _local_source_format_error_message(rel_path),
                    rel_path=rel_path,
                )
                if prev and isinstance(prev, dict):
                    new_entries[rel_path] = dict(prev)
                continue
            size_bytes = int(meta.get("size") or 0)
            if size_bytes > int(local_size_limit_bytes):
                _append_error(
                    "space_source_file_too_large",
                    _local_source_size_error_message(
                        rel_path,
                        size_bytes=size_bytes,
                        limit_bytes=int(local_size_limit_bytes),
                    ),
                    rel_path=rel_path,
                )
                if prev and isinstance(prev, dict):
                    new_entries[rel_path] = dict(prev)
                continue
            marker = _marker_title(path_hash, rel_path)
            sid = ""
            source = None
            if prev_sid and prev_sid in remote_by_id:
                sid = prev_sid
                source = remote_by_id.get(prev_sid)
                claimed_remote_ids.add(prev_sid)
            elif path_hash in managed_by_hash:
                for candidate in managed_by_hash.get(path_hash) or []:
                    cand_sid = _source_id(candidate)
                    if not cand_sid or cand_sid in claimed_remote_ids:
                        continue
                    sid = cand_sid
                    source = candidate
                    claimed_remote_ids.add(cand_sid)
                    break

            same_content = bool(
                source
                and str(prev.get("sha256") or "") == str(meta.get("sha256") or "")
                and str(meta.get("sha256") or "")
            )
            remote_missing_for_prev = bool(prev_sid and prev_sid not in remote_by_id and source is None)
            local_unchanged = bool(
                str(meta.get("sha256") or "").strip()
                and str(prev.get("sha256") or "").strip()
                and str(meta.get("sha256") or "").strip() == str(prev.get("sha256") or "").strip()
            )
            remote_managed_prev = bool(prev_sid and prev_remote_title and not prev_marker_hash)
            source_title = str((source or {}).get("title") or prev.get("remote_title") or "").strip()
            source_marker_hash = _parse_marker_hash(source_title)
            remote_managed_bound = bool(source and sid and not source_marker_hash)

            try:
                # If a remote-managed source was removed remotely, drop local mirror instead of re-uploading it.
                if remote_missing_for_prev and remote_managed_prev:
                    abs_path_raw = str(meta.get("abs_path") or "").strip()
                    removed_local = False
                    if abs_path_raw:
                        path_obj = Path(abs_path_raw)
                        try:
                            path_obj.unlink(missing_ok=True)
                        except Exception:
                            pass
                        removed_local = not path_obj.exists()
                    if removed_local:
                            local_files.pop(rel_path, None)
                            if local_by_hash.get(path_hash) == rel_path:
                                local_by_hash.pop(path_hash, None)
                            continue
                    _append_error(
                        "space_remote_source_removed",
                        (
                            f"remote-managed source removed upstream; local mirror cleanup failed ({rel_path})"
                            if local_unchanged
                            else f"remote-managed source removed upstream; local edited mirror ignored ({rel_path})"
                        ),
                        rel_path=rel_path,
                    )
                    continue

                if remote_managed_bound and sid:
                    keep_source_ids.add(sid)
                    if rel_path.startswith(f"{_REMOTE_SOURCE_TEXT_ROOT}/") and (not _is_source_descriptor_path(rel_path)):
                        if not local_unchanged:
                            _append_error(
                                "space_remote_source_read_only",
                                (
                                    f"local edits ignored for remote-managed source mirror ({rel_path}); "
                                    "edit in provider origin instead"
                                ),
                                rel_path=rel_path,
                            )
                        else:
                            reused += 1
                        # Legacy mirror file is re-materialized as sources/<source_id>.source.json.
                        # Keep remote source alive but do not keep this file mapping in index.
                        local_files.pop(rel_path, None)
                        if local_by_hash.get(path_hash) == rel_path:
                            local_by_hash.pop(path_hash, None)
                        continue
                    if not local_unchanged:
                        _append_error(
                            "space_remote_source_read_only",
                            f"local edits ignored for remote-managed source ({rel_path}); edit in provider origin instead",
                            rel_path=rel_path,
                        )
                    else:
                        reused += 1
                    new_entries[rel_path] = {
                        "rel_path": rel_path,
                        "path_hash": path_hash,
                        "sha256": str(meta.get("sha256") or ""),
                        "size": int(meta.get("size") or 0),
                        "mtime_ns": int(meta.get("mtime_ns") or 0),
                        "source_id": sid,
                        "remote_title": source_title or prev_remote_title,
                        "last_synced_at": _now_iso(),
                    }
                    continue

                if same_content and sid:
                    reused += 1
                    if source_marker_hash and source_marker_hash != path_hash:
                        renamed = provider_rename_source(
                            provider,
                            remote_space_id=remote_space_id,
                            source_id=sid,
                            new_title=marker,
                        )
                        source["title"] = str(renamed.get("title") or marker)
                    keep_source_ids.add(sid)
                    new_entries[rel_path] = {
                        "rel_path": rel_path,
                        "path_hash": path_hash,
                        "sha256": str(meta.get("sha256") or ""),
                        "size": int(meta.get("size") or 0),
                        "mtime_ns": int(meta.get("mtime_ns") or 0),
                        "source_id": sid,
                        "remote_title": str(source.get("title") or marker),
                        "last_synced_at": _now_iso(),
                    }
                    continue

                add_out = provider_add_file_source(
                    provider,
                    remote_space_id=remote_space_id,
                    file_path=str(meta.get("abs_path") or ""),
                )
                new_sid = _source_id(add_out)
                if not new_sid:
                    raise SpaceProviderError(
                        code="space_provider_upstream_error",
                        message=f"provider add_file returned empty source_id: {rel_path}",
                        transient=False,
                        degrade_provider=False,
                    )
                desired_title = marker
                if source and sid and not source_marker_hash:
                    desired_title = source_title or marker
                elif remote_missing_for_prev and source_title and not source_marker_hash:
                    desired_title = source_title
                renamed = provider_rename_source(
                    provider,
                    remote_space_id=remote_space_id,
                    source_id=new_sid,
                    new_title=desired_title,
                )
                keep_source_ids.add(new_sid)
                remote_title = str(renamed.get("title") or desired_title)
                if source and sid and sid != new_sid:
                    try:
                        _ = provider_delete_source(
                            provider,
                            remote_space_id=remote_space_id,
                            source_id=sid,
                        )
                        deleted += 1
                        deleted_source_ids.add(sid)
                    except Exception as e:
                        _append_error(
                            "space_provider_upstream_error",
                            f"delete old source failed ({rel_path}): {e}",
                            rel_path=rel_path,
                        )
                if source and sid and sid != new_sid:
                    updated += 1
                else:
                    uploaded += 1
                new_entries[rel_path] = {
                    "rel_path": rel_path,
                    "path_hash": path_hash,
                    "sha256": str(meta.get("sha256") or ""),
                    "size": int(meta.get("size") or 0),
                    "mtime_ns": int(meta.get("mtime_ns") or 0),
                    "source_id": new_sid,
                    "remote_title": remote_title,
                    "last_synced_at": _now_iso(),
                }
            except Exception as e:
                code = "space_provider_upstream_error"
                if isinstance(e, SpaceProviderError):
                    code = str(e.code or code)
                _append_error(code, f"sync failed ({rel_path}): {e}", rel_path=rel_path)
                if prev and isinstance(prev, dict):
                    new_entries[rel_path] = dict(prev)

        if not rehydrate_only:
            for rel_path, prev in sorted(entries.items()):
                if rel_path in local_files:
                    continue
                prev_sid = str(prev.get("source_id") or "").strip()
                if not prev_sid:
                    continue
                if prev_sid not in remote_by_id:
                    continue
                try:
                    _ = provider_delete_source(
                        provider,
                        remote_space_id=remote_space_id,
                        source_id=prev_sid,
                    )
                    deleted += 1
                    deleted_source_ids.add(prev_sid)
                except Exception as e:
                    code = "space_provider_upstream_error"
                    if isinstance(e, SpaceProviderError):
                        code = str(e.code or code)
                    _append_error(
                        code,
                        f"delete missing-local source failed ({rel_path}): {e}",
                        rel_path=rel_path,
                    )

            for item in remote_sources:
                sid = _source_id(item)
                if not sid or sid in keep_source_ids or sid in deleted_source_ids:
                    continue
                marker_hash = _parse_marker_hash(item.get("title"))
                if not marker_hash:
                    continue
                if marker_hash in local_by_hash:
                    continue
                try:
                    _ = provider_delete_source(
                        provider,
                        remote_space_id=remote_space_id,
                        source_id=sid,
                    )
                    deleted += 1
                    deleted_source_ids.add(sid)
                except Exception as e:
                    code = "space_provider_upstream_error"
                    if isinstance(e, SpaceProviderError):
                        code = str(e.code or code)
                    _append_error(code, f"delete ghost source failed ({sid}): {e}", rel_path=f"remote:{sid}")

        try:
            remote_list_after = provider_list_sources(provider, remote_space_id=remote_space_id)
            rows_after = remote_list_after.get("sources") if isinstance(remote_list_after.get("sources"), list) else []
            remote_sources = [dict(item) for item in rows_after if isinstance(item, dict)]
        except Exception as e:
            code = "space_provider_upstream_error"
            if isinstance(e, SpaceProviderError):
                code = str(e.code or code)
            _append_error(code, f"refresh sources failed: {e}")

        remote_source_count = len(remote_sources)
        try:
            materialized_sources = _write_remote_source_snapshots(
                space_root,
                provider=provider,
                remote_space_id=remote_space_id,
                sources=remote_sources,
            )
        except Exception as e:
            _append_error("space_local_materialize_failed", f"materialize remote sources failed: {e}")
        try:
            source_text_changed, materialized_entries, materialize_errors = _materialize_remote_source_texts(
                space_root,
                provider=provider,
                remote_space_id=remote_space_id,
                sources=remote_sources,
                previous_entries=entries,
                local_files=local_files,
                mapped_entries=new_entries,
            )
            materialized_sources += source_text_changed
            for item in materialize_errors:
                if not isinstance(item, dict):
                    continue
                _append_error(
                    str(item.get("code") or "space_sync_error"),
                    str(item.get("message") or "materialize error"),
                    rel_path=str(item.get("rel_path") or ""),
                )
            for rel_path, entry in materialized_entries.items():
                if not isinstance(entry, dict):
                    continue
                new_entries[rel_path] = dict(entry)
        except Exception as e:
            _append_error("space_local_materialize_failed", f"materialize remote source texts failed: {e}")

        prev_artifact_entries = _load_remote_artifacts_manifest(space_root)
        next_artifact_entries: Dict[str, Dict[str, Any]] = dict(prev_artifact_entries)
        listed_artifacts: List[Dict[str, Any]] = []
        artifacts_list_ok = False
        try:
            listed = provider_list_artifacts(provider, remote_space_id=remote_space_id, kind="")
            rows = listed.get("artifacts") if isinstance(listed.get("artifacts"), list) else []
            listed_artifacts = [dict(item) for item in rows if isinstance(item, dict)]
            artifacts_list_ok = True
        except Exception as e:
            code = "space_provider_upstream_error"
            if isinstance(e, SpaceProviderError):
                code = str(e.code or code)
            _append_error(code, f"list artifacts failed: {e}")

        if artifacts_list_ok:
            remote_artifact_count = len(listed_artifacts)
            next_artifact_entries = {}
            artifact_root = space_root / "artifacts" / _safe_file_component(provider, fallback="provider")
            for art in listed_artifacts:
                aid = str(art.get("artifact_id") or art.get("id") or "").strip()
                kind = _normalize_artifact_kind(art.get("kind"))
                status = str(art.get("status") or "").strip()
                if not aid or not kind:
                    continue
                output_format = "markdown" if kind in {"quiz", "flashcards"} else ""
                ext = _artifact_extension(kind, output_format=output_format)
                safe_kind = _safe_file_component(kind, fallback="artifact")
                safe_aid = _safe_file_component(aid, fallback="artifact")
                target = artifact_root / safe_kind / f"{safe_aid}{ext}"
                key = f"{kind}:{aid}"
                row: Dict[str, Any] = {
                    "artifact_id": aid,
                    "kind": kind,
                    "title": str(art.get("title") or ""),
                    "status": status,
                    "created_at": str(art.get("created_at") or ""),
                    "url": str(art.get("url") or ""),
                    "local_path": str(target),
                }
                if _artifact_is_completed(status):
                    if not target.exists():
                        target.parent.mkdir(parents=True, exist_ok=True)
                        try:
                            _ = provider_download_artifact(
                                provider,
                                remote_space_id=remote_space_id,
                                kind=kind,
                                output_path=str(target),
                                artifact_id=aid,
                                output_format=output_format,
                            )
                            downloaded_artifacts += 1
                        except Exception as e:
                            code = "space_provider_upstream_error"
                            if isinstance(e, SpaceProviderError):
                                code = str(e.code or code)
                            _append_error(code, f"download artifact failed ({key}): {e}", rel_path=str(target))
                            row["download_error"] = str(e)
                    row["downloaded"] = bool(target.exists())
                else:
                    row["downloaded"] = bool(target.exists())
                next_artifact_entries[key] = row

            for key, prev in prev_artifact_entries.items():
                if key in next_artifact_entries:
                    continue
                old_path = str(prev.get("local_path") or "").strip()
                if not old_path:
                    continue
                path_obj = Path(old_path).expanduser()
                if not path_obj.is_absolute():
                    path_obj = (space_root / old_path).resolve()
                if path_obj.exists() and path_obj.is_file():
                    try:
                        path_obj.unlink(missing_ok=True)
                        pruned_artifacts += 1
                    except Exception:
                        pass

            try:
                _save_remote_artifacts_manifest(
                    space_root,
                    provider=provider,
                    remote_space_id=remote_space_id,
                    entries=next_artifact_entries,
                )
            except Exception as e:
                _append_error("space_local_materialize_failed", f"save artifact manifest failed: {e}")

    failed_count, failed_items = _build_failed_summary(
        local_files=local_files,
        new_entries=new_entries,
        errors=errors,
    )
    unsynced_count = int(failed_count)
    converged = unsynced_count == 0
    failure_signature = _failure_signature(failed_items)

    index_doc.update(
        {
            "group_id": gid,
            "provider": provider,
            "remote_space_id": remote_space_id,
            "entries": new_entries,
        }
    )
    _save_index(space_root, index_doc)

    state_doc.update(
        {
            "group_id": gid,
            "provider": provider,
            "remote_space_id": remote_space_id,
            "run_id": run_id,
            "last_run_at": _now_iso(),
            "state": ("ok" if converged else "error"),
            "converged": converged,
            "unsynced_count": int(unsynced_count),
            "failed_count": int(failed_count),
            "failed_items": failed_items[:_FAILED_ITEMS_LIMIT],
            "uploaded": int(uploaded),
            "updated": int(updated),
            "deleted": int(deleted),
            "reused": int(reused),
            "remote_sources": int(remote_source_count),
            "materialized_sources": int(materialized_sources),
            "remote_artifacts": int(remote_artifact_count),
            "downloaded_artifacts": int(downloaded_artifacts),
            "pruned_artifacts": int(pruned_artifacts),
            "last_error": (str(failed_items[0].get("message") or "") if failed_items else ""),
            "failure_signature": failure_signature,
            "last_fingerprint": fingerprint,
            "errors": errors[:_MAX_REMOTE_ERRORS],
        }
    )
    _save_state(space_root, state_doc)

    if not converged and (prev_converged or prev_failure_signature != failure_signature):
        _emit_sync_notification(
            group=group,
            by=by,
            provider=provider,
            remote_space_id=remote_space_id,
            run_id=run_id,
            converged=False,
            unsynced_count=int(unsynced_count),
            failed_count=int(failed_count),
            failed_items=failed_items,
        )
    elif converged and (not prev_converged):
        _emit_sync_notification(
            group=group,
            by=by,
            provider=provider,
            remote_space_id=remote_space_id,
            run_id=run_id,
            converged=True,
            unsynced_count=0,
            failed_count=0,
            failed_items=[],
        )

    return {
        "ok": True,
        "group_id": gid,
        "provider": provider,
        "remote_space_id": remote_space_id,
        "space_root": str(space_root),
        "skipped": False,
        "run_id": run_id,
        "state_code": ("ok" if converged else "error"),
        "converged": converged,
        "unsynced_count": int(unsynced_count),
        "failed_count": int(failed_count),
        "failed_items": failed_items[:_FAILED_ITEMS_LIMIT],
        "local_files": len(local_files),
        "uploaded": int(uploaded),
        "updated": int(updated),
        "deleted": int(deleted),
        "reused": int(reused),
        "remote_sources": int(remote_source_count),
        "materialized_sources": int(materialized_sources),
        "remote_artifacts": int(remote_artifact_count),
        "downloaded_artifacts": int(downloaded_artifacts),
        "pruned_artifacts": int(pruned_artifacts),
        "errors": errors[:_MAX_REMOTE_ERRORS],
        "state": state_doc,
    }


def process_due_space_syncs(*, provider: str = "notebooklm", limit: int = 20) -> Dict[str, Any]:
    max_items = max(1, min(int(limit or 20), 200))
    bindings = list_space_bindings(provider)
    seen = 0
    processed = 0
    skipped = 0
    converged = 0
    failed = 0
    for item in bindings:
        if seen >= max_items:
            break
        if not isinstance(item, dict):
            continue
        gid = str(item.get("group_id") or "").strip()
        remote_space_id = str(item.get("remote_space_id") or "").strip()
        status = str(item.get("status") or "").strip()
        if not gid or not remote_space_id or status != "bound":
            continue
        seen += 1
        try:
            result = sync_group_space_files(gid, provider=provider, force=False)
            if not bool(result.get("ok")):
                failed += 1
                continue
            if bool(result.get("skipped")):
                skipped += 1
                continue
            processed += 1
            if bool(result.get("converged")):
                converged += 1
            else:
                failed += 1
        except Exception:
            failed += 1
    return {
        "seen": seen,
        "processed": processed,
        "skipped": skipped,
        "converged": converged,
        "failed": failed,
    }
