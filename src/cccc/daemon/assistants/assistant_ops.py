"""Thin control seam for first-party built-in assistants."""

from __future__ import annotations

import base64
import binascii
import hashlib
import json
import logging
import os
import re
import shutil
import threading
import time
import unicodedata
import uuid
from datetime import datetime, timezone
from pathlib import Path, PurePosixPath
from typing import Any, Callable, Dict, Optional

from ...contracts.v1 import BuiltinAssistant, DaemonError, DaemonResponse, SystemNotifyData
from ...kernel.actors import find_foreman, list_visible_actors
from ...kernel.group import Group, load_group
from ...kernel.inbox import iter_events_reverse
from ...kernel.ledger import append_event
from ...kernel.permissions import require_group_permission
from ...kernel.pet_actor import is_desktop_pet_enabled
from ...kernel.prompt_files import resolve_active_scope_root
from ...kernel.voice_secretary_actor import VOICE_SECRETARY_ACTOR_ID, get_voice_secretary_actor
from ...paths import ensure_home
from ...util.conv import coerce_bool
from ...util.fs import atomic_write_json, atomic_write_text, read_json
from ...util.time import parse_utc_iso, utc_now_iso
from ..actors.actor_profile_runtime import resolve_linked_actor_before_start
from ..messaging.delivery import dispatch_system_notify_event_to_actor, emit_system_notify
from .voice_secretary_runtime_ops import (
    capture_voice_secretary_actor_state,
    is_voice_secretary_actor_running,
    restore_voice_secretary_actor_state,
    stop_voice_secretary_actor_runtime,
    sync_voice_secretary_actor_from_foreman,
    voice_secretary_runtime_changed,
)
from .voice_prompt_refine import build_voice_prompt_refine_input_text
from .voice_models import (
    VoiceModelError,
    begin_voice_model_install,
    get_voice_model_status,
    install_voice_model,
    list_voice_models,
)
from .voice_runtime_deps import (
    VOICE_RUNTIME_ID_SHERPA_ONNX_STREAMING,
    VoiceRuntimeDepsError,
    get_voice_runtime_status,
    install_voice_runtime_deps,
    list_voice_runtime_statuses,
)
from .sherpa_streaming_asr import sherpa_streaming_backend_status
from .voice_service_runtime import (
    VoiceServiceRuntimeError,
    read_voice_service_state,
    stop_voice_service,
    transcribe_voice_audio,
)


logger = logging.getLogger(__name__)

_VOICE_MODEL_INSTALL_LOCK = threading.Lock()
_VOICE_MODEL_INSTALL_THREADS: Dict[str, threading.Thread] = {}
_VOICE_RUNTIME_INSTALL_LOCK = threading.Lock()
_VOICE_RUNTIME_INSTALL_THREADS: Dict[str, threading.Thread] = {}


ASSISTANT_ID_PET = "pet"
ASSISTANT_ID_VOICE_SECRETARY = "voice_secretary"

_STATE_SCHEMA = 1
_STATE_FILENAME = "assistants.json"
_MAX_TRANSCRIPT_CHARS = 32_000
_MAX_TRANSCRIPT_SESSION_CHARS = 16_000
_MAX_PROMPT_REFINE_CHARS = 16_000
_MAX_PROMPT_DRAFT_CHARS = 16_000
_MAX_AUDIO_BYTES = 25 * 1024 * 1024
_MAX_VOICE_DOCUMENT_CHARS = 200_000
_MAX_VOICE_DOCUMENTS = 100
_DEFAULT_AUTO_DOCUMENT_QUIET_MS = 5_000
_DEFAULT_AUTO_DOCUMENT_MIN_CHARS = 700
_DEFAULT_AUTO_DOCUMENT_FAST_MIN_CHARS = 120
_DEFAULT_AUTO_DOCUMENT_MAX_WINDOW_SECONDS = 120
_MIN_AUTO_DOCUMENT_QUIET_MS = 1_000
_MIN_AUTO_DOCUMENT_MAX_WINDOW_SECONDS = 10
_DEFAULT_AUTO_DOCUMENT_MAX_WINDOW_CHARS = 12_000
_DEFAULT_AUTO_DOCUMENT_MIN_WINDOW_SEGMENTS = 3
_DEFAULT_VOICE_DOCUMENT_DIR = "docs/voice-secretary"
_VOICE_INPUT_NUDGE_RETRY_SECONDS = (180, 360, 720)
_VOICE_PENDING_PROMPT_DRAFT_STALE_SECONDS = 1_800
_VOICE_IDLE_REVIEW_FLUSH_THRESHOLD = 8
_VOICE_IDLE_REVIEW_GROUP_COOLDOWN_SECONDS = 300
_VOICE_IDLE_REVIEW_STOP_TRIGGER_KINDS = {"push_to_talk_stop"}
_VOICE_PREVIOUS_INPUT_TAIL_MAX_CHARS = 240
_VOICE_PREVIOUS_INPUT_TAIL_MAX_FRAGMENTS = 3
_VOICE_PREVIOUS_INPUT_TAIL_SCAN_ITEMS = 80
_VOICE_SECRETARY_TASK_INTENTS = {"document_instruction", "secretary_task", "peer_task", "task_instruction", "action_request", "mixed"}
_VOICE_DOCUMENT_MODES = {"meeting_minutes", "speech_summary", "interview_notes", "research_brief", "general_notes"}
_VOICE_TRANSCRIPT_TINY_FILLERS = {
    "嗯",
    "啊",
    "呃",
    "哦",
    "好",
    "对",
    "はい",
    "えー",
    "あの",
    "ok",
    "okay",
    "yeah",
    "yes",
    "uh",
    "um",
}

_VALID_LIFECYCLES = {"disabled", "idle", "running", "working", "waiting", "failed"}
_VALID_VOICE_CAPTURE_MODES = {"browser", "service"}
_VALID_VOICE_RECOGNITION_BACKENDS = {"mock", "assistant_service_local_asr", "browser_asr", "external_provider_asr"}
_VALID_RECOGNITION_LANGUAGE_RE = re.compile(r"^(auto|[A-Za-z]{2,3}(?:-[A-Za-z0-9]{2,8})*)$")
_VOICE_DOCUMENT_INSTRUCTION_PATTERNS = (
    re.compile(r"\b(?:new|create|start)\s+(?:a\s+)?(?:separate\s+|new\s+|fresh\s+)?(?:working\s+)?(?:document|notes?|minutes)\b", re.I),
    re.compile(r"\b(?:archive|rename|revise|rewrite|update)\s+(?:this\s+|the\s+)?(?:working\s+)?(?:document|notes?|minutes)\b", re.I),
    re.compile(r"\b(?:write|put|record|capture)\s+(?:this|that|it|the following|these|.+?)\s+(?:in|into|to)\s+(?:the\s+|a\s+)?(?:(?:separate|new|fresh|working)\s+)*(?:document|notes?|minutes)\b", re.I),
    re.compile(r"\bturn\s+.+?\s+into\s+(?:a\s+)?(?:document|notes?|minutes)\b", re.I),
    re.compile(r"(?:新建|创建|另开).*(?:文档|工作稿|笔记|会议纪要)"),
    re.compile(r"(?:归档|重命名|修改|改写|更新).*(?:文档|工作稿|笔记|会议纪要)"),
    re.compile(r"(?:整理|记录|写|放).*(?:到|进).*(?:文档|工作稿|笔记|会议纪要)"),
    re.compile(r"(?:文書|ドキュメント|議事録).*(?:作成|新規|更新|修正|アーカイブ)"),
)
_VOICE_SECRETARY_TASK_PATTERNS = (
    re.compile(r"^\s*(?:please\s+)?(?:inspect|check|review|investigate|analy[sz]e|summari[sz]e|organize|draft|compare|explain|look\s+into)\b", re.I),
    re.compile(r"\b(?:help\s+me\s+)?(?:inspect|check|review|investigate|analy[sz]e|summari[sz]e|organize|draft|compare|explain|look\s+into)\b", re.I),
    re.compile(r"(?:帮我|请你|麻烦|辛苦).*(?:看一下|检查|调查|分析|整理|总结|归纳|梳理|起草|比较|处理)"),
    re.compile(r"(?:調べて|確認|分析|整理|要約|まとめ|下書き|比較|説明)", re.I),
)
_VOICE_PEER_TASK_PATTERNS = (
    re.compile(r"\b(?:dispatch|send|tell|notify|ask)\s+(?:@?[\w.-]+|the\s+foreman|foreman|peer|agent)\b", re.I),
    re.compile(r"^\s*(?:please\s+)?(?:fix|implement|update|modify|change|refactor|build|deploy|commit|test|restart|start|stop)\b", re.I),
    re.compile(r"\b(?:create|open)\s+(?:a\s+)?task\b", re.I),
    re.compile(r"\bassign\s+.+\b", re.I),
    re.compile(r"(?:修复|实现|改代码|提交|部署|运行测试|启动|停止|重启|分配).*(?:任务|代码|服务|agent|智能体|actor)?"),
    re.compile(r"(?:告诉|通知|发给|派发|发送给|让).*(?:领班|foreman|agent|智能体|actor|同事|peer|管理员)"),
    re.compile(r"(?:送って|伝えて|通知|依頼).*(?:Foreman|エージェント|担当|peer|actor)", re.I),
)


_ASSISTANT_DEFAULTS: Dict[str, Dict[str, Any]] = {
    ASSISTANT_ID_PET: {
        "assistant_id": ASSISTANT_ID_PET,
        "kind": "pet",
        "enabled": False,
        "principal": "assistant:pet",
        "lifecycle": "disabled",
        "health": {},
        "policy": {
            "action_allowlist": ["pet.review", "pet.profile_refresh"],
            "requires_user_confirmation": [],
        },
        "config": {
            "settings_source": "group.features.desktop_pet_enabled",
        },
        "ui": {
            "surface": "pet_panel",
            "composer_control": "pet",
        },
    },
    ASSISTANT_ID_VOICE_SECRETARY: {
        "assistant_id": ASSISTANT_ID_VOICE_SECRETARY,
        "kind": "voice_secretary",
        "enabled": False,
        "principal": "assistant:voice_secretary",
        "lifecycle": "disabled",
        "health": {},
        "policy": {
            "action_allowlist": ["voice_secretary.request"],
            "requires_user_confirmation": [],
        },
        "config": {
            "capture_mode": "browser",
            "recognition_backend": "browser_asr",
            "recognition_language": "auto",
            "retention_ttl_seconds": 900,
            "auto_document_enabled": True,
            "document_default_dir": _DEFAULT_VOICE_DOCUMENT_DIR,
            "auto_document_quiet_ms": _DEFAULT_AUTO_DOCUMENT_QUIET_MS,
            "auto_document_min_chars": _DEFAULT_AUTO_DOCUMENT_MIN_CHARS,
            "auto_document_max_window_seconds": _DEFAULT_AUTO_DOCUMENT_MAX_WINDOW_SECONDS,
            "service_model_id": "",
            "tts_enabled": False,
        },
        "ui": {
            "surface": "composer_quick_strip",
            "composer_control": "voice_secretary_workspace",
        },
    },
}

_VOICE_CONFIG_KEYS = {
    "capture_mode",
    "recognition_backend",
    "recognition_language",
    "retention_ttl_seconds",
    "auto_document_enabled",
    "document_default_dir",
    "auto_document_quiet_ms",
    "auto_document_min_chars",
    "auto_document_max_window_seconds",
    "service_model_id",
    "tts_enabled",
}


def _error(code: str, message: str, *, details: Optional[Dict[str, Any]] = None) -> DaemonResponse:
    return DaemonResponse(ok=False, error=DaemonError(code=code, message=message, details=(details or {})))


def _normalize_assistant_id(value: Any) -> str:
    return str(value or "").strip().lower()


def _assistant_principal(assistant_id: str) -> str:
    default = _ASSISTANT_DEFAULTS.get(assistant_id)
    if isinstance(default, dict):
        return str(default.get("principal") or f"assistant:{assistant_id}").strip()
    return f"assistant:{assistant_id}"


def _state_path(group: Group):
    return group.path / "state" / _STATE_FILENAME


def _load_runtime_state(group: Group) -> Dict[str, Any]:
    payload = read_json(_state_path(group))
    if not isinstance(payload, dict) or int(payload.get("schema") or 0) != _STATE_SCHEMA:
        return {
            "schema": _STATE_SCHEMA,
            "group_id": group.group_id,
            "assistants": {},
            "voice_sessions": {},
            "voice_prompt_drafts": {},
            "voice_prompt_requests": {},
            "voice_ask_requests": {},
        }
    assistants = payload.get("assistants") if isinstance(payload.get("assistants"), dict) else {}
    voice_sessions = payload.get("voice_sessions") if isinstance(payload.get("voice_sessions"), dict) else {}
    voice_prompt_drafts = payload.get("voice_prompt_drafts") if isinstance(payload.get("voice_prompt_drafts"), dict) else {}
    voice_prompt_requests = payload.get("voice_prompt_requests") if isinstance(payload.get("voice_prompt_requests"), dict) else {}
    voice_ask_requests = payload.get("voice_ask_requests") if isinstance(payload.get("voice_ask_requests"), dict) else {}
    return {
        "schema": _STATE_SCHEMA,
        "group_id": group.group_id,
        "assistants": {str(k): v for k, v in assistants.items() if isinstance(v, dict)},
        "voice_sessions": {str(k): v for k, v in voice_sessions.items() if isinstance(v, dict)},
        "voice_prompt_drafts": {str(k): v for k, v in voice_prompt_drafts.items() if isinstance(v, dict)},
        "voice_prompt_requests": {str(k): v for k, v in voice_prompt_requests.items() if isinstance(v, dict)},
        "voice_ask_requests": {str(k): v for k, v in voice_ask_requests.items() if isinstance(v, dict)},
    }


def _voice_retention_ttl_seconds(group: Group) -> int:
    default_config = _ASSISTANT_DEFAULTS[ASSISTANT_ID_VOICE_SECRETARY].get("config") or {}
    stored = _stored_assistant_settings(group, ASSISTANT_ID_VOICE_SECRETARY)
    stored_config = stored.get("config") if isinstance(stored.get("config"), dict) else {}
    try:
        value = int(stored_config.get("retention_ttl_seconds", default_config.get("retention_ttl_seconds", 900)))
    except Exception:
        value = 900
    return min(max(0, value), 86_400)


def _save_runtime_state(group: Group, payload: Dict[str, Any]) -> None:
    normalized = {
        "schema": _STATE_SCHEMA,
        "group_id": group.group_id,
        "assistants": payload.get("assistants") if isinstance(payload.get("assistants"), dict) else {},
        "voice_sessions": payload.get("voice_sessions") if isinstance(payload.get("voice_sessions"), dict) else {},
        "voice_prompt_drafts": payload.get("voice_prompt_drafts") if isinstance(payload.get("voice_prompt_drafts"), dict) else {},
        "voice_prompt_requests": payload.get("voice_prompt_requests") if isinstance(payload.get("voice_prompt_requests"), dict) else {},
        "voice_ask_requests": payload.get("voice_ask_requests") if isinstance(payload.get("voice_ask_requests"), dict) else {},
    }
    _state_path(group).parent.mkdir(parents=True, exist_ok=True)
    atomic_write_json(_state_path(group), normalized, indent=2)


def _stored_assistant_settings(group: Group, assistant_id: str) -> Dict[str, Any]:
    assistants = group.doc.get("assistants") if isinstance(group.doc.get("assistants"), dict) else {}
    entry = assistants.get(assistant_id) if isinstance(assistants.get(assistant_id), dict) else {}
    return dict(entry)


def _runtime_assistant_entry(runtime_state: Dict[str, Any], assistant_id: str) -> Dict[str, Any]:
    assistants = runtime_state.get("assistants") if isinstance(runtime_state.get("assistants"), dict) else {}
    entry = assistants.get(assistant_id) if isinstance(assistants.get(assistant_id), dict) else {}
    return dict(entry)


def _normalize_voice_config(raw: Any, *, base: Dict[str, Any]) -> Dict[str, Any]:
    if not isinstance(raw, dict):
        raise ValueError("config must be an object")
    unknown = set(str(key) for key in raw.keys()) - _VOICE_CONFIG_KEYS
    if unknown:
        raise ValueError(f"invalid voice config keys: {', '.join(sorted(unknown))}")

    out = dict(base)
    if "capture_mode" in raw:
        value = str(raw.get("capture_mode") or "").strip()
        if value not in _VALID_VOICE_CAPTURE_MODES:
            raise ValueError("capture_mode must be 'browser' or 'service'")
        out["capture_mode"] = value
    if "recognition_backend" in raw:
        value = str(raw.get("recognition_backend") or "").strip()
        if value not in _VALID_VOICE_RECOGNITION_BACKENDS:
            raise ValueError("invalid recognition_backend")
        out["recognition_backend"] = value
    if "recognition_language" in raw:
        value = str(raw.get("recognition_language") or "auto").strip() or "auto"
        if not _VALID_RECOGNITION_LANGUAGE_RE.match(value):
            raise ValueError("recognition_language must be 'auto' or a BCP-47-like language tag")
        out["recognition_language"] = value
    if "retention_ttl_seconds" in raw:
        try:
            ttl = int(raw.get("retention_ttl_seconds"))
        except Exception as exc:
            raise ValueError("retention_ttl_seconds must be an integer") from exc
        out["retention_ttl_seconds"] = min(max(0, ttl), 86_400)
    if "auto_document_enabled" in raw:
        out["auto_document_enabled"] = coerce_bool(raw.get("auto_document_enabled"), default=True)
    if "document_default_dir" in raw:
        out["document_default_dir"] = _safe_voice_document_rel_dir(raw.get("document_default_dir"))
    if "auto_document_quiet_ms" in raw:
        try:
            quiet_ms = int(raw.get("auto_document_quiet_ms"))
        except Exception as exc:
            raise ValueError("auto_document_quiet_ms must be an integer") from exc
        out["auto_document_quiet_ms"] = min(max(_MIN_AUTO_DOCUMENT_QUIET_MS, quiet_ms), 60_000)
    if "auto_document_min_chars" in raw:
        try:
            min_chars = int(raw.get("auto_document_min_chars"))
        except Exception as exc:
            raise ValueError("auto_document_min_chars must be an integer") from exc
        out["auto_document_min_chars"] = min(max(40, min_chars), 8_000)
    if "auto_document_max_window_seconds" in raw:
        try:
            max_window_seconds = int(raw.get("auto_document_max_window_seconds"))
        except Exception as exc:
            raise ValueError("auto_document_max_window_seconds must be an integer") from exc
        out["auto_document_max_window_seconds"] = min(max(_MIN_AUTO_DOCUMENT_MAX_WINDOW_SECONDS, max_window_seconds), 300)
    if "service_model_id" in raw:
        value = str(raw.get("service_model_id") or "").strip()
        if value and not re.match(r"^[a-z0-9][a-z0-9._-]{0,63}$", value):
            raise ValueError("service_model_id must be a simple slug")
        out["service_model_id"] = value
    if "tts_enabled" in raw:
        out["tts_enabled"] = coerce_bool(raw.get("tts_enabled"), default=False)
    return out


def _effective_voice_config(stored_config: Any, *, base: Dict[str, Any]) -> Dict[str, Any]:
    if not isinstance(stored_config, dict):
        return dict(base)
    # Older builds wrote now-removed Voice Secretary fields and very aggressive
    # chunking values into group.yaml. Read paths must sanitize them too, not only
    # settings-update paths, otherwise stale group state silently overrides the
    # current capture policy.
    safe = {str(key): value for key, value in stored_config.items() if str(key) in _VOICE_CONFIG_KEYS}
    return _normalize_voice_config(safe, base=base)


def _effective_assistant(group: Group, assistant_id: str, *, runtime_state: Optional[Dict[str, Any]] = None) -> Dict[str, Any]:
    default = _ASSISTANT_DEFAULTS.get(assistant_id)
    if not isinstance(default, dict):
        raise ValueError(f"unknown assistant: {assistant_id}")

    stored = _stored_assistant_settings(group, assistant_id)
    runtime = _runtime_assistant_entry(runtime_state or _load_runtime_state(group), assistant_id)
    default_config = dict(default.get("config") if isinstance(default.get("config"), dict) else {})
    config = dict(default_config)
    stored_config = stored.get("config") if isinstance(stored.get("config"), dict) else {}
    if assistant_id == ASSISTANT_ID_VOICE_SECRETARY:
        config = _effective_voice_config(stored_config, base=default_config)
    else:
        config.update(stored_config)

    if assistant_id == ASSISTANT_ID_PET:
        enabled = is_desktop_pet_enabled(group)
    else:
        enabled = coerce_bool(stored.get("enabled"), default=bool(default.get("enabled")))

    lifecycle = str(runtime.get("lifecycle") or "").strip().lower()
    if lifecycle not in _VALID_LIFECYCLES:
        lifecycle = "idle" if enabled else "disabled"
    if not enabled:
        lifecycle = "disabled"

    health = runtime.get("health") if isinstance(runtime.get("health"), dict) else {}
    if assistant_id == ASSISTANT_ID_VOICE_SECRETARY:
        service_state = read_voice_service_state(group)
        service_used = bool(service_state.get("pid") or service_state.get("status") or service_state.get("port"))
        service_backend_selected = str(config.get("recognition_backend") or "").strip() == "assistant_service_local_asr"
        if service_used or service_backend_selected:
            health = dict(health)
            selected_model_id = str(config.get("service_model_id") or "").strip()
            managed_model = get_voice_model_status(selected_model_id) if selected_model_id else {}
            streaming_backend = sherpa_streaming_backend_status(selected_model_id)
            health["service"] = {
                "status": str(service_state.get("status") or ("not_started" if service_backend_selected else "")),
                "pid": service_state.get("pid"),
                "host": service_state.get("host"),
                "port": service_state.get("port"),
                "alive": bool(service_state.get("alive")),
                "asr_command_configured": bool(
                    service_state.get("asr_command_configured")
                    or str(os.environ.get("CCCC_VOICE_SECRETARY_ASR_COMMAND") or "").strip()
                ),
                "asr_mock_configured": bool(
                    service_state.get("asr_mock_configured")
                    or str(os.environ.get("CCCC_VOICE_SECRETARY_ASR_MOCK_TEXT") or "").strip()
                ),
                "selected_model_id": selected_model_id,
                "managed_model": managed_model,
                "streaming_backend": streaming_backend,
                "last_error": service_state.get("last_error") if isinstance(service_state.get("last_error"), dict) else {},
                "updated_at": str(service_state.get("updated_at") or ""),
            }

    assistant = BuiltinAssistant.model_validate(
        {
            **default,
            "enabled": enabled,
            "lifecycle": lifecycle,
            "health": health,
            "config": config,
        }
    )
    return assistant.model_dump()


def _require_status_permission(group: Group, *, assistant_id: str, by: str) -> None:
    who = str(by or "").strip()
    if not who or who == "user" or who == _assistant_principal(assistant_id):
        return
    require_group_permission(group, by=who, action="group.settings_update")


def _require_confirmation_permission(group: Group, *, by: str) -> None:
    who = str(by or "").strip()
    if not who or who == "user":
        return
    require_group_permission(group, by=who, action="group.update")


def _require_document_write_permission(group: Group, *, by: str) -> None:
    who = str(by or "").strip()
    if not who or who == "user" or who == _assistant_principal(ASSISTANT_ID_VOICE_SECRETARY) or who == VOICE_SECRETARY_ACTOR_ID:
        return
    require_group_permission(group, by=who, action="group.update")


def _clean_multiline_text(value: Any, *, max_len: int = _MAX_TRANSCRIPT_CHARS) -> str:
    lines = []
    for line in str(value or "").strip().splitlines():
        clean = " ".join(line.strip().split())
        if clean:
            lines.append(clean)
    text = "\n".join(lines) if lines else " ".join(str(value or "").strip().split())
    return text[:max_len]


_VOICE_ASK_REQUEST_STATUSES = {"pending", "working", "done", "needs_user", "failed", "handed_off"}
_VOICE_ASK_FINAL_STATUSES = {"done", "needs_user", "failed", "handed_off"}


def _clean_voice_request_id(value: Any, *, prefix: str) -> str:
    raw = str(value or "").strip()
    if not raw:
        raw = f"{prefix}-{uuid.uuid4().hex}"
    safe = re.sub(r"[^A-Za-z0-9_.:-]+", "-", raw).strip(".:-")[:128]
    return safe or f"{prefix}-{uuid.uuid4().hex}"


def _clean_voice_prompt_request_id(value: Any) -> str:
    return _clean_voice_request_id(value, prefix="voice-prompt")


def _clean_voice_ask_request_id(value: Any) -> str:
    return _clean_voice_request_id(value, prefix="voice-ask")


def _voice_prompt_draft_public(record: Dict[str, Any]) -> Dict[str, Any]:
    out = {
        "schema": 1,
        "request_id": str(record.get("request_id") or ""),
        "status": str(record.get("status") or "pending"),
        "operation": str(record.get("operation") or "append_to_composer_end"),
        "draft_text": str(record.get("draft_text") or ""),
        "draft_preview": str(record.get("draft_preview") or ""),
        "summary": str(record.get("summary") or ""),
        "composer_snapshot_hash": str(record.get("composer_snapshot_hash") or ""),
        "created_at": str(record.get("created_at") or ""),
        "updated_at": str(record.get("updated_at") or ""),
    }
    return {key: value for key, value in out.items() if value not in ("", None, [])}


def _latest_pending_voice_prompt_draft(group: Group, *, runtime_state: Optional[Dict[str, Any]] = None) -> Dict[str, Any]:
    state = runtime_state or _load_runtime_state(group)
    drafts = state.get("voice_prompt_drafts") if isinstance(state.get("voice_prompt_drafts"), dict) else {}
    pending = [
        dict(item)
        for item in drafts.values()
        if isinstance(item, dict) and str(item.get("status") or "").strip().lower() == "pending"
    ]
    if not pending:
        return {}
    pending.sort(key=lambda item: str(item.get("updated_at") or item.get("created_at") or ""))
    return _voice_prompt_draft_public(pending[-1])


def _pending_voice_prompt_draft_by_request(
    group: Group,
    *,
    request_id: str,
    runtime_state: Optional[Dict[str, Any]] = None,
) -> Dict[str, Any]:
    rid = str(request_id or "").strip()
    if not rid:
        return _latest_pending_voice_prompt_draft(group, runtime_state=runtime_state)
    state = runtime_state or _load_runtime_state(group)
    drafts = state.get("voice_prompt_drafts") if isinstance(state.get("voice_prompt_drafts"), dict) else {}
    record = drafts.get(rid) if isinstance(drafts.get(rid), dict) else {}
    if str(record.get("status") or "").strip().lower() != "pending":
        return {}
    return _voice_prompt_draft_public(dict(record))


def _trim_voice_prompt_drafts(drafts: Dict[str, Any], *, keep: int = 30) -> Dict[str, Any]:
    items = [(str(key), value) for key, value in drafts.items() if isinstance(value, dict)]
    items.sort(key=lambda pair: str(pair[1].get("updated_at") or pair[1].get("created_at") or ""))
    return {key: value for key, value in items[-keep:]}


