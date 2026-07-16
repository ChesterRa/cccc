from __future__ import annotations

import argparse
import base64
import json
import sys
from typing import Any

import numpy as np
import sherpa_onnx


def _send(payload: dict[str, Any]) -> None:
    print(json.dumps(payload, ensure_ascii=False), flush=True)


def _pcm16_to_float32(audio_b64: str) -> np.ndarray:
    raw = base64.b64decode(str(audio_b64 or ""), validate=True)
    if not raw:
        return np.zeros(0, dtype=np.float32)
    return np.frombuffer(raw, dtype="<i2").astype(np.float32) / 32768.0


def _build_vad(args: argparse.Namespace) -> tuple[Any, int]:
    config = sherpa_onnx.VadModelConfig()
    config.silero_vad.model = args.model
    config.silero_vad.threshold = float(args.threshold)
    config.silero_vad.min_silence_duration = float(args.min_silence_duration)
    config.silero_vad.min_speech_duration = float(args.min_speech_duration)
    config.silero_vad.max_speech_duration = float(args.max_speech_duration)
    config.sample_rate = int(args.sample_rate)
    window_size = int(config.silero_vad.window_size)
    return sherpa_onnx.VoiceActivityDetector(config, buffer_size_in_seconds=float(args.buffer_size)), window_size


def _segments_from_samples(samples: np.ndarray, args: argparse.Namespace) -> list[dict[str, Any]]:
    vad, window_size = _build_vad(args)
    offset = 0
    while offset + window_size <= samples.shape[0]:
        vad.accept_waveform(samples[offset : offset + window_size])
        offset += window_size
    if offset < samples.shape[0]:
        vad.accept_waveform(samples[offset:])
    vad.flush()

    segments: list[dict[str, Any]] = []
    sample_rate = int(args.sample_rate)
    while not vad.empty():
        segment = vad.front
        start_sample = int(segment.start)
        end_sample = start_sample + int(len(segment.samples))
        start_ms = max(0, int(round(start_sample * 1000.0 / sample_rate)))
        end_ms = max(start_ms, int(round(end_sample * 1000.0 / sample_rate)))
        segments.append({"start_ms": start_ms, "end_ms": end_ms})
        vad.pop()
    return segments


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="CCCC sherpa-onnx JSONL VAD worker.")
    parser.add_argument("--model", required=True)
    parser.add_argument("--sample-rate", type=int, default=16000)
    parser.add_argument("--threshold", type=float, default=0.2)
    parser.add_argument("--min-silence-duration", type=float, default=0.25)
    parser.add_argument("--min-speech-duration", type=float, default=0.25)
    parser.add_argument("--max-speech-duration", type=float, default=12.0)
    parser.add_argument("--buffer-size", type=float, default=100.0)
    args = parser.parse_args(argv)

    try:
        _build_vad(args)
    except Exception as exc:
        _send({"type": "error", "error": {"code": "vad_backend_failed", "message": str(exc), "details": {}}})
        return 2

    _send({"type": "ready"})
    try:
        for line in sys.stdin:
            try:
                message = json.loads(line)
            except Exception as exc:
                _send({"type": "error", "error": {"code": "invalid_json", "message": str(exc), "details": {}}})
                continue
            message_type = str(message.get("type") or "").strip()
            seq = message.get("seq")
            if message_type == "segment":
                samples = _pcm16_to_float32(str(message.get("audio_base64") or ""))
                _send({"type": "segments", "seq": seq, "segments": _segments_from_samples(samples, args)})
                continue
            if message_type == "close":
                _send({"type": "closed", "seq": seq})
                return 0
            _send({
                "type": "error",
                "seq": seq,
                "error": {"code": "unsupported_message", "message": f"unsupported message type: {message_type}", "details": {}},
            })
    except Exception as exc:
        _send({"type": "error", "error": {"code": "vad_backend_failed", "message": str(exc), "details": {}}})
        return 2
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
