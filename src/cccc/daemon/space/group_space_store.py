from __future__ import annotations

import hashlib
import json
import os
import re
import secrets
import threading
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Callable, Dict, List, Optional, Tuple

from ...contracts.v1 import (
    SpaceBinding,
    SpaceJob,
    SpaceLane,
    SpaceProviderCredentialState,
    SpaceProviderState,
    SpaceQueueSummary,
)
from ...paths import ensure_home
from ...util.fs import atomic_write_json, read_json
from ...util.time import parse_utc_iso, utc_now_iso

_PROVIDER_IDS = {"notebooklm"}
_SPACE_LANES = {"work", "memory"}
_JOB_ID_PREFIX = "spj_"
_PROVIDER_SECRET_KEY_RE = re.compile(r"^[A-Za-z_][A-Za-z0-9_]*$")
_DOC_CACHE_LOCK = threading.Lock()
_DOC_CACHE: Dict[str, Tuple[Optional[Tuple[int, int]], Dict[str, Any]]] = {}
_JOBS_SCAN_WARN_TS: float = 0.0
_JOBS_SCAN_WARN_LOCK = threading.Lock()
_JOBS_COMPACT_LOCK = threading.Lock()
_JOB_PAYLOAD_INLINE_MAX_BYTES = 0
_JOB_TERMINAL_STATES = {"succeeded", "failed", "canceled"}


def _jobs_scan_max_bytes() -> int:
    raw = str(os.environ.get("CCCC_SPACE_JOBS_SCAN_MAX_BYTES") or "").strip()
    try:
        value = int(raw) if raw else 16 * 1024 * 1024
    except Exception:
        value = 16 * 1024 * 1024
    return max(1_048_576, value)


def _jobs_scan_allowed(path: Path) -> bool:
    try:
        size = int(path.stat().st_size or 0)
    except Exception:
        return True
    limit = _jobs_scan_max_bytes()
    if size <= limit:
        return True
    try:
        with _JOBS_COMPACT_LOCK:
            if path.exists() and int(path.stat().st_size or 0) > limit:
                doc = _load_cached_doc(path, normalize=_normalize_jobs_doc)
                _compact_jobs_doc(path, doc, force=True)
        if int(path.stat().st_size or 0) <= limit:
            return True
    except Exception:
        pass

    now_ts = datetime.now(timezone.utc).timestamp()
    global _JOBS_SCAN_WARN_TS
    with _JOBS_SCAN_WARN_LOCK:
        if now_ts - float(_JOBS_SCAN_WARN_TS or 0.0) >= 30.0:
            _JOBS_SCAN_WARN_TS = now_ts
            try:
                import logging

                logging.getLogger(__name__).warning(
                    "space jobs scan skipped: jobs.json too large size=%s limit=%s path=%s",
                    size,
                    limit,
                    path,
                )
            except Exception:
                pass
    return False


def _space_root(home: Path) -> Path:
    return home / "state" / "space"


def _providers_path(home: Path) -> Path:
    return _space_root(home) / "providers.json"


def _bindings_path(home: Path) -> Path:
    return _space_root(home) / "bindings.json"


def _jobs_path(home: Path) -> Path:
    return _space_root(home) / "jobs.json"


def _history_path(home: Path) -> Path:
    return _space_root(home) / "jobs.history.jsonl"


def _job_payload_root(home: Path) -> Path:
    return _space_root(home) / "job_payloads"


def _provider_secret_root(home: Path) -> Path:
    return home / "state" / "secrets" / "space_providers"


def _provider_secret_filename(provider: str) -> str:
    pid = _provider_or_raise(provider)
    digest = hashlib.sha256(pid.encode("utf-8")).hexdigest()[:16]
    slug = re.sub(r"[^A-Za-z0-9._-]+", "_", pid).strip("._-")
    if not slug:
        slug = "provider"
    slug = slug[:32]
    return f"{slug}.{digest}.json"


def _provider_secret_path(provider: str) -> Path:
    home = ensure_home()
    root = _provider_secret_root(home)
    return root / _provider_secret_filename(provider)


def _ensure_dir(path: Path, mode: int = 0o700) -> None:
    path.mkdir(parents=True, exist_ok=True)
    try:
        os.chmod(path, mode)
    except Exception:
        pass


def _safe_id(raw: Any, *, field: str) -> str:
    value = str(raw or "").strip()
    if not value:
        raise ValueError(f"missing {field}")
    if "/" in value or "\\" in value or ".." in value:
        raise ValueError(f"invalid {field}")
    return value


def _provider_or_raise(raw: Any) -> str:
    provider = str(raw or "notebooklm").strip() or "notebooklm"
    if provider not in _PROVIDER_IDS:
        raise ValueError(f"unsupported provider: {provider}")
    return provider


def _lane_or_raise(raw: Any) -> str:
    lane = str(raw or "work").strip().lower() or "work"
    if lane not in _SPACE_LANES:
        raise ValueError(f"unsupported lane: {lane}")
    return lane


def _validate_provider_secret_key(key: Any) -> str:
    k = str(key or "").strip()
    if not k:
        raise ValueError("missing secret key")
    if not _PROVIDER_SECRET_KEY_RE.match(k):
        raise ValueError(f"invalid secret key: {k}")
    return k


def _mask_secret_value(value: Any) -> str:
    raw = str(value or "")
    if len(raw) <= 6:
        return "******"
    return f"{raw[:2]}******{raw[-2:]}"


def _path_mtime_iso(path: Path) -> Optional[str]:
    try:
        stat = path.stat()
        return datetime.fromtimestamp(float(stat.st_mtime), tz=timezone.utc).isoformat().replace("+00:00", "Z")
    except Exception:
        return None


