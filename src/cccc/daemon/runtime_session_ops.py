"""Durable provider runtime session metadata.

This stores provider-owned session/thread identifiers separately from hot
runner state. Hot runner state describes a live process; runtime session state
describes whether a future actor start can safely resume a provider session.
"""

from __future__ import annotations

import hashlib
import json
import os
import re
import threading
import time
import uuid
from pathlib import Path
from typing import Any, Callable, Dict, Iterable, Optional, Tuple

from ..paths import ensure_home
from ..runners import pty as pty_runner
from .runner_state_ops import pty_state_path
from ..util.fs import atomic_write_json, read_json
from ..util.time import utc_now_iso


_ANSI_RE = re.compile(r"\x1b\[[0-?]*[ -/]*[@-~]")
_ID_RE = r"([A-Za-z0-9][A-Za-z0-9._:-]{7,})"
_CLAUDE_RESUME_RE = re.compile(rf"\bclaude\s+(?:--resume|-r)\s+{_ID_RE}\b")
_CODEX_RESUME_RE = re.compile(rf"\bcodex\s+resume\s+{_ID_RE}\b")
_GEMINI_RESUME_RE = re.compile(rf"\bgemini\s+(?:--resume|-r)\s+{_ID_RE}\b")
_CODEX_SESSION_ID_RE = r"([0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12})"
_CODEX_STATUS_SESSION_RE = re.compile(rf"\bSession:\s*{_CODEX_SESSION_ID_RE}\b")

_NO_RESUME_ENV_VALUES = {"0", "false", "no", "off"}
_DEFAULT_CODEX_STATUS_CAPTURE_SECONDS = 8.0
_DEFAULT_CODEX_STATUS_SUBMIT_DELAY_SECONDS = 1.5
_DEFAULT_PTY_RESUME_VERIFY_SECONDS = 20.0
_DEFAULT_PTY_RESUME_FOREGROUND_VERIFY_SECONDS = 2.0
_CODEX_KNOWN_SUBCOMMANDS = {
    "app-server",
    "completion",
    "debug",
    "exec",
    "help",
    "login",
    "logout",
    "mcp",
    "proto",
    "resume",
    "sandbox",
    "server",
    "status",
}
_GEMINI_KNOWN_SUBCOMMANDS = {
    "auth",
    "extensions",
    "help",
    "mcp",
    "resume",
}


def runtime_session_path(group_id: str, actor_id: str) -> Path:
    return (
        ensure_home()
        / "groups"
        / str(group_id)
        / "state"
        / "runtime_sessions"
        / f"{str(actor_id)}.json"
    )


def read_runtime_session(group_id: str, actor_id: str) -> Dict[str, Any]:
    path = runtime_session_path(group_id, actor_id)
    raw = read_json(path)
    return raw if isinstance(raw, dict) else {}


def write_runtime_session(group_id: str, actor_id: str, payload: Dict[str, Any]) -> None:
    path = runtime_session_path(group_id, actor_id)
    out = dict(payload or {})
    out.setdefault("v", 1)
    out.setdefault("kind", "runtime_session")
    out["group_id"] = str(group_id)
    out["actor_id"] = str(actor_id)
    out.setdefault("updated_at", utc_now_iso())
    atomic_write_json(path, out, indent=2)


def remove_runtime_session(group_id: str, actor_id: str) -> None:
    try:
        runtime_session_path(group_id, actor_id).unlink(missing_ok=True)
    except Exception:
        pass


def _stable_runtime_command_argv(command: Iterable[str]) -> list[str]:
    argv = [str(item) for item in list(command or []) if str(item).strip()]
    if len(argv) < 2 or Path(argv[0]).name != "codex" or argv[1] != "app-server":
        return argv

    stable: list[str] = []
    skip_next = False
    for item in argv:
        if skip_next:
            skip_next = False
            continue
        if item == "--listen":
            stable.append(item)
            skip_next = True
            continue
        if item.startswith("--listen="):
            stable.append("--listen")
            continue
        stable.append(item)
    return stable


def runtime_session_command_fingerprint(command: Iterable[str]) -> str:
    argv = _stable_runtime_command_argv(command)
    raw = json.dumps({"argv": argv}, ensure_ascii=False, sort_keys=True, separators=(",", ":"))
    return hashlib.sha256(raw.encode("utf-8")).hexdigest()


def _model_from_command(command: Iterable[str]) -> str:
    items = [str(item or "").strip() for item in list(command or [])]
    for idx, item in enumerate(items):
        if item in {"-m", "--model"}:
            return str(items[idx + 1] or "").strip() if idx + 1 < len(items) else ""
        if item.startswith("--model="):
            return item.split("=", 1)[1].strip()
    return ""


def _workspace_path(cwd: Path | str) -> str:
    try:
        return str(Path(cwd).expanduser().resolve())
    except Exception:
        return str(cwd or "")


def _codex_status_capture_seconds() -> float:
    value = str(os.environ.get("CCCC_CODEX_PTY_STATUS_CAPTURE_SECONDS") or "").strip()
    if not value:
        return _DEFAULT_CODEX_STATUS_CAPTURE_SECONDS
    try:
        return max(0.0, float(value))
    except Exception:
        return _DEFAULT_CODEX_STATUS_CAPTURE_SECONDS


