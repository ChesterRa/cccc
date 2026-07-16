from __future__ import annotations

from dataclasses import dataclass
import math
import struct

_PCM16_BYTES_PER_SAMPLE = 2


@dataclass(frozen=True)
class VoicePcmSegment:
    start_ms: int
    end_ms: int
    audio: bytes


def _pcm16_duration_ms(byte_count: int, sample_rate: int) -> int:
    rate = max(1, int(sample_rate or 16000))
    return int(max(0, byte_count) / (_PCM16_BYTES_PER_SAMPLE * rate) * 1000)


def _slice_pcm16_by_ms(pcm16_audio: bytes, *, start_ms: int, end_ms: int, sample_rate: int) -> bytes:
    if not pcm16_audio or end_ms <= start_ms:
        return b""
    rate = max(1, int(sample_rate or 16000))
    start_sample = max(0, int(start_ms * rate / 1000))
    end_sample = max(start_sample, int(end_ms * rate / 1000))
    start_byte = min(len(pcm16_audio), start_sample * _PCM16_BYTES_PER_SAMPLE)
    end_byte = min(len(pcm16_audio), end_sample * _PCM16_BYTES_PER_SAMPLE)
    return pcm16_audio[start_byte:end_byte]


def _frame_rms(frame: bytes) -> float:
    sample_count = len(frame) // _PCM16_BYTES_PER_SAMPLE
    if sample_count <= 0:
        return 0.0
    total = 0
    for (sample,) in struct.iter_unpack("<h", frame[: sample_count * _PCM16_BYTES_PER_SAMPLE]):
        total += int(sample) * int(sample)
    return math.sqrt(total / sample_count)


def _adaptive_threshold(rms_values: list[float]) -> float:
    if not rms_values:
        return 0.0
    ordered = sorted(rms_values)
    noise = ordered[min(len(ordered) - 1, max(0, int(len(ordered) * 0.2)))]
    high = ordered[min(len(ordered) - 1, max(0, int(len(ordered) * 0.8)))]
    if high <= noise * 1.5:
        return max(180.0, high * 0.18)
    return max(180.0, noise * 3.0, high * 0.18)


def _split_long_range(start_ms: int, end_ms: int, *, max_segment_ms: int) -> list[tuple[int, int]]:
    max_ms = max(1, int(max_segment_ms or 16000))
    ranges: list[tuple[int, int]] = []
    cursor = start_ms
    while cursor < end_ms:
        next_end = min(end_ms, cursor + max_ms)
        if next_end > cursor:
            ranges.append((cursor, next_end))
        cursor = next_end
    return ranges


def build_pcm16_segments_from_ranges(
    pcm16_audio: bytes,
    ranges: list[dict[str, int]],
    *,
    sample_rate: int = 16000,
    pad_ms: int = 120,
    merge_gap_ms: int = 250,
    max_segment_ms: int = 12000,
) -> list[VoicePcmSegment]:
    if not pcm16_audio or not ranges:
        return []
    rate = max(1, int(sample_rate or 16000))
    audio_duration_ms = _pcm16_duration_ms(len(pcm16_audio), rate)
    normalized: list[tuple[int, int]] = []
    for item in ranges:
        start_ms = max(0, int(item.get("start_ms") or 0) - max(0, int(pad_ms or 0)))
        end_ms = min(audio_duration_ms, int(item.get("end_ms") or 0) + max(0, int(pad_ms or 0)))
        if end_ms > start_ms:
            normalized.append((start_ms, end_ms))
    normalized.sort(key=lambda row: (row[0], row[1]))
    merged: list[tuple[int, int]] = []
    for start_ms, end_ms in normalized:
        if not merged or start_ms - merged[-1][1] > int(merge_gap_ms or 0):
            merged.append((start_ms, end_ms))
            continue
        merged[-1] = (merged[-1][0], max(merged[-1][1], end_ms))

    segments: list[VoicePcmSegment] = []
    for start_ms, end_ms in merged:
        for chunk_start_ms, chunk_end_ms in _split_long_range(start_ms, end_ms, max_segment_ms=max_segment_ms):
            audio = _slice_pcm16_by_ms(
                pcm16_audio,
                start_ms=chunk_start_ms,
                end_ms=chunk_end_ms,
                sample_rate=rate,
            )
            if audio:
                segments.append(VoicePcmSegment(start_ms=chunk_start_ms, end_ms=chunk_end_ms, audio=audio))
    return segments


