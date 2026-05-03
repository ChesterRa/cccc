from __future__ import annotations

import math
from pathlib import Path
from typing import Any

from .sherpa_diarization import run_sherpa_diarization, run_sherpa_diarization_file

_DEFAULT_SPEAKER_EMBEDDING_MATCH_THRESHOLD = 0.62
_LOCAL_SPEAKER_MERGE_THRESHOLD = 0.82
_SOFT_EXISTING_SPEAKER_MATCH_THRESHOLD = 0.50
_MIN_NEW_SPEAKER_DURATION_MS = 6_000
_MAX_LOCAL_MERGE_OVERLAP_MS = 250
_MIN_SEGMENT_DURATION_MS = 250
_MERGE_GAP_MS = 350
_ROLLING_REPLACE_OVERLAP_MS = 1_500


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
    used_indexes: set[int] = set()
    for item in previous_segments:
        try:
            speaker_index = int(item.get("speaker_index"))
        except Exception:
            continue
        if speaker_index >= 0:
            used_indexes.add(speaker_index)
    next_index = 0
    while next_index in used_indexes:
        next_index += 1
    return next_index


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


def _shift_segments(segments: Any, offset_ms: int) -> list[dict[str, Any]]:
    if not isinstance(segments, list):
        return []
    shifted: list[dict[str, Any]] = []
    for item in segments:
        if not isinstance(item, dict):
            continue
        try:
            start_ms = max(0, int(item.get("start_ms") or 0) + int(offset_ms))
            end_ms = max(0, int(item.get("end_ms") or 0) + int(offset_ms))
        except Exception:
            continue
        if end_ms <= start_ms:
            continue
        shifted.append({**item, "start_ms": start_ms, "end_ms": end_ms})
    return shifted


def _merge_rolling_segments(
    previous_segments: list[dict[str, Any]],
    current_segments: list[dict[str, Any]],
    *,
    window_start_ms: int,
) -> list[dict[str, Any]]:
    replace_from_ms = max(0, int(window_start_ms) - _ROLLING_REPLACE_OVERLAP_MS)
    retained: list[dict[str, Any]] = []
    for item in previous_segments:
        try:
            start_ms = int(item.get("start_ms") or 0)
            end_ms = int(item.get("end_ms") or 0)
        except Exception:
            continue
        if end_ms <= replace_from_ms:
            retained.append(dict(item))
            continue
        if start_ms < replace_from_ms < end_ms:
            retained.append({**item, "end_ms": replace_from_ms})
    return _merge_stable_segments([*retained, *current_segments])


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


def _speaker_durations_by_key(segments: list[dict[str, Any]]) -> dict[str, int]:
    durations: dict[str, int] = {}
    for item in segments:
        key = _speaker_key(item)
        if not key:
            continue
        try:
            duration_ms = max(0, int(item.get("end_ms") or 0) - int(item.get("start_ms") or 0))
        except Exception:
            continue
        durations[key] = durations.get(key, 0) + duration_ms
    return durations


def _speaker_pair_overlap_ms(segments: list[dict[str, Any]], left_key: str, right_key: str) -> int:
    left_segments = [item for item in segments if _speaker_key(item) == left_key]
    right_segments = [item for item in segments if _speaker_key(item) == right_key]
    overlap = 0
    for left in left_segments:
        for right in right_segments:
            overlap += _segment_overlap_ms(left, right)
            if overlap > _MAX_LOCAL_MERGE_OVERLAP_MS:
                return overlap
    return overlap


def _normalize_embedding(values: list[float]) -> list[float]:
    norm = math.sqrt(sum(value * value for value in values))
    return [value / norm for value in values] if norm > 0 else []


