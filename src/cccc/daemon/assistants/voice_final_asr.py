from __future__ import annotations

from collections.abc import AsyncIterator
from dataclasses import dataclass
from typing import Any

from .local_streaming_asr import LocalStreamingAsrError, open_local_offline_asr_session, transcribe_local_streaming_pcm16
from .sherpa_vad_segments import SherpaVadSegmentError, detect_sherpa_vad_segments
from .sense_voice_text import clean_sense_voice_text
from .voice_pcm_segments import VoicePcmSegment, build_pcm16_segments_from_ranges, split_pcm16_voice_segments
from .voice_final_asr_debug import voice_final_asr_quality_flags
from .voice_models import get_voice_model_status, resolve_installed_voice_model_offline_config


_FINAL_ASR_SOURCE = "assistant_service_local_asr_final"
_FINAL_ASR_PAD_MS = 240
_FINAL_ASR_MERGE_GAP_MS = 1200
_FINAL_ASR_MAX_SEGMENT_MS = 60000


@dataclass(frozen=True)
class FinalAsrEvent:
    payload: dict[str, Any]
    text: str = ""


def _pcm16_duration_ms(byte_count: int, sample_rate: int) -> int:
    rate = max(1, int(sample_rate or 16000))
    return int(max(0, byte_count) / (2 * rate) * 1000)


def _base_meta(selected_model_id: str, sample_rate: int) -> dict[str, Any]:
    status = get_voice_model_status(selected_model_id)
    offline = status.get("offline") if isinstance(status.get("offline"), dict) else {}
    streaming = status.get("streaming") if isinstance(status.get("streaming"), dict) else {}
    config: dict[str, Any] = {}
    if offline:
        try:
            config = resolve_installed_voice_model_offline_config(selected_model_id)
        except Exception:
            config = {}
    return {
        "model_id": str(selected_model_id or "").strip(),
        "engine": str(config.get("engine") or offline.get("engine") or streaming.get("engine") or "").strip(),
        "language": str(config.get("language") or offline.get("language") or "auto").strip() or "auto",
        "sample_rate": int(config.get("sample_rate") or offline.get("sample_rate") or streaming.get("sample_rate") or sample_rate or 16000),
        "model_ready": bool(status.get("offline_ready") or status.get("streaming_ready")),
    }


def _progress(
    stage: str,
    *,
    seq: Any,
    selected_model_id: str,
    sample_rate: int,
    segment_count: int = 0,
    segment: VoicePcmSegment | None = None,
    index: int = 0,
    fallback_reason: str = "",
    error: LocalStreamingAsrError | None = None,
) -> FinalAsrEvent:
    payload: dict[str, Any] = {
        "type": "final_asr_progress",
        "ok": True,
        "seq": seq,
        "stage": stage,
        "source": _FINAL_ASR_SOURCE,
        **_base_meta(selected_model_id, sample_rate),
    }
    if fallback_reason:
        payload["fallback_reason"] = fallback_reason
    if error is not None:
        payload["error"] = {"code": error.code, "message": error.message, "details": error.details}
    if segment_count:
        payload["segment_count"] = segment_count
    if index:
        payload["segment_index"] = index
    if segment is not None:
        payload["start_ms"] = segment.start_ms
        payload["end_ms"] = segment.end_ms
    return FinalAsrEvent(payload)


def _final(text: str, *, seq: Any, selected_model_id: str, sample_rate: int, segment: VoicePcmSegment) -> FinalAsrEvent:
    text = clean_sense_voice_text(text)
    return FinalAsrEvent(
        {
            "type": "final",
            "ok": True,
            "seq": seq,
            "text": text,
            "start_ms": segment.start_ms,
            "end_ms": segment.end_ms,
            "source": _FINAL_ASR_SOURCE,
            **_base_meta(selected_model_id, sample_rate),
            "quality_flags": voice_final_asr_quality_flags(text),
        },
        text=text,
    )


