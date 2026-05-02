from __future__ import annotations

import math
from typing import Any

from .sherpa_diarization import normalize_diarization_segments, run_sherpa_diarization

_DEFAULT_SPEAKER_EMBEDDING_MATCH_THRESHOLD = 0.62
_MIN_SEGMENT_DURATION_MS = 250
_MERGE_GAP_MS = 350


def _speaker_key(item: dict[str, Any]) -> str:
    key = str(item.get("speaker_index") if item.get("speaker_index") is not None else "").strip()
    if key:
        return key
    return str(item.get("speaker_label") or "").strip()


def _segment_overlap_ms(left: dict[str, Any], right: dict[str, Any]) -> int:
    try:
        left_start = int(left.get("start_ms") or 0)
        left_end = int(left.get("end_ms") or 0)
        right_start = int(right.get("start_ms") or 0)
        right_end = int(right.get("end_ms") or 0)
    except Exception:
        return 0
    return max(0, min(left_end, right_end) - max(left_start, right_start))


def _next_global_speaker_index(previous_segments: list[dict[str, Any]]) -> int:
    max_index = -1
    for item in previous_segments:
        try:
            max_index = max(max_index, int(item.get("speaker_index")))
        except Exception:
            continue
    return max_index + 1


def _merge_stable_segments(segments: list[dict[str, Any]]) -> list[dict[str, Any]]:
    ordered = sorted(
        (
            dict(item)
            for item in segments
            if int(item.get("end_ms") or 0) - int(item.get("start_ms") or 0) >= _MIN_SEGMENT_DURATION_MS
        ),
        key=lambda row: (int(row.get("start_ms") or 0), int(row.get("end_ms") or 0)),
    )
    merged: list[dict[str, Any]] = []
    for item in ordered:
        previous = merged[-1] if merged else None
        if (
            previous is not None
            and previous.get("speaker_index") == item.get("speaker_index")
            and int(item["start_ms"]) - int(previous["end_ms"]) <= _MERGE_GAP_MS
        ):
            previous["end_ms"] = max(int(previous["end_ms"]), int(item["end_ms"]))
            continue
        merged.append(dict(item))
    return merged


def _safe_embedding(value: Any) -> list[float]:
    if not isinstance(value, list):
        return []
    out: list[float] = []
    for item in value:
        try:
            out.append(float(item))
        except Exception:
            return []
    return out


def _cosine_similarity(left: list[float], right: list[float]) -> float:
    if not left or len(left) != len(right):
        return -1.0
    dot = 0.0
    left_norm = 0.0
    right_norm = 0.0
    for l_value, r_value in zip(left, right):
        dot += l_value * r_value
        left_norm += l_value * l_value
        right_norm += r_value * r_value
    denom = math.sqrt(left_norm) * math.sqrt(right_norm)
    return dot / denom if denom > 0 else -1.0


def _speaker_embeddings_by_key(speaker_embeddings: Any) -> dict[str, list[float]]:
    if not isinstance(speaker_embeddings, list):
        return {}
    out: dict[str, list[float]] = {}
    for item in speaker_embeddings:
        if not isinstance(item, dict):
            continue
        key = _speaker_key(item)
        embedding = _safe_embedding(item.get("embedding"))
        if key and embedding:
            out[key] = embedding
    return out


def _speaker_embeddings_by_index(speaker_embeddings: Any) -> dict[int, list[float]]:
    if not isinstance(speaker_embeddings, list):
        return {}
    out: dict[int, list[float]] = {}
    for item in speaker_embeddings:
        if not isinstance(item, dict):
            continue
        try:
            speaker_index = int(item.get("speaker_index"))
        except Exception:
            continue
        embedding = _safe_embedding(item.get("embedding"))
        if embedding:
            out[speaker_index] = embedding
    return out


def _assign_by_embedding(
    *,
    local_keys: list[str],
    current_embeddings: dict[str, list[float]],
    previous_embeddings: dict[int, list[float]],
    threshold: float,
) -> tuple[dict[str, int], set[int]]:
    scored: list[tuple[float, str, int]] = []
    for local_key in local_keys:
        current = current_embeddings.get(local_key)
        if not current:
            continue
        for previous_index, previous in previous_embeddings.items():
            score = _cosine_similarity(current, previous)
            if score >= threshold:
                scored.append((score, local_key, previous_index))
    scored.sort(reverse=True, key=lambda row: row[0])

    assignments: dict[str, int] = {}
    claimed_previous: set[int] = set()
    for _score, local_key, previous_index in scored:
        if local_key in assignments or previous_index in claimed_previous:
            continue
        assignments[local_key] = previous_index
        claimed_previous.add(previous_index)
    return assignments, claimed_previous


