from __future__ import annotations

import re
from typing import Any, Awaitable, Callable, Dict

DaemonCaller = Callable[[Dict[str, Any]], Awaitable[Dict[str, Any]]]

_LOW_VALUE_VOICE_CJK_CHARS = {"嗯", "呃", "啊", "哦"}
_LOW_VALUE_VOICE_WORDS = {
    "a",
    "ah",
    "an",
    "i",
    "l",
    "no",
    "nope",
    "oh",
    "ok",
    "okay",
    "the",
    "uh",
    "um",
    "yeah",
    "yep",
    "yes",
}


def _clean_text(value: Any) -> str:
    return re.sub(r"\s+", " ", str(value or "").strip())


def is_meaningful_voice_text(value: Any) -> bool:
    text = _clean_text(value)
    if not text:
        return False
    text = re.sub(r"^Speaker\s*\?:\s*", "", text, flags=re.IGNORECASE)
    cjk_text = re.sub(r"[^\u3040-\u30ff\u3400-\u9fff\uac00-\ud7af]+", "", text)
    if len(cjk_text) >= 2 and any(char not in _LOW_VALUE_VOICE_CJK_CHARS for char in cjk_text):
        return True
    tokens = [
        token
        for token in re.findall(r"[0-9A-Za-z]+", text.lower())
        if token not in _LOW_VALUE_VOICE_WORDS
    ]
    if not tokens:
        return False
    return any(any(char.isdigit() for char in token) or len(token) >= 2 for token in tokens)


def final_speaker_transcript_text(speaker_transcript_segments: Any) -> str:
    if not isinstance(speaker_transcript_segments, list):
        return ""
    lines: list[str] = []
    for segment in speaker_transcript_segments:
        if not isinstance(segment, dict):
            continue
        text = _clean_text(segment.get("text"))
        if not text:
            continue
        speaker_label = _clean_text(segment.get("speaker_label"))
        lines.append(f"{speaker_label}: {text}" if speaker_label else text)
    return "\n".join(lines).strip()


def build_final_transcript_append_request(
    *,
    group_id: str,
    session_id: str,
    document_path: str,
    speaker_transcript_segments: Any,
    sample_rate: int,
    language: str = "",
    final_asr_model_id: str = "",
    audio_duration_ms: int = 0,
    by: str = "user",
) -> Dict[str, Any] | None:
    clean_group_id = str(group_id or "").strip()
    clean_session_id = str(session_id or "").strip()
    clean_document_path = str(document_path or "").strip()
    text = final_speaker_transcript_text(speaker_transcript_segments)
    if not clean_group_id or not clean_session_id or not clean_document_path or not text:
        return None

    start_ms = None
    end_ms = None
    if isinstance(speaker_transcript_segments, list):
        starts = [
            int(segment.get("start_ms"))
            for segment in speaker_transcript_segments
            if isinstance(segment, dict) and str(segment.get("start_ms") or "").strip().lstrip("-").isdigit()
        ]
        ends = [
            int(segment.get("end_ms"))
            for segment in speaker_transcript_segments
            if isinstance(segment, dict) and str(segment.get("end_ms") or "").strip().lstrip("-").isdigit()
        ]
        if starts:
            start_ms = min(starts)
        if ends:
            end_ms = max(ends)
    if end_ms is None and int(audio_duration_ms or 0) > 0:
        end_ms = int(audio_duration_ms)

    args: Dict[str, Any] = {
        "group_id": clean_group_id,
        "session_id": clean_session_id,
        "segment_id": f"final-{clean_session_id}",
        "document_path": clean_document_path,
        "text": text,
        "language": str(language or "").strip(),
        "is_final": True,
        "flush": True,
        "trigger": {
            "mode": "meeting",
            "capture_mode": "service",
            "trigger_kind": "service_transcript",
            "recognition_backend": "assistant_service_local_asr_final",
            "client_session_id": clean_session_id,
            "final_asr_model_id": str(final_asr_model_id or "").strip(),
            "sample_rate": int(sample_rate or 16000),
            "speaker_segment_count": len(speaker_transcript_segments) if isinstance(speaker_transcript_segments, list) else 0,
        },
        "by": str(by or "user").strip() or "user",
    }
    if start_ms is not None:
        args["start_ms"] = start_ms
    if end_ms is not None:
        args["end_ms"] = end_ms
    return {"op": "assistant_voice_transcript_append", "args": args}


