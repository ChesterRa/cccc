from __future__ import annotations

import json
import os
import shutil
import subprocess
import sys
import time
from pathlib import Path
from typing import Any, Dict, List

from ...paths import ensure_home
from ...util.file_lock import acquire_lockfile, release_lockfile
from ...util.fs import atomic_write_json, read_json
from ...util.time import utc_now_iso


VOICE_RUNTIME_ID_SHERPA_ONNX_STREAMING = "sherpa_onnx_streaming"
VOICE_RUNTIME_STATUS_NOT_INSTALLED = "not_installed"
VOICE_RUNTIME_STATUS_INSTALLING = "installing"
VOICE_RUNTIME_STATUS_READY = "ready"
VOICE_RUNTIME_STATUS_FAILED = "failed"

_STATE_FILENAME = "runtime-state.json"
_LOCK_FILENAME = ".runtime.lock"
_INSTALL_TIMEOUT_SECONDS = 3600
_SUPPORTED_PYTHON_COMMANDS = ("python3.12", "python3.11", "python3.10", "python3.9")
_SHERPA_ONNX_STREAMING_PACKAGES = ("sherpa-onnx", "numpy")
_SHERPA_ONNX_STREAMING_MODULES = ("sherpa_onnx", "numpy")


class VoiceRuntimeDepsError(Exception):
    def __init__(self, code: str, message: str, *, details: Dict[str, Any] | None = None) -> None:
        super().__init__(message)
        self.code = code
        self.message = message
        self.details = details or {}


def _runtime_root(runtime_id: str) -> Path:
    return ensure_home() / "cache" / "voice-runtimes" / str(runtime_id or "").strip()


def _state_path(runtime_id: str) -> Path:
    return _runtime_root(runtime_id) / _STATE_FILENAME


def _lock_path(runtime_id: str) -> Path:
    return _runtime_root(runtime_id) / _LOCK_FILENAME


def _venv_dir(runtime_id: str) -> Path:
    return _runtime_root(runtime_id) / ".venv"


def _venv_python(runtime_id: str) -> Path:
    if os.name == "nt":
        return _venv_dir(runtime_id) / "Scripts" / "python.exe"
    return _venv_dir(runtime_id) / "bin" / "python"


def _read_state(runtime_id: str) -> Dict[str, Any]:
    state = read_json(_state_path(runtime_id))
    return dict(state) if isinstance(state, dict) else {}


def _write_state(runtime_id: str, payload: Dict[str, Any]) -> None:
    root = _runtime_root(runtime_id)
    root.mkdir(parents=True, exist_ok=True)
    atomic_write_json(_state_path(runtime_id), payload, indent=2)


def _runtime_packages(runtime_id: str) -> tuple[str, ...]:
    if runtime_id == VOICE_RUNTIME_ID_SHERPA_ONNX_STREAMING:
        return _SHERPA_ONNX_STREAMING_PACKAGES
    raise VoiceRuntimeDepsError("voice_runtime_unknown", f"unknown voice runtime: {runtime_id}")


def _runtime_modules(runtime_id: str) -> tuple[str, ...]:
    if runtime_id == VOICE_RUNTIME_ID_SHERPA_ONNX_STREAMING:
        return _SHERPA_ONNX_STREAMING_MODULES
    raise VoiceRuntimeDepsError("voice_runtime_unknown", f"unknown voice runtime: {runtime_id}")


def _runtime_title(runtime_id: str) -> str:
    if runtime_id == VOICE_RUNTIME_ID_SHERPA_ONNX_STREAMING:
        return "sherpa-onnx streaming ASR"
    return str(runtime_id or "voice runtime")


def _python_version(argv0: str) -> tuple[int, int]:
    proc = subprocess.run(
        [argv0, "-c", "import sys; print(f'{sys.version_info.major}.{sys.version_info.minor}')"],
        capture_output=True,
        check=False,
        text=True,
        timeout=15,
    )
    if proc.returncode != 0:
        return (0, 0)
    raw = str(proc.stdout or "").strip()
    try:
        major, minor = raw.split(".", 1)
        return int(major), int(minor)
    except Exception:
        return (0, 0)


def _select_base_python() -> str:
    current = sys.executable
    current_version = sys.version_info[:2]
    if (3, 9) <= current_version <= (3, 12):
        return current
    for name in _SUPPORTED_PYTHON_COMMANDS:
        path = shutil.which(name)
        if not path:
            continue
        version = _python_version(path)
        if (3, 9) <= version <= (3, 12):
            return path
    raise VoiceRuntimeDepsError(
        "voice_runtime_python_missing",
        "Voice ASR runtime needs Python 3.9-3.12 for compatible wheels.",
        details={"current_python": sys.executable, "current_version": ".".join(map(str, sys.version_info[:3]))},
    )


def _ensure_venv(runtime_id: str) -> Path:
    python_path = _venv_python(runtime_id)
    if python_path.exists():
        return python_path
    base_python = _select_base_python()
    proc = subprocess.run(
        [base_python, "-m", "venv", str(_venv_dir(runtime_id))],
        capture_output=True,
        check=False,
        text=True,
        timeout=120,
    )
    if proc.returncode != 0:
        raise VoiceRuntimeDepsError(
            "voice_runtime_venv_failed",
            f"Failed to create {_runtime_title(runtime_id)} environment.",
            details={"stderr": str(proc.stderr or "").strip()[-4000:], "base_python": base_python},
        )
    return python_path