async def iter_final_asr_events(
    pcm16_audio: bytes,
    *,
    selected_model_id: str,
    sample_rate: int = 16000,
    seq: Any = None,
) -> AsyncIterator[FinalAsrEvent]:
    yield FinalAsrEvent({"type": "final_asr_started", "ok": True, "seq": seq, "source": _FINAL_ASR_SOURCE, **_base_meta(selected_model_id, sample_rate)})
    try:
        try:
            vad_ranges = await detect_sherpa_vad_segments(pcm16_audio, sample_rate=sample_rate)
        except SherpaVadSegmentError:
            vad_ranges = []
            yield _progress("vad_fallback", seq=seq, selected_model_id=selected_model_id, sample_rate=sample_rate, fallback_reason="vad_failed")

        segments = build_pcm16_segments_from_ranges(
            pcm16_audio,
            vad_ranges,
            sample_rate=sample_rate,
            pad_ms=_FINAL_ASR_PAD_MS,
            merge_gap_ms=_FINAL_ASR_MERGE_GAP_MS,
            max_segment_ms=_FINAL_ASR_MAX_SEGMENT_MS,
        )
        if not segments:
            segments = split_pcm16_voice_segments(
                pcm16_audio,
                sample_rate=sample_rate,
                merge_gap_ms=_FINAL_ASR_MERGE_GAP_MS,
                pad_ms=_FINAL_ASR_PAD_MS,
                max_segment_ms=_FINAL_ASR_MAX_SEGMENT_MS,
            )
        if not segments and pcm16_audio:
            segments = [VoicePcmSegment(start_ms=0, end_ms=_pcm16_duration_ms(len(pcm16_audio), sample_rate), audio=pcm16_audio)]

        yield _progress("segments_ready", seq=seq, selected_model_id=selected_model_id, sample_rate=sample_rate, segment_count=len(segments))
        if not segments:
            return

        yield _progress("model_loading", seq=seq, selected_model_id=selected_model_id, sample_rate=sample_rate, segment_count=len(segments))
        try:
            offline_session = await open_local_offline_asr_session(selected_model_id, sample_rate=sample_rate)
        except LocalStreamingAsrError as exc:
            offline_session = None
            yield _progress(
                "offline_session_unavailable",
                seq=seq,
                selected_model_id=selected_model_id,
                sample_rate=sample_rate,
                segment_count=len(segments),
                fallback_reason="offline_session_unavailable",
                error=exc,
            )

        if offline_session is not None:
            offline_transcribe_failed = False
            try:
                for index, segment in enumerate(segments, start=1):
                    yield _progress("transcribing", seq=seq, selected_model_id=selected_model_id, sample_rate=sample_rate, segment_count=len(segments), segment=segment, index=index)
                    segment_text = (await offline_session.transcribe_pcm16(segment.audio, sample_rate=sample_rate)).strip()
                    if segment_text:
                        yield _final(segment_text, seq=seq, selected_model_id=selected_model_id, sample_rate=sample_rate, segment=segment)
            except LocalStreamingAsrError as exc:
                offline_transcribe_failed = True
                yield _progress(
                    "offline_transcribe_failed",
                    seq=seq,
                    selected_model_id=selected_model_id,
                    sample_rate=sample_rate,
                    segment_count=len(segments),
                    fallback_reason="offline_transcribe_failed",
                    error=exc,
                )
            finally:
                await offline_session.close()
            if not offline_transcribe_failed:
                return

        yield _progress("legacy_fallback", seq=seq, selected_model_id=selected_model_id, sample_rate=sample_rate, segment_count=len(segments), fallback_reason="offline_session_unavailable")
        for index, segment in enumerate(segments, start=1):
            yield _progress("transcribing", seq=seq, selected_model_id=selected_model_id, sample_rate=sample_rate, segment_count=len(segments), segment=segment, index=index)
            segment_text = (
                await transcribe_local_streaming_pcm16(
                    segment.audio,
                    selected_model_id=selected_model_id,
                    sample_rate=sample_rate,
                )
            ).strip()
            if segment_text:
                yield _final(segment_text, seq=seq, selected_model_id=selected_model_id, sample_rate=sample_rate, segment=segment)
    except LocalStreamingAsrError as exc:
        yield FinalAsrEvent(
            {
                "type": "final_asr_failed",
                "ok": False,
                "seq": seq,
                "source": _FINAL_ASR_SOURCE,
                **_base_meta(selected_model_id, sample_rate),
                "fallback_reason": "local_asr_error",
                "error": {"code": exc.code, "message": exc.message, "details": exc.details},
            }
        )
