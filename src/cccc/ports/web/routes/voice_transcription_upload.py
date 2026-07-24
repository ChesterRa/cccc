from __future__ import annotations

import base64
import binascii
import json
import os
import tempfile
from dataclasses import dataclass
from pathlib import Path
from typing import Any

import anyio
from fastapi import HTTPException, Request

from ....paths import ensure_home


MAX_AUDIO_BYTES = 100 * 1024 * 1024
MAX_LEGACY_JSON_BYTES = MAX_AUDIO_BYTES * 2


@dataclass(frozen=True)
class VoiceTranscriptionUpload:
    path: Path
    size: int
    mime_type: str
    language: str
    by: str

    def cleanup(self) -> None:
        self.path.unlink(missing_ok=True)


def _error(status_code: int, code: str, message: str) -> HTTPException:
    return HTTPException(
        status_code=status_code,
        detail={"code": code, "message": message, "details": {}},
    )


def _content_length(request: Request) -> int | None:
    raw = str(request.headers.get("content-length") or "").strip()
    if not raw:
        return None
    try:
        value = int(raw)
    except ValueError as exc:
        raise _error(400, "invalid_content_length", "invalid Content-Length header") from exc
    if value < 0:
        raise _error(400, "invalid_content_length", "invalid Content-Length header")
    return value


def _decode_legacy_json(body: bytes) -> tuple[bytes, str, str, str]:
    try:
        payload = json.loads(body.decode("utf-8"))
    except (UnicodeDecodeError, json.JSONDecodeError) as exc:
        raise _error(400, "invalid_json", "request body must be valid JSON") from exc
    if not isinstance(payload, dict):
        raise _error(400, "invalid_request", "request body must be an object")
    encoded = str(payload.get("audio_base64") or payload.get("audio_b64") or "").strip()
    if "," in encoded and encoded.split(",", 1)[0].startswith("data:"):
        encoded = encoded.split(",", 1)[1]
    try:
        audio = base64.b64decode(encoded, validate=True)
    except (binascii.Error, ValueError) as exc:
        raise _error(400, "invalid_audio_base64", "audio_base64 is invalid") from exc
    return (
        audio,
        str(payload.get("mime_type") or "application/octet-stream").strip()
        or "application/octet-stream",
        str(payload.get("language") or "").strip(),
        str(payload.get("by") or "user").strip() or "user",
    )


def _create_upload_path() -> Path:
    upload_dir = ensure_home() / "cache" / "voice-http-uploads"
    upload_dir.mkdir(parents=True, exist_ok=True)
    fd, raw_path = tempfile.mkstemp(prefix="voice-", suffix=".audio", dir=upload_dir)
    os.close(fd)
    return Path(raw_path)


async def receive_voice_transcription(
    request: Request,
    *,
    language: str,
    by: str,
) -> VoiceTranscriptionUpload:
    content_type = str(request.headers.get("content-type") or "application/octet-stream")
    media_type = content_type.split(";", 1)[0].strip().lower() or "application/octet-stream"
    content_length = _content_length(request)
    limit = MAX_LEGACY_JSON_BYTES if media_type == "application/json" else MAX_AUDIO_BYTES
    if content_length is not None and content_length > limit:
        raise _error(413, "audio_too_large", "audio payload exceeds 100 MiB")

    path = _create_upload_path()
    try:
        if media_type == "application/json":
            chunks: list[bytes] = []
            received = 0
            async for chunk in request.stream():
                received += len(chunk)
                if received > MAX_LEGACY_JSON_BYTES:
                    raise _error(413, "audio_too_large", "audio payload exceeds 100 MiB")
                if chunk:
                    chunks.append(chunk)
            body = b"".join(chunks)
            audio, media_type, language, by = _decode_legacy_json(body)
            if not audio:
                raise _error(400, "empty_audio", "audio payload cannot be empty")
            if len(audio) > MAX_AUDIO_BYTES:
                raise _error(413, "audio_too_large", "audio payload exceeds 100 MiB")
            async with await anyio.open_file(path, "wb") as output:
                await output.write(audio)
            size = len(audio)
        else:
            size = 0
            async with await anyio.open_file(path, "wb") as output:
                async for chunk in request.stream():
                    size += len(chunk)
                    if size > MAX_AUDIO_BYTES:
                        raise _error(413, "audio_too_large", "audio payload exceeds 100 MiB")
                    if chunk:
                        await output.write(chunk)
            if size == 0:
                raise _error(400, "empty_audio", "audio payload cannot be empty")
        return VoiceTranscriptionUpload(
            path=path,
            size=size,
            mime_type=media_type,
            language=str(language or "").strip(),
            by=str(by or "user").strip() or "user",
        )
    except BaseException:
        path.unlink(missing_ok=True)
        raise