def _codex_status_submit_delay_seconds() -> float:
    value = str(os.environ.get("CCCC_CODEX_PTY_STATUS_SUBMIT_DELAY_SECONDS") or "").strip()
    if not value:
        return _DEFAULT_CODEX_STATUS_SUBMIT_DELAY_SECONDS
    try:
        return max(0.0, float(value))
    except Exception:
        return _DEFAULT_CODEX_STATUS_SUBMIT_DELAY_SECONDS


def _codex_status_input_payload(*, group_id: str, actor_id: str) -> bytes:
    try:
        bracketed = bool(pty_runner.SUPERVISOR.bracketed_paste_enabled(group_id=group_id, actor_id=actor_id))
    except Exception:
        bracketed = False
    payload = b"/status"
    if bracketed:
        return b"\x1b[200~" + payload + b"\x1b[201~"
    return payload


def _submit_codex_status_command(*, group_id: str, actor_id: str) -> bool:
    payload = _codex_status_input_payload(group_id=group_id, actor_id=actor_id)
    if not pty_runner.SUPERVISOR.write_input(group_id=group_id, actor_id=actor_id, data=payload):
        return False
    delay = _codex_status_submit_delay_seconds()
    if delay > 0:
        time.sleep(delay)
    return bool(pty_runner.SUPERVISOR.write_input(group_id=group_id, actor_id=actor_id, data=b"\r"))


def _pty_resume_verify_seconds() -> float:
    value = str(os.environ.get("CCCC_PTY_RESUME_VERIFY_SECONDS") or "").strip()
    if not value:
        return _DEFAULT_PTY_RESUME_VERIFY_SECONDS
    try:
        return max(0.0, float(value))
    except Exception:
        return _DEFAULT_PTY_RESUME_VERIFY_SECONDS


def _pty_resume_foreground_verify_seconds() -> float:
    value = str(os.environ.get("CCCC_PTY_RESUME_FOREGROUND_VERIFY_SECONDS") or "").strip()
    if not value:
        return _DEFAULT_PTY_RESUME_FOREGROUND_VERIFY_SECONDS
    try:
        return max(0.0, float(value))
    except Exception:
        return _DEFAULT_PTY_RESUME_FOREGROUND_VERIFY_SECONDS


def parse_runtime_resume_hint(text: str | bytes, *, runtime: str = "") -> Dict[str, str]:
    if isinstance(text, bytes):
        source = text.decode("utf-8", errors="replace")
    else:
        source = str(text or "")
    source = _ANSI_RE.sub("", source)
    wanted = str(runtime or "").strip().lower()

    if wanted in {"", "claude"}:
        match = _CLAUDE_RESUME_RE.search(source)
        if match:
            return {
                "runtime": "claude",
                "provider_session_id": match.group(1),
                "resume_command_hint": match.group(0).strip(),
            }

    if wanted in {"", "codex"}:
        match = _CODEX_RESUME_RE.search(source)
        if match:
            return {
                "runtime": "codex",
                "provider_session_id": match.group(1),
                "resume_command_hint": match.group(0).strip(),
            }

    if wanted in {"", "gemini"}:
        match = _GEMINI_RESUME_RE.search(source)
        if match:
            return {
                "runtime": "gemini",
                "provider_session_id": match.group(1),
                "resume_command_hint": match.group(0).strip(),
            }

    return {}


def parse_codex_status_session_id(text: str | bytes) -> str:
    if isinstance(text, bytes):
        source = text.decode("utf-8", errors="replace")
    else:
        source = str(text or "")
    source = _ANSI_RE.sub("", source)
    match = _CODEX_STATUS_SESSION_RE.search(source)
    return str(match.group(1) if match else "").strip()


def _looks_like_resume_failure(text: str | bytes) -> bool:
    if isinstance(text, bytes):
        source = text.decode("utf-8", errors="replace")
    else:
        source = str(text or "")
    lowered = _ANSI_RE.sub("", source).lower()
    if not lowered:
        return False
    markers = (
        "no conversation found",
        "conversation not found",
        "session not found",
        "thread not found",
        "resume id not found",
        "resume session not found",
        "could not find conversation",
        "could not resume",
        "failed to resume",
        "invalid session",
        "invalid thread",
    )
    return any(marker in lowered for marker in markers)


def record_pty_runtime_session(
    *,
    group_id: str,
    actor_id: str,
    runtime: str,
    cwd: Path | str,
    command: Iterable[str],
    provider_session_id: str,
    resume_command_hint: str = "",
    captured_from: str,
    status: str = "usable",
    resume_eligible: bool = True,
    last_resume_error: str = "",
) -> Dict[str, Any]:
    now = utc_now_iso()
    payload: Dict[str, Any] = {
        "v": 1,
        "kind": "runtime_session",
        "group_id": str(group_id),
        "actor_id": str(actor_id),
        "runtime": str(runtime or "").strip().lower(),
        "runner": "pty",
        "workspace_path": _workspace_path(cwd),
        "command_fingerprint": runtime_session_command_fingerprint(command),
        "model": _model_from_command(command),
        "provider_session_id": str(provider_session_id or "").strip(),
        "provider_thread_id": "",
        "resume_command_hint": str(resume_command_hint or "").strip(),
        "captured_from": str(captured_from or "pty").strip(),
        "status": str(status or "usable").strip() or "usable",
        "resume_eligible": bool(resume_eligible),
        "last_seen_at": now,
        "last_resume_attempt_at": "",
        "last_resume_error": str(last_resume_error or "").strip(),
        "failure_count": 0,
        "updated_at": now,
    }
    write_runtime_session(str(group_id), str(actor_id), payload)
    return payload