def _save_doc(path: Path, doc: Dict[str, Any]) -> None:
    _ensure_dir(path.parent, 0o700)
    doc["updated_at"] = utc_now_iso()
    atomic_write_json(path, doc, indent=2)
    try:
        os.chmod(path, 0o600)
    except Exception:
        pass
    _update_doc_cache(path, doc)


def _doc_cache_key(path: Path) -> str:
    return str(path.resolve())


def _doc_signature(path: Path) -> Optional[Tuple[int, int]]:
    try:
        stat = path.stat()
    except Exception:
        return None
    return (int(getattr(stat, "st_mtime_ns", 0) or 0), int(stat.st_size or 0))


def _update_doc_cache(path: Path, doc: Dict[str, Any]) -> None:
    cache_key = _doc_cache_key(path)
    signature = _doc_signature(path)
    with _DOC_CACHE_LOCK:
        _DOC_CACHE[cache_key] = (signature, dict(doc))


def _load_cached_doc(path: Path, *, normalize: Callable[[Any], Dict[str, Any]]) -> Dict[str, Any]:
    cache_key = _doc_cache_key(path)
    signature = _doc_signature(path)
    with _DOC_CACHE_LOCK:
        cached = _DOC_CACHE.get(cache_key)
        if cached is not None and cached[0] == signature:
            return dict(cached[1])
    doc = normalize(read_json(path))
    with _DOC_CACHE_LOCK:
        _DOC_CACHE[cache_key] = (signature, dict(doc))
    return dict(doc)


def _new_provider_state(provider: str = "notebooklm") -> Dict[str, Any]:
    return SpaceProviderState(provider=provider).model_dump(exclude_none=True)


def _new_providers_doc() -> Dict[str, Any]:
    now = utc_now_iso()
    return {
        "v": 1,
        "created_at": now,
        "updated_at": now,
        "providers": {
            "notebooklm": _new_provider_state("notebooklm"),
        },
    }


def _normalize_providers_doc(raw: Any) -> Dict[str, Any]:
    if not isinstance(raw, dict):
        return _new_providers_doc()
    doc = dict(raw)
    now = utc_now_iso()
    if not str(doc.get("created_at") or "").strip():
        doc["created_at"] = now
    if not str(doc.get("updated_at") or "").strip():
        doc["updated_at"] = now
    providers_raw = doc.get("providers")
    providers: Dict[str, Any] = providers_raw if isinstance(providers_raw, dict) else {}
    normalized: Dict[str, Any] = {}
    for provider in sorted(_PROVIDER_IDS):
        candidate = providers.get(provider)
        try:
            model = SpaceProviderState.model_validate(
                candidate if isinstance(candidate, dict) else {"provider": provider}
            )
            normalized[provider] = model.model_dump(exclude_none=True)
        except Exception:
            normalized[provider] = _new_provider_state(provider)
    doc["providers"] = normalized
    doc["v"] = 1
    return doc


def _load_providers_doc() -> Tuple[Path, Dict[str, Any]]:
    home = ensure_home()
    path = _providers_path(home)
    return path, _load_cached_doc(path, normalize=_normalize_providers_doc)


def _new_bindings_doc() -> Dict[str, Any]:
    now = utc_now_iso()
    return {
        "v": 2,
        "created_at": now,
        "updated_at": now,
        "bindings": {},
    }


def _default_binding_doc(group_id: str, provider: str, lane: str) -> Dict[str, Any]:
    return SpaceBinding(
        group_id=group_id,
        provider=provider,
        lane=lane,
        remote_space_id="",
        bound_by="",
        status="unbound",
    ).model_dump(exclude_none=True)


def _normalize_provider_binding_lanes(group_id: str, provider: str, raw_item: Any) -> Dict[str, Dict[str, Any]]:
    out: Dict[str, Dict[str, Any]] = {}
    if not isinstance(raw_item, dict):
        return out

    if any(key in raw_item for key in ("remote_space_id", "status", "bound_at", "bound_by")):
        candidate = dict(raw_item)
        candidate["group_id"] = group_id
        candidate["provider"] = provider
        candidate["lane"] = "work"
        try:
            model = SpaceBinding.model_validate(candidate)
            out["work"] = model.model_dump(exclude_none=True)
        except Exception:
            pass
    else:
        for lane_raw, lane_item in raw_item.items():
            try:
                lane = _lane_or_raise(lane_raw)
            except Exception:
                continue
            if not isinstance(lane_item, dict):
                continue
            candidate = dict(lane_item)
            candidate["group_id"] = group_id
            candidate["provider"] = provider
            candidate["lane"] = lane
            try:
                model = SpaceBinding.model_validate(candidate)
                out[lane] = model.model_dump(exclude_none=True)
            except Exception:
                continue

    for lane in sorted(_SPACE_LANES):
        out.setdefault(lane, _default_binding_doc(group_id, provider, lane))
    return out


