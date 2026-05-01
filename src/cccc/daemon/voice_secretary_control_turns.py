"""Shared Voice Secretary headless control-turn helpers."""

from __future__ import annotations

import logging
from typing import Any, Dict

from ..kernel.group import load_group
from ..kernel.inbox import find_event
from ..paths import ensure_home
from ..util.fs import read_json

logger = logging.getLogger(__name__)


def _input_state(group_id: str) -> Dict[str, int]:
    path = ensure_home() / "voice-secretary" / str(group_id or "").strip() / "input_state.json"
    payload = read_json(path)
    if not isinstance(payload, dict):
        return {"latest_seq": 0, "secretary_read_cursor": 0}
    read_cursor = max(0, int(payload.get("secretary_read_cursor") or 0))
    delivery_cursor = max(0, int(payload.get("secretary_delivery_cursor") or read_cursor))
    return {
        "latest_seq": max(0, int(payload.get("latest_seq") or 0)),
        "secretary_read_cursor": max(read_cursor, delivery_cursor),
    }


def _prompt_draft_state(group_id: str, *, request_ids: list[str]) -> Dict[str, Dict[str, Any]]:
    if not request_ids:
        return {}
    group = load_group(group_id)
    if group is None:
        return {}
    payload = read_json(group.path / "state" / "assistants.json")
    if not isinstance(payload, dict):
        return {}
    drafts = payload.get("voice_prompt_drafts") if isinstance(payload.get("voice_prompt_drafts"), dict) else {}
    out: Dict[str, Dict[str, Any]] = {}
    for request_id in request_ids:
        normalized = str(request_id or "").strip()
        if not normalized:
            continue
        draft = drafts.get(normalized) if isinstance(drafts.get(normalized), dict) else {}
        out[normalized] = {
            "updated_at": str(draft.get("updated_at") or "").strip(),
            "draft_text": str(draft.get("draft_text") or ""),
            "status": str(draft.get("status") or "").strip(),
        }
    return out


def _ask_request_state(group_id: str, *, request_ids: list[str]) -> Dict[str, Dict[str, Any]]:
    if not request_ids:
        return {}
    group = load_group(group_id)
    if group is None:
        return {}
    payload = read_json(group.path / "state" / "assistants.json")
    if not isinstance(payload, dict):
        return {}
    requests = payload.get("voice_ask_requests") if isinstance(payload.get("voice_ask_requests"), dict) else {}
    out: Dict[str, Dict[str, Any]] = {}
    for request_id in request_ids:
        normalized = str(request_id or "").strip()
        if not normalized:
            continue
        request = requests.get(normalized) if isinstance(requests.get(normalized), dict) else {}
        out[normalized] = {
            "updated_at": str(request.get("updated_at") or "").strip(),
            "reply_text": str(request.get("reply_text") or ""),
            "status": str(request.get("status") or "").strip(),
        }
    return out


def _request_ids_from_input_batches(input_batches: list[Any]) -> tuple[list[str], list[str], list[str]]:
    composer_request_ids: list[str] = []
    report_request_ids: list[str] = []
    input_target_kinds: list[str] = []
    for batch in input_batches:
        if not isinstance(batch, dict):
            continue
        target_kind = str(batch.get("target_kind") or "").strip().lower()
        if target_kind:
            input_target_kinds.append(target_kind)
        request_ids = [
            str(item).strip()
            for item in (batch.get("request_ids") if isinstance(batch.get("request_ids"), list) else [])
            if str(item).strip()
        ]
        if target_kind == "composer":
            for request_id in request_ids:
                if request_id not in composer_request_ids:
                    composer_request_ids.append(request_id)
            continue
        if not bool(batch.get("requires_report")) and target_kind not in {"secretary", "document"}:
            continue
        for request_id in request_ids:
            if request_id not in report_request_ids:
                report_request_ids.append(request_id)
    return composer_request_ids, report_request_ids, input_target_kinds