def record_headless_runtime_session(
    *,
    group_id: str,
    actor_id: str,
    runtime: str,
    cwd: Path | str,
    model: str = "",
    command: Iterable[str] = (),
    provider_session_id: str = "",
    provider_thread_id: str = "",
    status: str = "usable",
    captured_from: str = "headless",
    resume_eligible: bool = True,
    last_resume_error: str = "",
) -> Dict[str, Any]:
    now = utc_now_iso()
    payload: Dict[str, Any] = {
        "v": 1,
        "kind": "runtime_session",
        "group_id": str(group_id),
        "actor_id": str(actor_id),
        "runtime": str(runtime or "").strip().lower(),
        "runner": "headless",
        "workspace_path": _workspace_path(cwd),
        "command_fingerprint": runtime_session_command_fingerprint(command),
        "model": str(model or "").strip(),
        "provider_session_id": str(provider_session_id or "").strip(),
        "provider_thread_id": str(provider_thread_id or "").strip(),
        "resume_command_hint": "",
        "captured_from": str(captured_from or "headless"),
        "status": str(status or "usable").strip() or "usable",
        "resume_eligible": bool(resume_eligible),
        "last_seen_at": now,
        "last_resume_attempt_at": "",
        "last_resume_error": str(last_resume_error or "").strip(),
        "failure_count": 0,
        "updated_at": now,
    }
    write_runtime_session(str(group_id), str(actor_id), payload)
    return payload


def record_codex_app_thread_runtime_session(
    *,
    group_id: str,
    actor_id: str,
    cwd: Path | str,
    command: Iterable[str] = (),
    model: str = "",
    provider_thread_id: str = "",
    runner: str = "headless",
    status: str = "usable",
    captured_from: str = "app_server_thread_start",
    resume_eligible: bool = True,
    last_resume_error: str = "",
) -> Dict[str, Any]:
    now = utc_now_iso()
    runner_kind = str(runner or "headless").strip().lower() or "headless"
    if runner_kind not in {"pty", "headless"}:
        runner_kind = "headless"
    payload: Dict[str, Any] = {
        "v": 1,
        "kind": "runtime_session",
        "group_id": str(group_id),
        "actor_id": str(actor_id),
        "runtime": "codex",
        "runner": runner_kind,
        "workspace_path": _workspace_path(cwd),
        "command_fingerprint": runtime_session_command_fingerprint(command),
        "model": str(model or "").strip(),
        "provider_session_id": "",
        "provider_thread_id": str(provider_thread_id or "").strip(),
        "resume_command_hint": "",
        "captured_from": str(captured_from or "app_server_thread_start"),
        "status": str(status or "usable").strip() or "usable",
        "resume_eligible": bool(resume_eligible),
        "last_seen_at": now,
        "last_resume_attempt_at": "",
        "last_resume_error": str(last_resume_error or "").strip(),
        "failure_count": 0,
        "updated_at": now,
    }
    write_runtime_session(str(group_id), str(actor_id), payload)
    return payload


def _runtime_resume_disabled() -> bool:
    return str(os.environ.get("CCCC_RUNTIME_RESUME", "")).strip().lower() in _NO_RESUME_ENV_VALUES


def runtime_resume_enabled() -> bool:
    return not _runtime_resume_disabled()


def _has_claude_session_control(args: list[str]) -> bool:
    for item in args[1:]:
        value = str(item or "").strip()
        if value in {"--resume", "-r", "--continue", "-c", "--session-id"}:
            return True
        if value.startswith("--resume=") or value.startswith("--session-id="):
            return True
    return False


def _has_gemini_session_control(args: list[str]) -> bool:
    for item in args[1:]:
        value = str(item or "").strip()
        if value in {"--resume", "-r", "--session-id"}:
            return True
        if value.startswith("--resume=") or value.startswith("--session-id="):
            return True
    return False


def _claude_resume_command(base_command: list[str], session_id: str) -> list[str]:
    if not base_command or _has_claude_session_control(base_command):
        return []
    return [base_command[0], "--resume", session_id, *base_command[1:]]


def _gemini_resume_command(base_command: list[str], session_id: str) -> list[str]:
    if not base_command or _has_gemini_session_control(base_command):
        return []
    rest = [str(item or "").strip() for item in base_command[1:]]
    if any(item in _GEMINI_KNOWN_SUBCOMMANDS for item in rest):
        return []
    return [base_command[0], "--resume", session_id, *base_command[1:]]


