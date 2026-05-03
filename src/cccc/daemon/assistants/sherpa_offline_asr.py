from __future__ import annotations

import asyncio
import base64
import json
import os
from pathlib import Path
from typing import Any

from ...paths import ensure_home
from .voice_models import (
    get_voice_model_status,
    list_voice_models,
    resolve_installed_voice_model_offline_config,
)
from .voice_runtime_deps import (
    VOICE_RUNTIME_ID_SHERPA_ONNX_STREAMING,
    VOICE_RUNTIME_STATUS_READY,
    get_voice_runtime_status,
)


class SherpaOfflineAsrError(Exception):
    def __init__(self, code: str, message: str, *, details: dict[str, Any] | None = None) -> None:
        super().__init__(message)
        self.code = code
        self.message = message
        self.details = details or {}


def resolve_sherpa_offline_model_id(selected_model_id: str = "") -> str:
    selected = str(selected_model_id or "").strip()
    if selected:
        selected_status = get_voice_model_status(selected)
        if (
            str(selected_status.get("runtime_id") or "") == VOICE_RUNTIME_ID_SHERPA_ONNX_STREAMING
            and bool(selected_status.get("offline"))
            and bool(selected_status.get("offline_ready"))
        ):
            return selected
    for item in list_voice_models():
        if not isinstance(item, dict):
            continue
        if str(item.get("runtime_id") or "") != VOICE_RUNTIME_ID_SHERPA_ONNX_STREAMING:
            continue
        if not item.get("offline"):
            continue
        if bool(item.get("offline_ready")):
            return str(item.get("model_id") or "").strip()
    for item in list_voice_models():
        if not isinstance(item, dict):
            continue
        if str(item.get("runtime_id") or "") != VOICE_RUNTIME_ID_SHERPA_ONNX_STREAMING:
            continue
        if item.get("offline"):
            return str(item.get("model_id") or "").strip()
    return ""


def sherpa_offline_backend_status(selected_model_id: str = "") -> dict[str, Any]:
    runtime = get_voice_runtime_status(VOICE_RUNTIME_ID_SHERPA_ONNX_STREAMING)
    model_id = resolve_sherpa_offline_model_id(selected_model_id)
    model = get_voice_model_status(model_id) if model_id else {}
    ready = (
        str(runtime.get("status") or "") == VOICE_RUNTIME_STATUS_READY
        and str(model.get("status") or "") == "ready"
        and bool(model.get("offline_ready"))
    )
    return {
        "runtime": runtime,
        "model_id": model_id,
        "model": model,
        "ready": ready,
    }


async def _read_worker(process: asyncio.subprocess.Process, timeout: float | None = None) -> dict[str, Any]:
    if process.stdout is None:
        raise SherpaOfflineAsrError("asr_backend_closed", "ASR worker stdout is closed")
    try:
        line = await asyncio.wait_for(process.stdout.readline(), timeout=timeout)
    except asyncio.TimeoutError as exc:
        raise SherpaOfflineAsrError("asr_backend_timeout", "ASR worker timed out") from exc
    if not line:
        stderr = ""
        if process.stderr is not None:
            try:
                raw = await asyncio.wait_for(process.stderr.read(), timeout=0.1)
                stderr = raw.decode("utf-8", errors="replace")[-4000:]
            except Exception:
                stderr = ""
        raise SherpaOfflineAsrError(
            "asr_backend_closed",
            "ASR worker exited",
            details={"returncode": process.returncode, "stderr": stderr},
        )
    try:
        payload = json.loads(line.decode("utf-8"))
    except Exception as exc:
        raise SherpaOfflineAsrError(
            "asr_backend_invalid_response",
            "ASR worker returned invalid JSON",
            details={"line": line.decode("utf-8", errors="replace")[:1000]},
        ) from exc
    return payload if isinstance(payload, dict) else {}


