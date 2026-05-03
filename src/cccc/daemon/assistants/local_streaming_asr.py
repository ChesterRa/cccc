from __future__ import annotations

from typing import Any

from .sherpa_offline_asr import SherpaOfflineAsrError, open_sherpa_offline_session, transcribe_sherpa_offline_pcm16
from .sherpa_streaming_asr import SherpaStreamingAsrError, open_sherpa_streaming_session, transcribe_sherpa_streaming_pcm16
from .voice_models import get_voice_model_status
from .voice_runtime_deps import VOICE_RUNTIME_ID_SHERPA_ONNX_STREAMING


class LocalStreamingAsrError(Exception):
    def __init__(self, code: str, message: str, *, details: dict[str, Any] | None = None) -> None:
        super().__init__(message)
        self.code = code
        self.message = message
        self.details = details or {}


class LocalStreamingAsrSession:
    def __init__(self, inner: Any) -> None:
        self._inner = inner

    async def send(self, payload: dict[str, Any]) -> None:
        try:
            await self._inner.send(payload)
        except SherpaStreamingAsrError as exc:
            raise LocalStreamingAsrError(exc.code, exc.message, details=exc.details) from exc

    async def receive(self, timeout: float | None = None) -> dict[str, Any]:
        try:
            return await self._inner.receive(timeout=timeout)
        except SherpaStreamingAsrError as exc:
            raise LocalStreamingAsrError(exc.code, exc.message, details=exc.details) from exc

    async def close(self) -> None:
        await self._inner.close()


class LocalOfflineAsrSession:
    def __init__(self, inner: Any) -> None:
        self._inner = inner

    async def transcribe_pcm16(self, pcm16_audio: bytes, *, sample_rate: int = 16000) -> str:
        try:
            return await self._inner.transcribe_pcm16(pcm16_audio, sample_rate=sample_rate)
        except SherpaOfflineAsrError as exc:
            raise LocalStreamingAsrError(exc.code, exc.message, details=exc.details) from exc

    async def close(self) -> None:
        await self._inner.close()


async def open_local_streaming_asr_session(selected_model_id: str = "") -> Any:
    selected = str(selected_model_id or "").strip()
    runtime_id = str((get_voice_model_status(selected) if selected else {}).get("runtime_id") or "").strip()
    try:
        if runtime_id in {"", VOICE_RUNTIME_ID_SHERPA_ONNX_STREAMING}:
            return LocalStreamingAsrSession(await open_sherpa_streaming_session(selected))
        raise LocalStreamingAsrError(
            "asr_runtime_unsupported",
            f"unsupported local ASR runtime: {runtime_id}",
            details={"model_id": selected, "runtime_id": runtime_id},
        )
    except SherpaStreamingAsrError as exc:
        raise LocalStreamingAsrError(exc.code, exc.message, details=exc.details) from exc


async def open_local_offline_asr_session(selected_model_id: str = "", *, sample_rate: int = 16000) -> LocalOfflineAsrSession:
    selected = str(selected_model_id or "").strip()
    model_status = get_voice_model_status(selected) if selected else {}
    runtime_id = str(model_status.get("runtime_id") or "").strip()
    try:
        if runtime_id in {"", VOICE_RUNTIME_ID_SHERPA_ONNX_STREAMING}:
            if not bool(model_status.get("offline")):
                raise LocalStreamingAsrError(
                    "asr_model_not_offline",
                    "selected local ASR model does not support offline transcription",
                    details={"model_id": selected, "model": model_status},
                )
            return LocalOfflineAsrSession(await open_sherpa_offline_session(selected, sample_rate=sample_rate))
        raise LocalStreamingAsrError(
            "asr_runtime_unsupported",
            f"unsupported local ASR runtime: {runtime_id}",
            details={"model_id": selected, "runtime_id": runtime_id},
        )
    except SherpaOfflineAsrError as exc:
        raise LocalStreamingAsrError(exc.code, exc.message, details=exc.details) from exc


async def transcribe_local_streaming_pcm16(
    pcm16_audio: bytes,
    *,
    selected_model_id: str = "",
    sample_rate: int = 16000,
) -> str:
    selected = str(selected_model_id or "").strip()
    model_status = get_voice_model_status(selected) if selected else {}
    runtime_id = str(model_status.get("runtime_id") or "").strip()
    try:
        if runtime_id in {"", VOICE_RUNTIME_ID_SHERPA_ONNX_STREAMING}:
            if bool(model_status.get("offline")):
                return await transcribe_sherpa_offline_pcm16(
                    pcm16_audio,
                    selected_model_id=selected,
                    sample_rate=sample_rate,
                )
            return await transcribe_sherpa_streaming_pcm16(
                pcm16_audio,
                selected_model_id=selected,
                sample_rate=sample_rate,
            )
        raise LocalStreamingAsrError(
            "asr_runtime_unsupported",
            f"unsupported local ASR runtime: {runtime_id}",
            details={"model_id": selected, "runtime_id": runtime_id},
        )
    except SherpaStreamingAsrError as exc:
        raise LocalStreamingAsrError(exc.code, exc.message, details=exc.details) from exc
    except SherpaOfflineAsrError as exc:
        raise LocalStreamingAsrError(exc.code, exc.message, details=exc.details) from exc