def _trim_voice_prompt_requests(requests: Dict[str, Any], *, keep: int = 30) -> Dict[str, Any]:
    items = [(str(key), value) for key, value in requests.items() if isinstance(value, dict)]
    items.sort(key=lambda pair: str(pair[1].get("updated_at") or pair[1].get("created_at") or ""))
    return {key: value for key, value in items[-keep:]}


def _trim_voice_ask_requests(requests: Dict[str, Any], *, keep: int = 30) -> Dict[str, Any]:
    items = [(str(key), value) for key, value in requests.items() if isinstance(value, dict)]
    items.sort(key=lambda pair: str(pair[1].get("updated_at") or pair[1].get("created_at") or ""))
    return {key: value for key, value in items[-keep:]}


def _clean_voice_artifact_paths(value: Any, *, document_path: str = "", existing: Any = None) -> list[str]:
    raw_items: list[Any] = []
    if isinstance(existing, (list, tuple, set)):
        raw_items.extend(existing)
    elif isinstance(existing, str) and existing.strip():
        raw_items.append(existing)
    if isinstance(value, (list, tuple, set)):
        raw_items.extend(value)
    elif isinstance(value, str) and value.strip():
        raw_items.extend(line.strip() for line in value.replace(",", "\n").splitlines())
    clean_document_path = str(document_path or "").strip()
    if clean_document_path:
        raw_items.append(clean_document_path)

    out: list[str] = []
    seen: set[str] = set()
    for raw in raw_items:
        text = str(raw or "").strip().replace("\\", "/")
        if not text or text in seen:
            continue
        seen.add(text)
        out.append(text[:512])
    return out[:12]


def _clean_voice_source_urls(value: Any, *, existing: Any = None) -> list[str]:
    raw_items: list[Any] = []
    if isinstance(existing, (list, tuple, set)):
        raw_items.extend(existing)
    elif isinstance(existing, str) and existing.strip():
        raw_items.append(existing)
    if isinstance(value, (list, tuple, set)):
        raw_items.extend(value)
    elif isinstance(value, str) and value.strip():
        raw_items.extend(line.strip() for line in value.replace(",", "\n").splitlines())

    out: list[str] = []
    seen: set[str] = set()
    for raw in raw_items:
        text = str(raw or "").strip()
        if not text:
            continue
        if not re.match(r"^https?://", text, flags=re.IGNORECASE):
            continue
        text = text[:1024]
        if text in seen:
            continue
        seen.add(text)
        out.append(text)
    return out[:12]


def _voice_ask_request_public(record: Dict[str, Any]) -> Dict[str, Any]:
    artifact_paths = _clean_voice_artifact_paths(record.get("artifact_paths"), document_path=str(record.get("document_path") or ""))
    source_urls = _clean_voice_source_urls(record.get("source_urls"))
    out = {
        "schema": 1,
        "request_id": str(record.get("request_id") or ""),
        "status": str(record.get("status") or "pending"),
        "request_text": str(record.get("request_text") or ""),
        "request_preview": str(record.get("request_preview") or ""),
        "reply_text": str(record.get("reply_text") or ""),
        "document_path": str(record.get("document_path") or ""),
        "artifact_paths": artifact_paths,
        "source_summary": str(record.get("source_summary") or ""),
        "checked_at": str(record.get("checked_at") or ""),
        "source_urls": source_urls,
        "target_kind": str(record.get("target_kind") or "secretary"),
        "intent_hint": str(record.get("intent_hint") or ""),
        "language": str(record.get("language") or ""),
        "handoff_target": str(record.get("handoff_target") or ""),
        "handoff_request_id": str(record.get("handoff_request_id") or ""),
        "target_actor_id": str(record.get("target_actor_id") or ""),
        "input_appended_at": str(record.get("input_appended_at") or ""),
        "first_read_at": str(record.get("first_read_at") or ""),
        "first_feedback_at": str(record.get("first_feedback_at") or ""),
        "last_feedback_at": str(record.get("last_feedback_at") or ""),
        "created_at": str(record.get("created_at") or ""),
        "updated_at": str(record.get("updated_at") or ""),
        "cleared_at": str(record.get("cleared_at") or ""),
    }
    return {key: value for key, value in out.items() if value not in ("", None)}


def _voice_ask_requests_public(state: Dict[str, Any], *, keep: int = 10, include_cleared: bool = False) -> list[Dict[str, Any]]:
    requests = state.get("voice_ask_requests") if isinstance(state.get("voice_ask_requests"), dict) else {}
    items = [
        dict(value)
        for value in requests.values()
        if isinstance(value, dict) and (include_cleared or not str(value.get("cleared_at") or "").strip())
    ]
    items.sort(key=lambda item: str(item.get("updated_at") or item.get("created_at") or ""), reverse=True)
    return [_voice_ask_request_public(item) for item in items[:keep]]


def _upsert_voice_ask_request(
    state: Dict[str, Any],
    *,
    group: Group,
    request_id: str,
    status: str,
    request_text: str = "",
    document_path: str = "",
    target_kind: str = "secretary",
    intent_hint: str = "",
    language: str = "",
    reply_text: str = "",
    artifact_paths: Any = None,
    source_summary: str = "",
    checked_at: str = "",
    source_urls: Any = None,
    handoff_target: str = "",
    handoff_request_id: str = "",
    target_actor_id: str = "",
    input_appended_at: str = "",
    first_read_at: str = "",
    feedback_at: str = "",
    now: str,
) -> Dict[str, Any]:
    clean_request_id = _clean_voice_ask_request_id(request_id)
    clean_status = str(status or "pending").strip().lower()
    if clean_status not in _VOICE_ASK_REQUEST_STATUSES:
        clean_status = "pending"
    requests = state.setdefault("voice_ask_requests", {})
    existing = requests.get(clean_request_id) if isinstance(requests.get(clean_request_id), dict) else {}
    text = _clean_multiline_text(request_text, max_len=4_000) or str(existing.get("request_text") or "")
    new_reply = _clean_multiline_text(reply_text, max_len=4_000)
    existing_status = str(existing.get("status") or "").strip().lower() if isinstance(existing, dict) else ""
    existing_reply = str(existing.get("reply_text") or "") if isinstance(existing, dict) else ""
    if new_reply:
        reply = new_reply
    elif clean_status == existing_status and clean_status in _VOICE_ASK_FINAL_STATUSES:
        reply = existing_reply
    else:
        reply = ""
    clean_source_summary = _clean_multiline_text(source_summary, max_len=1_200) or str(existing.get("source_summary") or "")
    clean_checked_at = str(checked_at or existing.get("checked_at") or "").strip()[:120]
    existing_cleared_at = str(existing.get("cleared_at") or "").strip() if isinstance(existing, dict) else ""
    should_reveal = bool(reply) or clean_status in {"needs_user", "failed", "handed_off"}
    clean_input_appended_at = str(input_appended_at or existing.get("input_appended_at") or "").strip()
    if not clean_input_appended_at and text:
        clean_input_appended_at = now
    clean_first_read_at = str(first_read_at or existing.get("first_read_at") or "").strip()
    clean_feedback_at = str(feedback_at or "").strip()
    clean_first_feedback_at = str(existing.get("first_feedback_at") or "").strip() or clean_feedback_at
    clean_last_feedback_at = clean_feedback_at or str(existing.get("last_feedback_at") or "").strip()
    record = {
        "schema": 1,
        "group_id": group.group_id,
        "assistant_id": ASSISTANT_ID_VOICE_SECRETARY,
        "request_id": clean_request_id,
        "status": clean_status,
        "request_text": text,
        "request_preview": _clean_multiline_text(text, max_len=240),
        "reply_text": reply,
        "document_path": str(document_path or existing.get("document_path") or ""),
        "artifact_paths": _clean_voice_artifact_paths(
            artifact_paths,
            document_path=str(document_path or existing.get("document_path") or ""),
            existing=existing.get("artifact_paths") if isinstance(existing, dict) else None,
        ),
        "source_summary": clean_source_summary,
        "checked_at": clean_checked_at,
        "source_urls": _clean_voice_source_urls(
            source_urls,
            existing=existing.get("source_urls") if isinstance(existing, dict) else None,
        ),
        "target_kind": str(target_kind or existing.get("target_kind") or "secretary"),
        "intent_hint": str(intent_hint or existing.get("intent_hint") or ""),
        "language": str(language or existing.get("language") or ""),
        "handoff_target": str(handoff_target or existing.get("handoff_target") or ""),
        "handoff_request_id": str(handoff_request_id or existing.get("handoff_request_id") or ""),
        "target_actor_id": str(target_actor_id or existing.get("target_actor_id") or ""),
        "input_appended_at": clean_input_appended_at,
        "first_read_at": clean_first_read_at,
        "first_feedback_at": clean_first_feedback_at,
        "last_feedback_at": clean_last_feedback_at,
        "created_at": str(existing.get("created_at") or now) if isinstance(existing, dict) else now,
        "updated_at": now,
    }
    if existing_cleared_at and not should_reveal:
        record["cleared_at"] = existing_cleared_at
    requests[clean_request_id] = record
    state["voice_ask_requests"] = _trim_voice_ask_requests(requests)
    return record


def _mark_voice_ask_requests_working(group: Group, *, request_ids: list[str], now: str) -> None:
    clean_ids = [_clean_voice_ask_request_id(item) for item in request_ids if str(item or "").strip()]
    if not clean_ids:
        return
    state = _load_runtime_state(group)
    changed = False
    requests = state.get("voice_ask_requests") if isinstance(state.get("voice_ask_requests"), dict) else {}
    for request_id in clean_ids:
        record = requests.get(request_id) if isinstance(requests.get(request_id), dict) else {}
        current = str(record.get("status") or "").strip().lower()
        if current not in {"pending", ""}:
            continue
        updated = _upsert_voice_ask_request(
            state,
            group=group,
            request_id=request_id,
            status="working",
            first_read_at=now,
            now=now,
        )
        append_event(
            group.ledger_path,
            kind="assistant.voice.request",
            group_id=group.group_id,
            scope_key="",
            by=_assistant_principal(ASSISTANT_ID_VOICE_SECRETARY),
            data={
                "assistant_id": ASSISTANT_ID_VOICE_SECRETARY,
                "request_id": request_id,
                "target_actor_id": "",
                "action": "report",
                "status": "working",
                "source_request_id": request_id,
                "document_path": str(updated.get("document_path") or ""),
                "artifact_paths": _clean_voice_artifact_paths(updated.get("artifact_paths")),
                "request_preview": str(updated.get("request_preview") or ""),
            },
        )
        changed = True
    if changed:
        _save_runtime_state(group, state)


def _merge_voice_prompt_request(
    state: Dict[str, Any],
    *,
    group: Group,
    request_id: str,
    composer_text: str,
    voice_transcript: str,
    operation: str,
    composer_context: Dict[str, Any],
    composer_snapshot_hash: str,
    now: str,
) -> Dict[str, Any]:
    requests = state.setdefault("voice_prompt_requests", {})
    existing = requests.get(request_id) if isinstance(requests.get(request_id), dict) else {}
    transcripts = [
        _clean_multiline_text(item, max_len=4_000)
        for item in (existing.get("voice_transcripts") if isinstance(existing.get("voice_transcripts"), list) else [])
    ]
    transcripts = [item for item in transcripts if item]
    clean_voice = _clean_multiline_text(voice_transcript, max_len=8_000)
    if clean_voice and (not transcripts or transcripts[-1] != clean_voice):
        transcripts.append(clean_voice)
    while len("\n\n".join(transcripts)) > _MAX_PROMPT_REFINE_CHARS and len(transcripts) > 1:
        transcripts.pop(0)
    record = {
        "schema": 1,
        "group_id": group.group_id,
        "assistant_id": ASSISTANT_ID_VOICE_SECRETARY,
        "request_id": request_id,
        "operation": operation,
        "composer_text": composer_text,
        "composer_context": dict(composer_context),
        "composer_snapshot_hash": composer_snapshot_hash,
        "voice_transcripts": transcripts[-12:],
        "created_at": str(existing.get("created_at") or now) if isinstance(existing, dict) else now,
        "updated_at": now,
    }
    requests[request_id] = record
    state["voice_prompt_requests"] = _trim_voice_prompt_requests(requests)
    return record


def _stale_pending_voice_prompt_draft_in_state(state: Dict[str, Any], *, request_id: str, now: str) -> bool:
    drafts = state.setdefault("voice_prompt_drafts", {})
    record = drafts.get(request_id) if isinstance(drafts.get(request_id), dict) else {}
    if str(record.get("status") or "").strip().lower() != "pending":
        return False
    updated = dict(record)
    updated["status"] = "stale"
    updated["updated_at"] = now
    drafts[request_id] = updated
    state["voice_prompt_drafts"] = _trim_voice_prompt_drafts(drafts)
    return True


def _stale_expired_voice_prompt_drafts_in_state(
    state: Dict[str, Any],
    *,
    now: str,
    stale_after_seconds: int = _VOICE_PENDING_PROMPT_DRAFT_STALE_SECONDS,
) -> int:
    drafts = state.setdefault("voice_prompt_drafts", {})
    now_dt = parse_utc_iso(now)
    if now_dt is None:
        return 0
    changed = 0
    for request_id, raw_record in list(drafts.items()):
        if not isinstance(raw_record, dict):
            continue
        if str(raw_record.get("status") or "").strip().lower() != "pending":
            continue
        updated_at = parse_utc_iso(str(raw_record.get("updated_at") or raw_record.get("created_at") or ""))
        if updated_at is None:
            continue
        if (now_dt - updated_at).total_seconds() < max(60, int(stale_after_seconds or 0)):
            continue
        updated = dict(raw_record)
        updated["status"] = "stale"
        updated["updated_at"] = now
        drafts[str(request_id)] = updated
        changed += 1
    if changed:
        state["voice_prompt_drafts"] = _trim_voice_prompt_drafts(drafts)
    return changed


def _voice_secretary_foreman_actor_id(group: Group) -> str:
    actor = find_foreman(group)
    if isinstance(actor, dict):
        return str(actor.get("id") or "").strip()
    return ""


def _resolve_voice_secretary_request_target(group: Group, target: Any) -> tuple[str, str]:
    requested = " ".join(str(target or "").strip().split())
    if not requested:
        raise ValueError("voice secretary request target is required; use @foreman or one concrete actor")
    normalized = requested[1:] if requested.startswith("@") else requested
    if normalized.lower() == "foreman":
        actor_id = _voice_secretary_foreman_actor_id(group)
        if not actor_id:
            raise ValueError("voice secretary request target @foreman is unavailable")
        return actor_id, "@foreman"
    if normalized in {"all", "peers", "user"}:
        raise ValueError("voice secretary requests must target @foreman or one concrete actor")
    visible_ids = {
        str(actor.get("id") or "").strip()
        for actor in list_visible_actors(group)
        if isinstance(actor, dict)
    }
    if normalized not in visible_ids:
        raise ValueError(f"voice secretary request target actor not found: {requested}")
    return normalized, normalized


def _safe_voice_session_id(value: Any) -> str:
    raw = str(value or "").strip() or f"session-{uuid.uuid4().hex}"
    safe = re.sub(r"[^A-Za-z0-9_.-]+", "-", raw).strip(".-")[:96]
    return safe or f"session-{uuid.uuid4().hex}"


def _voice_session_dir(group: Group, session_id: str):
    return ensure_home() / "voice-secretary" / group.group_id / _safe_voice_session_id(session_id)


def _voice_documents_root(group: Group) -> Path:
    return ensure_home() / "voice-secretary" / group.group_id / "documents"


def _voice_input_stream_path(group: Group) -> Path:
    return ensure_home() / "voice-secretary" / group.group_id / "input_events.jsonl"


def _voice_input_state_path(group: Group) -> Path:
    return ensure_home() / "voice-secretary" / group.group_id / "input_state.json"


def _voice_documents_index_path(group: Group) -> Path:
    return _voice_documents_root(group) / "index.json"


def _safe_voice_document_id(value: Any = "") -> str:
    raw = str(value or "").strip()
    if not raw:
        raw = f"doc-{uuid.uuid4().hex}"
    safe = re.sub(r"[^A-Za-z0-9_.-]+", "-", raw).strip(".-")[:96]
    return safe or f"doc-{uuid.uuid4().hex}"


def _slugify_voice_document_title(value: Any, *, fallback: str = "untitled-document") -> str:
    text = unicodedata.normalize("NFKC", str(value or "")).strip().casefold()
    text = re.sub(r"[/\\:*?\"<>|\x00-\x1f]+", "-", text)
    text = re.sub(r"[^\w.-]+", "-", text, flags=re.UNICODE)
    text = re.sub(r"-+", "-", text).strip(" .-_")
    if not text:
        return fallback
    reserved = {
        "con",
        "prn",
        "aux",
        "nul",
        *(f"com{index}" for index in range(1, 10)),
        *(f"lpt{index}" for index in range(1, 10)),
    }
    if text in reserved:
        text = f"{text}-document"
    return text[:80].strip(" .-_") or fallback


def _safe_voice_document_rel_dir(value: Any) -> str:
    text = str(value or _DEFAULT_VOICE_DOCUMENT_DIR).strip().replace("\\", "/")
    if not text:
        text = _DEFAULT_VOICE_DOCUMENT_DIR
    rel = PurePosixPath(text)
    if rel.is_absolute() or ".." in rel.parts:
        raise ValueError("document_default_dir must stay under the group's active scope root")
    clean = rel.as_posix().strip("/")
    return clean or _DEFAULT_VOICE_DOCUMENT_DIR


def _safe_voice_document_rel_path(value: Any) -> str:
    text = str(value or "").strip().replace("\\", "/")
    if not text:
        raise ValueError("missing workspace_path")
    rel = PurePosixPath(text)
    if rel.is_absolute() or ".." in rel.parts:
        raise ValueError("workspace_path must stay under the group's active scope root")
    if not rel.name.endswith(".md"):
        raise ValueError("voice secretary working documents must be markdown files")
    return rel.as_posix().strip("/")


def _load_voice_documents_index(group: Group) -> Dict[str, Any]:
    payload = read_json(_voice_documents_index_path(group))
    if not isinstance(payload, dict) or int(payload.get("schema") or 0) != 1:
        return {"schema": 1, "group_id": group.group_id, "active_document_id": "", "documents": {}}
    documents = payload.get("documents") if isinstance(payload.get("documents"), dict) else {}
    return {
        "schema": 1,
        "group_id": group.group_id,
        "active_document_id": str(payload.get("active_document_id") or ""),
        "documents": {str(k): v for k, v in documents.items() if isinstance(v, dict)},
    }


def _save_voice_documents_index(group: Group, payload: Dict[str, Any]) -> None:
    documents = payload.get("documents") if isinstance(payload.get("documents"), dict) else {}
    items = sorted(
        [
            (str(doc_id), dict(record))
            for doc_id, record in documents.items()
            if isinstance(record, dict)
        ],
        key=lambda item: str(item[1].get("updated_at") or item[1].get("created_at") or ""),
    )[-_MAX_VOICE_DOCUMENTS:]
    normalized = {
        "schema": 1,
        "group_id": group.group_id,
        "active_document_id": str(payload.get("active_document_id") or ""),
        "documents": dict(items),
    }
    _voice_documents_index_path(group).parent.mkdir(parents=True, exist_ok=True)
    atomic_write_json(_voice_documents_index_path(group), normalized, indent=2)


def _resolve_voice_document_storage_path(group: Group, record: Dict[str, Any]) -> Path:
    storage_kind = str(record.get("storage_kind") or "").strip()
    if storage_kind == "workspace":
        root = resolve_active_scope_root(group)
        if root is None:
            raise ValueError("group has no active scope for workspace-backed voice document")
        rel_text = _safe_voice_document_rel_path(record.get("workspace_path"))
        path = (root / Path(*PurePosixPath(rel_text).parts)).resolve()
        try:
            path.relative_to(root)
        except ValueError as exc:
            raise ValueError("workspace_path must stay under the group's active scope root") from exc
        return path
    storage_path = str(record.get("storage_path") or "").strip()
    if storage_path:
        return Path(storage_path).expanduser().resolve()
    doc_id = _safe_voice_document_id(record.get("document_id"))
    return (_voice_documents_root(group) / doc_id / "document.md").resolve()


def _read_voice_document_content(group: Group, record: Dict[str, Any]) -> str:
    try:
        path = _resolve_voice_document_storage_path(group, record)
        if path.exists() and path.is_file():
            return path.read_text(encoding="utf-8")
    except Exception:
        return ""
    return ""


def _read_voice_document_content_for_metadata(group: Group, record: Dict[str, Any]) -> str | None:
    try:
        path = _resolve_voice_document_storage_path(group, record)
        if path.exists() and path.is_file():
            return path.read_text(encoding="utf-8")
    except Exception:
        return None
    return None


def _voice_document_content_sha(content: str) -> str:
    return hashlib.sha256(str(content or "").encode("utf-8")).hexdigest()


def _voice_document_public_record(group: Group, record: Dict[str, Any], *, include_content: bool = True) -> Dict[str, Any]:
    out = dict(record)
    document_path = _voice_document_path(out)
    out["document_path"] = document_path
    out["filename"] = PurePosixPath(document_path).name if document_path else str(out.get("title") or "Untitled document")
    if include_content:
        content = _read_voice_document_content(group, record)
        out["content"] = content
        out["content_sha256"] = _voice_document_content_sha(content)
        out["content_chars"] = len(content)
    else:
        out.pop("content", None)
        content = _read_voice_document_content_for_metadata(group, record)
        if content is not None:
            out["content_sha256"] = _voice_document_content_sha(content)
            out["content_chars"] = len(content)
        else:
            try:
                out["content_chars"] = max(0, int(out.get("content_chars") or 0))
            except Exception:
                out["content_chars"] = 0
    # Keep low-level transcript provenance in sidecars/revisions. User-facing
    # document surfaces should not tempt the secretary to copy segment ids into
    # polished markdown.
    out.pop("last_source_segment_id", None)
    out.pop("source_segment_count", None)
    out.pop("storage_path", None)
    return out


def _voice_document_path(record: Dict[str, Any]) -> str:
    workspace_path = str(record.get("workspace_path") or "").strip()
    if workspace_path:
        return workspace_path
    storage_path = str(record.get("storage_path") or "").strip()
    if storage_path:
        return PurePosixPath(Path(storage_path).name).as_posix()
    title = str(record.get("title") or "").strip()
    if title:
        return f"{_slugify_voice_document_title(title)}.md"
    return ""


def _voice_document_workspace_dir(group: Group, config: Optional[Dict[str, Any]] = None) -> tuple[str, Path, Path] | None:
    root = resolve_active_scope_root(group)
    if root is None:
        return None
    rel_dir_text = _safe_voice_document_rel_dir((config or {}).get("document_default_dir") or _DEFAULT_VOICE_DOCUMENT_DIR)
    rel_dir = PurePosixPath(rel_dir_text)
    path = (root / Path(*rel_dir.parts)).resolve()
    try:
        path.relative_to(root)
    except ValueError:
        return None
    return rel_dir.as_posix(), path, root


def _voice_document_title_from_markdown(path: Path, *, fallback: str) -> str:
    try:
        content = path.read_text(encoding="utf-8", errors="replace")[:8192]
    except Exception:
        return fallback
    lines = content.splitlines()
    if lines and lines[0].strip() == "---":
        for line in lines[1:80]:
            stripped = line.strip()
            if stripped in {"---", "..."}:
                break
            match = re.match(r"title\s*:\s*(.+)", stripped, flags=re.IGNORECASE)
            if match:
                title = match.group(1).strip().strip("'\"").strip()
                if title:
                    return title[:160]
    for line in lines[:120]:
        match = re.match(r"^#\s+(.+)", line.strip())
        if match:
            title = match.group(1).strip()
            if title:
                return title[:160]
    return fallback


def _voice_document_id_for_workspace_path(workspace_path: str) -> str:
    digest = hashlib.sha1(str(workspace_path or "").encode("utf-8")).hexdigest()[:24]
    return _safe_voice_document_id(f"voice-doc-{digest}")


def _voice_workspace_document_record(group: Group, *, workspace_path: str, path: Path) -> Dict[str, Any]:
    try:
        stat = path.stat()
        updated_at = datetime.fromtimestamp(stat.st_mtime, tz=timezone.utc).isoformat()
    except Exception:
        updated_at = utc_now_iso()
    fallback_title = PurePosixPath(workspace_path).stem or "Untitled document"
    title = _voice_document_title_from_markdown(path, fallback=fallback_title)
    return {
        "schema": 1,
        "document_id": _voice_document_id_for_workspace_path(workspace_path),
        "assistant_id": ASSISTANT_ID_VOICE_SECRETARY,
        "title": title,
        "status": "active",
        "storage_kind": "workspace",
        "workspace_path": workspace_path,
        "created_at": updated_at,
        "updated_at": updated_at,
        "created_by": "workspace_import",
        "revision_count": 0,
        "source_segment_count": 0,
        "last_source_segment_id": "",
        "discovered": True,
    }


def _discover_workspace_voice_documents(group: Group, *, config: Optional[Dict[str, Any]] = None) -> Dict[str, Dict[str, Any]]:
    workspace = _voice_document_workspace_dir(group, config)
    if workspace is None:
        return {}
    _rel_dir, directory, root = workspace
    if not directory.exists() or not directory.is_dir():
        return {}
    candidates: list[tuple[float, str, Path]] = []
    for path in directory.rglob("*.md"):
        if not path.is_file():
            continue
        try:
            relative_to_voice_dir = PurePosixPath(path.relative_to(directory).as_posix())
            if "archive" in relative_to_voice_dir.parts[:-1]:
                continue
            workspace_path = _safe_voice_document_rel_path(path.relative_to(root).as_posix())
            stat = path.stat()
        except Exception:
            continue
        candidates.append((float(stat.st_mtime), workspace_path, path))
    candidates.sort(key=lambda item: (item[0], item[1]), reverse=True)
    records: Dict[str, Dict[str, Any]] = {}
    for _mtime, workspace_path, path in candidates[:_MAX_VOICE_DOCUMENTS]:
        records[workspace_path] = _voice_workspace_document_record(group, workspace_path=workspace_path, path=path)
    return records


def _workspace_voice_document_missing(group: Group, record: Dict[str, Any]) -> bool:
    if str(record.get("storage_kind") or "").strip() != "workspace":
        return False
    try:
        path = _resolve_voice_document_storage_path(group, record)
    except Exception:
        return True
    return not path.exists() or not path.is_file()


def _active_voice_document_from_index(group: Group, index: Dict[str, Any]) -> tuple[str, Optional[Dict[str, Any]]]:
    documents = index.get("documents") if isinstance(index.get("documents"), dict) else {}
    active_id = str(index.get("active_document_id") or "").strip()
    record = documents.get(active_id) if active_id and isinstance(documents.get(active_id), dict) else None
    if not isinstance(record, dict):
        return "", None
    if str(record.get("status") or "active").strip().lower() != "active":
        return "", None
    if _workspace_voice_document_missing(group, record):
        return "", None
    return active_id, dict(record)


