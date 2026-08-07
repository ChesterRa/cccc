from __future__ import annotations

import os
import shutil
import sys
from collections.abc import Mapping
from pathlib import Path
from typing import Any, Optional

_LAUNCHER_PATH_ENV = "CCCC_LAUNCHER_PATH"
_WINDOWS_EXECUTABLE_EXTENSIONS = (".com", ".exe", ".bat", ".cmd", ".ps1")


def _absolute_path(path: Path) -> Path:
    return Path(os.path.abspath(os.fspath(path.expanduser())))


def _path_key(path: Path) -> str:
    return os.path.normcase(os.fspath(_absolute_path(path)))


def _same_command(left: Path, right: Path) -> bool:
    try:
        return os.path.samefile(left, right)
    except (FileNotFoundError, OSError):
        return _path_key(left) == _path_key(right)


def _command_names(environ: Mapping[str, str]) -> tuple[str, ...]:
    if os.name != "nt":
        return ("cccc",)
    raw_extensions = str(environ.get("PATHEXT") or "")
    extensions = [item.strip().lower() for item in raw_extensions.split(";") if item.strip()]
    if not extensions:
        extensions = list(_WINDOWS_EXECUTABLE_EXTENSIONS)
    elif ".ps1" not in extensions:
        extensions.append(".ps1")
    names = ["cccc"]
    names.extend(f"cccc{extension if extension.startswith('.') else '.' + extension}" for extension in extensions)
    return tuple(dict.fromkeys(names))


def find_cccc_commands(
    *,
    path_value: Optional[str] = None,
    environ: Optional[Mapping[str, str]] = None,
) -> list[Path]:
    """Return every executable ``cccc`` command on PATH in resolution order."""
    environment = os.environ if environ is None else environ
    search_path = str(environment.get("PATH") or "") if path_value is None else str(path_value)
    names = _command_names(environment)
    commands: list[Path] = []
    seen: set[str] = set()
    for raw_directory in search_path.split(os.pathsep):
        directory = Path(raw_directory.strip('"') or os.curdir)
        for name in names:
            candidate = _absolute_path(directory / name)
            if not candidate.is_file():
                continue
            if os.name != "nt" and not os.access(candidate, os.X_OK):
                continue
            key = _path_key(candidate)
            if key in seen:
                continue
            seen.add(key)
            commands.append(candidate)
    return commands


def _current_launcher(
    *,
    argv0: Optional[str],
    environ: Mapping[str, str],
) -> Optional[Path]:
    raw = str(environ.get(_LAUNCHER_PATH_ENV) or "").strip()
    if not raw:
        raw_argv0 = str(sys.argv[0] if argv0 is None else argv0).strip()
        if Path(raw_argv0).name.lower() in {"cccc", "cccc.exe"}:
            raw = raw_argv0
    if not raw:
        return None
    candidate = Path(raw)
    if not candidate.is_absolute():
        located = shutil.which(raw, path=str(environ.get("PATH") or ""))
        if located:
            candidate = Path(located)
    candidate = _absolute_path(candidate)
    return candidate if candidate.is_file() else None


def inspect_cccc_installation(
    *,
    argv0: Optional[str] = None,
    environ: Optional[Mapping[str, str]] = None,
) -> dict[str, Any]:
    """Describe the invoked launcher and every competing PATH command."""
    environment = os.environ if environ is None else environ
    current = _current_launcher(argv0=argv0, environ=environment)
    candidates = find_cccc_commands(environ=environment)
    resolved = candidates[0] if candidates else None
    if current is None:
        status = "unknown"
    elif resolved is None:
        status = "missing"
    elif _same_command(current, resolved):
        status = "ok"
    else:
        status = "conflict"
    conflicts = [path for path in candidates if current is None or not _same_command(path, current)]
    return {
        "current_executable": str(current) if current else None,
        "resolved_command": str(resolved) if resolved else None,
        "command_candidates": [str(path) for path in candidates],
        "conflicting_commands": [str(path) for path in conflicts],
        "path_status": status,
        "path_conflict": status == "conflict",
    }
