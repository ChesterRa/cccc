from __future__ import annotations

import asyncio
import base64
import contextlib
import json
import logging
import mimetypes
import tempfile
from pathlib import Path
import time
from typing import Any, Dict, Optional
from urllib.parse import urlparse

from fastapi import APIRouter, Depends, FastAPI, File, Form, HTTPException, Request, UploadFile, WebSocket, WebSocketDisconnect
from fastapi.responses import FileResponse, StreamingResponse
from starlette.concurrency import run_in_threadpool

from ....contracts.v1.automation import AutomationRuleSet
from ....daemon.codex_app_sessions import SUPERVISOR as codex_app_supervisor
from ....daemon.server import get_daemon_endpoint
from ....daemon.group.presentation_ops import load_presentation_snapshot, resolve_workspace_asset_path
from ....daemon.assistants.sherpa_streaming_asr import (
    SherpaStreamingAsrError,
    open_sherpa_streaming_session,
    transcribe_sherpa_streaming_pcm16,
)
from ....daemon.assistants.sherpa_diarization import (
    SherpaDiarizationError,
    run_sherpa_diarization_file,
    sherpa_diarization_status,
)
from ....daemon.assistants.voice_speaker_identity import (
    diarization_result_segments,
    diarization_result_speaker_embeddings,
    run_provisional_diarization_prefix,
)
from ....daemon.assistants.voice_speaker_transcripts import build_speaker_transcript_segments
from ....daemon.context.context_ops import _get_summary_context_fast, _rebuild_summary_snapshot
from ....runners import headless as headless_runner
from ....runners import pty as pty_runner
from ....kernel.blobs import resolve_blob_attachment_path, store_blob_bytes
from ....kernel.headless_events import headless_events_path, read_headless_replay_events, read_headless_replay_lines
from ....kernel.group import get_group_state, load_group
from ....kernel.context import ContextStorage
from ....kernel.query_projections import get_groups_projection
from ....daemon.runner_state_ops import headless_state_path, pty_state_path, web_model_actor_running
from ....kernel.group_template import parse_group_template
from ....kernel.ledger import append_event, read_last_lines
from ....kernel.prompt_files import (
    DEFAULT_PREAMBLE_BODY,
    HELP_FILENAME,
    PREAMBLE_FILENAME,
    delete_group_prompt_file,
    load_builtin_help_markdown,
    read_group_prompt_file,
    resolve_active_scope_root,
    write_group_prompt_file,
)
from ....kernel.pet_prompt import build_pet_prompt_parts, build_pet_snapshot_text, load_pet_help_markdown
from ....kernel.pet_outcomes import append_pet_decision_outcome
from ....kernel.pet_actor import get_pet_actor, is_desktop_pet_enabled
from ....kernel.pet_signals import load_pet_signals
from ....kernel.pet_task_evidence import build_pet_task_evidence
from ....daemon.pet.review_scheduler import request_manual_pet_review
from ...mcp.utils.help_markdown import parse_help_markdown
from ....kernel.access_tokens import list_access_tokens
from ....kernel.pet_decisions import load_pet_decisions
from ....paths import ensure_home
from ....util.conv import coerce_bool
from ....util.fs import atomic_write_text
from ....util.time import utc_now_iso
from ....util.process import pid_is_alive
from ..schemas import (
    AttachRequest,
    AssistantSettingsUpdateRequest,
    AssistantStatusUpdateRequest,
    AssistantVoiceAskRequestsClearRequest,
    AssistantVoiceDocumentInstructionRequest,
    AssistantVoiceDocumentSaveRequest,
    AssistantVoiceInputRequest,
    AssistantVoiceModelInstallRequest,
    AssistantVoiceModelRemoveRequest,
    AssistantVoiceRuntimeInstallRequest,
    AssistantVoiceRuntimeRemoveRequest,
    AssistantVoicePromptDraftAckRequest,
    AssistantVoiceTranscriptSegmentRequest,
    AssistantVoiceTranscriptionRequest,
    CreateGroupRequest,
    GroupAutomationManageRequest,
    GroupAutomationRequest,
    GroupAutomationResetBaselineRequest,
    GroupPresentationBrowserSessionRequest,
    GroupPresentationClearRequest,
    GroupPresentationPublishRequest,
    GroupPresentationPublishWorkspaceRequest,
    GroupSettingsRequest,
    GroupTemplatePreviewRequest,
    GroupUpdateRequest,
    PetDecisionOutcomeRequest,
    ProjectMdUpdateRequest,
    RepoPromptUpdateRequest,
    RouteContext,
    WEB_MAX_TEMPLATE_BYTES,
    WEB_MAX_FILE_BYTES,
    _safe_int,
    check_group,
    filter_groups_for_principal,
    require_admin,
    require_group,
    require_user,
    resolve_websocket_principal,
    websocket_tokens_active,
)

_VOICE_DIARIZATION_INTERVAL_MS = 8_000
_VOICE_DIARIZATION_MIN_AUDIO_MS = 10_000
_VOICE_DIARIZATION_ENABLE_PROVISIONAL = True
_VOICE_PCM16_BYTES_PER_SAMPLE = 2


def _safe_voice_session_id(value: Any) -> str:
    raw = str(value or "").strip() or "session"
    safe = "".join(ch if ch.isalnum() or ch in "_.-" else "-" for ch in raw).strip(".-")[:96]
    return safe or "session"


def _voice_speaker_transcript_artifact_path(group_id: str, session_id: str) -> Path:
    return ensure_home() / "voice-secretary" / str(group_id or "").strip() / _safe_voice_session_id(session_id) / "transcripts" / "speaker_transcript.json"


def _voice_meeting_session_dir(group_id: str, session_id: str) -> Path:
    return ensure_home() / "voice-secretary" / str(group_id or "").strip() / _safe_voice_session_id(session_id)


def _voice_meeting_session_path(group_id: str, session_id: str) -> Path:
    return _voice_meeting_session_dir(group_id, session_id) / "session.json"


def _voice_meeting_segments_path(group_id: str, session_id: str) -> Path:
    return _voice_meeting_session_dir(group_id, session_id) / "segments.jsonl"


def _voice_meeting_audio_path(group_id: str, session_id: str) -> Path:
    return _voice_meeting_session_dir(group_id, session_id) / "audio.pcm16"


def _read_json_file(path: Path) -> dict[str, Any]:
    try:
        payload = json.loads(path.read_text(encoding="utf-8"))
    except Exception:
        return {}
    return payload if isinstance(payload, dict) else {}


def _write_voice_meeting_session(group_id: str, session_id: str, patch: dict[str, Any]) -> dict[str, Any]:
    path = _voice_meeting_session_path(group_id, session_id)
    path.parent.mkdir(parents=True, exist_ok=True)
    now = utc_now_iso()
    current = _read_json_file(path)
    session = {
        "schema": 1,
        "group_id": str(group_id or "").strip(),
        "session_id": _safe_voice_session_id(session_id),
        "created_at": str(current.get("created_at") or now),
        **current,
        **patch,
        "updated_at": now,
    }
    atomic_write_text(path, json.dumps(session, ensure_ascii=False, sort_keys=True, indent=2), encoding="utf-8")
    return session


def _append_voice_meeting_segment(group_id: str, session_id: str, segment: dict[str, Any]) -> str:
    text = str(segment.get("text") or "").strip()
    if not text:
        return ""
    path = _voice_meeting_segments_path(group_id, session_id)
    path.parent.mkdir(parents=True, exist_ok=True)
    payload = {
        "schema": 1,
        "group_id": str(group_id or "").strip(),
        "session_id": _safe_voice_session_id(session_id),
        "created_at": utc_now_iso(),
        **segment,
    }
    with path.open("a", encoding="utf-8") as handle:
        handle.write(json.dumps(payload, ensure_ascii=False, sort_keys=True) + "\n")
    return str(path)


def _read_voice_meeting_segments(group_id: str, session_id: str, *, limit: int = 200) -> list[dict[str, Any]]:
    path = _voice_meeting_segments_path(group_id, session_id)
    if not path.exists():
        return []
    rows: list[dict[str, Any]] = []
    try:
        lines = path.read_text(encoding="utf-8").splitlines()
    except Exception:
        return []
    for line in lines[-max(1, int(limit)):]:
        try:
            payload = json.loads(line)
        except Exception:
            continue
        if isinstance(payload, dict):
            rows.append(payload)
    return rows


def _voice_meeting_transcript_text(group_id: str, session_id: str, *, limit: int = 200) -> str:
    texts: list[str] = []
    for segment in _read_voice_meeting_segments(group_id, session_id, limit=limit):
        text = str(segment.get("text") or "").strip()
        if text:
            texts.append(text)
    return "\n".join(texts).strip()


def _read_voice_meeting_session(group_id: str, session_id: str) -> dict[str, Any]:
    safe_session_id = _safe_voice_session_id(session_id)
    session = _read_json_file(_voice_meeting_session_path(group_id, safe_session_id))
    if not session:
        return {}
    speaker_artifact = _read_json_file(_voice_speaker_transcript_artifact_path(group_id, safe_session_id))
    return {
        **session,
        "segments": _read_voice_meeting_segments(group_id, safe_session_id),
        "diarization": speaker_artifact,
    }


def _read_latest_voice_meeting_session(group_id: str, *, document_path: str = "") -> dict[str, Any]:
    root = ensure_home() / "voice-secretary" / str(group_id or "").strip()
    if not root.exists():
        return {}
    target_document_path = str(document_path or "").strip()
    candidates = []
    for path in root.glob("*/session.json"):
        if not path.is_file():
            continue
        if target_document_path:
            session = _read_json_file(path)
            if str(session.get("document_path") or "").strip() != target_document_path:
                continue
        candidates.append(path)
    if not candidates:
        return {}
    latest = max(candidates, key=lambda path: path.stat().st_mtime)
    return _read_voice_meeting_session(group_id, latest.parent.name)


def _persist_voice_meeting_pcm16(group_id: str, session_id: str, source_path: Path) -> Path:
    audio_path = _voice_meeting_audio_path(group_id, session_id)
    audio_path.parent.mkdir(parents=True, exist_ok=True)
    audio_path.write_bytes(source_path.read_bytes())
    return audio_path


def _write_voice_speaker_transcript_artifact(group_id: str, session_id: str, payload: dict[str, Any]) -> str:
    path = _voice_speaker_transcript_artifact_path(group_id, session_id)
    path.parent.mkdir(parents=True, exist_ok=True)
    artifact = {
        "schema": 1,
        "group_id": str(group_id or "").strip(),
        "session_id": _safe_voice_session_id(session_id),
        "updated_at": utc_now_iso(),
        **payload,
    }
    atomic_write_text(path, json.dumps(artifact, ensure_ascii=False, sort_keys=True, indent=2), encoding="utf-8")
    return str(path)


def _append_voice_meeting_session_event(
    group_id: str,
    session_id: str,
    *,
    status: str,
    action: str,
    artifact_path: str = "",
    error: dict[str, Any] | None = None,
) -> None:
    group = load_group(str(group_id or "").strip())
    if group is None:
        return
    append_event(
        group.ledger_path,
        kind="assistant.voice.session",
        group_id=group.group_id,
        scope_key="",
        by="voice-secretary",
        data={
            "assistant_id": "voice_secretary",
            "session_id": _safe_voice_session_id(session_id),
            "action": action,
            "status": status,
            "artifact_path": artifact_path,
            "error_code": str((error or {}).get("code") or ""),
            "error_message": str((error or {}).get("message") or ""),
        },
    )


async def _run_voice_meeting_diarization_background(
    *,
    group_id: str,
    session_id: str,
    pcm16_path: Path,
    selected_model_id: str,
    sample_rate: int,
    audio_duration_ms: int,
    selected_asr_model_id: str = "",
) -> None:
    speaker_transcript_segments: list[dict[str, Any]] = []
    speaker_transcript_error: dict[str, Any] | None = None
    try:
        diarization = await run_sherpa_diarization_file(
            pcm16_path,
            selected_model_id=selected_model_id,
            sample_rate=sample_rate,
        )
        speaker_segments = diarization.get("segments") if isinstance(diarization.get("segments"), list) else []
        try:
            pcm16_audio = pcm16_path.read_bytes()
            speaker_transcript_segments = await build_speaker_transcript_segments(
                pcm16_audio,
                speaker_segments,
                sample_rate=sample_rate,
                transcribe_segment=lambda audio, rate: transcribe_sherpa_streaming_pcm16(
                    audio,
                    selected_model_id=selected_asr_model_id,
                    sample_rate=rate,
                ),
            )
        except SherpaStreamingAsrError as exc:
            speaker_transcript_error = {"code": exc.code, "message": exc.message, "details": exc.details}
        except Exception as exc:
            speaker_transcript_error = {"code": "speaker_transcript_failed", "message": str(exc), "details": {}}
        artifact_path = _write_voice_speaker_transcript_artifact(
            group_id,
            session_id,
            {
                "status": "ready",
                "sample_rate": sample_rate,
                "audio_duration_ms": audio_duration_ms,
                "segments": speaker_segments,
                "speaker_transcript_segments": speaker_transcript_segments,
                "speaker_transcript_error": speaker_transcript_error,
            },
        )
        _write_voice_meeting_session(
            group_id,
            session_id,
            {
                "status": "closed",
                "audio_duration_ms": audio_duration_ms,
                "audio_path": str(pcm16_path),
                "diarization_artifact_path": artifact_path,
                "diarization": {
                    **diarization,
                    "speaker_transcript_segments": speaker_transcript_segments,
                    "speaker_transcript_error": speaker_transcript_error,
                    "artifact_path": artifact_path,
                    "provisional": False,
                },
                "error": None,
            },
        )
        _append_voice_meeting_session_event(
            group_id,
            session_id,
            status="ready",
            action="diarization_ready",
            artifact_path=artifact_path,
        )
    except SherpaDiarizationError as exc:
        error = {"code": exc.code, "message": exc.message, "details": exc.details}
        artifact_path = _write_voice_speaker_transcript_artifact(
            group_id,
            session_id,
            {
                "status": "failed",
                "sample_rate": sample_rate,
                "audio_duration_ms": audio_duration_ms,
                "segments": [],
                "speaker_transcript_segments": [],
                "speaker_transcript_error": error,
            },
        )
        _write_voice_meeting_session(
            group_id,
            session_id,
            {
                "status": "closed",
                "audio_duration_ms": audio_duration_ms,
                "audio_path": str(pcm16_path),
                "diarization_artifact_path": artifact_path,
                "error": error,
            },
        )
        _append_voice_meeting_session_event(
            group_id,
            session_id,
            status="failed",
            action="diarization_failed",
            artifact_path=artifact_path,
            error=error,
        )
    except Exception as exc:
        logger.exception("voice meeting diarization background job failed: group_id=%s session_id=%s", group_id, session_id)
        error = {"code": "diarization_background_failed", "message": str(exc), "details": {}}
        artifact_path = _write_voice_speaker_transcript_artifact(
            group_id,
            session_id,
            {
                "status": "failed",
                "sample_rate": sample_rate,
                "audio_duration_ms": audio_duration_ms,
                "segments": [],
                "speaker_transcript_segments": [],
                "speaker_transcript_error": error,
            },
        )
        _write_voice_meeting_session(
            group_id,
            session_id,
            {
                "status": "closed",
                "audio_duration_ms": audio_duration_ms,
                "audio_path": str(pcm16_path),
                "diarization_artifact_path": artifact_path,
                "error": error,
            },
        )
        _append_voice_meeting_session_event(
            group_id,
            session_id,
            status="failed",
            action="diarization_failed",
            artifact_path=artifact_path,
            error=error,
        )


def _pcm16_duration_ms(byte_count: int, sample_rate: int) -> int:
    rate = max(1, int(sample_rate or 16000))
    return int(max(0, byte_count) / (_VOICE_PCM16_BYTES_PER_SAMPLE * rate) * 1000)