def _local_speaker_aliases(
    *,
    local_keys: list[str],
    segments: list[dict[str, Any]],
    current_embeddings: dict[str, list[float]],
    durations: dict[str, int],
    threshold: float = _LOCAL_SPEAKER_MERGE_THRESHOLD,
) -> dict[str, str]:
    aliases = {key: key for key in local_keys}
    if len(local_keys) < 2 or not current_embeddings:
        return aliases

    parent = {key: key for key in local_keys}

    def find(key: str) -> str:
        root = key
        while parent[root] != root:
            root = parent[root]
        while parent[key] != key:
            next_key = parent[key]
            parent[key] = root
            key = next_key
        return root

    def union(left: str, right: str) -> None:
        left_root = find(left)
        right_root = find(right)
        if left_root == right_root:
            return
        left_duration = durations.get(left_root, 0)
        right_duration = durations.get(right_root, 0)
        if right_duration > left_duration or (right_duration == left_duration and right_root < left_root):
            parent[left_root] = right_root
        else:
            parent[right_root] = left_root

    scored: list[tuple[float, str, str]] = []
    for left_index, left_key in enumerate(local_keys):
        left_embedding = current_embeddings.get(left_key)
        if not left_embedding:
            continue
        for right_key in local_keys[left_index + 1 :]:
            right_embedding = current_embeddings.get(right_key)
            if not right_embedding:
                continue
            score = _cosine_similarity(left_embedding, right_embedding)
            if score >= threshold:
                scored.append((score, left_key, right_key))
    scored.sort(reverse=True, key=lambda row: row[0])

    for _score, left_key, right_key in scored:
        if _speaker_pair_overlap_ms(segments, left_key, right_key) > _MAX_LOCAL_MERGE_OVERLAP_MS:
            continue
        union(left_key, right_key)

    return {key: find(key) for key in local_keys}


def _local_speaker_merge_debug(
    *,
    local_keys: list[str],
    aliases: dict[str, str],
    current_embeddings: dict[str, list[float]],
    segments: list[dict[str, Any]],
    durations: dict[str, int],
) -> list[dict[str, Any]]:
    debug: list[dict[str, Any]] = []
    for local_key in local_keys:
        alias = aliases.get(local_key, local_key)
        if alias == local_key:
            continue
        score = _cosine_similarity(current_embeddings.get(local_key, []), current_embeddings.get(alias, []))
        debug.append(
            {
                "type": "local_cluster_merge",
                "local_key": local_key,
                "target_local_key": alias,
                "score": round(score, 4),
                "duration_ms": durations.get(local_key, 0),
                "target_duration_ms": durations.get(alias, 0),
                "overlap_ms": _speaker_pair_overlap_ms(segments, local_key, alias),
                "threshold": _LOCAL_SPEAKER_MERGE_THRESHOLD,
            }
        )
    return debug


def _aliased_speaker_embeddings(
    embeddings: dict[str, list[float]],
    aliases: dict[str, str],
    durations: dict[str, int],
) -> dict[str, list[float]]:
    grouped: dict[str, list[tuple[list[float], int]]] = {}
    for key, embedding in embeddings.items():
        alias = aliases.get(key, key)
        if embedding:
            grouped.setdefault(alias, []).append((embedding, max(1, durations.get(key, 0))))

    merged: dict[str, list[float]] = {}
    for alias, values in grouped.items():
        if not values:
            continue
        width = len(values[0][0])
        total_weight = 0
        accumulator = [0.0] * width
        for embedding, weight in values:
            if len(embedding) != width:
                continue
            total_weight += weight
            for index, value in enumerate(embedding):
                accumulator[index] += value * weight
        if total_weight <= 0:
            continue
        merged[alias] = _normalize_embedding([value / total_weight for value in accumulator])
    return merged


def _best_existing_speaker_match(
    current_embedding: list[float],
    previous_embeddings: dict[int, list[float]],
) -> tuple[int | None, float]:
    best_index: int | None = None
    best_score = -1.0
    for previous_index, previous_embedding in previous_embeddings.items():
        score = _cosine_similarity(current_embedding, previous_embedding)
        if score > best_score:
            best_index = previous_index
            best_score = score
    return best_index, best_score


def _nearest_effective_key(
    target_key: str,
    candidate_keys: list[str],
    segments: list[dict[str, Any]],
    durations: dict[str, int],
) -> str:
    best_key = ""
    best_gap: int | None = None
    target_segments = [item for item in segments if _speaker_key(item) == target_key]
    for candidate_key in candidate_keys:
        if candidate_key == target_key:
            continue
        candidate_segments = [item for item in segments if _speaker_key(item) == candidate_key]
        for target in target_segments:
            for candidate in candidate_segments:
                if int(candidate.get("end_ms") or 0) <= int(target.get("start_ms") or 0):
                    gap = int(target.get("start_ms") or 0) - int(candidate.get("end_ms") or 0)
                elif int(target.get("end_ms") or 0) <= int(candidate.get("start_ms") or 0):
                    gap = int(candidate.get("start_ms") or 0) - int(target.get("end_ms") or 0)
                else:
                    gap = 0
                if (
                    best_gap is None
                    or gap < best_gap
                    or (gap == best_gap and durations.get(candidate_key, 0) > durations.get(best_key, 0))
                ):
                    best_key = candidate_key
                    best_gap = gap
    return best_key