def _find_voice_document_by_path(
    group: Group,
    *,
    document_path: str = "",
    create: bool = False,
    config: Optional[Dict[str, Any]] = None,
) -> tuple[Dict[str, Any], Dict[str, Any]]:
    index = _load_voice_documents_index(group)
    documents = index.setdefault("documents", {})
    wanted = ""
    raw_path = str(document_path or "").strip()
    if raw_path:
        try:
            wanted = _safe_voice_document_rel_path(raw_path)
        except Exception:
            wanted = raw_path
    if not wanted:
        active_key = str(index.get("active_document_id") or "").strip()
        active_record = documents.get(active_key) if isinstance(documents.get(active_key), dict) else None
        if isinstance(active_record, dict):
            wanted = _voice_document_path(active_record)
    for raw_key, raw_record in documents.items():
        if not isinstance(raw_record, dict):
            continue
        if _voice_document_path(raw_record) == wanted:
            return index, dict(raw_record)
    if wanted:
        assistant = _effective_assistant(group, ASSISTANT_ID_VOICE_SECRETARY)
        assistant_config = config or (assistant.get("config") if isinstance(assistant.get("config"), dict) else {})
        discovered = _discover_workspace_voice_documents(group, config=assistant_config)
        record = discovered.get(wanted)
        if record is not None:
            doc_id = str(record.get("document_id") or "").strip()
            if doc_id:
                documents[doc_id] = dict(record)
            return index, dict(record)
    if create and not raw_path:
        return _get_voice_document(group, create=True, config=config)
    raise ValueError("voice secretary document not found")


def _load_voice_input_state(group: Group) -> Dict[str, Any]:
    payload = read_json(_voice_input_state_path(group))
    if not isinstance(payload, dict) or int(payload.get("schema") or 0) != 1:
        return {
            "schema": 1,
            "group_id": group.group_id,
            "latest_seq": 0,
            "secretary_read_cursor": 0,
            "secretary_delivery_cursor": 0,
            "last_notify_at": "",
            "retry_count": 0,
            "flush_count_since_idle_review": 0,
            "last_idle_review_at": "",
            "last_idle_review_input_seq": 0,
            "last_input_appended_at": "",
            "last_notify_emitted_at": "",
            "last_input_envelope_at": "",
            "last_input_envelope_id": "",
            "last_read_new_input_at": "",
        }
    return {
        "schema": 1,
        "group_id": group.group_id,
        "latest_seq": int(payload.get("latest_seq") or 0),
        "secretary_read_cursor": int(payload.get("secretary_read_cursor") or 0),
        "secretary_delivery_cursor": int(payload.get("secretary_delivery_cursor") or payload.get("secretary_read_cursor") or 0),
        "last_notify_at": str(payload.get("last_notify_at") or ""),
        "retry_count": max(0, int(payload.get("retry_count") or 0)),
        "flush_count_since_idle_review": max(0, int(payload.get("flush_count_since_idle_review") or 0)),
        "last_idle_review_at": str(payload.get("last_idle_review_at") or ""),
        "last_idle_review_input_seq": max(0, int(payload.get("last_idle_review_input_seq") or 0)),
        "last_input_appended_at": str(payload.get("last_input_appended_at") or ""),
        "last_notify_emitted_at": str(payload.get("last_notify_emitted_at") or ""),
        "last_input_envelope_at": str(payload.get("last_input_envelope_at") or ""),
        "last_input_envelope_id": str(payload.get("last_input_envelope_id") or ""),
        "last_read_new_input_at": str(payload.get("last_read_new_input_at") or ""),
    }


def _save_voice_input_state(group: Group, state: Dict[str, Any]) -> None:
    normalized = {
        "schema": 1,
        "group_id": group.group_id,
        "latest_seq": max(0, int(state.get("latest_seq") or 0)),
        "secretary_read_cursor": max(0, int(state.get("secretary_read_cursor") or 0)),
        "secretary_delivery_cursor": max(0, int(state.get("secretary_delivery_cursor") or 0)),
        "last_notify_at": str(state.get("last_notify_at") or ""),
        "retry_count": max(0, int(state.get("retry_count") or 0)),
        "flush_count_since_idle_review": max(0, int(state.get("flush_count_since_idle_review") or 0)),
        "last_idle_review_at": str(state.get("last_idle_review_at") or ""),
        "last_idle_review_input_seq": max(0, int(state.get("last_idle_review_input_seq") or 0)),
        "last_input_appended_at": str(state.get("last_input_appended_at") or ""),
        "last_notify_emitted_at": str(state.get("last_notify_emitted_at") or ""),
        "last_input_envelope_at": str(state.get("last_input_envelope_at") or ""),
        "last_input_envelope_id": str(state.get("last_input_envelope_id") or ""),
        "last_read_new_input_at": str(state.get("last_read_new_input_at") or ""),
    }
    path = _voice_input_state_path(group)
    path.parent.mkdir(parents=True, exist_ok=True)
    atomic_write_json(path, normalized, indent=2)


def _voice_input_delivery_cursor(state: Dict[str, Any]) -> int:
    return max(
        max(0, int(state.get("secretary_delivery_cursor") or 0)),
        max(0, int(state.get("secretary_read_cursor") or 0)),
    )


def _voice_input_timing_public(state: Dict[str, Any]) -> Dict[str, Any]:
    return {
        key: value
        for key, value in {
            "latest_seq": max(0, int(state.get("latest_seq") or 0)),
            "secretary_read_cursor": max(0, int(state.get("secretary_read_cursor") or 0)),
            "secretary_delivery_cursor": _voice_input_delivery_cursor(state),
            "last_input_appended_at": str(state.get("last_input_appended_at") or ""),
            "last_notify_emitted_at": str(state.get("last_notify_emitted_at") or state.get("last_notify_at") or ""),
            "last_input_envelope_at": str(state.get("last_input_envelope_at") or ""),
            "last_input_envelope_id": str(state.get("last_input_envelope_id") or ""),
            "last_read_new_input_at": str(state.get("last_read_new_input_at") or ""),
        }.items()
        if value not in ("", None)
    }


def _voice_idle_review_cooldown_elapsed(state: Dict[str, Any]) -> bool:
    last_dt = parse_utc_iso(str(state.get("last_idle_review_at") or ""))
    if last_dt is None:
        return True
    return (time.time() - last_dt.timestamp()) >= float(_VOICE_IDLE_REVIEW_GROUP_COOLDOWN_SECONDS)


def _voice_idle_review_trigger_kind(event: Dict[str, Any]) -> str:
    trigger = event.get("trigger") if isinstance(event.get("trigger"), dict) else {}
    return str(trigger.get("trigger_kind") or "").strip().lower()


def _voice_input_event_counts_for_idle_review(event: Dict[str, Any]) -> bool:
    return (
        str(event.get("kind") or "").strip() == "asr_transcript"
        and bool(_clean_multiline_text(event.get("text"), max_len=240))
        and bool(str(event.get("document_path") or "").strip())
    )


def _request_voice_idle_review_now(group: Group, *, reason: str, source_input_seq: int = 0) -> bool:
    before_state = _load_voice_input_state(group)
    before_latest_seq = int(before_state.get("latest_seq") or 0)
    try:
        from . import voice_idle_review_scheduler

        requested = voice_idle_review_scheduler.request_voice_idle_review(
            group.group_id,
            reason=reason,
            source_event_id=f"voice-input-{source_input_seq}" if source_input_seq > 0 else "",
            immediate=True,
        )
    except Exception:
        return False
    if not requested:
        return False
    after_state = _load_voice_input_state(group)
    return int(after_state.get("latest_seq") or 0) > before_latest_seq


def _maybe_request_voice_idle_review_for_input(group: Group, event: Dict[str, Any], state: Dict[str, Any]) -> bool:
    if not _voice_input_event_counts_for_idle_review(event):
        _save_voice_input_state(group, state)
        return False

    trigger_kind = _voice_idle_review_trigger_kind(event)
    seq = int(event.get("seq") or 0)
    state["flush_count_since_idle_review"] = int(state.get("flush_count_since_idle_review") or 0) + 1
    _save_voice_input_state(group, state)

    reason = ""
    if trigger_kind in _VOICE_IDLE_REVIEW_STOP_TRIGGER_KINDS:
        reason = "stop_flush"
    elif (
        int(state.get("flush_count_since_idle_review") or 0) >= _VOICE_IDLE_REVIEW_FLUSH_THRESHOLD
        and _voice_idle_review_cooldown_elapsed(state)
    ):
        reason = "input_batch"
    if not reason:
        return False
    return _request_voice_idle_review_now(group, reason=reason, source_input_seq=seq)


def _maybe_request_voice_idle_review_for_stop_flush(group: Group) -> bool:
    state = _load_voice_input_state(group)
    latest_seq = int(state.get("latest_seq") or 0)
    if latest_seq <= int(state.get("last_idle_review_input_seq") or 0):
        return False
    return _request_voice_idle_review_now(group, reason="stop_flush", source_input_seq=latest_seq)


def _voice_input_event_public(event: Dict[str, Any]) -> Dict[str, Any]:
    out = dict(event)
    out.pop("seq", None)
    out.pop("segment_id", None)
    out.pop("session_id", None)
    out.pop("trigger", None)
    metadata = out.get("metadata") if isinstance(out.get("metadata"), dict) else {}
    safe_metadata = {
        key: metadata.get(key)
        for key in (
            "capture_continuity",
            "suggested_document_mode",
            "target_kind",
            "request_id",
            "operation",
            "composer_snapshot_hash",
        )
        if metadata.get(key) not in (None, "")
    }
    if safe_metadata:
        out["metadata"] = safe_metadata
    else:
        out.pop("metadata", None)
    return out


def _append_voice_input_event(
    group: Group,
    *,
    kind: str,
    text: str,
    document: Dict[str, Any],
    language: str = "",
    intent_hint: str = "",
    source: str = "",
    session_id: str = "",
    segment_id: str = "",
    by: str = "",
    trigger: Optional[Dict[str, Any]] = None,
    metadata: Optional[Dict[str, Any]] = None,
    emit_notify: bool = True,
) -> Dict[str, Any]:
    clean_text = _clean_multiline_text(text, max_len=_MAX_TRANSCRIPT_SESSION_CHARS)
    if not clean_text:
        raise ValueError("voice secretary input text is empty")
    now = utc_now_iso()
    state = _load_voice_input_state(group)
    seq = int(state.get("latest_seq") or 0) + 1
    document_path = _voice_document_path(document)
    event = {
        "schema": 1,
        "seq": seq,
        "kind": str(kind or "input").strip() or "input",
        "group_id": group.group_id,
        "assistant_id": ASSISTANT_ID_VOICE_SECRETARY,
        "created_at": now,
        "updated_at": now,
        "text": clean_text,
        "language": str(language or ""),
        "document_path": document_path,
        "filename": PurePosixPath(document_path).name if document_path else "",
        "title": str(document.get("title") or ""),
        "storage_kind": str(document.get("storage_kind") or ""),
        "intent_hint": str(intent_hint or ""),
        "source": str(source or ""),
        "session_id": str(session_id or ""),
        "segment_id": str(segment_id or ""),
        "by": str(by or ""),
        "trigger": dict(trigger or {}),
        "metadata": dict(metadata or {}),
    }
    path = _voice_input_stream_path(group)
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("a", encoding="utf-8") as fh:
        fh.write(json.dumps(event, ensure_ascii=False, sort_keys=True) + "\n")
    state["latest_seq"] = seq
    state["last_input_appended_at"] = now
    idle_review_requested = _maybe_request_voice_idle_review_for_input(group, event, state)
    if emit_notify:
        if event["kind"] == "idle_review":
            _emit_voice_input_notify(group, reason="idle_review")
        elif idle_review_requested:
            pass
        else:
            _emit_voice_input_notify(group, reason="new_input")
    return _voice_input_event_public(event)


def _read_voice_input_events(group: Group, *, after_seq: int, max_items: int = 100, max_chars: int = 24_000) -> list[Dict[str, Any]]:
    path = _voice_input_stream_path(group)
    if not path.exists():
        return []
    out: list[Dict[str, Any]] = []
    chars = 0
    with path.open("r", encoding="utf-8") as fh:
        for line in fh:
            try:
                item = json.loads(line)
            except Exception:
                continue
            if not isinstance(item, dict):
                continue
            try:
                seq = int(item.get("seq") or 0)
            except Exception:
                seq = 0
            if seq <= after_seq:
                continue
            text = _clean_multiline_text(item.get("text"), max_len=_MAX_TRANSCRIPT_SESSION_CHARS)
            if not text:
                continue
            if out and (len(out) >= max_items or chars + len(text) > max_chars):
                break
            chars += len(text)
            out.append(dict(item))
    return out


def _latest_voice_input_event_for_idle_review(group: Group, *, after_seq: int) -> Dict[str, Any] | None:
    path = _voice_input_stream_path(group)
    if not path.exists():
        return None
    latest: Dict[str, Any] | None = None
    with path.open("r", encoding="utf-8") as fh:
        for line in fh:
            try:
                item = json.loads(line)
            except Exception:
                continue
            if not isinstance(item, dict):
                continue
            try:
                seq = int(item.get("seq") or 0)
            except Exception:
                seq = 0
            if seq <= after_seq:
                continue
            if str(item.get("kind") or "").strip() != "asr_transcript":
                continue
            if not str(item.get("document_path") or "").strip():
                continue
            if not _clean_multiline_text(item.get("text"), max_len=240):
                continue
            latest = dict(item)
    return latest


def _voice_input_target_kind(item: Dict[str, Any]) -> str:
    metadata = item.get("metadata") if isinstance(item.get("metadata"), dict) else {}
    target_kind = str(metadata.get("target_kind") or "").strip().lower()
    if target_kind in {"document", "composer", "secretary"}:
        return target_kind
    return "document" if str(item.get("document_path") or "").strip() else "secretary"


def _voice_previous_input_fragments(text: Any) -> list[str]:
    clean = _clean_multiline_text(text, max_len=1_000).replace("\n", " ").strip()
    if not clean:
        return []
    parts = [part.strip() for part in re.split(r"(?<=[。！？!?\.])\s*|\n+", clean) if part.strip()]
    if not parts:
        parts = [clean]
    return parts[-_VOICE_PREVIOUS_INPUT_TAIL_MAX_FRAGMENTS:]


def _voice_document_previous_input_tail(group: Group, *, document_path: str, before_seq: int) -> str:
    clean_path = str(document_path or "").strip()
    if not clean_path or before_seq <= 0:
        return ""
    path = _voice_input_stream_path(group)
    if not path.exists():
        return ""
    candidates: list[Dict[str, Any]] = []
    with path.open("r", encoding="utf-8") as fh:
        for line in fh:
            try:
                item = json.loads(line)
            except Exception:
                continue
            if not isinstance(item, dict):
                continue
            try:
                seq = int(item.get("seq") or 0)
            except Exception:
                seq = 0
            if seq <= 0 or seq >= before_seq:
                continue
            if _voice_input_target_kind(item) != "document":
                continue
            if str(item.get("document_path") or "").strip() != clean_path:
                continue
            if str(item.get("kind") or "").strip() == "idle_review":
                continue
            text = _clean_multiline_text(item.get("text"), max_len=1_000)
            if not text or _is_voice_transcript_tiny_filler(text):
                continue
            candidates.append({"seq": seq, "text": text})
            if len(candidates) > _VOICE_PREVIOUS_INPUT_TAIL_SCAN_ITEMS:
                candidates = candidates[-_VOICE_PREVIOUS_INPUT_TAIL_SCAN_ITEMS:]
    fragments: list[str] = []
    remaining = _VOICE_PREVIOUS_INPUT_TAIL_MAX_CHARS
    for item in reversed(candidates):
        for fragment in reversed(_voice_previous_input_fragments(item.get("text"))):
            clean = _clean_multiline_text(fragment, max_len=160).replace("\n", " ").strip()
            if not clean:
                continue
            if len(clean) > remaining:
                clean = clean[-remaining:].lstrip()
                if fragments and not clean.startswith("..."):
                    clean = f"...{clean}"[-remaining:]
            fragments.append(clean)
            remaining -= len(clean)
            if len(fragments) >= _VOICE_PREVIOUS_INPUT_TAIL_MAX_FRAGMENTS or remaining <= 0:
                return "\n".join(reversed(fragments)).strip()
    return "\n".join(reversed(fragments)).strip()


def _annotate_voice_input_previous_tails(group: Group, grouped: list[Dict[str, Any]], items: list[Dict[str, Any]]) -> list[Dict[str, Any]]:
    first_seq_by_document: Dict[str, int] = {}
    for item in items:
        if _voice_input_target_kind(item) != "document":
            continue
        document_path = str(item.get("document_path") or "").strip()
        if not document_path:
            continue
        try:
            seq = int(item.get("seq") or 0)
        except Exception:
            seq = 0
        if seq <= 0:
            continue
        existing = int(first_seq_by_document.get(document_path) or 0)
        if existing <= 0 or seq < existing:
            first_seq_by_document[document_path] = seq
    for group_item in grouped:
        if str(group_item.get("target_kind") or "").strip() != "document":
            continue
        document_path = str(group_item.get("document_path") or "").strip()
        before_seq = int(first_seq_by_document.get(document_path) or 0)
        tail = _voice_document_previous_input_tail(group, document_path=document_path, before_seq=before_seq)
        if tail:
            group_item["previous_input_tail"] = tail
    return grouped


def _group_voice_input_by_target(items: list[Dict[str, Any]]) -> list[Dict[str, Any]]:
    grouped: Dict[str, Dict[str, Any]] = {}
    for item in items:
        document_path = str(item.get("document_path") or "").strip()
        target_kind = _voice_input_target_kind(item)
        metadata = item.get("metadata") if isinstance(item.get("metadata"), dict) else {}
        request_id = str(metadata.get("request_id") or "").strip()
        key = (
            f"document:{document_path}"
            if target_kind == "document" and document_path
            else f"composer:{request_id or item.get('seq') or 'prompt'}"
            if target_kind == "composer"
            else "secretary"
        )
        group_item = grouped.setdefault(
            key,
            {
                "target_kind": target_kind,
                "request_kind": "",
                "document_path": document_path,
                "filename": str(item.get("filename") or ""),
                "title": str(item.get("title") or ""),
                "requires_report": False,
                "report_channel": "",
                "request_ids": [],
                "operations": [],
                "item_count": 0,
                "kinds": [],
                "intent_hints": [],
                "languages": [],
                "sources": [],
                "combined_text": "",
            },
        )
        if target_kind == "document" and document_path and not str(group_item.get("document_path") or "").strip():
            group_item["document_path"] = document_path
            group_item["filename"] = str(item.get("filename") or "")
            group_item["title"] = str(item.get("title") or "")
        public_item = _voice_input_event_public(item)
        group_item["item_count"] = int(group_item.get("item_count") or 0) + 1
        public_metadata = public_item.get("metadata") if isinstance(public_item.get("metadata"), dict) else {}
        for field, target in (("request_id", "request_ids"), ("operation", "operations")):
            value = str(public_metadata.get(field) or "").strip()
            values = group_item.get(target) if isinstance(group_item.get(target), list) else []
            if value and value not in values:
                values.append(value)
            group_item[target] = values
        for field, target in (
            ("kind", "kinds"),
            ("intent_hint", "intent_hints"),
            ("language", "languages"),
            ("source", "sources"),
        ):
            value = str(public_item.get(field) or "").strip()
            values = group_item.get(target) if isinstance(group_item.get(target), list) else []
            if value and value not in values:
                values.append(value)
            group_item[target] = values
        text = str(public_item.get("text") or "")
        if target_kind == "composer" and request_id:
            group_item["combined_text"] = text
        else:
            group_item["combined_text"] = text if not group_item["combined_text"] else f"{group_item['combined_text']}\n{text}"
    for group_item in grouped.values():
        target_kind = str(group_item.get("target_kind") or "").strip()
        request_ids = group_item.get("request_ids") if isinstance(group_item.get("request_ids"), list) else []
        has_request = any(str(item or "").strip() for item in request_ids)
        if target_kind == "secretary":
            group_item["request_kind"] = "ask_request" if has_request else "secretary_input"
        elif target_kind == "composer":
            group_item["request_kind"] = "prompt_refine" if has_request else "composer_input"
        elif target_kind == "document":
            group_item["request_kind"] = "document_request" if has_request else "document_input"
        if target_kind in {"secretary", "document"} and has_request:
            group_item["requires_report"] = True
            group_item["report_channel"] = "cccc_voice_secretary_request(action=\"report\")"
    return list(grouped.values())


def _voice_input_mode(group_item: Dict[str, Any]) -> str:
    target_kind = str(group_item.get("target_kind") or "").strip().lower()
    if target_kind == "composer":
        return "prompt"
    if target_kind == "secretary":
        return "ask"
    return "document"


def _voice_input_request_id_values(group_item: Dict[str, Any]) -> list[str]:
    return [
        str(item).strip()
        for item in (group_item.get("request_ids") if isinstance(group_item.get("request_ids"), list) else [])
        if str(item).strip()
    ]


def _voice_input_operation_values(group_item: Dict[str, Any]) -> list[str]:
    return [
        str(item).strip()
        for item in (group_item.get("operations") if isinstance(group_item.get("operations"), list) else [])
        if str(item).strip()
    ]


def _voice_input_required_outputs(group_item: Dict[str, Any]) -> list[str]:
    target_kind = str(group_item.get("target_kind") or "secretary").strip().lower()
    request_ids = _voice_input_request_id_values(group_item)
    request_arg = request_ids[0] if len(request_ids) == 1 else "..."
    operations = _voice_input_operation_values(group_item)
    composer_replace_operation = any(
        item.lower() in {"replace", "replace_with_refined_prompt"}
        for item in operations
    )
    if target_kind == "secretary" and request_ids:
        return [
            f"cccc_voice_secretary_request(action=\"report\", request_id=\"{request_arg}\", status=\"done\"|\"needs_user\"|\"failed\", reply_text=\"...\")."
        ]
    if target_kind == "composer" and request_ids:
        if composer_replace_operation:
            return [
                f"cccc_voice_secretary_composer(action=\"submit_prompt_draft\", request_id=\"{request_arg}\", draft_text=\"...\"). Return a complete replacement prompt."
            ]
        return [
            f"cccc_voice_secretary_composer(action=\"submit_prompt_draft\", request_id=\"{request_arg}\", draft_text=\"...\"). Return append-ready text only."
        ]
    if target_kind == "document":
        if bool(group_item.get("requires_report")):
            return [
                f"Edit the repository markdown directly, then cccc_voice_secretary_request(action=\"report\", request_id=\"{request_arg}\", status=\"done\", reply_text=\"...\")."
            ]
        return ["Edit the repository markdown directly."]
    return []


def _voice_input_is_structured_work_text(text: str) -> bool:
    clean = str(text or "").lstrip()
    return clean.startswith(("Task:\n", "Inputs:\n", "Context (not task):\n", "Output constraint:\n"))


def _voice_input_work_details_text(group_item: Dict[str, Any]) -> str:
    text = str(group_item.get("combined_text") or "").strip()
    if not text:
        return ""
    if _voice_input_is_structured_work_text(text):
        return text
    mode = _voice_input_mode(group_item)
    kinds = {
        str(item).strip()
        for item in (group_item.get("kinds") if isinstance(group_item.get("kinds"), list) else [])
        if str(item).strip()
    }
    if mode == "document":
        label = "Transcript/source material:" if "asr_transcript" in kinds else "Document input:"
        return "\n".join(["Inputs:", label, text]).strip()
    if mode == "ask":
        return "\n".join(["Task:", text]).strip()
    return "\n".join(["Inputs:", text]).strip()


def _voice_input_batch_text(grouped: list[Dict[str, Any]], *, item_count: int) -> str:
    if not grouped:
        return "No new Secretary input."
    blocks = [
        f"Voice Secretary input: {item_count} item{'s' if item_count != 1 else ''}. Follow Required output; console text is not delivered.",
    ]
    multiple_work_orders = len(grouped) > 1
    for index, group_item in enumerate(grouped, 1):
        target_kind = str(group_item.get("target_kind") or "secretary").strip()
        mode = _voice_input_mode(group_item)
        path = str(group_item.get("document_path") or "").strip()
        title = str(group_item.get("title") or "").strip()
        languages = ", ".join(str(item) for item in (group_item.get("languages") or []) if str(item).strip())
        intents = ", ".join(str(item) for item in (group_item.get("intent_hints") or []) if str(item).strip())
        kinds = ", ".join(str(item) for item in (group_item.get("kinds") or []) if str(item).strip())
        request_ids = ", ".join(str(item) for item in (group_item.get("request_ids") or []) if str(item).strip())
        operation_values = _voice_input_operation_values(group_item)
        operations = ", ".join(operation_values)
        work_text = _voice_input_work_details_text(group_item)
        previous_tail = str(group_item.get("previous_input_tail") or "").strip()
        required_outputs = _voice_input_required_outputs(group_item)
        lines = [f"Work order {index}:" if multiple_work_orders else "Work order:", f"Mode: {mode}", f"Target: {target_kind}"]
        if target_kind == "document":
            if path:
                lines.append(f"Document: {path}")
            if title:
                lines.append(f"Title: {title}")
        if request_ids:
            lines.append(f"Request id: {request_ids}")
        if operations:
            lines.append(f"Operation: {operations}")
        if languages:
            lines.append(f"Language: {languages}")
        if intents:
            lines.append(f"Intent: {intents}")
        if kinds and target_kind == "document":
            lines.append(f"Input kind: {kinds}")
        if required_outputs:
            lines.extend(["", "Required output:"])
            lines.extend(f"- {item}" for item in required_outputs)
        if work_text:
            lines.extend(["", work_text])
        if previous_tail:
            lines.extend([
                "",
                "Context (not task):",
                "Previous input tail for continuity only; do not copy verbatim:",
                previous_tail,
            ])
        blocks.append("\n".join(lines).strip())
    return "\n\n".join(block for block in blocks if block.strip()).strip()


def _peek_voice_input_batch(
    group: Group,
    *,
    after_seq: Optional[int] = None,
    max_items: int = 100,
    max_chars: int = 24_000,
) -> Dict[str, Any]:
    state = _load_voice_input_state(group)
    cursor = max(0, int(after_seq if after_seq is not None else state.get("secretary_read_cursor") or 0))
    items = _read_voice_input_events(group, after_seq=cursor, max_items=max_items, max_chars=max_chars)
    seq_values: list[int] = []
    for item in items:
        try:
            seq_value = max(0, int(item.get("seq") or 0))
        except Exception:
            seq_value = 0
        if seq_value > 0:
            seq_values.append(seq_value)
    seq_start = min(seq_values) if seq_values else 0
    seq_end = max(seq_values) if seq_values else cursor
    grouped = _group_voice_input_by_target(items)
    grouped = _annotate_voice_input_previous_tails(group, grouped, items)
    composer_request_ids: list[str] = []
    report_request_ids: list[str] = []
    for group_item in grouped:
        target_kind = str(group_item.get("target_kind") or "").strip()
        request_ids = group_item.get("request_ids") if isinstance(group_item.get("request_ids"), list) else []
        if target_kind == "composer":
            for request_id in request_ids:
                request_id_text = str(request_id or "").strip()
                if request_id_text and request_id_text not in composer_request_ids:
                    composer_request_ids.append(request_id_text)
            continue
        if bool(group_item.get("requires_report")):
            for request_id in (group_item.get("request_ids") if isinstance(group_item.get("request_ids"), list) else []):
                request_id_text = str(request_id or "").strip()
                if request_id_text and request_id_text not in report_request_ids:
                    report_request_ids.append(request_id_text)
    return {
        "item_count": len(items),
        "input_text": _voice_input_batch_text(grouped, item_count=len(items)),
        "input_batches": [_voice_input_batch_public(item) for item in grouped],
        "composer_request_ids": composer_request_ids,
        "secretary_request_ids": report_request_ids,
        "report_request_ids": report_request_ids,
        "seq_start": seq_start,
        "seq_end": seq_end,
        "delivery_id": f"voice-input:{group.group_id}:{seq_start}-{seq_end}" if seq_start and seq_end else "",
        "latest_seq": int(state.get("latest_seq") or 0),
        "secretary_read_cursor": cursor,
        "has_new_input": bool(items),
    }


