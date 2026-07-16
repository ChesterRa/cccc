from __future__ import annotations

import asyncio
import json
import os
import tempfile
from pathlib import Path
from typing import Any

from ...paths import ensure_home
from .voice_models import (
    get_voice_model_status,
    list_voice_models,
    resolve_installed_voice_model_diarization_config,
)
from .voice_runtime_deps import (
    VOICE_RUNTIME_ID_SHERPA_ONNX_STREAMING,
    VOICE_RUNTIME_STATUS_READY,
    get_voice_runtime_status,
)


class SherpaDiarizationError(Exception):
    def __init__(self, code: str, message: str, *, details: dict[str, Any] | None = None) -> None:
        super().__init__(message)
        self.code = code
        self.message = message
        self.details = details or {}


def resolve_diarization_num_speakers(value: Any) -> int:
    try:
        parsed = int(value)
    except Exception:
        return -1
    return parsed if parsed > 0 else -1


def _duration_ms(item: dict[str, Any]) -> int:
    return max(0, int(item["end_ms"]) - int(item["start_ms"]))


def _absorb_short_speaker_clusters(
    segments: list[dict[str, Any]],
    *,
    min_cluster_duration_ms: int = 1500,
    min_single_turn_ms: int = 900,
) -> list[dict[str, Any]]:
    if len(segments) < 3:
        return segments

    totals: dict[str, int] = {}
    counts: dict[str, int] = {}
    for item in segments:
        key = str(item["raw_speaker_key"])
        totals[key] = totals.get(key, 0) + _duration_ms(item)
        counts[key] = counts.get(key, 0) + 1
    if len(totals) <= 1:
        return segments

    short_keys = {
        key
        for key, total in totals.items()
        if total < int(min_cluster_duration_ms)
        or (counts.get(key, 0) == 1 and total < int(min_single_turn_ms))
    }
    stable_keys = set(totals) - short_keys
    if not short_keys or not stable_keys:
        return segments

    def nearest_stable_key(index: int) -> str:
        item = segments[index]
        best_key = ""
        best_gap: int | None = None
        for other_index, other in enumerate(segments):
            key = str(other["raw_speaker_key"])
            if other_index == index or key not in stable_keys:
                continue
            if int(other["end_ms"]) <= int(item["start_ms"]):
                gap = int(item["start_ms"]) - int(other["end_ms"])
            elif int(item["end_ms"]) <= int(other["start_ms"]):
                gap = int(other["start_ms"]) - int(item["end_ms"])
            else:
                gap = 0
            if best_gap is None or gap < best_gap or (gap == best_gap and totals.get(key, 0) > totals.get(best_key, 0)):
                best_key = key
                best_gap = gap
        return best_key

    absorbed: list[dict[str, Any]] = []
    for index, item in enumerate(segments):
        key = str(item["raw_speaker_key"])
        if key not in short_keys:
            absorbed.append(item)
            continue
        target_key = nearest_stable_key(index)
        absorbed.append({**item, "raw_speaker_key": target_key or key})
    return absorbed


def _cluster_absorb_thresholds(raw_segments: list[dict[str, Any]]) -> tuple[int, int]:
    if not raw_segments:
        return 1500, 900
    start_ms = min(int(item["start_ms"]) for item in raw_segments)
    end_ms = max(int(item["end_ms"]) for item in raw_segments)
    span_ms = max(0, end_ms - start_ms)
    if span_ms <= 12_000:
        return max(1500, int(span_ms * 0.2)), max(900, int(span_ms * 0.12))
    return 1500, 900


