"""First-party Voice Secretary local ASR service process.

The daemon owns this process, but ASR execution stays outside the daemon
through an explicit command adapter. This keeps heavy model runtimes optional
and makes "no ASR backend configured" an honest unavailable state.
"""

from __future__ import annotations

import argparse
import base64
import json
import os
import shlex
import subprocess
import tempfile
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from typing import Any

from ...util.fs import atomic_write_json
from ...util.process import resolve_subprocess_argv
from ...util.time import utc_now_iso
from .voice_models import get_voice_model_status


SERVICE_SCHEMA = 1
ASSISTANT_ID = "voice_secretary"
DEFAULT_HOST = "127.0.0.1"
DEFAULT_PORT = 0
DEFAULT_ASR_TIMEOUT_SECONDS = 90
DEFAULT_MAX_AUDIO_BYTES = 25 * 1024 * 1024


class AsrServiceError(Exception):
    def __init__(self, code: str, message: str, *, details: dict[str, Any] | None = None) -> None:
        super().__init__(message)
        self.code = code
        self.message = message
        self.details = details or {}


def _safe_int_env(name: str, fallback: int, *, minimum: int, maximum: int) -> int:
    try:
        value = int(str(os.environ.get(name) or "").strip())
    except Exception:
        value = fallback
    return max(minimum, min(maximum, value))


def _audio_suffix(mime_type: str) -> str:
    raw = str(mime_type or "").split(";", 1)[0].strip().lower()
    if raw == "audio/wav" or raw == "audio/x-wav":
        return ".wav"
    if raw == "audio/mpeg" or raw == "audio/mp3":
        return ".mp3"
    if raw == "audio/mp4" or raw == "audio/aac":
        return ".m4a"
    if raw == "audio/ogg":
        return ".ogg"
    if raw == "audio/webm":
        return ".webm"
    return ".audio"


def _command_argv(raw_command: str, *, audio_path: Path, mime_type: str, language: str) -> list[str]:
    raw = str(raw_command or "").strip()
    if not raw:
        return []
    has_path_placeholder = "{audio_path}" in raw or "{input_path}" in raw or "{input}" in raw
    rendered = (
        raw.replace("{audio_path}", str(audio_path))
        .replace("{input_path}", str(audio_path))
        .replace("{input}", str(audio_path))
        .replace("{mime_type}", mime_type)
        .replace("{language}", language)
    )
    argv = shlex.split(rendered)
    if not has_path_placeholder:
        argv.append(str(audio_path))
    return argv


def _parse_transcript_stdout(stdout: str) -> str:
    text = str(stdout or "").strip()
    if not text:
        return ""
    try:
        payload = json.loads(text)
    except Exception:
        return text
    if isinstance(payload, dict):
        for key in ("text", "transcript", "result"):
            value = payload.get(key)
            if isinstance(value, str) and value.strip():
                return value.strip()
    return text


def _parse_asr_stderr(stderr: str) -> dict[str, Any]:
    text = str(stderr or "").strip()
    if not text:
        return {}
    try:
        payload = json.loads(text)
    except Exception:
        return {}
    if not isinstance(payload, dict):
        return {}
    error = payload.get("error")
    return dict(error) if isinstance(error, dict) else {}