class SherpaOfflineSession:
    def __init__(self, process: asyncio.subprocess.Process, *, default_sample_rate: int = 16000) -> None:
        self.process = process
        self.default_sample_rate = max(1, int(default_sample_rate or 16000))
        self._seq = 0
        self._write_lock = asyncio.Lock()

    async def transcribe_pcm16(self, pcm16_audio: bytes, *, sample_rate: int | None = None, timeout: float = 60.0) -> str:
        if not pcm16_audio:
            return ""
        if self.process.stdin is None:
            raise SherpaOfflineAsrError("asr_backend_closed", "ASR worker stdin is closed")
        self._seq += 1
        seq = self._seq
        payload = {
            "type": "transcribe",
            "seq": seq,
            "sample_rate": int(sample_rate or self.default_sample_rate),
            "audio_base64": base64.b64encode(pcm16_audio).decode("ascii"),
        }
        async with self._write_lock:
            self.process.stdin.write(json.dumps(payload, ensure_ascii=False).encode("utf-8") + b"\n")
            await self.process.stdin.drain()
            result = await _read_worker(self.process, timeout=timeout)
        if str(result.get("type") or "") == "error":
            error = result.get("error") if isinstance(result.get("error"), dict) else {}
            raise SherpaOfflineAsrError(
                str(error.get("code") or "asr_backend_failed"),
                str(error.get("message") or "ASR worker failed"),
                details=error.get("details") if isinstance(error.get("details"), dict) else {},
            )
        if str(result.get("type") or "") != "transcript":
            raise SherpaOfflineAsrError("asr_backend_failed", "ASR worker returned unexpected response", details={"response": result})
        return str(result.get("text") or "").strip()

    async def close(self) -> None:
        if self.process.returncode is not None:
            return
        try:
            if self.process.stdin is not None:
                self._seq += 1
                self.process.stdin.write(json.dumps({"type": "close", "seq": self._seq}, ensure_ascii=False).encode("utf-8") + b"\n")
                await self.process.stdin.drain()
        except Exception:
            pass
        try:
            await asyncio.wait_for(self.process.wait(), timeout=2.0)
        except Exception:
            try:
                self.process.terminate()
            except Exception:
                pass


async def open_sherpa_offline_session(
    selected_model_id: str = "",
    *,
    sample_rate: int = 16000,
) -> SherpaOfflineSession:
    status = sherpa_offline_backend_status(selected_model_id)
    runtime = status.get("runtime") if isinstance(status.get("runtime"), dict) else {}
    model = status.get("model") if isinstance(status.get("model"), dict) else {}
    model_id = str(status.get("model_id") or "").strip()
    if str(runtime.get("status") or "") != VOICE_RUNTIME_STATUS_READY:
        raise SherpaOfflineAsrError(
            "asr_runtime_not_ready",
            "sherpa-onnx runtime is not installed",
            details={"runtime": runtime},
        )
    if str(model.get("status") or "") != "ready" or not bool(model.get("offline_ready")):
        raise SherpaOfflineAsrError(
            "asr_model_not_ready",
            "sherpa-onnx offline ASR model is not installed",
            details={"model": model, "model_id": model_id},
        )
    config = resolve_installed_voice_model_offline_config(model_id)
    python_path = str(runtime.get("python") or "").strip()
    if not python_path:
        raise SherpaOfflineAsrError("asr_runtime_not_ready", "sherpa-onnx runtime Python is missing", details={"runtime": runtime})
    argv = [
        python_path,
        "-m",
        "cccc.daemon.assistants.sherpa_offline_worker",
        "--engine",
        str(config.get("engine") or ""),
        "--model",
        str(config.get("model") or ""),
        "--tokens",
        str(config.get("tokens") or ""),
        "--sample-rate",
        str(int(config.get("sample_rate") or 16000)),
        "--num-threads",
        str(int(config.get("num_threads") or 2)),
        "--provider",
        str(config.get("provider") or "cpu"),
        "--language",
        str(config.get("language") or "auto"),
        "--use-itn" if bool(config.get("use_itn", True)) else "--no-use-itn",
    ]
    env = os.environ.copy()
    env["CCCC_HOME"] = str(ensure_home())
    source_root = str(Path(__file__).resolve().parents[3])
    env["PYTHONPATH"] = source_root if not env.get("PYTHONPATH") else f"{source_root}{os.pathsep}{env['PYTHONPATH']}"
    env.pop("__PYVENV_LAUNCHER__", None)
    process = await asyncio.create_subprocess_exec(
        *argv,
        stdin=asyncio.subprocess.PIPE,
        stdout=asyncio.subprocess.PIPE,
        stderr=asyncio.subprocess.PIPE,
        env=env,
    )
    session = SherpaOfflineSession(process, default_sample_rate=sample_rate)
    try:
        ready = await _read_worker(process, timeout=30.0)
    except Exception:
        await session.close()
        raise
    if str(ready.get("type") or "") == "error":
        await session.close()
        error = ready.get("error") if isinstance(ready.get("error"), dict) else {}
        raise SherpaOfflineAsrError(
            str(error.get("code") or "asr_backend_failed"),
            str(error.get("message") or "ASR worker failed to start"),
            details=error.get("details") if isinstance(error.get("details"), dict) else {},
        )
    if str(ready.get("type") or "") != "ready":
        await session.close()
        raise SherpaOfflineAsrError("asr_backend_failed", "ASR worker did not become ready", details={"response": ready})
    return session


async def transcribe_sherpa_offline_pcm16(
    pcm16_audio: bytes,
    *,
    selected_model_id: str = "",
    sample_rate: int = 16000,
) -> str:
    if not pcm16_audio:
        return ""
    session = await open_sherpa_offline_session(selected_model_id, sample_rate=sample_rate)
    try:
        return await session.transcribe_pcm16(pcm16_audio, sample_rate=sample_rate)
    finally:
        await session.close()