def normalize_diarization_segments(
    segments: Any,
    *,
    min_duration_ms: int = 250,
    merge_gap_ms: int = 350,
) -> list[dict[str, Any]]:
    if not isinstance(segments, list):
        return []
    raw_segments: list[dict[str, Any]] = []
    for item in segments:
        if not isinstance(item, dict):
            continue
        try:
            start_ms = max(0, int(item.get("start_ms") or 0))
            end_ms = max(0, int(item.get("end_ms") or 0))
        except Exception:
            continue
        if end_ms - start_ms < int(min_duration_ms):
            continue
        raw_key = str(item.get("speaker_index") if item.get("speaker_index") is not None else item.get("speaker_label") or "").strip()
        if not raw_key:
            continue
        raw_segments.append({"start_ms": start_ms, "end_ms": end_ms, "raw_speaker_key": raw_key})
    raw_segments.sort(key=lambda row: (int(row["start_ms"]), int(row["end_ms"])))
    min_cluster_duration_ms, min_single_turn_ms = _cluster_absorb_thresholds(raw_segments)
    raw_segments = _absorb_short_speaker_clusters(
        raw_segments,
        min_cluster_duration_ms=min_cluster_duration_ms,
        min_single_turn_ms=min_single_turn_ms,
    )
    speaker_map: dict[str, int] = {}
    normalized: list[dict[str, Any]] = []
    for item in raw_segments:
        raw_key = str(item["raw_speaker_key"])
        if raw_key not in speaker_map:
            speaker_map[raw_key] = len(speaker_map) + 1
        speaker_index = speaker_map[raw_key] - 1
        normalized.append(
            {
                "start_ms": int(item["start_ms"]),
                "end_ms": int(item["end_ms"]),
                "speaker_label": f"Speaker {speaker_index + 1}",
                "speaker_index": speaker_index,
            }
        )
    merged: list[dict[str, Any]] = []
    for item in normalized:
        previous = merged[-1] if merged else None
        if (
            previous is not None
            and previous.get("speaker_index") == item.get("speaker_index")
            and int(item["start_ms"]) - int(previous["end_ms"]) <= int(merge_gap_ms)
        ):
            previous["end_ms"] = max(int(previous["end_ms"]), int(item["end_ms"]))
            continue
        merged.append(dict(item))
    return merged


def resolve_sherpa_diarization_model_id(selected_model_id: str = "") -> str:
    selected = str(selected_model_id or "").strip()
    if selected:
        return selected
    for item in list_voice_models():
        if str(item.get("runtime_id") or "") != VOICE_RUNTIME_ID_SHERPA_ONNX_STREAMING:
            continue
        if str(item.get("kind") or "") == "diarization" and bool(item.get("diarization_ready")):
            return str(item.get("model_id") or "").strip()
    for item in list_voice_models():
        if str(item.get("runtime_id") or "") != VOICE_RUNTIME_ID_SHERPA_ONNX_STREAMING:
            continue
        if str(item.get("kind") or "") == "diarization":
            return str(item.get("model_id") or "").strip()
    return ""


def sherpa_diarization_status(selected_model_id: str = "") -> dict[str, Any]:
    runtime = get_voice_runtime_status(VOICE_RUNTIME_ID_SHERPA_ONNX_STREAMING)
    model_id = resolve_sherpa_diarization_model_id(selected_model_id)
    model = get_voice_model_status(model_id) if model_id else {}
    ready = (
        str(runtime.get("status") or "") == VOICE_RUNTIME_STATUS_READY
        and str(model.get("status") or "") == "ready"
        and bool(model.get("diarization_ready"))
    )
    return {
        "runtime": runtime,
        "model_id": model_id,
        "model": model,
        "ready": ready,
    }


async def run_sherpa_diarization(
    pcm16_audio: bytes,
    *,
    selected_model_id: str = "",
    sample_rate: int = 16000,
    include_speaker_embeddings: bool = False,
) -> dict[str, Any]:
    if not pcm16_audio:
        model_id = resolve_sherpa_diarization_model_id(selected_model_id)
        return {"segments": [], "model_id": model_id, "sample_rate": int(sample_rate)}
    tmp_path = Path("")
    try:
        fd, raw_tmp = tempfile.mkstemp(prefix="cccc-diarization-", suffix=".pcm16")
        tmp_path = Path(raw_tmp)
        with os.fdopen(fd, "wb") as handle:
            handle.write(pcm16_audio)
        return await run_sherpa_diarization_file(
            tmp_path,
            selected_model_id=selected_model_id,
            sample_rate=sample_rate,
            include_speaker_embeddings=include_speaker_embeddings,
        )
    finally:
        if tmp_path:
            try:
                tmp_path.unlink()
            except Exception:
                pass