def _run_asr(audio_bytes: bytes, *, mime_type: str, language: str) -> tuple[str, dict[str, Any]]:
    mock_text = str(os.environ.get("CCCC_VOICE_SECRETARY_ASR_MOCK_TEXT") or "").strip()
    if mock_text:
        return mock_text, {"backend": "env_mock"}

    command = str(os.environ.get("CCCC_VOICE_SECRETARY_ASR_COMMAND") or "").strip()
    managed_command = str(os.environ.get("CCCC_VOICE_SECRETARY_MANAGED_ASR_COMMAND") or "").strip()
    managed_model_id = str(os.environ.get("CCCC_VOICE_SECRETARY_MANAGED_MODEL_ID") or "").strip()
    if not command:
        command = managed_command
    if not command:
        managed_model = get_voice_model_status(managed_model_id) if managed_model_id else {}
        raise AsrServiceError(
            "asr_backend_unavailable",
            "assistant_service_local_asr has no active transcription backend",
            details={
                "managed_model": managed_model,
                "hint": "Use Browser ASR until the streaming ASR backend is connected.",
            },
        )

    suffix = _audio_suffix(mime_type)
    timeout = _safe_int_env(
        "CCCC_VOICE_SECRETARY_ASR_TIMEOUT_SECONDS",
        DEFAULT_ASR_TIMEOUT_SECONDS,
        minimum=1,
        maximum=600,
    )
    tmp_path = Path("")
    try:
        fd, raw_tmp = tempfile.mkstemp(prefix="cccc-voice-secretary-", suffix=suffix)
        tmp_path = Path(raw_tmp)
        with os.fdopen(fd, "wb") as handle:
            handle.write(audio_bytes)
        argv = _command_argv(command, audio_path=tmp_path, mime_type=mime_type, language=language)
        if not argv:
            raise AsrServiceError("asr_backend_unavailable", "ASR command is empty")
        if argv and not Path(str(argv[0])).exists():
            argv = resolve_subprocess_argv(argv)
        env = os.environ.copy()
        env.pop("__PYVENV_LAUNCHER__", None)
        env["CCCC_VOICE_AUDIO_PATH"] = str(tmp_path)
        env["CCCC_VOICE_MIME_TYPE"] = mime_type
        env["CCCC_VOICE_LANGUAGE"] = language
        completed = subprocess.run(
            argv,
            capture_output=True,
            check=False,
            env=env,
            text=True,
            timeout=timeout,
        )
    except subprocess.TimeoutExpired as exc:
        raise AsrServiceError(
            "asr_backend_timeout",
            f"ASR command timed out after {timeout}s",
            details={"timeout_seconds": timeout},
        ) from exc
    except FileNotFoundError as exc:
        raise AsrServiceError(
            "asr_backend_command_not_found",
            str(exc),
            details={"command": command},
        ) from exc
    finally:
        if tmp_path:
            try:
                tmp_path.unlink()
            except Exception:
                pass

    if completed.returncode != 0:
        structured_error = _parse_asr_stderr(completed.stderr)
        if structured_error:
            code = str(structured_error.get("code") or "asr_backend_failed").strip() or "asr_backend_failed"
            message = str(structured_error.get("message") or "ASR command failed").strip() or "ASR command failed"
            details = structured_error.get("details") if isinstance(structured_error.get("details"), dict) else {}
            raise AsrServiceError(
                code,
                message,
                details={
                    **details,
                    "returncode": completed.returncode,
                    "stderr": str(completed.stderr or "").strip()[:4000],
                },
            )
        raise AsrServiceError(
            "asr_backend_failed",
            "ASR command failed",
            details={
                "returncode": completed.returncode,
                "stderr": str(completed.stderr or "").strip()[:4000],
            },
        )
    transcript = _parse_transcript_stdout(completed.stdout)
    if not transcript:
        raise AsrServiceError("asr_empty_transcript", "ASR command returned empty transcript")
    return transcript, {"backend": "command", "command": shlex.split(command)[0] if command else ""}


class VoiceSecretaryServiceContext:
    def __init__(self, *, group_id: str, state_path: Path, host: str, port: int) -> None:
        self.group_id = group_id
        self.state_path = state_path
        self.host = host
        self.port = port
        self.status = "starting"
        self.last_error: dict[str, Any] = {}
        self._lock = threading.Lock()

    def snapshot(self) -> dict[str, Any]:
        managed_model_id = str(os.environ.get("CCCC_VOICE_SECRETARY_MANAGED_MODEL_ID") or "").strip()
        with self._lock:
            return {
                "schema": SERVICE_SCHEMA,
                "assistant_id": ASSISTANT_ID,
                "group_id": self.group_id,
                "pid": os.getpid(),
                "host": self.host,
                "port": self.port,
                "status": self.status,
                "asr_command_configured": bool(str(os.environ.get("CCCC_VOICE_SECRETARY_ASR_COMMAND") or "").strip()),
                "managed_asr_command_configured": bool(str(os.environ.get("CCCC_VOICE_SECRETARY_MANAGED_ASR_COMMAND") or "").strip()),
                "asr_mock_configured": bool(str(os.environ.get("CCCC_VOICE_SECRETARY_ASR_MOCK_TEXT") or "").strip()),
                "selected_model_id": managed_model_id,
                "managed_model": get_voice_model_status(managed_model_id) if managed_model_id else {},
                "last_error": dict(self.last_error),
                "updated_at": utc_now_iso(),
            }

    def write_state(self, *, status: str | None = None, last_error: dict[str, Any] | None = None) -> None:
        managed_model_id = str(os.environ.get("CCCC_VOICE_SECRETARY_MANAGED_MODEL_ID") or "").strip()
        with self._lock:
            if status:
                self.status = status
            if last_error is not None:
                self.last_error = dict(last_error)
            payload = {
                "schema": SERVICE_SCHEMA,
                "assistant_id": ASSISTANT_ID,
                "group_id": self.group_id,
                "pid": os.getpid(),
                "host": self.host,
                "port": self.port,
                "status": self.status,
                "asr_command_configured": bool(str(os.environ.get("CCCC_VOICE_SECRETARY_ASR_COMMAND") or "").strip()),
                "managed_asr_command_configured": bool(str(os.environ.get("CCCC_VOICE_SECRETARY_MANAGED_ASR_COMMAND") or "").strip()),
                "asr_mock_configured": bool(str(os.environ.get("CCCC_VOICE_SECRETARY_ASR_MOCK_TEXT") or "").strip()),
                "selected_model_id": managed_model_id,
                "managed_model": get_voice_model_status(managed_model_id) if managed_model_id else {},
                "last_error": dict(self.last_error),
                "updated_at": utc_now_iso(),
            }
        self.state_path.parent.mkdir(parents=True, exist_ok=True)
        atomic_write_json(self.state_path, payload, indent=2)


class VoiceSecretaryHTTPServer(ThreadingHTTPServer):
    service_context: VoiceSecretaryServiceContext