def _codex_resume_command(base_command: list[str], session_id: str) -> list[str]:
    if not base_command:
        return []
    rest = [str(item or "").strip() for item in base_command[1:]]
    if any(item in _CODEX_KNOWN_SUBCOMMANDS for item in rest):
        return []
    return [base_command[0], *base_command[1:], "resume", session_id]


def _claude_initial_session_command(
    *,
    group_id: str,
    actor_id: str,
    cwd: Path,
    base_command: list[str],
) -> Tuple[list[str], Optional[Dict[str, Any]]]:
    if not base_command or _has_claude_session_control(base_command):
        return base_command, None
    session_id = str(uuid.uuid4())
    doc = record_pty_runtime_session(
        group_id=group_id,
        actor_id=actor_id,
        runtime="claude",
        cwd=cwd,
        command=base_command,
        provider_session_id=session_id,
        resume_command_hint=f"claude --resume {session_id}",
        captured_from="claude_generated_session_id",
        status="usable",
        resume_eligible=True,
    )
    return [base_command[0], "--session-id", session_id, *base_command[1:]], doc


def _gemini_initial_session_command(
    *,
    group_id: str,
    actor_id: str,
    cwd: Path,
    base_command: list[str],
) -> Tuple[list[str], Optional[Dict[str, Any]]]:
    if not base_command or _has_gemini_session_control(base_command):
        return base_command, None
    rest = [str(item or "").strip() for item in base_command[1:]]
    if any(item in _GEMINI_KNOWN_SUBCOMMANDS for item in rest):
        return base_command, None
    session_id = str(uuid.uuid4())
    doc = record_pty_runtime_session(
        group_id=group_id,
        actor_id=actor_id,
        runtime="gemini",
        cwd=cwd,
        command=base_command,
        provider_session_id=session_id,
        resume_command_hint=f"gemini --resume {session_id}",
        captured_from="gemini_generated_session_id",
        status="usable",
        resume_eligible=True,
    )
    return [base_command[0], "--session-id", session_id, *base_command[1:]], doc


def prepare_headless_runtime_resume(
    *,
    group_id: str,
    actor_id: str,
    runtime: str,
    cwd: Path,
    command: Iterable[str],
    model: str = "",
) -> Dict[str, Any]:
    if _runtime_resume_disabled():
        return {}
    runtime_norm = str(runtime or "").strip().lower()
    if runtime_norm not in {"claude", "codex"}:
        return {}
    if runtime_norm == "codex":
        return prepare_codex_app_thread_resume(
            group_id=group_id,
            actor_id=actor_id,
            cwd=cwd,
            command=command,
            model=model,
        )
    doc = read_runtime_session(group_id, actor_id)
    if not doc:
        return {}
    if str(doc.get("runtime") or "").strip().lower() != runtime_norm:
        return {}
    if str(doc.get("status") or "").strip().lower() != "usable":
        return {}
    if not bool(doc.get("resume_eligible")):
        return {}
    if str(doc.get("workspace_path") or "") != _workspace_path(cwd):
        return {}
    if str(doc.get("command_fingerprint") or "") != runtime_session_command_fingerprint(command):
        return {}
    expected_model = str(model or _model_from_command(command) or "").strip()
    saved_model = str(doc.get("model") or "").strip()
    if saved_model != expected_model:
        return {}
    if runtime_norm == "claude":
        if not str(doc.get("provider_session_id") or "").strip():
            return {}
        if str(doc.get("provider_thread_id") or "").strip():
            return {}

    next_doc = dict(doc)
    next_doc["last_resume_attempt_at"] = utc_now_iso()
    next_doc["updated_at"] = utc_now_iso()
    write_runtime_session(group_id, actor_id, next_doc)
    return next_doc


def prepare_codex_app_thread_resume(
    *,
    group_id: str,
    actor_id: str,
    cwd: Path,
    command: Iterable[str],
    model: str = "",
) -> Dict[str, Any]:
    if _runtime_resume_disabled():
        return {}
    doc = read_runtime_session(group_id, actor_id)
    if not doc:
        return {}
    if str(doc.get("runtime") or "").strip().lower() != "codex":
        return {}
    if str(doc.get("status") or "").strip().lower() != "usable":
        return {}
    if not bool(doc.get("resume_eligible")):
        return {}
    if str(doc.get("workspace_path") or "") != _workspace_path(cwd):
        return {}
    if str(doc.get("command_fingerprint") or "") != runtime_session_command_fingerprint(command):
        return {}
    expected_model = str(model or _model_from_command(command) or "").strip()
    saved_model = str(doc.get("model") or "").strip()
    if saved_model != expected_model:
        return {}
    if not str(doc.get("provider_thread_id") or "").strip():
        return {}
    if str(doc.get("provider_session_id") or "").strip():
        return {}

    next_doc = dict(doc)
    next_doc["last_resume_attempt_at"] = utc_now_iso()
    next_doc["updated_at"] = utc_now_iso()
    write_runtime_session(group_id, actor_id, next_doc)
    return next_doc