def _normalize_bindings_doc(raw: Any) -> Dict[str, Any]:
    if not isinstance(raw, dict):
        return _new_bindings_doc()
    doc = dict(raw)
    now = utc_now_iso()
    if not str(doc.get("created_at") or "").strip():
        doc["created_at"] = now
    if not str(doc.get("updated_at") or "").strip():
        doc["updated_at"] = now
    bindings_raw = doc.get("bindings")
    bindings = bindings_raw if isinstance(bindings_raw, dict) else {}
    normalized: Dict[str, Dict[str, Dict[str, Dict[str, Any]]]] = {}
    for group_id_raw, per_group_raw in bindings.items():
        try:
            group_id = _safe_id(group_id_raw, field="group_id")
        except Exception:
            continue
        if not isinstance(per_group_raw, dict):
            continue
        per_group: Dict[str, Dict[str, Dict[str, Any]]] = {}
        for provider_raw, item_raw in per_group_raw.items():
            try:
                provider = _provider_or_raise(provider_raw)
            except Exception:
                continue
            lanes = _normalize_provider_binding_lanes(group_id, provider, item_raw)
            if lanes:
                per_group[provider] = lanes
        if per_group:
            normalized[group_id] = per_group
    doc["bindings"] = normalized
    doc["v"] = 2
    return doc


def _load_bindings_doc() -> Tuple[Path, Dict[str, Any]]:
    home = ensure_home()
    path = _bindings_path(home)
    return path, _load_cached_doc(path, normalize=_normalize_bindings_doc)


def _new_jobs_doc() -> Dict[str, Any]:
    now = utc_now_iso()
    return {
        "v": 2,
        "created_at": now,
        "updated_at": now,
        "jobs": {},
    }


def _normalize_jobs_doc(raw: Any) -> Dict[str, Any]:
    if not isinstance(raw, dict):
        return _new_jobs_doc()
    doc = dict(raw)
    now = utc_now_iso()
    if not str(doc.get("created_at") or "").strip():
        doc["created_at"] = now
    if not str(doc.get("updated_at") or "").strip():
        doc["updated_at"] = now
    jobs_raw = doc.get("jobs")
    jobs = jobs_raw if isinstance(jobs_raw, dict) else {}
    normalized: Dict[str, Dict[str, Any]] = {}
    for job_id_raw, item_raw in jobs.items():
        if not isinstance(item_raw, dict):
            continue
        try:
            job_id = _safe_id(job_id_raw, field="job_id")
        except Exception:
            continue
        candidate = dict(item_raw)
        candidate["job_id"] = job_id
        candidate["lane"] = _lane_or_raise(candidate.get("lane") or "work")
        try:
            model = SpaceJob.model_validate(candidate)
        except Exception:
            continue
        normalized[job_id] = model.model_dump(exclude_none=True)
    doc["jobs"] = normalized
    doc["v"] = 2
    return doc


def _jobs_retention_limit() -> int:
    raw = str(os.environ.get("CCCC_SPACE_JOBS_TERMINAL_KEEP") or "").strip()
    try:
        value = int(raw) if raw else 50
    except Exception:
        value = 50
    return max(1, min(value, 5000))


def _jobs_retention_days() -> int:
    raw = str(os.environ.get("CCCC_SPACE_JOBS_TERMINAL_MAX_AGE_DAYS") or "").strip()
    try:
        value = int(raw) if raw else 3
    except Exception:
        value = 3
    return max(1, min(value, 365))


def _running_job_stale_seconds() -> int:
    raw = str(os.environ.get("CCCC_SPACE_RUNNING_JOB_STALE_SECONDS") or "").strip()
    try:
        value = int(raw) if raw else 6 * 3600
    except Exception:
        value = 6 * 3600
    return max(600, min(value, 30 * 86400))


def _running_job_is_stale(item: Dict[str, Any], *, now_ts: Optional[float] = None) -> bool:
    if str(item.get("state") or "").strip().lower() != "running":
        return False
    updated_ts = _job_updated_ts(item)
    if updated_ts <= 0:
        return True
    current_ts = datetime.now(timezone.utc).timestamp() if now_ts is None else float(now_ts)
    return (current_ts - updated_ts) >= float(_running_job_stale_seconds())


def _jobs_doc_needs_compaction(doc: Dict[str, Any]) -> bool:
    jobs = doc.get("jobs") if isinstance(doc.get("jobs"), dict) else {}
    terminal_count = 0
    keep_limit = _jobs_retention_limit()
    cutoff_ts = datetime.now(timezone.utc).timestamp() - (_jobs_retention_days() * 86400)
    now_ts = datetime.now(timezone.utc).timestamp()
    for item in jobs.values():
        if not isinstance(item, dict):
            continue
        payload = item.get("payload") if isinstance(item.get("payload"), dict) else {}
        if payload:
            return True
        state = str(item.get("state") or "").strip().lower()
        if state == "running" and _running_job_is_stale(item, now_ts=now_ts):
            return True
        if state in _JOB_TERMINAL_STATES:
            terminal_count += 1
            if terminal_count > keep_limit or _job_updated_ts(item) < cutoff_ts:
                return True
    return False