_PRESENTATION_BROWSER_STREAM_LIMIT_BYTES = 16 * 1024 * 1024
_CONTEXT_INFLIGHT: Dict[str, asyncio.Future[Dict[str, Any]]] = {}
_CONTEXT_GENERATION: Dict[str, int] = {}
_CONTEXT_LOCK = asyncio.Lock()
logger = logging.getLogger("cccc.web.groups")


def _actor_running_local(group_id: str, actor: Any) -> bool:
    gid = str(group_id or "").strip()
    if not gid or not isinstance(actor, dict):
        return False
    aid = str(actor.get("id") or "").strip()
    if not aid:
        return False
    runtime = str(actor.get("runtime") or "").strip().lower()
    runner_kind = str(actor.get("runner") or "pty").strip().lower() or "pty"
    effective_runner = "headless" if runner_kind == "headless" else "pty"
    if runtime == "web_model" and effective_runner == "headless":
        return web_model_actor_running(gid, actor)
    if runtime == "codex":
        if codex_app_supervisor.actor_running(gid, aid):
            return True
        try:
            raw = json.loads(headless_state_path(gid, aid).read_text(encoding="utf-8"))
            pid = int(raw.get("pid") or 0)
            status = str(raw.get("status") or "").strip().lower()
            state_runtime = str(raw.get("runtime") or "").strip().lower()
        except Exception:
            pid = 0
            status = ""
            state_runtime = ""
        return bool(pid > 0 and pid_is_alive(pid) and status != "stopped" and (not state_runtime or state_runtime == "codex"))
    if runtime == "claude" and effective_runner == "headless":
        try:
            raw = json.loads(headless_state_path(gid, aid).read_text(encoding="utf-8"))
            pid = int(raw.get("pid") or 0)
            status = str(raw.get("status") or "").strip().lower()
            state_runtime = str(raw.get("runtime") or "").strip().lower()
        except Exception:
            pid = 0
            status = ""
            state_runtime = ""
        return bool(pid > 0 and pid_is_alive(pid) and status != "stopped" and (not state_runtime or state_runtime == "claude"))
    if effective_runner == "pty":
        pty_state = pty_state_path(gid, aid)
        if pty_state.exists():
            try:
                raw = json.loads(pty_state.read_text(encoding="utf-8"))
                pid = int(raw.get("pid") or 0)
            except Exception:
                pid = 0
            if pid > 0 and pid_is_alive(pid):
                return True
        return bool(pty_runner.SUPERVISOR.actor_running(gid, aid))
    return bool(headless_runner.SUPERVISOR.actor_running(gid, aid))


def _group_runtime_status_local(group: Any) -> Dict[str, Any]:
    gid = str(getattr(group, "group_id", "") or "").strip()
    lifecycle_state = get_group_state(group) if group is not None else "active"
    if not gid or group is None:
        return {
            "lifecycle_state": lifecycle_state,
            "runtime_running": False,
            "running_actor_count": 0,
            "has_running_foreman": False,
        }
    actors = group.doc.get("actors") if isinstance(group.doc.get("actors"), list) else []
    running_actor_count = 0
    has_running_foreman = False
    for index, actor in enumerate(actors):
        if not _actor_running_local(gid, actor):
            continue
        running_actor_count += 1
        role = str(actor.get("role") or "").strip().lower() if isinstance(actor, dict) else ""
        enabled = coerce_bool(actor.get("enabled"), default=True) if isinstance(actor, dict) else False
        if role == "foreman" or (not role and index == 0 and enabled):
            has_running_foreman = True
    runtime_running = bool(
        running_actor_count > 0
        or codex_app_supervisor.group_running(gid)
        or pty_runner.SUPERVISOR.group_running(gid)
        or headless_runner.SUPERVISOR.group_running(gid)
    )
    # If the group doc says running=True but no processes are alive yet,
    # the daemon is still autostarting actors after a restart.  Report
    # runtime_running=True so the UI doesn't flash "stopped" during boot.
    doc_running = coerce_bool(group.doc.get("running"), default=False) if group is not None else False
    booting = bool(doc_running and not runtime_running and lifecycle_state not in ("stopped",))
    if booting:
        runtime_running = True
    return {
        "lifecycle_state": lifecycle_state,
        "runtime_running": runtime_running,
        "running_actor_count": running_actor_count,
        "has_running_foreman": has_running_foreman,
        "booting": booting,
    }


def _read_groups_local() -> Dict[str, Any]:
    projection = get_groups_projection()
    groups = projection.get("groups") if isinstance(projection.get("groups"), list) else []
    out: list[dict[str, Any]] = []
    for item in groups:
        if not isinstance(item, dict):
            continue
        gid = str(item.get("group_id") or "").strip()
        row = dict(item)
        group = load_group(gid)
        runtime_status = _group_runtime_status_local(group)
        row["state"] = str(runtime_status.get("lifecycle_state") or row.get("state") or "active")
        row["running"] = bool(runtime_status.get("runtime_running"))
        row["runtime_status"] = runtime_status
        out.append(row)
    return {
        "ok": True,
        "result": {
            "groups": out,
            "registry_health": dict(projection.get("registry_health") or {}),
        },
    }


def _read_group_local(group_id: str) -> Dict[str, Any]:
    gid = str(group_id or "").strip()
    if not gid:
        return {"ok": False, "error": {"code": "missing_group_id", "message": "missing group_id"}}
    group = load_group(gid)
    if group is None:
        return {"ok": False, "error": {"code": "group_not_found", "message": f"group not found: {gid}"}}
    doc = json.loads(json.dumps(group.doc))
    runtime_status = _group_runtime_status_local(group)
    doc["state"] = str(runtime_status.get("lifecycle_state") or doc.get("state") or "active")
    doc["running"] = bool(runtime_status.get("runtime_running"))
    doc["runtime_status"] = runtime_status
    im = doc.get("im")
    if isinstance(im, dict):
        im.pop("token", None)
        im.pop("bot_token", None)
        im.pop("app_token", None)
    return {"ok": True, "result": {"group": doc}}


def _read_headless_snapshot(group: Any, *, limit: int = 400) -> Dict[str, Any]:
    events = read_headless_replay_events(group.path, limit=limit)
    return {
        "group_id": str(getattr(group, "group_id", "") or "").strip(),
        "events": events,
        "count": len(events),
    }


def _read_presentation_local(group_id: str) -> Dict[str, Any]:
    gid = str(group_id or "").strip()
    if not gid:
        return {"ok": False, "error": {"code": "missing_group_id", "message": "missing group_id"}}
    group = load_group(gid)
    if group is None:
        return {"ok": False, "error": {"code": "group_not_found", "message": f"group not found: {gid}"}}
    snapshot = load_presentation_snapshot(group.group_id)
    return {
        "ok": True,
        "result": {
            "group_id": group.group_id,
            "presentation": snapshot.model_dump(mode="json", exclude_none=True),
        },
    }


def _read_context_summary_local(group_id: str) -> Dict[str, Any]:
    gid = str(group_id or "").strip()
    if not gid:
        return {"ok": False, "error": {"code": "missing_group_id", "message": "missing group_id"}}
    group = load_group(gid)
    if group is None:
        return {"ok": False, "error": {"code": "group_not_found", "message": f"group not found: {gid}"}}
    storage = ContextStorage(group)
    return {"ok": True, "result": _get_summary_context_fast(storage, group_id=gid)}


def _context_cache_key(group_id: str, detail: str) -> str:
    gid = str(group_id or "").strip()
    mode = str(detail or "summary").strip().lower() or "summary"
    return f"context:{gid}:{mode}"


async def invalidate_context_read(group_id: str, *, detail: Optional[str] = None) -> None:
    gid = str(group_id or "").strip()
    if not gid:
        return
    mode = str(detail or "").strip().lower()
    if mode in {"summary", "full"}:
        keys = [_context_cache_key(gid, mode)]
    else:
        keys = [_context_cache_key(gid, "summary"), _context_cache_key(gid, "full")]
    async with _CONTEXT_LOCK:
        touched = False
        for key in keys:
            if key in _CONTEXT_GENERATION or key in _CONTEXT_INFLIGHT:
                _CONTEXT_GENERATION[key] = int(_CONTEXT_GENERATION.get(key, 0)) + 1
                touched = True
            _CONTEXT_INFLIGHT.pop(key, None)
        if not touched:
            for key in keys:
                _CONTEXT_GENERATION[key] = 1


def _build_pet_context_payload(
    group: Any,
    help_prompt: Dict[str, Any],
    context_payload: Dict[str, Any],
    *,
    fresh: bool = False,
    verbose: bool = False,
) -> Dict[str, Any]:
    help_content = str(help_prompt.get("content") or "")
    persona = str(help_prompt.get("persona") or "").strip()
    source = str(help_prompt.get("pet_source") or "default").strip() or "default"
    signals = load_pet_signals(
        group,
        context_payload=context_payload,
        recent_chat_limit=50 if verbose or fresh else 10,
        recent_chat_source="active_tail",
        context_sync_limit=0,
        include_reply_obligation_status=False,
    )
    enriched_context_payload = dict(context_payload)
    enriched_context_payload["pet_signals"] = signals
    parts = build_pet_prompt_parts(group, help_markdown=help_content, context_payload=enriched_context_payload)
    decisions = load_pet_decisions(group)
    task_evidence: list[dict[str, Any]] = []
    try:
        storage = ContextStorage(group)
        task_evidence = build_pet_task_evidence(
            storage.list_tasks(),
            getattr(storage.load_agents(), "agents", []) or [],
            limit=4,
        )
    except Exception:
        task_evidence = []
    snapshot = str(parts.get("snapshot") or "").strip() or build_pet_snapshot_text(group, enriched_context_payload)
    payload = {
        "persona": persona,
        "snapshot": snapshot,
        "decisions": decisions,
        "task_evidence": task_evidence,
        "signals": signals,
        "source": str(parts.get("source") or source),
        "companion": parts.get("profile") or {},
    }
    if not verbose:
        return payload
    payload.update(
        {
            "help": str(parts.get("help") or ""),
            "prompt": str(parts.get("prompt") or ""),
            "help_prompt": help_prompt,
        }
    )
    return payload


def _collect_ledger_event_statuses(
    group: Any,
    events: list[dict[str, Any]],
    *,
    with_read_status: bool = True,
    with_ack_status: bool = True,
    with_obligation_status: bool = True,
) -> dict[str, dict[str, Any]]:
    started_at = time.perf_counter()
    status_by_event_id: dict[str, dict[str, Any]] = {}
    if not events:
        return status_by_event_id
    chat_events = [event for event in events if str(event.get("kind") or "") == "chat.message"]
    cached_statuses: dict[str, dict[str, Any]] = {}
    cached_ids: set[str] = set()
    if chat_events:
        from ....kernel.ledger_status_cache import get_cached_message_status_batch

        cached_statuses = get_cached_message_status_batch(
            group,
            [str(event.get("id") or "").strip() for event in chat_events],
        )
        cached_ids = set(cached_statuses.keys())
        for event_id, payload in cached_statuses.items():
            event_payload = status_by_event_id.setdefault(str(event_id), {})
            if with_read_status and "read_status" in payload:
                event_payload["read_status"] = payload["read_status"]
            if with_ack_status and "ack_status" in payload:
                event_payload["ack_status"] = payload["ack_status"]
            if with_obligation_status and "obligation_status" in payload:
                event_payload["obligation_status"] = payload["obligation_status"]

    missing_events = [
        event for event in chat_events
        if str(event.get("id") or "").strip() and str(event.get("id") or "").strip() not in cached_ids
    ]
    cache_hit_count = len(cached_ids)
    cache_miss_count = len(missing_events)
    if missing_events:
        read_status_by_event: dict[str, dict[str, bool]] = {}
        ack_status_by_event: dict[str, dict[str, bool]] = {}
        obligation_status_by_event: dict[str, dict[str, dict[str, bool]]] = {}

        if with_read_status:
            from ....kernel.inbox import get_read_status_batch

            read_status_by_event = get_read_status_batch(group, missing_events)
            for event_id, read_status in read_status_by_event.items():
                status_by_event_id.setdefault(str(event_id), {})["read_status"] = read_status

        if with_ack_status:
            from ....kernel.inbox import get_ack_status_batch

            ack_status_by_event = get_ack_status_batch(group, missing_events)
            for event_id, ack_status in ack_status_by_event.items():
                status_by_event_id.setdefault(str(event_id), {})["ack_status"] = ack_status

        if with_obligation_status:
            from ....kernel.inbox import get_obligation_status_batch

            obligation_status_by_event = get_obligation_status_batch(group, missing_events)
            for event_id, obligation_status in obligation_status_by_event.items():
                status_by_event_id.setdefault(str(event_id), {})["obligation_status"] = obligation_status

        if with_read_status and with_ack_status and with_obligation_status:
            try:
                from ....kernel.ledger_status_cache import store_message_status_batch

                store_message_status_batch(
                    group,
                    missing_events,
                    read_status_by_event=read_status_by_event,
                    ack_status_by_event=ack_status_by_event,
                    obligation_status_by_event=obligation_status_by_event,
                )
            except Exception:
                logger.debug("ledger_status_cache_store_failed group_id=%s", str(getattr(group, "group_id", "") or ""), exc_info=True)

    logger.debug(
        "ledger_statuses group_id=%s total=%d chat=%d cache_hit=%d cache_miss=%d elapsed_ms=%.1f",
        str(getattr(group, "group_id", "") or ""),
        len(events),
        len(chat_events),
        cache_hit_count,
        cache_miss_count,
        (time.perf_counter() - started_at) * 1000.0,
    )

    return status_by_event_id


def _apply_ledger_event_statuses(events: list[dict[str, Any]], status_by_event_id: dict[str, dict[str, Any]]) -> None:
    if not events or not status_by_event_id:
        return
    for ev in events:
        event_id = str(ev.get("id") or "").strip()
        if not event_id:
            continue
        payload = status_by_event_id.get(event_id)
        if not isinstance(payload, dict):
            continue
        if "read_status" in payload:
            ev["_read_status"] = payload["read_status"]
        if "ack_status" in payload:
            ev["_ack_status"] = payload["ack_status"]
        if "obligation_status" in payload:
            ev["_obligation_status"] = payload["obligation_status"]


