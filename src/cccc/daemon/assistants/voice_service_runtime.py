"""Daemon-side supervisor/client for the Voice Secretary service process."""

from __future__ import annotations

import base64
import json
import os
import subprocess
import sys
import time
import urllib.error
import urllib.request
from pathlib import Path
from typing import Any

from ...kernel.group import Group
from ...paths import ensure_home
from ...util.fs import atomic_write_json, read_json
from ...util.process import (
    pid_is_alive,
    resolve_background_python_argv,
    supervised_process_popen_kwargs,
    terminate_pid,
)
from ...util.time import utc_now_iso
from .voice_models import get_voice_model_status, resolve_installed_voice_model_command


SERVICE_STATE_SCHEMA = 1
SERVICE_STATE_FILENAME = "voice_secretary_service.json"
SERVICE_LOG_FILENAME = "voice_secretary_service.log"
DEFAULT_START_TIMEOUT_SECONDS = 5.0
DEFAULT_HTTP_TIMEOUT_SECONDS = 120.0


class VoiceServiceRuntimeError(Exception):
    def __init__(self, code: str, message: str, *, details: dict[str, Any] | None = None) -> None:
        super().__init__(message)
        self.code = code
        self.message = message
        self.details = details or {}


def _service_pythonpath() -> str:
    """Return a PYTHONPATH that survives launching the service from group cwd."""
    existing = str(os.environ.get("PYTHONPATH") or "").strip()
    source_root = str(Path(__file__).resolve().parents[3])
    if not existing:
        return source_root
    parts = [item for item in existing.split(os.pathsep) if item]
    if source_root not in parts:
        parts.insert(0, source_root)
    return os.pathsep.join(parts)


def voice_service_state_path(group: Group) -> Path:
    return group.path / "state" / SERVICE_STATE_FILENAME


def voice_service_log_path(group: Group) -> Path:
    return group.path / "logs" / SERVICE_LOG_FILENAME


def _state_pid_alive(state: dict[str, Any]) -> bool:
    try:
        pid = int(state.get("pid") or 0)
    except Exception:
        pid = 0
    return pid > 0 and pid_is_alive(pid)


def read_voice_service_state(group: Group) -> dict[str, Any]:
    state = read_json(voice_service_state_path(group))
    if not isinstance(state, dict):
        state = {}
    if int(state.get("schema") or 0) != SERVICE_STATE_SCHEMA:
        state = {}
    out = dict(state)
    out["alive"] = _state_pid_alive(out)
    return out


def _service_state_ready(state: dict[str, Any], *, pid: int | None = None) -> bool:
    try:
        state_pid = int(state.get("pid") or 0)
        port = int(state.get("port") or 0)
    except Exception:
        return False
    if pid is not None and state_pid != int(pid):
        return False
    host = str(state.get("host") or "").strip()
    status = str(state.get("status") or "").strip()
    return bool(state_pid > 0 and port > 0 and host and status in {"running", "working", "failed"})


def _write_starting_state(group: Group, *, pid: int) -> None:
    selected_model_id = _selected_service_model_id(group)
    managed_model = get_voice_model_status(selected_model_id) if selected_model_id else {}
    state_path = voice_service_state_path(group)
    state_path.parent.mkdir(parents=True, exist_ok=True)
    atomic_write_json(
        state_path,
        {
            "schema": SERVICE_STATE_SCHEMA,
            "assistant_id": "voice_secretary",
            "group_id": group.group_id,
            "pid": pid,
            "host": "127.0.0.1",
            "port": 0,
            "status": "starting",
            "asr_command_configured": bool(str(os.environ.get("CCCC_VOICE_SECRETARY_ASR_COMMAND") or "").strip()),
            "asr_mock_configured": bool(str(os.environ.get("CCCC_VOICE_SECRETARY_ASR_MOCK_TEXT") or "").strip()),
            "selected_model_id": selected_model_id,
            "managed_model": managed_model,
            "last_error": {},
            "updated_at": utc_now_iso(),
        },
        indent=2,
    )


def _selected_service_model_id(group: Group) -> str:
    assistants = group.doc.get("assistants") if isinstance(group.doc.get("assistants"), dict) else {}
    entry = assistants.get("voice_secretary") if isinstance(assistants.get("voice_secretary"), dict) else {}
    config = entry.get("config") if isinstance(entry.get("config"), dict) else {}
    return str(config.get("service_model_id") or "").strip()


