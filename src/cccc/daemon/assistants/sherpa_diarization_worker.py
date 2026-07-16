from __future__ import annotations

import argparse
import json
from pathlib import Path
from typing import Any

import numpy as np
import sherpa_onnx


def _send(payload: dict[str, Any]) -> None:
    print(json.dumps(payload, ensure_ascii=False), flush=True)


def _read_pcm16(path: str) -> np.ndarray:
    raw = Path(path).read_bytes()
    if not raw:
        return np.zeros(0, dtype=np.float32)
    return np.frombuffer(raw, dtype="<i2").astype(np.float32) / 32768.0


def _build_diarizer(args: argparse.Namespace) -> Any:
    config = sherpa_onnx.OfflineSpeakerDiarizationConfig(
        segmentation=sherpa_onnx.OfflineSpeakerSegmentationModelConfig(
            pyannote=sherpa_onnx.OfflineSpeakerSegmentationPyannoteModelConfig(
                model=args.segmentation_model,
            ),
            num_threads=int(args.num_threads),
            provider=args.provider,
        ),
        embedding=sherpa_onnx.SpeakerEmbeddingExtractorConfig(
            model=args.embedding_model,
            num_threads=int(args.num_threads),
            provider=args.provider,
        ),
        clustering=sherpa_onnx.FastClusteringConfig(
            num_clusters=int(args.num_speakers),
            threshold=float(args.cluster_threshold),
        ),
        min_duration_on=float(args.min_duration_on),
        min_duration_off=float(args.min_duration_off),
    )
    if not config.validate():
        raise RuntimeError("invalid sherpa-onnx diarization config")
    return sherpa_onnx.OfflineSpeakerDiarization(config)


def _build_embedding_extractor(args: argparse.Namespace) -> Any:
    config = sherpa_onnx.SpeakerEmbeddingExtractorConfig(
        model=args.embedding_model,
        num_threads=int(args.num_threads),
        provider=args.provider,
    )
    if not config.validate():
        raise RuntimeError("invalid sherpa-onnx speaker embedding config")
    return sherpa_onnx.SpeakerEmbeddingExtractor(config)


def _slice_samples(samples: np.ndarray, *, start_ms: int, end_ms: int, sample_rate: int) -> np.ndarray:
    if samples.size == 0 or end_ms <= start_ms:
        return np.zeros(0, dtype=np.float32)
    rate = max(1, int(sample_rate or 16000))
    start = max(0, int(start_ms * rate / 1000))
    end = min(samples.size, max(start, int(end_ms * rate / 1000)))
    return samples[start:end].astype(np.float32, copy=False)


def _compute_embedding(extractor: Any, samples: np.ndarray, *, sample_rate: int) -> list[float]:
    stream = extractor.create_stream()
    stream.accept_waveform(int(sample_rate), samples)
    stream.input_finished()
    if not extractor.is_ready(stream):
        return []
    return [float(value) for value in extractor.compute(stream)]


def _mean_embedding(embeddings: list[list[float]]) -> list[float]:
    if not embeddings:
        return []
    matrix = np.asarray(embeddings, dtype=np.float32)
    if matrix.ndim != 2 or matrix.shape[0] <= 0:
        return []
    mean = matrix.mean(axis=0)
    norm = float(np.linalg.norm(mean))
    if norm > 0:
        mean = mean / norm
    return [float(value) for value in mean.tolist()]


def _speaker_embeddings(
    *,
    args: argparse.Namespace,
    samples: np.ndarray,
    segments: list[dict[str, Any]],
    sample_rate: int,
) -> list[dict[str, Any]]:
    if not segments:
        return []
    extractor = _build_embedding_extractor(args)
    grouped: dict[int, list[list[float]]] = {}
    durations: dict[int, int] = {}
    for item in segments:
        speaker_index = int(item["speaker_index"])
        start_ms = int(item["start_ms"])
        end_ms = int(item["end_ms"])
        segment_samples = _slice_samples(samples, start_ms=start_ms, end_ms=end_ms, sample_rate=sample_rate)
        if segment_samples.size == 0:
            continue
        embedding = _compute_embedding(extractor, segment_samples, sample_rate=sample_rate)
        if not embedding:
            continue
        grouped.setdefault(speaker_index, []).append(embedding)
        durations[speaker_index] = durations.get(speaker_index, 0) + max(0, end_ms - start_ms)
    return [
        {
            "speaker_index": speaker_index,
            "speaker_label": f"Speaker {speaker_index + 1}",
            "embedding": _mean_embedding(grouped[speaker_index]),
            "duration_ms": durations.get(speaker_index, 0),
            "sample_count": len(grouped[speaker_index]),
        }
        for speaker_index in sorted(grouped)
        if grouped.get(speaker_index)
    ]


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="CCCC sherpa-onnx speaker diarization worker.")
    parser.add_argument("--pcm16", required=True)
    parser.add_argument("--segmentation-model", required=True)
    parser.add_argument("--embedding-model", required=True)
    parser.add_argument("--sample-rate", type=int, default=16000)
    parser.add_argument("--num-threads", type=int, default=2)
    parser.add_argument("--provider", default="cpu")
    parser.add_argument("--num-speakers", type=int, default=-1)
    parser.add_argument("--cluster-threshold", type=float, default=0.5)
    parser.add_argument("--min-duration-on", type=float, default=0.3)
    parser.add_argument("--min-duration-off", type=float, default=0.5)
    parser.add_argument("--include-speaker-embeddings", action="store_true")
    args = parser.parse_args(argv)

    try:
        diarizer = _build_diarizer(args)
        samples = _read_pcm16(args.pcm16)
        if samples.size == 0:
            _send({"ok": True, "segments": [], "sample_rate": int(args.sample_rate)})
            return 0
        if int(args.sample_rate) != int(diarizer.sample_rate):
            raise RuntimeError(f"expected sample_rate={diarizer.sample_rate}, got {args.sample_rate}")
        result = diarizer.process(samples).sort_by_start_time()
        segments = [
            {
                "start_ms": int(round(float(item.start) * 1000.0)),
                "end_ms": int(round(float(item.end) * 1000.0)),
                "speaker_label": f"Speaker {int(item.speaker) + 1}",
                "speaker_index": int(item.speaker),
            }
            for item in result
            if float(item.end) > float(item.start)
        ]
        payload = {"ok": True, "segments": segments, "sample_rate": int(diarizer.sample_rate)}
        if args.include_speaker_embeddings:
            payload["speaker_embeddings"] = _speaker_embeddings(
                args=args,
                samples=samples,
                segments=segments,
                sample_rate=int(diarizer.sample_rate),
            )
        _send(payload)
        return 0
    except Exception as exc:
        _send({"ok": False, "error": {"code": "diarization_backend_failed", "message": str(exc), "details": {}}})
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
