from __future__ import annotations

from typing import Any, Awaitable, Callable

_PCM16_BYTES_PER_SAMPLE = 2
_DEFAULT_MAX_SPEAKER_TRANSCRIPT_SEGMENTS = 48
_MIN_TRANSCRIBE_DURATION_MS = 300

SpeakerTranscriber = Callable[[bytes, int], Awaitable[str]]


def _safe_ms(value: Any) -> int | None:
    try:
        parsed = int(value)
    except Exception:
        return None
    return max(0, parsed)


def slice_pcm16_by_ms(pcm16_audio: bytes, *, start_ms: int, end_ms: int, sample_rate: int = 16000) -> bytes:
    if not pcm16_audio or end_ms <= start_ms:
        return b""
    rate = max(1, int(sample_rate or 16000))
    start_sample = max(0, int(start_ms * rate / 1000))
    end_sample = max(start_sample, int(end_ms * rate / 1000))
    start_byte = min(len(pcm16_audio), start_sample * _PCM16_BYTES_PER_SAMPLE)
    end_byte = min(len(pcm16_audio), end_sample * _PCM16_BYTES_PER_SAMPLE)
    return pcm16_audio[start_byte:end_byte]


def normalized_speaker_turns(
    speaker_segments: Any,
    *,
    max_segments: int = _DEFAULT_MAX_SPEAKER_TRANSCRIPT_SEGMENTS,
) -> list[dict[str, Any]]:
    if not isinstance(speaker_segments, list):
        return []
    turns: list[dict[str, Any]] = []
    for item in speaker_segments:
        if not isinstance(item, dict):
            continue
        start_ms = _safe_ms(item.get("start_ms"))
        end_ms = _safe_ms(item.get("end_ms"))
        if start_ms is None or end_ms is None or end_ms <= start_ms:
            continue
        label = str(item.get("speaker_label") or "").strip()
        if not label:
            continue
        turns.append(
            {
                "start_ms": start_ms,
                "end_ms": end_ms,
                "speaker_label": label,
                "speaker_index": item.get("speaker_index"),
            }
        )
    return sorted(turns, key=lambda row: (int(row["start_ms"]), int(row["end_ms"])))[:max_segments]


async def build_speaker_transcript_segments(
    pcm16_audio: bytes,
    speaker_segments: Any,
    *,
    sample_rate: int = 16000,
    transcribe_segment: SpeakerTranscriber,
    max_segments: int = _DEFAULT_MAX_SPEAKER_TRANSCRIPT_SEGMENTS,
) -> list[dict[str, Any]]:
    transcript_segments: list[dict[str, Any]] = []
    for turn in normalized_speaker_turns(speaker_segments, max_segments=max_segments):
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
    return transcript_segments
