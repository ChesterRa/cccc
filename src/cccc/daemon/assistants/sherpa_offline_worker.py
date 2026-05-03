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


def _build_recognizer(args: argparse.Namespace) -> Any:
    if args.engine != "sense_voice":
        raise ValueError(f"unsupported sherpa offline engine: {args.engine}")
    return sherpa_onnx.OfflineRecognizer.from_sense_voice(
        model=args.model,
        tokens=args.tokens,
        num_threads=int(args.num_threads),
        sample_rate=int(args.sample_rate),
        language=args.language,
        use_itn=bool(args.use_itn),
        provider=args.provider,
    )


def _text_from_result(result: Any) -> str:
    text = getattr(result, "text", result)
    return str(text or "").strip()


def _recognizer_result_text(recognizer: Any, stream: Any) -> str:
    get_result = getattr(recognizer, "get_result", None)
    if callable(get_result):
        return _text_from_result(get_result(stream))
    return _text_from_result(getattr(stream, "result", ""))


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="CCCC sherpa-onnx JSONL offline ASR worker.")
    parser.add_argument("--engine", required=True)
    parser.add_argument("--model", required=True)
    parser.add_argument("--tokens", required=True)
    parser.add_argument("--sample-rate", type=int, default=16000)
    parser.add_argument("--num-threads", type=int, default=2)
    parser.add_argument("--provider", default="cpu")
    parser.add_argument("--language", default="auto")
    parser.add_argument("--use-itn", action=argparse.BooleanOptionalAction, default=True)
    args = parser.parse_args(argv)

    try:
        recognizer = _build_recognizer(args)
    except Exception as exc:
        _send({"type": "error", "error": {"code": "asr_backend_failed", "message": str(exc), "details": {}}})
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
            if message_type == "transcribe":
                samples = _pcm16_to_float32(str(message.get("audio_base64") or ""))
                stream = recognizer.create_stream()
                if samples.size:
                    stream.accept_waveform(int(message.get("sample_rate") or args.sample_rate), samples)
                recognizer.decode_stream(stream)
                _send({"type": "transcript", "seq": seq, "text": _recognizer_result_text(recognizer, stream)})
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
        _send({"type": "error", "error": {"code": "asr_backend_failed", "message": str(exc), "details": {}}})
        return 2
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
