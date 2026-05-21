"""Voice Secretary control-turn output completion helpers."""

from __future__ import annotations

from typing import Any, Dict, List

from ...kernel.group import load_group
from ...kernel.ledger import append_event
from ...kernel.voice_secretary_actor import VOICE_SECRETARY_ACTOR_ID
from ...util.time import utc_now_iso
from .assistant_ops import (
    ASSISTANT_ID_VOICE_SECRETARY,
    _MAX_PROMPT_DRAFT_CHARS,
    _assistant_principal,
    _clean_multiline_text,
    _effective_assistant,
    _load_runtime_state,
    _save_runtime_state,
    _set_voice_assistant_runtime,
    _trim_voice_prompt_drafts,
)

_PROMPT_DRAFT_NO_OP_STATUSES = {"no_change", "no_op"}


def _missing_composer_request_ids(
    diagnostics: Dict[str, Any],
    *,
    prefixes: tuple[str, ...] = ("composer_draft:",),
) -> List[str]:
    missing = diagnostics.get("missing") if isinstance(diagnostics.get("missing"), list) else []
    out: List[str] = []
    for item in missing:
        text = str(item or "").strip()
        if not text.startswith(prefixes):
            continue
        request_id = text.split(":", 1)[1].strip()
        if request_id and request_id not in out:
            out.append(request_id)
    return out


def _fallback_draft_text(request_record: Dict[str, Any]) -> str:
    transcripts = request_record.get("voice_transcripts") if isinstance(request_record.get("voice_transcripts"), list) else []
    voice_parts = [
        _clean_multiline_text(item, max_len=4_000)
        for item in transcripts
        if str(item or "").strip()
    ]
    text = _clean_multiline_text("\n\n".join(voice_parts[-3:]), max_len=_MAX_PROMPT_DRAFT_CHARS)
    if text:
        return text
    return _clean_multiline_text(request_record.get("composer_text"), max_len=_MAX_PROMPT_DRAFT_CHARS)