def prepare_claude_headless_launch_command(
    *,
    group_id: str,
    actor_id: str,
    cwd: Path,
    base_command: Iterable[str],
    model: str = "",
) -> Tuple[list[str], Optional[Dict[str, Any]], str]:
    command = [str(item) for item in list(base_command or []) if str(item).strip()]
    resume_doc = prepare_headless_runtime_resume(
        group_id=group_id,
        actor_id=actor_id,
        runtime="claude",
        cwd=cwd,
        command=command,
        model=model,
    )
    if resume_doc:
        session_id = str(resume_doc.get("provider_session_id") or "").strip()
        resume_command = _claude_resume_command(command, session_id)
        if resume_command:
            return resume_command, resume_doc, "resume"

    if _runtime_resume_disabled() or not command or _has_claude_session_control(command):
        return command, None, "fresh"
    session_id = str(uuid.uuid4())
    return [command[0], "--session-id", session_id, *command[1:]], {
        "provider_session_id": session_id,
        "captured_from": "claude_generated_session_id",
    }, "fresh"


def _record_codex_pty_session_from_status_text(
    *,
    group_id: str,
    actor_id: str,
    cwd: Path,
    base_command: list[str],
    text: str | bytes,
) -> Dict[str, Any]:
    if not base_command or _codex_resume_command(base_command, "seed-probe") == []:
        return {}
    if _runtime_resume_disabled():
        return {}
    session_id = parse_codex_status_session_id(text)
    if not session_id:
        return {}
    return record_pty_runtime_session(
        group_id=group_id,
        actor_id=actor_id,
        runtime="codex",
        cwd=cwd,
        command=base_command,
        provider_session_id=session_id,
        resume_command_hint=f"codex resume {session_id}",
        captured_from="codex_status_command",
        status="usable",
        resume_eligible=True,
    )


def _capture_codex_pty_session_from_status(
    *,
    group_id: str,
    actor_id: str,
    cwd: Path,
    base_command: list[str],
    timeout_seconds: float,
) -> Dict[str, Any]:
    if _runtime_resume_disabled():
        return {}
    if timeout_seconds <= 0:
        return {}
    started = time.monotonic()
    deadline = started + float(timeout_seconds)
    wrote_status = False
    first_output_seen_at: Optional[float] = None
    while time.monotonic() <= deadline:
        try:
            if not pty_runner.SUPERVISOR.actor_running(group_id, actor_id):
                return {}
        except Exception:
            return {}
        try:
            tail = pty_runner.SUPERVISOR.tail_output(group_id=group_id, actor_id=actor_id, max_bytes=64_000)
        except Exception:
            tail = b""
        doc = _record_codex_pty_session_from_status_text(
            group_id=group_id,
            actor_id=actor_id,
            cwd=cwd,
            base_command=base_command,
            text=tail,
        )
        if doc:
            return doc
        if tail and first_output_seen_at is None:
            first_output_seen_at = time.monotonic()
        now = time.monotonic()
        ready_long_enough = first_output_seen_at is not None and now - first_output_seen_at >= 0.3
        fallback_wait = min(3.0, max(0.1, float(timeout_seconds) * 0.5))
        waited_long_enough = now - started >= fallback_wait
        if not wrote_status and (ready_long_enough or waited_long_enough):
            try:
                wrote_status = _submit_codex_status_command(group_id=group_id, actor_id=actor_id)
            except Exception:
                wrote_status = True
        time.sleep(0.05)
    return {}


def _schedule_codex_pty_status_capture(
    *,
    group_id: str,
    actor_id: str,
    cwd: Path,
    base_command: list[str],
) -> None:
    if _runtime_resume_disabled():
        return
    timeout = _codex_status_capture_seconds()
    if timeout <= 0:
        return

    def _worker() -> None:
        try:
            _capture_codex_pty_session_from_status(
                group_id=group_id,
                actor_id=actor_id,
                cwd=cwd,
                base_command=base_command,
                timeout_seconds=timeout,
            )
        except Exception:
            return

    thread = threading.Thread(target=_worker, name=f"cccc-codex-status-session:{group_id}:{actor_id}", daemon=True)
    thread.start()


def prepare_initial_pty_session_command(
    *,
    group_id: str,
    actor_id: str,
    runtime: str,
    cwd: Path,
    base_command: Iterable[str],
    env: Dict[str, str],
    max_backlog_bytes: int,
) -> Tuple[list[str], Optional[Dict[str, Any]]]:
    command = [str(item) for item in list(base_command or []) if str(item).strip()]
    if _runtime_resume_disabled():
        return command, None
    runtime_norm = str(runtime or "").strip().lower()
    if runtime_norm == "claude":
        return _claude_initial_session_command(group_id=group_id, actor_id=actor_id, cwd=cwd, base_command=command)
    if runtime_norm == "gemini":
        return _gemini_initial_session_command(group_id=group_id, actor_id=actor_id, cwd=cwd, base_command=command)
    if runtime_norm == "codex":
        return command, None
    return command, None