def _module_status(python_path: Path, modules: tuple[str, ...]) -> Dict[str, bool]:
    if not python_path.exists():
        return {name: False for name in modules}
    script = (
        "import importlib.util, json; "
        f"mods={list(modules)!r}; "
        "print(json.dumps({m: importlib.util.find_spec(m) is not None for m in mods}))"
    )
    try:
        proc = subprocess.run(
            [str(python_path), "-c", script],
            capture_output=True,
            check=False,
            text=True,
            timeout=30,
        )
    except Exception:
        return {name: False for name in modules}
    if proc.returncode != 0:
        return {name: False for name in modules}
    try:
        payload = json.loads(str(proc.stdout or "{}"))
    except Exception:
        payload = {}
    return {name: bool(payload.get(name)) for name in modules}


def get_voice_runtime_status(runtime_id: str = VOICE_RUNTIME_ID_SHERPA_ONNX_STREAMING) -> Dict[str, Any]:
    runtime_id = str(runtime_id or "").strip() or VOICE_RUNTIME_ID_SHERPA_ONNX_STREAMING
    modules = _runtime_modules(runtime_id)
    packages = _runtime_packages(runtime_id)
    state = _read_state(runtime_id)
    python_path = _venv_python(runtime_id)
    modules_ready = _module_status(python_path, modules)
    missing = [name for name, ready in modules_ready.items() if not ready]
    state_status = str(state.get("status") or "").strip()
    status = VOICE_RUNTIME_STATUS_READY if not missing else (state_status or VOICE_RUNTIME_STATUS_NOT_INSTALLED)
    if status == VOICE_RUNTIME_STATUS_READY and missing:
        status = VOICE_RUNTIME_STATUS_NOT_INSTALLED
    return {
        "runtime_id": runtime_id,
        "status": status,
        "available": True,
        "installed": status == VOICE_RUNTIME_STATUS_READY,
        "install_dir": str(_runtime_root(runtime_id)),
        "python": str(python_path) if python_path.exists() else "",
        "packages": list(packages),
        "modules": modules_ready,
        "missing_modules": missing,
        "updated_at": str(state.get("updated_at") or ""),
        "installed_at": str(state.get("installed_at") or ""),
        "error": state.get("error") if isinstance(state.get("error"), dict) else {},
    }


def resolve_voice_runtime_python(runtime_id: str = VOICE_RUNTIME_ID_SHERPA_ONNX_STREAMING) -> str:
    status = get_voice_runtime_status(runtime_id)
    if str(status.get("status") or "") != VOICE_RUNTIME_STATUS_READY:
        return ""
    return str(status.get("python") or "").strip()


def list_voice_runtime_statuses() -> List[Dict[str, Any]]:
    return [
        get_voice_runtime_status(VOICE_RUNTIME_ID_SHERPA_ONNX_STREAMING),
    ]


def install_voice_runtime_deps(runtime_id: str = VOICE_RUNTIME_ID_SHERPA_ONNX_STREAMING) -> Dict[str, Any]:
    runtime_id = str(runtime_id or "").strip() or VOICE_RUNTIME_ID_SHERPA_ONNX_STREAMING
    packages = _runtime_packages(runtime_id)
    lock_handle = acquire_lockfile(_lock_path(runtime_id))
    try:
        _write_state(
            runtime_id,
            {
                "runtime_id": runtime_id,
                "status": VOICE_RUNTIME_STATUS_INSTALLING,
                "updated_at": utc_now_iso(),
                "error": {},
                "packages": list(packages),
            },
        )
        python_path = _ensure_venv(runtime_id)
        commands: List[List[str]] = [
            [str(python_path), "-m", "pip", "install", "-U", "pip"],
            [str(python_path), "-m", "pip", "install", *packages],
        ]
        for command in commands:
            started = time.monotonic()
            proc = subprocess.run(
                command,
                capture_output=True,
                check=False,
                text=True,
                timeout=_INSTALL_TIMEOUT_SECONDS,
            )
            if proc.returncode != 0:
                raise VoiceRuntimeDepsError(
                    "voice_runtime_install_failed",
                    f"Failed to install {_runtime_title(runtime_id)} dependencies.",
                    details={
                        "command": " ".join(command),
                        "duration_seconds": round(time.monotonic() - started, 1),
                        "stderr": str(proc.stderr or "").strip()[-4000:],
                        "stdout": str(proc.stdout or "").strip()[-2000:],
                    },
                )
        status = get_voice_runtime_status(runtime_id)
        if status.get("missing_modules"):
            raise VoiceRuntimeDepsError(
                "voice_runtime_dependency_missing",
                f"{_runtime_title(runtime_id)} dependencies are still missing after install.",
                details={"missing_modules": status.get("missing_modules")},
            )
        payload = {
            "runtime_id": runtime_id,
            "status": VOICE_RUNTIME_STATUS_READY,
            "updated_at": utc_now_iso(),
            "installed_at": utc_now_iso(),
            "error": {},
            "packages": list(packages),
            "python": str(python_path),
        }
        _write_state(runtime_id, payload)
        return get_voice_runtime_status(runtime_id)
    except VoiceRuntimeDepsError as exc:
        _write_state(
            runtime_id,
            {
                "runtime_id": runtime_id,
                "status": VOICE_RUNTIME_STATUS_FAILED,
                "updated_at": utc_now_iso(),
                "error": {"code": exc.code, "message": exc.message, "details": exc.details},
                "packages": list(packages),
            },
        )
        raise
    finally:
        release_lockfile(lock_handle)