def _voice_input_snapshot_from_envelope(*, group_id: str, event_id: str, envelope: Dict[str, Any]) -> Dict[str, Any]:
    input_batches = envelope.get("input_batches") if isinstance(envelope.get("input_batches"), list) else []
    composer_request_ids = [
        str(item).strip()
        for item in (envelope.get("composer_request_ids") if isinstance(envelope.get("composer_request_ids"), list) else [])
        if str(item).strip()
    ]
    report_request_ids = [
        str(item).strip()
        for item in (envelope.get("report_request_ids") if isinstance(envelope.get("report_request_ids"), list) else [])
        if str(item).strip()
    ]
    legacy_report_request_ids = [
        str(item).strip()
        for item in (envelope.get("secretary_request_ids") if isinstance(envelope.get("secretary_request_ids"), list) else [])
        if str(item).strip()
    ]
    if not report_request_ids:
        report_request_ids = legacy_report_request_ids
    input_target_kinds = [
        str(item).strip().lower()
        for item in (envelope.get("input_target_kinds") if isinstance(envelope.get("input_target_kinds"), list) else [])
        if str(item).strip()
    ]
    if not composer_request_ids and not report_request_ids:
        composer_request_ids, report_request_ids, input_target_kinds = _request_ids_from_input_batches(input_batches)
    seq_end = max(0, int(envelope.get("seq_end") or envelope.get("latest_seq") or 0))
    latest_seq = max(seq_end, int(envelope.get("latest_seq") or 0))
    return {
        "kind": "voice_secretary_input",
        "event_id": str(event_id or "").strip(),
        "before_latest_seq": latest_seq,
        "before_secretary_read_cursor": seq_end,
        "input_envelope_delivered": True,
        "delivery_id": str(envelope.get("delivery_id") or "").strip(),
        "composer_request_ids": composer_request_ids,
        "secretary_request_ids": report_request_ids,
        "report_request_ids": report_request_ids,
        "input_target_kinds": input_target_kinds,
        "before_prompt_drafts": _prompt_draft_state(group_id, request_ids=composer_request_ids),
        "before_ask_requests": _ask_request_state(group_id, request_ids=report_request_ids),
    }


def control_snapshot(*, group_id: str, actor_id: str, event_id: str, control_kind: str) -> Dict[str, Any]:
    if str(actor_id or "").strip() != "voice-secretary":
        return {}
    if str(control_kind or "").strip().lower() != "system_notify":
        return {}
    group = load_group(group_id)
    if group is None:
        return {}
    event = find_event(group, str(event_id or "").strip())
    if not isinstance(event, dict):
        return {}
    if str(event.get("kind") or "").strip() != "system.notify":
        return {}
    data = event.get("data") if isinstance(event.get("data"), dict) else {}
    context = data.get("context") if isinstance(data.get("context"), dict) else {}
    if str(context.get("kind") or "").strip() != "voice_secretary_input":
        return {}
    envelope = context.get("input_envelope") if isinstance(context.get("input_envelope"), dict) else {}
    if envelope:
        return _voice_input_snapshot_from_envelope(group_id=group.group_id, event_id=event_id, envelope=envelope)

    state = _input_state(group.group_id)
    try:
        from .assistants.assistant_ops import _peek_voice_input_batch

        preview = _peek_voice_input_batch(group)
        input_batches = preview.get("input_batches") if isinstance(preview.get("input_batches"), list) else []
        composer_request_ids = [
            str(item).strip()
            for item in ((preview or {}).get("composer_request_ids") if isinstance((preview or {}).get("composer_request_ids"), list) else [])
            if str(item).strip()
        ]
        report_request_ids = [
            str(item).strip()
            for item in ((preview or {}).get("report_request_ids") if isinstance((preview or {}).get("report_request_ids"), list) else [])
            if str(item).strip()
        ]
        if not report_request_ids:
            report_request_ids = [
                str(item).strip()
                for item in ((preview or {}).get("secretary_request_ids") if isinstance((preview or {}).get("secretary_request_ids"), list) else [])
                if str(item).strip()
            ]
        input_target_kinds = [
            str(item.get("target_kind") or "").strip().lower()
            for item in input_batches
            if isinstance(item, dict) and str(item.get("target_kind") or "").strip()
        ]
    except Exception:
        composer_request_ids = []
        report_request_ids = []
        input_target_kinds = []

    return {
        "kind": "voice_secretary_input",
        "event_id": str(event_id or "").strip(),
        "before_latest_seq": int(state.get("latest_seq") or 0),
        "before_secretary_read_cursor": int(state.get("secretary_read_cursor") or 0),
        "composer_request_ids": composer_request_ids,
        "secretary_request_ids": report_request_ids,
        "report_request_ids": report_request_ids,
        "input_target_kinds": input_target_kinds,
        "before_prompt_drafts": _prompt_draft_state(group.group_id, request_ids=composer_request_ids),
        "before_ask_requests": _ask_request_state(group.group_id, request_ids=report_request_ids),
    }


