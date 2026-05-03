from __future__ import annotations

import asyncio
from typing import Any

from .local_streaming_asr import LocalStreamingAsrError, transcribe_local_streaming_pcm16
from .sherpa_offline_asr import sherpa_offline_backend_status
from .voice_models import get_voice_model_status, list_voice_models


DEFAULT_LIVE_SERVICE_MODEL_ID = "sherpa_onnx_streaming_paraformer_trilingual_zh_cantonese_en"
DEFAULT_FINAL_SERVICE_MODEL_ID = "sherpa_onnx_sense_voice_zh_en_ja_ko_yue_int8"
DEFAULT_SERVICE_MODEL_ID = DEFAULT_FINAL_SERVICE_MODEL_ID
LEGACY_DEFAULT_SERVICE_MODEL_IDS = {
    "",
    DEFAULT_LIVE_SERVICE_MODEL_ID,
}


def effective_service_model_id(value: Any) -> str:
    model_id = str(value or "").strip()
    if model_id in LEGACY_DEFAULT_SERVICE_MODEL_IDS:
        return DEFAULT_SERVICE_MODEL_ID
    return model_id


def effective_live_service_model_id(value: Any) -> str:
    model_id = str(value or "").strip()
    if model_id:
        status = get_voice_model_status(model_id)
        if status.get("streaming") and status.get("streaming_ready"):
            return model_id
    fallback = get_voice_model_status(DEFAULT_LIVE_SERVICE_MODEL_ID)
    if fallback.get("streaming") and fallback.get("streaming_ready"):
        return DEFAULT_LIVE_SERVICE_MODEL_ID
    for item in list_voice_models():
        if isinstance(item, dict) and item.get("streaming") and item.get("streaming_ready"):
            return str(item.get("model_id") or "").strip()
    return ""


def effective_final_service_model_id(value: Any) -> str:
    model_id = effective_service_model_id(value)
    status = get_voice_model_status(model_id) if model_id else {}
    if (
        bool(status.get("offline"))
        and str(status.get("runtime_id") or "") == "sherpa_onnx_streaming"
        and bool(status.get("offline_ready"))
    ):
        return model_id
    default_status = get_voice_model_status(DEFAULT_FINAL_SERVICE_MODEL_ID)
    if (
        bool(default_status.get("offline"))
        and str(default_status.get("runtime_id") or "") == "sherpa_onnx_streaming"
        and bool(default_status.get("offline_ready"))
    ):
        return DEFAULT_FINAL_SERVICE_MODEL_ID
    for item in list_voice_models():
        if not isinstance(item, dict):
            continue
        if (
            item.get("offline")
            and str(item.get("runtime_id") or "") == "sherpa_onnx_streaming"
            and bool(item.get("offline_ready"))
        ):
            return str(item.get("model_id") or "").strip()
    return DEFAULT_FINAL_SERVICE_MODEL_ID


def service_model_status(model_id: str) -> dict[str, Any]:
    selected = effective_service_model_id(model_id)
    return get_voice_model_status(selected) if selected else {}


def service_model_is_offline(model_id: str) -> bool:
    return bool(service_model_status(model_id).get("offline"))


def transcribe_offline_service_pcm16(
    audio_bytes: bytes,
    *,
    selected_model_id: str,
    sample_rate: int = 16000,
    mime_type: str = "application/octet-stream",
    language: str = "",
) -> dict[str, Any]:
    transcript = asyncio.run(
        transcribe_local_streaming_pcm16(
            audio_bytes,
            selected_model_id=selected_model_id,
            sample_rate=sample_rate,
        )
    )
    return {
        "transcript": transcript,
        "mime_type": mime_type,
        "language": language,
        "bytes": len(audio_bytes),
        "service": {
            "selected_model_id": selected_model_id,
            "managed_model": get_voice_model_status(selected_model_id),
            "offline_backend": sherpa_offline_backend_status(selected_model_id),
        },
        "asr": {"backend": "sherpa_onnx_offline"},
    }


__all__ = [
    "DEFAULT_FINAL_SERVICE_MODEL_ID",
    "DEFAULT_LIVE_SERVICE_MODEL_ID",
    "DEFAULT_SERVICE_MODEL_ID",
    "LEGACY_DEFAULT_SERVICE_MODEL_IDS",
    "LocalStreamingAsrError",
    "effective_service_model_id",
    "effective_final_service_model_id",
    "effective_live_service_model_id",
    "service_model_is_offline",
    "service_model_status",
    "transcribe_offline_service_pcm16",
]