def build_final_text_transcript_append_request(
    *,
    group_id: str,
    session_id: str,
    document_path: str,
    text: str,
    sample_rate: int,
    language: str = "",
    final_asr_model_id: str = "",
    audio_duration_ms: int = 0,
    by: str = "user",
) -> Dict[str, Any] | None:
    clean_group_id = str(group_id or "").strip()
    clean_session_id = str(session_id or "").strip()
    clean_document_path = str(document_path or "").strip()
    clean_text = _clean_text(text)
    if not clean_group_id or not clean_session_id or not clean_document_path or not is_meaningful_voice_text(clean_text):
        return None

    args: Dict[str, Any] = {
        "group_id": clean_group_id,
        "session_id": clean_session_id,
        "segment_id": f"final-{clean_session_id}",
        "document_path": clean_document_path,
        "text": clean_text,
        "language": str(language or "").strip(),
        "is_final": True,
        "flush": True,
        "trigger": {
            "mode": "meeting",
            "capture_mode": "service",
            "trigger_kind": "service_transcript",
            "recognition_backend": "assistant_service_local_asr_final",
            "client_session_id": clean_session_id,
            "final_asr_model_id": str(final_asr_model_id or "").strip(),
            "sample_rate": int(sample_rate or 16000),
            "speaker_segment_count": 0,
        },
        "by": str(by or "user").strip() or "user",
    }
    if int(audio_duration_ms or 0) > 0:
        args["start_ms"] = 0
        args["end_ms"] = int(audio_duration_ms or 0)
    return {"op": "assistant_voice_transcript_append", "args": args}


async def apply_final_speaker_transcript_to_document(
    daemon: DaemonCaller,
    *,
    group_id: str,
    session_id: str,
    document_path: str,
    speaker_transcript_segments: Any,
    sample_rate: int,
    language: str = "",
    final_asr_model_id: str = "",
    audio_duration_ms: int = 0,
    by: str = "user",
) -> Dict[str, Any]:
    request = build_final_transcript_append_request(
        group_id=group_id,
        session_id=session_id,
        document_path=document_path,
        speaker_transcript_segments=speaker_transcript_segments,
        sample_rate=sample_rate,
        language=language,
        final_asr_model_id=final_asr_model_id,
        audio_duration_ms=audio_duration_ms,
        by=by,
    )
    if request is None:
        return {"ok": True, "result": {"applied": False, "reason": "empty_final_transcript"}}
    response = await daemon(request)
    if not bool(response.get("ok")):
        return {"ok": False, "error": response.get("error") or {}, "result": {"applied": False}}
    return {"ok": True, "result": {"applied": True, "daemon_result": response.get("result") or {}}}


async def apply_final_text_to_document(
    daemon: DaemonCaller,
    *,
    group_id: str,
    session_id: str,
    document_path: str,
    text: str,
    sample_rate: int,
    language: str = "",
    final_asr_model_id: str = "",
    audio_duration_ms: int = 0,
    by: str = "user",
) -> Dict[str, Any]:
    request = build_final_text_transcript_append_request(
        group_id=group_id,
        session_id=session_id,
        document_path=document_path,
        text=text,
        sample_rate=sample_rate,
        language=language,
        final_asr_model_id=final_asr_model_id,
        audio_duration_ms=audio_duration_ms,
        by=by,
    )
    if request is None:
        return {"ok": True, "result": {"applied": False, "reason": "empty_final_transcript"}}
    response = await daemon(request)
    if not bool(response.get("ok")):
        return {"ok": False, "error": response.get("error") or {}, "result": {"applied": False}}
    return {"ok": True, "result": {"applied": True, "daemon_result": response.get("result") or {}}}