def _voice_input_envelope_from_preview(
    group: Group,
    *,
    preview: Dict[str, Any],
    reason: str,
    created_at: str,
) -> Dict[str, Any]:
    if not bool(preview.get("has_new_input")):
        return {}
    seq_start = max(0, int(preview.get("seq_start") or 0))
    seq_end = max(0, int(preview.get("seq_end") or 0))
    if seq_start <= 0 or seq_end < seq_start:
        return {}
    input_batches = preview.get("input_batches") if isinstance(preview.get("input_batches"), list) else []
    composer_request_ids = [
        str(item).strip()
        for item in (preview.get("composer_request_ids") if isinstance(preview.get("composer_request_ids"), list) else [])
        if str(item).strip()
    ]
    secretary_request_ids = [
        str(item).strip()
        for item in (preview.get("secretary_request_ids") if isinstance(preview.get("secretary_request_ids"), list) else [])
        if str(item).strip()
    ]
    report_request_ids = [
        str(item).strip()
        for item in (preview.get("report_request_ids") if isinstance(preview.get("report_request_ids"), list) else [])
        if str(item).strip()
    ] or secretary_request_ids
    input_target_kinds = [
        str(item.get("target_kind") or "").strip().lower()
        for item in input_batches
        if isinstance(item, dict) and str(item.get("target_kind") or "").strip()
    ]
    return {
        "schema": 1,
        "delivery_id": str(preview.get("delivery_id") or f"voice-input:{group.group_id}:{seq_start}-{seq_end}"),
        "group_id": group.group_id,
        "target_actor_id": VOICE_SECRETARY_ACTOR_ID,
        "created_at": created_at,
        "reason": str(reason or "new_input").strip() or "new_input",
        "delivery_mode": "daemon_input_envelope",
        "cursor_policy": "advance_delivery_cursor_on_envelope_emit",
        "seq_start": seq_start,
        "seq_end": seq_end,
        "latest_seq": max(0, int(preview.get("latest_seq") or seq_end)),
        "item_count": max(0, int(preview.get("item_count") or 0)),
        "input_text": str(preview.get("input_text") or ""),
        "input_batches": input_batches,
        "composer_request_ids": composer_request_ids,
        "secretary_request_ids": report_request_ids,
        "report_request_ids": report_request_ids,
        "input_target_kinds": input_target_kinds,
    }


def _voice_input_batch_public(group_item: Dict[str, Any]) -> Dict[str, Any]:
    required_outputs = _voice_input_required_outputs(group_item)
    out: Dict[str, Any] = {
        "mode": _voice_input_mode(group_item),
        "target_kind": str(group_item.get("target_kind") or ""),
        "document_path": str(group_item.get("document_path") or ""),
        "filename": str(group_item.get("filename") or ""),
        "title": str(group_item.get("title") or ""),
        "request_kind": str(group_item.get("request_kind") or ""),
        "item_count": max(0, int(group_item.get("item_count") or 0)),
    }
    if bool(group_item.get("requires_report")):
        out["requires_report"] = True
        out["report_channel"] = str(group_item.get("report_channel") or "cccc_voice_secretary_request(action=\"report\")")
    if required_outputs:
        out["required_outputs"] = required_outputs
    for key in ("request_ids", "operations", "kinds", "intent_hints", "languages", "sources"):
        values = [str(item).strip() for item in (group_item.get(key) if isinstance(group_item.get(key), list) else []) if str(item).strip()]
        if values:
            out[key] = values
    previous_tail = str(group_item.get("previous_input_tail") or "").strip()
    if previous_tail:
        out["previous_input_tail"] = previous_tail
        out["context_text"] = previous_tail
    return {key: value for key, value in out.items() if value not in ("", [], None)}


def _emit_voice_input_notify(group: Group, *, reason: str) -> Dict[str, Any]:
    assistant = _effective_assistant(group, ASSISTANT_ID_VOICE_SECRETARY)
    if not bool(assistant.get("enabled")):
        return {}
    actor = get_voice_secretary_actor(group)
    if not isinstance(actor, dict):
        return {}
    state = _load_voice_input_state(group)
    delivery_cursor = _voice_input_delivery_cursor(state)
    if int(state.get("latest_seq") or 0) <= delivery_cursor:
        return {}
    now = utc_now_iso()
    preview = _peek_voice_input_batch(group, after_seq=delivery_cursor)
    envelope = _voice_input_envelope_from_preview(group, preview=preview, reason=reason, created_at=now)
    if not envelope:
        return {}
    notify = SystemNotifyData(
        kind="info",
        priority="normal",
        title="Voice Secretary input available",
        message="Secretary input is ready in this notification's input_envelope.",
        target_actor_id=VOICE_SECRETARY_ACTOR_ID,
        requires_ack=False,
        context={
            "kind": "voice_secretary_input",
            "reason": reason,
            "input_envelope": envelope,
        },
    )
    notify_event = emit_system_notify(group, by="system", notify=notify)
    latest_seq = int(state.get("latest_seq") or 0)
    seq_end = max(0, int(envelope.get("seq_end") or 0))
    if seq_end > int(state.get("secretary_delivery_cursor") or 0):
        state["secretary_delivery_cursor"] = seq_end
    state["last_notify_at"] = now
    state["last_notify_emitted_at"] = now
    state["last_input_envelope_at"] = now
    state["last_input_envelope_id"] = str(envelope.get("delivery_id") or "")
    if _voice_input_delivery_cursor(state) >= latest_seq:
        state["last_notify_at"] = ""
        state["retry_count"] = 0
    elif reason != "new_input":
        state["retry_count"] = int(state.get("retry_count") or 0) + 1
    else:
        state["retry_count"] = 0
    _save_voice_input_state(group, state)
    return notify_event


def _maybe_emit_voice_input_retry_notify(group: Group) -> None:
    state = _load_voice_input_state(group)
    if int(state.get("latest_seq") or 0) <= _voice_input_delivery_cursor(state):
        return
    retry_count = int(state.get("retry_count") or 0)
    if retry_count >= len(_VOICE_INPUT_NUDGE_RETRY_SECONDS):
        return
    last_notify_at = str(state.get("last_notify_at") or "")
    if not last_notify_at:
        _emit_voice_input_notify(group, reason="new_input")
        return
    last_dt = parse_utc_iso(last_notify_at)
    if last_dt is None:
        _emit_voice_input_notify(group, reason="new_input")
        return
    if time.time() - last_dt.timestamp() >= _VOICE_INPUT_NUDGE_RETRY_SECONDS[retry_count]:
        _emit_voice_input_notify(group, reason="unread_retry")


def _retained_voice_documents(group: Group, *, include_archived: bool = False, include_content: bool = True) -> list[Dict[str, Any]]:
    index = _load_voice_documents_index(group)
    documents = index.get("documents") if isinstance(index.get("documents"), dict) else {}
    assistant = _effective_assistant(group, ASSISTANT_ID_VOICE_SECRETARY)
    assistant_config = assistant.get("config") if isinstance(assistant.get("config"), dict) else {}
    out_by_path: Dict[str, Dict[str, Any]] = {}
    suppressed_paths: set[str] = set()
    for record in documents.values():
        if not isinstance(record, dict):
            continue
        status = str(record.get("status") or "active").strip() or "active"
        document_path = _voice_document_path(record)
        if status == "deleted":
            if document_path:
                suppressed_paths.add(document_path)
            continue
        if status == "archived" and not include_archived:
            if document_path:
                suppressed_paths.add(document_path)
            continue
        if status == "active" and _workspace_voice_document_missing(group, record):
            continue
        public_record = _voice_document_public_record(group, record, include_content=include_content)
        document_path = str(public_record.get("document_path") or "").strip()
        if document_path:
            out_by_path[document_path] = public_record
    for document_path, record in _discover_workspace_voice_documents(group, config=assistant_config).items():
        if document_path in out_by_path or document_path in suppressed_paths:
            continue
        out_by_path[document_path] = _voice_document_public_record(group, record, include_content=include_content)
    return sorted(
        out_by_path.values(),
        key=lambda item: (
            str(item.get("created_at") or ""),
            str(item.get("document_id") or ""),
        ),
        reverse=True,
    )


def _default_voice_document_workspace_path(group: Group, *, title: str, config: Dict[str, Any]) -> tuple[str, Path] | None:
    root = resolve_active_scope_root(group)
    if root is None:
        return None
    rel_dir_text = _safe_voice_document_rel_dir(config.get("document_default_dir") or _DEFAULT_VOICE_DOCUMENT_DIR)
    rel_dir = PurePosixPath(rel_dir_text)
    today = datetime.now(timezone.utc).strftime("%Y-%m-%d")
    base_slug = _slugify_voice_document_title(f"{today}-{title}", fallback=f"{today}-untitled-document")
    for index in range(1, 200):
        name = f"{base_slug}.md" if index == 1 else f"{base_slug}-{index}.md"
        rel = (rel_dir / name).as_posix()
        path = (root / Path(*PurePosixPath(rel).parts)).resolve()
        try:
            path.relative_to(root)
        except ValueError:
            return None
        if not path.exists():
            return rel, path
    rel = (rel_dir / f"{base_slug}-{uuid.uuid4().hex[:8]}.md").as_posix()
    return rel, (root / Path(*PurePosixPath(rel).parts)).resolve()


def _unique_voice_document_workspace_archive_path(group: Group, *, workspace_path: str) -> tuple[str, Path] | None:
    root = resolve_active_scope_root(group)
    if root is None:
        return None
    current_rel = PurePosixPath(_safe_voice_document_rel_path(workspace_path))
    archive_dir = current_rel.parent / "archive"
    base_name = current_rel.name
    stem = current_rel.stem or "untitled-document"
    suffix = current_rel.suffix or ".md"
    for index in range(1, 200):
        name = base_name if index == 1 else f"{stem}-{index}{suffix}"
        rel = (archive_dir / name).as_posix()
        path = (root / Path(*PurePosixPath(rel).parts)).resolve()
        try:
            path.relative_to(root)
        except ValueError:
            return None
        if not path.exists():
            return rel, path
    rel = (archive_dir / f"{stem}-{uuid.uuid4().hex[:8]}{suffix}").as_posix()
    return rel, (root / Path(*PurePosixPath(rel).parts)).resolve()


def _default_voice_document_content(group: Group, *, title: str, now: str) -> str:
    _ = group
    _ = title
    _ = now
    return ""


def _create_voice_document_record(group: Group, *, title: str = "", config: Optional[Dict[str, Any]] = None) -> Dict[str, Any]:
    now = utc_now_iso()
    config = config or {}
    clean_title = " ".join(str(title or "").strip().split()) or "Untitled document"
    doc_id = _safe_voice_document_id(f"voice-doc-{uuid.uuid4().hex}")
    workspace = _default_voice_document_workspace_path(group, title=clean_title, config=config)
    if workspace is not None:
        rel_path, abs_path = workspace
        storage_kind = "workspace"
        record = {
            "schema": 1,
            "document_id": doc_id,
            "assistant_id": ASSISTANT_ID_VOICE_SECRETARY,
            "title": clean_title,
            "status": "active",
            "storage_kind": storage_kind,
            "workspace_path": rel_path,
            "created_at": now,
            "updated_at": now,
            "created_by": _assistant_principal(ASSISTANT_ID_VOICE_SECRETARY),
            "revision_count": 0,
            "source_segment_count": 0,
            "last_source_segment_id": "",
        }
        content = _default_voice_document_content(group, title=clean_title, now=now)
        abs_path.parent.mkdir(parents=True, exist_ok=True)
        atomic_write_text(abs_path, content, encoding="utf-8")
        return record

    raise ValueError("Voice Secretary working documents require an attached repository scope")


def _get_voice_document(
    group: Group,
    *,
    document_id: str = "",
    create: bool = False,
    config: Optional[Dict[str, Any]] = None,
) -> tuple[Dict[str, Any], Dict[str, Any]]:
    index = _load_voice_documents_index(group)
    documents = index.setdefault("documents", {})
    wanted = _safe_voice_document_id(document_id) if document_id else str(index.get("active_document_id") or "").strip()
    record = documents.get(wanted) if wanted and isinstance(documents.get(wanted), dict) else None
    if record is not None and not document_id:
        status = str(record.get("status") or "active").strip().lower() or "active"
        if status != "active" or _workspace_voice_document_missing(group, record):
            record = None
            wanted = ""
    if record is None and create and not document_id:
        record = _create_voice_document_record(group, config=config)
        wanted = str(record.get("document_id") or "")
        documents[wanted] = record
        index["active_document_id"] = wanted
        _save_voice_documents_index(group, index)
    if record is None:
        raise ValueError("voice secretary document not found")
    return index, dict(record)


def _create_new_voice_document(
    group: Group,
    *,
    title: str = "",
    config: Optional[Dict[str, Any]] = None,
) -> tuple[Dict[str, Any], Dict[str, Any]]:
    index = _load_voice_documents_index(group)
    documents = index.setdefault("documents", {})
    record = _create_voice_document_record(group, title=title, config=config)
    doc_id = str(record.get("document_id") or "").strip()
    documents[doc_id] = record
    index["active_document_id"] = doc_id
    _save_voice_documents_index(group, index)
    return index, dict(record)


def _select_next_active_voice_document_id(index: Dict[str, Any], *, exclude_document_id: str = "") -> str:
    documents = index.get("documents") if isinstance(index.get("documents"), dict) else {}
    candidates = []
    for doc_id, record in documents.items():
        if not isinstance(record, dict):
            continue
        candidate_id = str(record.get("document_id") or doc_id or "").strip()
        if not candidate_id or candidate_id == exclude_document_id:
            continue
        if str(record.get("status") or "active").strip().lower() != "active":
            continue
        candidates.append((str(record.get("updated_at") or record.get("created_at") or ""), candidate_id))
    candidates.sort(reverse=True)
    return candidates[0][1] if candidates else ""


def _save_voice_document_record(group: Group, index: Dict[str, Any], record: Dict[str, Any]) -> None:
    doc_id = str(record.get("document_id") or "").strip()
    if not doc_id:
        raise ValueError("missing document_id")
    documents = index.setdefault("documents", {})
    documents[doc_id] = dict(record)
    _save_voice_documents_index(group, index)


def _archive_voice_document_storage(group: Group, record: Dict[str, Any]) -> Dict[str, Any]:
    doc_id = _safe_voice_document_id(record.get("document_id"))
    next_record = dict(record)
    sidecar_root = _voice_documents_root(group)
    sidecar_source = sidecar_root / doc_id
    sidecar_archive = sidecar_root / "archive" / doc_id
    storage_kind = str(next_record.get("storage_kind") or "").strip()

    if storage_kind == "workspace":
        current_rel = str(next_record.get("workspace_path") or "").strip()
        if current_rel and "/archive/" not in f"/{current_rel}":
            archived = _unique_voice_document_workspace_archive_path(group, workspace_path=current_rel)
            if archived is not None:
                archive_rel, archive_path = archived
                current_path = _resolve_voice_document_storage_path(group, next_record)
                if current_path.exists() and current_path.resolve() != archive_path.resolve():
                    archive_path.parent.mkdir(parents=True, exist_ok=True)
                    shutil.move(str(current_path), str(archive_path))
                next_record["archived_from_workspace_path"] = current_rel
                next_record["workspace_path"] = archive_rel
    else:
        archive_path = (sidecar_archive / "document.md").resolve()
        try:
            current_path: Optional[Path] = _resolve_voice_document_storage_path(group, next_record)
        except Exception:
            current_path = None
        if current_path is not None and current_path.exists() and current_path.resolve() != archive_path.resolve():
            archive_path.parent.mkdir(parents=True, exist_ok=True)
            shutil.move(str(current_path), str(archive_path))
        next_record["storage_path"] = str(archive_path)

    try:
        if sidecar_source.exists() and sidecar_source.is_dir() and sidecar_source.resolve() != sidecar_archive.resolve():
            sidecar_archive.parent.mkdir(parents=True, exist_ok=True)
            if not sidecar_archive.exists():
                shutil.move(str(sidecar_source), str(sidecar_archive))
            else:
                for child in sidecar_source.iterdir():
                    target = sidecar_archive / child.name
                    if not target.exists():
                        shutil.move(str(child), str(target))
                try:
                    sidecar_source.rmdir()
                except OSError:
                    pass
    except OSError:
        pass

    return next_record


def _append_voice_document_jsonl(group: Group, *, document_id: str, filename: str, item: Dict[str, Any]) -> str:
    path = _voice_documents_root(group) / _safe_voice_document_id(document_id) / filename
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("a", encoding="utf-8") as fh:
        fh.write(json.dumps(item, ensure_ascii=False, sort_keys=True) + "\n")
    return str(path)


def _write_voice_document_content(
    group: Group,
    index: Dict[str, Any],
    record: Dict[str, Any],
    *,
    content: str,
    reason: str,
    source_segment_id: str = "",
    source_path: str = "",
    by: str = "",
) -> Dict[str, Any]:
    next_content = str(content or "")
    if len(next_content) > _MAX_VOICE_DOCUMENT_CHARS:
        next_content = next_content[:_MAX_VOICE_DOCUMENT_CHARS].rstrip() + "\n"
    path = _resolve_voice_document_storage_path(group, record)
    path.parent.mkdir(parents=True, exist_ok=True)
    previous = _read_voice_document_content(group, record)
    if previous == next_content:
        record = dict(record)
        record.update(
            {
                "updated_at": utc_now_iso(),
                "content_sha256": _voice_document_content_sha(next_content),
                "content_chars": len(next_content),
            }
        )
        _save_voice_document_record(group, index, record)
        return record
    now = utc_now_iso()
    atomic_write_text(path, next_content, encoding="utf-8")
    revision_count = int(record.get("revision_count") or 0) + 1
    record = dict(record)
    record.update(
        {
            "updated_at": now,
            "revision_count": revision_count,
            "content_sha256": _voice_document_content_sha(next_content),
            "content_chars": len(next_content),
        }
    )
    if source_segment_id:
        record["last_source_segment_id"] = source_segment_id
    if source_path:
        record["last_source_path"] = source_path
    _append_voice_document_jsonl(
        group,
        document_id=str(record.get("document_id") or ""),
        filename="revisions.jsonl",
        item={
            "schema": 1,
            "document_id": str(record.get("document_id") or ""),
            "revision": revision_count,
            "created_at": now,
            "reason": reason,
            "by": by or _assistant_principal(ASSISTANT_ID_VOICE_SECRETARY),
            "sha256": record["content_sha256"],
            "source_segment_id": source_segment_id,
            "source_path": source_path,
        },
    )
    _save_voice_document_record(group, index, record)
    return record


def _append_voice_document_source(group: Group, *, record: Dict[str, Any], segment: Dict[str, Any], segment_path: str) -> None:
    doc_id = str(record.get("document_id") or "").strip()
    if not doc_id:
        return
    _append_voice_document_jsonl(
        group,
        document_id=doc_id,
        filename="sources.jsonl",
        item={
            "schema": 1,
            "document_id": doc_id,
            "created_at": utc_now_iso(),
            "source_type": "voice_transcript_segment",
            "segment": dict(segment),
            "segment_path": segment_path,
        },
    )


def _voice_auto_document_max_window_seconds(config: Dict[str, Any]) -> int:
    try:
        max_window_seconds = int(config.get("auto_document_max_window_seconds") or _DEFAULT_AUTO_DOCUMENT_MAX_WINDOW_SECONDS)
    except Exception:
        max_window_seconds = _DEFAULT_AUTO_DOCUMENT_MAX_WINDOW_SECONDS
    return min(max(_MIN_AUTO_DOCUMENT_MAX_WINDOW_SECONDS, max_window_seconds), 300)


def _voice_document_input_due(
    *,
    config: Dict[str, Any],
    window_text: str,
    intent_hint: str,
    trigger: Dict[str, Any],
    flush: bool,
    window_started_at: str,
    window_segment_count: int = 0,
    now: str,
) -> bool:
    clean = _clean_multiline_text(window_text, max_len=_MAX_TRANSCRIPT_SESSION_CHARS)
    if not clean:
        return False
    if _is_voice_transcript_tiny_filler(clean):
        return False
    trigger_kind = str(trigger.get("trigger_kind") or "").strip().lower()
    hard_flush = flush and trigger_kind in {"push_to_talk_stop", "service_transcript", "user_instruction", "manual_transcript", "document_switch"}
    if hard_flush:
        return True
    # Browser ASR often emits many short finals during continuous speech. For
    # normal meeting/speech capture, only a real quiet flush, a long time window,
    # or a hard character cap should wake the secretary actor. Explicit task-like
    # input keeps the fast path because responsiveness matters more there.
    fast_intent = intent_hint in _VOICE_SECRETARY_TASK_INTENTS or trigger_kind in {
        "service_transcript",
        "user_instruction",
        "manual_transcript",
        "document_switch",
    }
    try:
        min_chars = int(config.get("auto_document_min_chars") or _DEFAULT_AUTO_DOCUMENT_MIN_CHARS)
    except Exception:
        min_chars = _DEFAULT_AUTO_DOCUMENT_MIN_CHARS
    min_chars = min(max(40, min_chars), 8_000)
    if fast_intent and len(clean) >= _DEFAULT_AUTO_DOCUMENT_FAST_MIN_CHARS:
        return True
    if flush and int(window_segment_count or 0) >= _DEFAULT_AUTO_DOCUMENT_MIN_WINDOW_SEGMENTS and len(clean) >= _DEFAULT_AUTO_DOCUMENT_FAST_MIN_CHARS:
        return True
    if flush:
        return True
    if len(clean) >= _DEFAULT_AUTO_DOCUMENT_MAX_WINDOW_CHARS:
        return True
    started = parse_utc_iso(window_started_at)
    now_dt = parse_utc_iso(now)
    if started is None or now_dt is None:
        return False
    max_window_seconds = _voice_auto_document_max_window_seconds(config)
    return (now_dt - started).total_seconds() >= max_window_seconds


def _is_voice_transcript_tiny_filler(text: Any) -> bool:
    clean = _clean_multiline_text(text, max_len=120)
    if not clean:
        return True
    normalized = re.sub(r"[\s,.;:!?，。！？、…~\-—_]+", "", clean).lower()
    if not normalized:
        return True
    return normalized in _VOICE_TRANSCRIPT_TINY_FILLERS


def _voice_capture_continuity(*, segment_count: int, trigger: Dict[str, Any]) -> str:
    explicit = str(trigger.get("capture_continuity") or "").strip().lower()
    if explicit in {"continuous", "fragmented", "single_segment"}:
        return explicit
    if int(segment_count or 0) <= 1:
        return "single_segment"
    trigger_kind = str(trigger.get("trigger_kind") or "").strip().lower()
    if trigger_kind in {
        "meeting_window",
        "speech_end",
        "result_idle",
        "result_idle_fallback",
        "max_window",
        "service_transcript",
        "push_to_talk_stop",
    }:
        return "continuous"
    return "fragmented"


def _voice_suggested_document_mode(text: Any, *, trigger: Dict[str, Any], intent_hint: str, segment_count: int = 0) -> str:
    explicit = str(trigger.get("document_mode") or trigger.get("suggested_document_mode") or "").strip().lower()
    if explicit in _VOICE_DOCUMENT_MODES:
        return explicit
    clean = _clean_multiline_text(text, max_len=4_000)
    lowered = clean.lower()
    if intent_hint in {"peer_task", "secretary_task"}:
        return "research_brief"
    if any(token in lowered for token in ("interview", "q&a", "question:", "answer:")) or any(token in clean for token in ("インタビュー", "質問", "回答", "问答", "访谈")):
        return "interview_notes"
    if any(token in lowered for token in ("decision", "action item", "owner", "follow-up", "meeting")) or any(token in clean for token in ("決定", "宿題", "担当", "行动项", "负责人", "会议")):
        return "meeting_minutes"
    if int(segment_count or 0) >= _DEFAULT_AUTO_DOCUMENT_MIN_WINDOW_SEGMENTS or len(clean) >= 400:
        return "speech_summary"
    return "general_notes"


