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
        _send({"ok": True, "segments": segments, "sample_rate": int(diarizer.sample_rate)})
        return 0
    except Exception as exc:
        _send({"ok": False, "error": {"code": "diarization_backend_failed", "message": str(exc), "details": {}}})
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