def _speaker_index_assignments(
    segments: Any,
    previous_segments: Any,
    *,
    speaker_embeddings: Any = None,
    previous_speaker_embeddings: Any = None,
    embedding_match_threshold: float = _DEFAULT_SPEAKER_EMBEDDING_MATCH_THRESHOLD,
) -> dict[str, int]:
    if not isinstance(segments, list):
        return {}
    current = [dict(item) for item in segments if isinstance(item, dict)]
    previous = [dict(item) for item in previous_segments if isinstance(item, dict)] if isinstance(previous_segments, list) else []
    if not current:
        return {}
    if not previous:
        normalized = normalize_diarization_segments(current)
        return {_speaker_key(item): int(item["speaker_index"]) for item in normalized if _speaker_key(item)}

    overlap_by_pair: dict[tuple[str, int], int] = {}
    for item in current:
        local_key = _speaker_key(item)
        if not local_key:
            continue
        for prev in previous:
            try:
                prev_index = int(prev.get("speaker_index"))
            except Exception:
                continue
            overlap = _segment_overlap_ms(item, prev)
            if overlap <= 0:
                continue
            pair = (local_key, prev_index)
            overlap_by_pair[pair] = overlap_by_pair.get(pair, 0) + overlap

    local_keys = []
    for item in current:
        key = _speaker_key(item)
        if key and key not in local_keys:
            local_keys.append(key)

    current_embeddings = _speaker_embeddings_by_key(speaker_embeddings)
    previous_embeddings = _speaker_embeddings_by_index(previous_speaker_embeddings)
    assignments, claimed_previous = _assign_by_embedding(
        local_keys=local_keys,
        current_embeddings=current_embeddings,
        previous_embeddings=previous_embeddings,
        threshold=float(embedding_match_threshold),
    )

    ranked_pairs = sorted(overlap_by_pair.items(), key=lambda row: row[1], reverse=True)
    for (local_key, prev_index), _overlap in ranked_pairs:
        if local_key in assignments or prev_index in claimed_previous:
            continue
        assignments[local_key] = prev_index
        claimed_previous.add(prev_index)

    next_index = _next_global_speaker_index(previous)
    for local_key in local_keys:
        if local_key in assignments:
            continue
        while next_index in claimed_previous:
            next_index += 1
        assignments[local_key] = next_index
        claimed_previous.add(next_index)
        next_index += 1
    return assignments


def stabilize_diarization_speaker_identity(
    segments: Any,
    previous_segments: Any,
    *,
    speaker_embeddings: Any = None,
    previous_speaker_embeddings: Any = None,
    embedding_match_threshold: float = _DEFAULT_SPEAKER_EMBEDDING_MATCH_THRESHOLD,
) -> list[dict[str, Any]]:
    """Keep provisional speaker ids stable across repeated full-prefix diarization runs."""
    if not isinstance(segments, list):
        return []
    current = [dict(item) for item in segments if isinstance(item, dict)]
    assignments = _speaker_index_assignments(
        current,
        previous_segments,
        speaker_embeddings=speaker_embeddings,
        previous_speaker_embeddings=previous_speaker_embeddings,
        embedding_match_threshold=embedding_match_threshold,
    )

    remapped: list[dict[str, Any]] = []
    for item in current:
        local_key = _speaker_key(item)
        speaker_index = assignments.get(local_key)
        if speaker_index is None:
            continue
        remapped.append(
            {
                **item,
                "speaker_index": speaker_index,
                "speaker_label": f"Speaker {speaker_index + 1}",
            }
        )
    return _merge_stable_segments(remapped)


def diarization_result_segments(result: dict[str, Any]) -> list[dict[str, Any]]:
    return [dict(item) for item in result.get("segments", []) if isinstance(item, dict)]


def diarization_result_speaker_embeddings(result: dict[str, Any]) -> list[dict[str, Any]]:
    return [dict(item) for item in result.get("speaker_embeddings", []) if isinstance(item, dict)]


def remap_speaker_embeddings(
    speaker_embeddings: Any,
    assignments: dict[str, int],
) -> list[dict[str, Any]]:
    if not isinstance(speaker_embeddings, list):
        return []
    remapped: list[dict[str, Any]] = []
    for item in speaker_embeddings:
        if not isinstance(item, dict):
            continue
        local_key = _speaker_key(item)
        speaker_index = assignments.get(local_key)
        if speaker_index is None:
            continue
        remapped.append(
            {
                **item,
                "speaker_index": speaker_index,
                "speaker_label": f"Speaker {speaker_index + 1}",
            }
        )
    return remapped


async def run_provisional_diarization_prefix(
    pcm16_audio: bytes,
    *,
    selected_model_id: str,
    sample_rate: int,
    run_seq: int,
    audio_duration_ms: int,
    previous_segments: list[dict[str, Any]],
    previous_speaker_embeddings: list[dict[str, Any]] | None = None,
) -> dict[str, Any]:
    result = await run_sherpa_diarization(
        pcm16_audio,
        selected_model_id=selected_model_id,
        sample_rate=sample_rate,
        include_speaker_embeddings=True,
    )
    assignments = _speaker_index_assignments(
        result.get("segments"),
        previous_segments,
        speaker_embeddings=result.get("speaker_embeddings"),
        previous_speaker_embeddings=previous_speaker_embeddings or [],
    )
    return {
        **result,
        "segments": stabilize_diarization_speaker_identity(
            result.get("segments"),
            previous_segments,
            speaker_embeddings=result.get("speaker_embeddings"),
            previous_speaker_embeddings=previous_speaker_embeddings or [],
        ),
        "speaker_embeddings": remap_speaker_embeddings(result.get("speaker_embeddings"), assignments),
        "run_seq": run_seq,
        "audio_duration_ms": audio_duration_ms,
        "provisional": True,
    }
