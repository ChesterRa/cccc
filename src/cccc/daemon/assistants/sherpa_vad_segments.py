from __future__ import annotations

import asyncio
import base64
import contextlib
import hashlib
import json
import os
import urllib.request
from pathlib import Path
from typing import Any

from ...paths import ensure_home
from .voice_runtime_deps import (
    VOICE_RUNTIME_ID_SHERPA_ONNX_STREAMING,
    VOICE_RUNTIME_STATUS_READY,
    get_voice_runtime_status,
)

_SILERO_VAD_URL = "https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/silero_vad.onnx"
_SILERO_VAD_SHA256 = "9e2449e1087496d8d4caba907f23e0bd3f78d91fa552479bb9c23ac09cbb1fd6"
_SILERO_VAD_SIZE_BYTES = 643854
_READ_CHUNK_BYTES = 1024 * 1024


class SherpaVadSegmentError(Exception):
    def __init__(self, code: str, message: str, *, details: dict[str, Any] | None = None) -> None:
        super().__init__(message)
        self.code = code
        self.message = message
        self.details = details or {}


def _vad_model_path() -> Path:
    return ensure_home() / "cache" / "voice-models" / "_shared" / "silero_vad.onnx"


def _sha256_file(path: Path) -> str:
    hasher = hashlib.sha256()
    with path.open("rb") as handle:
        while True:
            chunk = handle.read(_READ_CHUNK_BYTES)
            if not chunk:
                break
            hasher.update(chunk)
    return hasher.hexdigest()


def ensure_silero_vad_model() -> Path:
    model_path = _vad_model_path()
    if model_path.exists() and model_path.stat().st_size == _SILERO_VAD_SIZE_BYTES and _sha256_file(model_path) == _SILERO_VAD_SHA256:
        return model_path
    model_path.parent.mkdir(parents=True, exist_ok=True)
    tmp_path = model_path.with_suffix(".onnx.part")
    try:
        hasher = hashlib.sha256()
        size = 0
        request = urllib.request.Request(_SILERO_VAD_URL, headers={"User-Agent": "cccc-voice-vad/1.0"})
        with urllib.request.urlopen(request, timeout=60) as resp, tmp_path.open("wb") as handle:
            while True:
                chunk = resp.read(_READ_CHUNK_BYTES)
                if not chunk:
                    break
                hasher.update(chunk)
                handle.write(chunk)
                size += len(chunk)
        digest = hasher.hexdigest()
        if size != _SILERO_VAD_SIZE_BYTES or digest != _SILERO_VAD_SHA256:
            raise SherpaVadSegmentError(
                "vad_model_invalid",
                "downloaded Silero VAD model failed integrity check",
                details={"expected_size_bytes": _SILERO_VAD_SIZE_BYTES, "actual_size_bytes": size, "expected_sha256": _SILERO_VAD_SHA256, "actual_sha256": digest},
            )
        os.replace(tmp_path, model_path)
        return model_path
    except SherpaVadSegmentError:
        raise
    except Exception as exc:
        raise SherpaVadSegmentError("vad_model_download_failed", "failed to download Silero VAD model", details={"error": str(exc)}) from exc
    finally:
        with contextlib.suppress(Exception):
            tmp_path.unlink()


async def _read_worker(process: asyncio.subprocess.Process, timeout: float | None = None) -> dict[str, Any]:
    if process.stdout is None:
        raise SherpaVadSegmentError("vad_backend_closed", "VAD worker stdout is closed")
    try:
        line = await asyncio.wait_for(process.stdout.readline(), timeout=timeout)
    except asyncio.TimeoutError as exc:
        raise SherpaVadSegmentError("vad_backend_timeout", "VAD worker timed out") from exc
    if not line:
        stderr = ""
        if process.stderr is not None:
            try:
                raw = await asyncio.wait_for(process.stderr.read(), timeout=0.1)
                stderr = raw.decode("utf-8", errors="replace")[-4000:]
            except Exception:
                stderr = ""
        raise SherpaVadSegmentError(
            "vad_backend_closed",
            "VAD worker exited",
            details={"returncode": process.returncode, "stderr": stderr},
        )
    try:
        payload = json.loads(line.decode("utf-8"))
    except Exception as exc:
        raise SherpaVadSegmentError(
            "vad_backend_invalid_response",
            "VAD worker returned invalid JSON",
            details={"line": line.decode("utf-8", errors="replace")[:1000]},
        ) from exc
    return payload if isinstance(payload, dict) else {}