def _flush_voice_session_window(
    group: Group,
    *,
    session_id: str,
    session_entry: Dict[str, Any],
    assistant_config: Dict[str, Any],
    now: str,
    reason: str,
) -> bool:
    window_text = _clean_multiline_text(session_entry.get("window_text"), max_len=_MAX_TRANSCRIPT_SESSION_CHARS)
    if not window_text:
        return False
    if _is_voice_transcript_tiny_filler(window_text):
        _clear_voice_session_window(group, session_id=session_id, now=now)
        return False
    if not coerce_bool(assistant_config.get("auto_document_enabled"), default=True):
        return False

    effective_language = str(session_entry.get("language") or assistant_config.get("recognition_language") or "")
    target_document_path = str(session_entry.get("document_path") or "").strip()
    target_document_id = str(session_entry.get("document_id") or "").strip()
    window_segment_count = int(session_entry.get("window_segment_count") or 0)
    window_first_segment_id = str(session_entry.get("window_first_segment_id") or "")
    window_last_segment_id = str(
        session_entry.get("window_last_segment_id") or session_entry.get("last_segment_id") or ""
    )
    trigger_for_document = {
        "mode": "meeting",
        "trigger_kind": "meeting_window",
        "capture_mode": str(assistant_config.get("capture_mode") or "browser"),
        "recognition_backend": str(assistant_config.get("recognition_backend") or "browser_asr"),
        "client_session_id": session_id,
        "input_device_label": str(session_entry.get("input_device_label") or ""),
        "language": effective_language,
        "intent_hint": _infer_voice_transcript_intent(window_text, {}),
        "window_segment_count": window_segment_count,
        "instruction_policy": _voice_instruction_policy(),
    }
    document_intent_hint = _infer_voice_transcript_intent(window_text, trigger_for_document)
    if target_document_path:
        document_index, document_record = _find_voice_document_by_path(
            group,
            document_path=target_document_path,
            create=True,
            config=assistant_config,
        )
    else:
        document_index, document_record = _get_voice_document(
            group,
            document_id=target_document_id,
            create=True,
            config=assistant_config,
        )
    if str(document_record.get("status") or "active").strip().lower() == "active":
        document_index["active_document_id"] = str(document_record.get("document_id") or target_document_id)
        _save_voice_documents_index(group, document_index)

    document = _voice_document_public_record(group, document_record)
    source_segment = {
        "schema": 1,
        "segment_id": window_last_segment_id,
        "session_id": session_id,
        "group_id": group.group_id,
        "assistant_id": ASSISTANT_ID_VOICE_SECRETARY,
        "created_at": now,
        "updated_at": now,
        "text": window_text,
        "language": effective_language,
        "document_id": str(document.get("document_id") or target_document_id),
        "intent_hint": document_intent_hint,
        "is_final": True,
        "source": str(trigger_for_document.get("recognition_backend") or "browser_asr"),
        "by": "system",
        "source_segment_count": window_segment_count,
        "first_source_segment_id": window_first_segment_id,
        "last_source_segment_id": window_last_segment_id,
        "source_segment_range": (
            f"{window_first_segment_id}..{window_last_segment_id}"
            if window_first_segment_id and window_last_segment_id and window_first_segment_id != window_last_segment_id
            else window_first_segment_id or window_last_segment_id
        ),
    }
    _append_voice_document_source(
        group,
        record=document,
        segment=source_segment,
        segment_path=str(session_entry.get("last_segment_path") or ""),
    )
    _append_voice_input_event(
        group,
        kind="asr_transcript",
        text=window_text,
        document=document,
        language=effective_language,
        intent_hint=document_intent_hint,
        source=str(trigger_for_document.get("recognition_backend") or "browser_asr"),
        session_id=session_id,
        segment_id=window_last_segment_id,
        by="system",
        trigger=trigger_for_document,
        metadata={
            "source_segment_count": window_segment_count,
            "source_segment_range": str(source_segment.get("source_segment_range") or ""),
            "capture_continuity": _voice_capture_continuity(segment_count=window_segment_count, trigger=trigger_for_document),
            "suggested_document_mode": _voice_suggested_document_mode(
                window_text,
                trigger=trigger_for_document,
                intent_hint=document_intent_hint,
                segment_count=window_segment_count,
            ),
            "flush_reason": reason or "stale_window",
        },
    )
    append_event(
        group.ledger_path,
        kind="assistant.voice.document",
        group_id=group.group_id,
        scope_key="",
        by=_assistant_principal(ASSISTANT_ID_VOICE_SECRETARY),
        data={
            "assistant_id": ASSISTANT_ID_VOICE_SECRETARY,
            "document_path": str(document.get("document_path") or document.get("workspace_path") or ""),
            "action": "input_appended",
            "input_kind": "asr_transcript",
            "status": str(document.get("status") or "active"),
            "workspace_path": str(document.get("workspace_path") or ""),
            "title": str(document.get("title") or ""),
            "input_preview": window_text[:500],
        },
    )
    _clear_voice_session_window(
        group,
        session_id=session_id,
        now=now,
        last_document_id=str(document.get("document_id") or target_document_id),
    )
    return True


def _flush_stale_voice_session_windows(group: Group) -> int:
    assistant = _effective_assistant(group, ASSISTANT_ID_VOICE_SECRETARY)
    if not bool(assistant.get("enabled")):
        return 0
    assistant_config = assistant.get("config") if isinstance(assistant.get("config"), dict) else {}
    max_window_seconds = _voice_auto_document_max_window_seconds(assistant_config)
    now = utc_now_iso()
    now_dt = parse_utc_iso(now)
    if now_dt is None:
        return 0
    state = _load_runtime_state(group)
    sessions = state.get("voice_sessions") if isinstance(state.get("voice_sessions"), dict) else {}
    flushed = 0
    for raw_session_id, raw_session_entry in list(sessions.items()):
        if not isinstance(raw_session_entry, dict):
            continue
        session_id = str(raw_session_id or raw_session_entry.get("session_id") or "").strip()
        if not session_id:
            continue
        window_text = _clean_multiline_text(raw_session_entry.get("window_text"), max_len=_MAX_TRANSCRIPT_SESSION_CHARS)
        if not window_text:
            continue
        updated_at = str(raw_session_entry.get("updated_at") or raw_session_entry.get("window_started_at") or "")
        updated_dt = parse_utc_iso(updated_at)
        if updated_dt is None:
            continue
        if (now_dt - updated_dt).total_seconds() < max_window_seconds:
            continue
        if _flush_voice_session_window(
            group,
            session_id=session_id,
            session_entry=dict(raw_session_entry),
            assistant_config=assistant_config,
            now=now,
            reason="stale_window",
        ):
            flushed += 1
    return flushed


def _clear_voice_session_window(group: Group, *, session_id: str, now: str, last_document_id: str = "") -> None:
    state_after = _load_runtime_state(group)
    sessions_after = state_after.setdefault("voice_sessions", {})
    session_after = sessions_after.get(session_id) if isinstance(sessions_after.get(session_id), dict) else {}
    session_after = dict(session_after)
    session_after["window_text"] = ""
    session_after["window_started_at"] = ""
    session_after["window_segment_count"] = 0
    session_after["window_first_segment_id"] = ""
    session_after["window_last_segment_id"] = ""
    session_after["document_id"] = ""
    session_after["document_path"] = ""
    if last_document_id:
        session_after["last_document_id"] = last_document_id
        session_after["last_document_at"] = now
    session_after["last_flush_at"] = now
    session_after["updated_at"] = now
    sessions_after[session_id] = session_after
    _save_runtime_state(group, state_after)


def _voice_idle_review_candidate(group: Group) -> tuple[Dict[str, Any], Dict[str, Any]] | None:
    state = _load_voice_input_state(group)
    last_reviewed_seq = int(state.get("last_idle_review_input_seq") or 0)
    source_event = _latest_voice_input_event_for_idle_review(group, after_seq=last_reviewed_seq)
    if not source_event:
        return None
    document_path = str(source_event.get("document_path") or "").strip()
    if not document_path:
        return None
    try:
        index, record = _find_voice_document_by_path(group, document_path=document_path, create=False)
    except Exception:
        return None
    if str(record.get("status") or "active").strip().lower() != "active":
        return None
    record = dict(record)
    try:
        basis_seq = int(source_event.get("seq") or 0)
    except Exception:
        basis_seq = 0
    record["_idle_review_basis_input_seq"] = max(last_reviewed_seq, basis_seq)
    record["_idle_review_source_event_created_at"] = str(source_event.get("created_at") or "")
    return index, record


def voice_idle_review_available(group_id: str) -> bool:
    group = load_group(str(group_id or "").strip())
    if group is None:
        return False
    _flush_stale_voice_session_windows(group)
    assistant = _effective_assistant(group, ASSISTANT_ID_VOICE_SECRETARY)
    if not bool(assistant.get("enabled")):
        return False
    actor = get_voice_secretary_actor(group)
    if not isinstance(actor, dict) or not coerce_bool(actor.get("enabled"), default=True):
        return False
    return _voice_idle_review_candidate(group) is not None


def voice_idle_review_unavailable_reason(group_id: str) -> str:
    group = load_group(str(group_id or "").strip())
    if group is None:
        return "group_not_found"
    assistant = _effective_assistant(group, ASSISTANT_ID_VOICE_SECRETARY)
    if not bool(assistant.get("enabled")):
        return "voice_secretary_disabled"
    actor = get_voice_secretary_actor(group)
    if not isinstance(actor, dict):
        return "voice_secretary_actor_missing"
    if not coerce_bool(actor.get("enabled"), default=True):
        return "voice_secretary_actor_disabled"
    if _voice_idle_review_candidate(group) is None:
        return "no_idle_review_candidate"
    return "idle_review_unavailable"


def _voice_idle_review_source_text(record: Dict[str, Any], *, reasons: set[str]) -> str:
    title = str(record.get("title") or "Voice Secretary document").strip()
    reason_text = ", ".join(sorted({str(item or "").strip() for item in reasons if str(item or "").strip()}))
    if not reason_text:
        reason_text = "document_quiet"
    return (
        "Task:\n"
        f"Publishable document refinement request: {title}\n\n"
        "Inputs:\n"
        f"Reason: {reason_text}\n\n"
        "The stream is quiet. Read the current document and refine it into a coherent publishable artifact without lossy compression.\n"
        "Use evidence-bounded reconstruction from transcript, document context, group context, common knowledge, and verified lightweight research when needed; do not fabricate facts.\n"
        "Correct likely ASR term errors from context, merge fragmented points into themes, and compactly mark low-confidence entities, numbers, quotations, or dates.\n"
        "Preserve useful concrete details: named people, organizations, dates, numbers, examples, quoted claims, causal links, opposing views, constraints, risks, and follow-up needs.\n"
        "If the document has Pending Inputs, Open Questions, or items needing verification, resolve what can be resolved from current context or lightweight verified research, and keep only real user-decision items.\n\n"
        "Output constraint:\n"
        "Do not replace detail-rich material with a short executive summary; reorganize, enrich, de-duplicate, and fix structure instead.\n"
        "Never include transcript segment ids, source ranges, job ids, cursor/sequence ids, ASR chunk ids, or tool-processing notes in visible markdown.\n"
        "Skip only if the document is already polished, coherent, detail-rich, useful, and free of internal refs/logs.\n"
    )


def dispatch_voice_idle_review(
    group_id: str,
    reasons: set[str],
    source_event_id: str,
    trigger_class: str,
) -> bool:
    del trigger_class
    group = load_group(str(group_id or "").strip())
    if group is None:
        return False
    candidate = _voice_idle_review_candidate(group)
    if candidate is None:
        return True
    _index, record = candidate
    doc_id = str(record.get("document_id") or "").strip()
    if not doc_id:
        return True
    now = utc_now_iso()
    try:
        basis_input_seq = int(record.get("_idle_review_basis_input_seq") or 0)
    except Exception:
        basis_input_seq = 0
    state = _load_voice_input_state(group)
    if basis_input_seq <= 0:
        basis_input_seq = int(state.get("latest_seq") or 0)
    document = _voice_document_public_record(group, record)
    trigger = {
        "mode": "meeting",
        "trigger_kind": "idle_review",
        "capture_mode": "assistant",
        "recognition_backend": "voice_secretary_idle_review",
        "client_session_id": "voice-secretary-idle-review",
        "language": str(record.get("language") or ""),
        "intent_hint": "idle_review",
        "instruction_policy": _voice_instruction_policy(),
        "source_event_id": str(source_event_id or ""),
        "idle_review_basis_input_seq": basis_input_seq,
    }
    source_segment = {
        "schema": 1,
        "segment_id": f"idle-review-{uuid.uuid4().hex}",
        "session_id": "voice-secretary-idle-review",
        "group_id": group.group_id,
        "assistant_id": ASSISTANT_ID_VOICE_SECRETARY,
        "created_at": now,
        "updated_at": now,
        "text": _voice_idle_review_source_text(record, reasons=reasons),
        "language": str(trigger.get("language") or ""),
        "intent_hint": "idle_review",
        "is_final": True,
        "source": "idle_review",
        "by": "system",
    }
    try:
        _append_voice_input_event(
            group,
            kind="idle_review",
            text=str(source_segment.get("text") or ""),
            document=document,
            language=str(trigger.get("language") or ""),
            intent_hint="idle_review",
            source="voice_secretary_idle_review",
            session_id="voice-secretary-idle-review",
            segment_id=str(source_segment.get("segment_id") or ""),
            by="system",
            trigger=trigger,
            metadata={
                "idle_review_basis_input_seq": basis_input_seq,
                "source_event_id": str(source_event_id or ""),
            },
        )
    except Exception:
        return False
    state = _load_voice_input_state(group)
    state["last_idle_review_at"] = now
    state["last_idle_review_input_seq"] = max(int(state.get("last_idle_review_input_seq") or 0), basis_input_seq)
    state["flush_count_since_idle_review"] = 0
    _save_voice_input_state(group, state)
    append_event(
        group.ledger_path,
        kind="assistant.voice.document",
        group_id=group.group_id,
        scope_key="",
        by=_assistant_principal(ASSISTANT_ID_VOICE_SECRETARY),
        data={
            "assistant_id": ASSISTANT_ID_VOICE_SECRETARY,
            "document_path": str(document.get("document_path") or document.get("workspace_path") or ""),
            "action": "input_appended",
            "input_kind": "idle_review",
            "status": str(record.get("status") or "active"),
            "workspace_path": str(record.get("workspace_path") or ""),
            "title": str(record.get("title") or ""),
        },
    )
    try:
        from ..pet import assistive_jobs

        assistive_jobs.mark_job_completed(group.group_id, assistive_jobs.JOB_KIND_VOICE_IDLE_REVIEW)
    except Exception:
        pass
    return True


def _prune_voice_session_storage(group: Group) -> None:
    ttl = _voice_retention_ttl_seconds(group)
    if ttl <= 0:
        return
    root = ensure_home() / "voice-secretary" / group.group_id
    if not root.exists():
        return
    cutoff = time.time() - float(ttl)
    for child in root.iterdir():
        try:
            if not child.is_dir() or child.name == "documents":
                continue
            latest_mtime = child.stat().st_mtime
            for item in child.rglob("*"):
                try:
                    latest_mtime = max(latest_mtime, item.stat().st_mtime)
                except OSError:
                    continue
            if latest_mtime < cutoff:
                shutil.rmtree(child, ignore_errors=True)
        except OSError:
            continue


def _append_voice_segment_jsonl(group: Group, *, session_id: str, segment: Dict[str, Any]) -> str:
    _prune_voice_session_storage(group)
    path = _voice_session_dir(group, session_id) / "transcripts" / "segments.jsonl"
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("a", encoding="utf-8") as fh:
        fh.write(json.dumps(segment, ensure_ascii=False, sort_keys=True) + "\n")
    return _display_voice_path(path)


def _display_voice_path(path: Path) -> str:
    resolved = Path(path).expanduser().resolve()
    raw_home = str(os.environ.get("CCCC_HOME") or "").strip()
    if raw_home:
        raw_home_path = Path(raw_home).expanduser()
        try:
            relative = resolved.relative_to(raw_home_path.resolve())
        except ValueError:
            return str(resolved)
        return str(raw_home_path / relative)
    return str(resolved)


def _append_voice_window_text(previous: Any, next_text: str) -> str:
    prev = _clean_multiline_text(previous, max_len=_MAX_TRANSCRIPT_SESSION_CHARS)
    text = _clean_multiline_text(next_text, max_len=_MAX_TRANSCRIPT_SESSION_CHARS)
    if not text:
        return prev
    combined = text if not prev else f"{prev}\n{text}"
    if len(combined) <= _MAX_TRANSCRIPT_SESSION_CHARS:
        return combined
    return combined[-_MAX_TRANSCRIPT_SESSION_CHARS:].lstrip()


def _voice_instruction_policy() -> Dict[str, Any]:
    return {
        "default": "classify_each_job_before_writing",
        "memo": "synthesize_into_working_document",
        "document_instruction": "modify_create_or_archive_voice_documents_when_clear",
        "secretary_task": "handle_safe_secretary_scope_work_yourself_when_transcript_backlog_is_clear",
        "peer_task": "handoff_only_when_work_belongs_to_foreman_or_a_concrete_peer",
        "mixed": "split_memo_secretary_work_and_peer_handoffs",
        "document_updates": "safe_to_apply_when_instruction_or_memo_is_clear",
        "new_document": "create_only_when_separate_deliverable_is_clear",
        "handoff": "use_voice_secretary_request_for_explicit_user_requested_peer_or_foreman_work",
        "request_notify": "use_voice_secretary_request_only_for_explicit_handoff_to_foreman_or_one_actor",
        "queue_priority": "while_transcript_jobs_are_pending_prioritize_intake_then_process_secretary_queue",
        "unclear": "record_as_context_or_open_question_do_not_notify_peers",
    }


def _matches_any_pattern(text: str, patterns: tuple[re.Pattern[str], ...]) -> bool:
    return any(pattern.search(text) for pattern in patterns)


def _infer_voice_transcript_intent(text: Any, trigger: Optional[Dict[str, Any]] = None) -> str:
    clean = _clean_multiline_text(text, max_len=4_000)
    trigger_dict = trigger if isinstance(trigger, dict) else {}
    if not clean:
        return "unclear"
    explicit = str(trigger_dict.get("intent_hint") or "").strip().lower()
    if explicit in {"memo", "document_instruction", "secretary_task", "peer_task", "mixed", "unclear"}:
        return explicit
    if explicit in {"task_instruction", "action_request"}:
        return "peer_task"
    document_instruction = _matches_any_pattern(clean, _VOICE_DOCUMENT_INSTRUCTION_PATTERNS)
    secretary_task = _matches_any_pattern(clean, _VOICE_SECRETARY_TASK_PATTERNS)
    peer_task = _matches_any_pattern(clean, _VOICE_PEER_TASK_PATTERNS)
    if document_instruction and (secretary_task or peer_task):
        return "mixed"
    if secretary_task and peer_task:
        return "mixed"
    if document_instruction:
        return "document_instruction"
    if peer_task:
        return "peer_task"
    if secretary_task:
        return "secretary_task"
    trigger_kind = str(trigger_dict.get("trigger_kind") or "").strip().lower()
    if trigger_kind == "user_instruction":
        return "document_instruction"
    return "memo"


def handle_assistant_state(args: Dict[str, Any]) -> DaemonResponse:
    group_id = str(args.get("group_id") or "").strip()
    assistant_id = _normalize_assistant_id(args.get("assistant_id"))
    prompt_request_id = str(args.get("prompt_request_id") or args.get("request_id") or "").strip()
    if not group_id:
        return _error("missing_group_id", "missing group_id")
    group = load_group(group_id)
    if group is None:
        return _error("group_not_found", f"group not found: {group_id}")
    if not assistant_id or assistant_id == ASSISTANT_ID_VOICE_SECRETARY:
        _flush_stale_voice_session_windows(group)
        _maybe_emit_voice_input_retry_notify(group)
    runtime_state = _load_runtime_state(group)
    if assistant_id:
        if assistant_id not in _ASSISTANT_DEFAULTS:
            return _error("assistant_not_found", f"assistant not found: {assistant_id}")
        assistant = _effective_assistant(group, assistant_id, runtime_state=runtime_state)
        documents = _retained_voice_documents(group) if assistant_id == ASSISTANT_ID_VOICE_SECRETARY else []
        document_index = _load_voice_documents_index(group) if assistant_id == ASSISTANT_ID_VOICE_SECRETARY else {}
        active_document_id, active_record = (
            _active_voice_document_from_index(group, document_index)
            if assistant_id == ASSISTANT_ID_VOICE_SECRETARY
            else ("", None)
        )
        active_document_path = _voice_document_path(active_record) if isinstance(active_record, dict) else ""
        input_state = _load_voice_input_state(group) if assistant_id == ASSISTANT_ID_VOICE_SECRETARY else {}
        prompt_draft = (
            _pending_voice_prompt_draft_by_request(group, request_id=prompt_request_id, runtime_state=runtime_state)
            if assistant_id == ASSISTANT_ID_VOICE_SECRETARY
            else {}
        )
        ask_requests = _voice_ask_requests_public(runtime_state) if assistant_id == ASSISTANT_ID_VOICE_SECRETARY else []
        service_models = list_voice_models() if assistant_id == ASSISTANT_ID_VOICE_SECRETARY else []
        service_runtimes = list_voice_runtime_statuses() if assistant_id == ASSISTANT_ID_VOICE_SECRETARY else []
        service_runtime = get_voice_runtime_status() if assistant_id == ASSISTANT_ID_VOICE_SECRETARY else {}
        return DaemonResponse(
            ok=True,
            result={
                "group_id": group.group_id,
                "assistant": assistant,
                "documents": documents,
                "documents_by_id": {str(item.get("document_id")): item for item in documents},
                "documents_by_path": {str(item.get("document_path")): item for item in documents if str(item.get("document_path") or "").strip()},
                "active_document_id": active_document_id,
                "capture_target_document_id": active_document_id,
                "active_document_path": active_document_path,
                "capture_target_document_path": active_document_path,
                "new_input_available": int(input_state.get("latest_seq") or 0) > _voice_input_delivery_cursor(input_state),
                "input_timing": _voice_input_timing_public(input_state),
                "prompt_draft": prompt_draft,
                "ask_requests": ask_requests,
                "latest_ask_request": ask_requests[0] if ask_requests else {},
                "service_models": service_models,
                "service_models_by_id": {str(item.get("model_id")): item for item in service_models},
                "service_runtime": service_runtime,
                "service_runtimes": service_runtimes,
                "service_runtimes_by_id": {str(item.get("runtime_id")): item for item in service_runtimes},
            },
        )
    assistants = [
        _effective_assistant(group, item, runtime_state=runtime_state)
        for item in sorted(_ASSISTANT_DEFAULTS.keys())
    ]
    documents = _retained_voice_documents(group)
    document_index = _load_voice_documents_index(group)
    active_document_id, active_record = _active_voice_document_from_index(group, document_index)
    active_document_path = _voice_document_path(active_record) if isinstance(active_record, dict) else ""
    input_state = _load_voice_input_state(group)
    prompt_draft = _pending_voice_prompt_draft_by_request(group, request_id=prompt_request_id, runtime_state=runtime_state)
    ask_requests = _voice_ask_requests_public(runtime_state)
    service_models = list_voice_models()
    service_runtimes = list_voice_runtime_statuses()
    service_runtime = get_voice_runtime_status()
    return DaemonResponse(
        ok=True,
        result={
            "group_id": group.group_id,
            "assistants": assistants,
            "assistants_by_id": {str(item.get("assistant_id")): item for item in assistants},
            "documents": documents,
            "documents_by_id": {str(item.get("document_id")): item for item in documents},
            "documents_by_path": {str(item.get("document_path")): item for item in documents if str(item.get("document_path") or "").strip()},
            "active_document_id": active_document_id,
            "capture_target_document_id": active_document_id,
            "active_document_path": active_document_path,
            "capture_target_document_path": active_document_path,
            "new_input_available": int(input_state.get("latest_seq") or 0) > _voice_input_delivery_cursor(input_state),
            "input_timing": _voice_input_timing_public(input_state),
            "prompt_draft": prompt_draft,
            "ask_requests": ask_requests,
            "latest_ask_request": ask_requests[0] if ask_requests else {},
            "service_models": service_models,
            "service_models_by_id": {str(item.get("model_id")): item for item in service_models},
            "service_runtime": service_runtime,
            "service_runtimes": service_runtimes,
            "service_runtimes_by_id": {str(item.get("runtime_id")): item for item in service_runtimes},
        },
    )


def handle_assistant_voice_model_install(args: Dict[str, Any]) -> DaemonResponse:
    group_id = str(args.get("group_id") or "").strip()
    by = str(args.get("by") or "user").strip()
    model_id = str(args.get("model_id") or "").strip()
    background = coerce_bool(args.get("background"), default=False)
    if not group_id:
        return _error("missing_group_id", "missing group_id")
    if not model_id:
        return _error("missing_model_id", "missing model_id")
    group = load_group(group_id)
    if group is None:
        return _error("group_not_found", f"group not found: {group_id}")
    try:
        require_group_permission(group, by=by, action="group.settings_update")
        if background:
            installed = _start_voice_model_install_background(model_id)
        else:
            installed = install_voice_model(model_id)
    except VoiceModelError as exc:
        return _error(exc.code, exc.message, details=exc.details)
    except VoiceRuntimeDepsError as exc:
        return _error(exc.code, exc.message, details=exc.details)
    except Exception as exc:
        return _error("assistant_voice_model_install_failed", str(exc))
    return DaemonResponse(
        ok=True,
        result={
            "group_id": group.group_id,
            "assistant": _effective_assistant(group, ASSISTANT_ID_VOICE_SECRETARY),
            "model": installed,
            "service_runtime": get_voice_runtime_status(),
        },
    )


def handle_assistant_voice_runtime_install(args: Dict[str, Any]) -> DaemonResponse:
    group_id = str(args.get("group_id") or "").strip()
    by = str(args.get("by") or "user").strip()
    runtime_id = str(args.get("runtime_id") or "").strip() or VOICE_RUNTIME_ID_SHERPA_ONNX_STREAMING
    background = coerce_bool(args.get("background"), default=True)
    if not group_id:
        return _error("missing_group_id", "missing group_id")
    group = load_group(group_id)
    if group is None:
        return _error("group_not_found", f"group not found: {group_id}")
    try:
        require_group_permission(group, by=by, action="group.settings_update")
        runtime = (
            _start_voice_runtime_install_background(runtime_id)
            if background
            else install_voice_runtime_deps(runtime_id)
        )
    except VoiceRuntimeDepsError as exc:
        return _error(exc.code, exc.message, details=exc.details)
    except Exception as exc:
        return _error("assistant_voice_runtime_install_failed", str(exc))
    return DaemonResponse(
        ok=True,
        result={
            "group_id": group.group_id,
            "assistant": _effective_assistant(group, ASSISTANT_ID_VOICE_SECRETARY),
            "service_runtime": runtime,
        },
    )


def _start_voice_model_install_background(model_id: str) -> Dict[str, Any]:
    model_id = str(model_id or "").strip()
    with _VOICE_MODEL_INSTALL_LOCK:
        existing = _VOICE_MODEL_INSTALL_THREADS.get(model_id)
        if existing is not None and existing.is_alive():
            return get_voice_model_status(model_id)
        started = begin_voice_model_install(model_id)

        def _run() -> None:
            try:
                install_voice_model(model_id)
            except Exception:
                logger.exception("voice model install failed in background: model_id=%s", model_id)
            finally:
                with _VOICE_MODEL_INSTALL_LOCK:
                    current = _VOICE_MODEL_INSTALL_THREADS.get(model_id)
                    if current is threading.current_thread():
                        _VOICE_MODEL_INSTALL_THREADS.pop(model_id, None)

        thread = threading.Thread(target=_run, name=f"voice-model-install-{model_id}", daemon=True)
        _VOICE_MODEL_INSTALL_THREADS[model_id] = thread
        thread.start()
        return started


def _start_voice_runtime_install_background(runtime_id: str) -> Dict[str, Any]:
    runtime_id = str(runtime_id or "").strip() or VOICE_RUNTIME_ID_SHERPA_ONNX_STREAMING
    if runtime_id not in {VOICE_RUNTIME_ID_SHERPA_ONNX_STREAMING}:
        raise VoiceRuntimeDepsError("voice_runtime_unknown", f"unknown voice runtime: {runtime_id}")
    with _VOICE_RUNTIME_INSTALL_LOCK:
        existing = _VOICE_RUNTIME_INSTALL_THREADS.get(runtime_id)
        if existing is not None and existing.is_alive():
            return get_voice_runtime_status(runtime_id)

        def _run() -> None:
            try:
                install_voice_runtime_deps(runtime_id)
            except Exception:
                logger.exception("voice runtime dependency install failed in background: runtime_id=%s", runtime_id)
            finally:
                with _VOICE_RUNTIME_INSTALL_LOCK:
                    current = _VOICE_RUNTIME_INSTALL_THREADS.get(runtime_id)
                    if current is threading.current_thread():
                        _VOICE_RUNTIME_INSTALL_THREADS.pop(runtime_id, None)

        thread = threading.Thread(target=_run, name=f"voice-runtime-install-{runtime_id}", daemon=True)
        _VOICE_RUNTIME_INSTALL_THREADS[runtime_id] = thread
        thread.start()
        return {
            **get_voice_runtime_status(runtime_id),
            "status": "installing",
        }