def _bootstrap_candidate_aliases(
    effective_keys: list[str],
    segments: list[dict[str, Any]],
    durations: dict[str, int],
) -> tuple[dict[str, str], list[dict[str, Any]]]:
    if len(effective_keys) <= 1:
        return {key: key for key in effective_keys}, []
    stable_keys = [key for key in effective_keys if durations.get(key, 0) >= _MIN_NEW_SPEAKER_DURATION_MS]
    min_initial_speakers = min(2, len(effective_keys))
    if len(stable_keys) < min_initial_speakers:
        for key in sorted(effective_keys, key=lambda item: durations.get(item, 0), reverse=True):
            if key not in stable_keys:
                stable_keys.append(key)
            if len(stable_keys) >= min_initial_speakers:
                break
    aliases = {key: key for key in effective_keys}
    events: list[dict[str, Any]] = []
    for key in effective_keys:
        if key in stable_keys:
            continue
        target = _nearest_effective_key(key, stable_keys, segments, durations) or stable_keys[0]
        aliases[key] = target
        events.append(
            {
                "type": "speaker_initial_candidate_suppressed",
                "effective_key": key,
                "target_effective_key": target,
                "duration_ms": durations.get(key, 0),
                "target_duration_ms": durations.get(target, 0),
                "min_new_speaker_duration_ms": _MIN_NEW_SPEAKER_DURATION_MS,
            }
        )
    return aliases, events


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


def _assignment_debug_item(
    *,
    event_type: str,
    effective_key: str,
    speaker_index: int,
    local_keys: list[str],
    aliases: dict[str, str],
    durations: dict[str, int],
    score: float | None = None,
    overlap_ms: int | None = None,
) -> dict[str, Any]:
    grouped_local_keys = [key for key in local_keys if aliases.get(key, key) == effective_key]
    item: dict[str, Any] = {
        "type": event_type,
        "effective_key": effective_key,
        "local_keys": grouped_local_keys,
        "speaker_index": speaker_index,
        "speaker_label": f"Speaker {speaker_index + 1}",
        "duration_ms": sum(durations.get(key, 0) for key in grouped_local_keys),
    }
    if score is not None:
        item["score"] = round(score, 4)
    if overlap_ms is not None:
        item["overlap_ms"] = overlap_ms
    return item


def _speaker_index_assignments(
    segments: Any,
    previous_segments: Any,
    *,
    speaker_embeddings: Any = None,
    previous_speaker_embeddings: Any = None,
    embedding_match_threshold: float = _DEFAULT_SPEAKER_EMBEDDING_MATCH_THRESHOLD,
) -> dict[str, int]:
    assignments, _debug = _speaker_index_assignments_with_debug(
        segments,
        previous_segments,
        speaker_embeddings=speaker_embeddings,
        previous_speaker_embeddings=previous_speaker_embeddings,
        embedding_match_threshold=embedding_match_threshold,
    )
    return assignments


