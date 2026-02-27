"""Group Space (external memory control-plane) operation handlers for daemon."""

from __future__ import annotations

from collections import deque
from dataclasses import dataclass, field
from datetime import datetime, timezone
import logging
import os
from pathlib import Path
import re
import threading
from typing import Any, Deque, Dict, Optional, Tuple

from ...contracts.v1 import DaemonError, DaemonResponse, SpaceBinding
from ...kernel.group import load_group
from ...kernel.ledger import append_event
from ...kernel.permissions import require_group_permission
from .group_space_paths import resolve_space_root_from_group
from .group_space_provider import (
    SpaceProviderError,
    provider_delete_source,
    provider_create_space,
    provider_download_artifact,
    provider_generate_artifact,
    provider_list_artifacts,
    provider_list_sources,
    provider_list_spaces,
    provider_refresh_source,
    provider_rename_source,
    provider_wait_artifact,
)
from ...providers.notebooklm.errors import NotebookLMProviderError
from ...providers.notebooklm.health import notebooklm_health_check, parse_notebooklm_auth_json
from .notebooklm_auth_flow import (
    cancel_notebooklm_auth_flow,
    get_notebooklm_auth_flow_status,
    start_notebooklm_auth_flow,
)
from .group_space_sync import group_space_local_file_policy, read_group_space_sync_state, sync_group_space_files
from .group_space_projection import sync_group_space_projection
from .group_space_runtime import acquire_space_provider_write, execute_space_job, retry_space_job, run_space_query
from .group_space_store import (
    cancel_space_job,
    describe_space_provider_credential_state,
    enqueue_space_job,
    get_space_binding,
    get_space_job,
    get_space_provider_state,
    list_space_bindings,
    list_space_jobs,
    load_space_provider_secrets,
    set_space_binding_unbound,
    set_space_provider_state,
    space_queue_summary,
    update_space_provider_secrets,
    upsert_space_binding,
)