def handle_assistant_settings_update(
    args: Dict[str, Any],
    *,
    effective_runner_kind: Callable[[str], str],
    start_actor_process: Callable[..., dict[str, Any]],
    load_actor_private_env: Callable[[str, str], Dict[str, str]],
    update_actor_private_env: Callable[..., Dict[str, str]],
    delete_actor_private_env: Callable[[str, str], None],
    get_actor_profile: Callable[[str], Optional[Dict[str, Any]]],
    load_actor_profile_secrets: Callable[[str], Dict[str, str]],
    remove_headless_state: Callable[[str, str], None],
    remove_pty_state_if_pid: Callable[..., None],
) -> DaemonResponse:
    group_id = str(args.get("group_id") or "").strip()
    by = str(args.get("by") or "user").strip()
    assistant_id = _normalize_assistant_id(args.get("assistant_id"))
    patch = args.get("patch") if isinstance(args.get("patch"), dict) else {}
    if not group_id:
        return _error("missing_group_id", "missing group_id")
    if assistant_id not in _ASSISTANT_DEFAULTS:
        return _error("assistant_not_found", f"assistant not found: {assistant_id}")
    if not patch:
        return _error("invalid_patch", "empty patch")

    group = load_group(group_id)
    if group is None:
        return _error("group_not_found", f"group not found: {group_id}")
    entry: Dict[str, Any] = {}
    try:
        require_group_permission(group, by=by, action="group.settings_update")
        if assistant_id == ASSISTANT_ID_PET:
            return _error(
                "assistant_settings_read_only",
                "pet assistant settings are mirrored from desktop_pet_enabled during M0",
                details={"settings_source": "group_settings_update.desktop_pet_enabled"},
            )
        unknown = set(patch.keys()) - {"enabled", "config"}
        if unknown:
            return _error("invalid_patch", "invalid patch keys", details={"unknown_keys": sorted(unknown)})

        assistants = group.doc.get("assistants") if isinstance(group.doc.get("assistants"), dict) else {}
        assistants = dict(assistants)
        entry = assistants.get(assistant_id) if isinstance(assistants.get(assistant_id), dict) else {}
        entry_before_present = assistant_id in assistants and isinstance(assistants.get(assistant_id), dict)
        entry_before = dict(entry) if isinstance(entry, dict) else {}
        voice_state_before = (
            capture_voice_secretary_actor_state(group, load_actor_private_env=load_actor_private_env)
            if assistant_id == ASSISTANT_ID_VOICE_SECRETARY
            else None
        )
        voice_actor_before = get_voice_secretary_actor(group) if assistant_id == ASSISTANT_ID_VOICE_SECRETARY else None
        voice_was_running = (
            is_voice_secretary_actor_running(group, actor=voice_actor_before, effective_runner_kind=effective_runner_kind)
            if assistant_id == ASSISTANT_ID_VOICE_SECRETARY
            else False
        )
        entry = dict(entry)
        if "enabled" in patch:
            entry["enabled"] = coerce_bool(patch.get("enabled"), default=False)
        if "config" in patch:
            default_config = dict((_ASSISTANT_DEFAULTS[assistant_id].get("config") or {}))
            prior_config = entry.get("config") if isinstance(entry.get("config"), dict) else {}
            base_config = (
                _effective_voice_config(prior_config, base=default_config)
                if assistant_id == ASSISTANT_ID_VOICE_SECRETARY
                else {**default_config, **prior_config}
            )
            entry["config"] = _normalize_voice_config(patch.get("config"), base=base_config)
        assistants[assistant_id] = entry
        group.doc["assistants"] = assistants
        if assistant_id == ASSISTANT_ID_VOICE_SECRETARY:
            try:
                desired_enabled = coerce_bool(entry.get("enabled"), default=False)
                resolve_before_start = lambda grp, aid, caller_id="", is_admin=False: resolve_linked_actor_before_start(
                    grp,
                    aid,
                    get_actor_profile=get_actor_profile,
                    load_actor_profile_secrets=load_actor_profile_secrets,
                    update_actor_private_env=update_actor_private_env,
                    caller_id=caller_id,
                    is_admin=is_admin,
                )
                if not desired_enabled:
                    if isinstance(voice_actor_before, dict):
                        stop_voice_secretary_actor_runtime(
                            group,
                            actor=voice_actor_before,
                            by=by,
                            effective_runner_kind=effective_runner_kind,
                            remove_headless_state=remove_headless_state,
                            remove_pty_state_if_pid=remove_pty_state_if_pid,
                            emit_event=voice_was_running,
                        )
                    sync_voice_secretary_actor_from_foreman(
                        group,
                        effective_runner_kind=effective_runner_kind,
                        load_actor_private_env=load_actor_private_env,
                        update_actor_private_env=update_actor_private_env,
                        delete_actor_private_env=delete_actor_private_env,
                        resolve_linked_actor_before_start=resolve_before_start,
                        caller_id=str(args.get("caller_id") or "").strip(),
                        is_admin=coerce_bool(args.get("is_admin"), default=False),
                    )
                else:
                    sync_voice_secretary_actor_from_foreman(
                        group,
                        effective_runner_kind=effective_runner_kind,
                        load_actor_private_env=load_actor_private_env,
                        update_actor_private_env=update_actor_private_env,
                        delete_actor_private_env=delete_actor_private_env,
                        resolve_linked_actor_before_start=resolve_before_start,
                        caller_id=str(args.get("caller_id") or "").strip(),
                        is_admin=coerce_bool(args.get("is_admin"), default=False),
                    )
                    voice_actor_after = get_voice_secretary_actor(group)
                    voice_private_env_after = load_actor_private_env(group.group_id, VOICE_SECRETARY_ACTOR_ID)
                    if coerce_bool(group.doc.get("running"), default=False) and isinstance(voice_actor_after, dict):
                        voice_private_env_before = (
                            voice_state_before.get("private_env")
                            if isinstance(voice_state_before, dict) and isinstance(voice_state_before.get("private_env"), dict)
                            else {}
                        )
                        voice_actor_before_doc = (
                            voice_state_before.get("actor_doc")
                            if isinstance(voice_state_before, dict) and isinstance(voice_state_before.get("actor_doc"), dict)
                            else None
                        )
                        config_changed = voice_secretary_runtime_changed(
                            voice_actor_before_doc,
                            voice_actor_after,
                            before_private_env=voice_private_env_before,
                            after_private_env=voice_private_env_after,
                        )
                        if voice_was_running and config_changed:
                            stop_voice_secretary_actor_runtime(
                                group,
                                actor=voice_actor_before,
                                by=by,
                                effective_runner_kind=effective_runner_kind,
                                remove_headless_state=remove_headless_state,
                                remove_pty_state_if_pid=remove_pty_state_if_pid,
                                emit_event=True,
                            )
                        if (not voice_was_running) or config_changed:
                            start_result = start_actor_process(
                                group,
                                VOICE_SECRETARY_ACTOR_ID,
                                command=list(voice_actor_after.get("command") or []),
                                env=dict(voice_actor_after.get("env") or {}),
                                runner=str(voice_actor_after.get("runner") or "pty"),
                                runtime=str(voice_actor_after.get("runtime") or "codex"),
                                by=by,
                                caller_id=str(args.get("caller_id") or "").strip(),
                                is_admin=coerce_bool(args.get("is_admin"), default=False),
                            )
                            if not bool(start_result.get("success")):
                                start_error = str(start_result.get("error") or "").strip()
                                raise RuntimeError(f"failed to start voice secretary actor: {start_error or 'unknown error'}")
                next_config = entry.get("config") if isinstance(entry.get("config"), dict) else {}
                next_backend = str(next_config.get("recognition_backend") or "").strip()
                if not desired_enabled or next_backend != "assistant_service_local_asr":
                    stop_voice_service(group)
            except Exception:
                restored_assistants = group.doc.get("assistants") if isinstance(group.doc.get("assistants"), dict) else {}
                restored_assistants = dict(restored_assistants)
                if entry_before_present:
                    restored_assistants[assistant_id] = entry_before
                else:
                    restored_assistants.pop(assistant_id, None)
                group.doc["assistants"] = restored_assistants
                restored_actor = restore_voice_secretary_actor_state(
                    group,
                    voice_state_before,
                    update_actor_private_env=update_actor_private_env,
                    delete_actor_private_env=delete_actor_private_env,
                )
                if (
                    coerce_bool(entry_before.get("enabled"), default=False)
                    and voice_was_running
                    and isinstance(restored_actor, dict)
                ):
                    restart_result = start_actor_process(
                        group,
                        VOICE_SECRETARY_ACTOR_ID,
                        command=list(restored_actor.get("command") or []),
                        env=dict(restored_actor.get("env") or {}),
                        runner=str(restored_actor.get("runner") or "pty"),
                        runtime=str(restored_actor.get("runtime") or "codex"),
                        by=by,
                        caller_id=str(args.get("caller_id") or "").strip(),
                        is_admin=coerce_bool(args.get("is_admin"), default=False),
                    )
                    if not bool(restart_result.get("success")):
                        raise RuntimeError(
                            f"voice secretary start failed and rollback restart failed: {restart_result.get('error') or 'unknown error'}"
                        )
                try:
                    group.save()
                except Exception:
                    pass
                raise
        group.save()
    except Exception as exc:
        return _error("assistant_settings_update_failed", str(exc))

    event = append_event(
        group.ledger_path,
        kind="assistant.settings_update",
        group_id=group.group_id,
        scope_key="",
        by=by,
        data={
            "assistant_id": assistant_id,
            "enabled": entry.get("enabled") if "enabled" in patch else None,
            "config_keys": sorted((patch.get("config") or {}).keys()) if isinstance(patch.get("config"), dict) else [],
        },
    )
    return DaemonResponse(
        ok=True,
        result={"group_id": group.group_id, "assistant": _effective_assistant(group, assistant_id), "event": event},
    )


def handle_assistant_status_update(args: Dict[str, Any]) -> DaemonResponse:
    group_id = str(args.get("group_id") or "").strip()
    by = str(args.get("by") or "").strip()
    assistant_id = _normalize_assistant_id(args.get("assistant_id"))
    lifecycle = str(args.get("lifecycle") or "").strip().lower()
    health = args.get("health") if isinstance(args.get("health"), dict) else {}
    if not group_id:
        return _error("missing_group_id", "missing group_id")
    if assistant_id not in _ASSISTANT_DEFAULTS:
        return _error("assistant_not_found", f"assistant not found: {assistant_id}")
    if lifecycle not in _VALID_LIFECYCLES:
        return _error("invalid_lifecycle", "invalid assistant lifecycle")

    group = load_group(group_id)
    if group is None:
        return _error("group_not_found", f"group not found: {group_id}")
    try:
        _require_status_permission(group, assistant_id=assistant_id, by=by)
        state = _load_runtime_state(group)
        assistants = state.setdefault("assistants", {})
        entry = assistants.get(assistant_id) if isinstance(assistants.get(assistant_id), dict) else {}
        entry = dict(entry)
        entry["lifecycle"] = lifecycle
        entry["health"] = dict(health)
        entry["updated_at"] = utc_now_iso()
        assistants[assistant_id] = entry
        _save_runtime_state(group, state)
    except Exception as exc:
        return _error("assistant_status_update_failed", str(exc))

    event = append_event(
        group.ledger_path,
        kind="assistant.status_update",
        group_id=group.group_id,
        scope_key="",
        by=by or _assistant_principal(assistant_id),
        data={
            "assistant_id": assistant_id,
            "lifecycle": lifecycle,
            "health_keys": sorted(health.keys()),
        },
    )
    return DaemonResponse(
        ok=True,
        result={"group_id": group.group_id, "assistant": _effective_assistant(group, assistant_id), "event": event},
    )


def _decode_audio_base64(raw: Any) -> bytes:
    text = str(raw or "").strip()
    if "," in text and text.split(",", 1)[0].startswith("data:"):
        text = text.split(",", 1)[1]
    if not text:
        raise ValueError("audio_base64 cannot be empty")
    if len(text) > _MAX_AUDIO_BYTES * 2:
        raise ValueError("audio payload is too large")
    try:
        audio = base64.b64decode(text, validate=True)
    except (binascii.Error, ValueError) as exc:
        raise ValueError("audio_base64 is invalid") from exc
    if not audio:
        raise ValueError("audio payload cannot be empty")
    if len(audio) > _MAX_AUDIO_BYTES:
        raise ValueError("audio payload is too large")
    return audio


def _set_voice_assistant_runtime(group: Group, *, lifecycle: str, health: Dict[str, Any]) -> Dict[str, Any]:
    now = utc_now_iso()
    state = _load_runtime_state(group)
    assistants = state.setdefault("assistants", {})
    entry = assistants.get(ASSISTANT_ID_VOICE_SECRETARY) if isinstance(assistants.get(ASSISTANT_ID_VOICE_SECRETARY), dict) else {}
    entry = dict(entry)
    entry["lifecycle"] = lifecycle
    entry["health"] = dict(health)
    entry["updated_at"] = now
    assistants[ASSISTANT_ID_VOICE_SECRETARY] = entry
    _save_runtime_state(group, state)
    return _effective_assistant(group, ASSISTANT_ID_VOICE_SECRETARY, runtime_state=state)


def handle_assistant_voice_transcribe(args: Dict[str, Any]) -> DaemonResponse:
    group_id = str(args.get("group_id") or "").strip()
    by = str(args.get("by") or "user").strip()
    mime_type = str(args.get("mime_type") or "application/octet-stream").strip() or "application/octet-stream"
    language = str(args.get("language") or "").strip()
    if not group_id:
        return _error("missing_group_id", "missing group_id")
    group = load_group(group_id)
    if group is None:
        return _error("group_not_found", f"group not found: {group_id}")
    try:
        _require_status_permission(group, assistant_id=ASSISTANT_ID_VOICE_SECRETARY, by=by)
        audio_bytes = _decode_audio_base64(args.get("audio_base64") or args.get("audio_b64"))
    except Exception as exc:
        return _error("assistant_voice_transcribe_failed", str(exc))
    assistant = _effective_assistant(group, ASSISTANT_ID_VOICE_SECRETARY)
    if not bool(assistant.get("enabled")):
        return _error("assistant_disabled", "voice_secretary is disabled")
    assistant_config = assistant.get("config") if isinstance(assistant.get("config"), dict) else {}
    backend = str(assistant_config.get("recognition_backend") or "browser_asr").strip()
    if backend != "assistant_service_local_asr":
        return _error(
            "assistant_voice_backend_mismatch",
            "assistant_voice_transcribe requires recognition_backend=assistant_service_local_asr",
            details={"recognition_backend": backend},
        )

    _set_voice_assistant_runtime(
        group,
        lifecycle="working",
        health={
            "status": "transcribing",
            "backend": "assistant_service_local_asr",
            "mime_type": mime_type,
            "language": language,
        },
    )
    try:
        result = transcribe_voice_audio(group, audio_bytes=audio_bytes, mime_type=mime_type, language=language)
    except VoiceServiceRuntimeError as exc:
        assistant_after = _set_voice_assistant_runtime(
            group,
            lifecycle="failed",
            health={
                "status": "transcribe_failed",
                "backend": "assistant_service_local_asr",
                "error": {"code": exc.code, "message": exc.message, "details": exc.details},
                "service": read_voice_service_state(group),
            },
        )
        return _error(exc.code, exc.message, details={**exc.details, "assistant": assistant_after})
    except Exception as exc:
        assistant_after = _set_voice_assistant_runtime(
            group,
            lifecycle="failed",
            health={
                "status": "transcribe_failed",
                "backend": "assistant_service_local_asr",
                "error": {"code": "assistant_voice_transcribe_failed", "message": str(exc), "details": {}},
                "service": read_voice_service_state(group),
            },
        )
        return _error("assistant_voice_transcribe_failed", str(exc), details={"assistant": assistant_after})

    service_state = result.get("service") if isinstance(result.get("service"), dict) else read_voice_service_state(group)
    assistant_after = _set_voice_assistant_runtime(
        group,
        lifecycle="idle",
        health={
            "status": "idle",
            "last_transcription_at": utc_now_iso(),
            "backend": "assistant_service_local_asr",
            "service": service_state,
        },
    )
    return DaemonResponse(
        ok=True,
        result={
            "group_id": group.group_id,
            "assistant": assistant_after,
            "transcript": str(result.get("transcript") or "").strip(),
            "mime_type": str(result.get("mime_type") or mime_type),
            "language": str(result.get("language") or language),
            "bytes": result.get("bytes"),
            "backend": "assistant_service_local_asr",
            "service": service_state,
            "asr": result.get("asr") if isinstance(result.get("asr"), dict) else {},
        },
    )


def handle_assistant_voice_transcript_append(
    args: Dict[str, Any],
    *,
    effective_runner_kind: Optional[Callable[[str], str]] = None,
    start_actor_process: Optional[Callable[..., dict[str, Any]]] = None,
) -> DaemonResponse:
    group_id = str(args.get("group_id") or "").strip()
    by = str(args.get("by") or "user").strip()
    session_id = _safe_voice_session_id(args.get("session_id"))
    segment_id = _safe_voice_session_id(args.get("segment_id") or f"seg-{uuid.uuid4().hex}")
    text = _clean_multiline_text(args.get("text"), max_len=_MAX_TRANSCRIPT_CHARS)
    language = str(args.get("language") or "").strip()
    is_final = coerce_bool(args.get("is_final"), default=True)
    flush = coerce_bool(args.get("flush"), default=False)
    raw_trigger = dict(args.get("trigger")) if isinstance(args.get("trigger"), dict) else {}
    raw_requested_document_path = str(args.get("document_path") or raw_trigger.get("document_path") or raw_trigger.get("workspace_path") or "").strip()
    if not group_id:
        return _error("missing_group_id", "missing group_id")
    if not text and not flush:
        return _error("empty_transcript_segment", "text cannot be empty unless flush=true")

    group = load_group(group_id)
    if group is None:
        return _error("group_not_found", f"group not found: {group_id}")
    try:
        _require_status_permission(group, assistant_id=ASSISTANT_ID_VOICE_SECRETARY, by=by)
    except Exception as exc:
        return _error("assistant_voice_transcript_append_failed", str(exc))
    assistant = _effective_assistant(group, ASSISTANT_ID_VOICE_SECRETARY)
    if not bool(assistant.get("enabled")):
        return _error("assistant_disabled", "voice_secretary is disabled")

    assistant_config = assistant.get("config") if isinstance(assistant.get("config"), dict) else {}
    effective_language = language or str(raw_trigger.get("language") or assistant_config.get("recognition_language") or "")
    intent_hint = _infer_voice_transcript_intent(text, raw_trigger)
    now = utc_now_iso()
    state = _load_runtime_state(group)
    sessions = state.setdefault("voice_sessions", {})
    session_entry = sessions.get(session_id) if isinstance(sessions.get(session_id), dict) else {}
    session_entry = dict(session_entry)
    session_document_id = str(session_entry.get("document_id") or "").strip()
    session_document_path = str(session_entry.get("document_path") or "").strip()
    target_document_id = session_document_id
    target_document_path = raw_requested_document_path or session_document_path
    if flush and not text and session_document_id:
        target_document_id = session_document_id
    segment_path = ""
    segment: Dict[str, Any] = {
        "schema": 1,
        "segment_id": segment_id,
        "session_id": session_id,
        "group_id": group.group_id,
        "assistant_id": ASSISTANT_ID_VOICE_SECRETARY,
        "created_at": now,
        "updated_at": now,
        "text": text,
        "language": effective_language,
        "document_id": target_document_id,
        "intent_hint": intent_hint,
        "is_final": is_final,
        "source": str(raw_trigger.get("recognition_backend") or raw_trigger.get("source") or ""),
        "by": by,
    }
    if text:
        segment_path = _append_voice_segment_jsonl(group, session_id=session_id, segment=segment)

    previous_window_text = _clean_multiline_text(session_entry.get("window_text"), max_len=_MAX_TRANSCRIPT_SESSION_CHARS)
    window_started_at = str(session_entry.get("window_started_at") or "")
    previous_window_segment_count = int(session_entry.get("window_segment_count") or 0)
    window_first_segment_id = str(session_entry.get("window_first_segment_id") or "")
    if text and not previous_window_text.strip():
        window_started_at = now
        window_first_segment_id = segment_id
    window_text = _append_voice_window_text(session_entry.get("window_text"), text if is_final else "")
    if not window_text.strip():
        window_started_at = ""
        window_first_segment_id = ""
    window_segment_count = previous_window_segment_count + (1 if text and is_final else 0)
    if not window_text.strip():
        window_segment_count = 0
    window_last_segment_id = segment_id if text and is_final else str(session_entry.get("window_last_segment_id") or session_entry.get("last_segment_id") or "")
    segment_count = int(session_entry.get("segment_count") or 0) + (1 if text else 0)
    session_entry.update(
        {
            "schema": 1,
            "session_id": session_id,
            "updated_at": now,
            "window_text": window_text,
            "window_started_at": window_started_at,
            "window_segment_count": window_segment_count,
            "window_first_segment_id": window_first_segment_id,
            "window_last_segment_id": window_last_segment_id,
            "language": effective_language or str(session_entry.get("language") or ""),
            "document_id": target_document_id,
            "document_path": target_document_path,
            "segment_count": segment_count,
            "last_segment_id": segment_id if text else str(session_entry.get("last_segment_id") or ""),
            "last_segment_path": segment_path or str(session_entry.get("last_segment_path") or ""),
            "input_device_label": str(raw_trigger.get("input_device_label") or session_entry.get("input_device_label") or ""),
        }
    )
    sessions[session_id] = session_entry
    assistants = state.setdefault("assistants", {})
    assistant_entry = assistants.get(ASSISTANT_ID_VOICE_SECRETARY) if isinstance(assistants.get(ASSISTANT_ID_VOICE_SECRETARY), dict) else {}
    assistant_entry = dict(assistant_entry)
    assistant_entry["lifecycle"] = "working" if text and not flush else "idle"
    assistant_entry["health"] = {
        "status": "listening" if text and not flush else "idle",
        "last_transcript_segment_id": segment_id if text else "",
        "last_transcript_session_id": session_id,
        "last_transcript_at": now if text else str(assistant_entry.get("updated_at") or now),
    }
    assistant_entry["updated_at"] = now
    assistants[ASSISTANT_ID_VOICE_SECRETARY] = assistant_entry
    _save_runtime_state(group, state)

    document: Dict[str, Any] | None = None
    document_updated = False
    input_event: Dict[str, Any] | None = None
    input_event_created = False
    input_notify_emitted = False
    actor_woken = False
    actor_wake_error = ""
    actor_notify_delivered = False
    actor_notify_delivery_error = ""
    document_window_consumed = False
    trigger_kind_for_window = str(raw_trigger.get("trigger_kind") or "").strip().lower()
    hard_flush_consumes_window = flush and trigger_kind_for_window in {
        "push_to_talk_stop",
        "service_transcript",
        "user_instruction",
        "manual_transcript",
        "document_switch",
    }
    filler_flush_consumes_window = flush and _is_voice_transcript_tiny_filler(window_text)
    document_due = _voice_document_input_due(
        config=assistant_config,
        window_text=window_text,
        intent_hint=_infer_voice_transcript_intent(window_text, raw_trigger),
        trigger=raw_trigger,
        flush=flush,
        window_started_at=window_started_at,
        window_segment_count=window_segment_count,
        now=now,
    )
    document_source_text = window_text.strip() if document_due else ""
    if document_source_text and is_final and coerce_bool(assistant_config.get("auto_document_enabled"), default=True):
        try:
            document_intent_hint = _infer_voice_transcript_intent(document_source_text, raw_trigger)
            trigger_for_document = {
                "mode": str(raw_trigger.get("mode") or "meeting"),
                "trigger_kind": str(raw_trigger.get("trigger_kind") or "meeting_window"),
                "capture_mode": str(raw_trigger.get("capture_mode") or assistant_config.get("capture_mode") or "browser"),
                "recognition_backend": str(raw_trigger.get("recognition_backend") or assistant_config.get("recognition_backend") or "browser_asr"),
                "client_session_id": session_id,
                "input_device_label": str(raw_trigger.get("input_device_label") or ""),
                "language": effective_language,
                "intent_hint": document_intent_hint,
                "window_segment_count": window_segment_count,
                "instruction_policy": _voice_instruction_policy(),
            }
            if target_document_path:
                document_index, document_record = _find_voice_document_by_path(
                    group,
                    document_path=target_document_path,
                    create=True,
                    config=assistant_config,
                )
            else:
                document_index, document_record = _get_voice_document(
                    group,
                    document_id=target_document_id,
                    create=True,
                    config=assistant_config,
                )
            if str(document_record.get("status") or "active").strip().lower() == "active":
                document_index["active_document_id"] = str(document_record.get("document_id") or target_document_id)
                _save_voice_documents_index(group, document_index)
            document = _voice_document_public_record(group, document_record)
            source_segment = dict(segment)
            source_segment["text"] = document_source_text
            source_segment["is_final"] = True
            source_segment["language"] = effective_language
            source_segment["intent_hint"] = document_intent_hint
            source_segment["source_segment_count"] = window_segment_count
            source_segment["first_source_segment_id"] = window_first_segment_id
            source_segment["last_source_segment_id"] = window_last_segment_id
            source_segment["source_segment_range"] = (
                f"{window_first_segment_id}..{window_last_segment_id}"
                if window_first_segment_id and window_last_segment_id and window_first_segment_id != window_last_segment_id
                else window_first_segment_id or window_last_segment_id
            )
            if flush and not text:
                source_segment["segment_id"] = str(session_entry.get("last_segment_id") or "")
            elif not str(source_segment.get("segment_id") or "").strip():
                source_segment["segment_id"] = str(session_entry.get("last_segment_id") or "")
            _append_voice_document_source(
                group,
                record=document,
                segment=source_segment,
                segment_path=str(session_entry.get("last_segment_path") or segment_path or ""),
            )
            input_event = _append_voice_input_event(
                group,
                kind="asr_transcript",
                text=document_source_text,
                document=document,
                language=effective_language,
                intent_hint=document_intent_hint,
                source=str(trigger_for_document.get("recognition_backend") or "browser_asr"),
                session_id=session_id,
                segment_id=str(source_segment.get("segment_id") or ""),
                by=by,
                trigger=trigger_for_document,
                metadata={
                    "source_segment_count": window_segment_count,
                    "source_segment_range": str(source_segment.get("source_segment_range") or ""),
                    "capture_continuity": _voice_capture_continuity(segment_count=window_segment_count, trigger=trigger_for_document),
                    "suggested_document_mode": _voice_suggested_document_mode(
                        document_source_text,
                        trigger=trigger_for_document,
                        intent_hint=document_intent_hint,
                        segment_count=window_segment_count,
                    ),
                },
            )
            input_event_created = True
            input_notify_emitted = True
            document_event = append_event(
                group.ledger_path,
                kind="assistant.voice.document",
                group_id=group.group_id,
                scope_key="",
                by=_assistant_principal(ASSISTANT_ID_VOICE_SECRETARY),
                data={
                    "assistant_id": ASSISTANT_ID_VOICE_SECRETARY,
                    "document_path": str(document.get("document_path") or document.get("workspace_path") or ""),
                    "action": "input_appended",
                    "input_kind": "asr_transcript",
                    "status": str(document.get("status") or "active"),
                    "workspace_path": str(document.get("workspace_path") or ""),
                    "title": str(document.get("title") or ""),
                    "input_preview": document_source_text[:500],
                },
            )
            _ = document_event
            actor_woken, actor_wake_error = _try_wake_voice_secretary_actor_after_input(
                group,
                by=by,
                args=args,
                effective_runner_kind=effective_runner_kind,
                start_actor_process=start_actor_process,
            )
            actor_notify_delivered, actor_notify_delivery_error = _try_deliver_voice_input_notify_after_wake(
                group,
                actor_woken=actor_woken,
            )
            document_window_consumed = True
        except Exception as exc:
            return _error("assistant_voice_document_update_failed", str(exc))

    if (document_window_consumed or hard_flush_consumes_window or filler_flush_consumes_window) and window_text.strip():
        last_document_id = str((document or {}).get("document_id") or target_document_id or "").strip()
        _clear_voice_session_window(group, session_id=session_id, now=utc_now_iso(), last_document_id=last_document_id)
    assistant = _effective_assistant(group, ASSISTANT_ID_VOICE_SECRETARY)

    if (
        flush
        and trigger_kind_for_window in _VOICE_IDLE_REVIEW_STOP_TRIGGER_KINDS
        and not input_event_created
    ):
        _maybe_request_voice_idle_review_for_stop_flush(group)

    return DaemonResponse(
        ok=True,
        result={
            "group_id": group.group_id,
            "assistant": assistant,
            "session_id": session_id,
            "segment": segment if text else {},
            "segment_path": segment_path,
            "document": document,
            "document_updated": document_updated,
            "input_event": input_event or {},
            "input_event_created": input_event_created,
            "input_notify_emitted": input_notify_emitted,
            "actor_woken": actor_woken,
            "actor_wake_error": actor_wake_error,
            "actor_notify_delivered": actor_notify_delivered,
            "actor_notify_delivery_error": actor_notify_delivery_error,
        },
    )