def split_pcm16_voice_segments(
    pcm16_audio: bytes,
    *,
    sample_rate: int = 16000,
    frame_ms: int = 30,
    min_speech_ms: int = 450,
    min_silence_ms: int = 700,
    merge_gap_ms: int = 500,
    pad_ms: int = 180,
    max_segment_ms: int = 16000,
) -> list[VoicePcmSegment]:
    if not pcm16_audio:
        return []
    rate = max(1, int(sample_rate or 16000))
    frame_duration_ms = max(10, int(frame_ms or 30))
    frame_bytes = max(_PCM16_BYTES_PER_SAMPLE, int(rate * frame_duration_ms / 1000) * _PCM16_BYTES_PER_SAMPLE)
    frame_bytes -= frame_bytes % _PCM16_BYTES_PER_SAMPLE
    if frame_bytes <= 0:
        return []

    frames: list[tuple[int, int, float]] = []
    for offset in range(0, len(pcm16_audio), frame_bytes):
        frame = pcm16_audio[offset : offset + frame_bytes]
        if len(frame) < _PCM16_BYTES_PER_SAMPLE:
            continue
        start_ms = int(offset / (_PCM16_BYTES_PER_SAMPLE * rate) * 1000)
        end_ms = int((offset + len(frame)) / (_PCM16_BYTES_PER_SAMPLE * rate) * 1000)
        frames.append((start_ms, end_ms, _frame_rms(frame)))
    if not frames:
        return []

    threshold = _adaptive_threshold([rms for _, _, rms in frames])
    min_silence = max(frame_duration_ms, int(min_silence_ms or 700))
    raw_ranges: list[tuple[int, int]] = []
    active_start: int | None = None
    last_voice_end = 0

    for start_ms, end_ms, rms in frames:
        if rms >= threshold:
            if active_start is None:
                active_start = start_ms
            last_voice_end = end_ms
            continue
        if active_start is not None and start_ms - last_voice_end >= min_silence:
            if last_voice_end - active_start >= int(min_speech_ms or 450):
                raw_ranges.append((active_start, last_voice_end))
            active_start = None
    if active_start is not None and last_voice_end - active_start >= int(min_speech_ms or 450):
        raw_ranges.append((active_start, last_voice_end))
    if not raw_ranges:
        return []

    audio_duration_ms = _pcm16_duration_ms(len(pcm16_audio), rate)
    padded = [
        (max(0, start_ms - max(0, int(pad_ms or 0))), min(audio_duration_ms, end_ms + max(0, int(pad_ms or 0))))
        for start_ms, end_ms in raw_ranges
    ]
    merged: list[tuple[int, int]] = []
    for start_ms, end_ms in padded:
        if not merged or start_ms - merged[-1][1] > int(merge_gap_ms or 0):
            merged.append((start_ms, end_ms))
            continue
        merged[-1] = (merged[-1][0], max(merged[-1][1], end_ms))

    segments: list[VoicePcmSegment] = []
    for start_ms, end_ms in merged:
        for chunk_start_ms, chunk_end_ms in _split_long_range(start_ms, end_ms, max_segment_ms=max_segment_ms):
            audio = _slice_pcm16_by_ms(
                pcm16_audio,
                start_ms=chunk_start_ms,
                end_ms=chunk_end_ms,
                sample_rate=rate,
            )
            if audio:
                segments.append(VoicePcmSegment(start_ms=chunk_start_ms, end_ms=chunk_end_ms, audio=audio))
    return segments