async def detect_sherpa_vad_segments(
    pcm16_audio: bytes,
    *,
    sample_rate: int = 16000,
) -> list[dict[str, int]]:
    if not pcm16_audio:
        return []
    runtime = get_voice_runtime_status(VOICE_RUNTIME_ID_SHERPA_ONNX_STREAMING)
    if str(runtime.get("status") or "") != VOICE_RUNTIME_STATUS_READY:
        raise SherpaVadSegmentError("vad_runtime_not_ready", "sherpa-onnx runtime is not installed", details={"runtime": runtime})
    python_path = str(runtime.get("python") or "").strip()
    if not python_path:
        raise SherpaVadSegmentError("vad_runtime_not_ready", "sherpa-onnx runtime Python is missing", details={"runtime": runtime})
    model_path = ensure_silero_vad_model()
    source_root = str(Path(__file__).resolve().parents[3])
    env = os.environ.copy()
    env["CCCC_HOME"] = str(ensure_home())
    env["PYTHONPATH"] = source_root if not env.get("PYTHONPATH") else f"{source_root}{os.pathsep}{env['PYTHONPATH']}"
    env.pop("__PYVENV_LAUNCHER__", None)
    process = await asyncio.create_subprocess_exec(
        python_path,
        "-m",
        "cccc.daemon.assistants.sherpa_vad_worker",
        "--model",
        str(model_path),
        "--sample-rate",
        str(int(sample_rate or 16000)),
        "--max-speech-duration",
        "12.0",
        stdin=asyncio.subprocess.PIPE,
        stdout=asyncio.subprocess.PIPE,
        stderr=asyncio.subprocess.PIPE,
        env=env,
    )
    try:
        ready = await _read_worker(process, timeout=30.0)
        if str(ready.get("type") or "") == "error":
            error = ready.get("error") if isinstance(ready.get("error"), dict) else {}
            raise SherpaVadSegmentError(
                str(error.get("code") or "vad_backend_failed"),
                str(error.get("message") or "VAD worker failed to start"),
                details=error.get("details") if isinstance(error.get("details"), dict) else {},
            )
        if str(ready.get("type") or "") != "ready":
            raise SherpaVadSegmentError("vad_backend_failed", "VAD worker did not become ready", details={"response": ready})
        if process.stdin is None:
            raise SherpaVadSegmentError("vad_backend_closed", "VAD worker stdin is closed")
        process.stdin.write(
            json.dumps(
                {
                    "type": "segment",
                    "seq": 1,
                    "audio_base64": base64.b64encode(pcm16_audio).decode("ascii"),
                },
                ensure_ascii=False,
            ).encode("utf-8")
            + b"\n"
        )
        await process.stdin.drain()
        result = await _read_worker(process, timeout=60.0)
        if str(result.get("type") or "") == "error":
            error = result.get("error") if isinstance(result.get("error"), dict) else {}
            raise SherpaVadSegmentError(
                str(error.get("code") or "vad_backend_failed"),
                str(error.get("message") or "VAD worker failed"),
                details=error.get("details") if isinstance(error.get("details"), dict) else {},
            )
        segments: list[dict[str, int]] = []
        for item in result.get("segments") if isinstance(result.get("segments"), list) else []:
            if not isinstance(item, dict):
                continue
            start_ms = max(0, int(item.get("start_ms") or 0))
            end_ms = max(start_ms, int(item.get("end_ms") or 0))
            if end_ms > start_ms:
                segments.append({"start_ms": start_ms, "end_ms": end_ms})
        return segments
    finally:
        if process.returncode is None:
            try:
                if process.stdin is not None:
                    process.stdin.close()
            except Exception:
                pass
            try:
                await asyncio.wait_for(process.wait(), timeout=2.0)
            except Exception:
                try:
                    process.terminate()
                except Exception:
                    pass
