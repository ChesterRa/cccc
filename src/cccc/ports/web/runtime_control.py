from __future__ import annotations

import http.client
import os
import subprocess
import sys
import time
import urllib.error
import urllib.request
from pathlib import Path
from typing import Any, Dict, Optional

from ...paths import ensure_home
from ...util.fs import atomic_write_json, read_json
from ...util.process import resolve_background_python_argv, supervised_process_popen_kwargs, terminate_pid
from ...util.time import utc_now_iso

WEB_RUNTIME_RESTART_EXIT_CODE = 75
DEFAULT_WEB_READY_TIMEOUT_SECONDS = 10.0


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


def spawn_web_child(
    *,
    home: Path,
    host: str,
    port: int,
    mode: str,
    log_level: str,
    reload: bool,
    launch_source: str,
) -> subprocess.Popen[str]:
    argv = [
        sys.executable,
        "-m",
        "cccc.ports.web.main",
        "--serve-child",
        "--host",
        str(host),
        "--port",
        str(int(port)),
        "--mode",
        str(mode or "normal"),
        "--log-level",
        str(log_level or "info"),
    ]
    if reload:
        argv.append("--reload")

    env = os.environ.copy()
    env["CCCC_HOME"] = str(home)
    env["CCCC_WEB_MODE"] = str(mode or "normal")
    env["CCCC_WEB_SUPERVISED"] = "1"
    env["CCCC_WEB_EFFECTIVE_HOST"] = str(host)
    env["CCCC_WEB_EFFECTIVE_PORT"] = str(int(port))
    env["CCCC_WEB_EFFECTIVE_MODE"] = str(mode or "normal")
    env["CCCC_WEB_SUPERVISOR_PID"] = str(os.getpid())
    env["CCCC_WEB_LAUNCH_SOURCE"] = str(launch_source or "unknown")

    popen_kwargs: Dict[str, Any] = {
        "env": env,
        "stdin": subprocess.DEVNULL,
        "cwd": str(home),
        **supervised_process_popen_kwargs(),
    }
    if os.name == "nt":
        log_path = web_runtime_log_path(home)
        log_path.parent.mkdir(parents=True, exist_ok=True)
        log_file = log_path.open("a", encoding="utf-8")
        try:
            return subprocess.Popen(
                resolve_background_python_argv(argv),
                stdout=log_file,
                stderr=log_file,
                **popen_kwargs,
            )
        finally:
            try:
                log_file.close()
            except Exception:
                pass
    return subprocess.Popen(
        resolve_background_python_argv(argv),
        **popen_kwargs,
    )


def stop_web_child(proc: subprocess.Popen[str], *, timeout_s: float = 2.0) -> bool:
    return terminate_pid(int(getattr(proc, "pid", 0) or 0), timeout_s=timeout_s, include_group=True, force=True)


def wait_for_child_exit_interruptibly(
    proc: subprocess.Popen[str],
    *,
    poll_interval_s: float = 0.2,
) -> int:
    interval = max(float(poll_interval_s or 0.0), 0.05)
    while True:
        ret = proc.poll()
        if ret is not None:
            return int(ret or 0)
        time.sleep(interval)


def wait_for_web_ready(*, host: str, port: int, timeout_s: float = 6.0) -> bool:
    target = http_url(local_connect_host(host), int(port), path="/api/v1/health")
    deadline = time.time() + max(float(timeout_s or 0.0), 0.1)
    while time.time() < deadline:
        try:
            with urllib.request.urlopen(target, timeout=0.5) as resp:
                if int(getattr(resp, "status", 0) or 0) == 200:
                    return True
        except (urllib.error.URLError, urllib.error.HTTPError, http.client.HTTPException, OSError, ValueError):
            pass
        time.sleep(0.1)
    return False


def web_ready_timeout_seconds() -> float:
    raw = str(os.environ.get("CCCC_WEB_READY_TIMEOUT_SECONDS") or "").strip()
    if raw:
        try:
            value = float(raw)
            if value > 0:
                return value
        except Exception:
            pass
    return DEFAULT_WEB_READY_TIMEOUT_SECONDS


def start_supervised_web_child(
    *,
    home: Path,
    host: str,
    port: int,
    mode: str,
    reload: bool,
    log_level: str,
    launch_source: str,
) -> tuple[Optional[subprocess.Popen[str]], Optional[str]]:
    try:
        from .bind_preflight import ensure_tcp_port_bindable

        ensure_tcp_port_bindable(host=str(host), port=int(port))
    except RuntimeError as e:
        return None, str(e)

    proc = spawn_web_child(
        home=home,
        host=str(host),
        port=int(port),
        mode=str(mode or "normal"),
        log_level=str(log_level or "info"),
        reload=bool(reload),
        launch_source=launch_source,
    )
    runtime_pid = int(getattr(proc, "pid", 0) or 0)
    ready_timeout_s = web_ready_timeout_seconds()
    try:
        if not wait_for_web_ready(host=str(host), port=int(port), timeout_s=ready_timeout_s):
            ret = proc.poll()
            if ret is None:
                stop_web_child(proc, timeout_s=1.0)
            clear_web_runtime_state(home=home, pid=runtime_pid if runtime_pid > 0 else None)
            return None, f"web server failed to become ready on {host}:{int(port)} within {ready_timeout_s:g}s"
    except BaseException:
        try:
            if proc.poll() is None:
                stop_web_child(proc, timeout_s=1.0)
        finally:
            clear_web_runtime_state(home=home, pid=runtime_pid if runtime_pid > 0 else None)
        raise
    update_web_runtime_state({"last_apply_error": None}, home=home, pid=int(getattr(proc, "pid", 0) or 0))
    return proc, None


def restart_supervised_web_child_with_fallback(
    *,
    home: Path,
    previous_host: str,
    previous_port: int,
    mode: str,
    reload: bool,
    log_level: str,
    launch_source: str,
    resolve_binding,
    log,
) -> tuple[Optional[subprocess.Popen[str]], str, int]:
    desired_host, desired_port = resolve_binding()
    desired_proc, desired_error = start_supervised_web_child(
        home=home,
        host=desired_host,
        port=desired_port,
        mode=mode,
        reload=reload,
        log_level=log_level,
        launch_source=launch_source,
    )
    if desired_proc is not None:
        return desired_proc, desired_host, desired_port

    error_text = str(desired_error or f"failed to apply {desired_host}:{int(desired_port)}").strip()
    if desired_host == previous_host and int(desired_port) == int(previous_port):
        log(f"Error: {error_text}")
        return None, desired_host, desired_port

    log(f"Warning: failed to apply new Web binding: {error_text}")
    log("Restoring previous Web binding...")
    fallback_proc, fallback_error = start_supervised_web_child(
        home=home,
        host=previous_host,
        port=previous_port,
        mode=mode,
        reload=reload,
        log_level=log_level,
        launch_source=launch_source,
    )
    if fallback_proc is not None:
        update_web_runtime_state(
            {"last_apply_error": error_text},
            home=home,
            pid=int(getattr(fallback_proc, "pid", 0) or 0),
        )
        return fallback_proc, previous_host, int(previous_port)

    log(f"Error: failed to restore previous Web binding after apply failure ({fallback_error or 'unknown error'})")
    return None, previous_host, int(previous_port)