class VoiceSecretaryHandler(BaseHTTPRequestHandler):
    server: VoiceSecretaryHTTPServer
    protocol_version = "HTTP/1.1"

    def log_message(self, format: str, *args: Any) -> None:
        return

    def _json(self, status_code: int, payload: dict[str, Any]) -> None:
        body = json.dumps(payload, ensure_ascii=False).encode("utf-8")
        self.send_response(status_code)
        self.send_header("Content-Type", "application/json; charset=utf-8")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self) -> None:
        if self.path != "/health":
            self._json(404, {"ok": False, "error": {"code": "not_found", "message": "not found"}})
            return
        self._json(200, {"ok": True, "result": self.server.service_context.snapshot()})

    def do_POST(self) -> None:
        if self.path != "/v1/transcribe":
            self._json(404, {"ok": False, "error": {"code": "not_found", "message": "not found"}})
            return
        try:
            content_length = int(str(self.headers.get("Content-Length") or "0"))
        except Exception:
            content_length = 0
        max_audio_bytes = _safe_int_env(
            "CCCC_VOICE_SECRETARY_MAX_AUDIO_BYTES",
            DEFAULT_MAX_AUDIO_BYTES,
            minimum=1,
            maximum=200 * 1024 * 1024,
        )
        max_body_bytes = max_audio_bytes * 2
        if content_length <= 0 or content_length > max_body_bytes:
            self._json(
                413,
                {
                    "ok": False,
                    "error": {
                        "code": "audio_payload_too_large",
                        "message": "audio payload is empty or too large",
                        "details": {"max_audio_bytes": max_audio_bytes},
                    },
                },
            )
            return
        try:
            payload = json.loads(self.rfile.read(content_length).decode("utf-8"))
        except Exception as exc:
            self._json(400, {"ok": False, "error": {"code": "invalid_json", "message": str(exc)}})
            return
        audio_b64 = str(payload.get("audio_b64") or payload.get("audio_base64") or "").strip()
        if "," in audio_b64 and audio_b64.split(",", 1)[0].startswith("data:"):
            audio_b64 = audio_b64.split(",", 1)[1]
        try:
            audio_bytes = base64.b64decode(audio_b64, validate=True)
        except Exception as exc:
            self._json(400, {"ok": False, "error": {"code": "invalid_audio_base64", "message": str(exc)}})
            return
        if not audio_bytes or len(audio_bytes) > max_audio_bytes:
            self._json(
                413,
                {
                    "ok": False,
                    "error": {
                        "code": "audio_payload_too_large",
                        "message": "audio payload is empty or too large",
                        "details": {"max_audio_bytes": max_audio_bytes},
                    },
                },
            )
            return

        mime_type = str(payload.get("mime_type") or "application/octet-stream").strip() or "application/octet-stream"
        language = str(payload.get("language") or "").strip()
        self.server.service_context.write_state(status="working", last_error={})
        try:
            transcript, meta = _run_asr(audio_bytes, mime_type=mime_type, language=language)
        except AsrServiceError as exc:
            error = {"code": exc.code, "message": exc.message, "details": exc.details}
            self.server.service_context.write_state(status="failed", last_error=error)
            self._json(503, {"ok": False, "error": error})
            return
        except Exception as exc:
            error = {"code": "asr_backend_failed", "message": str(exc), "details": {}}
            self.server.service_context.write_state(status="failed", last_error=error)
            self._json(503, {"ok": False, "error": error})
            return
        self.server.service_context.write_state(status="running", last_error={})
        self._json(
            200,
            {
                "ok": True,
                "result": {
                    "transcript": transcript,
                    "mime_type": mime_type,
                    "language": language,
                    "bytes": len(audio_bytes),
                    "service": self.server.service_context.snapshot(),
                    "asr": meta,
                },
            },
        )


def _heartbeat(ctx: VoiceSecretaryServiceContext, stop_event: threading.Event) -> None:
    while not stop_event.wait(5.0):
        try:
            ctx.write_state()
        except Exception:
            pass


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Run the CCCC Voice Secretary local ASR service.")
    parser.add_argument("--group-id", required=True)
    parser.add_argument("--state-path", required=True)
    parser.add_argument("--host", default=DEFAULT_HOST)
    parser.add_argument("--port", type=int, default=DEFAULT_PORT)
    args = parser.parse_args(argv)

    server = VoiceSecretaryHTTPServer((args.host, int(args.port)), VoiceSecretaryHandler)
    host, port = server.server_address[:2]
    ctx = VoiceSecretaryServiceContext(
        group_id=str(args.group_id),
        state_path=Path(args.state_path),
        host=str(host),
        port=int(port),
    )
    server.service_context = ctx
    stop_event = threading.Event()
    heartbeat = threading.Thread(target=_heartbeat, args=(ctx, stop_event), daemon=True)
    ctx.write_state(status="running", last_error={})
    heartbeat.start()
    try:
        server.serve_forever(poll_interval=0.5)
    finally:
        stop_event.set()
        ctx.write_state(status="stopped")
        server.server_close()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