async def run_sherpa_diarization_file(
    pcm16_path: Path,
    *,
    selected_model_id: str = "",
    sample_rate: int = 16000,
    include_speaker_embeddings: bool = False,
) -> dict[str, Any]:
    status = sherpa_diarization_status(selected_model_id)
    runtime = status.get("runtime") if isinstance(status.get("runtime"), dict) else {}
    model = status.get("model") if isinstance(status.get("model"), dict) else {}
    model_id = str(status.get("model_id") or "").strip()
    if not pcm16_path.exists() or pcm16_path.stat().st_size <= 0:
        return {"segments": [], "model_id": model_id, "sample_rate": int(sample_rate)}
    if str(runtime.get("status") or "") != VOICE_RUNTIME_STATUS_READY:
        raise SherpaDiarizationError(
            "diarization_runtime_not_ready",
            "sherpa-onnx runtime is not installed",
            details={"runtime": runtime},
        )
    if str(model.get("status") or "") != "ready" or not bool(model.get("diarization_ready")):
        raise SherpaDiarizationError(
            "diarization_model_not_ready",
            "sherpa-onnx diarization model is not installed",
            details={"model": model, "model_id": model_id},
        )
    config = resolve_installed_voice_model_diarization_config(model_id)
    python_path = str(runtime.get("python") or "").strip()
    if not python_path:
        raise SherpaDiarizationError("diarization_runtime_not_ready", "sherpa-onnx runtime Python is missing", details={"runtime": runtime})

    process: asyncio.subprocess.Process | None = None
    try:
        argv = [
            python_path,
            "-m",
            "cccc.daemon.assistants.sherpa_diarization_worker",
            "--pcm16",
            str(pcm16_path),
            "--segmentation-model",
            str(config.get("segmentation_model") or ""),
            "--embedding-model",
            str(config.get("embedding_model") or ""),
            "--sample-rate",
            str(int(sample_rate or config.get("sample_rate") or 16000)),
            "--num-threads",
            str(int(config.get("num_threads") or 2)),
            "--provider",
            str(config.get("provider") or "cpu"),
            "--num-speakers",
            str(resolve_diarization_num_speakers(config.get("num_speakers"))),
            "--cluster-threshold",
            str(float(config.get("cluster_threshold") or 0.5)),
            "--min-duration-on",
            str(float(config.get("min_duration_on") or 0.3)),
            "--min-duration-off",
            str(float(config.get("min_duration_off") or 0.5)),
        ]
        if include_speaker_embeddings:
            argv.append("--include-speaker-embeddings")
        env = os.environ.copy()
        env["CCCC_HOME"] = str(ensure_home())
        source_root = str(Path(__file__).resolve().parents[3])
        env["PYTHONPATH"] = source_root if not env.get("PYTHONPATH") else f"{source_root}{os.pathsep}{env['PYTHONPATH']}"
        env.pop("__PYVENV_LAUNCHER__", None)
        process = await asyncio.create_subprocess_exec(
            *argv,
            stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.PIPE,
            env=env,
        )
        stdout, stderr = await asyncio.wait_for(process.communicate(), timeout=180.0)
    except asyncio.TimeoutError as exc:
        if process is not None and process.returncode is None:
            try:
                process.terminate()
                await asyncio.wait_for(process.wait(), timeout=2.0)
            except Exception:
                try:
                    process.kill()
                except Exception:
                    pass
        raise SherpaDiarizationError("diarization_backend_timeout", "sherpa-onnx diarization timed out") from exc

    text = stdout.decode("utf-8", errors="replace").strip()
    try:
        payload = json.loads(text or "{}")
    except Exception as exc:
        raise SherpaDiarizationError(
            "diarization_backend_invalid_response",
            "sherpa-onnx diarization returned invalid JSON",
            details={"stdout": text[:1000], "stderr": stderr.decode("utf-8", errors="replace")[-2000:]},
        ) from exc
    if process.returncode != 0 or not bool(payload.get("ok")):
        error = payload.get("error") if isinstance(payload.get("error"), dict) else {}
        raise SherpaDiarizationError(
            str(error.get("code") or "diarization_backend_failed"),
            str(error.get("message") or "sherpa-onnx diarization failed"),
            details={
                **(error.get("details") if isinstance(error.get("details"), dict) else {}),
                "returncode": process.returncode,
                "stderr": stderr.decode("utf-8", errors="replace")[-4000:],
            },
        )
    result = {
        "segments": normalize_diarization_segments(payload.get("segments")),
        "model_id": model_id,
        "sample_rate": int(payload.get("sample_rate") or sample_rate or 16000),
    }
    if include_speaker_embeddings and isinstance(payload.get("speaker_embeddings"), list):
        result["speaker_embeddings"] = payload.get("speaker_embeddings")
    return result
