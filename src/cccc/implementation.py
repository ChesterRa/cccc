from __future__ import annotations

import json
import os
import re
import subprocess
from pathlib import Path
from typing import Any, Literal, Optional

from . import __version__
from .paths import ensure_home
from .util.fs import atomic_write_json

ImplementationName = Literal["python", "rust"]
DaemonImplementation = Literal["python", "rust", "unknown"]

_STATE_SCHEMA = 1
_DEFAULT_IMPLEMENTATION: ImplementationName = "python"
_IMPLEMENTATIONS = {"python", "rust"}
_RUST_BINARY_ENV = "CCCC_RUST_BINARY"
_RUST_BINARY_NAME = "cccc-rust.exe" if os.name == "nt" else "cccc-rust"
_RUST_VERSION_RE = re.compile(
    r"(?<![0-9])(?P<base>[0-9]+\.[0-9]+\.[0-9]+)(?:-(?P<phase>alpha|beta|rc)(?P<number>[0-9]+))?(?![0-9])"
)
_PYTHON_VERSION_RE = re.compile(
    r"^(?P<base>[0-9]+\.[0-9]+\.[0-9]+)(?:(?P<phase>a|b|rc)(?P<number>[0-9]+))?$"
)


class ImplementationError(RuntimeError):
    """Raised when implementation selection or native payload validation fails."""


def implementation_state_path(home: Optional[Path] = None) -> Path:
    root = Path(home).resolve() if home is not None else ensure_home()
    return root / "implementation.json"


def implementation_lock_path(home: Optional[Path] = None) -> Path:
    root = Path(home).resolve() if home is not None else ensure_home()
    return root / "daemon" / "implementation.lock"


def normalize_implementation(value: str) -> ImplementationName:
    normalized = str(value or "").strip().lower()
    if normalized not in _IMPLEMENTATIONS:
        raise ImplementationError(
            f"unknown CCCC implementation {value!r}; expected 'python' or 'rust'"
        )
    return normalized  # type: ignore[return-value]


def load_selected_implementation(home: Optional[Path] = None) -> ImplementationName:
    path = implementation_state_path(home)
    if not path.exists():
        return _DEFAULT_IMPLEMENTATION
    try:
        payload = json.loads(path.read_text(encoding="utf-8"))
    except Exception as error:
        raise ImplementationError(
            f"invalid implementation state at {path}: {error}; run `cccc python` to replace it explicitly"
        ) from error
    if not isinstance(payload, dict):
        raise ImplementationError(
            f"invalid implementation state at {path}: expected a JSON object"
        )
    if payload.get("schema") != _STATE_SCHEMA:
        raise ImplementationError(
            f"unsupported implementation state schema at {path}: {payload.get('schema')!r}"
        )
    return normalize_implementation(str(payload.get("selected") or ""))


def save_selected_implementation(
    selected: str,
    home: Optional[Path] = None,
) -> ImplementationName:
    normalized = normalize_implementation(selected)
    atomic_write_json(
        implementation_state_path(home),
        {"schema": _STATE_SCHEMA, "selected": normalized},
    )
    return normalized


def _source_checkout_rust_binary() -> Optional[Path]:
    package_file = Path(__file__).resolve()
    try:
        root = package_file.parents[2]
    except IndexError:
        return None
    if not root.joinpath("Cargo.toml").is_file():
        return None
    candidate = root / "target" / "release" / ("cccc.exe" if os.name == "nt" else "cccc")
    return candidate if candidate.is_file() else None


def rust_binary_path() -> Optional[Path]:
    explicit = str(os.environ.get(_RUST_BINARY_ENV) or "").strip()
    if explicit:
        return Path(explicit).expanduser().resolve()

    packaged = Path(__file__).resolve().parent / "_native" / _RUST_BINARY_NAME
    if packaged.is_file():
        return packaged
    return _source_checkout_rust_binary()


def _canonical_python_version(value: str) -> Optional[str]:
    match = _PYTHON_VERSION_RE.fullmatch(str(value or "").strip())
    if match is None:
        return None
    phase = {"a": "alpha", "b": "beta", "rc": "rc"}.get(match.group("phase") or "", "")
    suffix = f"-{phase}{match.group('number')}" if phase else ""
    return f"{match.group('base')}{suffix}"


def _rust_version_from_output(output: str) -> Optional[str]:
    match = _RUST_VERSION_RE.search(str(output or ""))
    if match is None:
        return None
    phase = match.group("phase") or ""
    suffix = f"-{phase}{match.group('number')}" if phase else ""
    return f"{match.group('base')}{suffix}"


def probe_rust_implementation(*, timeout_s: float = 5.0) -> dict[str, Any]:
    binary = rust_binary_path()
    if binary is None:
        return {
            "available": False,
            "path": None,
            "version": None,
            "error": "this installation does not contain the Rust implementation",
        }
    if not binary.is_file():
        return {
            "available": False,
            "path": str(binary),
            "version": None,
            "error": f"Rust implementation binary does not exist: {binary}",
        }
    if os.name != "nt" and not os.access(binary, os.X_OK):
        return {
            "available": False,
            "path": str(binary),
            "version": None,
            "error": f"Rust implementation binary is not executable: {binary}",
        }
    try:
        completed = subprocess.run(
            [str(binary), "--version"],
            capture_output=True,
            text=True,
            check=False,
            timeout=max(float(timeout_s), 0.1),
        )
    except Exception as error:
        return {
            "available": False,
            "path": str(binary),
            "version": None,
            "error": f"could not execute the Rust implementation: {error}",
        }
    output = "\n".join(part for part in (completed.stdout, completed.stderr) if part).strip()
    if completed.returncode != 0:
        return {
            "available": False,
            "path": str(binary),
            "version": None,
            "error": f"Rust implementation version probe failed with exit code {completed.returncode}: {output}",
        }
    rust_version = _rust_version_from_output(output)
    expected = _canonical_python_version(__version__)
    if rust_version is None:
        return {
            "available": False,
            "path": str(binary),
            "version": None,
            "error": f"Rust implementation returned an unrecognized version: {output!r}",
        }
    if expected is None or rust_version != expected:
        return {
            "available": False,
            "path": str(binary),
            "version": rust_version,
            "error": (
                "Rust implementation version does not match the installed CCCC product: "
                f"rust={rust_version}, python={__version__}"
            ),
        }
    return {
        "available": True,
        "path": str(binary),
        "version": rust_version,
        "error": None,
    }


def require_rust_implementation() -> Path:
    probe = probe_rust_implementation()
    if not bool(probe.get("available")):
        raise ImplementationError(str(probe.get("error") or "Rust implementation is unavailable"))
    return Path(str(probe["path"])).resolve()


def daemon_implementation(ping_response: dict[str, Any]) -> Optional[DaemonImplementation]:
    if not bool(ping_response.get("ok")):
        return None
    result = ping_response.get("result") if isinstance(ping_response.get("result"), dict) else {}
    raw = result.get("implementation")
    value = str(raw or "").strip().lower()
    if value in _IMPLEMENTATIONS:
        return value  # type: ignore[return-value]
    # Daemons predating the implementation field were Python-only.
    if raw is None or not value:
        return "python"
    return "unknown"