def _speaker_index_assignments_with_debug(
    segments: Any,
    previous_segments: Any,
    *,
    speaker_embeddings: Any = None,
    previous_speaker_embeddings: Any = None,
    embedding_match_threshold: float = _DEFAULT_SPEAKER_EMBEDDING_MATCH_THRESHOLD,
) -> tuple[dict[str, int], dict[str, Any]]:
    if not isinstance(segments, list):
        return {}, {"events": [], "local_speaker_count": 0, "effective_speaker_count": 0}
    current = [dict(item) for item in segments if isinstance(item, dict)]
    previous = [dict(item) for item in previous_segments if isinstance(item, dict)] if isinstance(previous_segments, list) else []
    if not current:
        return {}, {"events": [], "local_speaker_count": 0, "effective_speaker_count": 0}

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
    durations = _speaker_durations_by_key(current)
    aliases = _local_speaker_aliases(
        local_keys=local_keys,
        segments=current,
        current_embeddings=current_embeddings,
        durations=durations,
    )
    effective_keys = []
    for key in local_keys:
        alias = aliases.get(key, key)
        if alias not in effective_keys:
            effective_keys.append(alias)
    effective_embeddings = _aliased_speaker_embeddings(current_embeddings, aliases, durations)
    debug_events = _local_speaker_merge_debug(
        local_keys=local_keys,
        aliases=aliases,
        current_embeddings=current_embeddings,
        segments=current,
        durations=durations,
    )

    if not previous:
        effective_durations = {
            effective_key: sum(
                duration
                for key, duration in durations.items()
                if aliases.get(key, key) == effective_key
            )
            for effective_key in effective_keys
        }
        bootstrap_aliases, bootstrap_events = _bootstrap_candidate_aliases(
            effective_keys,
            current,
            effective_durations,
        )
        debug_events.extend(bootstrap_events)
        bootstrapped_keys = []
        for effective_key in effective_keys:
            alias = bootstrap_aliases.get(effective_key, effective_key)
            if alias not in bootstrapped_keys:
                bootstrapped_keys.append(alias)
        assignments: dict[str, int] = {}
        for effective_key in bootstrapped_keys:
            assignments[effective_key] = len(assignments)
            debug_events.append(
                _assignment_debug_item(
                    event_type="speaker_created_initial",
                    effective_key=effective_key,
                    speaker_index=assignments[effective_key],
                    local_keys=local_keys,
                    aliases=aliases,
                    durations=durations,
                )
            )
        return (
            {
                local_key: assignments[bootstrap_aliases.get(aliases.get(local_key, local_key), aliases.get(local_key, local_key))]
                for local_key in local_keys
                if bootstrap_aliases.get(aliases.get(local_key, local_key), aliases.get(local_key, local_key)) in assignments
            },
            {
                "events": debug_events,
                "local_speaker_count": len(local_keys),
                "effective_speaker_count": len(bootstrapped_keys),
            },
        )

    assignments, claimed_previous = _assign_by_embedding(
        local_keys=effective_keys,
        current_embeddings=effective_embeddings,
        previous_embeddings=previous_embeddings,
        threshold=float(embedding_match_threshold),
    )
    for effective_key, speaker_index in assignments.items():
        score = _cosine_similarity(effective_embeddings.get(effective_key, []), previous_embeddings.get(speaker_index, []))
        debug_events.append(
            _assignment_debug_item(
                event_type="speaker_matched_embedding",
                effective_key=effective_key,
                speaker_index=speaker_index,
                local_keys=local_keys,
                aliases=aliases,
                durations=durations,
                score=score,
            )
        )

    ranked_pairs = sorted(overlap_by_pair.items(), key=lambda row: row[1], reverse=True)
    for (local_key, prev_index), overlap in ranked_pairs:
        effective_key = aliases.get(local_key, local_key)
        if effective_key in assignments or prev_index in claimed_previous:
            continue
        assignments[effective_key] = prev_index
        claimed_previous.add(prev_index)
        debug_events.append(
            _assignment_debug_item(
                event_type="speaker_matched_overlap",
                effective_key=effective_key,
                speaker_index=prev_index,
                local_keys=local_keys,
                aliases=aliases,
                durations=durations,
                overlap_ms=overlap,
            )
        )

    if previous_embeddings:
        soft_matches: list[tuple[float, str, int]] = []
        for effective_key in effective_keys:
            if effective_key in assignments:
                continue
            current_embedding = effective_embeddings.get(effective_key)
            if not current_embedding:
                continue
            duration_ms = sum(
                duration
                for key, duration in durations.items()
                if aliases.get(key, key) == effective_key
            )
            best_index, best_score = _best_existing_speaker_match(current_embedding, previous_embeddings)
            if best_index is None:
                continue
            if duration_ms < _MIN_NEW_SPEAKER_DURATION_MS or best_score >= _SOFT_EXISTING_SPEAKER_MATCH_THRESHOLD:
                soft_matches.append((best_score, effective_key, best_index))
        soft_matches.sort(reverse=True, key=lambda row: row[0])
        for _score, effective_key, previous_index in soft_matches:
            if effective_key in assignments:
                continue
            assignments[effective_key] = previous_index
            debug_events.append(
                _assignment_debug_item(
                    event_type="speaker_candidate_suppressed",
                    effective_key=effective_key,
                    speaker_index=previous_index,
                    local_keys=local_keys,
                    aliases=aliases,
                    durations=durations,
                    score=_score,
                )
            )

    next_index = _next_global_speaker_index(previous)
    for effective_key in effective_keys:
        if effective_key in assignments:
            continue
        current_embedding = effective_embeddings.get(effective_key)
        if current_embedding and previous_embeddings:
            duration_ms = sum(
                duration
                for key, duration in durations.items()
                if aliases.get(key, key) == effective_key
            )
            best_index, best_score = _best_existing_speaker_match(current_embedding, previous_embeddings)
            if (
                best_index is not None
                and duration_ms < _MIN_NEW_SPEAKER_DURATION_MS
                and best_score > -1.0
            ):
                assignments[effective_key] = best_index
                debug_events.append(
                    _assignment_debug_item(
                        event_type="speaker_candidate_suppressed",
                        effective_key=effective_key,
                        speaker_index=best_index,
                        local_keys=local_keys,
                        aliases=aliases,
                        durations=durations,
                        score=best_score,
                    )
                )
                continue
        while next_index in claimed_previous:
            next_index += 1
        assignments[effective_key] = next_index
        claimed_previous.add(next_index)
        debug_events.append(
            _assignment_debug_item(
                event_type="speaker_created",
                effective_key=effective_key,
                speaker_index=next_index,
                local_keys=local_keys,
                aliases=aliases,
                durations=durations,
            )
        )
        next_index += 1
    return (
        {local_key: assignments[aliases.get(local_key, local_key)] for local_key in local_keys if aliases.get(local_key, local_key) in assignments},
        {
            "events": debug_events,
            "local_speaker_count": len(local_keys),
            "effective_speaker_count": len(effective_keys),
        },
    )


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
    grouped: dict[int, list[tuple[list[float], int, int]]] = {}
    for item in speaker_embeddings:
        if not isinstance(item, dict):
            continue
        local_key = _speaker_key(item)
        speaker_index = assignments.get(local_key)
        if speaker_index is None:
            continue
        embedding = _safe_embedding(item.get("embedding"))
        if not embedding:
            continue
        try:
            duration_ms = max(1, int(item.get("duration_ms") or 0))
        except Exception:
            duration_ms = 1
        try:
            sample_count = max(0, int(item.get("sample_count") or 0))
        except Exception:
            sample_count = 0
        grouped.setdefault(speaker_index, []).append((embedding, duration_ms, sample_count))

    remapped: list[dict[str, Any]] = []
    for speaker_index in sorted(grouped):
        values = grouped[speaker_index]
        if not values:
            continue
        width = len(values[0][0])
        total_duration = 0
        sample_count = 0
        accumulator = [0.0] * width
        for embedding, duration_ms, count in values:
            if len(embedding) != width:
                continue
            total_duration += duration_ms
            sample_count += count
            for index, value in enumerate(embedding):
                accumulator[index] += value * duration_ms
        if total_duration <= 0:
            continue
        remapped.append(
            {
                "speaker_index": speaker_index,
                "speaker_label": f"Speaker {speaker_index + 1}",
                "embedding": _normalize_embedding([value / total_duration for value in accumulator]),
                "duration_ms": total_duration,
                "sample_count": sample_count,
            }
        )
    return remapped