_SPACE_PROVIDER_IDS = {"notebooklm"}
_SPACE_JOB_KINDS = {"context_sync", "resource_ingest"}
_SPACE_JOB_STATES = {"pending", "running", "succeeded", "failed", "canceled"}
_SPACE_JOB_ACTIONS = {"list", "retry", "cancel"}
_SPACE_SYNC_ACTIONS = {"status", "run"}
_SPACE_SOURCE_ACTIONS = {"list", "delete", "rename", "refresh"}
_SPACE_ARTIFACT_ACTIONS = {"list", "generate", "download"}
_SPACE_ARTIFACT_KINDS = {
    "audio",
    "video",
    "report",
    "study_guide",
    "quiz",
    "flashcards",
    "infographic",
    "slide_deck",
    "data_table",
    "mind_map",
}
_SPACE_ARTIFACT_KIND_ALIASES = {
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
_SPACE_PROVIDER_AUTH_ACTIONS = {"status", "start", "cancel"}
_SPACE_PROVIDER_SECRET_KEYS = {"notebooklm": "NOTEBOOKLM_AUTH_JSON"}
_SPACE_RESOURCE_INGEST_TYPES = {
    "file",
    "web_page",
    "youtube",
    "pasted_text",
    "google_docs",
    "google_slides",
    "google_spreadsheet",
}
_SPACE_QUERY_OPTION_KEYS = {"source_ids"}
_LOG = logging.getLogger("cccc.daemon.group_space_ops")
_QUERY_ACTIVE_BY_LANE: Dict[str, int] = {}
_QUERY_ACTIVE_LOCK = threading.Lock()
_GENERATE_LANES: Dict[str, "_GenerateLaneState"] = {}
_GENERATE_LANES_LOCK = threading.Lock()
_DEFAULT_QUERY_RETRY_AFTER_SECONDS = 2
_DEFAULT_GENERATE_RETRY_AFTER_SECONDS = 5
_DEFAULT_ASYNC_GENERATE_WAIT_TIMEOUT_SECONDS = 7200.0


@dataclass
class _GenerateRequest:
    lane_key: str
    group_id: str
    provider: str
    remote_space_id: str
    kind: str
    options: Dict[str, Any]
    save_to_space: bool
    output_path: str
    output_format: str
    artifact_id: str
    by: str
    timeout_seconds: float
    initial_interval: float
    max_interval: float


@dataclass
class _GenerateLaneState:
    active: int = 0
    pending: Deque[_GenerateRequest] = field(default_factory=deque)


def _error(code: str, message: str, *, details: Optional[Dict[str, Any]] = None) -> DaemonResponse:
    return DaemonResponse(ok=False, error=DaemonError(code=code, message=message, details=(details or {})))


def _provider_or_error(raw: Any) -> str:
    provider = str(raw or "notebooklm").strip() or "notebooklm"
    if provider not in _SPACE_PROVIDER_IDS:
        raise ValueError(f"unsupported provider: {provider}")
    return provider


def _kind_or_error(raw: Any) -> str:
    kind = str(raw or "context_sync").strip() or "context_sync"
    if kind not in _SPACE_JOB_KINDS:
        raise ValueError(f"invalid kind: {kind}")
    return kind


def _action_or_error(raw: Any) -> str:
    action = str(raw or "list").strip() or "list"
    if action not in _SPACE_JOB_ACTIONS:
        raise ValueError(f"invalid action: {action}")
    return action


def _sync_action_or_error(raw: Any) -> str:
    action = str(raw or "status").strip() or "status"
    if action not in _SPACE_SYNC_ACTIONS:
        raise ValueError(f"invalid action: {action}")
    return action


def _source_action_or_error(raw: Any) -> str:
    action = str(raw or "list").strip() or "list"
    if action not in _SPACE_SOURCE_ACTIONS:
        raise ValueError(f"invalid action: {action}")
    return action


def _provider_auth_action_or_error(raw: Any) -> str:
    action = str(raw or "status").strip() or "status"
    if action not in _SPACE_PROVIDER_AUTH_ACTIONS:
        raise ValueError(f"invalid action: {action}")
    return action


def _artifact_action_or_error(raw: Any) -> str:
    action = str(raw or "list").strip() or "list"
    if action not in _SPACE_ARTIFACT_ACTIONS:
        raise ValueError(f"invalid action: {action}")
    return action


def _normalize_artifact_kind(raw: Any) -> str:
    text = str(raw or "").strip().lower()
    if not text:
        return ""
    if "." in text:
        text = text.split(".")[-1].strip()
    text = text.replace("-", "_")
    return _SPACE_ARTIFACT_KIND_ALIASES.get(text, text)


def _artifact_kind_or_error(raw: Any, *, allow_empty: bool = False) -> str:
    kind = _normalize_artifact_kind(raw)
    if not kind:
        if allow_empty:
            return ""
        raise ValueError("missing kind")
    if kind not in _SPACE_ARTIFACT_KINDS:
        example = _SPACE_ARTIFACT_KIND_ALIASES.get(kind)
        if not example:
            for alias, target in _SPACE_ARTIFACT_KIND_ALIASES.items():
                if kind.startswith(alias) or alias.startswith(kind):
                    example = target
                    break
        if example:
            raise ValueError(f"invalid kind: {kind} (try: {example})")
        raise ValueError(f"invalid kind: {kind}")
    return kind


def _bool_or_default(raw: Any, *, default: bool) -> bool:
    if raw is None:
        return default
    if isinstance(raw, bool):
        return raw
    text = str(raw).strip().lower()
    if text in {"1", "true", "yes", "y", "on"}:
        return True
    if text in {"0", "false", "no", "n", "off"}:
        return False
    return default


def _float_or_default(raw: Any, *, default: float, lo: float, hi: float) -> float:
    if raw is None:
        return default
    try:
        value = float(raw)
    except Exception:
        return default
    return max(lo, min(value, hi))


def _artifact_status_completed(raw: Any) -> bool:
    status = str(raw or "").strip().lower()
    return status in {"completed", "succeeded", "ready", "done"}


def _safe_path_fragment(raw: Any, *, fallback: str) -> str:
    text = str(raw or "").strip()
    text = re.sub(r"[^A-Za-z0-9._-]+", "-", text).strip("-._")
    if not text:
        text = fallback
    return text[:64]


def _artifact_extension(kind: str, *, output_format: str) -> str:
    k = str(kind or "").strip().lower()
    fmt = str(output_format or "").strip().lower()
    if k == "audio":
        return ".mp3"
    if k == "video":
        return ".mp4"
    if k in {"report", "study_guide"}:
        return ".md"
    if k == "quiz" or k == "flashcards":
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


def _artifact_row_id(row: Dict[str, Any]) -> str:
    return str(row.get("artifact_id") or row.get("id") or "").strip()


def _resolve_generated_artifact_id(
    *,
    provider: str,
    remote_space_id: str,
    kind: str,
    task_id: str,
    explicit_artifact_id: str,
) -> str:
    explicit = str(explicit_artifact_id or "").strip()
    if explicit:
        return explicit
    tid = str(task_id or "").strip()
    if not tid:
        return ""
    try:
        listed = provider_list_artifacts(provider, remote_space_id=remote_space_id, kind=kind)
    except Exception:
        return tid
    rows = listed.get("artifacts") if isinstance(listed.get("artifacts"), list) else []
    artifacts = [dict(item) for item in rows if isinstance(item, dict)]
    if not artifacts:
        return tid
    ids = {_artifact_row_id(item) for item in artifacts}
    if tid in ids:
        return tid
    completed_ids = [
        _artifact_row_id(item)
        for item in artifacts
        if _artifact_row_id(item) and _artifact_status_completed(item.get("status"))
    ]
    if len(completed_ids) == 1:
        return str(completed_ids[0] or tid)
    return tid


def _cleanup_legacy_task_named_artifact_file(
    *,
    group: Any,
    provider: str,
    kind: str,
    output_format: str,
    task_id: str,
    canonical_artifact_id: str,
    canonical_output_path: str,
) -> None:
    tid = str(task_id or "").strip()
    canonical_id = str(canonical_artifact_id or "").strip()
    if (not tid) or (not canonical_id) or canonical_id == tid:
        return
    try:
        legacy_path = _default_artifact_output_path(
            group=group,
            provider=provider,
            kind=kind,
            output_format=output_format,
            artifact_id="",
            task_id=tid,
        )
    except Exception:
        return
    legacy = Path(str(legacy_path or "")).expanduser()
    current = Path(str(canonical_output_path or "")).expanduser()
    if not legacy.is_absolute():
        return
    if legacy == current:
        return
    if legacy.exists() and legacy.is_file():
        try:
            legacy.unlink(missing_ok=True)
        except Exception:
            pass


def _default_artifact_output_path(
    *,
    group: Any,
    provider: str,
    kind: str,
    output_format: str,
    artifact_id: str,
    task_id: str,
) -> str:
    space_root = resolve_space_root_from_group(group, create=True)
    if space_root is None:
        raise ValueError("space_root_unavailable (attach scope first or provide output_path)")
    stamp = datetime.now(timezone.utc).strftime("%Y%m%d-%H%M%SZ")
    ident = _safe_path_fragment(artifact_id or task_id, fallback="latest")
    ext = _artifact_extension(kind, output_format=output_format)
    target = (
        Path(space_root)
        / "artifacts"
        / _safe_path_fragment(provider, fallback="provider")
        / _safe_path_fragment(kind, fallback="artifact")
        / f"{stamp}-{ident}{ext}"
    )
    target.parent.mkdir(parents=True, exist_ok=True)
    return str(target)


def _require_group(group_id: str):
    gid = str(group_id or "").strip()
    if not gid:
        raise ValueError("missing_group_id")
    group = load_group(gid)
    if group is None:
        raise LookupError(f"group not found: {gid}")
    return group


def _auto_notebook_title_for_group(group: Any) -> str:
    group_title = ""
    try:
        group_title = str(group.doc.get("title") or "").strip()
    except Exception:
        group_title = ""
    if not group_title:
        group_title = str(getattr(group, "group_id", "") or "").strip() or "Group"
    # NotebookLM title length is not strictly documented; keep a conservative cap.
    return f"CCCC · {group_title[:96]}"


def _default_binding(group_id: str, provider: str) -> Dict[str, Any]:
    return SpaceBinding(
        group_id=str(group_id or "").strip(),
        provider=provider,
        remote_space_id="",
        bound_by="",
        status="unbound",
    ).model_dump(exclude_none=True)


def _assert_write_permission(group: Any, *, by: str) -> None:
    require_group_permission(group, by=str(by or "user").strip(), action="group.update")


def _is_user_writer(by: str) -> bool:
    who = str(by or "").strip()
    return not who or who == "user"


def _provider_secret_key(provider: str) -> str:
    key = _SPACE_PROVIDER_SECRET_KEYS.get(str(provider or "").strip())
    if not key:
        raise ValueError(f"unsupported provider: {provider}")
    return key


def _resolve_auth_json(provider: str) -> str:
    pid = _provider_or_error(provider)
    key = _provider_secret_key(pid)
    if pid == "notebooklm":
        import os

        raw_env = str(os.environ.get("CCCC_NOTEBOOKLM_AUTH_JSON") or "").strip()
        if raw_env:
            return raw_env
    secrets_map = load_space_provider_secrets(pid)
    return str(secrets_map.get(key) or "").strip()


def _truthy_env(name: str) -> bool:
    value = str(os.environ.get(name) or "").strip().lower()
    return value in {"1", "true", "yes", "y", "on"}


def _int_env(name: str, *, default: int, lo: int, hi: int) -> int:
    raw = str(os.environ.get(name) or "").strip()
    try:
        value = int(raw) if raw else int(default)
    except Exception:
        value = int(default)
    return max(int(lo), min(int(hi), int(value)))


def _float_env(name: str, *, default: float, lo: float, hi: float) -> float:
    raw = str(os.environ.get(name) or "").strip()
    try:
        value = float(raw) if raw else float(default)
    except Exception:
        value = float(default)
    return max(float(lo), min(float(hi), float(value)))


def _query_inflight_limit() -> int:
    return _int_env("CCCC_SPACE_QUERY_MAX_INFLIGHT", default=1, lo=1, hi=8)


def _generate_active_limit() -> int:
    return _int_env("CCCC_SPACE_GENERATE_MAX_ACTIVE", default=1, lo=1, hi=4)


def _generate_pending_limit() -> int:
    return _int_env("CCCC_SPACE_GENERATE_MAX_PENDING", default=1, lo=0, hi=8)


def _async_generate_wait_timeout_seconds(request_timeout: float) -> float:
    floor = _float_env(
        "CCCC_SPACE_GENERATE_ASYNC_TIMEOUT_SECONDS",
        default=_DEFAULT_ASYNC_GENERATE_WAIT_TIMEOUT_SECONDS,
        lo=60.0,
        hi=86400.0,
    )
    return max(float(request_timeout or 0.0), float(floor))


def _space_lane_key(*, group_id: str, provider: str, remote_space_id: str) -> str:
    gid = str(group_id or "").strip()
    pid = str(provider or "notebooklm").strip() or "notebooklm"
    rid = str(remote_space_id or "").strip()
    return f"{gid}:{pid}:{rid}"


def _query_lane_snapshot(*, active: int, limit: int) -> Dict[str, Any]:
    return {
        "lane": "query",
        "active": max(0, int(active)),
        "active_limit": max(1, int(limit)),
        "pending": 0,
        "pending_limit": 0,
        "retry_after_seconds": _DEFAULT_QUERY_RETRY_AFTER_SECONDS,
    }


def _acquire_query_slot(*, lane_key: str) -> Tuple[bool, Dict[str, Any]]:
    with _QUERY_ACTIVE_LOCK:
        limit = _query_inflight_limit()
        active = int(_QUERY_ACTIVE_BY_LANE.get(lane_key) or 0)
        if active >= limit:
            return False, _query_lane_snapshot(active=active, limit=limit)
        _QUERY_ACTIVE_BY_LANE[lane_key] = active + 1
        return True, _query_lane_snapshot(active=(active + 1), limit=limit)


def _release_query_slot(*, lane_key: str) -> None:
    with _QUERY_ACTIVE_LOCK:
        active = int(_QUERY_ACTIVE_BY_LANE.get(lane_key) or 0)
        if active <= 1:
            _QUERY_ACTIVE_BY_LANE.pop(lane_key, None)
            return
        _QUERY_ACTIVE_BY_LANE[lane_key] = active - 1


def _generate_lane_snapshot(
    *,
    lane: _GenerateLaneState,
    active_limit: int,
    pending_limit: int,
    retry_after_seconds: int = _DEFAULT_GENERATE_RETRY_AFTER_SECONDS,
) -> Dict[str, Any]:
    return {
        "lane": "generate",
        "active": max(0, int(lane.active)),
        "active_limit": max(1, int(active_limit)),
        "pending": max(0, len(lane.pending)),
        "pending_limit": max(0, int(pending_limit)),
        "retry_after_seconds": max(1, int(retry_after_seconds)),
    }


def _admit_generate_request(req: _GenerateRequest, *, allow_queue: bool) -> Tuple[str, Dict[str, Any]]:
    with _GENERATE_LANES_LOCK:
        lane = _GENERATE_LANES.get(req.lane_key)
        if lane is None:
            lane = _GenerateLaneState()
            _GENERATE_LANES[req.lane_key] = lane
        active_limit = _generate_active_limit()
        pending_limit = _generate_pending_limit()
        if int(lane.active) < int(active_limit):
            lane.active = int(lane.active) + 1
            return "start", _generate_lane_snapshot(
                lane=lane, active_limit=active_limit, pending_limit=pending_limit
            )
        if allow_queue and len(lane.pending) < int(pending_limit):
            lane.pending.append(req)
            return "queued", _generate_lane_snapshot(
                lane=lane, active_limit=active_limit, pending_limit=pending_limit
            )
        snapshot = _generate_lane_snapshot(lane=lane, active_limit=active_limit, pending_limit=pending_limit)
        if int(lane.active) <= 0 and not lane.pending:
            _GENERATE_LANES.pop(req.lane_key, None)
        return "reject", snapshot


def _release_generate_slot(lane_key: str) -> Optional[_GenerateRequest]:
    next_req: Optional[_GenerateRequest] = None
    with _GENERATE_LANES_LOCK:
        lane = _GENERATE_LANES.get(lane_key)
        if lane is None:
            return None
        lane.active = max(0, int(lane.active) - 1)
        active_limit = _generate_active_limit()
        if lane.pending and int(lane.active) < int(active_limit):
            next_req = lane.pending.popleft()
            lane.active = int(lane.active) + 1
        if int(lane.active) <= 0 and not lane.pending:
            _GENERATE_LANES.pop(lane_key, None)
    return next_req


def _notify_target_from_by(by: str) -> str:
    actor_id = str(by or "").strip()
    if not actor_id or actor_id == "user":
        return ""
    return actor_id


def _emit_artifact_async_notify(
    *,
    group_id: str,
    by: str,
    provider: str,
    kind: str,
    task_id: str,
    status: str,
    output_path: str,
    ok: bool,
    error_code: str = "",
    error_message: str = "",
) -> None:
    try:
        group = load_group(group_id)
        if group is None:
            return
        title = "Group Space artifact ready" if ok else "Group Space artifact failed"
        if ok:
            message = f"{kind} generation completed."
        else:
            message = f"{kind} generation failed: {error_message or error_code or 'unknown error'}"
        context = {
            "group_id": str(group_id or ""),
            "provider": str(provider or "notebooklm"),
            "kind": str(kind or ""),
            "task_id": str(task_id or ""),
            "status": str(status or ""),
            "output_path": str(output_path or ""),
            "error": {"code": str(error_code or ""), "message": str(error_message or "")},
        }
        _ = append_event(
            group.ledger_path,
            kind="system.notify",
            group_id=group.group_id,
            scope_key="",
            by="system",
            data={
                "kind": ("info" if ok else "error"),
                "priority": ("normal" if ok else "high"),
                "title": title,
                "message": message,
                "target_actor_id": _notify_target_from_by(by),
                "requires_ack": False,
                "context": context,
            },
        )
    except Exception as e:
        _LOG.warning("group-space async artifact notify failed group=%s: %s", group_id, e)


def _start_generate_worker(req: _GenerateRequest, *, initial_generate_result: Optional[Dict[str, Any]] = None) -> None:
    t = threading.Thread(
        target=_run_generate_worker,
        kwargs={"req": req, "initial_generate_result": initial_generate_result},
        name="cccc-space-generate",
        daemon=True,
    )
    t.start()


def _run_generate_worker(*, req: _GenerateRequest, initial_generate_result: Optional[Dict[str, Any]] = None) -> None:
    task_id = ""
    artifact_status = ""
    final_output_path = ""
    error_code = ""
    error_message = ""
    ok = False
    try:
        generate_result = dict(initial_generate_result or {})
        if not generate_result:
            with acquire_space_provider_write(req.provider, req.remote_space_id):
                generate_result = provider_generate_artifact(
                    req.provider,
                    remote_space_id=req.remote_space_id,
                    kind=req.kind,
                    options=dict(req.options or {}),
                )
        task_id = str(generate_result.get("task_id") or "").strip()
        artifact_status = str(generate_result.get("status") or "").strip()
        if not task_id:
            raise SpaceProviderError(
                "space_provider_upstream_error",
                "provider generate returned empty task_id",
                transient=False,
                degrade_provider=False,
            )

        if not _artifact_status_completed(artifact_status):
            wait_result = provider_wait_artifact(
                req.provider,
                remote_space_id=req.remote_space_id,
                task_id=task_id,
                timeout_seconds=_async_generate_wait_timeout_seconds(req.timeout_seconds),
                initial_interval=float(req.initial_interval),
                max_interval=float(req.max_interval),
            )
            task_id = str(wait_result.get("task_id") or task_id).strip()
            artifact_status = str(wait_result.get("status") or artifact_status).strip()

        if req.save_to_space and _artifact_status_completed(artifact_status):
            group = _require_group(req.group_id)
            target_path = req.output_path
            selected_artifact_id = _resolve_generated_artifact_id(
                provider=req.provider,
                remote_space_id=req.remote_space_id,
                kind=req.kind,
                task_id=task_id,
                explicit_artifact_id=req.artifact_id,
            )
            if not target_path:
                target_path = _default_artifact_output_path(
                    group=group,
                    provider=req.provider,
                    kind=req.kind,
                    output_format=req.output_format,
                    artifact_id=(selected_artifact_id or req.artifact_id),
                    task_id=task_id,
                )
            if not selected_artifact_id:
                selected_artifact_id = req.artifact_id or task_id
            with acquire_space_provider_write(req.provider, req.remote_space_id):
                download_result = provider_download_artifact(
                    req.provider,
                    remote_space_id=req.remote_space_id,
                    kind=req.kind,
                    output_path=target_path,
                    artifact_id=selected_artifact_id,
                    output_format=req.output_format,
                )
            final_output_path = str(download_result.get("output_path") or target_path)
            if not req.output_path:
                _cleanup_legacy_task_named_artifact_file(
                    group=group,
                    provider=req.provider,
                    kind=req.kind,
                    output_format=req.output_format,
                    task_id=task_id,
                    canonical_artifact_id=selected_artifact_id,
                    canonical_output_path=final_output_path,
                )
        ok = _artifact_status_completed(artifact_status)
        if not ok:
            error_code = "space_artifact_not_ready"
            error_message = f"artifact status is {artifact_status or 'unknown'}"
    except Exception as e:
        if isinstance(e, SpaceProviderError):
            error_code = str(e.code or "space_provider_upstream_error")
        else:
            error_code = "group_space_artifact_failed"
        error_message = str(e)
    finally:
        _emit_artifact_async_notify(
            group_id=req.group_id,
            by=req.by,
            provider=req.provider,
            kind=req.kind,
            task_id=task_id,
            status=artifact_status,
            output_path=final_output_path,
            ok=bool(ok),
            error_code=error_code,
            error_message=error_message,
        )
        next_req = _release_generate_slot(req.lane_key)
        if next_req is not None:
            _start_generate_worker(next_req)


def _provider_runtime_readiness(provider: str) -> Dict[str, Any]:
    pid = _provider_or_error(provider)
    if pid != "notebooklm":
        return {"write_ready": False, "reason": "unsupported_provider"}
    provider_state = get_space_provider_state(pid)
    real_enabled = bool(provider_state.get("real_enabled"))
    stub_enabled = bool(_truthy_env("CCCC_NOTEBOOKLM_STUB"))
    auth_configured = False
    credential_read_error = ""
    try:
        auth_configured = bool(_resolve_auth_json(pid))
    except Exception as e:
        credential_read_error = str(e)
        _LOG.warning("group-space credential read failed provider=%s: %s", pid, credential_read_error)
    write_ready = (real_enabled and auth_configured) or ((not real_enabled) and stub_enabled)
    reason = "ok"
    if credential_read_error:
        reason = "credential_read_failed"
        write_ready = False
    if not write_ready:
        if real_enabled and not auth_configured:
            reason = "missing_auth"
        elif (not real_enabled) and (not stub_enabled):
            reason = "real_disabled_and_stub_disabled"
        else:
            reason = "not_ready"
    return {
        "real_adapter_enabled": real_enabled,
        "stub_adapter_enabled": stub_enabled,
        "auth_configured": auth_configured,
        "write_ready": write_ready,
        "readiness_reason": reason,
        "credential_read_error": credential_read_error or None,
    }


def _build_provider_credential_status(provider: str) -> Dict[str, Any]:
    pid = _provider_or_error(provider)
    key = _provider_secret_key(pid)
    base = describe_space_provider_credential_state(pid, key=key)
    auth_json = _resolve_auth_json(pid)
    env_configured = False
    store_configured = bool(base.get("store_configured"))
    source = "none"
    if pid == "notebooklm":
        import os

        env_configured = bool(str(os.environ.get("CCCC_NOTEBOOKLM_AUTH_JSON") or "").strip())
    if env_configured:
        source = "env"
    elif store_configured:
        source = "store"
    out = dict(base)
    out["configured"] = bool(auth_json)
    out["source"] = source
    out["env_configured"] = env_configured
    out["store_configured"] = store_configured
    if bool(auth_json):
        if source == "env":
            out["masked_value"] = "EN******ON"
        elif not str(out.get("masked_value") or "").strip():
            out["masked_value"] = "ST******ED"
    else:
        out["masked_value"] = None
    return out


def _sync_projection_best_effort(group_id: str, provider: str) -> None:
    try:
        _ = sync_group_space_projection(group_id, provider=provider)
    except Exception as e:
        _LOG.warning("group-space projection sync failed group=%s provider=%s: %s", group_id, provider, e)


def _resource_ingest_capabilities() -> Dict[str, Any]:
    return {
        "source_types": sorted(_SPACE_RESOURCE_INGEST_TYPES),
        "required_fields": {
            "file": ["source_type", "file_path"],
            "web_page": ["source_type", "url"],
            "youtube": ["source_type", "url"],
            "pasted_text": ["source_type", "content"],
            "google_docs": ["source_type", "file_id"],
            "google_slides": ["source_type", "file_id"],
            "google_spreadsheet": ["source_type", "file_id"],
        },
        "optional_fields": {
            "file": ["title", "mime_type", "path", "url"],
            "web_page": ["title"],
            "youtube": ["title"],
            "pasted_text": ["title"],
            "google_docs": ["title", "mime_type"],
            "google_slides": ["title", "mime_type"],
            "google_spreadsheet": ["title", "mime_type"],
        },
        "aliases": {
            "local_file": "file",
            "path": "file",
            "url": "web_page",
            "text": "pasted_text",
            "google_doc": "google_docs",
            "google_slide": "google_slides",
            "google_sheet": "google_spreadsheet",
        },
        "examples": {
            "file": {"source_type": "file", "file_path": "/abs/path/to/spec.md", "title": "Spec"},
            "web_page": {"source_type": "web_page", "url": "https://example.com/spec"},
            "youtube": {"source_type": "youtube", "url": "https://www.youtube.com/watch?v=dQw4w9WgXcQ"},
            "pasted_text": {"source_type": "pasted_text", "content": "Design notes..."},
            "google_docs": {"source_type": "google_docs", "file_id": "1abcDEF...", "title": "Roadmap Doc"},
        },
    }


def _artifact_capabilities() -> Dict[str, Any]:
    return {
        "actions": sorted(_SPACE_ARTIFACT_ACTIONS),
        "kinds": sorted(_SPACE_ARTIFACT_KINDS),
        "options": {
            "language": "Preferred output language (e.g., zh-CN, ja, en)",
            "instructions": "Provider-side generation instructions",
            "source_ids": "Optional remote source_id list to constrain generation scope",
        },
        "aliases": {
            "slide": "slide_deck",
            "slides": "slide_deck",
            "deck": "slide_deck",
            "overview": "report",
            "summary": "report",
            "study": "study_guide",
        },
        "examples": {
            "generate_audio": {
                "action": "generate",
                "kind": "audio",
                "wait": False,
                "save_to_space": True,
                "options": {"language": "zh-CN"},
            },
            "generate_slide": {
                "action": "generate",
                "kind": "slide_deck",
                "wait": False,
                "save_to_space": True,
                "options": {"language": "zh-CN"},
            },
            "download_report": {"action": "download", "kind": "report", "artifact_id": "<task_or_artifact_id>"},
            "list_latest": {"action": "list"},
        },
    }


def _query_capabilities() -> Dict[str, Any]:
    return {
        "options": {
            "source_ids": "Optional remote source_id list to constrain retrieval scope",
        },
        "unsupported_options": {
            "language": "Not supported by NotebookLM query API. Put language requirements in query text.",
            "lang": "Alias of language; also unsupported for query.",
        },
        "examples": {
            "basic": {"query": "Summarize key decisions from the notebook."},
            "scoped": {"query": "Summarize only this source.", "options": {"source_ids": ["src_123"]}},
        },
    }


def _normalize_query_options(raw: Any) -> Dict[str, Any]:
    if raw is None:
        return {}
    if not isinstance(raw, dict):
        raise ValueError("options must be an object")

    normalized: Dict[str, Any] = {}
    unsupported: list[str] = []
    for key, value in raw.items():
        name = str(key or "").strip()
        if not name:
            continue
        if name == "source_ids":
            if value is None:
                normalized["source_ids"] = []
                continue
            if not isinstance(value, list):
                raise ValueError("options.source_ids must be an array of non-empty strings")
            source_ids: list[str] = []
            for idx, item in enumerate(value):
                sid = str(item or "").strip()
                if not sid:
                    raise ValueError(f"options.source_ids[{idx}] must be a non-empty string")
                source_ids.append(sid)
            normalized["source_ids"] = source_ids
            continue
        unsupported.append(name)

    if unsupported:
        unique = sorted(set(unsupported))
        if any(name in {"language", "lang"} for name in unique):
            raise ValueError(
                "query options do not support language/lang; NotebookLM query API has no language parameter. "
                "Put language requirements in query text."
            )
        supported = ", ".join(sorted(_SPACE_QUERY_OPTION_KEYS))
        raise ValueError(f"unsupported query options: {', '.join(unique)} (supported: {supported})")

    return normalized


def handle_group_space_capabilities(args: Dict[str, Any]) -> DaemonResponse:
    group_id = str(args.get("group_id") or "").strip()
    provider_raw = args.get("provider")
    try:
        group = _require_group(group_id)
        provider = _provider_or_error(provider_raw)
        space_root = resolve_space_root_from_group(group, create=False)
        return DaemonResponse(
            ok=True,
            result={
                "group_id": group.group_id,
                "provider": provider,
                "local_scope_attached": bool(space_root),
                "space_root": str(space_root) if space_root is not None else "",
                "local_file_policy": group_space_local_file_policy(),
                "ingest": {
                    "kinds": sorted(_SPACE_JOB_KINDS),
                    "resource_ingest": _resource_ingest_capabilities(),
                },
                "query": _query_capabilities(),
                "artifacts": _artifact_capabilities(),
                "notes": [
                    "Put local files under repo/space (including repo/space/sources) for file sync uploads.",
                    "URL/Youtube/Google Docs/file-path resources should use group_space_ingest with kind=resource_ingest.",
                    "group_space_query currently supports only options.source_ids; language/lang must be in query text.",
                    "Artifact generation requires action=generate (or MCP auto-infers generate when source/options are present).",
                    "NotebookLM integration is best-effort and may change due to upstream unofficial APIs.",
                ],
            },
        )
    except LookupError as e:
        return _error("group_not_found", str(e))
    except ValueError as e:
        message = str(e)
        if message == "missing_group_id":
            return _error("missing_group_id", "missing group_id")
        if "permission denied" in message.lower():
            return _error("space_permission_denied", message)
        return _error("space_job_invalid", message)
    except Exception as e:
        return _error("group_space_capabilities_failed", str(e))


def handle_group_space_status(args: Dict[str, Any]) -> DaemonResponse:
    group_id = str(args.get("group_id") or "").strip()
    provider_raw = args.get("provider")
    try:
        group = _require_group(group_id)
        provider = _provider_or_error(provider_raw)
        provider_state = get_space_provider_state(provider)
        provider_state.update(_provider_runtime_readiness(provider))
        binding = get_space_binding(group.group_id, provider=provider) or _default_binding(group.group_id, provider)
        summary = space_queue_summary(group_id=group.group_id, provider=provider)
        sync_state = read_group_space_sync_state(group.group_id)
        return DaemonResponse(
            ok=True,
            result={
                "group_id": group.group_id,
                "provider": provider_state,
                "binding": binding,
                "queue_summary": summary,
                "sync": sync_state,
            },
        )
    except LookupError as e:
        return _error("group_not_found", str(e))
    except ValueError as e:
        message = str(e)
        if "permission denied" in message.lower():
            return _error("space_permission_denied", message)
        if message == "missing_group_id":
            return _error("missing_group_id", "missing group_id")
        return _error("space_job_invalid", message)
    except Exception as e:
        return _error("group_space_status_failed", str(e))


def handle_group_space_spaces(args: Dict[str, Any]) -> DaemonResponse:
    group_id = str(args.get("group_id") or "").strip()
    provider_raw = args.get("provider")
    try:
        group = _require_group(group_id)
        provider = _provider_or_error(provider_raw)
        spaces_result = provider_list_spaces(provider)
        spaces = spaces_result.get("spaces") if isinstance(spaces_result.get("spaces"), list) else []
        binding = get_space_binding(group.group_id, provider=provider) or _default_binding(group.group_id, provider)
        provider_state = get_space_provider_state(provider)
        provider_state.update(_provider_runtime_readiness(provider))
        return DaemonResponse(
            ok=True,
            result={
                "group_id": group.group_id,
                "provider": provider,
                "provider_state": provider_state,
                "binding": binding,
                "spaces": spaces,
            },
        )
    except SpaceProviderError as e:
        return _error(str(e.code or "space_provider_upstream_error"), str(e))
    except LookupError as e:
        return _error("group_not_found", str(e))
    except ValueError as e:
        message = str(e)
        if message == "missing_group_id":
            return _error("missing_group_id", "missing group_id")
        return _error("space_job_invalid", message)
    except Exception as e:
        return _error("group_space_spaces_failed", str(e))


def handle_group_space_bind(args: Dict[str, Any]) -> DaemonResponse:
    group_id = str(args.get("group_id") or "").strip()
    by = str(args.get("by") or "user").strip() or "user"
    provider_raw = args.get("provider")
    action = str(args.get("action") or "bind").strip().lower() or "bind"
    remote_space_id = str(args.get("remote_space_id") or "").strip()
    if action not in {"bind", "unbind"}:
        return _error("space_job_invalid", "action must be bind|unbind")
    try:
        group = _require_group(group_id)
        _assert_write_permission(group, by=by)
        provider = _provider_or_error(provider_raw)
        sync_result: Optional[Dict[str, Any]] = None
        if action == "bind":
            if not remote_space_id:
                try:
                    created = provider_create_space(
                        provider,
                        title=_auto_notebook_title_for_group(group),
                    )
                    remote_space_id = str(created.get("remote_space_id") or "").strip()
                    if not remote_space_id:
                        return _error("space_provider_upstream_error", "provider create_space returned empty remote_space_id")
                except SpaceProviderError as e:
                    return _error(str(e.code or "space_provider_upstream_error"), str(e))
            binding = upsert_space_binding(
                group.group_id,
                provider=provider,
                remote_space_id=remote_space_id,
                by=by,
                status="bound",
            )
            provider_state = set_space_provider_state(
                provider,
                enabled=True,
                mode="degraded",
                last_error="synchronizing group space after bind",
                touch_health=True,
            )
            try:
                sync_result = sync_group_space_files(group.group_id, provider=provider, force=True, by=by)
            except Exception as e:
                sync_result = {"ok": False, "code": "space_sync_failed", "message": str(e)}
            if isinstance(sync_result, dict) and bool(sync_result.get("ok")):
                provider_state = set_space_provider_state(
                    provider,
                    enabled=True,
                    mode="active",
                    last_error="",
                    touch_health=True,
                )
            else:
                last_error = str((sync_result or {}).get("message") or "group space sync failed")
                provider_state = set_space_provider_state(
                    provider,
                    enabled=True,
                    mode="degraded",
                    last_error=last_error,
                    touch_health=True,
                )
        else:
            binding = set_space_binding_unbound(group.group_id, provider=provider, by=by)
            has_any_bound = any(
                str(item.get("status") or "") == "bound" and str(item.get("remote_space_id") or "").strip()
                for item in list_space_bindings(provider)
            )
            if has_any_bound:
                provider_state = get_space_provider_state(provider)
            else:
                provider_state = set_space_provider_state(
                    provider,
                    enabled=False,
                    mode="disabled",
                    last_error="",
                    touch_health=True,
                )
        summary = space_queue_summary(group_id=group.group_id, provider=provider)
        _sync_projection_best_effort(group.group_id, provider)
        sync_state = read_group_space_sync_state(group.group_id)
        return DaemonResponse(
            ok=True,
            result={
                "group_id": group.group_id,
                "provider": provider_state,
                "binding": binding,
                "queue_summary": summary,
                "sync": sync_state,
                "sync_result": sync_result if isinstance(sync_result, dict) else {},
            },
        )
    except LookupError as e:
        return _error("group_not_found", str(e))
    except ValueError as e:
        message = str(e)
        if "permission denied" in message.lower():
            return _error("space_permission_denied", message)
        if message == "missing_group_id":
            return _error("missing_group_id", "missing group_id")
        return _error("space_job_invalid", message)
    except Exception as e:
        if "permission denied" in str(e).lower():
            return _error("space_permission_denied", str(e))
        return _error("group_space_bind_failed", str(e))


def handle_group_space_ingest(args: Dict[str, Any]) -> DaemonResponse:
    group_id = str(args.get("group_id") or "").strip()
    by = str(args.get("by") or "user").strip() or "user"
    provider_raw = args.get("provider")
    kind_raw = args.get("kind")
    payload_raw = args.get("payload")
    idempotency_key = str(args.get("idempotency_key") or "").strip()
    try:
        group = _require_group(group_id)
        _assert_write_permission(group, by=by)
        provider = _provider_or_error(provider_raw)
        kind = _kind_or_error(kind_raw)
        payload = payload_raw if isinstance(payload_raw, dict) else {}
        binding = get_space_binding(group.group_id, provider=provider)
        if not isinstance(binding, dict):
            return _error("space_binding_missing", "group is not bound to provider")
        if str(binding.get("status") or "") != "bound":
            return _error("space_binding_missing", "group space binding is not active")
        remote_space_id = str(binding.get("remote_space_id") or "").strip()
        if not remote_space_id:
            return _error("space_binding_missing", "binding has no remote_space_id")
        provider_state = get_space_provider_state(provider)
        if not bool(provider_state.get("enabled")) or str(provider_state.get("mode") or "") == "disabled":
            return _error("space_provider_disabled", "provider is disabled")
        job, deduped = enqueue_space_job(
            group_id=group.group_id,
            provider=provider,
            remote_space_id=remote_space_id,
            kind=kind,
            payload=payload,
            idempotency_key=idempotency_key,
        )
        final_job = dict(job)
        if not deduped:
            final_job = execute_space_job(str(job.get("job_id") or ""))
        _sync_projection_best_effort(group.group_id, provider)
        return DaemonResponse(
            ok=True,
            result={
                "group_id": group.group_id,
                "job_id": str(final_job.get("job_id") or ""),
                "accepted": True,
                "deduped": bool(deduped),
                "job": final_job,
                "queue_summary": space_queue_summary(group_id=group.group_id, provider=provider),
                "provider_mode": str(get_space_provider_state(provider).get("mode") or "disabled"),
            },
        )
    except LookupError as e:
        return _error("group_not_found", str(e))
    except ValueError as e:
        message = str(e)
        if message == "missing_group_id":
            return _error("missing_group_id", "missing group_id")
        return _error("space_job_invalid", message)
    except Exception as e:
        if "permission denied" in str(e).lower():
            return _error("space_permission_denied", str(e))
        return _error("group_space_ingest_failed", str(e))


def handle_group_space_query(args: Dict[str, Any]) -> DaemonResponse:
    group_id = str(args.get("group_id") or "").strip()
    provider_raw = args.get("provider")
    query_text = str(args.get("query") or "").strip()
    options_raw = args.get("options")
    if not query_text:
        return _error("space_job_invalid", "missing query")
    query_lane_key = ""
    query_slot_acquired = False
    try:
        options = _normalize_query_options(options_raw)
        group = _require_group(group_id)
        provider = _provider_or_error(provider_raw)
        binding = get_space_binding(group.group_id, provider=provider)
        if not isinstance(binding, dict):
            return _error("space_binding_missing", "group is not bound to provider")
        remote_space_id = str(binding.get("remote_space_id") or "").strip()
        if not remote_space_id or str(binding.get("status") or "") != "bound":
            return _error("space_binding_missing", "group space binding is not active")
        provider_state = get_space_provider_state(provider)
        mode = str(provider_state.get("mode") or "disabled")
        enabled = bool(provider_state.get("enabled"))
        if (not enabled) or mode == "disabled":
            return DaemonResponse(
                ok=True,
                result={
                    "group_id": group.group_id,
                    "provider": provider,
                    "provider_mode": "disabled",
                    "degraded": True,
                    "answer": "",
                    "references": [],
                    "error": {"code": "space_provider_disabled", "message": "provider is disabled"},
                },
            )
        query_lane_key = _space_lane_key(
            group_id=group.group_id,
            provider=provider,
            remote_space_id=remote_space_id,
        )
        query_ok, query_lane = _acquire_query_slot(lane_key=query_lane_key)
        if not query_ok:
            return _error(
                "space_backpressure",
                "query lane is busy; retry later",
                details=query_lane,
            )
        query_slot_acquired = True
        result = run_space_query(
            provider=provider,
            remote_space_id=remote_space_id,
            query=query_text,
            options=dict(options),
        )
        provider_after = get_space_provider_state(provider)
        return DaemonResponse(
            ok=True,
            result={
                "group_id": group.group_id,
                "provider": provider,
                "provider_mode": str(provider_after.get("mode") or mode),
                "degraded": bool(result.get("degraded")),
                "answer": str(result.get("answer") or ""),
                "references": list(result.get("references") or []),
                "error": result.get("error"),
            },
        )
    except LookupError as e:
        return _error("group_not_found", str(e))
    except ValueError as e:
        message = str(e)
        if message == "missing_group_id":
            return _error("missing_group_id", "missing group_id")
        return _error("space_job_invalid", message)
    except Exception as e:
        return _error("group_space_query_failed", str(e))
    finally:
        if query_slot_acquired and query_lane_key:
            _release_query_slot(lane_key=query_lane_key)


def handle_group_space_sources(args: Dict[str, Any]) -> DaemonResponse:
    group_id = str(args.get("group_id") or "").strip()
    by = str(args.get("by") or "user").strip() or "user"
    provider_raw = args.get("provider")
    action_raw = args.get("action")
    source_id = str(args.get("source_id") or "").strip()
    new_title = str(args.get("new_title") or "").strip()
    try:
        group = _require_group(group_id)
        provider = _provider_or_error(provider_raw)
        action = _source_action_or_error(action_raw)
        binding = get_space_binding(group.group_id, provider=provider)
        if not isinstance(binding, dict):
            return _error("space_binding_missing", "group is not bound to provider")
        remote_space_id = str(binding.get("remote_space_id") or "").strip()
        status = str(binding.get("status") or "").strip()
        if not remote_space_id or status != "bound":
            return _error("space_binding_missing", "group space binding is not active")
        provider_state = get_space_provider_state(provider)
        provider_mode = str(provider_state.get("mode") or "disabled")
        if (not bool(provider_state.get("enabled"))) or provider_mode == "disabled":
            return _error("space_provider_disabled", "provider is disabled")

        if action == "list":
            listed = provider_list_sources(provider, remote_space_id=remote_space_id)
            sources = listed.get("sources") if isinstance(listed.get("sources"), list) else []
            return DaemonResponse(
                ok=True,
                result={
                    "group_id": group.group_id,
                    "provider": provider,
                    "provider_mode": provider_mode,
                    "binding": binding,
                    "action": action,
                    "sources": sources,
                    "list_result": listed,
                },
            )

        _assert_write_permission(group, by=by)
        if not source_id:
            return _error("space_job_invalid", "source_id is required")

        if action == "delete":
            out = provider_delete_source(
                provider,
                remote_space_id=remote_space_id,
                source_id=source_id,
            )
            return DaemonResponse(
                ok=True,
                result={
                    "group_id": group.group_id,
                    "provider": provider,
                    "provider_mode": str(get_space_provider_state(provider).get("mode") or provider_mode),
                    "binding": binding,
                    "action": action,
                    "source_id": source_id,
                    "delete_result": out,
                },
            )

        if action == "rename":
            if not new_title:
                return _error("space_job_invalid", "new_title is required for rename")
            out = provider_rename_source(
                provider,
                remote_space_id=remote_space_id,
                source_id=source_id,
                new_title=new_title,
            )
            return DaemonResponse(
                ok=True,
                result={
                    "group_id": group.group_id,
                    "provider": provider,
                    "provider_mode": str(get_space_provider_state(provider).get("mode") or provider_mode),
                    "binding": binding,
                    "action": action,
                    "source_id": source_id,
                    "rename_result": out,
                },
            )

        out = provider_refresh_source(
            provider,
            remote_space_id=remote_space_id,
            source_id=source_id,
        )
        return DaemonResponse(
            ok=True,
            result={
                "group_id": group.group_id,
                "provider": provider,
                "provider_mode": str(get_space_provider_state(provider).get("mode") or provider_mode),
                "binding": binding,
                "action": action,
                "source_id": source_id,
                "refresh_result": out,
            },
        )
    except SpaceProviderError as e:
        return _error(str(e.code or "space_provider_upstream_error"), str(e))
    except LookupError as e:
        return _error("group_not_found", str(e))
    except ValueError as e:
        message = str(e)
        if "permission denied" in message.lower():
            return _error("space_permission_denied", message)
        if message == "missing_group_id":
            return _error("missing_group_id", "missing group_id")
        return _error("space_job_invalid", message)
    except Exception as e:
        if "permission denied" in str(e).lower():
            return _error("space_permission_denied", str(e))
        return _error("group_space_sources_failed", str(e))


def handle_group_space_artifact(args: Dict[str, Any]) -> DaemonResponse:
    group_id = str(args.get("group_id") or "").strip()
    by = str(args.get("by") or "user").strip() or "user"
    provider_raw = args.get("provider")
    action_raw = args.get("action")
    kind_raw = args.get("kind")
    options_raw = args.get("options")
    output_path = str(args.get("output_path") or "").strip()
    output_format = str(args.get("output_format") or "").strip().lower()
    artifact_id = str(args.get("artifact_id") or "").strip()
    save_to_space = _bool_or_default(args.get("save_to_space"), default=True)
    wait_for_completion = _bool_or_default(args.get("wait"), default=True)
    timeout_seconds = _float_or_default(args.get("timeout_seconds"), default=600.0, lo=10.0, hi=3600.0)
    initial_interval = _float_or_default(args.get("initial_interval"), default=2.0, lo=0.5, hi=60.0)
    max_interval = _float_or_default(args.get("max_interval"), default=10.0, lo=1.0, hi=120.0)
    if max_interval < initial_interval:
        max_interval = initial_interval
    lane_key = ""
    generate_slot_owned = False
    try:
        group = _require_group(group_id)
        provider = _provider_or_error(provider_raw)
        action = _artifact_action_or_error(action_raw)
        binding = get_space_binding(group.group_id, provider=provider)
        if not isinstance(binding, dict):
            return _error("space_binding_missing", "group is not bound to provider")
        remote_space_id = str(binding.get("remote_space_id") or "").strip()
        status = str(binding.get("status") or "").strip()
        if not remote_space_id or status != "bound":
            return _error("space_binding_missing", "group space binding is not active")
        provider_state = get_space_provider_state(provider)
        provider_mode = str(provider_state.get("mode") or "disabled")
        if (not bool(provider_state.get("enabled"))) or provider_mode == "disabled":
            return _error("space_provider_disabled", "provider is disabled")

        if action == "list":
            kind = _artifact_kind_or_error(kind_raw, allow_empty=True)
            listed = provider_list_artifacts(provider, remote_space_id=remote_space_id, kind=kind)
            artifacts = listed.get("artifacts") if isinstance(listed.get("artifacts"), list) else []
            return DaemonResponse(
                ok=True,
                result={
                    "group_id": group.group_id,
                    "provider": provider,
                    "provider_mode": provider_mode,
                    "binding": binding,
                    "action": action,
                    "kind": kind,
                    "artifacts": artifacts,
                    "list_result": listed,
                },
            )

        _assert_write_permission(group, by=by)
        kind = _artifact_kind_or_error(kind_raw, allow_empty=False)

        if action == "download":
            if not output_path and not save_to_space:
                return _error("space_job_invalid", "output_path is required when save_to_space=false")
            target_path = output_path
            if not target_path:
                target_path = _default_artifact_output_path(
                    group=group,
                    provider=provider,
                    kind=kind,
                    output_format=output_format,
                    artifact_id=artifact_id,
                    task_id="",
                )
            with acquire_space_provider_write(provider, remote_space_id):
                download_result = provider_download_artifact(
                    provider,
                    remote_space_id=remote_space_id,
                    kind=kind,
                    output_path=target_path,
                    artifact_id=artifact_id,
                    output_format=output_format,
                )
            return DaemonResponse(
                ok=True,
                result={
                    "group_id": group.group_id,
                    "provider": provider,
                    "provider_mode": str(get_space_provider_state(provider).get("mode") or provider_mode),
                    "binding": binding,
                    "action": action,
                    "kind": kind,
                    "saved_to_space": bool(save_to_space),
                    "output_path": str(download_result.get("output_path") or target_path),
                    "download_result": download_result,
                },
            )

        options = options_raw if isinstance(options_raw, dict) else {}
        lane_key = _space_lane_key(
            group_id=group.group_id,
            provider=provider,
            remote_space_id=remote_space_id,
        )
        gen_req = _GenerateRequest(
            lane_key=lane_key,
            group_id=group.group_id,
            provider=provider,
            remote_space_id=remote_space_id,
            kind=kind,
            options=dict(options),
            save_to_space=bool(save_to_space),
            output_path=output_path,
            output_format=output_format,
            artifact_id=artifact_id,
            by=by,
            timeout_seconds=timeout_seconds,
            initial_interval=initial_interval,
            max_interval=max_interval,
        )
        decision, lane = _admit_generate_request(gen_req, allow_queue=(not wait_for_completion))
        if decision == "reject":
            return _error(
                "space_backpressure",
                "generate lane is full; retry later",
                details=lane,
            )
        if decision == "queued":
            return DaemonResponse(
                ok=True,
                result={
                    "group_id": group.group_id,
                    "provider": provider,
                    "provider_mode": str(get_space_provider_state(provider).get("mode") or provider_mode),
                    "binding": binding,
                    "action": action,
                    "kind": kind,
                    "task_id": "",
                    "status": "queued",
                    "wait": False,
                    "queued": True,
                    "accepted": True,
                    "saved_to_space": False,
                    "output_path": "",
                    "generate_result": {},
                    "wait_result": {},
                    "download_result": {},
                    "queue": lane,
                },
            )
        generate_slot_owned = True
        if not wait_for_completion:
            _start_generate_worker(gen_req)
            generate_slot_owned = False
            return DaemonResponse(
                ok=True,
                result={
                    "group_id": group.group_id,
                    "provider": provider,
                    "provider_mode": str(get_space_provider_state(provider).get("mode") or provider_mode),
                    "binding": binding,
                    "action": action,
                    "kind": kind,
                    "task_id": "",
                    "status": "pending",
                    "wait": False,
                    "queued": False,
                    "accepted": True,
                    "saved_to_space": False,
                    "output_path": "",
                    "generate_result": {},
                    "wait_result": {},
                    "download_result": {},
                    "queue": lane,
                },
            )

        with acquire_space_provider_write(provider, remote_space_id):
            generate_result = provider_generate_artifact(
                provider,
                remote_space_id=remote_space_id,
                kind=kind,
                options=dict(options),
            )
        task_id = str(generate_result.get("task_id") or "").strip()
        artifact_status = str(generate_result.get("status") or "").strip()
        wait_result: Dict[str, Any] = {}
        if wait_for_completion and task_id and (not _artifact_status_completed(artifact_status)):
            wait_result = provider_wait_artifact(
                provider,
                remote_space_id=remote_space_id,
                task_id=task_id,
                timeout_seconds=timeout_seconds,
                initial_interval=initial_interval,
                max_interval=max_interval,
            )
            task_id = str(wait_result.get("task_id") or task_id).strip()
            artifact_status = str(wait_result.get("status") or artifact_status).strip()

        download_result: Dict[str, Any] = {}
        final_output_path = ""
        if save_to_space and _artifact_status_completed(artifact_status):
            target_path = output_path
            selected_artifact_id = _resolve_generated_artifact_id(
                provider=provider,
                remote_space_id=remote_space_id,
                kind=kind,
                task_id=task_id,
                explicit_artifact_id=artifact_id,
            )
            if not target_path:
                target_path = _default_artifact_output_path(
                    group=group,
                    provider=provider,
                    kind=kind,
                    output_format=output_format,
                    artifact_id=(selected_artifact_id or artifact_id),
                    task_id=task_id,
                )
            if not selected_artifact_id:
                selected_artifact_id = artifact_id or task_id
            with acquire_space_provider_write(provider, remote_space_id):
                download_result = provider_download_artifact(
                    provider,
                    remote_space_id=remote_space_id,
                    kind=kind,
                    output_path=target_path,
                    artifact_id=selected_artifact_id,
                    output_format=output_format,
                )
            final_output_path = str(download_result.get("output_path") or target_path)
            if not output_path:
                _cleanup_legacy_task_named_artifact_file(
                    group=group,
                    provider=provider,
                    kind=kind,
                    output_format=output_format,
                    task_id=task_id,
                    canonical_artifact_id=selected_artifact_id,
                    canonical_output_path=final_output_path,
                )
        next_req = _release_generate_slot(lane_key)
        generate_slot_owned = False
        if next_req is not None:
            _start_generate_worker(next_req)

        return DaemonResponse(
            ok=True,
            result={
                "group_id": group.group_id,
                "provider": provider,
                "provider_mode": str(get_space_provider_state(provider).get("mode") or provider_mode),
                "binding": binding,
                "action": action,
                "kind": kind,
                "task_id": task_id,
                "status": artifact_status,
                "wait": bool(wait_for_completion),
                "queued": False,
                "accepted": True,
                "saved_to_space": bool(save_to_space and bool(final_output_path)),
                "output_path": final_output_path,
                "generate_result": generate_result,
                "wait_result": wait_result,
                "download_result": download_result,
                "queue": lane,
            },
        )
    except SpaceProviderError as e:
        return _error(str(e.code or "space_provider_upstream_error"), str(e))
    except LookupError as e:
        return _error("group_not_found", str(e))
    except ValueError as e:
        message = str(e)
        if "permission denied" in message.lower():
            return _error("space_permission_denied", message)
        if message == "missing_group_id":
            return _error("missing_group_id", "missing group_id")
        return _error("space_job_invalid", message)
    except Exception as e:
        if "permission denied" in str(e).lower():
            return _error("space_permission_denied", str(e))
        return _error("group_space_artifact_failed", str(e))
    finally:
        if generate_slot_owned and lane_key:
            next_req = _release_generate_slot(lane_key)
            if next_req is not None:
                _start_generate_worker(next_req)


def handle_group_space_jobs(args: Dict[str, Any]) -> DaemonResponse:
    group_id = str(args.get("group_id") or "").strip()
    by = str(args.get("by") or "user").strip() or "user"
    provider_raw = args.get("provider")
    action_raw = args.get("action")
    state_filter = str(args.get("state") or "").strip()
    try:
        group = _require_group(group_id)
        provider = _provider_or_error(provider_raw)
        action = _action_or_error(action_raw)
        if action == "list":
            if state_filter and state_filter not in _SPACE_JOB_STATES:
                return _error("space_job_invalid", f"invalid state: {state_filter}")
            limit = max(1, min(int(args.get("limit") or 50), 500))
            jobs = list_space_jobs(
                group_id=group.group_id,
                provider=provider,
                state=state_filter,
                limit=limit,
            )
            return DaemonResponse(
                ok=True,
                result={
                    "group_id": group.group_id,
                    "provider": provider,
                    "jobs": jobs,
                    "queue_summary": space_queue_summary(group_id=group.group_id, provider=provider),
                },
            )

        _assert_write_permission(group, by=by)
        job_id = str(args.get("job_id") or "").strip()
        if not job_id:
            return _error("space_job_invalid", "missing job_id")
        job = get_space_job(job_id)
        if not isinstance(job, dict):
            return _error("space_job_not_found", f"job not found: {job_id}")
        if str(job.get("group_id") or "") != group.group_id:
            return _error("space_job_not_found", f"job not found: {job_id}")
        if str(job.get("provider") or "") != provider:
            return _error("space_job_not_found", f"job not found: {job_id}")

        if action == "retry":
            updated = retry_space_job(job_id)
        elif action == "cancel":
            updated = cancel_space_job(job_id)
        else:
            return _error("space_job_invalid", f"invalid action: {action}")
        _sync_projection_best_effort(group.group_id, provider)

        return DaemonResponse(
            ok=True,
            result={
                "group_id": group.group_id,
                "provider": provider,
                "job": updated,
                "queue_summary": space_queue_summary(group_id=group.group_id, provider=provider),
            },
        )
    except LookupError as e:
        return _error("group_not_found", str(e))
    except ValueError as e:
        message = str(e)
        if "permission denied" in message.lower():
            return _error("space_permission_denied", message)
        if message == "missing_group_id":
            return _error("missing_group_id", "missing group_id")
        if message.startswith("cannot retry job in state=") or message.startswith("cannot cancel job in state="):
            return _error("space_job_state_conflict", message)
        return _error("space_job_invalid", message)
    except Exception as e:
        if "permission denied" in str(e).lower():
            return _error("space_permission_denied", str(e))
        if "cannot retry job in state=" in str(e):
            return _error("space_job_state_conflict", str(e))
        if "cannot cancel job in state=" in str(e):
            return _error("space_job_state_conflict", str(e))
        return _error("group_space_jobs_failed", str(e))


def handle_group_space_sync(args: Dict[str, Any]) -> DaemonResponse:
    group_id = str(args.get("group_id") or "").strip()
    by = str(args.get("by") or "user").strip() or "user"
    provider_raw = args.get("provider")
    action_raw = args.get("action")
    force = bool(args.get("force") is True)
    try:
        group = _require_group(group_id)
        provider = _provider_or_error(provider_raw)
        action = _sync_action_or_error(action_raw)
        if action == "status":
            return DaemonResponse(
                ok=True,
                result={
                    "group_id": group.group_id,
                    "provider": provider,
                    "sync": read_group_space_sync_state(group.group_id),
                },
            )
        _assert_write_permission(group, by=by)
        result = sync_group_space_files(group.group_id, provider=provider, force=force, by=by)
        _sync_projection_best_effort(group.group_id, provider)
        if not bool(result.get("ok")):
            return _error(
                str(result.get("code") or "space_sync_failed"),
                str(result.get("message") or "group space sync failed"),
                details=result,
            )
        return DaemonResponse(
            ok=True,
            result={
                "group_id": group.group_id,
                "provider": provider,
                "sync": read_group_space_sync_state(group.group_id),
                "sync_result": result,
            },
        )
    except LookupError as e:
        return _error("group_not_found", str(e))
    except ValueError as e:
        message = str(e)
        if "permission denied" in message.lower():
            return _error("space_permission_denied", message)
        if message == "missing_group_id":
            return _error("missing_group_id", "missing group_id")
        return _error("space_job_invalid", message)
    except Exception as e:
        if "permission denied" in str(e).lower():
            return _error("space_permission_denied", str(e))
        return _error("group_space_sync_failed", str(e))


def handle_group_space_provider_credential_status(args: Dict[str, Any]) -> DaemonResponse:
    provider_raw = args.get("provider")
    by = str(args.get("by") or "user").strip() or "user"
    if not _is_user_writer(by):
        return _error("space_permission_denied", "only user can read provider credentials")
    try:
        provider = _provider_or_error(provider_raw)
        status = _build_provider_credential_status(provider)
        return DaemonResponse(ok=True, result={"provider": provider, "credential": status})
    except ValueError as e:
        return _error("space_job_invalid", str(e))
    except Exception as e:
        return _error("group_space_provider_credential_status_failed", str(e))


def handle_group_space_provider_credential_update(args: Dict[str, Any]) -> DaemonResponse:
    provider_raw = args.get("provider")
    by = str(args.get("by") or "user").strip() or "user"
    clear = bool(args.get("clear") is True)
    auth_json = str(args.get("auth_json") or "").strip()
    if not _is_user_writer(by):
        return _error("space_permission_denied", "only user can update provider credentials")
    try:
        provider = _provider_or_error(provider_raw)
        secret_key = _provider_secret_key(provider)
        if clear:
            _ = update_space_provider_secrets(
                provider,
                set_vars={},
                unset_keys=[secret_key],
                clear=True,
            )
        else:
            if not auth_json:
                return _error("space_provider_not_configured", "missing auth_json")
            _ = parse_notebooklm_auth_json(auth_json, label=secret_key)
            _ = update_space_provider_secrets(
                provider,
                set_vars={secret_key: auth_json},
                unset_keys=[],
                clear=False,
            )
        status = _build_provider_credential_status(provider)
        return DaemonResponse(ok=True, result={"provider": provider, "credential": status})
    except NotebookLMProviderError as e:
        return _error(str(e.code or "space_provider_auth_invalid"), str(e))
    except ValueError as e:
        return _error("space_job_invalid", str(e))
    except Exception as e:
        return _error("group_space_provider_credential_update_failed", str(e))


def handle_group_space_provider_health_check(args: Dict[str, Any]) -> DaemonResponse:
    provider_raw = args.get("provider")
    by = str(args.get("by") or "user").strip() or "user"
    if not _is_user_writer(by):
        return _error("space_permission_denied", "only user can run provider health checks")
    try:
        provider = _provider_or_error(provider_raw)
        current_state = get_space_provider_state(provider)
        auth_json = _resolve_auth_json(provider)
        try:
            health = notebooklm_health_check(
                auth_json_raw=auth_json,
                real_enabled=bool(current_state.get("real_enabled")),
            )
            mode = "active" if bool(current_state.get("enabled")) else "disabled"
            provider_state = set_space_provider_state(
                provider,
                mode=mode,
                last_error="",
                touch_health=True,
            )
            return DaemonResponse(
                ok=True,
                result={
                    "provider": provider,
                    "healthy": True,
                    "health": dict(health or {}),
                    "provider_state": provider_state,
                    "credential": _build_provider_credential_status(provider),
                },
            )
        except NotebookLMProviderError as e:
            mode = "degraded" if bool(current_state.get("enabled")) else "disabled"
            provider_state = set_space_provider_state(
                provider,
                mode=mode,
                last_error=str(e),
                touch_health=True,
            )
            return DaemonResponse(
                ok=True,
                result={
                    "provider": provider,
                    "healthy": False,
                    "error": {"code": str(e.code or "space_provider_upstream_error"), "message": str(e)},
                    "provider_state": provider_state,
                    "credential": _build_provider_credential_status(provider),
                },
            )
    except ValueError as e:
        return _error("space_job_invalid", str(e))
    except Exception as e:
        return _error("group_space_provider_health_check_failed", str(e))


def handle_group_space_provider_auth(args: Dict[str, Any]) -> DaemonResponse:
    provider_raw = args.get("provider")
    by = str(args.get("by") or "user").strip() or "user"
    action_raw = args.get("action")
    timeout_seconds = int(args.get("timeout_seconds") or 900)
    if not _is_user_writer(by):
        return _error("space_permission_denied", "only user can run provider auth flow")
    try:
        provider = _provider_or_error(provider_raw)
        if provider != "notebooklm":
            return _error("space_job_invalid", f"unsupported provider auth flow: {provider}")
        action = _provider_auth_action_or_error(action_raw)
        if action == "start":
            auth = start_notebooklm_auth_flow(timeout_seconds=timeout_seconds)
        elif action == "cancel":
            auth = cancel_notebooklm_auth_flow()
        else:
            auth = get_notebooklm_auth_flow_status()
        provider_state = get_space_provider_state(provider)
        provider_state.update(_provider_runtime_readiness(provider))
        credential = _build_provider_credential_status(provider)
        return DaemonResponse(
            ok=True,
            result={
                "provider": provider,
                "provider_state": provider_state,
                "credential": credential,
                "auth": auth,
            },
        )
    except ValueError as e:
        return _error("space_job_invalid", str(e))
    except Exception as e:
        return _error("group_space_provider_auth_failed", str(e))


def try_handle_group_space_op(op: str, args: Dict[str, Any]) -> Optional[DaemonResponse]:
    if op == "group_space_capabilities":
        return handle_group_space_capabilities(args)
    if op == "group_space_status":
        return handle_group_space_status(args)
    if op == "group_space_spaces":
        return handle_group_space_spaces(args)
    if op == "group_space_bind":
        return handle_group_space_bind(args)
    if op == "group_space_ingest":
        return handle_group_space_ingest(args)
    if op == "group_space_query":
        return handle_group_space_query(args)
    if op == "group_space_sources":
        return handle_group_space_sources(args)
    if op == "group_space_artifact":
        return handle_group_space_artifact(args)
    if op == "group_space_jobs":
        return handle_group_space_jobs(args)
    if op == "group_space_sync":
        return handle_group_space_sync(args)
    if op == "group_space_provider_credential_status":
        return handle_group_space_provider_credential_status(args)
    if op == "group_space_provider_credential_update":
        return handle_group_space_provider_credential_update(args)
    if op == "group_space_provider_health_check":
        return handle_group_space_provider_health_check(args)
    if op == "group_space_provider_auth":
        return handle_group_space_provider_auth(args)
    return None
