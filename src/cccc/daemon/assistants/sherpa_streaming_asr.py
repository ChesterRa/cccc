from __future__ import annotations

import asyncio
import json
import os
from pathlib import Path
from typing import Any

from ...paths import ensure_home
from .voice_models import (
    get_voice_model_status,
    list_voice_models,
    resolve_installed_voice_model_streaming_config,
)
from .voice_runtime_deps import (
    VOICE_RUNTIME_ID_SHERPA_ONNX_STREAMING,
    VOICE_RUNTIME_STATUS_READY,
    get_voice_runtime_status,
)


class SherpaStreamingAsrError(Exception):
    def __init__(self, code: str, message: str, *, details: dict[str, Any] | None = None) -> None:
        super().__init__(message)
        self.code = code
        self.message = message
        self.details = details or {}


def resolve_sherpa_streaming_model_id(selected_model_id: str = "") -> str:
    selected = str(selected_model_id or "").strip()
    if selected:
        return selected
    for item in list_voice_models():
        if not isinstance(item, dict):
            continue
        if str(item.get("runtime_id") or "") != VOICE_RUNTIME_ID_SHERPA_ONNX_STREAMING:
            continue
        if bool(item.get("streaming_ready")):
            return str(item.get("model_id") or "").strip()
    for item in list_voice_models():
        if not isinstance(item, dict):
            continue
        if str(item.get("runtime_id") or "") == VOICE_RUNTIME_ID_SHERPA_ONNX_STREAMING:
            return str(item.get("model_id") or "").strip()
    return ""


def sherpa_streaming_backend_status(selected_model_id: str = "") -> dict[str, Any]:
    runtime = get_voice_runtime_status(VOICE_RUNTIME_ID_SHERPA_ONNX_STREAMING)
    model_id = resolve_sherpa_streaming_model_id(selected_model_id)
    model = get_voice_model_status(model_id) if model_id else {}
    ready = (
        str(runtime.get("status") or "") == VOICE_RUNTIME_STATUS_READY
        and str(model.get("status") or "") == "ready"
        and bool(model.get("streaming_ready"))
    )
    return {
        "runtime": runtime,
        "model_id": model_id,
        "model": model,
        "ready": ready,
    }


class SherpaStreamingSession:
    def __init__(self, process: asyncio.subprocess.Process) -> None:
        self.process = process
        self._write_lock = asyncio.Lock()

    async def send(self, payload: dict[str, Any]) -> None:
        if self.process.stdin is None:
            raise SherpaStreamingAsrError("asr_backend_closed", "ASR worker stdin is closed")
        data = json.dumps(payload, ensure_ascii=False).encode("utf-8") + b"\n"
        async with self._write_lock:
            self.process.stdin.write(data)
            await self.process.stdin.drain()

    async def receive(self, timeout: float | None = None) -> dict[str, Any]:
        if self.process.stdout is None:
            raise SherpaStreamingAsrError("asr_backend_closed", "ASR worker stdout is closed")
        try:
            line = await asyncio.wait_for(self.process.stdout.readline(), timeout=timeout)
        except asyncio.TimeoutError as exc:
            raise SherpaStreamingAsrError("asr_backend_timeout", "ASR worker timed out") from exc
        if not line:
            stderr = ""
            if self.process.stderr is not None:
                try:
                    raw = await asyncio.wait_for(self.process.stderr.read(), timeout=0.1)
                    stderr = raw.decode("utf-8", errors="replace")[-4000:]
                except Exception:
                    stderr = ""
            raise SherpaStreamingAsrError(
                "asr_backend_closed",
                "ASR worker exited",
                details={"returncode": self.process.returncode, "stderr": stderr},
            )
        try:
            payload = json.loads(line.decode("utf-8"))
        except Exception as exc:
            raise SherpaStreamingAsrError(
                "asr_backend_invalid_response",
                "ASR worker returned invalid JSON",
                details={"line": line.decode("utf-8", errors="replace")[:1000]},
            ) from exc
        return payload if isinstance(payload, dict) else {}

    async def close(self) -> None:
        if self.process.returncode is not None:
            return
        try:
            if self.process.stdin is not None:
                self.process.stdin.close()
        except Exception:
            pass
        try:
            await asyncio.wait_for(self.process.wait(), timeout=2.0)
        except Exception:
            try:
                self.process.terminate()
            except Exception:
                pass


async def open_sherpa_streaming_session(selected_model_id: str = "") -> SherpaStreamingSession:
    status = sherpa_streaming_backend_status(selected_model_id)
    runtime = status.get("runtime") if isinstance(status.get("runtime"), dict) else {}
    model = status.get("model") if isinstance(status.get("model"), dict) else {}
    model_id = str(status.get("model_id") or "").strip()
    if str(runtime.get("status") or "") != VOICE_RUNTIME_STATUS_READY:
        raise SherpaStreamingAsrError(
            "asr_runtime_not_ready",
            "sherpa-onnx streaming runtime is not installed",
            details={"runtime": runtime},
        )
    if str(model.get("status") or "") != "ready" or not bool(model.get("streaming_ready")):
        raise SherpaStreamingAsrError(
            "asr_model_not_ready",
            "sherpa-onnx streaming model is not installed",
            details={"model": model, "model_id": model_id},
        )
    config = resolve_installed_voice_model_streaming_config(model_id)
    python_path = str(runtime.get("python") or "").strip()
    if not python_path:
        raise SherpaStreamingAsrError("asr_runtime_not_ready", "sherpa-onnx runtime Python is missing", details={"runtime": runtime})
    worker_module = "cccc.daemon.assistants.sherpa_streaming_worker"
    argv = [
        python_path,
        "-m",
        worker_module,
        "--engine",
        str(config.get("engine") or ""),
        "--tokens",
        str(config.get("tokens") or ""),
        "--sample-rate",
        str(int(config.get("sample_rate") or 16000)),
        "--num-threads",
        str(int(config.get("num_threads") or 2)),
        "--provider",
        str(config.get("provider") or "cpu"),
    ]
    if config.get("model"):
        argv.extend(["--model", str(config["model"])])
    if config.get("encoder"):
        argv.extend(["--encoder", str(config["encoder"])])
    if config.get("decoder"):
        argv.extend(["--decoder", str(config["decoder"])])
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
    session = SherpaStreamingSession(process)
    await session.send({"type": "start", "seq": 0})
    ready = await session.receive(timeout=30.0)
    if str(ready.get("type") or "") == "error":
        error = ready.get("error") if isinstance(ready.get("error"), dict) else {}
        raise SherpaStreamingAsrError(
            str(error.get("code") or "asr_backend_failed"),
            str(error.get("message") or "ASR worker failed to start"),
            details=error.get("details") if isinstance(error.get("details"), dict) else {},
        )
    if str(ready.get("type") or "") != "ready":
        raise SherpaStreamingAsrError("asr_backend_failed", "ASR worker did not become ready", details={"response": ready})
    return session
