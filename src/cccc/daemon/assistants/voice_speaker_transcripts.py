from __future__ import annotations

from typing import Any, Awaitable, Callable

from .local_streaming_asr import open_local_offline_asr_session
from .voice_speaker_transcript_windows import (
    merge_adjacent_speaker_turns,
    merge_display_transcript_segments,
    normalized_speaker_turns,
    slice_pcm16_by_ms,
)

_DEFAULT_MAX_SPEAKER_TRANSCRIPT_SEGMENTS = 48
_MIN_TRANSCRIBE_DURATION_MS = 300

SpeakerTranscriber = Callable[[bytes, int], Awaitable[str]]


async def build_speaker_transcript_segments(
    pcm16_audio: bytes,
    speaker_segments: Any,
    *,
    sample_rate: int = 16000,
    transcribe_segment: SpeakerTranscriber,
    max_segments: int = _DEFAULT_MAX_SPEAKER_TRANSCRIPT_SEGMENTS,
) -> list[dict[str, Any]]:
    transcript_segments: list[dict[str, Any]] = []
    normalized_turns = normalized_speaker_turns(speaker_segments, max_segments=max_segments)
    for turn in merge_adjacent_speaker_turns(normalized_turns):
        start_ms = int(turn["start_ms"])
        end_ms = int(turn["end_ms"])
        if end_ms - start_ms < _MIN_TRANSCRIBE_DURATION_MS:
            continue
        audio = slice_pcm16_by_ms(
            pcm16_audio,
            start_ms=start_ms,
            end_ms=end_ms,
            sample_rate=sample_rate,
        )
        text = (await transcribe_segment(audio, int(sample_rate or 16000))).strip()
        if not text:
            continue
        transcript_segments.append(
            {
                "start_ms": start_ms,
                "end_ms": end_ms,
                "speaker_label": str(turn["speaker_label"]),
                "speaker_index": turn.get("speaker_index"),
                "text": text,
            }
        )
    return merge_display_transcript_segments(transcript_segments)


async def build_offline_speaker_transcript_segments(
    pcm16_audio: bytes,
    speaker_segments: Any,
    *,
    selected_model_id: str,
    sample_rate: int = 16000,
    max_segments: int = _DEFAULT_MAX_SPEAKER_TRANSCRIPT_SEGMENTS,
) -> list[dict[str, Any]]:
    session = await open_local_offline_asr_session(selected_model_id, sample_rate=sample_rate)
    try:
        return await build_speaker_transcript_segments(
            pcm16_audio,
            speaker_segments,
            sample_rate=sample_rate,
            max_segments=max_segments,
            transcribe_segment=lambda audio, rate: session.transcribe_pcm16(audio, sample_rate=rate),
        )
    finally:
        await session.close()
