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
    common = {
        "tokens": args.tokens,
        "num_threads": int(args.num_threads),
        "sample_rate": int(args.sample_rate),
        "enable_endpoint_detection": True,
        "provider": args.provider,
    }
    if args.engine == "zipformer2_ctc":
        return sherpa_onnx.OnlineRecognizer.from_zipformer2_ctc(
            model=args.model,
            **common,
        )
    if args.engine == "transducer":
        return sherpa_onnx.OnlineRecognizer.from_transducer(
            encoder=args.encoder,
            decoder=args.decoder,
            joiner=args.joiner,
            **common,
        )
    if args.engine == "paraformer":
        return sherpa_onnx.OnlineRecognizer.from_paraformer(
            encoder=args.encoder,
            decoder=args.decoder,
            **common,
        )
    raise ValueError(f"unsupported sherpa streaming engine: {args.engine}")


def _decode_ready(recognizer: Any, stream: Any) -> str:
    while recognizer.is_ready(stream):
        recognizer.decode_stream(stream)
    return str(recognizer.get_result(stream) or "").strip()


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="CCCC sherpa-onnx JSONL streaming ASR worker.")
    parser.add_argument("--engine", required=True)
    parser.add_argument("--tokens", required=True)
    parser.add_argument("--model", default="")
    parser.add_argument("--encoder", default="")
    parser.add_argument("--decoder", default="")
    parser.add_argument("--joiner", default="")
    parser.add_argument("--sample-rate", type=int, default=16000)
    parser.add_argument("--num-threads", type=int, default=2)
    parser.add_argument("--provider", default="cpu")
    args = parser.parse_args(argv)

    try:
        recognizer = _build_recognizer(args)
        stream = recognizer.create_stream()
    except Exception as exc:
        _send({"type": "error", "error": {"code": "asr_backend_failed", "message": str(exc), "details": {}}})
        return 2

    last_partial = ""
    try:
        for line in sys.stdin:
            try:
                message = json.loads(line)
            except Exception as exc:
                _send({"type": "error", "error": {"code": "invalid_json", "message": str(exc), "details": {}}})
                continue
            message_type = str(message.get("type") or "").strip()
            seq = message.get("seq")
            if message_type == "start":
                _send({"type": "ready", "seq": seq})
                continue
            if message_type == "audio":
                samples = _pcm16_to_float32(str(message.get("audio_base64") or ""))
                if samples.size:
                    stream.accept_waveform(int(message.get("sample_rate") or args.sample_rate), samples)
                text = _decode_ready(recognizer, stream)
                if text and text != last_partial:
                    last_partial = text
                    _send({"type": "partial", "seq": seq, "text": text})
                if recognizer.is_endpoint(stream):
                    final_text = str(recognizer.get_result(stream) or "").strip()
                    if final_text:
                        _send({"type": "final", "seq": seq, "text": final_text})
                    recognizer.reset(stream)
                    last_partial = ""
                continue
            if message_type == "stop":
                stream.input_finished()
                text = _decode_ready(recognizer, stream)
                final_text = text or last_partial
                if final_text:
                    _send({"type": "final", "seq": seq, "text": final_text})
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