def _compact_jobs_doc(path: Path, doc: Dict[str, Any], *, force: bool = False) -> Dict[str, Any]:
    jobs = doc.get("jobs") if isinstance(doc.get("jobs"), dict) else {}
    if not jobs:
        return doc
    if not force and not _jobs_doc_needs_compaction(doc):
        return doc

    home = ensure_home()
    keep_limit = _jobs_retention_limit()
    cutoff_ts = datetime.now(timezone.utc).timestamp() - (_jobs_retention_days() * 86400)
    now_iso = utc_now_iso()
    now_dt = parse_utc_iso(now_iso)
    now_ts = now_dt.timestamp() if now_dt is not None else datetime.now(timezone.utc).timestamp()
    terminal_items: List[Tuple[str, Dict[str, Any]]] = []
    active_items: List[Tuple[str, Dict[str, Any]]] = []
    for job_id, item in jobs.items():
        if not isinstance(item, dict):
            continue
        if _running_job_is_stale(item, now_ts=now_ts):
            stale = dict(item)
            stale["state"] = "failed"
            stale["next_run_at"] = None
            stale["updated_at"] = now_iso
            stale["last_error"] = {
                "code": "space_job_stale_running",
                "message": "running job was left active by an earlier daemon process",
            }
            terminal_items.append((job_id, stale))
            continue
        state = str(item.get("state") or "").strip().lower()
        if state in _JOB_TERMINAL_STATES:
            terminal_items.append((job_id, item))
        else:
            active_items.append((job_id, item))

    terminal_items.sort(key=lambda pair: _job_updated_ts(pair[1]), reverse=True)
    kept_jobs: Dict[str, Dict[str, Any]] = {}
    dropped: List[Tuple[str, Dict[str, Any]]] = []
    for job_id, item in active_items:
        kept_jobs[job_id] = _stored_job_doc(home, SpaceJob.model_validate(item).model_dump(exclude_none=True))
    for index, (job_id, item) in enumerate(terminal_items):
        normalized = SpaceJob.model_validate(item).model_dump(exclude_none=True)
        keep = index < keep_limit or _job_updated_ts(normalized) >= cutoff_ts
        if keep:
            kept_jobs[job_id] = _stored_job_doc(home, normalized)
        else:
            dropped.append((job_id, normalized))

    changed = len(kept_jobs) != len(jobs)
    if not changed:
        for job_id, kept in kept_jobs.items():
            current = jobs.get(job_id) if isinstance(jobs.get(job_id), dict) else {}
            if kept != current:
                changed = True
                break
    if not changed:
        return doc

    for _, item in dropped:
        _delete_payload_blob(home, str(item.get("payload_ref") or ""))
    next_doc = dict(doc)
    next_doc["jobs"] = kept_jobs
    _save_doc(path, next_doc)
    return next_doc


def _load_jobs_doc() -> Tuple[Path, Dict[str, Any]]:
    home = ensure_home()
    path = _jobs_path(home)
    doc = _load_cached_doc(path, normalize=_normalize_jobs_doc)
    if _jobs_doc_needs_compaction(doc):
        with _JOBS_COMPACT_LOCK:
            doc = _load_cached_doc(path, normalize=_normalize_jobs_doc)
            doc = _compact_jobs_doc(path, doc)
    return path, doc


def _append_history(job_id: str, event: str, detail: Optional[Dict[str, Any]] = None) -> None:
    home = ensure_home()
    path = _history_path(home)
    _ensure_dir(path.parent, 0o700)
    rec = {
        "ts": utc_now_iso(),
        "job_id": str(job_id or ""),
        "event": str(event or ""),
        "detail": detail if isinstance(detail, dict) else {},
    }
    try:
        with path.open("a", encoding="utf-8") as f:
            f.write(json.dumps(rec, ensure_ascii=False) + "\n")
        os.chmod(path, 0o600)
    except Exception:
        pass


def get_space_provider_state(provider: str = "notebooklm") -> Dict[str, Any]:
    pid = _provider_or_raise(provider)
    _, doc = _load_providers_doc()
    providers = doc.get("providers") if isinstance(doc.get("providers"), dict) else {}
    item = providers.get(pid)
    if isinstance(item, dict):
        return dict(item)
    return _new_provider_state(pid)


def set_space_provider_state(
    provider: str = "notebooklm",
    *,
    enabled: Optional[bool] = None,
    real_enabled: Optional[bool] = None,
    mode: Optional[str] = None,
    last_error: Optional[str] = None,
    touch_health: bool = False,
) -> Dict[str, Any]:
    pid = _provider_or_raise(provider)
    path, doc = _load_providers_doc()
    providers = doc.get("providers") if isinstance(doc.get("providers"), dict) else {}
    current_raw = providers.get(pid) if isinstance(providers, dict) else None
    current = (
        dict(current_raw)
        if isinstance(current_raw, dict)
        else _new_provider_state(pid)
    )
    if enabled is not None:
        current["enabled"] = bool(enabled)
    if real_enabled is not None:
        current["real_enabled"] = bool(real_enabled)
    if mode is not None:
        current["mode"] = str(mode)
    if last_error is not None:
        current["last_error"] = str(last_error or "") or None
    if touch_health:
        current["last_health_at"] = utc_now_iso()
    model = SpaceProviderState.model_validate(current)
    item = model.model_dump(exclude_none=True)
    providers[pid] = item
    doc["providers"] = providers
    _save_doc(path, doc)
    return dict(item)


def load_space_provider_secrets(provider: str = "notebooklm") -> Dict[str, str]:
    pid = _provider_or_raise(provider)
    path = _provider_secret_path(pid)
    raw = read_json(path)
    out: Dict[str, str] = {}
    if not isinstance(raw, dict):
        return out
    for key, value in raw.items():
        if value is None:
            continue
        try:
            kk = _validate_provider_secret_key(key)
        except Exception:
            continue
        out[kk] = str(value)
    return out


def update_space_provider_secrets(
    provider: str = "notebooklm",
    *,
    set_vars: Dict[str, str],
    unset_keys: List[str],
    clear: bool,
) -> Dict[str, str]:
    pid = _provider_or_raise(provider)
    current = {} if clear else load_space_provider_secrets(pid)
    for key in unset_keys:
        kk = _validate_provider_secret_key(key)
        current.pop(kk, None)
    for key, value in set_vars.items():
        kk = _validate_provider_secret_key(key)
        current[kk] = str(value)

    path = _provider_secret_path(pid)
    root = path.parent
    if not current:
        try:
            path.unlink(missing_ok=True)
        except Exception:
            pass
        return {}

    _ensure_dir(root, 0o700)
    atomic_write_json(path, current, indent=2)
    try:
        os.chmod(path, 0o600)
    except Exception:
        pass
    return dict(current)