def prepare_pty_resume_command(
    *,
    group_id: str,
    actor_id: str,
    runtime: str,
    cwd: Path,
    base_command: Iterable[str],
    model: str = "",
) -> Tuple[list[str], Optional[Dict[str, Any]]]:
    command = [str(item) for item in list(base_command or []) if str(item).strip()]
    if _runtime_resume_disabled():
        return command, None

    runtime_norm = str(runtime or "").strip().lower()
    if runtime_norm not in {"claude", "codex", "gemini"}:
        return command, None

    doc = read_runtime_session(group_id, actor_id)
    if not doc:
        return command, None
    if str(doc.get("runtime") or "").strip().lower() != runtime_norm:
        return command, None
    if str(doc.get("status") or "").strip().lower() != "usable":
        return command, None
    if not bool(doc.get("resume_eligible")):
        return command, None
    if str(doc.get("workspace_path") or "") != _workspace_path(cwd):
        return command, None
    if str(doc.get("command_fingerprint") or "") != runtime_session_command_fingerprint(command):
        return command, None
    expected_model = str(model or _model_from_command(command) or "").strip()
    saved_model = str(doc.get("model") or "").strip()
    if saved_model != expected_model:
        return command, None

    session_id = str(doc.get("provider_session_id") or "").strip()
    if not session_id:
        return command, None
    if str(doc.get("provider_thread_id") or "").strip():
        return command, None

    if runtime_norm == "claude":
        resume_command = _claude_resume_command(command, session_id)
    elif runtime_norm == "gemini":
        resume_command = _gemini_resume_command(command, session_id)
    else:
        resume_command = _codex_resume_command(command, session_id)
    if not resume_command:
        return command, None

    next_doc = dict(doc)
    next_doc["last_resume_attempt_at"] = utc_now_iso()
    next_doc["updated_at"] = utc_now_iso()
    write_runtime_session(group_id, actor_id, next_doc)
    return resume_command, next_doc


def mark_runtime_session_resume_failed(
    *,
    group_id: str,
    actor_id: str,
    error: str,
) -> Dict[str, Any]:
    doc = read_runtime_session(group_id, actor_id)
    if not doc:
        return {}
    next_doc = dict(doc)
    try:
        failure_count = int(next_doc.get("failure_count") or 0)
    except Exception:
        failure_count = 0
    next_doc["status"] = "resume_failed"
    next_doc["resume_eligible"] = False
    next_doc["failure_count"] = failure_count + 1
    next_doc["last_resume_error"] = str(error or "").strip()[:1000]
    next_doc["updated_at"] = utc_now_iso()
    write_runtime_session(group_id, actor_id, next_doc)
    return next_doc


def mark_runtime_session_auth_failed(
    *,
    group_id: str,
    actor_id: str,
    error: str,
) -> Dict[str, Any]:
    doc = read_runtime_session(group_id, actor_id)
    if not doc:
        return {}
    next_doc = dict(doc)
    try:
        failure_count = int(next_doc.get("failure_count") or 0)
    except Exception:
        failure_count = 0
    next_doc["status"] = "auth_failed"
    next_doc["resume_eligible"] = False
    next_doc["failure_count"] = failure_count + 1
    next_doc["last_resume_error"] = str(error or "").strip()[:1000]
    next_doc["updated_at"] = utc_now_iso()
    write_runtime_session(group_id, actor_id, next_doc)
    return next_doc


def _verify_pty_resume_start(
    *,
    group_id: str,
    actor_id: str,
    timeout_seconds: float,
) -> str:
    timeout = float(timeout_seconds or 0.0)
    if timeout <= 0:
        return ""
    foreground_timeout = min(timeout, _pty_resume_foreground_verify_seconds())
    deadline = time.monotonic() + max(0.0, foreground_timeout)
    last_tail = b""
    while True:
        try:
            last_tail = pty_runner.SUPERVISOR.tail_output(
                group_id=group_id,
                actor_id=actor_id,
                max_bytes=64_000,
            )
        except Exception:
            last_tail = b""
        if _looks_like_resume_failure(last_tail):
            return "provider reported resume failure"
        try:
            running = bool(pty_runner.SUPERVISOR.actor_running(group_id, actor_id))
        except Exception:
            running = False
        if not running:
            detail = last_tail.decode("utf-8", errors="replace").strip()
            if detail:
                return f"provider resume process exited early: {detail[-500:]}"
            return "provider resume process exited early"
        if time.monotonic() >= deadline:
            return ""
        time.sleep(0.05)


def _pty_state_pid_status(group_id: str, actor_id: str, expected_pid: int) -> str:
    if int(expected_pid or 0) <= 0:
        return "match"
    doc = read_json(pty_state_path(group_id, actor_id))
    if not isinstance(doc, dict):
        return "missing"
    try:
        return "match" if int(doc.get("pid") or 0) == int(expected_pid) else "mismatch"
    except Exception:
        return "mismatch"