def handle_assistant_voice_document_list(args: Dict[str, Any]) -> DaemonResponse:
    group_id = str(args.get("group_id") or "").strip()
    include_archived = coerce_bool(args.get("include_archived"), default=False)
    include_content = coerce_bool(args.get("include_content"), default=True)
    include_documents_by_id = coerce_bool(args.get("include_documents_by_id"), default=True)
    include_documents_by_path = coerce_bool(args.get("include_documents_by_path"), default=True)
    requested_document_path = str(args.get("document_path") or args.get("workspace_path") or "").strip()
    if not group_id:
        return _error("missing_group_id", "missing group_id")
    group = load_group(group_id)
    if group is None:
        return _error("group_not_found", f"group not found: {group_id}")
    _flush_stale_voice_session_windows(group)
    _maybe_emit_voice_input_retry_notify(group)
    documents = _retained_voice_documents(group, include_archived=include_archived, include_content=include_content)
    if requested_document_path:
        try:
            wanted = _safe_voice_document_rel_path(requested_document_path)
        except Exception:
            wanted = requested_document_path
        documents = [item for item in documents if str(item.get("document_path") or "").strip() == wanted]
    index = _load_voice_documents_index(group)
    active_document_id, active_record = _active_voice_document_from_index(group, index)
    active_document_path = _voice_document_path(active_record) if isinstance(active_record, dict) else ""
    input_state = _load_voice_input_state(group)
    result = {
        "group_id": group.group_id,
        "documents": documents,
        "active_document_id": active_document_id,
        "capture_target_document_id": active_document_id,
        "active_document_path": active_document_path,
        "capture_target_document_path": active_document_path,
        "new_input_available": int(input_state.get("latest_seq") or 0) > _voice_input_delivery_cursor(input_state),
        "input_timing": _voice_input_timing_public(input_state),
    }
    if include_documents_by_id:
        result["documents_by_id"] = {str(item.get("document_id")): item for item in documents}
    if include_documents_by_path:
        result["documents_by_path"] = {str(item.get("document_path")): item for item in documents if str(item.get("document_path") or "").strip()}
    return DaemonResponse(ok=True, result=result)


def handle_assistant_voice_document_select(args: Dict[str, Any]) -> DaemonResponse:
    group_id = str(args.get("group_id") or "").strip()
    by = str(args.get("by") or "user").strip()
    document_path = str(args.get("document_path") or args.get("workspace_path") or "").strip()
    if not group_id:
        return _error("missing_group_id", "missing group_id")
    if not document_path:
        return _error("missing_document_path", "missing document_path")
    group = load_group(group_id)
    if group is None:
        return _error("group_not_found", f"group not found: {group_id}")
    try:
        _require_document_write_permission(group, by=by)
        index, record = _find_voice_document_by_path(group, document_path=document_path, create=False)
        if str(record.get("status") or "active").strip().lower() != "active":
            return _error("assistant_voice_document_select_failed", "voice secretary document is archived")
        document_id = str(record.get("document_id") or "").strip()
        index["active_document_id"] = document_id
        _save_voice_documents_index(group, index)
        document = _voice_document_public_record(group, record)
        try:
            _append_voice_input_event(
                group,
                kind="target_document_changed",
                text=f"Default capture document changed to {document.get('document_path') or document.get('workspace_path') or document.get('title')}.",
                document=document,
                source="secretary_panel",
                by=by,
                metadata={},
            )
        except Exception:
            pass
        return DaemonResponse(
            ok=True,
            result={
                "group_id": group.group_id,
                "document": document,
                "active_document_id": document_id,
                "active_document_path": str(document.get("document_path") or ""),
            },
        )
    except Exception as exc:
        return _error("assistant_voice_document_select_failed", str(exc))


def handle_assistant_voice_document_input_read(args: Dict[str, Any]) -> DaemonResponse:
    group_id = str(args.get("group_id") or "").strip()
    by = str(args.get("by") or "assistant:voice_secretary").strip()
    if not group_id:
        return _error("missing_group_id", "missing group_id")
    group = load_group(group_id)
    if group is None:
        return _error("group_not_found", f"group not found: {group_id}")
    if by not in {VOICE_SECRETARY_ACTOR_ID, _assistant_principal(ASSISTANT_ID_VOICE_SECRETARY), "assistant:voice_secretary"}:
        return _error("assistant_voice_document_input_read_failed", "read_new_input is only available to voice-secretary")
    try:
        _flush_stale_voice_session_windows(group)
        read_at = utc_now_iso()
        state = _load_voice_input_state(group)
        cursor = int(state.get("secretary_read_cursor") or 0)
        items = _read_voice_input_events(group, after_seq=cursor)
        state["last_read_new_input_at"] = read_at
        if items:
            state["secretary_read_cursor"] = max(int(item.get("seq") or 0) for item in items)
            state["secretary_delivery_cursor"] = max(
                int(state.get("secretary_delivery_cursor") or 0),
                int(state.get("secretary_read_cursor") or 0),
            )
            if int(state.get("secretary_read_cursor") or 0) >= int(state.get("latest_seq") or 0):
                state["last_notify_at"] = ""
                state["retry_count"] = 0
        _save_voice_input_state(group, state)
        public_items = [_voice_input_event_public(item) for item in items]
        grouped = _group_voice_input_by_target(items)
        grouped = _annotate_voice_input_previous_tails(group, grouped, items)
        ask_request_ids: list[str] = []
        for group_item in grouped:
            if str(group_item.get("target_kind") or "").strip() == "composer":
                continue
            for request_id in (group_item.get("request_ids") if isinstance(group_item.get("request_ids"), list) else []):
                request_id_text = str(request_id or "").strip()
                if request_id_text and request_id_text not in ask_request_ids:
                    ask_request_ids.append(request_id_text)
        _mark_voice_ask_requests_working(group, request_ids=ask_request_ids, now=utc_now_iso())
        runtime_state = _load_runtime_state(group)
        _stale_expired_voice_prompt_drafts_in_state(runtime_state, now=read_at)
        _save_runtime_state(group, runtime_state)
        input_text = _voice_input_batch_text(grouped, item_count=len(public_items))
        if not public_items:
            input_text = (
                "No new Secretary input. New voice_secretary_input notifications carry the "
                "daemon-delivered input_envelope inline; work from that notification body. "
                "read_new_input remains a fallback for legacy pointer notifications and manual recovery."
            )
        input_batches = [_voice_input_batch_public(item) for item in grouped]
        referenced_paths = {str(item.get("document_path") or "").strip() for item in grouped if str(item.get("document_path") or "").strip()}
        documents = [
            item
            for item in _retained_voice_documents(group, include_content=False)
            if str(item.get("document_path") or "").strip() in referenced_paths
        ] if referenced_paths else []
        return DaemonResponse(
            ok=True,
            result={
                "group_id": group.group_id,
                "input_text": input_text,
                "item_count": len(public_items),
                "document_count": len(input_batches),
                "delivery_mode": "legacy_read_new_input" if public_items else "daemon_envelope_primary",
                "input_batches": input_batches,
                "documents": documents,
                "has_new_input": bool(public_items),
                "input_timing": _voice_input_timing_public(state),
            },
        )
    except Exception as exc:
        return _error("assistant_voice_document_input_read_failed", str(exc))


def handle_assistant_voice_document_save(args: Dict[str, Any]) -> DaemonResponse:
    group_id = str(args.get("group_id") or "").strip()
    by = str(args.get("by") or "user").strip()
    document_path = str(args.get("document_path") or args.get("workspace_path") or "").strip()
    title = " ".join(str(args.get("title") or "").strip().split())
    content_provided = any(key in args for key in ("content", "new_source", "markdown"))
    raw_content = args.get("content") if "content" in args else args.get("new_source") if "new_source" in args else args.get("markdown")
    content = str(raw_content or "")
    status = str(args.get("status") or "").strip().lower()
    create_new = coerce_bool(args.get("create_new"), default=False)
    if not group_id:
        return _error("missing_group_id", "missing group_id")
    group = load_group(group_id)
    if group is None:
        return _error("group_not_found", f"group not found: {group_id}")
    try:
        _require_document_write_permission(group, by=by)
        assistant = _effective_assistant(group, ASSISTANT_ID_VOICE_SECRETARY)
        config = assistant.get("config") if isinstance(assistant.get("config"), dict) else {}
        if create_new:
            index, record = _create_new_voice_document(group, title=title, config=config)
        else:
            if not content_provided:
                return _error("assistant_voice_document_save_failed", "content is required for document save")
            if not document_path:
                return _error("missing_document_path", "missing document_path")
            index, record = _find_voice_document_by_path(group, document_path=document_path, create=True, config=config)
        existing_status = str(record.get("status") or "active").strip().lower() or "active"
        if existing_status != "active" and status != "active":
            return _error(
                "assistant_voice_document_save_failed",
                "voice secretary document is archived; create or select an active document",
            )
        if create_new and title:
            record["title"] = title[:160]
        if status in {"active", "archived"}:
            record["status"] = status
        elif status:
            return _error("invalid_document_status", "status must be active or archived")
        if str(record.get("status") or "active").strip().lower() == "archived":
            record = _archive_voice_document_storage(group, record)
            if str(index.get("active_document_id") or "").strip() == str(record.get("document_id") or "").strip():
                index["active_document_id"] = _select_next_active_voice_document_id(
                    index,
                    exclude_document_id=str(record.get("document_id") or "").strip(),
                )
        if content_provided:
            updated = _write_voice_document_content(
                group,
                index,
                record,
                content=content,
                reason="voice_input" if by == _assistant_principal(ASSISTANT_ID_VOICE_SECRETARY) else "manual_save",
                by=by,
            )
            if by == _assistant_principal(ASSISTANT_ID_VOICE_SECRETARY):
                updated = dict(updated)
                index_after_write = _load_voice_documents_index(group)
                _save_voice_document_record(group, index_after_write, updated)
        else:
            _save_voice_document_record(group, index, record)
            updated = record
        event = append_event(
            group.ledger_path,
            kind="assistant.voice.document",
            group_id=group.group_id,
            scope_key="",
            by=by,
            data={
                "assistant_id": ASSISTANT_ID_VOICE_SECRETARY,
                "document_path": _voice_document_path(updated),
                "action": "create" if create_new else "save",
                "status": str(updated.get("status") or "active"),
                "workspace_path": str(updated.get("workspace_path") or ""),
                "title": str(updated.get("title") or ""),
            },
        )
        return DaemonResponse(
            ok=True,
            result={
                "group_id": group.group_id,
                "document": _voice_document_public_record(group, updated),
                "event": event,
            },
        )
    except Exception as exc:
        return _error("assistant_voice_document_save_failed", str(exc))


def _wake_voice_secretary_actor_if_needed(
    group: Group,
    *,
    by: str,
    args: Dict[str, Any],
    effective_runner_kind: Optional[Callable[[str], str]],
    start_actor_process: Optional[Callable[..., dict[str, Any]]],
) -> bool:
    if effective_runner_kind is None or start_actor_process is None:
        return False
    actor = get_voice_secretary_actor(group)
    if not isinstance(actor, dict):
        return False
    if is_voice_secretary_actor_running(group, actor=actor, effective_runner_kind=effective_runner_kind):
        return False
    start_result = start_actor_process(
        group,
        VOICE_SECRETARY_ACTOR_ID,
        command=list(actor.get("command") or []),
        env=dict(actor.get("env") or {}),
        runner=str(actor.get("runner") or "pty"),
        runtime=str(actor.get("runtime") or "codex"),
        by=by,
        caller_id=str(args.get("caller_id") or "").strip(),
        is_admin=coerce_bool(args.get("is_admin"), default=False),
    )
    if not bool(start_result.get("success")):
        start_error = str(start_result.get("error") or "").strip()
        raise RuntimeError(f"failed to start voice secretary actor: {start_error or 'unknown error'}")
    return True


def _try_wake_voice_secretary_actor_after_input(
    group: Group,
    *,
    by: str,
    args: Dict[str, Any],
    effective_runner_kind: Optional[Callable[[str], str]],
    start_actor_process: Optional[Callable[..., dict[str, Any]]],
) -> tuple[bool, str]:
    try:
        woken = _wake_voice_secretary_actor_if_needed(
            group,
            by=by,
            args=args,
            effective_runner_kind=effective_runner_kind,
            start_actor_process=start_actor_process,
        )
        return bool(woken), ""
    except Exception as exc:
        message = str(exc).strip() or exc.__class__.__name__
        logger.warning("voice secretary actor wake failed after input append: group=%s error=%s", group.group_id, message)
        return False, message


def _latest_voice_input_notify_event(group: Group) -> Dict[str, Any]:
    state = _load_voice_input_state(group)
    wanted_delivery_id = str(state.get("last_input_envelope_id") or "").strip()
    fallback: Dict[str, Any] = {}
    for event in iter_events_reverse(group.ledger_path):
        if str(event.get("kind") or "").strip() != "system.notify":
            continue
        data = event.get("data") if isinstance(event.get("data"), dict) else {}
        context = data.get("context") if isinstance(data.get("context"), dict) else {}
        if str(context.get("kind") or "").strip() != "voice_secretary_input":
            continue
        if not fallback:
            fallback = event
        envelope = context.get("input_envelope") if isinstance(context.get("input_envelope"), dict) else {}
        delivery_id = str(envelope.get("delivery_id") or "").strip()
        if wanted_delivery_id and delivery_id == wanted_delivery_id:
            return event
        if not wanted_delivery_id:
            return event
    return fallback


def _try_emit_voice_input_notify_after_input(group: Group, *, reason: str) -> tuple[bool, str, Dict[str, Any]]:
    try:
        event = _emit_voice_input_notify(group, reason=reason)
        return True, "", event if isinstance(event, dict) else {}
    except Exception as exc:
        message = str(exc).strip() or exc.__class__.__name__
        logger.warning("voice secretary input notify failed after durable append: group=%s error=%s", group.group_id, message)
        return False, message, {}


def _try_deliver_voice_input_notify_after_wake(
    group: Group,
    *,
    actor_woken: bool,
    notify_event: Optional[Dict[str, Any]] = None,
    allow_fallback_latest: bool = True,
) -> tuple[bool, str]:
    if not actor_woken:
        return False, ""
    event = notify_event if isinstance(notify_event, dict) and notify_event.get("id") else {}
    if not event and allow_fallback_latest:
        event = _latest_voice_input_notify_event(group)
    if not event:
        return False, "missing_voice_input_notify_event"
    try:
        delivered = dispatch_system_notify_event_to_actor(
            group,
            event=event,
            actor_id=VOICE_SECRETARY_ACTOR_ID,
            async_flush=True,
        )
        return bool(delivered), "" if delivered else "voice_input_notify_not_delivered"
    except Exception as exc:
        message = str(exc).strip() or exc.__class__.__name__
        logger.warning(
            "voice secretary input notify delivery failed after actor wake: group=%s event=%s error=%s",
            group.group_id,
            event.get("id"),
            message,
        )
        return False, message


def handle_assistant_voice_input_append(
    args: Dict[str, Any],
    *,
    effective_runner_kind: Optional[Callable[[str], str]] = None,
    start_actor_process: Optional[Callable[..., dict[str, Any]]] = None,
) -> DaemonResponse:
    group_id = str(args.get("group_id") or "").strip()
    by = str(args.get("by") or "user").strip()
    input_kind = str(args.get("kind") or args.get("input_kind") or "").strip().lower()
    if not group_id:
        return _error("missing_group_id", "missing group_id")
    if input_kind not in {"voice_instruction", "prompt_refine"}:
        return _error("invalid_voice_input_kind", "kind must be voice_instruction or prompt_refine")
    group = load_group(group_id)
    if group is None:
        return _error("group_not_found", f"group not found: {group_id}")
    try:
        _require_confirmation_permission(group, by=by)
    except Exception as exc:
        return _error("assistant_voice_input_append_failed", str(exc))
    assistant = _effective_assistant(group, ASSISTANT_ID_VOICE_SECRETARY)
    if not bool(assistant.get("enabled")):
        return _error("assistant_disabled", "voice_secretary is disabled")

    raw_trigger = dict(args.get("trigger")) if isinstance(args.get("trigger"), dict) else {}
    language = str(args.get("language") or raw_trigger.get("language") or (assistant.get("config") or {}).get("recognition_language") or "").strip()
    document: Dict[str, Any] = {}
    document_path = str(args.get("document_path") or args.get("workspace_path") or raw_trigger.get("document_path") or "").strip()
    if input_kind == "voice_instruction" and document_path:
        try:
            _, record = _find_voice_document_by_path(group, document_path=document_path, create=False)
            if str(record.get("status") or "active").strip().lower() != "active":
                return _error(
                    "assistant_voice_input_append_failed",
                    "voice secretary document is archived; choose an active document or send a general secretary instruction",
                )
            document = _voice_document_public_record(group, record)
        except Exception as exc:
            return _error("assistant_voice_input_append_failed", str(exc))

    request_id = ""
    metadata: Dict[str, Any] = {}
    event_kind = input_kind
    source = "secretary_panel"
    session_id = "voice-secretary-user-instruction"
    segment_id = f"instruction-{uuid.uuid4().hex}"
    if input_kind == "prompt_refine":
        request_id = _clean_voice_prompt_request_id(args.get("request_id"))
        voice_transcript = _clean_multiline_text(args.get("voice_transcript") or args.get("text"), max_len=8_000)
        composer_text = _clean_multiline_text(args.get("composer_text"), max_len=8_000)
        if not voice_transcript and not composer_text:
            return _error("empty_prompt_refine_input", "voice_transcript or composer_text is required")
        operation = str(args.get("operation") or "append_to_composer_end").strip() or "append_to_composer_end"
        composer_context = args.get("composer_context") if isinstance(args.get("composer_context"), dict) else {}
        composer_snapshot_hash = str(args.get("composer_snapshot_hash") or "").strip()
        now = utc_now_iso()
        runtime_state = _load_runtime_state(group)
        prompt_request = _merge_voice_prompt_request(
            runtime_state,
            group=group,
            request_id=request_id,
            composer_text=composer_text,
            voice_transcript=voice_transcript,
            operation=operation,
            composer_context=composer_context,
            composer_snapshot_hash=composer_snapshot_hash,
            now=now,
        )
        prompt_draft_staled = _stale_pending_voice_prompt_draft_in_state(runtime_state, request_id=request_id, now=now)
        _save_runtime_state(group, runtime_state)
        merged_voice_transcript = "\n\n".join(
            str(item).strip()
            for item in (prompt_request.get("voice_transcripts") if isinstance(prompt_request.get("voice_transcripts"), list) else [])
            if str(item).strip()
        )
        text = _clean_multiline_text(
            build_voice_prompt_refine_input_text(
                composer_text=str(prompt_request.get("composer_text") or composer_text),
                voice_transcript=merged_voice_transcript or voice_transcript,
                operation=operation,
                composer_context=prompt_request.get("composer_context") if isinstance(prompt_request.get("composer_context"), dict) else composer_context,
            ),
            max_len=_MAX_PROMPT_REFINE_CHARS,
        )
        raw_trigger.setdefault("trigger_kind", "prompt_refine")
        raw_trigger.setdefault("mode", "prompt")
        raw_trigger.setdefault("intent_hint", "prompt_refine")
        metadata = {
            "target_kind": "composer",
            "request_id": request_id,
            "operation": operation,
            "composer_snapshot_hash": composer_snapshot_hash,
            "prompt_request_append_count": len(prompt_request.get("voice_transcripts") or []),
            "prompt_draft_staled": prompt_draft_staled,
        }
        source = "composer_prompt_refine"
        session_id = "voice-secretary-prompt-refine"
        segment_id = request_id
        intent_hint = "prompt_refine"
    else:
        request_id = _clean_voice_ask_request_id(args.get("request_id"))
        instruction = _clean_multiline_text(args.get("instruction") or args.get("text"), max_len=8_000)
        source_text = _clean_multiline_text(args.get("source_text"), max_len=_MAX_TRANSCRIPT_CHARS)
        if not instruction and not source_text:
            return _error("empty_voice_instruction", "instruction or source_text is required")
        intent_hint = _infer_voice_transcript_intent(instruction or source_text, raw_trigger)
        raw_trigger.setdefault("trigger_kind", "voice_instruction")
        raw_trigger.setdefault("mode", "voice_instruction")
        raw_trigger.setdefault("intent_hint", intent_hint)
        parts = []
        if instruction:
            parts.extend(["Task:", instruction])
        if source_text:
            if instruction:
                parts.extend(["", "Context (not task):", "Additional source:", source_text])
            else:
                parts.extend(["Task:", "Handle the provided voice input as a secretary Ask request.", "", "Inputs:", source_text])
        text = "\n\n".join(parts).strip()
        metadata = {
            "target_kind": "document" if document else "secretary",
            "request_id": request_id,
        }

    raw_trigger.setdefault("recognition_backend", str((assistant.get("config") or {}).get("recognition_backend") or "browser_asr"))
    raw_trigger.setdefault("language", language)
    raw_trigger.setdefault("instruction_policy", _voice_instruction_policy())
    try:
        input_event = _append_voice_input_event(
            group,
            kind=event_kind,
            text=text,
            document=document,
            language=language,
            intent_hint=intent_hint,
            source=source,
            session_id=session_id,
            segment_id=segment_id,
            by=by,
            trigger=raw_trigger,
            metadata=metadata,
            emit_notify=False,
        )
        if input_kind == "voice_instruction":
            runtime_state = _load_runtime_state(group)
            request_now = utc_now_iso()
            _upsert_voice_ask_request(
                runtime_state,
                group=group,
                request_id=request_id,
                status="pending",
                request_text=text,
                document_path=str(document.get("document_path") or document.get("workspace_path") or ""),
                target_kind=str(metadata.get("target_kind") or "secretary"),
                intent_hint=intent_hint,
                language=language,
                input_appended_at=request_now,
                now=request_now,
            )
            _save_runtime_state(group, runtime_state)
        event = append_event(
            group.ledger_path,
            kind="assistant.voice.input",
            group_id=group.group_id,
            scope_key="",
            by=by,
            data={
                "assistant_id": ASSISTANT_ID_VOICE_SECRETARY,
                "input_kind": event_kind,
                "target_kind": str(metadata.get("target_kind") or ""),
                "request_id": request_id,
                "document_path": str(document.get("document_path") or document.get("workspace_path") or ""),
                "input_preview": _clean_multiline_text(text, max_len=240),
            },
        )
        assistant_after = _set_voice_assistant_runtime(
            group,
            lifecycle="working",
            health={
                "status": "prompt_refine_requested" if input_kind == "prompt_refine" else "instruction_requested",
                "last_input_kind": event_kind,
                "last_prompt_request_id": request_id if input_kind == "prompt_refine" else "",
                "last_ask_request_id": request_id if input_kind == "voice_instruction" else "",
                "last_document_path": str(document.get("document_path") or document.get("workspace_path") or ""),
                "last_input_at": utc_now_iso(),
            },
        )
        input_notify_emitted, input_notify_error, input_notify_event = _try_emit_voice_input_notify_after_input(group, reason="new_input")
        actor_woken, actor_wake_error = _try_wake_voice_secretary_actor_after_input(
            group,
            by=by,
            args=args,
            effective_runner_kind=effective_runner_kind,
            start_actor_process=start_actor_process,
        )
        actor_notify_delivered, actor_notify_delivery_error = _try_deliver_voice_input_notify_after_wake(
            group,
            actor_woken=actor_woken,
            notify_event=input_notify_event,
            allow_fallback_latest=False,
        )
        return DaemonResponse(
            ok=True,
            result={
                "group_id": group.group_id,
                "assistant": assistant_after,
                "document": document,
                "input_event": input_event,
                "input_event_created": True,
                "input_notify_emitted": input_notify_emitted,
                "input_notify_error": input_notify_error,
                "actor_woken": actor_woken,
                "actor_wake_error": actor_wake_error,
                "actor_notify_delivered": actor_notify_delivered,
                "actor_notify_delivery_error": actor_notify_delivery_error,
                "event": event,
                "request_id": request_id,
            },
        )
    except Exception as exc:
        return _error("assistant_voice_input_append_failed", str(exc))


