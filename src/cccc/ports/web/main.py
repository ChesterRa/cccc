from __future__ import annotations

import argparse
import os
import socket
import sys
from typing import Optional

import uvicorn

from ...daemon.server import call_daemon
from ...kernel.settings import resolve_remote_access_web_binding
from ...paths import ensure_home
from .runtime_control import (
    WEB_RUNTIME_RESTART_EXIT_CODE,
    clear_web_runtime_state,
    restart_supervised_web_child_with_fallback,
    start_supervised_web_child,
    stop_web_child,
    wait_for_child_exit_interruptibly,
)


def _check_daemon_running() -> bool:
    """Check if daemon is running (don't auto-start)."""
    resp = call_daemon({"op": "ping"})
    return resp.get("ok", False)


def _effective_binding() -> tuple[str, int]:
    binding = resolve_remote_access_web_binding()
    return str(binding.get("web_host") or "").strip() or "127.0.0.1", int(binding.get("web_port") or 8848)


def _display_local_host(host: str) -> str:
    h = str(host or "").strip()
    if h in {"0.0.0.0", "::", "[::]"}:
        return "localhost"
    return h or "localhost"


def _http_host_literal(host: str) -> str:
    h = _display_local_host(host)
    if h != "localhost" and ":" in h and not (h.startswith("[") and h.endswith("]")):
        return f"[{h}]"
    return h


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


def _print_web_banner(host: str, port: int) -> None:
    print("[cccc] Starting web server...", file=sys.stderr)
    print(f"[cccc]   Local:   http://{_http_host_literal(host)}:{int(port)}", file=sys.stderr)
    lan_ip = _get_lan_ip()
    if lan_ip and lan_ip != host and lan_ip != "127.0.0.1":
        print(f"[cccc]   Network: http://{lan_ip}:{int(port)}", file=sys.stderr)


def _run_web_child(*, host: str, port: int, mode: str, reload: bool, log_level: str) -> int:
    os.environ["CCCC_WEB_MODE"] = str(mode or "normal")
    config = uvicorn.Config(
        "cccc.ports.web.app:create_app",
        factory=True,
        host=str(host),
        port=int(port),
        log_level=str(log_level),
        reload=bool(reload),
        timeout_graceful_shutdown=0.2,
    )
    server = uvicorn.Server(config)
    try:
        server.run()
    except (KeyboardInterrupt, SystemExit):
        pass
    return 0


def _run_supervised_web(*, host: str, port: int, mode: str, reload: bool, log_level: str, launch_source: str) -> int:
    home = ensure_home()
    if not _check_daemon_running():
        print("error: daemon is not running. Start it with: cccc daemon start", file=sys.stderr)
        print("  or use: cccc (to start both daemon and web together)", file=sys.stderr)
        return 1

    proc, error_text = start_supervised_web_child(
        home=home,
        host=host,
        port=port,
        mode=mode,
        reload=reload,
        log_level=log_level,
        launch_source=launch_source,
    )
    if proc is None:
        if error_text:
            print(f"error: {error_text}", file=sys.stderr)
        return 1
    _print_web_banner(host, port)

    current_host, current_port = host, int(port)
    while True:
        try:
            ret = wait_for_child_exit_interruptibly(proc)
        except KeyboardInterrupt:
            web_pid = int(getattr(proc, "pid", 0) or 0)
            stop_web_child(proc, timeout_s=2.0)
            clear_web_runtime_state(home=home, pid=web_pid if web_pid > 0 else None)
            return 0

        if int(ret or 0) == WEB_RUNTIME_RESTART_EXIT_CODE:
            print("[cccc] Applying saved Web binding changes...", file=sys.stderr)
            restarted, current_host, current_port = restart_supervised_web_child_with_fallback(
                home=home,
                previous_host=current_host,
                previous_port=current_port,
                mode=mode,
                reload=reload,
                log_level=log_level,
                launch_source=launch_source,
                resolve_binding=_effective_binding,
                log=lambda msg: print(f"[cccc] {msg}", file=sys.stderr),
            )
            if restarted is None:
                return 1
            proc = restarted
            _print_web_banner(current_host, current_port)
            continue
        return int(ret or 0)


def main(argv: Optional[list[str]] = None) -> int:
    binding = resolve_remote_access_web_binding()
    default_host = str(binding.get("web_host") or "").strip() or "127.0.0.1"
    default_port = int(binding.get("web_port") or 8848)
    parser = argparse.ArgumentParser(prog="cccc web", description="cccc web port (FastAPI)")
    parser.add_argument("--host", default=default_host, help=f"Bind host (default: {default_host})")
    parser.add_argument("--port", type=int, default=default_port, help=f"Bind port (default: {default_port})")
    parser.add_argument(
        "--mode",
        default=str(os.environ.get("CCCC_WEB_MODE") or "normal"),
        choices=["normal", "exhibit"],
        help="Web mode: normal (read/write) or exhibit (read-only) (default: normal)",
    )
    parser.add_argument("--exhibit", action="store_true", help="Shortcut for: --mode exhibit")
    parser.add_argument("--reload", action="store_true", help="Enable autoreload (dev)")
    parser.add_argument("--log-level", default="info", help="Uvicorn log level (default: info)")
    parser.add_argument("--serve-child", action="store_true", help=argparse.SUPPRESS)
    args = parser.parse_args(argv)
    if bool(getattr(args, "exhibit", False)):
        args.mode = "exhibit"

    if bool(getattr(args, "serve_child", False)):
        return _run_web_child(
            host=str(args.host),
            port=int(args.port),
            mode=str(args.mode or "normal"),
            reload=bool(args.reload),
            log_level=str(args.log_level or "info"),
        )

    return _run_supervised_web(
        host=str(args.host),
        port=int(args.port),
        mode=str(args.mode or "normal"),
        reload=bool(args.reload),
        log_level=str(args.log_level or "info"),
        launch_source="web_cmd",
    )


if __name__ == "__main__":
    raise SystemExit(main())