def _schedule_pty_resume_failure_monitor(
    *,
    group_id: str,
    actor_id: str,
    expected_pid: int,
    timeout_seconds: float,
) -> None:
    timeout = float(timeout_seconds or 0.0)
    if timeout <= 0:
        return
    deadline = time.monotonic() + timeout

    def _worker() -> None:
        state_seen = False
        state_attach_deadline = time.monotonic() + 3.0
        while time.monotonic() < deadline:
            state_status = _pty_state_pid_status(group_id, actor_id, expected_pid)
            if state_status == "match":
                state_seen = True
            elif state_seen or time.monotonic() >= state_attach_deadline:
                return
            try:
                tail = pty_runner.SUPERVISOR.tail_output(group_id=group_id, actor_id=actor_id, max_bytes=64_000)
            except Exception:
                tail = b""
            if _looks_like_resume_failure(tail):
                mark_runtime_session_resume_failed(
                    group_id=group_id,
                    actor_id=actor_id,
                    error="provider reported resume failure",
                )
                try:
                    if _pty_state_pid_status(group_id, actor_id, expected_pid) == "match":
                        pty_runner.SUPERVISOR.stop_actor(group_id=group_id, actor_id=actor_id)
                except Exception:
                    pass
                return
            try:
                running = bool(pty_runner.SUPERVISOR.actor_running(group_id, actor_id))
            except Exception:
                running = False
            if not running:
                if _pty_state_pid_status(group_id, actor_id, expected_pid) != "match":
                    return
                detail = tail.decode("utf-8", errors="replace").strip()
                mark_runtime_session_resume_failed(
                    group_id=group_id,
                    actor_id=actor_id,
                    error=(
                        f"provider resume process exited early: {detail[-500:]}"
                        if detail
                        else "provider resume process exited early"
                    ),
                )
                try:
                    pty_runner.SUPERVISOR.stop_actor(group_id=group_id, actor_id=actor_id)
                except Exception:
                    pass
                return
            time.sleep(0.25)

    threading.Thread(
        target=_worker,
        name=f"cccc-pty-resume-monitor:{group_id}:{actor_id}",
        daemon=True,
    ).start()


def _abort_pty_actor_after_resume_failure(
    *,
    group_id: str,
    actor_id: str,
    error: str,
) -> None:
    mark_runtime_session_resume_failed(group_id=group_id, actor_id=actor_id, error=error)
    try:
        pty_runner.SUPERVISOR.stop_actor(group_id=group_id, actor_id=actor_id)
    except Exception:
        pass


def _start_fresh_pty_actor_after_resume_failure(
    *,
    group_id: str,
    actor_id: str,
    cwd: Path,
    base_command: list[str],
    env: Dict[str, str],
    runtime: str,
    max_backlog_bytes: int,
    runtime_start_preflight_error: Callable[..., str],
    cause: Optional[BaseException] = None,
) -> Any:
    runtime_norm = str(runtime or "").strip().lower()
    fresh_command = list(base_command)
    fresh_doc: Optional[Dict[str, Any]] = None
    if runtime_norm in {"claude", "gemini"}:
        fresh_command, fresh_doc = prepare_initial_pty_session_command(
            group_id=group_id,
            actor_id=actor_id,
            runtime=runtime,
            cwd=cwd,
            base_command=base_command,
            env=env,
            max_backlog_bytes=max_backlog_bytes,
        )

    runtime_error = runtime_start_preflight_error(runtime, fresh_command, runner="pty")
    if runtime_error:
        if fresh_doc:
            mark_runtime_session_resume_failed(
                group_id=group_id,
                actor_id=actor_id,
                error=f"fresh runtime session launch rejected: {runtime_error}",
            )
        if cause is not None:
            raise RuntimeError(runtime_error) from cause
        raise RuntimeError(runtime_error)
    try:
        session = pty_runner.SUPERVISOR.start_actor(
            group_id=group_id,
            actor_id=actor_id,
            cwd=cwd,
            command=fresh_command,
            env=env,
            runtime=runtime,
            max_backlog_bytes=max_backlog_bytes,
        )
    except Exception as exc:
        if fresh_doc:
            mark_runtime_session_resume_failed(
                group_id=group_id,
                actor_id=actor_id,
                error=f"fresh runtime session launch failed: {exc}",
            )
        raise
    try:
        setattr(session, "_cccc_base_command", tuple(base_command))
    except Exception:
        pass
    if not fresh_doc:
        remove_runtime_session(group_id, actor_id)
    if runtime_norm == "codex":
        _schedule_codex_pty_status_capture(
            group_id=group_id,
            actor_id=actor_id,
            cwd=cwd,
            base_command=base_command,
        )
    return session