def prefetched_control_snapshot(*, group_id: str, event_id: str, prefetched_input: Dict[str, Any]) -> Dict[str, Any]:
    input_batches = prefetched_input.get("input_batches") if isinstance(prefetched_input.get("input_batches"), list) else []
    input_timing = prefetched_input.get("input_timing") if isinstance(prefetched_input.get("input_timing"), dict) else {}
    composer_request_ids, report_request_ids, input_target_kinds = _request_ids_from_input_batches(input_batches)
    latest_seq = max(0, int(input_timing.get("latest_seq") or 0))
    current_cursor = max(0, int(input_timing.get("secretary_read_cursor") or latest_seq))
    return {
        "kind": "voice_secretary_input",
        "event_id": str(event_id or "").strip(),
        "before_latest_seq": latest_seq,
        "before_secretary_read_cursor": current_cursor,
        "prefetched_read_new_input": True,
        "composer_request_ids": composer_request_ids,
        "secretary_request_ids": report_request_ids,
        "report_request_ids": report_request_ids,
        "input_target_kinds": input_target_kinds,
        "before_prompt_drafts": _prompt_draft_state(group_id, request_ids=composer_request_ids),
        "before_ask_requests": _ask_request_state(group_id, request_ids=report_request_ids),
    }


def prepare_control_turn(
    *,
    group_id: str,
    actor_id: str,
    text: str,
    event_id: str,
    control_kind: str,
    snapshot_fn: Any = None,
) -> tuple[str, Dict[str, Any]]:
    base_text = str(text or "")
    build_snapshot = snapshot_fn if callable(snapshot_fn) else control_snapshot
    snapshot = build_snapshot(
        group_id=group_id,
        actor_id=actor_id,
        event_id=event_id,
        control_kind=control_kind,
    )
    if str((snapshot or {}).get("kind") or "").strip() != "voice_secretary_input":
        return base_text, snapshot
    if bool((snapshot or {}).get("input_envelope_delivered")):
        return base_text, snapshot
    try:
        from .assistants.assistant_ops import handle_assistant_voice_document_input_read

        response = handle_assistant_voice_document_input_read(
            {
                "group_id": group_id,
                "by": "voice-secretary",
            }
        )
    except Exception:
        logger.exception("voice-secretary control prefetch crashed: %s", group_id)
        return base_text, snapshot
    if not bool(getattr(response, "ok", False)):
        return base_text, snapshot
    result = response.result if isinstance(response.result, dict) else {}
    if not isinstance(result, dict):
        return base_text, snapshot
    input_text = str(result.get("input_text") or "").strip()
    prefetched_snapshot = prefetched_control_snapshot(
        group_id=group_id,
        event_id=event_id,
        prefetched_input=result,
    )
    if not input_text:
        return base_text, prefetched_snapshot
    lines = [
        base_text.strip(),
        "",
        "[CCCC] SYSTEM PREFETCH: read_new_input already ran before this turn.",
        "Use the fetched secretary input below as the source of truth for this turn.",
        "Do not go back to the original notify pointer text.",
        "",
        "[CCCC] FETCHED INPUT:",
        input_text,
    ]
    return "\n".join(part for part in lines if part).strip(), prefetched_snapshot