def ensure_voice_service(group: Group, *, timeout_seconds: float = DEFAULT_START_TIMEOUT_SECONDS) -> dict[str, Any]:
    current = read_voice_service_state(group)
    if _service_state_ready(current) and bool(current.get("alive")):
        return current

    state_path = voice_service_state_path(group)
    log_path = voice_service_log_path(group)
    state_path.parent.mkdir(parents=True, exist_ok=True)
    log_path.parent.mkdir(parents=True, exist_ok=True)

    env = os.environ.copy()
    env["CCCC_HOME"] = str(ensure_home())
    env["PYTHONPATH"] = _service_pythonpath()
    selected_model_id = _selected_service_model_id(group)
    managed_command = ""
    if not str(env.get("CCCC_VOICE_SECRETARY_ASR_COMMAND") or "").strip():
        managed_command = resolve_installed_voice_model_command(selected_model_id)
        if managed_command:
            env["CCCC_VOICE_SECRETARY_MANAGED_ASR_COMMAND"] = managed_command
    if selected_model_id:
        env["CCCC_VOICE_SECRETARY_MANAGED_MODEL_ID"] = selected_model_id
    argv = resolve_background_python_argv(
        [
            sys.executable,
            "-m",
            "cccc.daemon.assistants.voice_secretary_service",
            "--group-id",
            group.group_id,
            "--state-path",
            str(state_path),
        ]
    )
    try:
        with log_path.open("ab") as log_file:
            proc = subprocess.Popen(
                argv,
                cwd=str(group.path),
                env=env,
                stdin=subprocess.DEVNULL,
                stdout=log_file,
                stderr=log_file,
                **supervised_process_popen_kwargs(),
            )
    except Exception as exc:
        raise VoiceServiceRuntimeError(
            "voice_service_start_failed",
            str(exc),
            details={"argv": argv, "log_path": str(log_path)},
        ) from exc

    _write_starting_state(group, pid=int(proc.pid))
    deadline = time.time() + max(0.1, float(timeout_seconds))
    last_state: dict[str, Any] = {}
    while time.time() < deadline:
        last_state = read_voice_service_state(group)
        if _service_state_ready(last_state, pid=int(proc.pid)) and bool(last_state.get("alive")):
            return last_state
        if proc.poll() is not None:
            break
        time.sleep(0.05)

    last_state = read_voice_service_state(group)
    if _service_state_ready(last_state, pid=int(proc.pid)) and bool(last_state.get("alive")):
        return last_state
    raise VoiceServiceRuntimeError(
        "voice_service_start_timeout",
        "Voice Secretary service did not become ready",
        details={"pid": int(proc.pid), "state": last_state, "log_path": str(log_path)},
    )


def stop_voice_service(group: Group) -> dict[str, Any]:
    state = read_voice_service_state(group)
    try:
        pid = int(state.get("pid") or 0)
    except Exception:
        pid = 0
    stopped = False
    if pid > 0 and bool(state.get("alive")):
        stopped = terminate_pid(pid, timeout_s=2.0, include_group=True, force=True)
    state["alive"] = pid > 0 and pid_is_alive(pid)
    state["stopped"] = stopped
    return state


def _parse_http_error_body(exc: urllib.error.HTTPError) -> dict[str, Any]:
    try:
        body = exc.read().decode("utf-8")
        payload = json.loads(body)
    except Exception:
        return {"code": "voice_service_http_error", "message": str(exc), "details": {"status": exc.code}}
    error = payload.get("error") if isinstance(payload, dict) and isinstance(payload.get("error"), dict) else {}
    return {
        "code": str(error.get("code") or "voice_service_http_error"),
        "message": str(error.get("message") or str(exc)),
        "details": error.get("details") if isinstance(error.get("details"), dict) else {"status": exc.code},
    }


def transcribe_voice_audio(
    group: Group,
    *,
    audio_bytes: bytes,
    mime_type: str,
    language: str = "",
    timeout_seconds: float = DEFAULT_HTTP_TIMEOUT_SECONDS,
) -> dict[str, Any]:
    if not audio_bytes:
        raise VoiceServiceRuntimeError("empty_audio", "audio payload cannot be empty")
    state = ensure_voice_service(group)
    host = str(state.get("host") or "127.0.0.1").strip() or "127.0.0.1"
    try:
        port = int(state.get("port") or 0)
    except Exception:
        port = 0
    if port <= 0:
        raise VoiceServiceRuntimeError("voice_service_unavailable", "Voice Secretary service port is unavailable", details=state)

    payload = {
        "audio_b64": base64.b64encode(audio_bytes).decode("ascii"),
        "mime_type": str(mime_type or "application/octet-stream").strip() or "application/octet-stream",
        "language": str(language or "").strip(),
    }
    body = json.dumps(payload).encode("utf-8")
    req = urllib.request.Request(
        f"http://{host}:{port}/v1/transcribe",
        data=body,
        headers={"Content-Type": "application/json"},
        method="POST",
    )
    try:
        with urllib.request.urlopen(req, timeout=float(timeout_seconds)) as resp:
            raw = resp.read().decode("utf-8")
    except urllib.error.HTTPError as exc:
        error = _parse_http_error_body(exc)
        raise VoiceServiceRuntimeError(
            error["code"],
            error["message"],
            details=error.get("details") if isinstance(error.get("details"), dict) else {},
        ) from exc
    except Exception as exc:
        raise VoiceServiceRuntimeError("voice_service_request_failed", str(exc), details={"service": state}) from exc

    try:
        parsed = json.loads(raw)
    except Exception as exc:
        raise VoiceServiceRuntimeError("voice_service_invalid_response", str(exc), details={"body": raw[:4000]}) from exc
    if not isinstance(parsed, dict) or not bool(parsed.get("ok")):
        error = parsed.get("error") if isinstance(parsed, dict) and isinstance(parsed.get("error"), dict) else {}
        raise VoiceServiceRuntimeError(
            str(error.get("code") or "voice_service_failed"),
            str(error.get("message") or "Voice Secretary service failed"),
            details=error.get("details") if isinstance(error.get("details"), dict) else {},
        )
    result = parsed.get("result") if isinstance(parsed.get("result"), dict) else {}
    transcript = str(result.get("transcript") or "").strip()
    if not transcript:
        raise VoiceServiceRuntimeError("asr_empty_transcript", "Voice Secretary service returned empty transcript")
    return {
        "transcript": transcript,
        "mime_type": str(result.get("mime_type") or mime_type),
        "language": str(result.get("language") or language),
        "bytes": int(result.get("bytes") or len(audio_bytes)),
        "service": result.get("service") if isinstance(result.get("service"), dict) else read_voice_service_state(group),
        "asr": result.get("asr") if isinstance(result.get("asr"), dict) else {},
    }