def start_pty_actor_with_runtime_resume(
    *,
    group_id: str,
    actor_id: str,
    cwd: Path,
    base_command: Iterable[str],
    env: Dict[str, str],
    runtime: str,
    model: str = "",
    max_backlog_bytes: int = 2_000_000,
    runtime_start_preflight_error: Callable[..., str],
) -> Any:
    base_cmd = [str(item) for item in list(base_command or []) if str(item).strip()]
    launch_cmd, resume_doc = prepare_pty_resume_command(
        group_id=group_id,
        actor_id=actor_id,
        runtime=runtime,
        cwd=cwd,
        base_command=base_cmd,
        model=model,
    )
    initial_doc: Optional[Dict[str, Any]] = None
    if not resume_doc:
        try:
            if pty_runner.SUPERVISOR.actor_running(group_id=group_id, actor_id=actor_id):
                session = pty_runner.SUPERVISOR.start_actor(
                    group_id=group_id,
                    actor_id=actor_id,
                    cwd=cwd,
                    command=base_cmd,
                    env=env,
                    runtime=runtime,
                    max_backlog_bytes=max_backlog_bytes,
                )
                try:
                    setattr(session, "_cccc_base_command", tuple(base_cmd))
                except Exception:
                    pass
                if str(runtime or "").strip().lower() == "codex":
                    _schedule_codex_pty_status_capture(
                        group_id=group_id,
                        actor_id=actor_id,
                        cwd=cwd,
                        base_command=base_cmd,
                    )
                return session
        except Exception:
            pass
        runtime_error = runtime_start_preflight_error(runtime, base_cmd, runner="pty")
        if runtime_error:
            raise RuntimeError(runtime_error)
        launch_cmd, initial_doc = prepare_initial_pty_session_command(
            group_id=group_id,
            actor_id=actor_id,
            runtime=runtime,
            cwd=cwd,
            base_command=base_cmd,
            env=env,
            max_backlog_bytes=max_backlog_bytes,
        )

    runtime_error = runtime_start_preflight_error(runtime, launch_cmd, runner="pty")
    if runtime_error:
        if resume_doc:
            resume_exc = RuntimeError(runtime_error)
            mark_runtime_session_resume_failed(
                group_id=group_id,
                actor_id=actor_id,
                error=f"runtime resume launch rejected: {runtime_error}",
            )
            return _start_fresh_pty_actor_after_resume_failure(
                group_id=group_id,
                actor_id=actor_id,
                cwd=cwd,
                base_command=base_cmd,
                env=env,
                runtime=runtime,
                max_backlog_bytes=max_backlog_bytes,
                runtime_start_preflight_error=runtime_start_preflight_error,
                cause=resume_exc,
            )
        elif initial_doc:
            mark_runtime_session_resume_failed(
                group_id=group_id,
                actor_id=actor_id,
                error=f"initial runtime session launch rejected: {runtime_error}",
            )
        raise RuntimeError(runtime_error)

    try:
        session = pty_runner.SUPERVISOR.start_actor(
            group_id=group_id,
            actor_id=actor_id,
            cwd=cwd,
            command=launch_cmd,
            env=env,
            runtime=runtime,
            max_backlog_bytes=max_backlog_bytes,
        )
        try:
            setattr(session, "_cccc_base_command", tuple(base_cmd))
        except Exception:
            pass
        if resume_doc:
            verify_timeout = _pty_resume_verify_seconds()
            resume_error = _verify_pty_resume_start(
                group_id=group_id,
                actor_id=actor_id,
                timeout_seconds=verify_timeout,
            )
            if resume_error:
                resume_exc = RuntimeError(resume_error)
                _abort_pty_actor_after_resume_failure(
                    group_id=group_id,
                    actor_id=actor_id,
                    error=resume_error,
                )
                return _start_fresh_pty_actor_after_resume_failure(
                    group_id=group_id,
                    actor_id=actor_id,
                    cwd=cwd,
                    base_command=base_cmd,
                    env=env,
                    runtime=runtime,
                    max_backlog_bytes=max_backlog_bytes,
                    runtime_start_preflight_error=runtime_start_preflight_error,
                    cause=resume_exc,
                )
            _schedule_pty_resume_failure_monitor(
                group_id=group_id,
                actor_id=actor_id,
                expected_pid=int(getattr(session, "pid", 0) or 0),
                timeout_seconds=max(0.0, verify_timeout - _pty_resume_foreground_verify_seconds()),
            )
        if str(runtime or "").strip().lower() == "codex" and not resume_doc:
            _schedule_codex_pty_status_capture(
                group_id=group_id,
                actor_id=actor_id,
                cwd=cwd,
                base_command=base_cmd,
            )
        return session
    except Exception as exc:
        active_doc = resume_doc or initial_doc
        if not active_doc:
            raise
        if resume_doc:
            status = str((read_runtime_session(group_id, actor_id) or {}).get("status") or "").strip().lower()
            if status != "resume_failed":
                _abort_pty_actor_after_resume_failure(
                    group_id=group_id,
                    actor_id=actor_id,
                    error=str(exc),
                )
            return _start_fresh_pty_actor_after_resume_failure(
                group_id=group_id,
                actor_id=actor_id,
                cwd=cwd,
                base_command=base_cmd,
                env=env,
                runtime=runtime,
                max_backlog_bytes=max_backlog_bytes,
                runtime_start_preflight_error=runtime_start_preflight_error,
                cause=exc,
            )
        mark_runtime_session_resume_failed(group_id=group_id, actor_id=actor_id, error=str(exc))
        fallback_cmd = base_cmd
        runtime_error = runtime_start_preflight_error(runtime, fallback_cmd, runner="pty")
        if runtime_error:
            raise RuntimeError(runtime_error) from exc
        session = pty_runner.SUPERVISOR.start_actor(
            group_id=group_id,
            actor_id=actor_id,
            cwd=cwd,
            command=fallback_cmd,
            env=env,
            runtime=runtime,
            max_backlog_bytes=max_backlog_bytes,
        )
        try:
            setattr(session, "_cccc_base_command", tuple(base_cmd))
        except Exception:
            pass
        if str(runtime or "").strip().lower() == "codex":
            _schedule_codex_pty_status_capture(
                group_id=group_id,
                actor_id=actor_id,
                cwd=cwd,
                base_command=base_cmd,
            )
        return session