def complete_missing_composer_drafts(*, group_id: str, diagnostics: Dict[str, Any]) -> Dict[str, Any]:
    normalized_diagnostics = diagnostics if isinstance(diagnostics, dict) else {}
    missing_draft_request_ids = _missing_composer_request_ids(normalized_diagnostics)
    missing_updated_at_request_ids = _missing_composer_request_ids(
        normalized_diagnostics,
        prefixes=("composer_draft_updated_at:",),
    )
    request_ids = list(dict.fromkeys(missing_draft_request_ids + missing_updated_at_request_ids))
    if not request_ids:
        return {"completed_request_ids": []}
    group = load_group(group_id)
    if group is None:
        return {"completed_request_ids": [], "error": "group_not_found"}
    assistant = _effective_assistant(group, ASSISTANT_ID_VOICE_SECRETARY)
    if not bool(assistant.get("enabled")):
        return {"completed_request_ids": [], "error": "assistant_disabled"}

    now = utc_now_iso()
    state = _load_runtime_state(group)
    requests = state.get("voice_prompt_requests") if isinstance(state.get("voice_prompt_requests"), dict) else {}
    drafts = state.setdefault("voice_prompt_drafts", {})
    completed: List[str] = []
    events: List[Dict[str, Any]] = []
    pending_draft_completed = False
    for request_id in request_ids:
        existing = drafts.get(request_id) if isinstance(drafts.get(request_id), dict) else {}
        existing_status = str(existing.get("status") or "").strip().lower()
        if existing_status in _PROMPT_DRAFT_NO_OP_STATUSES:
            if request_id in missing_updated_at_request_ids:
                updated = dict(existing)
                updated["updated_at"] = now
                if not str(updated.get("by") or "").strip():
                    updated["by"] = _assistant_principal(ASSISTANT_ID_VOICE_SECRETARY)
                drafts[request_id] = updated
                events.append(
                    append_event(
                        group.ledger_path,
                        kind="assistant.voice.prompt_draft",
                        group_id=group.group_id,
                        scope_key="",
                        by=_assistant_principal(ASSISTANT_ID_VOICE_SECRETARY),
                        data={
                            "assistant_id": ASSISTANT_ID_VOICE_SECRETARY,
                            "request_id": request_id,
                            "action": "auto_refresh",
                            "status": str(updated.get("status") or "no_change"),
                            "draft_preview": "",
                        },
                    )
                )
            completed.append(request_id)
            continue
        if request_id in missing_updated_at_request_ids and str(existing.get("draft_text") or "").strip():
            updated = dict(existing)
            updated["updated_at"] = now
            if not str(updated.get("by") or "").strip():
                updated["by"] = _assistant_principal(ASSISTANT_ID_VOICE_SECRETARY)
            drafts[request_id] = updated
            completed.append(request_id)
            events.append(
                append_event(
                    group.ledger_path,
                    kind="assistant.voice.prompt_draft",
                    group_id=group.group_id,
                    scope_key="",
                    by=_assistant_principal(ASSISTANT_ID_VOICE_SECRETARY),
                    data={
                        "assistant_id": ASSISTANT_ID_VOICE_SECRETARY,
                        "request_id": request_id,
                        "action": "auto_refresh",
                        "status": str(updated.get("status") or "pending"),
                        "draft_preview": str(updated.get("draft_preview") or ""),
                    },
                )
            )
            pending_draft_completed = True
            continue
        if str(existing.get("draft_text") or "").strip():
            completed.append(request_id)
            pending_draft_completed = True
            continue
        request_record = requests.get(request_id) if isinstance(requests.get(request_id), dict) else {}
        if not request_record:
            continue
        draft_text = _fallback_draft_text(request_record)
        if not draft_text:
            continue
        operation = str(request_record.get("operation") or "append_to_composer_end").strip() or "append_to_composer_end"
        record = {
            "schema": 1,
            "group_id": group.group_id,
            "assistant_id": ASSISTANT_ID_VOICE_SECRETARY,
            "request_id": request_id,
            "status": "pending",
            "operation": operation,
            "draft_text": draft_text,
            "draft_preview": _clean_multiline_text(draft_text, max_len=240),
            "summary": "Auto-filled from captured composer input after the control turn did not submit a draft.",
            "composer_snapshot_hash": str(request_record.get("composer_snapshot_hash") or ""),
            "created_at": str(existing.get("created_at") or now) if isinstance(existing, dict) else now,
            "updated_at": now,
            "by": _assistant_principal(ASSISTANT_ID_VOICE_SECRETARY),
            "auto_filled": True,
        }
        drafts[request_id] = record
        completed.append(request_id)
        pending_draft_completed = True
        events.append(
            append_event(
                group.ledger_path,
                kind="assistant.voice.prompt_draft",
                group_id=group.group_id,
                scope_key="",
                by=_assistant_principal(ASSISTANT_ID_VOICE_SECRETARY),
                data={
                    "assistant_id": ASSISTANT_ID_VOICE_SECRETARY,
                    "request_id": request_id,
                    "action": "auto_submit",
                    "status": "pending",
                    "draft_preview": str(record.get("draft_preview") or ""),
                },
            )
        )

    if completed:
        state["voice_prompt_drafts"] = _trim_voice_prompt_drafts(drafts)
        _save_runtime_state(group, state)
        lifecycle = "waiting" if pending_draft_completed else "idle"
        health_status = "prompt_draft_auto_filled" if pending_draft_completed else "prompt_draft_no_change"
        _set_voice_assistant_runtime(
            group,
            lifecycle=lifecycle,
            health={
                "status": health_status,
                "last_prompt_request_id": completed[-1],
                "last_prompt_draft_at": now,
            },
        )
    return {
        "completed_request_ids": completed,
        "events": events,
        "by": VOICE_SECRETARY_ACTOR_ID,
    }