def handle_assistant_voice_document_instruction(
    args: Dict[str, Any],
    *,
    effective_runner_kind: Optional[Callable[[str], str]] = None,
    start_actor_process: Optional[Callable[..., dict[str, Any]]] = None,
) -> DaemonResponse:
    group_id = str(args.get("group_id") or "").strip()
    by = str(args.get("by") or "user").strip()
    document_path = str(args.get("document_path") or args.get("workspace_path") or "").strip()
    instruction = _clean_multiline_text(args.get("instruction"), max_len=8_000)
    source_text = _clean_multiline_text(args.get("source_text"), max_len=_MAX_TRANSCRIPT_CHARS)
    raw_trigger = dict(args.get("trigger")) if isinstance(args.get("trigger"), dict) else {}
    if not group_id:
        return _error("missing_group_id", "missing group_id")
    if not instruction and not source_text:
        return _error("empty_document_instruction", "instruction or source_text is required")
    group = load_group(group_id)
    if group is None:
        return _error("group_not_found", f"group not found: {group_id}")
    try:
        _require_confirmation_permission(group, by=by)
    except Exception as exc:
        return _error("assistant_voice_document_instruction_failed", str(exc))
    assistant = _effective_assistant(group, ASSISTANT_ID_VOICE_SECRETARY)
    if not bool(assistant.get("enabled")):
        return _error("assistant_disabled", "voice_secretary is disabled")

    raw_trigger.setdefault("intent_hint", _infer_voice_transcript_intent(instruction or source_text, raw_trigger))
    raw_trigger.setdefault("instruction_policy", _voice_instruction_policy())
    try:
        if not document_path:
            return _error("missing_document_path", "missing document_path")
        document_index, record = _find_voice_document_by_path(group, document_path=document_path, create=False)
        if str(record.get("status") or "active").strip().lower() != "active":
            return _error(
                "assistant_voice_document_instruction_failed",
                "voice secretary document is archived; create or select an active document",
            )
        document = _voice_document_public_record(group, record)
        intent_hint = _infer_voice_transcript_intent(instruction or source_text, raw_trigger)
        request_id = _clean_voice_ask_request_id(args.get("request_id"))
        raw_trigger.setdefault("trigger_kind", "user_instruction")
        raw_trigger.setdefault("mode", "meeting")
        raw_trigger.setdefault("recognition_backend", str((assistant.get("config") or {}).get("recognition_backend") or "browser_asr"))
        raw_trigger.setdefault("intent_hint", intent_hint)
        job_source_parts = []
        if instruction:
            job_source_parts.extend(["Task:", instruction])
        elif source_text:
            job_source_parts.extend(["Task:", "Update the target document using the provided source material."])
        if source_text:
            job_source_parts.extend(["", "Inputs:", "Additional source:", source_text])
        job_source_text = "\n\n".join(job_source_parts).strip()
        source_segment = {
            "schema": 1,
            "segment_id": f"instruction-{uuid.uuid4().hex}",
            "session_id": "voice-secretary-user-instruction",
            "group_id": group.group_id,
            "assistant_id": ASSISTANT_ID_VOICE_SECRETARY,
            "created_at": utc_now_iso(),
            "updated_at": utc_now_iso(),
            "text": job_source_text,
            "language": str(raw_trigger.get("language") or ""),
            "intent_hint": intent_hint,
            "is_final": True,
            "source": "user_instruction",
            "by": by,
        }
        input_event = _append_voice_input_event(
            group,
            kind="user_instruction",
            text=job_source_text,
            document=document,
            language=str(raw_trigger.get("language") or ""),
            intent_hint=intent_hint,
            source="secretary_panel",
            session_id="voice-secretary-user-instruction",
            segment_id=str(source_segment.get("segment_id") or ""),
            by=by,
            trigger=raw_trigger,
            metadata={"target_kind": "document", "request_id": request_id},
            emit_notify=False,
        )
        runtime_state = _load_runtime_state(group)
        request_now = utc_now_iso()
        _upsert_voice_ask_request(
            runtime_state,
            group=group,
            request_id=request_id,
            status="pending",
            request_text=job_source_text,
            document_path=str(document.get("document_path") or document.get("workspace_path") or ""),
            target_kind="document",
            intent_hint=intent_hint,
            language=str(raw_trigger.get("language") or ""),
            input_appended_at=request_now,
            now=request_now,
        )
        _save_runtime_state(group, runtime_state)
        event = append_event(
            group.ledger_path,
            kind="assistant.voice.document",
            group_id=group.group_id,
            scope_key="",
            by=by,
            data={
                "assistant_id": ASSISTANT_ID_VOICE_SECRETARY,
                "document_path": str(document.get("document_path") or document.get("workspace_path") or ""),
                "action": "input_appended",
                "input_kind": "user_instruction",
                "request_id": request_id,
                "status": str(document.get("status") or "active"),
                "workspace_path": str(document.get("workspace_path") or ""),
                "title": str(document.get("title") or ""),
            },
        )
        assistant_after = _set_voice_assistant_runtime(
            group,
            lifecycle="working",
            health={
                "status": "document_refine_requested",
                "last_document_path": str(document.get("document_path") or document.get("workspace_path") or ""),
                "last_ask_request_id": request_id,
                "last_document_instruction_at": utc_now_iso(),
            },
        )
        input_notify_emitted, input_notify_error, input_notify_event = _try_emit_voice_input_notify_after_input(group, reason="new_input")
        actor_woken, actor_wake_error = _try_wake_voice_secretary_actor_after_input(
            group,
            by=by,
            args=args,
            effective_runner_kind=effective_runner_kind,
            start_actor_process=start_actor_process,
        )
        actor_notify_delivered, actor_notify_delivery_error = _try_deliver_voice_input_notify_after_wake(
            group,
            actor_woken=actor_woken,
            notify_event=input_notify_event,
            allow_fallback_latest=False,
        )
        return DaemonResponse(
            ok=True,
            result={
                "group_id": group.group_id,
                "document": document,
                "assistant": assistant_after,
                "input_event": input_event,
                "input_event_created": True,
                "input_notify_emitted": input_notify_emitted,
                "input_notify_error": input_notify_error,
                "actor_woken": actor_woken,
                "actor_wake_error": actor_wake_error,
                "actor_notify_delivered": actor_notify_delivered,
                "actor_notify_delivery_error": actor_notify_delivery_error,
                "event": event,
                "request_id": request_id,
            },
        )
    except Exception as exc:
        return _error("assistant_voice_document_instruction_failed", str(exc))


def handle_assistant_voice_document_archive(args: Dict[str, Any]) -> DaemonResponse:
    group_id = str(args.get("group_id") or "").strip()
    by = str(args.get("by") or "user").strip()
    document_path = str(args.get("document_path") or args.get("workspace_path") or "").strip()
    if not group_id:
        return _error("missing_group_id", "missing group_id")
    if not document_path:
        return _error("missing_document_path", "missing document_path")
    group = load_group(group_id)
    if group is None:
        return _error("group_not_found", f"group not found: {group_id}")
    try:
        _require_document_write_permission(group, by=by)
        index, record = _find_voice_document_by_path(group, document_path=document_path, create=False)
        record = _archive_voice_document_storage(group, record)
        record["status"] = "archived"
        record["updated_at"] = utc_now_iso()
        if str(index.get("active_document_id") or "").strip() == str(record.get("document_id") or "").strip():
            index["active_document_id"] = _select_next_active_voice_document_id(
                index,
                exclude_document_id=str(record.get("document_id") or "").strip(),
            )
        _save_voice_document_record(group, index, record)
        event = append_event(
            group.ledger_path,
            kind="assistant.voice.document",
            group_id=group.group_id,
            scope_key="",
            by=by,
            data={
                "assistant_id": ASSISTANT_ID_VOICE_SECRETARY,
                "document_path": _voice_document_path(record),
                "action": "archive",
                "status": "archived",
                "workspace_path": str(record.get("workspace_path") or ""),
                "title": str(record.get("title") or ""),
            },
        )
        return DaemonResponse(ok=True, result={"group_id": group.group_id, "document": _voice_document_public_record(group, record), "event": event})
    except Exception as exc:
        return _error("assistant_voice_document_archive_failed", str(exc))


def handle_assistant_voice_prompt_draft_submit(args: Dict[str, Any]) -> DaemonResponse:
    group_id = str(args.get("group_id") or "").strip()
    by = str(args.get("by") or VOICE_SECRETARY_ACTOR_ID).strip()
    raw_request_id = str(args.get("request_id") or "").strip()
    draft_text = _clean_multiline_text(args.get("draft_text"), max_len=_MAX_PROMPT_DRAFT_CHARS)
    summary = _clean_multiline_text(args.get("summary"), max_len=800)
    raw_operation = str(args.get("operation") or "").strip()
    composer_snapshot_hash = str(args.get("composer_snapshot_hash") or "").strip()
    if not group_id:
        return _error("missing_group_id", "missing group_id")
    if by not in {VOICE_SECRETARY_ACTOR_ID, _assistant_principal(ASSISTANT_ID_VOICE_SECRETARY), "assistant:voice_secretary"}:
        return _error("assistant_voice_prompt_draft_forbidden", "prompt drafts can only be submitted by voice-secretary")
    if not raw_request_id:
        return _error("missing_prompt_request_id", "request_id is required for voice prompt drafts")
    request_id = _clean_voice_prompt_request_id(raw_request_id)
    if not draft_text:
        return _error("empty_voice_prompt_draft", "draft_text is required")
    group = load_group(group_id)
    if group is None:
        return _error("group_not_found", f"group not found: {group_id}")
    assistant = _effective_assistant(group, ASSISTANT_ID_VOICE_SECRETARY)
    if not bool(assistant.get("enabled")):
        return _error("assistant_disabled", "voice_secretary is disabled")
    try:
        now = utc_now_iso()
        state = _load_runtime_state(group)
        requests = state.get("voice_prompt_requests") if isinstance(state.get("voice_prompt_requests"), dict) else {}
        request_record = requests.get(request_id) if isinstance(requests.get(request_id), dict) else {}
        if not request_record:
            return _error("prompt_request_not_found", f"prompt request not found: {request_id}")
        operation = raw_operation or str(request_record.get("operation") or "").strip() or "append_to_composer_end"
        drafts = state.setdefault("voice_prompt_drafts", {})
        existing = drafts.get(request_id) if isinstance(drafts.get(request_id), dict) else {}
        record = {
            "schema": 1,
            "group_id": group.group_id,
            "assistant_id": ASSISTANT_ID_VOICE_SECRETARY,
            "request_id": request_id,
            "status": "pending",
            "operation": operation,
            "draft_text": draft_text,
            "draft_preview": _clean_multiline_text(draft_text, max_len=240),
            "summary": summary,
            "composer_snapshot_hash": composer_snapshot_hash,
            "created_at": str(existing.get("created_at") or now) if isinstance(existing, dict) else now,
            "updated_at": now,
            "by": _assistant_principal(ASSISTANT_ID_VOICE_SECRETARY),
        }
        drafts[request_id] = record
        state["voice_prompt_drafts"] = _trim_voice_prompt_drafts(drafts)
        _save_runtime_state(group, state)
        event = append_event(
            group.ledger_path,
            kind="assistant.voice.prompt_draft",
            group_id=group.group_id,
            scope_key="",
            by=_assistant_principal(ASSISTANT_ID_VOICE_SECRETARY),
            data={
                "assistant_id": ASSISTANT_ID_VOICE_SECRETARY,
                "request_id": request_id,
                "action": "submit",
                "status": "pending",
                "draft_preview": str(record.get("draft_preview") or ""),
            },
        )
        assistant_after = _set_voice_assistant_runtime(
            group,
            lifecycle="waiting",
            health={
                "status": "prompt_draft_ready",
                "last_prompt_request_id": request_id,
                "last_prompt_draft_at": now,
            },
        )
        return DaemonResponse(
            ok=True,
            result={
                "group_id": group.group_id,
                "assistant": assistant_after,
                "prompt_draft": _voice_prompt_draft_public(record),
                "event": event,
            },
        )
    except Exception as exc:
        return _error("assistant_voice_prompt_draft_submit_failed", str(exc))


def handle_assistant_voice_prompt_draft_ack(args: Dict[str, Any]) -> DaemonResponse:
    group_id = str(args.get("group_id") or "").strip()
    by = str(args.get("by") or "user").strip()
    request_id = str(args.get("request_id") or "").strip()
    status = str(args.get("status") or "").strip().lower()
    if not group_id:
        return _error("missing_group_id", "missing group_id")
    if not request_id:
        return _error("missing_request_id", "missing request_id")
    if status not in {"applied", "dismissed", "stale"}:
        return _error("invalid_prompt_draft_status", "status must be applied, dismissed, or stale")
    group = load_group(group_id)
    if group is None:
        return _error("group_not_found", f"group not found: {group_id}")
    try:
        _require_confirmation_permission(group, by=by)
        state = _load_runtime_state(group)
        drafts = state.setdefault("voice_prompt_drafts", {})
        record = drafts.get(request_id) if isinstance(drafts.get(request_id), dict) else {}
        if not record:
            return _error("prompt_draft_not_found", f"prompt draft not found: {request_id}")
        record = dict(record)
        record["status"] = status
        record["updated_at"] = utc_now_iso()
        drafts[request_id] = record
        state["voice_prompt_drafts"] = _trim_voice_prompt_drafts(drafts)
        _save_runtime_state(group, state)
        event = append_event(
            group.ledger_path,
            kind="assistant.voice.prompt_draft",
            group_id=group.group_id,
            scope_key="",
            by=by,
            data={
                "assistant_id": ASSISTANT_ID_VOICE_SECRETARY,
                "request_id": request_id,
                "action": "ack",
                "status": status,
                "draft_preview": str(record.get("draft_preview") or ""),
            },
        )
        assistant_after = _set_voice_assistant_runtime(
            group,
            lifecycle="idle",
            health={
                "status": f"prompt_draft_{status}",
                "last_prompt_request_id": request_id,
                "last_prompt_draft_ack_at": utc_now_iso(),
            },
        )
        return DaemonResponse(
            ok=True,
            result={
                "group_id": group.group_id,
                "assistant": assistant_after,
                "prompt_draft": _voice_prompt_draft_public(record),
                "event": event,
            },
        )
    except Exception as exc:
        return _error("assistant_voice_prompt_draft_ack_failed", str(exc))


def handle_assistant_voice_instruction_feedback(args: Dict[str, Any]) -> DaemonResponse:
    group_id = str(args.get("group_id") or "").strip()
    by = str(args.get("by") or VOICE_SECRETARY_ACTOR_ID).strip()
    raw_request_id = str(args.get("request_id") or args.get("source_request_id") or "").strip()
    status = str(args.get("status") or "").strip().lower()
    reply_text = _clean_multiline_text(args.get("reply_text") or args.get("result_text") or args.get("message"), max_len=4_000)
    document_path = str(args.get("document_path") or args.get("workspace_path") or "").strip()
    artifact_paths = _clean_voice_artifact_paths(args.get("artifact_paths"), document_path=document_path)
    source_summary = _clean_multiline_text(args.get("source_summary"), max_len=1_200)
    checked_at = str(args.get("checked_at") or "").strip()[:120]
    source_urls = _clean_voice_source_urls(args.get("source_urls"))
    if not group_id:
        return _error("missing_group_id", "missing group_id")
    if by not in {VOICE_SECRETARY_ACTOR_ID, _assistant_principal(ASSISTANT_ID_VOICE_SECRETARY), "assistant:voice_secretary"}:
        return _error("assistant_voice_instruction_feedback_forbidden", "voice instruction feedback can only be submitted by voice-secretary")
    if not raw_request_id:
        return _error("missing_voice_ask_request_id", "request_id is required")
    if status not in {"working", "done", "needs_user", "failed"}:
        return _error("invalid_voice_ask_status", "status must be working, done, needs_user, or failed")
    group = load_group(group_id)
    if group is None:
        return _error("group_not_found", f"group not found: {group_id}")
    assistant = _effective_assistant(group, ASSISTANT_ID_VOICE_SECRETARY)
    if not bool(assistant.get("enabled")):
        return _error("assistant_disabled", "voice_secretary is disabled")
    try:
        request_id = _clean_voice_ask_request_id(raw_request_id)
        now = utc_now_iso()
        state = _load_runtime_state(group)
        requests = state.get("voice_ask_requests") if isinstance(state.get("voice_ask_requests"), dict) else {}
        existing = requests.get(request_id) if isinstance(requests.get(request_id), dict) else {}
        if not existing:
            return _error("voice_ask_request_not_found", f"voice ask request not found: {request_id}")
        record = _upsert_voice_ask_request(
            state,
            group=group,
            request_id=request_id,
            status=status,
            document_path=document_path,
            artifact_paths=artifact_paths,
            reply_text=reply_text,
            source_summary=source_summary,
            checked_at=checked_at,
            source_urls=source_urls,
            feedback_at=now,
            now=now,
        )
        _save_runtime_state(group, state)
        event = append_event(
            group.ledger_path,
            kind="assistant.voice.request",
            group_id=group.group_id,
            scope_key="",
            by=_assistant_principal(ASSISTANT_ID_VOICE_SECRETARY),
            data={
                "assistant_id": ASSISTANT_ID_VOICE_SECRETARY,
                "request_id": request_id,
                "target_actor_id": "",
                "action": "report",
                "status": status,
                "source_request_id": request_id,
                "document_path": str(record.get("document_path") or ""),
                "artifact_paths": _clean_voice_artifact_paths(record.get("artifact_paths")),
                "source_summary": str(record.get("source_summary") or ""),
                "checked_at": str(record.get("checked_at") or ""),
                "source_urls": _clean_voice_source_urls(record.get("source_urls")),
                "request_preview": str(record.get("request_preview") or ""),
                "reply_text": str(record.get("reply_text") or ""),
            },
        )
        assistant_after = _set_voice_assistant_runtime(
            group,
            lifecycle="working" if status == "working" else "waiting" if status == "needs_user" else "idle",
            health={
                "status": f"ask_{status}",
                "last_ask_request_id": request_id,
                "last_ask_feedback_at": now,
            },
        )
        return DaemonResponse(
            ok=True,
            result={
                "group_id": group.group_id,
                "assistant": assistant_after,
                "ask_request": _voice_ask_request_public(record),
                "event": event,
            },
        )
    except Exception as exc:
        return _error("assistant_voice_instruction_feedback_failed", str(exc))


def handle_assistant_voice_ask_requests_clear(args: Dict[str, Any]) -> DaemonResponse:
    group_id = str(args.get("group_id") or "").strip()
    keep_active = bool(args.get("keep_active", False))
    if not group_id:
        return _error("missing_group_id", "missing group_id")
    group = load_group(group_id)
    if group is None:
        return _error("group_not_found", f"group not found: {group_id}")
    try:
        now = utc_now_iso()
        state = _load_runtime_state(group)
        requests = state.get("voice_ask_requests") if isinstance(state.get("voice_ask_requests"), dict) else {}
        active_statuses = {"pending", "working"}
        cleared = 0
        for key, value in requests.items():
            if not isinstance(value, dict):
                continue
            status = str(value.get("status") or "pending").strip().lower()
            if keep_active and status in active_statuses:
                continue
            if str(value.get("cleared_at") or "").strip():
                continue
            next_value = dict(value)
            next_value["cleared_at"] = now
            next_value["updated_at"] = now
            requests[str(key)] = next_value
            cleared += 1
        state["voice_ask_requests"] = _trim_voice_ask_requests(requests)
        _save_runtime_state(group, state)
        return DaemonResponse(
            ok=True,
            result={
                "group_id": group.group_id,
                "assistant": _effective_assistant(group, ASSISTANT_ID_VOICE_SECRETARY),
                "ask_requests": _voice_ask_requests_public(state),
                "cleared_count": cleared,
                "removed_count": cleared,
                "kept_count": len(_voice_ask_requests_public(state)),
            },
        )
    except Exception as exc:
        return _error("assistant_voice_ask_requests_clear_failed", str(exc))


def handle_assistant_voice_request(args: Dict[str, Any]) -> DaemonResponse:
    group_id = str(args.get("group_id") or "").strip()
    by = str(args.get("by") or VOICE_SECRETARY_ACTOR_ID).strip()
    action = str(args.get("action") or "handoff").strip().lower()
    if action == "report":
        return handle_assistant_voice_instruction_feedback(args)
    if action != "handoff":
        return _error("invalid_voice_secretary_request_action", "action must be handoff or report")
    request_text = _clean_multiline_text(args.get("request_text"), max_len=4_000)
    summary = _clean_multiline_text(args.get("summary"), max_len=800)
    document_path = str(args.get("document_path") or args.get("workspace_path") or "").strip()
    source_event_id = str(args.get("source_event_id") or "").strip()
    source_request_id = str(args.get("source_request_id") or "").strip()
    priority = str(args.get("priority") or "normal").strip().lower()
    requires_ack = coerce_bool(args.get("requires_ack"), default=True)
    if priority not in {"low", "normal", "high", "urgent"}:
        priority = "normal"
    if not group_id:
        return _error("missing_group_id", "missing group_id")
    if not request_text:
        return _error("empty_voice_secretary_request", "request_text is required")
    if by not in {VOICE_SECRETARY_ACTOR_ID, _assistant_principal(ASSISTANT_ID_VOICE_SECRETARY)}:
        return _error("assistant_voice_request_forbidden", "voice secretary request can only be sent by the voice-secretary actor")
    group = load_group(group_id)
    if group is None:
        return _error("group_not_found", f"group not found: {group_id}")
    assistant = _effective_assistant(group, ASSISTANT_ID_VOICE_SECRETARY)
    if not bool(assistant.get("enabled")):
        return _error("assistant_disabled", "voice_secretary is disabled")
    try:
        target_actor_id, target_requested = _resolve_voice_secretary_request_target(group, args.get("target"))
    except Exception as exc:
        return _error("assistant_voice_request_failed", str(exc))

    request_id = f"voice-request-{uuid.uuid4().hex}"
    clean_source_request_id = _clean_voice_ask_request_id(source_request_id) if source_request_id else ""
    request_preview = _clean_multiline_text(request_text, max_len=240)
    notify_message = request_preview
    if summary:
        notify_message = f"{summary}\n\n{request_preview}".strip()
    notify = SystemNotifyData(
        kind="info",
        priority=priority,  # type: ignore[arg-type]
        title="Voice Secretary action request",
        message=notify_message,
        target_actor_id=target_actor_id,
        requires_ack=requires_ack,
        related_event_id=source_event_id or None,
        context={
            "kind": "voice_secretary_action_request",
            "assistant_id": ASSISTANT_ID_VOICE_SECRETARY,
            "request_id": request_id,
            "source_request_id": clean_source_request_id,
            "target_requested": target_requested,
            "target_actor_id": target_actor_id,
            "document_path": document_path,
            "source_event_id": source_event_id,
            "summary": summary,
            "request_text": request_text,
            "request_preview": request_preview,
        },
    )
    notify_event = emit_system_notify(
        group,
        by=_assistant_principal(ASSISTANT_ID_VOICE_SECRETARY),
        notify=notify,
    )
    notify_event_id = str(notify_event.get("id") or "").strip()
    ask_request: Dict[str, Any] = {}
    if clean_source_request_id:
        state = _load_runtime_state(group)
        ask_request = _upsert_voice_ask_request(
            state,
            group=group,
            request_id=clean_source_request_id,
            status="handed_off",
            document_path=document_path,
            reply_text=summary or request_preview,
            handoff_target=target_requested,
            handoff_request_id=request_id,
            target_actor_id=target_actor_id,
            now=utc_now_iso(),
        )
        _save_runtime_state(group, state)
    event = append_event(
        group.ledger_path,
        kind="assistant.voice.request",
        group_id=group.group_id,
        scope_key="",
        by=_assistant_principal(ASSISTANT_ID_VOICE_SECRETARY),
        data={
                "assistant_id": ASSISTANT_ID_VOICE_SECRETARY,
                "request_id": request_id,
                "target_actor_id": target_actor_id,
                "action": "handoff",
                "status": "handed_off" if clean_source_request_id else "",
                "source_request_id": clean_source_request_id,
                "document_path": document_path,
                "source_event_id": source_event_id,
                "request_preview": request_preview,
                "reply_text": summary or request_preview,
                "notify_event_id": notify_event_id,
            },
    )
    assistant_after = _set_voice_assistant_runtime(
        group,
        lifecycle="waiting" if requires_ack else "idle",
        health={
            "status": "request_sent",
            "last_request_id": request_id,
            "last_request_target_actor_id": target_actor_id,
            "last_request_notify_event_id": notify_event_id,
        },
    )
    return DaemonResponse(
        ok=True,
        result={
            "group_id": group.group_id,
            "assistant": assistant_after,
                "request": {
                    "request_id": request_id,
                    "source_request_id": clean_source_request_id,
                    "target_actor_id": target_actor_id,
                    "target_requested": target_requested,
                    "document_path": document_path,
                    "request_preview": request_preview,
                    "requires_ack": requires_ack,
                },
                "ask_request": _voice_ask_request_public(ask_request) if ask_request else {},
                "notify_event": notify_event,
                "event": event,
            },
    )


def try_handle_assistant_op(
    op: str,
    args: Dict[str, Any],
    *,
    effective_runner_kind: Callable[[str], str],
    start_actor_process: Callable[..., dict[str, Any]],
    load_actor_private_env: Callable[[str, str], Dict[str, str]],
    update_actor_private_env: Callable[..., Dict[str, str]],
    delete_actor_private_env: Callable[[str, str], None],
    get_actor_profile: Callable[[str], Optional[Dict[str, Any]]],
    load_actor_profile_secrets: Callable[[str], Dict[str, str]],
    remove_headless_state: Callable[[str, str], None],
    remove_pty_state_if_pid: Callable[..., None],
) -> Optional[DaemonResponse]:
    if op == "assistant_state":
        return handle_assistant_state(args)
    if op == "assistant_settings_update":
        return handle_assistant_settings_update(
            args,
            effective_runner_kind=effective_runner_kind,
            start_actor_process=start_actor_process,
            load_actor_private_env=load_actor_private_env,
            update_actor_private_env=update_actor_private_env,
            delete_actor_private_env=delete_actor_private_env,
            get_actor_profile=get_actor_profile,
            load_actor_profile_secrets=load_actor_profile_secrets,
            remove_headless_state=remove_headless_state,
            remove_pty_state_if_pid=remove_pty_state_if_pid,
        )
    if op == "assistant_status_update":
        return handle_assistant_status_update(args)
    if op == "assistant_voice_transcribe":
        return handle_assistant_voice_transcribe(args)
    if op == "assistant_voice_model_install":
        return handle_assistant_voice_model_install(args)
    if op == "assistant_voice_runtime_install":
        return handle_assistant_voice_runtime_install(args)
    if op == "assistant_voice_transcript_append":
        return handle_assistant_voice_transcript_append(
            args,
            effective_runner_kind=effective_runner_kind,
            start_actor_process=start_actor_process,
        )
    if op == "assistant_voice_document_list":
        return handle_assistant_voice_document_list(args)
    if op == "assistant_voice_document_select":
        return handle_assistant_voice_document_select(args)
    if op == "assistant_voice_document_input_read":
        return handle_assistant_voice_document_input_read(args)
    if op == "assistant_voice_document_save":
        return handle_assistant_voice_document_save(args)
    if op == "assistant_voice_input_append":
        return handle_assistant_voice_input_append(
            args,
            effective_runner_kind=effective_runner_kind,
            start_actor_process=start_actor_process,
        )
    if op == "assistant_voice_document_instruction":
        return handle_assistant_voice_document_instruction(
            args,
            effective_runner_kind=effective_runner_kind,
            start_actor_process=start_actor_process,
        )
    if op == "assistant_voice_document_archive":
        return handle_assistant_voice_document_archive(args)
    if op == "assistant_voice_prompt_draft_submit":
        return handle_assistant_voice_prompt_draft_submit(args)
    if op == "assistant_voice_prompt_draft_ack":
        return handle_assistant_voice_prompt_draft_ack(args)
    if op == "assistant_voice_instruction_feedback":
        return handle_assistant_voice_instruction_feedback(args)
    if op == "assistant_voice_ask_requests_clear":
        return handle_assistant_voice_ask_requests_clear(args)
    if op == "assistant_voice_request":
        return handle_assistant_voice_request(args)
    return None
