from __future__ import annotations

"""``cccc dev`` – one-command development environment.

Starts the daemon + API server (uvicorn on :8848) **and** the Vite dev
server (HMR on :5173) so that frontend changes are hot-reloaded.
"""

import argparse
import os
import shutil
import signal
import socket
import subprocess
import sys
import threading
import time
from pathlib import Path
from typing import Optional

from .common import (
    __version__,
    _env_flag,
    _http_host_literal,
    call_daemon,
)

__all__ = ["cmd_dev"]


# ---------------------------------------------------------------------------
# helpers
# ---------------------------------------------------------------------------

def _find_web_dir() -> Optional[Path]:
    """Walk up from this file to find the repo-level ``web/`` directory."""
    try:
        for parent in Path(__file__).resolve().parents:
            candidate = parent / "web"
            if (candidate / "package.json").exists():
                return candidate
    except Exception:
        pass
    return None


def _get_lan_ip() -> str:
    try:
        s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        s.settimeout(0.1)
        s.connect(("8.8.8.8", 80))
        ip = s.getsockname()[0]
        s.close()
        return ip
    except Exception:
        return ""


# ---------------------------------------------------------------------------
# cmd_dev
# ---------------------------------------------------------------------------

def cmd_dev(args: argparse.Namespace) -> int:
    """Start daemon + web API + Vite dev server for frontend development."""
    from ..paths import ensure_home

    web_dir = _find_web_dir()
    if web_dir is None:
        print("[cccc dev] Error: could not locate web/ directory", file=sys.stderr)
        return 1

    # Check node_modules
    if not (web_dir / "node_modules").exists():
        print(f"[cccc dev] Error: {web_dir}/node_modules not found.", file=sys.stderr)
        print("[cccc dev] Run: npm install --prefix web", file=sys.stderr)
        return 1

    # Resolve npm binary
    npm_bin = shutil.which("npm")
    if npm_bin is None:
        print("[cccc dev] Error: npm not found in PATH", file=sys.stderr)
        return 1

    vite_port = int(getattr(args, "port", 5173) or 5173)
    home = ensure_home()
    log_path = home / "daemon" / "ccccd.log"

    daemon_process: Optional[subprocess.Popen] = None  # type: ignore[type-arg]
    vite_process: Optional[subprocess.Popen] = None  # type: ignore[type-arg]
    shutdown_requested = False

    # ---- daemon start (reuse _default_entry logic) -----------------------

    def _start_daemon() -> bool:
        nonlocal daemon_process
        resp = call_daemon({"op": "ping"}, timeout_s=1.0)
        if resp.get("ok"):
            try:
                res = resp.get("result") if isinstance(resp.get("result"), dict) else {}
                daemon_version = str(res.get("version") or "").strip()
                daemon_pid = int(res.get("pid") or 0)
            except Exception:
                daemon_version = ""
                daemon_pid = 0

            needs_restart = False
            if daemon_version and daemon_version != __version__:
                needs_restart = True

            if needs_restart:
                print(
                    f"[cccc dev] Daemon version mismatch (running {daemon_version}, expected {__version__}); restarting...",
                    file=sys.stderr,
                )
                try:
                    call_daemon({"op": "shutdown"}, timeout_s=2.0)
                except Exception:
                    pass
                deadline = time.time() + 3.0
                while time.time() < deadline:
                    if not call_daemon({"op": "ping"}, timeout_s=0.5).get("ok"):
                        break
                    time.sleep(0.1)
                if call_daemon({"op": "ping"}, timeout_s=0.5).get("ok") and daemon_pid > 0:
                    try:
                        os.kill(daemon_pid, signal.SIGTERM)
                    except Exception:
                        pass
                if call_daemon({"op": "ping"}, timeout_s=0.5).get("ok"):
                    print("[cccc dev] Warning: could not stop stale daemon; using existing.", file=sys.stderr)
                    return True
            else:
                print("[cccc dev] Daemon already running", file=sys.stderr)
                return True

        # Spawn daemon
        (home / "daemon").mkdir(parents=True, exist_ok=True)
        log_file = log_path.open("a", encoding="utf-8")
        try:
            daemon_process = subprocess.Popen(
                [sys.executable, "-m", "cccc.daemon_main", "run"],
                stdout=log_file,
                stderr=log_file,
                env=os.environ.copy(),
                start_new_session=True,
            )
            try:
                log_file.close()
            except Exception:
                pass
        except Exception as e:
            try:
                log_file.close()
            except Exception:
                pass
            print(f"[cccc dev] Failed to start daemon: {e}", file=sys.stderr)
            return False

        for _ in range(50):
            time.sleep(0.1)
            if daemon_process.poll() is not None:
                print(f"[cccc dev] Daemon crashed! Check log: {log_path}", file=sys.stderr)
                try:
                    lines = log_path.read_text().strip().split("\n")[-20:]
                    for line in lines:
                        print(f"  {line}", file=sys.stderr)
                except Exception:
                    pass
                return False
            if call_daemon({"op": "ping"}).get("ok"):
                return True

        print("[cccc dev] Daemon failed to start in time", file=sys.stderr)
        return False

    # ---- stop helpers ----------------------------------------------------

    def _stop_vite() -> None:
        nonlocal vite_process
        if vite_process is not None:
            try:
                vite_process.terminate()
                vite_process.wait(timeout=5.0)
            except Exception:
                try:
                    vite_process.kill()
                except Exception:
                    pass
            vite_process = None

    def _stop_daemon() -> None:
        nonlocal daemon_process
        try:
            call_daemon({"op": "shutdown"}, timeout_s=2.0)
        except Exception:
            pass
        if daemon_process is not None:
            try:
                daemon_process.wait(timeout=10.0)
            except subprocess.TimeoutExpired:
                try:
                    daemon_process.terminate()
                    daemon_process.wait(timeout=2.0)
                except Exception:
                    try:
                        daemon_process.kill()
                    except Exception:
                        pass
            daemon_process = None

    # ---- monitor threads -------------------------------------------------

    def _monitor_daemon() -> None:
        nonlocal daemon_process, shutdown_requested
        while not shutdown_requested and daemon_process is not None:
            ret = daemon_process.poll()
            if ret is not None and not shutdown_requested:
                print(f"\n[cccc dev] Daemon crashed (exit code {ret})! Check log: {log_path}", file=sys.stderr)
                break
            time.sleep(1.0)

    def _monitor_vite() -> None:
        nonlocal vite_process, shutdown_requested
        while not shutdown_requested and vite_process is not None:
            ret = vite_process.poll()
            if ret is not None and not shutdown_requested:
                print(f"\n[cccc dev] Vite dev server exited (code {ret})", file=sys.stderr)
                break
            time.sleep(1.0)

    # ---- main flow -------------------------------------------------------

    # 1) Start daemon
    print("[cccc dev] Starting daemon...", file=sys.stderr)
    if not _start_daemon():
        print("[cccc dev] Error: could not start daemon", file=sys.stderr)
        return 1
    print("[cccc dev] Daemon ready", file=sys.stderr)

    monitor_d = threading.Thread(target=_monitor_daemon, daemon=True)
    monitor_d.start()

    # 2) Start uvicorn (API server)
    host = str(os.environ.get("CCCC_WEB_HOST") or "").strip() or "0.0.0.0"
    api_port = int(os.environ.get("CCCC_WEB_PORT") or 8848)
    log_level = str(os.environ.get("CCCC_WEB_LOG_LEVEL") or "").strip() or "info"
    reload_mode = _env_flag("CCCC_WEB_RELOAD", default=False)

    import uvicorn

    config = uvicorn.Config(
        "cccc.ports.web.app:create_app",
        factory=True,
        host=host,
        port=api_port,
        log_level=log_level,
        reload=reload_mode,
        timeout_graceful_shutdown=3,
    )
    server = uvicorn.Server(config)

    # Run uvicorn in a background thread so we can manage vite in the main thread
    uvicorn_thread = threading.Thread(target=server.run, daemon=True)
    uvicorn_thread.start()

    # Wait for uvicorn to be ready
    for _ in range(50):
        time.sleep(0.1)
        if server.started:
            break

    print(f"[cccc dev] API server: http://{_http_host_literal(host)}:{api_port}", file=sys.stderr)

    # 3) Start Vite dev server
    vite_env = os.environ.copy()
    vite_env["BROWSER"] = "none"  # don't auto-open browser
    try:
        vite_process = subprocess.Popen(
            [npm_bin, "run", "dev", "--", "--port", str(vite_port), "--strictPort"],
            cwd=str(web_dir),
            env=vite_env,
        )
    except Exception as e:
        print(f"[cccc dev] Failed to start Vite: {e}", file=sys.stderr)
        shutdown_requested = True
        server.should_exit = True
        _stop_daemon()
        return 1

    monitor_v = threading.Thread(target=_monitor_vite, daemon=True)
    monitor_v.start()

    print(f"[cccc dev] Vite HMR:   http://localhost:{vite_port}/ui/", file=sys.stderr)
    lan_ip = _get_lan_ip()
    if lan_ip and lan_ip != "127.0.0.1":
        print(f"[cccc dev] Network:    http://{lan_ip}:{vite_port}/ui/", file=sys.stderr)
    print("[cccc dev] Ready! Press Ctrl+C to stop.", file=sys.stderr)

    # 4) Wait for Ctrl+C
    try:
        while True:
            # Exit if vite dies
            if vite_process is not None and vite_process.poll() is not None:
                break
            time.sleep(0.5)
    except KeyboardInterrupt:
        pass
    finally:
        print("\n[cccc dev] Shutting down...", file=sys.stderr)
        shutdown_requested = True
        _stop_vite()
        server.should_exit = True
        uvicorn_thread.join(timeout=5.0)
        _stop_daemon()
        print("[cccc dev] Stopped.", file=sys.stderr)

    return 0