def _speaker_identity_debug(
    *,
    debug: dict[str, Any],
    assignments: dict[str, int],
    run_kind: str,
    window_start_ms: int | None = None,
    audio_duration_ms: int | None = None,
) -> dict[str, Any]:
    events = debug.get("events") if isinstance(debug.get("events"), list) else []
    payload: dict[str, Any] = {
        "kind": run_kind,
        "events": events,
        "event_count": len(events),
        "local_speaker_count": int(debug.get("local_speaker_count") or 0),
        "effective_speaker_count": int(debug.get("effective_speaker_count") or 0),
        "assignment_count": len(assignments),
        "thresholds": {
            "embedding_match": _DEFAULT_SPEAKER_EMBEDDING_MATCH_THRESHOLD,
            "local_merge": _LOCAL_SPEAKER_MERGE_THRESHOLD,
            "soft_existing_match": _SOFT_EXISTING_SPEAKER_MATCH_THRESHOLD,
            "min_new_speaker_duration_ms": _MIN_NEW_SPEAKER_DURATION_MS,
        },
    }
    if window_start_ms is not None:
        payload["window_start_ms"] = int(window_start_ms)
    if audio_duration_ms is not None:
        payload["audio_duration_ms"] = int(audio_duration_ms)
    return payload


async def run_provisional_diarization_window(
    pcm16_audio: bytes,
    *,
    selected_model_id: str,
    sample_rate: int,
    run_seq: int,
    audio_duration_ms: int,
    window_start_ms: int,
    previous_segments: list[dict[str, Any]],
    previous_speaker_embeddings: list[dict[str, Any]] | None = None,
) -> dict[str, Any]:
    result = await run_sherpa_diarization(
        pcm16_audio,
        selected_model_id=selected_model_id,
        sample_rate=sample_rate,
        include_speaker_embeddings=True,
    )
    shifted_raw_segments = _shift_segments(result.get("segments"), window_start_ms)
    base_previous_embeddings = previous_speaker_embeddings or []
    assignments, debug = _speaker_index_assignments_with_debug(
        shifted_raw_segments,
        previous_segments,
        speaker_embeddings=result.get("speaker_embeddings"),
        previous_speaker_embeddings=base_previous_embeddings,
    )
    stable_window_segments = stabilize_diarization_speaker_identity(
        shifted_raw_segments,
        previous_segments,
        speaker_embeddings=result.get("speaker_embeddings"),
        previous_speaker_embeddings=base_previous_embeddings,
    )
    return {
        **result,
        "segments": _merge_rolling_segments(
            previous_segments,
            stable_window_segments,
            window_start_ms=window_start_ms,
        ),
        "speaker_embeddings": remap_speaker_embeddings(result.get("speaker_embeddings"), assignments),
        "run_seq": run_seq,
        "audio_duration_ms": audio_duration_ms,
        "window_start_ms": window_start_ms,
        "window_duration_ms": max(0, int(audio_duration_ms) - int(window_start_ms)),
        "speaker_identity_debug": _speaker_identity_debug(
            debug=debug,
            assignments=assignments,
            run_kind="provisional_window",
            window_start_ms=window_start_ms,
            audio_duration_ms=audio_duration_ms,
        ),
        "provisional": True,
    }


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
    return await run_provisional_diarization_window(
        pcm16_audio,
        selected_model_id=selected_model_id,
        sample_rate=sample_rate,
        run_seq=run_seq,
        audio_duration_ms=audio_duration_ms,
        window_start_ms=0,
        previous_segments=previous_segments,
        previous_speaker_embeddings=previous_speaker_embeddings,
    )


