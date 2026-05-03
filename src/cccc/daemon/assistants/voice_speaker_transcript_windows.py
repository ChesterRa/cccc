from __future__ import annotations

from typing import Any

from .sense_voice_text import clean_sense_voice_text

_PCM16_BYTES_PER_SAMPLE = 2
_DEFAULT_MAX_SPEAKER_TRANSCRIPT_SEGMENTS = 48
_MERGE_SAME_SPEAKER_GAP_MS = 1800
_MERGE_MAX_TURN_DURATION_MS = 30_000
_DISPLAY_MERGE_GAP_MS = 2400
_DISPLAY_MAX_TURN_DURATION_MS = 45_000


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


def merge_adjacent_speaker_turns(
    speaker_turns: list[dict[str, Any]],
    *,
    max_gap_ms: int = _MERGE_SAME_SPEAKER_GAP_MS,
    max_duration_ms: int = _MERGE_MAX_TURN_DURATION_MS,
) -> list[dict[str, Any]]:
    merged: list[dict[str, Any]] = []
    for turn in speaker_turns:
        if not merged:
            merged.append(dict(turn))
            continue
        previous = merged[-1]
        turn_start_ms = int(turn["start_ms"])
        previous_end_ms = int(previous["end_ms"])
        if turn_start_ms < previous_end_ms:
            merged.append(dict(turn))
            continue
        same_speaker = (
            str(previous.get("speaker_label") or "") == str(turn.get("speaker_label") or "")
            and previous.get("speaker_index") == turn.get("speaker_index")
        )
        gap_ms = turn_start_ms - previous_end_ms
        merged_duration_ms = int(turn["end_ms"]) - int(previous["start_ms"])
        if same_speaker and 0 <= gap_ms <= max_gap_ms and merged_duration_ms <= max_duration_ms:
            previous["end_ms"] = int(turn["end_ms"])
            continue
        merged.append(dict(turn))
    return merged


def merge_display_transcript_segments(
    transcript_segments: list[dict[str, Any]],
    *,
    max_gap_ms: int = _DISPLAY_MERGE_GAP_MS,
    max_duration_ms: int = _DISPLAY_MAX_TURN_DURATION_MS,
) -> list[dict[str, Any]]:
    merged: list[dict[str, Any]] = []
    for segment in transcript_segments:
        text = clean_sense_voice_text(str(segment.get("text") or ""))
        if not text:
            continue
        item = {**segment, "text": text}
        if not merged:
            merged.append(item)
            continue
        previous = merged[-1]
        item_start_ms = int(item.get("start_ms") or 0)
        previous_end_ms = int(previous.get("end_ms") or 0)
        if item_start_ms < previous_end_ms:
            merged.append(item)
            continue
        same_speaker = (
            str(previous.get("speaker_label") or "") == str(item.get("speaker_label") or "")
            and previous.get("speaker_index") == item.get("speaker_index")
        )
        gap_ms = item_start_ms - previous_end_ms
        merged_duration_ms = int(item.get("end_ms") or 0) - int(previous.get("start_ms") or 0)
        if same_speaker and 0 <= gap_ms <= max_gap_ms and merged_duration_ms <= max_duration_ms:
            previous["end_ms"] = int(item.get("end_ms") or previous.get("end_ms") or 0)
            previous["text"] = _join_transcript_text(str(previous.get("text") or ""), text)
            continue
        merged.append(item)
    return merged


def _join_transcript_text(previous: str, next_text: str) -> str:
    left = clean_sense_voice_text(previous)
    right = clean_sense_voice_text(next_text)
    if not left:
        return right
    if not right:
        return left
    if left.endswith(right):
        return left
    if right.startswith(left):
        return right
    cjk_boundary = _ends_with_cjk(left) and _starts_with_cjk(right)
    punctuation_boundary = left[-1:] in ".!?;:，。！？；：、"
    return f"{left}{right}" if cjk_boundary or punctuation_boundary else f"{left} {right}"


def _ends_with_cjk(value: str) -> bool:
    if not value:
        return False
    char = value[-1]
    return "\u3040" <= char <= "\u30ff" or "\u3400" <= char <= "\u9fff"


def _starts_with_cjk(value: str) -> bool:
    if not value:
        return False
    char = value[0]
    return "\u3040" <= char <= "\u30ff" or "\u3400" <= char <= "\u9fff"