def control_consumption_diagnostics(*, group_id: str, snapshot: Dict[str, Any]) -> Dict[str, Any]:
    if str((snapshot or {}).get("kind") or "").strip() != "voice_secretary_input":
        return {}
    before_latest = int((snapshot or {}).get("before_latest_seq") or 0)
    before_cursor = int((snapshot or {}).get("before_secretary_read_cursor") or 0)
    prefetched = bool((snapshot or {}).get("prefetched_read_new_input"))
    envelope_delivered = bool((snapshot or {}).get("input_envelope_delivered"))
    state = _input_state(group_id)
    current_cursor = int(state.get("secretary_read_cursor") or 0)
    cursor_advanced = current_cursor > before_cursor
    composer_request_ids = [
        str(item).strip()
        for item in ((snapshot or {}).get("composer_request_ids") if isinstance((snapshot or {}).get("composer_request_ids"), list) else [])
        if str(item).strip()
    ]
    report_request_ids = [
        str(item).strip()
        for item in ((snapshot or {}).get("report_request_ids") if isinstance((snapshot or {}).get("report_request_ids"), list) else [])
        if str(item).strip()
    ]
    if not report_request_ids:
        report_request_ids = [
            str(item).strip()
            for item in ((snapshot or {}).get("secretary_request_ids") if isinstance((snapshot or {}).get("secretary_request_ids"), list) else [])
            if str(item).strip()
        ]
    input_target_kinds = [
        str(item).strip().lower()
        for item in ((snapshot or {}).get("input_target_kinds") if isinstance((snapshot or {}).get("input_target_kinds"), list) else [])
        if str(item).strip()
    ]
    if before_latest <= before_cursor and not composer_request_ids and not report_request_ids:
        return {
            "before_latest_seq": before_latest,
            "before_secretary_read_cursor": before_cursor,
            "current_secretary_read_cursor": current_cursor,
            "cursor_advanced": cursor_advanced,
            "missing": [],
        }

    missing: list[str] = []
    if not prefetched and not envelope_delivered and not cursor_advanced:
        missing.append("read_new_input")
    if report_request_ids:
        before_ask_requests = (snapshot or {}).get("before_ask_requests") if isinstance((snapshot or {}).get("before_ask_requests"), dict) else {}
        current_ask_requests = _ask_request_state(group_id, request_ids=report_request_ids)
        for request_id in report_request_ids:
            current = current_ask_requests.get(request_id) if isinstance(current_ask_requests.get(request_id), dict) else {}
            before = before_ask_requests.get(request_id) if isinstance(before_ask_requests.get(request_id), dict) else {}
            if str(current.get("status") or "").strip() not in {"done", "needs_user", "failed", "handed_off"}:
                missing.append(f"secretary_report:{request_id}")
                continue
            if not str(current.get("reply_text") or "").strip():
                missing.append(f"secretary_reply_text:{request_id}")
                continue
            if str(current.get("updated_at") or "").strip() == str(before.get("updated_at") or "").strip():
                missing.append(f"secretary_report_updated_at:{request_id}")
    if composer_request_ids:
        before_prompt_drafts = (snapshot or {}).get("before_prompt_drafts") if isinstance((snapshot or {}).get("before_prompt_drafts"), dict) else {}
        current_prompt_drafts = _prompt_draft_state(group_id, request_ids=composer_request_ids)
        for request_id in composer_request_ids:
            current = current_prompt_drafts.get(request_id) if isinstance(current_prompt_drafts.get(request_id), dict) else {}
            before = before_prompt_drafts.get(request_id) if isinstance(before_prompt_drafts.get(request_id), dict) else {}
            if not str(current.get("draft_text") or "").strip():
                missing.append(f"composer_draft:{request_id}")
                continue
            if str(current.get("updated_at") or "").strip() == str(before.get("updated_at") or "").strip():
                missing.append(f"composer_draft_updated_at:{request_id}")
    return {
        "before_latest_seq": before_latest,
        "before_secretary_read_cursor": before_cursor,
        "current_secretary_read_cursor": current_cursor,
        "cursor_advanced": cursor_advanced,
        "prefetched_read_new_input": prefetched,
        "input_envelope_delivered": envelope_delivered,
        "delivery_id": str((snapshot or {}).get("delivery_id") or "").strip(),
        "input_target_kinds": input_target_kinds,
        "composer_request_ids": composer_request_ids,
        "secretary_request_ids": report_request_ids,
        "report_request_ids": report_request_ids,
        "missing": sorted(set(missing)),
    }