async def run_final_diarization_file(
    pcm16_path: Path,
    *,
    selected_model_id: str,
    sample_rate: int,
    previous_segments: list[dict[str, Any]] | None = None,
    previous_speaker_embeddings: list[dict[str, Any]] | None = None,
) -> dict[str, Any]:
    result = await run_sherpa_diarization_file(
        pcm16_path,
        selected_model_id=selected_model_id,
        sample_rate=sample_rate,
        include_speaker_embeddings=True,
    )
    base_previous_segments = previous_segments or []
    base_previous_embeddings = previous_speaker_embeddings or []
    assignments, debug = _speaker_index_assignments_with_debug(
        result.get("segments"),
        base_previous_segments,
        speaker_embeddings=result.get("speaker_embeddings"),
        previous_speaker_embeddings=base_previous_embeddings,
    )
    return {
        **result,
        "segments": stabilize_diarization_speaker_identity(
            result.get("segments"),
            base_previous_segments,
            speaker_embeddings=result.get("speaker_embeddings"),
            previous_speaker_embeddings=base_previous_embeddings,
        ),
        "speaker_embeddings": remap_speaker_embeddings(result.get("speaker_embeddings"), assignments),
        "speaker_identity_debug": _speaker_identity_debug(
            debug=debug,
            assignments=assignments,
            run_kind="final",
        ),
        "provisional": False,
    }
