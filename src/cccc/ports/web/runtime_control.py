from __future__ import annotations

import http.client
import json
import os
import secrets
import time
import urllib.error
import urllib.request
from pathlib import Path
from typing import Any, Dict, Optional

from ...kernel.access_tokens import list_access_tokens
from ...paths import ensure_home
from ...util.fs import atomic_write_json, read_json
from ...util.time import utc_now_iso

def _home_dir(home: Optional[Path] = None) -> Path:
    return Path(home).resolve() if home is not None else ensure_home()


def web_runtime_state_path(home: Optional[Path] = None) -> Path:
    return _home_dir(home) / "daemon" / "web_runtime.json"


def read_web_runtime_state(home: Optional[Path] = None) -> Dict[str, Any]:
    doc = read_json(web_runtime_state_path(home))
    return doc if isinstance(doc, dict) else {}


def web_runtime_pid_candidates(runtime: Optional[Dict[str, Any]]) -> list[int]:
    doc = runtime if isinstance(runtime, dict) else {}
    candidates: list[int] = []
    for key in ("launcher_pid", "pid"):
        try:
            candidate = int(doc.get(key) or 0)
        except Exception:
            candidate = 0
        if candidate > 0 and candidate not in candidates:
            candidates.append(candidate)
    return candidates


def write_web_runtime_state(
    *,
    home: Optional[Path] = None,
    pid: int,
    host: str,
    port: int,
    mode: str,
    supervisor_managed: bool,
    supervisor_pid: Optional[int],
    launcher_pid: Optional[int] = None,
    launch_source: str,
    last_apply_error: Optional[str] = None,
    runtime_id: Optional[str] = None,
) -> Dict[str, Any]:
    current = read_json(web_runtime_state_path(home))
    current_doc = current if isinstance(current, dict) else {}
    try:
        resolved_launcher_pid = int(launcher_pid or 0)
    except Exception:
        resolved_launcher_pid = 0
    if resolved_launcher_pid <= 0:
        try:
            resolved_launcher_pid = int(current_doc.get("launcher_pid") or 0)
        except Exception:
            resolved_launcher_pid = 0
    doc: Dict[str, Any] = {
        "pid": int(pid),
        "runtime_id": str(runtime_id or "").strip() or f"web_{secrets.token_hex(16)}",
        "host": str(host or "").strip() or "127.0.0.1",
        "port": int(port),
        "mode": str(mode or "normal").strip() or "normal",
        "started_at": utc_now_iso(),
        "supervisor_managed": bool(supervisor_managed),
        "supervisor_pid": int(supervisor_pid) if int(supervisor_pid or 0) > 0 else None,
        "launcher_pid": resolved_launcher_pid if resolved_launcher_pid > 0 else None,
        "launch_source": str(launch_source or "").strip() or "unknown",
        "last_apply_error": str(last_apply_error or "").strip() or None,
    }
    atomic_write_json(web_runtime_state_path(home), doc)
    return doc


def update_web_runtime_state(
    patch: Dict[str, Any],
    *,
    home: Optional[Path] = None,
    pid: Optional[int] = None,
) -> Dict[str, Any]:
    path = web_runtime_state_path(home)
    current = read_json(path)
    doc = current if isinstance(current, dict) else {}
    if int(pid or 0) > 0 and int(doc.get("pid") or 0) != int(pid):
        return doc
    merged = dict(doc)
    merged.update(dict(patch or {}))
    atomic_write_json(path, merged)
    return merged


def clear_web_runtime_state(*, home: Optional[Path] = None, pid: Optional[int] = None) -> None:
    path = web_runtime_state_path(home)
    if not path.exists():
        return
    if int(pid or 0) > 0:
        doc = read_json(path)
        if int(doc.get("pid") or 0) != int(pid) and int(doc.get("launcher_pid") or 0) != int(pid):
            return
    path.unlink(missing_ok=True)


def is_loopback_host(host: str) -> bool:
    normalized = str(host or "").strip().lower()
    return normalized in {"", "127.0.0.1", "localhost", "::1", "[::1]"}


def allow_unauthenticated_web_listener() -> bool:
    value = str(os.environ.get("CCCC_WEB_ALLOW_UNAUTHENTICATED") or "").strip().lower()
    return value in {"1", "true", "yes", "y", "on"}


def remote_web_exposure(*, host: str, public_url: str = "") -> bool:
    return bool(str(public_url or "").strip()) or not is_loopback_host(host)


def has_admin_access_token(home: Optional[Path] = None) -> bool:
    return any(bool(item.get("is_admin")) for item in list_access_tokens(home))


def web_listener_auth_error(*, home: Path, host: str, public_url: str = "") -> Optional[str]:
    if not remote_web_exposure(host=host, public_url=public_url) or allow_unauthenticated_web_listener():
        return None
    try:
        admin_token_available = has_admin_access_token(home)
    except Exception:
        return "refusing remote Web exposure because the access token store is unavailable"
    if admin_token_available:
        return None
    return (
        "refusing remote Web exposure without an administrator access token; "
        "use CCCC_WEB_ALLOW_UNAUTHENTICATED=1 only behind a trusted local network boundary"
    )


def is_wildcard_host(host: str) -> bool:
    normalized = str(host or "").strip().lower()
    return normalized in {"0.0.0.0", "::", "[::]"}


def _url_host_literal(host: str) -> str:
    raw = str(host or "").strip() or "127.0.0.1"
    if ":" in raw and not (raw.startswith("[") and raw.endswith("]")):
        return f"[{raw}]"
    return raw


def http_url(host: str, port: int, *, path: str = "/ui/") -> str:
    normalized_path = path if str(path or "").startswith("/") else f"/{path}"
    return f"http://{_url_host_literal(host)}:{int(port)}{normalized_path}"


def local_connect_host(host: str) -> str:
    normalized = str(host or "").strip().lower()
    if normalized in {"::", "[::]"}:
        return "::1"
    if normalized == "0.0.0.0":
        return "127.0.0.1"
    if normalized in {"localhost", ""}:
        return "127.0.0.1"
    return str(host or "").strip()


def local_display_url(host: str, port: int) -> str:
    normalized = str(host or "").strip().lower()
    if normalized in {"::", "[::]"}:
        display_host = "::1"
    elif normalized == "0.0.0.0":
        display_host = "127.0.0.1"
    else:
        display_host = str(host or "").strip() or "127.0.0.1"
    return http_url(display_host, port)


def web_runtime_log_path(home: Optional[Path] = None) -> Path:
    return _home_dir(home) / "daemon" / "cccc-web.log"

def wait_for_web_ready(
    *,
    host: str,
    port: int,
    timeout_s: float = 6.0,
    expected_runtime_id: str = "",
) -> bool:
    target = http_url(local_connect_host(host), int(port), path="/api/v1/ready")
    deadline = time.time() + max(float(timeout_s or 0.0), 0.1)
    while time.time() < deadline:
        try:
            with urllib.request.urlopen(target, timeout=0.5) as resp:
                if int(getattr(resp, "status", 0) or 0) != 200:
                    continue
                expected = str(expected_runtime_id or "").strip()
                if not expected:
                    return True
                payload = json.load(resp)
                result = payload.get("result") if isinstance(payload, dict) else None
                if (
                    isinstance(result, dict)
                    and result.get("web") == "ready"
                    and str(result.get("runtime_id") or "").strip() == expected
                ):
                    return True
        except (urllib.error.URLError, urllib.error.HTTPError, http.client.HTTPException, OSError, ValueError):
            pass
        time.sleep(0.1)
    return False