def control_consumed_input(*, group_id: str, snapshot: Dict[str, Any]) -> bool:
    diagnostics = control_consumption_diagnostics(group_id=group_id, snapshot=snapshot)
    return not bool(diagnostics.get("missing") if isinstance(diagnostics, dict) else [])


def control_completion_state(
    *,
    group_id: str,
    snapshot: Dict[str, Any],
    diagnostics_fn: Any = None,
) -> tuple[bool, Dict[str, Any]]:
    get_diagnostics = diagnostics_fn if callable(diagnostics_fn) else control_consumption_diagnostics
    diagnostics = get_diagnostics(group_id=group_id, snapshot=snapshot)
    if diagnostics.get("missing"):
        try:
            from .assistants.voice_secretary_output_completion import complete_missing_composer_drafts

            repaired = complete_missing_composer_drafts(group_id=group_id, diagnostics=diagnostics)
        except Exception:
            logger.exception("voice-secretary output completion crashed: %s", group_id)
            repaired = {}
        if isinstance(repaired, dict) and repaired.get("completed_request_ids"):
            diagnostics = get_diagnostics(group_id=group_id, snapshot=snapshot)
    return not bool(diagnostics.get("missing") if isinstance(diagnostics, dict) else []), diagnostics


def repair_control_text(*, text: str, diagnostics: Dict[str, Any]) -> str:
    missing = [
        str(item).strip()
        for item in (diagnostics.get("missing") if isinstance(diagnostics, dict) and isinstance(diagnostics.get("missing"), list) else [])
        if str(item).strip()
    ]
    if not missing:
        return str(text or "")
    legacy_input_missing = "read_new_input" in missing
    lines = [str(text or "").strip(), ""]
    if legacy_input_missing:
        lines.extend(
            [
                "[CCCC] REPAIR HINT: previous legacy pointer turn did not fetch the secretary input.",
                "Call MCP tool cccc_voice_secretary_document(action=\"read_new_input\") before doing other work, then complete every missing action below:",
            ]
        )
    else:
        lines.extend(
            [
                "[CCCC] REPAIR HINT: previous Voice Secretary turn did not complete the required output.",
                "Use the daemon-delivered input_envelope already included in this control turn; use MCP tool cccc_voice_secretary_document(action=\"read_new_input\") only for a legacy pointer with no envelope.",
                "Complete every missing action below before ending the turn:",
            ]
        )
    for item in missing:
        lines.append(f"- {item}")
    return "\n".join(lines).strip()


def retryable_control_failure(diagnostics: Dict[str, Any]) -> bool:
    missing = {
        str(item).strip()
        for item in (
            diagnostics.get("missing")
            if isinstance(diagnostics, dict) and isinstance(diagnostics.get("missing"), list)
            else []
        )
        if str(item).strip()
    }
    return bool(missing) and "read_new_input" not in missing


def control_failure_reason(diagnostics: Dict[str, Any]) -> str:
    missing = {
        str(item).strip()
        for item in (
            diagnostics.get("missing")
            if isinstance(diagnostics, dict) and isinstance(diagnostics.get("missing"), list)
            else []
        )
        if str(item).strip()
    }
    if "read_new_input" in missing:
        return "voice_secretary_input_not_consumed"
    if missing:
        return "voice_secretary_output_not_completed"
    return "voice_secretary_input_not_consumed"


def prepare_repair_retry(*, text: str, diagnostics: Dict[str, Any]) -> tuple[str, Dict[str, Any]]:
    current_diagnostics = diagnostics if isinstance(diagnostics, dict) else {}
    retry_text = repair_control_text(text=text, diagnostics=current_diagnostics)
    return retry_text, current_diagnostics
