from __future__ import annotations

import importlib
import os
from typing import Any, Dict, List, Optional, Tuple


def _windows_pty_hints() -> List[str]:
    return [
        "Run `python -m pip install 'pywinpty>=2.0,<3.0.4'` to install the Windows ConPTY dependency.",
        "If you installed the project with uv, rerun `uv pip install -e .` to refresh platform dependencies.",
        "After installation, rerun `cccc doctor` or `pytest tests/test_windows_pty_backend.py -q` to verify support.",
    ]


def _probe_windows_pty_process() -> Tuple[Optional[Any], str, str, List[str]]:
    if os.name != "nt":
        return None, "", "", []

    hints = _windows_pty_hints()
    try:
        module = importlib.import_module("winpty")
    except ModuleNotFoundError as exc:
        missing = str(getattr(exc, "name", "") or "").strip()
        if missing in {"winpty", "pywinpty"}:
            return (
                None,
                "pywinpty_missing",
                "Windows PTY backend unavailable: install pywinpty to enable ConPTY actors.",
                hints,
            )
        return (
            None,
            "winpty_import_failed",
            f"Windows PTY backend import failed because module `{missing}` is missing.",
            hints,
        )
    except Exception as exc:
        return (
            None,
            "winpty_import_failed",
            f"Windows PTY backend import failed: {exc}",
            hints,
        )

    pty_process = getattr(module, "PtyProcess", None)
    if pty_process is None:
        return (
            None,
            "winpty_import_failed",
            "Windows PTY backend import succeeded but `winpty.PtyProcess` was not found.",
            hints,
        )
    return pty_process, "", "", []


def load_winpty_process_class() -> Optional[Any]:
    pty_process, _, _, _ = _probe_windows_pty_process()
    return pty_process


def pty_support_details() -> Dict[str, Any]:
    """Return PTY support diagnostics for the current platform."""
    details: Dict[str, Any] = {
        "platform": os.name,
        "supported": True,
        "code": "",
        "message": "",
        "hints": [],
    }

    if os.name != "nt":
        return details

    pty_process, code, message, hints = _probe_windows_pty_process()
    if pty_process is not None:
        return details

    details.update(
        {
            "supported": False,
            "code": code,
            "message": message,
            "hints": hints,
        }
    )
    return details


def pty_support_error_message() -> str:
    """Return a user-facing PTY support error message."""
    details = pty_support_details()
    if bool(details.get("supported")):
        return ""
    hints = details.get("hints") if isinstance(details.get("hints"), list) else []
    suffix = " ".join(str(item) for item in hints if str(item).strip()).strip()
    message = str(details.get("message") or "PTY runner is not supported in this environment.").strip()
    return f"{message}{(' ' + suffix) if suffix else ''}".strip()