def describe_space_provider_credential_state(
    provider: str = "notebooklm",
    *,
    key: str,
) -> Dict[str, Any]:
    pid = _provider_or_raise(provider)
    secret_key = _validate_provider_secret_key(key)
    secrets_map = load_space_provider_secrets(pid)
    value = str(secrets_map.get(secret_key) or "")
    configured = bool(value)
    path = _provider_secret_path(pid)
    model = SpaceProviderCredentialState(
        provider=pid,
        key=secret_key,
        configured=configured,
        source=("store" if configured else "none"),
        env_configured=False,
        store_configured=configured,
        updated_at=_path_mtime_iso(path),
        masked_value=(_mask_secret_value(value) if configured else None),
    )
    return model.model_dump(exclude_none=True)


def get_space_bindings(group_id: str, provider: str = "notebooklm") -> Dict[str, Dict[str, Any]]:
    gid = _safe_id(group_id, field="group_id")
    pid = _provider_or_raise(provider)
    _, doc = _load_bindings_doc()
    bindings = doc.get("bindings") if isinstance(doc.get("bindings"), dict) else {}
    per_group = bindings.get(gid) if isinstance(bindings.get(gid), dict) else {}
    per_provider = per_group.get(pid) if isinstance(per_group, dict) else {}
    out: Dict[str, Dict[str, Any]] = {}
    for lane in sorted(_SPACE_LANES):
        item = per_provider.get(lane) if isinstance(per_provider, dict) else None
        if isinstance(item, dict):
            try:
                out[lane] = SpaceBinding.model_validate(item).model_dump(exclude_none=True)
                continue
            except Exception:
                pass
        out[lane] = _default_binding_doc(gid, pid, lane)
    return out


def get_space_binding(group_id: str, provider: str = "notebooklm", lane: str = "work") -> Optional[Dict[str, Any]]:
    gid = _safe_id(group_id, field="group_id")
    pid = _provider_or_raise(provider)
    lid = _lane_or_raise(lane)
    _, doc = _load_bindings_doc()
    bindings = doc.get("bindings") if isinstance(doc.get("bindings"), dict) else {}
    per_group = bindings.get(gid) if isinstance(bindings.get(gid), dict) else {}
    per_provider = per_group.get(pid) if isinstance(per_group, dict) else {}
    item = per_provider.get(lid) if isinstance(per_provider, dict) else None
    if not isinstance(item, dict):
        return None
    try:
        return SpaceBinding.model_validate(item).model_dump(exclude_none=True)
    except Exception:
        return None


def list_space_bindings(provider: str = "notebooklm", *, lane: str = "") -> List[Dict[str, Any]]:
    pid = _provider_or_raise(provider)
    lane_filter = _lane_or_raise(lane) if str(lane or "").strip() else ""
    _, doc = _load_bindings_doc()
    bindings = doc.get("bindings") if isinstance(doc.get("bindings"), dict) else {}
    out: List[Dict[str, Any]] = []
    for group_id in sorted(bindings.keys()):
        per_group = bindings.get(group_id)
        if not isinstance(per_group, dict):
            continue
        per_provider = per_group.get(pid)
        if not isinstance(per_provider, dict):
            continue
        for lane_name, item in per_provider.items():
            if lane_filter and lane_name != lane_filter:
                continue
            if not isinstance(item, dict):
                continue
            try:
                model = SpaceBinding.model_validate(item)
                out.append(model.model_dump(exclude_none=True))
            except Exception:
                continue
    return out


def upsert_space_binding(
    group_id: str,
    *,
    provider: str = "notebooklm",
    lane: str = "work",
    remote_space_id: str,
    by: str,
    status: str = "bound",
) -> Dict[str, Any]:
    gid = _safe_id(group_id, field="group_id")
    pid = _provider_or_raise(provider)
    lid = _lane_or_raise(lane)
    rid = str(remote_space_id or "").strip()
    who = str(by or "user").strip() or "user"
    payload = {
        "group_id": gid,
        "provider": pid,
        "lane": lid,
        "remote_space_id": rid,
        "bound_by": who,
        "bound_at": utc_now_iso(),
        "status": str(status or "bound"),
    }
    model = SpaceBinding.model_validate(payload)
    out = model.model_dump(exclude_none=True)

    path, doc = _load_bindings_doc()
    bindings = doc.get("bindings") if isinstance(doc.get("bindings"), dict) else {}
    per_group = bindings.get(gid) if isinstance(bindings.get(gid), dict) else {}
    per_provider = per_group.get(pid) if isinstance(per_group.get(pid), dict) else {}
    per_provider[lid] = out
    for other in sorted(_SPACE_LANES):
        per_provider.setdefault(other, _default_binding_doc(gid, pid, other))
    per_group[pid] = per_provider
    bindings[gid] = per_group
    doc["bindings"] = bindings
    _save_doc(path, doc)
    return dict(out)


def set_space_binding_unbound(
    group_id: str,
    *,
    provider: str = "notebooklm",
    lane: str = "work",
    by: str,
) -> Dict[str, Any]:
    return upsert_space_binding(
        group_id,
        provider=provider,
        lane=lane,
        remote_space_id="",
        by=by,
        status="unbound",
    )


def _new_job_id() -> str:
    return f"{_JOB_ID_PREFIX}{secrets.token_hex(8)}"