def create_routers(ctx: RouteContext) -> list[APIRouter]:
    # --- global router (user/admin scope, per-route guard where needed) ---
    global_router = APIRouter(prefix="/api/v1")

    # --- group-scoped router ---
    group_router = APIRouter(prefix="/api/v1/groups/{group_id}", dependencies=[Depends(require_group)])
    async def _deduped_context_get(group_id: str, detail: str, fetcher) -> Dict[str, Any]:  # type: ignore[no-untyped-def]
        gid = str(group_id or "").strip()
        if not gid:
            return await fetcher()

        key = _context_cache_key(gid, detail)
        if ctx.read_only:
            ttl = max(0.0, min(5.0, ctx.exhibit_cache_ttl_s))
            return await ctx.cached_json(key, ttl, fetcher)

        fut: asyncio.Future[Dict[str, Any]] | None = None
        fetch_generation = 0
        do_fetch = False

        async with _CONTEXT_LOCK:
            fut = _CONTEXT_INFLIGHT.get(key)
            if fut is None or fut.done():
                loop = asyncio.get_running_loop()
                fut = loop.create_future()
                _CONTEXT_INFLIGHT[key] = fut
                fetch_generation = int(_CONTEXT_GENERATION.get(key, 0))
                do_fetch = True

        if fut is not None and not do_fetch:
            return await fut

        try:
            val = await fetcher()
            async with _CONTEXT_LOCK:
                if int(_CONTEXT_GENERATION.get(key, 0)) == fetch_generation and _CONTEXT_INFLIGHT.get(key) is fut:
                    if fut is not None and not fut.done():
                        fut.set_result(val)
                elif fut is not None and not fut.done():
                    fut.set_result(val)
            return val
        except Exception as exc:
            async with _CONTEXT_LOCK:
                if fut is not None and not fut.done():
                    fut.set_exception(exc)
            raise
        finally:
            async with _CONTEXT_LOCK:
                if _CONTEXT_INFLIGHT.get(key) is fut:
                    _CONTEXT_INFLIGHT.pop(key, None)

    def _request_access_token(request: Request) -> str:
        auth = str(request.headers.get("authorization") or "").strip()
        if auth.lower().startswith("bearer "):
            return str(auth[7:] or "").strip()
        cookie_token = str(request.cookies.get("cccc_access_token") or "").strip()
        if cookie_token:
            return cookie_token
        return str(request.query_params.get("token") or "").strip()

    def _presentation_card_type_for_url(url: str) -> str:
        suffix = Path(urlparse(str(url or "").strip()).path or "").suffix.lower()
        if suffix in {".png", ".jpg", ".jpeg", ".gif", ".webp", ".svg", ".bmp", ".avif"}:
            return "image"
        if suffix == ".pdf":
            return "pdf"
        return "web_preview"

    def _presentation_card_type_for_upload(filename: str, content_type: str) -> str:
        suffix = Path(str(filename or "").strip()).suffix.lower()
        mime = str(content_type or "").strip().lower()
        if suffix in {".md", ".markdown"} or mime in {"text/markdown", "text/x-markdown"}:
            return "markdown"
        if suffix in {".html", ".htm"} or mime == "text/html":
            return "web_preview"
        if suffix in {".png", ".jpg", ".jpeg", ".gif", ".webp", ".svg", ".bmp", ".avif"} or mime.startswith("image/"):
            return "image"
        if suffix == ".pdf" or mime == "application/pdf":
            return "pdf"
        return "file"

    def _normalize_presentation_slot(value: str) -> str:
        normalized = str(value or "auto").strip().lower().replace("_", "-") or "auto"
        if normalized in {"auto", "slot-1", "slot-2", "slot-3", "slot-4"}:
            return normalized
        raise HTTPException(
            status_code=400,
            detail={"code": "invalid_slot", "message": "slot must be auto or one of: slot-1, slot-2, slot-3, slot-4"},
        )

    def _presentation_workspace_root(group_id: str) -> Path:
        group = load_group(group_id)
        if group is None:
            raise HTTPException(status_code=404, detail={"code": "group_not_found", "message": f"group not found: {group_id}"})
        root = resolve_active_scope_root(group)
        if root is None:
            raise HTTPException(
                status_code=400,
                detail={"code": "presentation_workspace_missing_scope", "message": "group has no active scope"},
            )
        return root

    def _resolve_presentation_workspace_dir(group_id: str, raw_path: str) -> tuple[Path, Path]:
        root = _presentation_workspace_root(group_id)
        path_text = str(raw_path or "").strip().replace("\\", "/")
        target = root if not path_text else (root / Path(*Path(path_text).parts)).resolve()
        try:
            target.relative_to(root)
        except ValueError as exc:
            raise HTTPException(
                status_code=400,
                detail={
                    "code": "presentation_workspace_out_of_scope",
                    "message": "path must stay under the group's active scope root",
                },
            ) from exc
        if not target.exists():
            raise HTTPException(
                status_code=404,
                detail={"code": "presentation_workspace_not_found", "message": f"path not found: {path_text or '.'}"},
            )
        if not target.is_dir():
            raise HTTPException(
                status_code=400,
                detail={"code": "presentation_workspace_not_dir", "message": f"not a directory: {path_text or '.'}"},
            )
        return root, target

    def _presentation_workspace_relpath(root: Path, path: Path) -> str:
        rel = path.relative_to(root)
        rel_text = rel.as_posix()
        return "" if rel_text == "." else rel_text

    # ------------------------------------------------------------------ #
    # Global routes
    # ------------------------------------------------------------------ #

    @global_router.get("/groups")
    async def groups(request: Request) -> Dict[str, Any]:
        async def _fetch() -> Dict[str, Any]:
            return await run_in_threadpool(_read_groups_local)

        ttl = max(0.0, min(5.0, ctx.exhibit_cache_ttl_s))
        resp = await ctx.cached_json("groups", ttl, _fetch)
        result = resp.get("result") if isinstance(resp, dict) else None
        groups_list = result.get("groups") if isinstance(result, dict) else None
        if isinstance(groups_list, list):
            resp = dict(resp)
            out = dict(result)
            out["groups"] = filter_groups_for_principal(request, groups_list)
            resp["result"] = out
        return resp

    @global_router.post("/groups", dependencies=[Depends(require_admin)])
    async def group_create(req: CreateGroupRequest) -> Dict[str, Any]:
        return await ctx.daemon({"op": "group_create", "args": {"title": req.title, "topic": req.topic, "by": req.by}})

    @global_router.post("/groups/from_template", dependencies=[Depends(require_admin)])
    async def group_create_from_template(
        path: str = Form(...),
        title: str = Form("working-group"),
        topic: str = Form(""),
        by: str = Form("user"),
        file: UploadFile = File(...),
    ) -> Dict[str, Any]:
        raw = await file.read()
        if len(raw) > WEB_MAX_TEMPLATE_BYTES:
            raise HTTPException(status_code=413, detail={"code": "template_too_large", "message": "template too large"})
        template_text = raw.decode("utf-8", errors="replace")
        return await ctx.daemon(
            {
                "op": "group_create_from_template",
                "args": {"path": path, "title": title, "topic": topic, "by": by, "template": template_text},
            }
        )

    @global_router.post("/templates/preview", dependencies=[Depends(require_admin)])
    async def template_preview(file: UploadFile = File(...)) -> Dict[str, Any]:
        raw = await file.read()
        if len(raw) > WEB_MAX_TEMPLATE_BYTES:
            return {"ok": False, "error": {"code": "template_too_large", "message": "template too large"}}
        template_text = raw.decode("utf-8", errors="replace")
        try:
            tpl = parse_group_template(template_text)
        except Exception as e:
            return {"ok": False, "error": {"code": "invalid_template", "message": str(e)}}

        def _prompt_preview(value: Any, limit: int = 2000) -> Dict[str, Any]:
            if value is None:
                return {"source": "builtin"}
            raw_text = str(value)
            if not raw_text.strip():
                return {"source": "builtin"}
            out = raw_text.strip()
            if len(out) > limit:
                out = out[:limit] + "\n…"
            # Templates now map prompt overrides to CCCC_HOME/group prompts.
            return {"source": "home", "chars": len(raw_text), "preview": out}

        return {
            "ok": True,
            "result": {
                "template": {
                    "kind": tpl.kind,
                    "v": tpl.v,
                    "title": tpl.title,
                    "topic": tpl.topic,
                    "exported_at": tpl.exported_at,
                    "cccc_version": tpl.cccc_version,
                    "actors": [
                        {
                            "id": a.actor_id,
                            "title": a.title,
                            "runtime": a.runtime,
                            "runner": a.runner,
                            "command": a.command,
                            "submit": a.submit,
                            "enabled": bool(a.enabled),
                        }
                        for a in tpl.actors
                    ],
                    "settings": tpl.settings.model_dump(),
                    "automation": {
                        "rules": len(tpl.automation.rules),
                        "snippets": len(tpl.automation.snippets),
                    },
                    "prompts": {
                        "preamble": _prompt_preview(tpl.prompts.preamble),
                        "help": _prompt_preview(tpl.prompts.help),
                    },
                }
            },
        }

    @global_router.get("/events/stream", dependencies=[Depends(require_user)])
    async def global_events_stream() -> StreamingResponse:
        """SSE stream for global events (group created/deleted, etc.)."""
        from ..streams import sse_global_events_tail, create_sse_response
        return create_sse_response(sse_global_events_tail(ctx.home))

    # ------------------------------------------------------------------ #
    # Group-scoped routes
    # ------------------------------------------------------------------ #

    @group_router.get("")
    async def group_show(group_id: str) -> Dict[str, Any]:
        gid = str(group_id or "").strip()

        async def _fetch() -> Dict[str, Any]:
            return await run_in_threadpool(_read_group_local, gid)

        ttl = max(0.0, min(5.0, ctx.exhibit_cache_ttl_s))
        return await ctx.cached_json(f"group:{gid}", ttl, _fetch)

    @group_router.put("")
    async def group_update(group_id: str, req: GroupUpdateRequest) -> Dict[str, Any]:
        """Update group metadata (title/topic)."""
        patch: Dict[str, Any] = {}
        if req.title is not None:
            patch["title"] = req.title
        if req.topic is not None:
            patch["topic"] = req.topic
        if not patch:
            return {"ok": True, "result": {"message": "no changes"}}
        return await ctx.daemon({"op": "group_update", "args": {"group_id": group_id, "by": req.by, "patch": patch}})

    @group_router.delete("")
    async def group_delete(request: Request, group_id: str, confirm: str = "", by: str = "user") -> Dict[str, Any]:
        """Delete a group (admin-only, requires confirm=group_id)."""
        require_admin(request)
        if confirm != group_id:
            raise HTTPException(
                status_code=400,
                detail={"code": "confirmation_required", "message": f"confirm must equal group_id: {group_id}"}
            )
        return await ctx.daemon({"op": "group_delete", "args": {"group_id": group_id, "by": by}})

    @group_router.get("/context")
    async def group_context(group_id: str, fresh: bool = False, detail: str = "summary") -> Dict[str, Any]:
        """Get a group context view (summary by default, full when requested)."""
        gid = str(group_id or "").strip()
        detail_mode = str(detail or "summary").strip().lower() or "summary"
        if detail_mode not in {"summary", "full"}:
            raise HTTPException(
                status_code=400,
                detail={
                    "code": "invalid_detail",
                    "message": "detail must be 'summary' or 'full'",
                    "details": {"detail": detail_mode},
                },
            )

        async def _fetch() -> Dict[str, Any]:
            return await ctx.daemon({"op": "context_get", "args": {"group_id": gid, "detail": detail_mode}})

        if fresh:
            await invalidate_context_read(gid, detail=detail_mode)
            return await _fetch()
        if detail_mode == "summary":
            return await run_in_threadpool(_read_context_summary_local, gid)
        return await _deduped_context_get(gid, detail_mode, _fetch)

    @group_router.get("/template/export")
    async def group_template_export(group_id: str) -> Dict[str, Any]:
        return await ctx.daemon({"op": "group_template_export", "args": {"group_id": group_id}})

    @group_router.post("/template/preview")
    async def group_template_preview(group_id: str, req: GroupTemplatePreviewRequest) -> Dict[str, Any]:
        return await ctx.daemon({"op": "group_template_preview", "args": {"group_id": group_id, "template": req.template, "by": req.by}})

    @group_router.post("/template/preview_upload")
    async def group_template_preview_upload(
        group_id: str,
        by: str = Form("user"),
        file: UploadFile = File(...),
    ) -> Dict[str, Any]:
        raw = await file.read()
        if len(raw) > WEB_MAX_TEMPLATE_BYTES:
            raise HTTPException(status_code=413, detail={"code": "template_too_large", "message": "template too large"})
        template_text = raw.decode("utf-8", errors="replace")
        return await ctx.daemon({"op": "group_template_preview", "args": {"group_id": group_id, "template": template_text, "by": by}})

    @group_router.post("/template/import_replace")
    async def group_template_import_replace(
        group_id: str,
        confirm: str = Form(""),
        by: str = Form("user"),
        file: UploadFile = File(...),
    ) -> Dict[str, Any]:
        raw = await file.read()
        if len(raw) > WEB_MAX_TEMPLATE_BYTES:
            raise HTTPException(status_code=413, detail={"code": "template_too_large", "message": "template too large"})
        template_text = raw.decode("utf-8", errors="replace")
        await invalidate_context_read(group_id)
        return await ctx.daemon(
            {
                "op": "group_template_import_replace",
                "args": {"group_id": group_id, "confirm": confirm, "by": by, "template": template_text},
            }
        )

    @group_router.get("/tasks")
    async def group_tasks(group_id: str, task_id: Optional[str] = None) -> Dict[str, Any]:
        """List tasks (or fetch a single task when task_id is provided)."""
        args: Dict[str, Any] = {"group_id": group_id}
        if task_id:
            args["task_id"] = task_id
        return await ctx.daemon({"op": "task_list", "args": args})

    @group_router.get("/presentation")
    async def group_presentation_get(group_id: str) -> Dict[str, Any]:
        return await run_in_threadpool(_read_presentation_local, group_id)

    @group_router.post("/presentation/publish")
    async def group_presentation_publish(group_id: str, req: GroupPresentationPublishRequest) -> Dict[str, Any]:
        url = str(req.url or "").strip()
        if not url:
            raise HTTPException(status_code=400, detail={"code": "missing_url", "message": "missing url"})
        slot = _normalize_presentation_slot(req.slot)
        return await ctx.daemon(
            {
                "op": "presentation_publish",
                "args": {
                    "group_id": group_id,
                    "by": req.by,
                    "slot": slot,
                    "card_type": _presentation_card_type_for_url(url),
                    "title": req.title,
                    "summary": req.summary,
                    "url": url,
                },
            }
        )

    @group_router.get("/presentation/workspace/list")
    async def group_presentation_workspace_list(group_id: str, path: str = "") -> Dict[str, Any]:
        root, target = _resolve_presentation_workspace_dir(group_id, path)
        items = []
        try:
            for entry in sorted(target.iterdir(), key=lambda item: (not item.is_dir(), item.name.lower())):
                if entry.name.startswith("."):
                    continue
                rel_path = _presentation_workspace_relpath(root, entry)
                items.append(
                    {
                        "name": entry.name,
                        "path": rel_path,
                        "is_dir": entry.is_dir(),
                        "mime_type": (str(mimetypes.guess_type(entry.name)[0] or "") if entry.is_file() else ""),
                    }
                )
        except PermissionError as exc:
            raise HTTPException(
                status_code=403,
                detail={"code": "presentation_workspace_permission_denied", "message": str(exc)},
            ) from exc

        current_path = _presentation_workspace_relpath(root, target)
        parent_path = None
        if target != root:
            parent_path = _presentation_workspace_relpath(root, target.parent)
        return {
            "ok": True,
            "result": {
                "root_path": str(root),
                "path": current_path,
                "parent": parent_path,
                "items": items[:200],
            },
        }

    @group_router.post("/presentation/publish_workspace")
    async def group_presentation_publish_workspace(
        group_id: str, req: GroupPresentationPublishWorkspaceRequest
    ) -> Dict[str, Any]:
        path_text = str(req.path or "").strip()
        if not path_text:
            raise HTTPException(status_code=400, detail={"code": "missing_path", "message": "missing path"})
        _resolve_presentation_workspace_dir(group_id, Path(path_text).parent.as_posix() if Path(path_text).parent.as_posix() != "." else "")
        slot = _normalize_presentation_slot(req.slot)
        return await ctx.daemon(
            {
                "op": "presentation_publish",
                "args": {
                    "group_id": group_id,
                    "by": req.by,
                    "slot": slot,
                    "title": req.title,
                    "summary": req.summary,
                    "path": path_text,
                },
            }
        )

    @group_router.post("/presentation/publish_upload")
    async def group_presentation_publish_upload(
        group_id: str,
        by: str = Form("user"),
        slot: str = Form("auto"),
        title: str = Form(""),
        summary: str = Form(""),
        file: UploadFile = File(...),
    ) -> Dict[str, Any]:
        group = load_group(group_id)
        if group is None:
            raise HTTPException(status_code=404, detail={"code": "group_not_found", "message": f"group not found: {group_id}"})

        raw = await file.read()
        if len(raw) > WEB_MAX_FILE_BYTES:
            raise HTTPException(status_code=413, detail={"code": "file_too_large", "message": "file too large"})

        normalized_slot = _normalize_presentation_slot(slot)
        filename = str(getattr(file, "filename", "") or "file").strip() or "file"
        content_type = str(getattr(file, "content_type", "") or "").strip()
        card_type = _presentation_card_type_for_upload(filename, content_type)
        resolved_title = str(title or "").strip() or filename
        args: Dict[str, Any] = {
            "group_id": group_id,
            "by": str(by or "user").strip() or "user",
            "slot": normalized_slot,
            "card_type": card_type,
            "title": resolved_title,
            "summary": str(summary or "").strip(),
            "source_label": filename,
        }

        if card_type == "markdown":
            args["content"] = raw.decode("utf-8", errors="replace")
            args["source_ref"] = filename
        else:
            stored = store_blob_bytes(
                group,
                data=raw,
                filename=filename,
                mime_type=content_type,
            )
            args["blob_rel_path"] = str(stored.get("path") or "")
            args["source_ref"] = str(stored.get("path") or "")

        return await ctx.daemon({"op": "presentation_publish", "args": args})

    @group_router.post("/presentation/ref_snapshot")
    async def group_presentation_reference_snapshot_upload(
        group_id: str,
        by: str = Form("user"),
        slot: str = Form(""),
        source: str = Form("browser_surface"),
        captured_at: str = Form(""),
        width: int = Form(0),
        height: int = Form(0),
        file: UploadFile = File(...),
    ) -> Dict[str, Any]:
        _ = by
        group = load_group(group_id)
        if group is None:
            raise HTTPException(status_code=404, detail={"code": "group_not_found", "message": f"group not found: {group_id}"})

        raw = await file.read()
        if len(raw) > WEB_MAX_FILE_BYTES:
            raise HTTPException(status_code=413, detail={"code": "file_too_large", "message": "file too large"})

        normalized_slot = _normalize_presentation_slot(slot)
        filename = str(getattr(file, "filename", "") or "").strip()
        if not filename:
            suffix = Path(str(getattr(file, "filename", "") or "snapshot.jpg")).suffix or ".jpg"
            filename = f"presentation-ref-{normalized_slot or 'slot'}{suffix}"
        content_type = str(getattr(file, "content_type", "") or "").strip() or "image/jpeg"

        stored = store_blob_bytes(
            group,
            data=raw,
            filename=filename,
            mime_type=content_type,
            kind="image",
        )
        return {
            "ok": True,
            "result": {
                "group_id": group_id,
                "snapshot": {
                    "path": str(stored.get("path") or ""),
                    "mime_type": str(stored.get("mime_type") or content_type),
                    "bytes": int(stored.get("bytes") or len(raw)),
                    "sha256": str(stored.get("sha256") or ""),
                    "width": _safe_int(width, default=0, min_value=0),
                    "height": _safe_int(height, default=0, min_value=0),
                    "captured_at": str(captured_at or "").strip(),
                    "source": str(source or "").strip() or "browser_surface",
                },
            },
        }

    @group_router.post("/presentation/clear")
    async def group_presentation_clear(group_id: str, req: GroupPresentationClearRequest) -> Dict[str, Any]:
        return await ctx.daemon(
            {
                "op": "presentation_clear",
                "args": {
                    "group_id": group_id,
                    "by": req.by,
                    "slot": req.slot,
                    "all": bool(req.all),
                },
            }
        )

    @group_router.get("/presentation/browser_surface/session")
    async def group_presentation_browser_surface_info(group_id: str, slot: str = "") -> Dict[str, Any]:
        return await ctx.daemon({"op": "presentation_browser_info", "args": {"group_id": group_id, "slot": slot}})

    @group_router.post("/presentation/browser_surface/session")
    async def group_presentation_browser_surface_open(
        group_id: str, req: GroupPresentationBrowserSessionRequest
    ) -> Dict[str, Any]:
        url = str(req.url or "").strip()
        if not url:
            raise HTTPException(status_code=400, detail={"code": "missing_url", "message": "missing url"})
        width = _safe_int(req.width, default=1280, min_value=640, max_value=2560)
        height = _safe_int(req.height, default=800, min_value=480, max_value=1600)
        return await ctx.daemon(
            {
                "op": "presentation_browser_open",
                "args": {
                    "group_id": group_id,
                    "slot": req.slot,
                    "by": req.by,
                    "url": url,
                    "width": width,
                    "height": height,
                },
            }
        )

    @group_router.post("/presentation/browser_surface/session/close")
    async def group_presentation_browser_surface_close(
        group_id: str, req: GroupPresentationBrowserSessionRequest
    ) -> Dict[str, Any]:
        return await ctx.daemon(
            {
                "op": "presentation_browser_close",
                "args": {
                    "group_id": group_id,
                    "slot": req.slot,
                    "by": req.by,
                },
            }
        )

    @group_router.get("/presentation/slots/{slot_id}/asset")
    async def group_presentation_asset(group_id: str, slot_id: str, download: bool = False) -> FileResponse:
        group = load_group(group_id)
        if group is None:
            raise HTTPException(status_code=404, detail={"code": "group_not_found", "message": f"group not found: {group_id}"})

        normalized_slot_id = str(slot_id or "").strip().lower().replace("_", "-")
        if normalized_slot_id not in {"slot-1", "slot-2", "slot-3", "slot-4"}:
            raise HTTPException(
                status_code=400,
                detail={"code": "invalid_slot", "message": "slot must be one of: slot-1, slot-2, slot-3, slot-4"},
            )

        snapshot = load_presentation_snapshot(group_id)
        slot = next((item for item in snapshot.slots if str(item.slot_id or "") == normalized_slot_id), None)
        card = slot.card if slot else None
        workspace_rel_path = str((card.content.workspace_rel_path if card and card.content else "") or "").strip()
        blob_rel_path = str((card.content.blob_rel_path if card and card.content else "") or "").strip()
        if not card or (not blob_rel_path and not workspace_rel_path):
            raise HTTPException(
                status_code=404,
                detail={"code": "presentation_asset_not_found", "message": f"presentation slot has no local asset: {normalized_slot_id}"},
            )

        abs_path: Path
        if workspace_rel_path:
            try:
                abs_path = resolve_workspace_asset_path(group, workspace_rel_path)
            except FileNotFoundError:
                raise HTTPException(
                    status_code=404,
                    detail={"code": "workspace_file_not_found", "message": f"workspace file not found: {workspace_rel_path}"},
                )
            except ValueError as exc:
                raise HTTPException(
                    status_code=400,
                    detail={"code": "workspace_file_invalid", "message": str(exc)},
                ) from exc
        else:
            try:
                abs_path = resolve_blob_attachment_path(group, rel_path=blob_rel_path)
            except FileNotFoundError:
                raise HTTPException(
                    status_code=404,
                    detail={"code": "blob_not_found", "message": f"blob not found: {blob_rel_path}"},
                )

        media_type = str(card.content.mime_type or "").strip()
        if not media_type:
            media_type = str(mimetypes.guess_type(abs_path.name)[0] or "application/octet-stream")
        filename = str(card.content.file_name or card.source_label or abs_path.name).strip() or abs_path.name
        response = FileResponse(
            path=abs_path,
            media_type=media_type,
            filename=filename,
            content_disposition_type="attachment" if download else "inline",
        )
        response.headers["Cache-Control"] = "no-store"
        return response

    @group_router.get("/project_md")
    async def project_md_get(group_id: str) -> Dict[str, Any]:
        """Get PROJECT.md content for the group's active scope root (repo root)."""
        group = load_group(group_id)
        if group is None:
            raise HTTPException(status_code=404, detail={"code": "group_not_found", "message": f"group not found: {group_id}"})

        scopes = group.doc.get("scopes") if isinstance(group.doc.get("scopes"), list) else []
        active_scope_key = str(group.doc.get("active_scope_key") or "")

        project_root: Optional[str] = None
        for sc in scopes:
            if not isinstance(sc, dict):
                continue
            sk = str(sc.get("scope_key") or "")
            if sk == active_scope_key:
                project_root = str(sc.get("url") or "")
                break
        if not project_root:
            if scopes and isinstance(scopes[0], dict):
                project_root = str(scopes[0].get("url") or "")
        if not project_root:
            return {"ok": True, "result": {"found": False, "path": None, "content": None, "error": "No scope attached to group. Use 'cccc attach <path>' first."}}

        root = Path(project_root).expanduser()
        if not root.exists() or not root.is_dir():
            return {"ok": True, "result": {"found": False, "path": str(root / "PROJECT.md"), "content": None, "error": f"Project root does not exist: {root}"}}

        project_md_path = root / "PROJECT.md"
        if not project_md_path.exists():
            project_md_path_lower = root / "project.md"
            if project_md_path_lower.exists():
                project_md_path = project_md_path_lower
            else:
                return {"ok": True, "result": {"found": False, "path": str(project_md_path), "content": None, "error": f"PROJECT.md not found at {project_md_path}"}}

        try:
            content = project_md_path.read_text(encoding="utf-8", errors="replace")
            return {"ok": True, "result": {"found": True, "path": str(project_md_path), "content": content}}
        except Exception as e:
            return {"ok": True, "result": {"found": False, "path": str(project_md_path), "content": None, "error": f"Failed to read PROJECT.md: {e}"}}

    @group_router.put("/project_md")
    async def project_md_put(group_id: str, req: ProjectMdUpdateRequest) -> Dict[str, Any]:
        """Create or update PROJECT.md in the group's active scope root (repo root)."""
        group = load_group(group_id)
        if group is None:
            raise HTTPException(status_code=404, detail={"code": "group_not_found", "message": f"group not found: {group_id}"})

        scopes = group.doc.get("scopes") if isinstance(group.doc.get("scopes"), list) else []
        active_scope_key = str(group.doc.get("active_scope_key") or "")

        project_root: Optional[str] = None
        for sc in scopes:
            if not isinstance(sc, dict):
                continue
            sk = str(sc.get("scope_key") or "")
            if sk == active_scope_key:
                project_root = str(sc.get("url") or "")
                break
        if not project_root:
            if scopes and isinstance(scopes[0], dict):
                project_root = str(scopes[0].get("url") or "")
        if not project_root:
            return {"ok": False, "error": {"code": "NO_SCOPE", "message": "No scope attached to group. Use 'cccc attach <path>' first."}}

        root = Path(project_root).expanduser()
        if not root.exists() or not root.is_dir():
            return {"ok": False, "error": {"code": "INVALID_SCOPE", "message": f"Project root does not exist: {root}"}}

        # Write to existing file if present; otherwise create PROJECT.md.
        project_md_path = root / "PROJECT.md"
        if not project_md_path.exists():
            project_md_path_lower = root / "project.md"
            if project_md_path_lower.exists():
                project_md_path = project_md_path_lower

        try:
            atomic_write_text(project_md_path, str(req.content or ""), encoding="utf-8")
            content = project_md_path.read_text(encoding="utf-8", errors="replace")
            try:
                resp = await ctx.daemon({
                    "op": "context_sync",
                    "args": {
                        "group_id": group_id,
                        "by": "user",
                        "ops": [{"op": "coordination.brief.update", "project_brief_stale": True}],
                    },
                })
                if bool(resp.get("ok")):
                    await invalidate_context_read(group_id)
            except Exception:
                pass
            return {"ok": True, "result": {"found": True, "path": str(project_md_path), "content": content}}
        except Exception as e:
            return {"ok": False, "error": {"code": "WRITE_FAILED", "message": f"Failed to write PROJECT.md: {e}"}}

    def _prompt_kind_to_filename(kind: str) -> str:
        k = str(kind or "").strip().lower()
        if k == "preamble":
            return PREAMBLE_FILENAME
        if k == "help":
            return HELP_FILENAME
        raise HTTPException(status_code=400, detail={"code": "invalid_kind", "message": f"unknown prompt kind: {kind}"})

    def _builtin_prompt_markdown(kind: str) -> str:
        k = str(kind or "").strip().lower()
        if k == "preamble":
            return str(DEFAULT_PREAMBLE_BODY or "").strip()
        if k == "help":
            return str(load_builtin_help_markdown() or "").strip()
        return ""

    def _normalize_help_changed_blocks(raw: Any) -> list[str]:
        if not isinstance(raw, list):
            return []
        out: list[str] = []
        seen: set[str] = set()
        for item in raw:
            value = str(item or "").strip()
            if not value or value in seen:
                continue
            if value == "common":
                seen.add(value)
                out.append(value)
                continue
            if value in ("role:foreman", "role:peer"):
                seen.add(value)
                out.append(value)
                continue
            if value in {"pet", "voice_secretary"}:
                seen.add(value)
                out.append(value)
                continue
            if value.startswith("actor:"):
                actor_id = str(value[len("actor:"):]).strip()
                if actor_id:
                    normalized = f"actor:{actor_id}"
                    if normalized not in seen:
                        seen.add(normalized)
                        out.append(normalized)
        return out

    async def _list_running_actor_views(group_id: str) -> list[dict[str, Any]]:
        try:
            resp = await ctx.daemon({"op": "actor_list", "args": {"group_id": group_id, "include_unread": False}})
        except Exception:
            return []
        result = resp.get("result") if isinstance(resp, dict) else None
        actors = result.get("actors") if isinstance(result, dict) else None
        if not isinstance(actors, list):
            return []
        out: list[dict[str, Any]] = []
        for item in actors:
            if not isinstance(item, dict):
                continue
            aid = str(item.get("id") or "").strip()
            if not aid:
                continue
            if not coerce_bool(item.get("running"), default=False):
                continue
            out.append(item)
        return out

    def _help_update_reason_labels(*, actor: dict[str, Any], changed_blocks: list[str], editor_mode: str) -> list[str]:
        mode = str(editor_mode or "").strip().lower()
        if mode != "structured":
            return []
        aid = str(actor.get("id") or "").strip()
        role = str(actor.get("role") or "").strip().lower()
        if not aid:
            return []
        labels: list[str] = []
        seen: set[str] = set()

        def _add(label: str) -> None:
            if label and label not in seen:
                seen.add(label)
                labels.append(label)

        blocks = list(changed_blocks or [])
        if "common" in blocks:
            _add("common guidance")
        if "role:foreman" in blocks and role == "foreman":
            _add("foreman notes")
        if "role:peer" in blocks and role == "peer":
            _add("peer notes")
        if f"actor:{aid}" in blocks:
            _add("your actor note")
        if "voice_secretary" in blocks:
            internal_kind = str(actor.get("internal_kind") or "").strip()
            if internal_kind == "voice_secretary" or aid == "voice-secretary":
                _add("Voice Secretary guidance")
        return labels

    def _help_update_notify_copy(*, labels: list[str]) -> tuple[str, str]:
        reasons = [str(label or "").strip() for label in labels if str(label or "").strip()]
        if not reasons:
            return (
                "Help updated",
                "Group help changed. Run `cccc_help` now to refresh your effective playbook.",
            )
        if len(reasons) == 1:
            title = f"Help updated: {reasons[0]}"
        else:
            title = "Help updated: multiple sections"
        joined = ", ".join(reasons)
        message = (
            f"Updated: {joined}. Run `cccc_help` now to refresh your effective playbook; "
            "then update `cccc_agent_state` if your plan changes."
        )
        return title, message

    async def _notify_help_update(
        group_id: str,
        *,
        by: str,
        editor_mode: str,
        changed_blocks: list[str],
        content_changed: bool,
    ) -> list[str]:
        if not content_changed:
            return []
        normalized_blocks: set[str] = set()
        if str(editor_mode or "").strip().lower() == "structured":
            normalized_blocks = {str(item or "").strip() for item in list(changed_blocks or []) if str(item or "").strip()}
            if normalized_blocks and normalized_blocks.issubset({"pet"}):
                return []
        running = await _list_running_actor_views(group_id)
        if not running:
            return []

        target_reasons: dict[str, list[str]] = {}
        for actor in running:
            aid = str(actor.get("id") or "").strip()
            if not aid:
                continue
            reasons = _help_update_reason_labels(
                actor=actor,
                changed_blocks=changed_blocks,
                editor_mode=editor_mode,
            )
            if reasons:
                target_reasons[aid] = reasons

        if not target_reasons and normalized_blocks and normalized_blocks.issubset({"pet", "voice_secretary"}):
            return []

        if not target_reasons:
            for actor in running:
                aid = str(actor.get("id") or "").strip()
                if aid:
                    target_reasons[aid] = []

        notified: list[str] = []
        for aid in sorted(target_reasons.keys()):
            title, message = _help_update_notify_copy(labels=target_reasons.get(aid) or [])
            try:
                resp = await ctx.daemon({
                    "op": "system_notify",
                    "args": {
                        "group_id": group_id,
                        "by": "system",
                        "kind": "info",
                        "priority": "normal",
                        "title": title,
                        "message": message,
                        "target_actor_id": aid,
                        "requires_ack": False,
                    },
                })
                if isinstance(resp, dict) and resp.get("ok"):
                    notified.append(aid)
            except Exception:
                continue
        return notified

    @group_router.get("/prompts")
    async def prompts_get(group_id: str) -> Dict[str, Any]:
        """Get effective group guidance markdown (preamble/help) and override status."""
        group = load_group(group_id)
        if group is None:
            raise HTTPException(status_code=404, detail={"code": "group_not_found", "message": f"group not found: {group_id}"})

        def _one(kind: str) -> Dict[str, Any]:
            filename = _prompt_kind_to_filename(kind)
            pf = read_group_prompt_file(group, filename)
            if pf.found and isinstance(pf.content, str) and pf.content.strip():
                return {"kind": kind, "source": "home", "filename": filename, "path": pf.path, "content": str(pf.content)}
            return {
                "kind": kind,
                "source": "builtin",
                "filename": filename,
                "path": pf.path,
                "content": _builtin_prompt_markdown(kind),
            }

        return {
            "ok": True,
            "result": {
                "preamble": _one("preamble"),
                "help": _one("help"),
            },
        }

    @group_router.get("/pet-context")
    async def pet_context_get(group_id: str, fresh: bool = False, verbose: bool = False) -> Dict[str, Any]:
        """Get the injected context payload for the independent pet peer."""
        group = load_group(group_id)
        if group is None:
            raise HTTPException(status_code=404, detail={"code": "group_not_found", "message": f"group not found: {group_id}"})

        def _help_prompt() -> Dict[str, Any]:
            pf = read_group_prompt_file(group, HELP_FILENAME)
            content = ""
            prompt_source = "builtin"
            if pf.found and isinstance(pf.content, str) and pf.content.strip():
                content = str(pf.content)
                prompt_source = "home"
            else:
                content = load_pet_help_markdown(group)
            parsed = parse_help_markdown(content)
            persona = str(parsed.get("pet") or "").strip()
            return {
                "kind": "help",
                "source": prompt_source,
                "pet_source": "help" if persona else "default",
                "prompt_source": prompt_source,
                "filename": HELP_FILENAME,
                "path": pf.path,
                "content": content,
                "persona": persona,
            }

        if fresh:
            await invalidate_context_read(group_id, detail="summary")
        def _load_pet_context_payload() -> Dict[str, Any]:
            storage = ContextStorage(group)
            if fresh:
                _rebuild_summary_snapshot(group_id)
                snapshot = storage.load_summary_snapshot()
                context_payload = snapshot.get("result") if isinstance(snapshot.get("result"), dict) else {}
                if context_payload:
                    return context_payload
            return _get_summary_context_fast(storage, group_id=group_id)

        context_payload = await run_in_threadpool(_load_pet_context_payload)

        help_prompt = _help_prompt()
        return {
            "ok": True,
            "result": {
                **_build_pet_context_payload(
                    group,
                    help_prompt,
                    context_payload,
                    fresh=fresh,
                    verbose=verbose,
                ),
            },
        }

    @group_router.post("/pet-context/review")
    async def pet_context_review_post(group_id: str) -> Dict[str, Any]:
        group = load_group(group_id)
        if group is None:
            raise HTTPException(status_code=404, detail={"code": "group_not_found", "message": f"group not found: {group_id}"})
        if not is_desktop_pet_enabled(group):
            return {"ok": False, "error": {"code": "desktop_pet_disabled", "message": "desktop pet is disabled"}}
        if get_group_state(group) not in {"active", "idle"}:
            return {"ok": False, "error": {"code": "group_not_active", "message": "pet review requires active or idle group state"}}
        pet_actor = get_pet_actor(group)
        if not isinstance(pet_actor, dict) or not bool(pet_actor.get("enabled", True)):
            return {"ok": False, "error": {"code": "pet_actor_unavailable", "message": "pet actor is unavailable"}}
        accepted = await run_in_threadpool(
            request_manual_pet_review,
            group_id,
            reason="bubble_click",
        )
        if not accepted:
            return {"ok": False, "error": {"code": "pet_review_unavailable", "message": "pet review is currently unavailable"}}
        return {"ok": True, "result": {"accepted": True}}

    @group_router.post("/pet-decisions/outcome")
    async def pet_decision_outcome_post(group_id: str, req: PetDecisionOutcomeRequest) -> Dict[str, Any]:
        group = load_group(group_id)
        if group is None:
            raise HTTPException(status_code=404, detail={"code": "group_not_found", "message": f"group not found: {group_id}"})
        event = await run_in_threadpool(
            append_pet_decision_outcome,
            group,
            by=str(req.by or "user").strip() or "user",
            fingerprint=str(req.fingerprint or "").strip(),
            outcome=str(req.outcome or "").strip(),
            decision_id=str(req.decision_id or "").strip(),
            action_type=str(req.action_type or "").strip(),
            cooldown_ms=int(req.cooldown_ms or 0),
            source_event_id=str(req.source_event_id or "").strip(),
        )
        return {"ok": True, "result": {"event": event}}

    @group_router.put("/prompts/{kind}")
    async def prompts_put(group_id: str, kind: str, req: RepoPromptUpdateRequest) -> Dict[str, Any]:
        """Create or update a group prompt override file under CCCC_HOME."""
        group = load_group(group_id)
        if group is None:
            raise HTTPException(status_code=404, detail={"code": "group_not_found", "message": f"group not found: {group_id}"})

        filename = _prompt_kind_to_filename(kind)
        try:
            current_pf = read_group_prompt_file(group, filename)
            current_content = str(current_pf.content or "") if current_pf.found and isinstance(current_pf.content, str) else _builtin_prompt_markdown(kind)
            raw = str(req.content or "")
            editor_mode = str(req.editor_mode or "").strip().lower()
            changed_blocks = _normalize_help_changed_blocks(req.changed_blocks)
            content_changed = str(current_content) != str(raw if raw.strip() else _builtin_prompt_markdown(kind))
            if not raw.strip():
                pf = delete_group_prompt_file(group, filename)
                notified = []
                if str(kind).strip().lower() == "help":
                    notified = await _notify_help_update(
                        group_id,
                        by=str(req.by or "user").strip() or "user",
                        editor_mode="raw",
                        changed_blocks=[],
                        content_changed=content_changed,
                    )
                return {"ok": True, "result": {"kind": kind, "source": "builtin", "filename": filename, "path": pf.path, "content": _builtin_prompt_markdown(kind), "notified_actor_ids": notified}}
            pf = write_group_prompt_file(group, filename, raw)
            notified = []
            if str(kind).strip().lower() == "help":
                notified = await _notify_help_update(
                    group_id,
                    by=str(req.by or "user").strip() or "user",
                    editor_mode=editor_mode,
                    changed_blocks=changed_blocks,
                    content_changed=content_changed,
                )
            return {"ok": True, "result": {"kind": kind, "source": "home", "filename": filename, "path": pf.path, "content": pf.content or "", "notified_actor_ids": notified}}
        except Exception as e:
            return {"ok": False, "error": {"code": "WRITE_FAILED", "message": f"Failed to write {filename}: {e}"}}

    @group_router.delete("/prompts/{kind}")
    async def prompts_delete(group_id: str, kind: str, confirm: str = "") -> Dict[str, Any]:
        """Reset a group prompt override by deleting the CCCC_HOME file (requires confirm=kind)."""
        if str(confirm or "").strip().lower() != str(kind or "").strip().lower():
            raise HTTPException(status_code=400, detail={"code": "confirmation_required", "message": f"confirm must equal kind: {kind}"})

        group = load_group(group_id)
        if group is None:
            raise HTTPException(status_code=404, detail={"code": "group_not_found", "message": f"group not found: {group_id}"})

        filename = _prompt_kind_to_filename(kind)
        try:
            current_pf = read_group_prompt_file(group, filename)
            current_content = str(current_pf.content or "") if current_pf.found and isinstance(current_pf.content, str) else _builtin_prompt_markdown(kind)
            next_content = _builtin_prompt_markdown(kind)
            content_changed = str(current_content) != str(next_content)
            pf = delete_group_prompt_file(group, filename)
            notified = []
            if str(kind).strip().lower() == "help":
                notified = await _notify_help_update(
                    group_id,
                    by="user",
                    editor_mode="raw",
                    changed_blocks=[],
                    content_changed=content_changed,
                )
            return {"ok": True, "result": {"kind": kind, "source": "builtin", "filename": filename, "path": pf.path, "content": _builtin_prompt_markdown(kind), "notified_actor_ids": notified}}
        except Exception as e:
            return {"ok": False, "error": {"code": "DELETE_FAILED", "message": f"Failed to delete {filename}: {e}"}}

    @group_router.post("/context")
    async def group_context_sync(group_id: str, request: Request) -> Dict[str, Any]:
        """Update group context via batch operations (v3).

        Body: {"ops": [{"op": "coordination.brief.update", ...}, ...], "by": "user"}

        Supported ops:
        - coordination.brief.update / coordination.note.add
        - task.create/update/move/restore/delete
        - agent_state.update/clear
        - meta.merge (advanced, restricted keys)
        """
        try:
            body = await request.json()
        except Exception:
            raise HTTPException(status_code=400, detail={"code": "invalid_json", "message": "invalid JSON body"})

        ops = body.get("ops") if isinstance(body.get("ops"), list) else []
        by = str(body.get("by") or "user")
        dry_run = coerce_bool(body.get("dry_run"), default=False)

        resp = await ctx.daemon({
            "op": "context_sync",
            "args": {"group_id": group_id, "ops": ops, "by": by, "dry_run": dry_run}
        })
        # A successful context write with explicit ops must not leave later reads
        # attached to a stale inflight fetch started before the write.
        if bool(resp.get("ok")) and not dry_run and ops:
            await invalidate_context_read(group_id)
        return resp

    @group_router.get("/settings")
    async def group_settings_get(group_id: str) -> Dict[str, Any]:
        """Get group-scoped automation + delivery settings."""
        group = load_group(group_id)
        if group is None:
            raise HTTPException(status_code=404, detail={"code": "group_not_found", "message": f"group not found: {group_id}"})

        automation = group.doc.get("automation") if isinstance(group.doc.get("automation"), dict) else {}
        delivery = group.doc.get("delivery") if isinstance(group.doc.get("delivery"), dict) else {}
        features = group.doc.get("features") if isinstance(group.doc.get("features"), dict) else {}
        from ....kernel.terminal_transcript import get_terminal_transcript_settings
        from ....kernel.messaging import get_default_send_to

        tt = get_terminal_transcript_settings(group.doc)
        return {
            "ok": True,
            "result": {
                "settings": {
                    "default_send_to": get_default_send_to(group.doc),
                    "nudge_after_seconds": _safe_int(automation.get("nudge_after_seconds", 300), default=300, min_value=0),
                    "reply_required_nudge_after_seconds": _safe_int(automation.get("reply_required_nudge_after_seconds", 300), default=300, min_value=0),
                    "attention_ack_nudge_after_seconds": _safe_int(automation.get("attention_ack_nudge_after_seconds", 600), default=600, min_value=0),
                    "unread_nudge_after_seconds": _safe_int(automation.get("unread_nudge_after_seconds", 900), default=900, min_value=0),
                    "nudge_digest_min_interval_seconds": _safe_int(automation.get("nudge_digest_min_interval_seconds", 120), default=120, min_value=0),
                    "nudge_max_repeats_per_obligation": _safe_int(automation.get("nudge_max_repeats_per_obligation", 3), default=3, min_value=0),
                    "nudge_escalate_after_repeats": _safe_int(automation.get("nudge_escalate_after_repeats", 2), default=2, min_value=0),
                    "actor_idle_timeout_seconds": _safe_int(automation.get("actor_idle_timeout_seconds", 0), default=0, min_value=0),
                    "keepalive_delay_seconds": _safe_int(automation.get("keepalive_delay_seconds", 120), default=120, min_value=0),
                    "keepalive_max_per_actor": _safe_int(automation.get("keepalive_max_per_actor", 3), default=3, min_value=0),
                    "silence_timeout_seconds": _safe_int(automation.get("silence_timeout_seconds", 0), default=0, min_value=0),
                    "help_nudge_interval_seconds": _safe_int(automation.get("help_nudge_interval_seconds", 600), default=600, min_value=0),
                    "help_nudge_min_messages": _safe_int(automation.get("help_nudge_min_messages", 10), default=10, min_value=0),
                    "min_interval_seconds": _safe_int(delivery.get("min_interval_seconds", 0), default=0, min_value=0),
                    "auto_mark_on_delivery": coerce_bool(delivery.get("auto_mark_on_delivery"), default=False),
                    "terminal_transcript_visibility": str(tt.get("visibility") or "foreman"),
                    "terminal_transcript_notify_tail": coerce_bool(tt.get("notify_tail"), default=False),
                    "terminal_transcript_notify_lines": _safe_int(tt.get("notify_lines", 20), default=20, min_value=1, max_value=80),
                    "panorama_enabled": coerce_bool(features.get("panorama_enabled"), default=False),
                    "desktop_pet_enabled": coerce_bool(features.get("desktop_pet_enabled"), default=False),
                }
            }
        }

    @group_router.get("/desktop_pet/launch_token")
    async def group_desktop_pet_launch_token(request: Request, group_id: str) -> Dict[str, Any]:
        token = _request_access_token(request)
        if not token:
            # Empty password mode: no tokens configured → allow with empty token
            if not list_access_tokens():
                return {"ok": True, "result": {"token": ""}}
            raise HTTPException(
                status_code=403,
                detail={"code": "permission_denied", "message": "authentication required", "details": {}},
            )
        return {"ok": True, "result": {"token": token}}

    @group_router.get("/assistants")
    async def group_assistants_get(group_id: str) -> Dict[str, Any]:
        return await ctx.daemon({"op": "assistant_state", "args": {"group_id": group_id}})

    @group_router.get("/assistants/{assistant_id}")
    async def group_assistant_get(group_id: str, assistant_id: str, prompt_request_id: str = "") -> Dict[str, Any]:
        return await ctx.daemon(
            {
                "op": "assistant_state",
                "args": {
                    "group_id": group_id,
                    "assistant_id": str(assistant_id or "").strip(),
                    "prompt_request_id": str(prompt_request_id or "").strip(),
                },
            }
        )

    @group_router.put("/assistants/{assistant_id}/settings")
    async def group_assistant_settings_update(
        group_id: str,
        assistant_id: str,
        req: AssistantSettingsUpdateRequest,
    ) -> Dict[str, Any]:
        patch: Dict[str, Any] = {}
        if req.enabled is not None:
            patch["enabled"] = bool(req.enabled)
        if req.config is not None:
            patch["config"] = dict(req.config)
        if not patch:
            return {"ok": True, "result": {"message": "no changes"}}
        return await ctx.daemon(
            {
                "op": "assistant_settings_update",
                "args": {
                    "group_id": group_id,
                    "assistant_id": str(assistant_id or "").strip(),
                    "patch": patch,
                    "by": req.by,
                },
            }
        )

    @group_router.post("/assistants/{assistant_id}/status")
    async def group_assistant_status_update(
        group_id: str,
        assistant_id: str,
        req: AssistantStatusUpdateRequest,
    ) -> Dict[str, Any]:
        requested_assistant_id = str(req.assistant_id or assistant_id or "").strip()
        path_assistant_id = str(assistant_id or "").strip()
        if requested_assistant_id and path_assistant_id and requested_assistant_id != path_assistant_id:
            raise HTTPException(
                status_code=400,
                detail={
                    "code": "assistant_id_mismatch",
                    "message": "assistant_id in path and body must match",
                    "details": {"path_assistant_id": path_assistant_id, "body_assistant_id": requested_assistant_id},
                },
            )
        return await ctx.daemon(
            {
                "op": "assistant_status_update",
                "args": {
                    "group_id": group_id,
                    "assistant_id": path_assistant_id,
                    "lifecycle": req.lifecycle,
                    "health": dict(req.health),
                    "by": req.by,
                },
            }
        )

    @group_router.post("/assistants/voice_secretary/transcriptions")
    async def group_voice_secretary_transcription_create(
        group_id: str,
        req: AssistantVoiceTranscriptionRequest,
    ) -> Dict[str, Any]:
        return await ctx.daemon(
            {
                "op": "assistant_voice_transcribe",
                "args": {
                    "group_id": group_id,
                    "audio_base64": req.audio_base64,
                    "mime_type": req.mime_type,
                    "language": req.language,
                    "by": req.by,
                },
            }
        )

    @global_router.websocket("/groups/{group_id}/assistants/voice_secretary/transcriptions/ws")
    async def group_voice_secretary_transcription_ws(websocket: WebSocket, group_id: str) -> None:
        await websocket.accept()

        principal = resolve_websocket_principal(websocket)
        websocket.state.principal = principal

        auth_header = str((getattr(websocket, "headers", {}) or {}).get("authorization") or "").strip()
        has_header_token = auth_header.lower().startswith("bearer ") and bool(str(auth_header[7:] or "").strip())
        has_cookie_token = False
        try:
            cookies = getattr(websocket, "cookies", None) or {}
            has_cookie_token = bool(str(cookies.get("cccc_access_token") or "").strip())
        except Exception:
            has_cookie_token = False
        has_query_token = bool(str(websocket.query_params.get("token") or "").strip())
        if (has_header_token or has_cookie_token or has_query_token) and str(getattr(principal, "kind", "anonymous") or "anonymous") != "user" and websocket_tokens_active():
            try:
                await websocket.send_json({"type": "error", "ok": False, "error": {"code": "auth_required", "message": "Invalid or missing authentication token"}})
            except Exception:
                pass
            await websocket.close(code=4401)
            return

        try:
            check_group(websocket, group_id)
        except HTTPException as exc:
            detail = exc.detail if isinstance(exc.detail, dict) else {"code": "permission_denied", "message": str(exc.detail or "permission denied")}
            try:
                await websocket.send_json({"type": "error", "ok": False, "error": detail})
            except Exception:
                pass
            await websocket.close(code=1008)
            return

        if ctx.read_only:
            try:
                await websocket.send_json({"type": "error", "ok": False, "error": {"code": "read_only", "message": "web is read-only"}})
            except Exception:
                pass
            await websocket.close(code=1008)
            return

        streaming_session = None
        streaming_pcm16_audio = bytearray()
        streaming_pcm16_path: Path | None = None
        streaming_pcm16_bytes = 0
        streaming_sample_rate = 16000
        streaming_diarization_model_id = ""
        streaming_diarization_ready = False
        streaming_diarization_task: asyncio.Task[dict[str, Any]] | None = None
        streaming_last_diarization_ms = 0
        streaming_diarization_seq = 0
        streaming_stable_diarization_segments: list[dict[str, Any]] = []
        streaming_stable_speaker_embeddings: list[dict[str, Any]] = []
        streaming_client_session_id = ""

        def cleanup_streaming_pcm16() -> None:
            nonlocal streaming_pcm16_path, streaming_pcm16_bytes
            if streaming_pcm16_path is not None:
                with contextlib.suppress(Exception):
                    streaming_pcm16_path.unlink()
            streaming_pcm16_path = None
            streaming_pcm16_bytes = 0

        def append_streaming_pcm16(chunk: bytes) -> None:
            nonlocal streaming_pcm16_path, streaming_pcm16_bytes
            if not chunk:
                return
            if streaming_pcm16_path is None:
                tmp = tempfile.NamedTemporaryFile(prefix="cccc-voice-stream-", suffix=".pcm16", delete=False)
                streaming_pcm16_path = Path(tmp.name)
                tmp.close()
            with streaming_pcm16_path.open("ab") as handle:
                handle.write(chunk)
            streaming_pcm16_bytes += len(chunk)

        async def cleanup_streaming_state() -> None:
            nonlocal streaming_session, streaming_diarization_task, streaming_pcm16_audio
            if streaming_diarization_task is not None:
                if not streaming_diarization_task.done():
                    streaming_diarization_task.cancel()
                    with contextlib.suppress(asyncio.CancelledError):
                        await streaming_diarization_task
                streaming_diarization_task = None
            if streaming_session is not None:
                await streaming_session.close()
                streaming_session = None
            streaming_pcm16_audio = bytearray()
            cleanup_streaming_pcm16()

        try:
            while True:
                try:
                    payload = await websocket.receive_json()
                except WebSocketDisconnect:
                    return
                except Exception as exc:
                    await websocket.send_json({"type": "error", "ok": False, "error": {"code": "invalid_json", "message": str(exc)}})
                    continue

                message_type = str(payload.get("type") or "transcribe").strip()
                seq = payload.get("seq")
                if message_type == "start":
                    streaming_client_session_id = _safe_voice_session_id(payload.get("session_id") or payload.get("sessionId") or "")
                    group = load_group(group_id)
                    if group is None:
                        await websocket.send_json({"type": "error", "ok": False, "seq": seq, "error": {"code": "group_not_found", "message": f"group not found: {group_id}"}})
                        continue
                    assistants = group.doc.get("assistants") if isinstance(group.doc.get("assistants"), dict) else {}
                    assistant = assistants.get("voice_secretary") if isinstance(assistants.get("voice_secretary"), dict) else {}
                    config = assistant.get("config") if isinstance(assistant.get("config"), dict) else {}
                    selected_model_id = str(config.get("service_model_id") or "").strip()
                    streaming_diarization_model_id = str(config.get("service_diarization_model_id") or "").strip()
                    cleanup_streaming_pcm16()
                    streaming_pcm16_audio = bytearray()
                    streaming_sample_rate = int(payload.get("sample_rate") or 16000)
                    streaming_diarization_ready = bool(sherpa_diarization_status(streaming_diarization_model_id).get("ready"))
                    streaming_diarization_task = None
                    streaming_last_diarization_ms = 0
                    streaming_diarization_seq = 0
                    streaming_stable_diarization_segments = []
                    streaming_stable_speaker_embeddings = []
                    _write_voice_speaker_transcript_artifact(
                        group_id,
                        streaming_client_session_id,
                        {
                            "status": "recording",
                            "sample_rate": streaming_sample_rate,
                            "diarization_ready": streaming_diarization_ready,
                            "segments": [],
                            "speaker_transcript_segments": [],
                            "speaker_transcript_error": None,
                        },
                    )
                    _write_voice_meeting_session(
                        group_id,
                        streaming_client_session_id,
                        {
                            "status": "recording",
                            "sample_rate": streaming_sample_rate,
                            "diarization_ready": streaming_diarization_ready,
                            "latest_partial": "",
                            "error": None,
                        },
                    )
                    try:
                        streaming_session = await open_sherpa_streaming_session(selected_model_id)
                    except SherpaStreamingAsrError as exc:
                        _write_voice_meeting_session(
                            group_id,
                            streaming_client_session_id,
                            {
                                "status": "failed",
                                "error": {"code": exc.code, "message": exc.message, "details": exc.details},
                            },
                        )
                        await websocket.send_json({"type": "error", "ok": False, "seq": seq, "error": {"code": exc.code, "message": exc.message, "details": exc.details}})
                        continue
                    await websocket.send_json({"type": "ready", "ok": True, "seq": seq})
                    continue

                if message_type == "audio":
                    if streaming_session is None:
                        await websocket.send_json({"type": "error", "ok": False, "seq": seq, "error": {"code": "asr_stream_not_started", "message": "send start before audio"}})
                        continue
                    try:
                        audio_base64 = str(payload.get("audio_base64") or payload.get("audio_b64") or "")
                        sample_rate = int(payload.get("sample_rate") or 16000)
                        if sample_rate == 16000 and streaming_diarization_ready:
                            try:
                                pcm16_chunk = base64.b64decode(audio_base64, validate=True)
                                append_streaming_pcm16(pcm16_chunk)
                                streaming_pcm16_audio.extend(pcm16_chunk)
                                streaming_sample_rate = sample_rate
                            except Exception:
                                pass
                        if _VOICE_DIARIZATION_ENABLE_PROVISIONAL and streaming_diarization_task is not None and streaming_diarization_task.done():
                            try:
                                delta_result = streaming_diarization_task.result()
                                streaming_stable_diarization_segments = diarization_result_segments(delta_result)
                                streaming_stable_speaker_embeddings = diarization_result_speaker_embeddings(delta_result)
                                await websocket.send_json({
                                    "type": "diarization_delta",
                                    "ok": True,
                                    "seq": seq,
                                    "result": delta_result,
                                    "provisional": True,
                                })
                            except SherpaDiarizationError as exc:
                                await websocket.send_json({
                                    "type": "diarization_delta",
                                    "ok": False,
                                    "seq": seq,
                                    "error": {"code": exc.code, "message": exc.message, "details": exc.details},
                                    "provisional": True,
                                })
                            except Exception as exc:
                                await websocket.send_json({
                                    "type": "diarization_delta",
                                    "ok": False,
                                    "seq": seq,
                                    "error": {"code": "diarization_backend_failed", "message": str(exc), "details": {}},
                                    "provisional": True,
                                })
                            streaming_diarization_task = None
                        audio_duration_ms = _pcm16_duration_ms(streaming_pcm16_bytes, streaming_sample_rate)
                        if (
                            _VOICE_DIARIZATION_ENABLE_PROVISIONAL
                            and
                            streaming_diarization_ready
                            and streaming_pcm16_audio
                            and audio_duration_ms >= _VOICE_DIARIZATION_MIN_AUDIO_MS
                            and audio_duration_ms - streaming_last_diarization_ms >= _VOICE_DIARIZATION_INTERVAL_MS
                            and streaming_diarization_task is None
                        ):
                            prefix_audio = bytes(streaming_pcm16_audio)
                            previous_segments = list(streaming_stable_diarization_segments)
                            previous_speaker_embeddings = list(streaming_stable_speaker_embeddings)
                            streaming_diarization_seq += 1
                            task_seq = streaming_diarization_seq
                            streaming_last_diarization_ms = audio_duration_ms

                            streaming_diarization_task = asyncio.create_task(
                                run_provisional_diarization_prefix(
                                    prefix_audio,
                                    selected_model_id=streaming_diarization_model_id,
                                    sample_rate=streaming_sample_rate,
                                    run_seq=task_seq,
                                    audio_duration_ms=audio_duration_ms,
                                    previous_segments=previous_segments,
                                    previous_speaker_embeddings=previous_speaker_embeddings,
                                )
                            )
                        await streaming_session.send(
                            {
                                "type": "audio",
                                "seq": seq,
                                "sample_rate": sample_rate,
                                "audio_base64": audio_base64,
                            }
                        )
                        while True:
                            try:
                                event = await streaming_session.receive(timeout=0.01)
                            except SherpaStreamingAsrError as exc:
                                if exc.code == "asr_backend_timeout":
                                    break
                                await websocket.send_json({"type": "error", "ok": False, "seq": seq, "error": {"code": exc.code, "message": exc.message, "details": exc.details}})
                                break
                            event["ok"] = str(event.get("type") or "") != "error"
                            event_type = str(event.get("type") or "")
                            if event_type == "partial":
                                _write_voice_meeting_session(
                                    group_id,
                                    streaming_client_session_id,
                                    {
                                        "status": "recording",
                                        "latest_partial": str(event.get("text") or "").strip(),
                                    },
                                )
                            elif event_type == "final":
                                _write_voice_meeting_session(
                                    group_id,
                                    streaming_client_session_id,
                                    {
                                        "status": "recording",
                                        "latest_partial": "",
                                        "last_final_text": str(event.get("text") or "").strip(),
                                    },
                                )
                            await websocket.send_json(event)
                    except SherpaStreamingAsrError as exc:
                        await websocket.send_json({"type": "error", "ok": False, "seq": seq, "error": {"code": exc.code, "message": exc.message, "details": exc.details}})
                    continue

                if message_type in {"close", "stop"}:
                    if streaming_session is not None:
                        try:
                            await streaming_session.send({"type": "stop", "seq": seq})
                            while True:
                                event = await streaming_session.receive(timeout=5.0)
                                event["ok"] = str(event.get("type") or "") != "error"
                                if str(event.get("type") or "") == "closed":
                                    break
                                await websocket.send_json(event)
                            if streaming_diarization_task is not None:
                                if _VOICE_DIARIZATION_ENABLE_PROVISIONAL and streaming_diarization_task.done():
                                    try:
                                        delta_result = streaming_diarization_task.result()
                                        await websocket.send_json({
                                            "type": "diarization_delta",
                                            "ok": True,
                                            "seq": seq,
                                            "result": delta_result,
                                            "provisional": True,
                                        })
                                    except SherpaDiarizationError as exc:
                                        await websocket.send_json({
                                            "type": "diarization_delta",
                                            "ok": False,
                                            "seq": seq,
                                            "error": {"code": exc.code, "message": exc.message, "details": exc.details},
                                            "provisional": True,
                                        })
                                    except Exception as exc:
                                        await websocket.send_json({
                                            "type": "diarization_delta",
                                            "ok": False,
                                            "seq": seq,
                                            "error": {"code": "diarization_backend_failed", "message": str(exc), "details": {}},
                                            "provisional": True,
                                        })
                                else:
                                    streaming_diarization_task.cancel()
                                    with contextlib.suppress(asyncio.CancelledError):
                                        await streaming_diarization_task
                                streaming_diarization_task = None
                            if streaming_pcm16_path is not None and streaming_pcm16_bytes > 0 and streaming_diarization_ready:
                                audio_duration_ms = _pcm16_duration_ms(streaming_pcm16_bytes, streaming_sample_rate)
                                persisted_pcm16_path = _persist_voice_meeting_pcm16(
                                    group_id,
                                    streaming_client_session_id,
                                    streaming_pcm16_path,
                                )
                                artifact_path = _write_voice_speaker_transcript_artifact(
                                    group_id,
                                    streaming_client_session_id,
                                    {
                                        "status": "separating_speakers",
                                        "sample_rate": streaming_sample_rate,
                                        "audio_duration_ms": audio_duration_ms,
                                        "segments": [],
                                        "speaker_transcript_segments": [],
                                        "speaker_transcript_error": None,
                                    },
                                )
                                _write_voice_meeting_session(
                                    group_id,
                                    streaming_client_session_id,
                                    {
                                        "status": "separating_speakers",
                                        "audio_duration_ms": audio_duration_ms,
                                        "audio_path": str(persisted_pcm16_path),
                                        "diarization_artifact_path": artifact_path,
                                    },
                                )
                                await websocket.send_json({
                                    "type": "diarization_status",
                                    "ok": True,
                                    "seq": seq,
                                    "status": "separating_speakers",
                                    "artifact_path": artifact_path,
                                })
                                asyncio.create_task(_run_voice_meeting_diarization_background(
                                    group_id=group_id,
                                    session_id=streaming_client_session_id,
                                    pcm16_path=persisted_pcm16_path,
                                    selected_model_id=streaming_diarization_model_id,
                                    selected_asr_model_id=selected_model_id,
                                    sample_rate=streaming_sample_rate,
                                    audio_duration_ms=audio_duration_ms,
                                ))
                            else:
                                _write_voice_meeting_session(
                                    group_id,
                                    streaming_client_session_id,
                                    {
                                        "status": "closed",
                                        "audio_duration_ms": _pcm16_duration_ms(streaming_pcm16_bytes, streaming_sample_rate),
                                    },
                                )
                            await websocket.send_json({"type": "closed", "ok": True, "seq": seq})
                        except SherpaStreamingAsrError as exc:
                            _write_voice_meeting_session(
                                group_id,
                                streaming_client_session_id,
                                {
                                    "status": "failed",
                                    "error": {"code": exc.code, "message": exc.message, "details": exc.details},
                                },
                            )
                            await websocket.send_json({"type": "error", "ok": False, "seq": seq, "error": {"code": exc.code, "message": exc.message, "details": exc.details}})
                        finally:
                            await cleanup_streaming_state()
                            await websocket.close(code=1000)
                        return
                    await websocket.send_json({"type": "closed", "ok": True, "seq": seq})
                    await websocket.close(code=1000)
                    return
                if message_type != "transcribe":
                    await websocket.send_json({"type": "error", "ok": False, "seq": seq, "error": {"code": "unsupported_message", "message": f"unsupported message type: {message_type}"}})
                    continue

                resp = await ctx.daemon(
                    {
                        "op": "assistant_voice_transcribe",
                        "args": {
                            "group_id": group_id,
                            "audio_base64": str(payload.get("audio_base64") or payload.get("audio_b64") or ""),
                            "mime_type": str(payload.get("mime_type") or payload.get("mimeType") or "application/octet-stream"),
                            "language": str(payload.get("language") or ""),
                            "by": str(payload.get("by") or "user").strip() or "user",
                        },
                    }
                )
                if not bool(resp.get("ok")):
                    await websocket.send_json({"type": "transcript", "ok": False, "seq": seq, "error": resp.get("error") or {"code": "transcribe_failed", "message": "transcribe failed"}})
                    continue
                await websocket.send_json({"type": "transcript", "ok": True, "seq": seq, "result": resp.get("result") or {}})
        except WebSocketDisconnect:
            return
        finally:
            await cleanup_streaming_state()

    @group_router.post("/assistants/voice_secretary/models/install")
    async def group_voice_secretary_model_install(
        group_id: str,
        req: AssistantVoiceModelInstallRequest,
    ) -> Dict[str, Any]:
        return await ctx.daemon(
            {
                "op": "assistant_voice_model_install",
                "args": {
                    "group_id": group_id,
                    "model_id": req.model_id,
                    "by": req.by,
                    "background": req.background,
                },
            }
        )

    @group_router.post("/assistants/voice_secretary/models/remove")
    async def group_voice_secretary_model_remove(
        group_id: str,
        req: AssistantVoiceModelRemoveRequest,
    ) -> Dict[str, Any]:
        return await ctx.daemon(
            {
                "op": "assistant_voice_model_remove",
                "args": {
                    "group_id": group_id,
                    "model_id": req.model_id,
                    "by": req.by,
                },
            }
        )

    @group_router.post("/assistants/voice_secretary/runtime/install")
    async def group_voice_secretary_runtime_install(
        group_id: str,
        req: AssistantVoiceRuntimeInstallRequest,
    ) -> Dict[str, Any]:
        return await ctx.daemon(
            {
                "op": "assistant_voice_runtime_install",
                "args": {
                    "group_id": group_id,
                    "runtime_id": req.runtime_id,
                    "by": req.by,
                    "background": req.background,
                },
            }
        )

    @group_router.post("/assistants/voice_secretary/runtime/remove")
    async def group_voice_secretary_runtime_remove(
        group_id: str,
        req: AssistantVoiceRuntimeRemoveRequest,
    ) -> Dict[str, Any]:
        return await ctx.daemon(
            {
                "op": "assistant_voice_runtime_remove",
                "args": {
                    "group_id": group_id,
                    "runtime_id": req.runtime_id,
                    "by": req.by,
                },
            }
        )

    @group_router.get("/assistants/voice_secretary/sessions/latest")
    async def group_voice_secretary_latest_session_get(group_id: str, document_path: str = "") -> Dict[str, Any]:
        return {
            "ok": True,
            "result": {
                "group_id": group_id,
                "session": _read_latest_voice_meeting_session(group_id, document_path=document_path),
            },
        }

    @group_router.get("/assistants/voice_secretary/sessions/{session_id}")
    async def group_voice_secretary_session_get(group_id: str, session_id: str) -> Dict[str, Any]:
        return {"ok": True, "result": {"group_id": group_id, "session": _read_voice_meeting_session(group_id, session_id)}}

    @group_router.post("/assistants/voice_secretary/transcript_segments")
    async def group_voice_secretary_transcript_segment_append(
        group_id: str,
        req: AssistantVoiceTranscriptSegmentRequest,
    ) -> Dict[str, Any]:
        segment_path = _append_voice_meeting_segment(
            group_id,
            req.session_id,
            {
                "segment_id": req.segment_id,
                "text": req.text,
                "language": req.language,
                "is_final": req.is_final,
                "start_ms": req.start_ms,
                "end_ms": req.end_ms,
                "speaker_label": req.speaker_label,
                "document_path": req.document_path,
                "trigger": dict(req.trigger),
                "by": req.by,
            },
        )
        trigger_kind = str(req.trigger.get("trigger_kind") or "").strip().lower()
        _write_voice_meeting_session(
            group_id,
            req.session_id,
            {
                "status": (
                    "closed"
                    if req.flush and trigger_kind in {"push_to_talk_stop", "service_transcript"}
                    else "recording"
                ),
                "language": req.language,
                "document_path": req.document_path,
                "latest_partial": "",
            },
        )
        if not req.flush:
            return {
                "ok": True,
                "result": {
                    "group_id": group_id,
                    "session_id": req.session_id,
                    "segment": (
                        {
                            "segment_id": req.segment_id,
                            "session_id": req.session_id,
                            "group_id": group_id,
                            "text": req.text,
                            "language": req.language,
                            "is_final": req.is_final,
                            "start_ms": req.start_ms,
                            "end_ms": req.end_ms,
                            "speaker_label": req.speaker_label,
                            "by": req.by,
                        }
                        if str(req.text or "").strip()
                        else {}
                    ),
                    "segment_path": segment_path,
                    "document": None,
                    "document_updated": False,
                    "input_event": {},
                    "input_event_created": False,
                    "input_notify_emitted": False,
                    "actor_woken": False,
                    "actor_wake_error": "",
                    "actor_notify_delivered": False,
                    "actor_notify_delivery_error": "",
                    "deferred_document_update": True,
                },
            }
        daemon_text = req.text
        daemon_segment_id = req.segment_id
        if not str(daemon_text or "").strip():
            daemon_text = _voice_meeting_transcript_text(group_id, req.session_id)
            if str(daemon_text or "").strip() and not str(daemon_segment_id or "").strip():
                daemon_segment_id = f"flush-{_safe_voice_session_id(req.session_id)}"
        return await ctx.daemon(
            {
                "op": "assistant_voice_transcript_append",
                "args": {
                    "group_id": group_id,
                    "session_id": req.session_id,
                    "segment_id": daemon_segment_id,
                    "document_path": req.document_path,
                    "text": daemon_text,
                    "language": req.language,
                    "is_final": req.is_final,
                    "flush": req.flush,
                    "start_ms": req.start_ms,
                    "end_ms": req.end_ms,
                    "speaker_label": req.speaker_label,
                    "trigger": dict(req.trigger),
                    "by": req.by,
                },
            }
        )

    @group_router.get("/assistants/voice_secretary/documents")
    async def group_voice_secretary_documents_get(group_id: str, include_archived: bool = False) -> Dict[str, Any]:
        return await ctx.daemon(
            {
                "op": "assistant_voice_document_list",
                "args": {
                    "group_id": group_id,
                    "include_archived": bool(include_archived),
                },
            }
        )

    @group_router.put("/assistants/voice_secretary/documents")
    async def group_voice_secretary_document_save(
        group_id: str,
        req: AssistantVoiceDocumentSaveRequest,
    ) -> Dict[str, Any]:
        args = {
            "group_id": group_id,
            "document_path": req.document_path,
            "workspace_path": req.workspace_path,
            "title": req.title,
            "status": req.status,
            "create_new": req.create_new,
            "by": req.by,
        }
        if req.content is not None:
            args["content"] = req.content
        return await ctx.daemon(
            {
                "op": "assistant_voice_document_save",
                "args": args,
            }
        )

    @group_router.post("/assistants/voice_secretary/documents/select")
    async def group_voice_secretary_document_select_by_path(
        group_id: str,
        req: AssistantVoiceDocumentSaveRequest,
    ) -> Dict[str, Any]:
        return await ctx.daemon(
            {
                "op": "assistant_voice_document_select",
                "args": {
                    "group_id": group_id,
                    "document_path": str(req.document_path or req.workspace_path or "").strip(),
                    "by": req.by,
                },
            }
        )

    @group_router.post("/assistants/voice_secretary/documents/instructions")
    async def group_voice_secretary_document_instruction_by_path(
        group_id: str,
        req: AssistantVoiceDocumentInstructionRequest,
    ) -> Dict[str, Any]:
        return await ctx.daemon(
            {
                "op": "assistant_voice_document_instruction",
                "args": {
                    "group_id": group_id,
                    "document_path": str(req.document_path or "").strip(),
                    "instruction": req.instruction,
                    "source_text": req.source_text,
                    "trigger": dict(req.trigger),
                    "by": req.by,
                },
            }
        )

    @group_router.post("/assistants/voice_secretary/inputs")
    async def group_voice_secretary_input_append(
        group_id: str,
        req: AssistantVoiceInputRequest,
    ) -> Dict[str, Any]:
        return await ctx.daemon(
            {
                "op": "assistant_voice_input_append",
                "args": {
                    "group_id": group_id,
                    "kind": req.kind,
                    "text": req.text,
                    "instruction": req.instruction,
                    "source_text": req.source_text,
                    "document_path": req.document_path,
                    "voice_transcript": req.voice_transcript,
                    "composer_text": req.composer_text,
                    "request_id": req.request_id,
                    "operation": req.operation,
                    "composer_context": dict(req.composer_context),
                    "composer_snapshot_hash": req.composer_snapshot_hash,
                    "language": req.language,
                    "trigger": dict(req.trigger),
                    "by": req.by,
                },
            }
        )

    @group_router.post("/assistants/voice_secretary/prompt_drafts/ack")
    async def group_voice_secretary_prompt_draft_ack(
        group_id: str,
        req: AssistantVoicePromptDraftAckRequest,
    ) -> Dict[str, Any]:
        return await ctx.daemon(
            {
                "op": "assistant_voice_prompt_draft_ack",
                "args": {
                    "group_id": group_id,
                    "request_id": req.request_id,
                    "status": req.status,
                    "by": req.by,
                },
            }
        )

    @group_router.post("/assistants/voice_secretary/ask_requests/clear")
    async def group_voice_secretary_ask_requests_clear(
        group_id: str,
        req: AssistantVoiceAskRequestsClearRequest,
    ) -> Dict[str, Any]:
        return await ctx.daemon(
            {
                "op": "assistant_voice_ask_requests_clear",
                "args": {
                    "group_id": group_id,
                    "keep_active": req.keep_active,
                    "by": req.by,
                },
            }
        )

    @group_router.post("/assistants/voice_secretary/documents/archive")
    async def group_voice_secretary_document_archive_by_path(
        group_id: str,
        req: AssistantVoiceDocumentSaveRequest,
    ) -> Dict[str, Any]:
        return await ctx.daemon(
            {
                "op": "assistant_voice_document_archive",
                "args": {
                    "group_id": group_id,
                    "document_path": str(req.document_path or req.workspace_path or "").strip(),
                    "by": req.by,
                },
            }
        )

    @group_router.put("/settings")
    async def group_settings_update(group_id: str, req: GroupSettingsRequest) -> Dict[str, Any]:
        """Update group-scoped automation + delivery settings."""
        patch: Dict[str, Any] = {}
        if req.default_send_to is not None:
            patch["default_send_to"] = str(req.default_send_to)
        if req.nudge_after_seconds is not None:
            patch["nudge_after_seconds"] = max(0, req.nudge_after_seconds)
        if req.reply_required_nudge_after_seconds is not None:
            patch["reply_required_nudge_after_seconds"] = max(0, req.reply_required_nudge_after_seconds)
        if req.attention_ack_nudge_after_seconds is not None:
            patch["attention_ack_nudge_after_seconds"] = max(0, req.attention_ack_nudge_after_seconds)
        if req.unread_nudge_after_seconds is not None:
            patch["unread_nudge_after_seconds"] = max(0, req.unread_nudge_after_seconds)
        if req.nudge_digest_min_interval_seconds is not None:
            patch["nudge_digest_min_interval_seconds"] = max(0, req.nudge_digest_min_interval_seconds)
        if req.nudge_max_repeats_per_obligation is not None:
            patch["nudge_max_repeats_per_obligation"] = max(0, req.nudge_max_repeats_per_obligation)
        if req.nudge_escalate_after_repeats is not None:
            patch["nudge_escalate_after_repeats"] = max(0, req.nudge_escalate_after_repeats)
        if req.actor_idle_timeout_seconds is not None:
            patch["actor_idle_timeout_seconds"] = max(0, req.actor_idle_timeout_seconds)
        if req.keepalive_delay_seconds is not None:
            patch["keepalive_delay_seconds"] = max(0, req.keepalive_delay_seconds)
        if req.keepalive_max_per_actor is not None:
            patch["keepalive_max_per_actor"] = max(0, req.keepalive_max_per_actor)
        if req.silence_timeout_seconds is not None:
            patch["silence_timeout_seconds"] = max(0, req.silence_timeout_seconds)
        if req.help_nudge_interval_seconds is not None:
            patch["help_nudge_interval_seconds"] = max(0, req.help_nudge_interval_seconds)
        if req.help_nudge_min_messages is not None:
            patch["help_nudge_min_messages"] = max(0, req.help_nudge_min_messages)
        if req.min_interval_seconds is not None:
            patch["min_interval_seconds"] = max(0, req.min_interval_seconds)
        if req.auto_mark_on_delivery is not None:
            patch["auto_mark_on_delivery"] = bool(req.auto_mark_on_delivery)

        # Terminal transcript policy (group-scoped)
        if req.terminal_transcript_visibility is not None:
            patch["terminal_transcript_visibility"] = str(req.terminal_transcript_visibility)
        if req.terminal_transcript_notify_tail is not None:
            patch["terminal_transcript_notify_tail"] = bool(req.terminal_transcript_notify_tail)
        if req.terminal_transcript_notify_lines is not None:
            patch["terminal_transcript_notify_lines"] = max(1, min(80, int(req.terminal_transcript_notify_lines)))

        if req.panorama_enabled is not None:
            patch["panorama_enabled"] = bool(req.panorama_enabled)
        if req.desktop_pet_enabled is not None:
            patch["desktop_pet_enabled"] = bool(req.desktop_pet_enabled)

        if not patch:
            return {"ok": True, "result": {"message": "no changes"}}

        return await ctx.daemon({
            "op": "group_settings_update",
            "args": {"group_id": group_id, "patch": patch, "by": req.by}
        })

    @group_router.get("/automation")
    async def group_automation_get(group_id: str) -> Dict[str, Any]:
        """Get group automation rules + snippets + runtime status."""
        return await ctx.daemon({"op": "group_automation_state", "args": {"group_id": group_id, "by": "user"}})

    @group_router.put("/automation")
    async def group_automation_update(group_id: str, req: GroupAutomationRequest) -> Dict[str, Any]:
        """Update group automation rules + snippets."""
        ruleset = AutomationRuleSet(rules=req.rules, snippets=req.snippets).model_dump()
        return await ctx.daemon(
            {
                "op": "group_automation_update",
                "args": {"group_id": group_id, "ruleset": ruleset, "expected_version": req.expected_version, "by": req.by},
            }
        )

    @group_router.post("/automation/manage")
    async def group_automation_manage(group_id: str, req: GroupAutomationManageRequest) -> Dict[str, Any]:
        """Manage group automation incrementally via actions."""
        return await ctx.daemon(
            {
                "op": "group_automation_manage",
                "args": {
                    "group_id": group_id,
                    "actions": [a for a in req.actions if isinstance(a, dict)],
                    "expected_version": req.expected_version,
                    "by": req.by,
                },
            }
        )

    @group_router.post("/automation/reset_baseline")
    async def group_automation_reset_baseline(group_id: str, req: GroupAutomationResetBaselineRequest) -> Dict[str, Any]:
        """Reset group automation rules/snippets to baseline defaults."""
        return await ctx.daemon(
            {
                "op": "group_automation_reset_baseline",
                "args": {
                    "group_id": group_id,
                    "expected_version": req.expected_version,
                    "by": req.by,
                },
            }
        )

    @group_router.post("/attach")
    async def group_attach(group_id: str, req: AttachRequest) -> Dict[str, Any]:
        return await ctx.daemon({"op": "attach", "args": {"path": req.path, "by": req.by, "group_id": group_id}})

    @group_router.delete("/scopes/{scope_key}")
    async def group_detach_scope(group_id: str, scope_key: str, by: str = "user") -> Dict[str, Any]:
        """Detach a scope from a group."""
        return await ctx.daemon({"op": "group_detach_scope", "args": {"group_id": group_id, "scope_key": scope_key, "by": by}})

    @group_router.get("/ledger/tail")
    async def ledger_tail(
        group_id: str,
        lines: int = 50,
        limit: Optional[int] = None,
        kind: str = "all",
        with_read_status: bool = False,
        with_ack_status: bool = False,
        with_obligation_status: bool = False,
    ) -> Dict[str, Any]:
        def _load() -> Dict[str, Any]:
            group = load_group(group_id)
            if group is None:
                raise HTTPException(status_code=404, detail={"code": "group_not_found", "message": f"group not found: {group_id}"})
            effective_limit = int(limit) if limit is not None else int(lines)
            if effective_limit <= 0:
                return {"ok": True, "result": {"events": [], "has_more": False, "count": 0}}
            kind_filter = str(kind or "all").strip().lower()
            if kind_filter in {"chat", "notify"}:
                from ....kernel.inbox import search_messages

                events, has_more = search_messages(
                    group,
                    query="",
                    kind_filter=kind_filter,  # type: ignore[arg-type]
                    by_filter="",
                    before_id="",
                    after_id="",
                    limit=effective_limit,
                )
                events = list(reversed(events))
            else:
                raw_lines = read_last_lines(group.ledger_path, effective_limit)
                events = []
                for ln in raw_lines:
                    try:
                        events.append(json.loads(ln))
                    except Exception:
                        continue

                events = events[-effective_limit:] if effective_limit > 0 else events
                has_more = False
                if effective_limit > 0 and events:
                    first_event_id = str(events[0].get("id") or "").strip()
                    if first_event_id:
                        from ....kernel.inbox import search_messages

                        older_events, older_has_more = search_messages(
                            group,
                            query="",
                            kind_filter="all",
                            by_filter="",
                            before_id=first_event_id,
                            after_id="",
                            limit=1,
                        )
                        has_more = bool(older_has_more or older_events)

            if with_read_status or with_ack_status or with_obligation_status:
                _apply_ledger_event_statuses(
                    events,
                    _collect_ledger_event_statuses(
                        group,
                        events,
                        with_read_status=with_read_status,
                        with_ack_status=with_ack_status,
                        with_obligation_status=with_obligation_status,
                    ),
                )

            return {"ok": True, "result": {"events": events, "has_more": has_more, "count": len(events)}}

        return await run_in_threadpool(_load)

    @group_router.get("/ledger/search")
    async def ledger_search(
        group_id: str,
        q: str = "",
        kind: str = "all",
        by: str = "",
        before: str = "",
        after: str = "",
        limit: int = 50,
        with_read_status: bool = False,
        with_ack_status: bool = False,
        with_obligation_status: bool = False,
    ) -> Dict[str, Any]:
        """Search and paginate messages in the ledger."""
        def _load() -> Dict[str, Any]:
            group = load_group(group_id)
            if group is None:
                raise HTTPException(status_code=404, detail={"code": "group_not_found", "message": f"group not found: {group_id}"})

            from ....kernel.inbox import search_messages, get_read_status_batch

            clamped_limit = max(1, min(200, limit))
            kind_filter = kind if kind in ("all", "chat", "notify") else "all"

            events, has_more = search_messages(
                group,
                query=q,
                kind_filter=kind_filter,  # type: ignore
                by_filter=by,
                before_id=before,
                after_id=after,
                limit=clamped_limit,
            )

            if with_read_status or with_ack_status or with_obligation_status:
                _apply_ledger_event_statuses(
                    events,
                    _collect_ledger_event_statuses(
                        group,
                        events,
                        with_read_status=with_read_status,
                        with_ack_status=with_ack_status,
                        with_obligation_status=with_obligation_status,
                    ),
                )

            return {
                "ok": True,
                "result": {
                    "events": events,
                    "has_more": has_more,
                    "count": len(events),
                },
            }

        return await run_in_threadpool(_load)

    @group_router.get("/ledger/window")
    async def ledger_window(
        group_id: str,
        center: str,
        kind: str = "chat",
        before: int = 30,
        after: int = 30,
        with_read_status: bool = False,
        with_ack_status: bool = False,
        with_obligation_status: bool = False,
    ) -> Dict[str, Any]:
        """Return a bounded window of events around a center event_id."""
        def _load() -> Dict[str, Any]:
            group = load_group(group_id)
            if group is None:
                raise HTTPException(status_code=404, detail={"code": "group_not_found", "message": f"group not found: {group_id}"})

            from ....kernel.inbox import find_event, search_messages, get_read_status_batch

            center_id = str(center or "").strip()
            if not center_id:
                raise HTTPException(status_code=400, detail={"code": "missing_center", "message": "missing center event_id"})

            center_event = find_event(group, center_id)
            if center_event is None:
                raise HTTPException(status_code=404, detail={"code": "event_not_found", "message": f"event not found: {center_id}"})

            clamped_before = max(0, min(200, int(before)))
            clamped_after = max(0, min(200, int(after)))
            kind_filter = kind if kind in ("all", "chat", "notify") else "chat"

            if kind_filter == "chat" and str(center_event.get("kind") or "") != "chat.message":
                raise HTTPException(status_code=400, detail={"code": "invalid_center_kind", "message": "center event kind must be chat.message for kind=chat"})

            before_events, has_more_before = search_messages(
                group,
                query="",
                kind_filter=kind_filter,  # type: ignore
                before_id=center_id,
                limit=clamped_before,
            )
            after_events, has_more_after = search_messages(
                group,
                query="",
                kind_filter=kind_filter,  # type: ignore
                after_id=center_id,
                limit=clamped_after,
            )

            events = [*before_events, center_event, *after_events]

            if with_read_status or with_ack_status or with_obligation_status:
                _apply_ledger_event_statuses(
                    events,
                    _collect_ledger_event_statuses(
                        group,
                        events,
                        with_read_status=with_read_status,
                        with_ack_status=with_ack_status,
                        with_obligation_status=with_obligation_status,
                    ),
                )

            return {
                "ok": True,
                "result": {
                    "center_id": center_id,
                    "center_index": len(before_events),
                    "events": events,
                    "has_more_before": has_more_before,
                    "has_more_after": has_more_after,
                    "count": len(events),
                },
            }

        return await run_in_threadpool(_load)

    @group_router.get("/events/{event_id}/read_status")
    async def event_read_status(group_id: str, event_id: str) -> Dict[str, Any]:
        """Get read status for a specific event (which actors have read it)."""
        def _load() -> Dict[str, Any]:
            group = load_group(group_id)
            if group is None:
                raise HTTPException(status_code=404, detail={"code": "group_not_found", "message": f"group not found: {group_id}"})

            from ....kernel.inbox import get_read_status

            status = get_read_status(group, event_id)
            return {"ok": True, "result": {"event_id": event_id, "read_status": status}}

        return await run_in_threadpool(_load)

    @group_router.post("/ledger/statuses")
    async def ledger_statuses(group_id: str, request: Request) -> Dict[str, Any]:
        """Batch load read/ack/obligation status for specific event ids."""

        def _load(event_ids: list[str]) -> Dict[str, Any]:
            group = load_group(group_id)
            if group is None:
                raise HTTPException(status_code=404, detail={"code": "group_not_found", "message": f"group not found: {group_id}"})

            from ....kernel.ledger_index import lookup_events_by_ids

            normalized_ids: list[str] = []
            seen_ids: set[str] = set()
            for item in event_ids:
                event_id = str(item or "").strip()
                if not event_id or event_id in seen_ids:
                    continue
                seen_ids.add(event_id)
                normalized_ids.append(event_id)
            if not normalized_ids:
                return {"ok": True, "result": {"statuses": {}}}

            events = [
                ev
                for ev in lookup_events_by_ids(group.ledger_path, normalized_ids)
                if isinstance(ev, dict) and str(ev.get("kind") or "") == "chat.message"
            ]
            statuses = _collect_ledger_event_statuses(group, events)
            return {"ok": True, "result": {"statuses": statuses}}

        try:
            payload = await request.json()
        except Exception:
            payload = {}
        event_ids = payload.get("event_ids") if isinstance(payload, dict) else []
        if not isinstance(event_ids, list):
            raise HTTPException(status_code=400, detail={"code": "invalid_event_ids", "message": "event_ids must be a list"})
        if len(event_ids) > 500:
            raise HTTPException(status_code=400, detail={"code": "too_many_event_ids", "message": "event_ids must not exceed 500 items"})
        return await run_in_threadpool(_load, [str(item or "") for item in event_ids])

    @global_router.websocket("/groups/{group_id}/presentation/browser_surface/ws")
    async def group_presentation_browser_surface_ws(websocket: WebSocket, group_id: str) -> None:
        await websocket.accept()
        slot_id = str(websocket.query_params.get("slot") or "").strip()

        principal = resolve_websocket_principal(websocket)
        websocket.state.principal = principal

        auth_header = str((getattr(websocket, "headers", {}) or {}).get("authorization") or "").strip()
        has_header_token = auth_header.lower().startswith("bearer ") and bool(str(auth_header[7:] or "").strip())
        has_cookie_token = False
        try:
            cookies = getattr(websocket, "cookies", None) or {}
            has_cookie_token = bool(str(cookies.get("cccc_access_token") or "").strip())
        except Exception:
            has_cookie_token = False
        has_query_token = bool(str(websocket.query_params.get("token") or "").strip())
        if (has_header_token or has_cookie_token or has_query_token) and str(getattr(principal, "kind", "anonymous") or "anonymous") != "user" and websocket_tokens_active():
            try:
                await websocket.send_json({"ok": False, "error": {"code": "auth_required", "message": "Invalid or missing authentication token"}})
            except Exception:
                pass
            await websocket.close(code=4401)
            return

        try:
            check_group(websocket, group_id)
        except HTTPException as exc:
            detail = exc.detail if isinstance(exc.detail, dict) else {"code": "permission_denied", "message": str(exc.detail or "permission denied")}
            try:
                await websocket.send_json({"ok": False, "error": detail})
            except Exception:
                pass
            await websocket.close(code=1008)
            return

        if ctx.read_only:
            try:
                await websocket.send_json(
                    {
                        "ok": False,
                        "error": {
                            "code": "read_only_browser_surface",
                            "message": "Browser surface control is disabled in read-only mode.",
                            "details": {},
                        },
                    }
                )
            except Exception:
                pass
            try:
                await websocket.close(code=1000)
            except Exception:
                pass
            return

        group = load_group(group_id)
        if group is None:
            await websocket.send_json({"ok": False, "error": {"code": "group_not_found", "message": f"group not found: {group_id}"}})
            await websocket.close(code=1008)
            return

        try:
            ep = get_daemon_endpoint()
            transport = str(ep.get("transport") or "").strip().lower()
            if transport == "tcp":
                host = str(ep.get("host") or "127.0.0.1").strip() or "127.0.0.1"
                port = int(ep.get("port") or 0)
                reader, writer = await asyncio.open_connection(host, port, limit=_PRESENTATION_BROWSER_STREAM_LIMIT_BYTES)
            else:
                sock_path = ctx.home / "daemon" / "ccccd.sock"
                path = str(ep.get("path") or sock_path)
                reader, writer = await asyncio.open_unix_connection(path, limit=_PRESENTATION_BROWSER_STREAM_LIMIT_BYTES)
        except Exception:
            await websocket.send_json({"ok": False, "error": {"code": "daemon_unavailable", "message": "ccccd unavailable"}})
            await websocket.close(code=1011)
            return

        try:
            req = {"op": "presentation_browser_attach", "args": {"group_id": group_id, "slot": slot_id, "by": "user"}}
            writer.write((json.dumps(req, ensure_ascii=False) + "\n").encode("utf-8"))
            await writer.drain()
            line = await reader.readline()
            try:
                resp = json.loads(line.decode("utf-8", errors="replace"))
            except Exception:
                resp = {}
            if not isinstance(resp, dict) or not resp.get("ok"):
                err = resp.get("error") if isinstance(resp.get("error"), dict) else {"code": "browser_surface_attach_failed", "message": "browser surface attach failed"}
                await websocket.send_json({"ok": False, "error": err})
                await websocket.close(code=1008)
                return

            async def _pump_out() -> None:
                while True:
                    line = await reader.readline()
                    if not line:
                        break
                    await websocket.send_text(line.decode("utf-8", errors="replace").rstrip("\n"))

            async def _pump_in() -> None:
                while True:
                    raw = await websocket.receive_text()
                    if not raw:
                        continue
                    writer.write((raw + "\n").encode("utf-8", errors="replace"))
                    await writer.drain()

            out_task = asyncio.create_task(_pump_out())
            in_task = asyncio.create_task(_pump_in())
            try:
                done, pending = await asyncio.wait({out_task, in_task}, return_when=asyncio.FIRST_COMPLETED)
                for task in done:
                    try:
                        _ = task.result()
                    except Exception:
                        pass
                for task in pending:
                    task.cancel()
                try:
                    await asyncio.gather(*pending, return_exceptions=True)
                except Exception:
                    pass
            except WebSocketDisconnect:
                pass
        finally:
            try:
                writer.close()
                await writer.wait_closed()
            except Exception:
                pass

    @group_router.get("/ledger/stream")
    async def ledger_stream(group_id: str) -> StreamingResponse:
        from ..streams import sse_ledger_tail, create_sse_response
        group = load_group(group_id)
        if group is None:
            raise HTTPException(status_code=404, detail={"code": "group_not_found", "message": f"group not found: {group_id}"})
        return create_sse_response(sse_ledger_tail(group.ledger_path))

    async def _serve_headless_snapshot(group_id: str) -> Dict[str, Any]:
        group = load_group(group_id)
        if group is None:
            raise HTTPException(status_code=404, detail={"code": "group_not_found", "message": f"group not found: {group_id}"})
        return {"ok": True, "result": _read_headless_snapshot(group)}

    async def _serve_headless_stream(group_id: str, replay: bool = True) -> StreamingResponse:
        from ..streams import create_sse_response, sse_jsonl_tail_shared

        group = load_group(group_id)
        if group is None:
            raise HTTPException(status_code=404, detail={"code": "group_not_found", "message": f"group not found: {group_id}"})
        replay_lines = read_headless_replay_lines(group.path) if replay else []
        return create_sse_response(
            sse_jsonl_tail_shared(
                headless_events_path(group.path),
                event_name="headless",
                heartbeat_s=30.0,
                poll_interval_s=0.05,
                initial_lines=replay_lines,
            )
        )

    @group_router.get("/headless/snapshot")
    async def headless_snapshot(group_id: str) -> Dict[str, Any]:
        return await _serve_headless_snapshot(group_id)

    @group_router.get("/codex/snapshot")
    async def codex_snapshot_legacy(group_id: str) -> Dict[str, Any]:
        return await _serve_headless_snapshot(group_id)

    @group_router.get("/headless/stream")
    async def headless_stream(group_id: str, replay: bool = True) -> StreamingResponse:
        return await _serve_headless_stream(group_id, replay=replay)

    @group_router.get("/codex/stream")
    async def codex_stream_legacy(group_id: str, replay: bool = True) -> StreamingResponse:
        return await _serve_headless_stream(group_id, replay=replay)

    return [global_router, group_router]


def register_group_routes(app: FastAPI, *, ctx: RouteContext) -> None:
    """Backward-compatible wrapper for app.py registration."""
    for router in create_routers(ctx):
        app.include_router(router)
