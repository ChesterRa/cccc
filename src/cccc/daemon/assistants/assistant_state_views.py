"""Response views for assistant state payloads."""

from __future__ import annotations

from typing import Any, Dict


VOICE_STATUS_VIEWS = {"voice_status"}
VOICE_WORKSPACE_VIEWS = {"voice_workspace"}

_VOICE_STATUS_KEYS = {
    "group_id",
    "assistant",
    "active_document_id",
    "capture_target_document_id",
    "active_document_path",
    "capture_target_document_path",
    "new_input_available",
    "input_timing",
    "prompt_draft",
    "ask_requests",
    "latest_ask_request",
    "service_runtime",
    "service_runtimes",
    "service_runtimes_by_id",
    "recording_lease",
}


def is_voice_status_view(view: str) -> bool:
    return str(view or "").strip() in VOICE_STATUS_VIEWS


def is_voice_workspace_view(view: str) -> bool:
    return str(view or "").strip() in VOICE_WORKSPACE_VIEWS


def assistant_state_view(result: Dict[str, Any], view: str) -> Dict[str, Any]:
    clean_view = str(view or "").strip()
    if clean_view not in VOICE_STATUS_VIEWS | VOICE_WORKSPACE_VIEWS:
        return result
    assistant = result.get("assistant") if isinstance(result.get("assistant"), dict) else {}
    if str(assistant.get("assistant_id") or "").strip() != "voice_secretary":
        return result
    if clean_view in VOICE_STATUS_VIEWS:
        return {key: result[key] for key in _VOICE_STATUS_KEYS if key in result}
    projected = dict(result)
    projected.pop("service_models", None)
    projected.pop("service_models_by_id", None)
    return projected