def _payload_digest(payload: Dict[str, Any]) -> str:
    stable = json.dumps(payload, ensure_ascii=False, sort_keys=True, separators=(",", ":"))
    return "sha256:" + hashlib.sha256(stable.encode("utf-8")).hexdigest()


def _payload_digest_slug(payload_digest: str) -> str:
    value = str(payload_digest or "").strip()
    if value.startswith("sha256:"):
        value = value.split(":", 1)[1]
    value = re.sub(r"[^a-fA-F0-9]+", "", value).lower()
    return (value or "payload")[:32]


def _job_payload_ref(job_id: str, payload_digest: str) -> str:
    return f"{job_id}.{_payload_digest_slug(payload_digest)}.json"


def _job_payload_path(home: Path, payload_ref: str) -> Path:
    name = str(payload_ref or "").strip()
    if not name:
        raise ValueError("missing payload_ref")
    if "/" in name or "\\" in name or ".." in name:
        raise ValueError("invalid payload_ref")
    return _job_payload_root(home) / name


def _payload_json_bytes(payload: Dict[str, Any]) -> int:
    return len(json.dumps(payload, ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode("utf-8"))


def _read_payload_blob(home: Path, payload_ref: str) -> Dict[str, Any]:
    try:
        raw = read_json(_job_payload_path(home, payload_ref))
    except Exception:
        return {}
    return dict(raw) if isinstance(raw, dict) else {}


def _write_payload_blob(home: Path, job_id: str, payload: Dict[str, Any], payload_digest: str) -> Tuple[str, int]:
    ref = _job_payload_ref(job_id, payload_digest)
    path = _job_payload_path(home, ref)
    _ensure_dir(path.parent, 0o700)
    atomic_write_json(path, payload, indent=2)
    try:
        os.chmod(path, 0o600)
    except Exception:
        pass
    return ref, _payload_json_bytes(payload)


def _delete_payload_blob(home: Path, payload_ref: str) -> None:
    if not str(payload_ref or "").strip():
        return
    try:
        _job_payload_path(home, payload_ref).unlink(missing_ok=True)
    except Exception:
        pass


def _stored_job_doc(home: Path, item: Dict[str, Any]) -> Dict[str, Any]:
    stored = dict(item)
    payload = stored.get("payload") if isinstance(stored.get("payload"), dict) else {}
    payload_digest = str(stored.get("payload_digest") or "").strip()
    payload_ref = str(stored.get("payload_ref") or "").strip()
    if payload and not payload_digest:
        payload_digest = _payload_digest(payload)
        stored["payload_digest"] = payload_digest
    if payload and _payload_json_bytes(payload) > _JOB_PAYLOAD_INLINE_MAX_BYTES:
        payload_ref, payload_bytes = _write_payload_blob(
            home,
            str(stored.get("job_id") or "").strip(),
            payload,
            payload_digest,
        )
        stored["payload_ref"] = payload_ref
        stored["payload_bytes"] = payload_bytes
        stored["payload"] = {}
        return stored
    if payload_ref and not payload:
        stored["payload_bytes"] = int(stored.get("payload_bytes") or 0)
        return stored
    stored["payload_ref"] = ""
    stored["payload_bytes"] = _payload_json_bytes(payload) if payload else int(stored.get("payload_bytes") or 0)
    return stored


def _hydrated_job_doc(home: Path, item: Dict[str, Any]) -> Dict[str, Any]:
    out = dict(item)
    payload = out.get("payload") if isinstance(out.get("payload"), dict) else {}
    payload_ref = str(out.get("payload_ref") or "").strip()
    if not payload and payload_ref:
        out["payload"] = _read_payload_blob(home, payload_ref)
    if not payload_ref and not isinstance(out.get("payload"), dict):
        out["payload"] = {}
    return out


def _job_updated_ts(item: Dict[str, Any]) -> float:
    parsed = parse_utc_iso(str(item.get("updated_at") or item.get("created_at") or "").strip())
    if parsed is None:
        return 0.0
    return parsed.timestamp()


def _default_idempotency_key(
    *,
    provider: str,
    lane: str = "work",
    remote_space_id: str,
    kind: str,
    payload_digest: str,
) -> str:
    return f"{provider}:{lane}:{remote_space_id}:{kind}:{payload_digest}"


def _normalize_idempotency_key(value: Any) -> str:
    key = str(value or "").strip()
    return key[:256]


def enqueue_space_job(
    *,
    group_id: str,
    provider: str,
    lane: str = "work",
    remote_space_id: str,
    kind: str,
    payload: Dict[str, Any],
    idempotency_key: str = "",
    max_attempts: int = 3,
) -> Tuple[Dict[str, Any], bool]:
    home = ensure_home()
    gid = _safe_id(group_id, field="group_id")
    pid = _provider_or_raise(provider)
    lid = _lane_or_raise(lane)
    rid = str(remote_space_id or "").strip()
    if not rid:
        raise ValueError("missing remote_space_id")
    if not isinstance(payload, dict):
        raise ValueError("payload must be an object")
    digest = _payload_digest(payload)
    idem = _normalize_idempotency_key(idempotency_key) or _default_idempotency_key(
        provider=pid,
        lane=lid,
        remote_space_id=rid,
        kind=str(kind or "context_sync"),
        payload_digest=digest,
    )
    path, doc = _load_jobs_doc()
    jobs = doc.get("jobs") if isinstance(doc.get("jobs"), dict) else {}

    for item in jobs.values():
        if not isinstance(item, dict):
            continue
        try:
            model = SpaceJob.model_validate(item)
        except Exception:
            continue
        if model.group_id != gid:
            continue
        if model.provider != pid:
            continue
        if model.lane != lid:
            continue
        if model.remote_space_id != rid:
            continue
        if str(model.idempotency_key or "") != idem:
            continue
        if model.state in ("pending", "running", "succeeded"):
            return _hydrated_job_doc(home, model.model_dump(exclude_none=True)), True

    now = utc_now_iso()
    job = SpaceJob(
        job_id=_new_job_id(),
        group_id=gid,
        provider=pid,
        lane=lid,
        remote_space_id=rid,
        kind=str(kind or "context_sync"),
        payload=dict(payload),
        payload_digest=digest,
        idempotency_key=idem,
        state="pending",
        attempt=0,
        max_attempts=max(1, int(max_attempts or 3)),
        next_run_at=None,
        created_at=now,
        updated_at=now,
    )
    item = _stored_job_doc(home, job.model_dump(exclude_none=True))
    jobs[item["job_id"]] = item
    doc["jobs"] = jobs
    _save_doc(path, doc)
    _append_history(item["job_id"], "created", {"kind": item.get("kind"), "lane": item.get("lane")})
    return _hydrated_job_doc(home, item), False


def get_space_job(job_id: str) -> Optional[Dict[str, Any]]:
    home = ensure_home()
    jid = _safe_id(job_id, field="job_id")
    _, doc = _load_jobs_doc()
    jobs = doc.get("jobs") if isinstance(doc.get("jobs"), dict) else {}
    item = jobs.get(jid)
    if not isinstance(item, dict):
        return None
    try:
        return _hydrated_job_doc(home, SpaceJob.model_validate(item).model_dump(exclude_none=True))
    except Exception:
        return None


def _update_space_job(job_id: str, mutator: Callable[[Dict[str, Any]], Dict[str, Any]]) -> Dict[str, Any]:
    home = ensure_home()
    jid = _safe_id(job_id, field="job_id")
    path, doc = _load_jobs_doc()
    jobs = doc.get("jobs") if isinstance(doc.get("jobs"), dict) else {}
    item = jobs.get(jid)
    if not isinstance(item, dict):
        raise ValueError(f"job not found: {jid}")
    current = _hydrated_job_doc(home, SpaceJob.model_validate(item).model_dump(exclude_none=True))
    candidate = mutator(dict(current))
    model = SpaceJob.model_validate(candidate)
    out = _stored_job_doc(home, model.model_dump(exclude_none=True))
    jobs[jid] = out
    doc["jobs"] = jobs
    _save_doc(path, doc)
    return _hydrated_job_doc(home, out)


def mark_space_job_running(job_id: str) -> Dict[str, Any]:
    out = _update_space_job(
        job_id,
        lambda item: {
            **item,
            "state": "running",
            "attempt": int(item.get("attempt") or 0) + 1,
            "next_run_at": None,
            "updated_at": utc_now_iso(),
        },
    )
    _append_history(str(out.get("job_id") or ""), "started", {"attempt": int(out.get("attempt") or 0)})
    return out


def mark_space_job_retry_scheduled(
    job_id: str,
    *,
    code: str,
    message: str,
    next_run_at: str,
) -> Dict[str, Any]:
    out = _update_space_job(
        job_id,
        lambda item: {
            **item,
            "state": "pending",
            "next_run_at": str(next_run_at or "").strip() or None,
            "updated_at": utc_now_iso(),
            "last_error": {"code": str(code or ""), "message": str(message or "")},
        },
    )
    _append_history(
        str(out.get("job_id") or ""),
        "retry_scheduled",
        {"attempt": int(out.get("attempt") or 0), "next_run_at": str(out.get("next_run_at") or "")},
    )
    return out


def mark_space_job_succeeded(job_id: str, *, result: Optional[Dict[str, Any]] = None) -> Dict[str, Any]:
    out = _update_space_job(
        job_id,
        lambda item: {
            **item,
            "state": "succeeded",
            "next_run_at": None,
            "updated_at": utc_now_iso(),
            "last_error": {"code": "", "message": ""},
            "result": dict(result) if isinstance(result, dict) else dict(item.get("result") or {}),
        },
    )
    _append_history(str(out.get("job_id") or ""), "succeeded", {"attempt": int(out.get("attempt") or 0)})
    return out


def mark_space_job_failed(job_id: str, *, code: str, message: str) -> Dict[str, Any]:
    out = _update_space_job(
        job_id,
        lambda item: {
            **item,
            "state": "failed",
            "next_run_at": None,
            "updated_at": utc_now_iso(),
            "last_error": {"code": str(code or ""), "message": str(message or "")},
        },
    )
    _append_history(
        str(out.get("job_id") or ""),
        "failed",
        {"attempt": int(out.get("attempt") or 0), "code": str(code or "")},
    )
    return out


def cancel_space_job(job_id: str) -> Dict[str, Any]:
    def _mutate(item: Dict[str, Any]) -> Dict[str, Any]:
        state = str(item.get("state") or "")
        if state not in ("pending", "running"):
            raise ValueError(f"cannot cancel job in state={state}")
        item["state"] = "canceled"
        item["updated_at"] = utc_now_iso()
        item["next_run_at"] = None
        return item

    out = _update_space_job(job_id, _mutate)
    _append_history(str(out.get("job_id") or ""), "canceled", {})
    return out


def reset_space_job_for_retry(job_id: str) -> Dict[str, Any]:
    def _mutate(item: Dict[str, Any]) -> Dict[str, Any]:
        state = str(item.get("state") or "")
        if state not in ("failed", "canceled"):
            raise ValueError(f"cannot retry job in state={state}")
        item["state"] = "pending"
        item["attempt"] = 0
        item["next_run_at"] = None
        item["updated_at"] = utc_now_iso()
        item["last_error"] = {"code": "", "message": ""}
        return item

    out = _update_space_job(job_id, _mutate)
    _append_history(str(out.get("job_id") or ""), "retry_scheduled", {"attempt": 0})
    return out


def list_space_jobs(
    *,
    group_id: str,
    provider: str = "notebooklm",
    lane: str = "",
    state: str = "",
    remote_space_id: str = "",
    limit: int = 50,
) -> List[Dict[str, Any]]:
    home = ensure_home()
    gid = _safe_id(group_id, field="group_id")
    pid = _provider_or_raise(provider)
    lane_filter = _lane_or_raise(lane) if str(lane or "").strip() else ""
    wanted_state = str(state or "").strip()
    wanted_remote = str(remote_space_id or "").strip()
    max_items = max(1, min(int(limit or 50), 500))
    if not _jobs_scan_allowed(_jobs_path(home)):
        return []
    _, doc = _load_jobs_doc()
    jobs = doc.get("jobs") if isinstance(doc.get("jobs"), dict) else {}
    out: List[Dict[str, Any]] = []
    for item in jobs.values():
        if not isinstance(item, dict):
            continue
        try:
            model = SpaceJob.model_validate(item)
        except Exception:
            continue
        if model.group_id != gid:
            continue
        if model.provider != pid:
            continue
        if lane_filter and model.lane != lane_filter:
            continue
        if wanted_state and model.state != wanted_state:
            continue
        if wanted_remote and model.remote_space_id != wanted_remote:
            continue
        out.append(_hydrated_job_doc(home, model.model_dump(exclude_none=True)))
    out.sort(key=lambda it: str(it.get("updated_at") or ""), reverse=True)
    return out[:max_items]


def list_due_space_jobs(*, limit: int = 50) -> List[Dict[str, Any]]:
    max_items = max(1, min(int(limit or 50), 500))
    now_dt = parse_utc_iso(utc_now_iso())
    if now_dt is None:
        return []
    if not _jobs_scan_allowed(_jobs_path(ensure_home())):
        return []
    _, doc = _load_jobs_doc()
    jobs = doc.get("jobs") if isinstance(doc.get("jobs"), dict) else {}
    out: List[Dict[str, Any]] = []
    for item in jobs.values():
        if not isinstance(item, dict):
            continue
        try:
            model = SpaceJob.model_validate(item)
        except Exception:
            continue
        if model.state != "pending":
            continue
        due_at = parse_utc_iso(str(model.next_run_at or "").strip())
        if due_at is not None and due_at > now_dt:
            continue
        out.append(model.model_dump(exclude_none=True))
    out.sort(
        key=lambda it: (
            str(it.get("next_run_at") or ""),
            str(it.get("created_at") or ""),
            str(it.get("job_id") or ""),
        )
    )
    return out[:max_items]


def space_queue_summary(
    *,
    group_id: str,
    provider: str = "notebooklm",
    lane: str = "",
    remote_space_id: str = "",
) -> Dict[str, Any]:
    gid = _safe_id(group_id, field="group_id")
    pid = _provider_or_raise(provider)
    lane_filter = _lane_or_raise(lane) if str(lane or "").strip() else ""
    wanted_remote = str(remote_space_id or "").strip()
    if not _jobs_scan_allowed(_jobs_path(ensure_home())):
        summary = SpaceQueueSummary(pending=0, running=0, failed=0)
        return summary.model_dump(exclude_none=True)
    _, doc = _load_jobs_doc()
    jobs = doc.get("jobs") if isinstance(doc.get("jobs"), dict) else {}
    pending = 0
    running = 0
    failed = 0
    for item in jobs.values():
        if not isinstance(item, dict):
            continue
        try:
            model = SpaceJob.model_validate(item)
        except Exception:
            continue
        if model.group_id != gid or model.provider != pid:
            continue
        if lane_filter and model.lane != lane_filter:
            continue
        if wanted_remote and model.remote_space_id != wanted_remote:
            continue
        if model.state == "pending":
            pending += 1
        elif model.state == "running":
            running += 1
        elif model.state == "failed":
            failed += 1
    summary = SpaceQueueSummary(pending=pending, running=running, failed=failed)
    return summary.model_dump(exclude_none=True)


def compact_space_jobs_storage() -> Dict[str, Any]:
    home = ensure_home()
    path = _jobs_path(home)
    with _JOBS_COMPACT_LOCK:
        doc = _load_cached_doc(path, normalize=_normalize_jobs_doc)
        before_jobs = doc.get("jobs") if isinstance(doc.get("jobs"), dict) else {}
        before_count = len(before_jobs)
        before_size = int(path.stat().st_size or 0) if path.exists() else 0
        next_doc = _compact_jobs_doc(path, doc, force=True)
        after_jobs = next_doc.get("jobs") if isinstance(next_doc.get("jobs"), dict) else {}
        after_size = int(path.stat().st_size or 0) if path.exists() else 0
    return {
        "before_jobs": before_count,
        "after_jobs": len(after_jobs),
        "dropped_jobs": max(0, before_count - len(after_jobs)),
        "before_bytes": before_size,
        "after_bytes": after_size,
    }


def space_queue_summaries(*, group_id: str, provider: str = "notebooklm") -> Dict[str, Dict[str, Any]]:
    gid = _safe_id(group_id, field="group_id")
    pid = _provider_or_raise(provider)
    return {lane: space_queue_summary(group_id=gid, provider=pid, lane=lane) for lane in sorted(_SPACE_LANES)}
