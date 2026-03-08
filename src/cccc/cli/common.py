from __future__ import annotations

import argparse
import json
import os
import socket
import subprocess
import sys
import time
import shlex
from pathlib import Path
from typing import Any, Optional

from .. import __version__
from ..contracts.v1 import ChatMessageData
from ..daemon.server import call_daemon
from ..kernel.active import load_active, set_active_group_id
from ..kernel.actors import add_actor, list_actors, remove_actor, resolve_recipient_tokens, update_actor
from ..kernel.group import (
    attach_scope_to_group,
    create_group,
    delete_group,
    detach_scope_from_group,
    ensure_group_for_scope,
    load_group,
    set_active_scope,
    update_group,
)
from ..kernel.inbox import find_event, get_cursor, get_quote_text, set_cursor, unread_messages
from ..kernel.ledger import append_event, follow, read_last_lines
from ..kernel.ledger_retention import compact as compact_ledger
from ..kernel.ledger_retention import snapshot as snapshot_ledger
from ..kernel.messaging import default_reply_recipients
from ..kernel.permissions import require_actor_permission, require_group_permission, require_inbox_permission
from ..kernel.registry import load_registry
from ..kernel.scope import detect_scope
from ..kernel.system_prompt import render_system_prompt
from ..paths import ensure_home
from ..ports.im.config_schema import canonicalize_im_config
from ..util.conv import coerce_bool

_SPACE_QUERY_OPTION_KEYS = {"source_ids"}


def _display_local_host(host: str) -> str:
    h = str(host or "").strip()
    if h in {"0.0.0.0", "::", "[::]"}:
        return "localhost"
    return h or "localhost"


def _http_host_literal(host: str) -> str:
    h = _display_local_host(host)
    # Keep localhost as-is; bracket raw IPv6 literals for URL correctness.
    if h != "localhost" and ":" in h and not (h.startswith("[") and h.endswith("]")):
        return f"[{h}]"
    return h

def _print_json(obj: Any) -> None:
    print(json.dumps(obj, ensure_ascii=False, indent=2))

def _parse_json_object_arg(raw: Any, *, field: str) -> dict[str, Any]:
    text = str(raw or "").strip()
    if not text:
        return {}
    try:
        obj = json.loads(text)
    except Exception as e:
        raise ValueError(f"{field} must be valid JSON object: {e}") from e
    if not isinstance(obj, dict):
        raise ValueError(f"{field} must be a JSON object")
    return dict(obj)

def _normalize_space_query_options_cli(options: dict[str, Any]) -> dict[str, Any]:
    normalized = dict(options or {})
    unsupported = sorted(k for k in normalized.keys() if str(k or "").strip() not in _SPACE_QUERY_OPTION_KEYS)
    if unsupported:
        if any(str(k or "").strip() in {"language", "lang"} for k in unsupported):
            raise ValueError(
                "query options do not support language/lang; NotebookLM query API has no language parameter. "
                "Put language requirements in query text."
            )
        supported = ", ".join(sorted(_SPACE_QUERY_OPTION_KEYS))
        raise ValueError(
            f"unsupported query options: {', '.join(str(k or '').strip() for k in unsupported)} (supported: {supported})"
        )

    if "source_ids" in normalized:
        raw_source_ids = normalized.get("source_ids")
        if raw_source_ids is None:
            normalized["source_ids"] = []
        elif not isinstance(raw_source_ids, list):
            raise ValueError("options.source_ids must be an array of non-empty strings")
        else:
            source_ids: list[str] = []
            for idx, item in enumerate(raw_source_ids):
                sid = str(item or "").strip()
                if not sid:
                    raise ValueError(f"options.source_ids[{idx}] must be a non-empty string")
                source_ids.append(sid)
            normalized["source_ids"] = source_ids
    return normalized

def _default_runner_kind() -> str:
    """Use the current product-standard runner."""
    return "pty"

def _ensure_daemon_running() -> bool:
    resp = call_daemon({"op": "ping"}, timeout_s=1.0)
    if resp.get("ok"):
        # If the daemon is from a different version, restart it. This commonly happens
        # after a package upgrade while an old background daemon is still running,
        # causing "unknown op" errors in newer Web/UI flows.
        try:
            res = resp.get("result") if isinstance(resp.get("result"), dict) else {}
            daemon_version = str(res.get("version") or "").strip()
            daemon_pid = int(res.get("pid") or 0)
        except Exception:
            daemon_version = ""
            daemon_pid = 0

        def _daemon_supports_required_ops() -> bool:
            try:
                # Probe a couple of newer ops so we don't get stuck with a stale
                # background daemon that lacks features (even if version string matches).
                for probe in (
                    {"op": "observability_get"},
                    {"op": "debug_snapshot", "args": {}},
                ):
                    r = call_daemon(probe, timeout_s=1.0)
                    if r.get("ok"):
                        continue
                    err = r.get("error") if isinstance(r.get("error"), dict) else {}
                    if str(err.get("code") or "") == "unknown_op":
                        return False
                return True
            except Exception:
                return False

        needs_restart = False
        if daemon_version and daemon_version != __version__:
            needs_restart = True
        elif not _daemon_supports_required_ops():
            needs_restart = True

        if needs_restart:
            try:
                shutdown_resp = call_daemon({"op": "shutdown"}, timeout_s=2.0)
                if not bool(shutdown_resp.get("ok")):
                    print("warn: daemon restart requested but shutdown RPC failed; trying fallback termination", file=sys.stderr)
            except Exception as e:
                print(f"warn: daemon restart requested but shutdown RPC errored: {e}", file=sys.stderr)

            deadline = time.time() + 2.0
            while time.time() < deadline:
                if not call_daemon({"op": "ping"}, timeout_s=0.5).get("ok"):
                    break
                time.sleep(0.1)

            # Last resort: terminate the stale daemon by pid (best-effort).
            if call_daemon({"op": "ping"}, timeout_s=0.5).get("ok") and daemon_pid > 0:
                try:
                    import signal

                    killed = False
                    try:
                        os.killpg(os.getpgid(daemon_pid), signal.SIGTERM)
                        killed = True
                    except Exception as e_pg:
                        try:
                            os.kill(daemon_pid, signal.SIGTERM)
                            killed = True
                        except Exception as e_kill:
                            print(
                                f"warn: failed to terminate stale daemon pid={daemon_pid}: killpg={e_pg}; kill={e_kill}",
                                file=sys.stderr,
                            )
                    if not killed:
                        return True
                except Exception as e:
                    print(f"warn: failed to terminate stale daemon pid={daemon_pid}: {e}", file=sys.stderr)
                    return True

                deadline = time.time() + 2.0
                while time.time() < deadline:
                    if not call_daemon({"op": "ping"}, timeout_s=0.5).get("ok"):
                        break
                    time.sleep(0.1)

            # If it's still running, don't stomp its socket/pid files.
            if call_daemon({"op": "ping"}, timeout_s=0.5).get("ok"):
                return True

            # Cleanup stale socket/pid files so a new daemon can bind.
            try:
                home = ensure_home()
                sock_path = home / "daemon" / "ccccd.sock"
                addr_path = home / "daemon" / "ccccd.addr.json"
                pid_path = home / "daemon" / "ccccd.pid"
                sock_path.unlink(missing_ok=True)
                addr_path.unlink(missing_ok=True)
                pid_path.unlink(missing_ok=True)
            except Exception as e:
                print(f"warn: failed to cleanup stale daemon state files: {e}", file=sys.stderr)
        else:
            return True

    try:
        subprocess.run(
            [sys.executable, "-m", "cccc.daemon_main", "start"],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            check=False,
        )
    except Exception:
        return False

    # Windows TCP startup can take longer than POSIX AF_UNIX; be patient but bounded.
    attempts = 200 if os.name == "nt" else 60
    for _ in range(attempts):
        time.sleep(0.05)
        resp = call_daemon({"op": "ping"}, timeout_s=0.5)
        if resp.get("ok"):
            return True
    return False

def _resolve_group_id(explicit: str) -> str:
    gid = (explicit or "").strip()
    if gid:
        return gid
    active = load_active()
    return str(active.get("active_group_id") or "").strip()

def _env_flag(name: str, *, default: bool = False) -> bool:
    return coerce_bool(os.environ.get(name), default=default)

def _is_first_run() -> bool:
    """Check if this is the first time running CCCC."""
    home = ensure_home()
    marker = home / ".initialized"
    if marker.exists():
        return False
    # Create marker file
    try:
        marker.write_text(__version__)
    except Exception:
        pass
    return True

def _show_welcome() -> None:
    """Show welcome message for first-time users."""
    print()
    print("=" * 60)
    print("  Welcome to CCCC - Collaborative Code Coordination Center")
    print("=" * 60)
    print()
    print("Quick Start:")
    print("  1. Create a working group:  cccc attach .")
    print("  2. Add an agent:            cccc actor add my-agent --runtime claude")
    print("  3. Start the group:         cccc group start")
    print("  4. Open Web UI:             http://127.0.0.1:8848")
    print()
    print("Useful Commands:")
    print("  cccc status      - Show daemon, groups, and actors")
    print("  cccc doctor      - Check environment and available runtimes")
    print("  cccc setup       - Configure MCP for agent runtimes")
    print()
    print("Documentation: https://github.com/ChesterRa/cccc")
    print("=" * 60)
    print()

def _default_entry() -> int:
    """Default entry: start daemon + web together, stop both on Ctrl+C."""
    import signal
    import threading
    
    from ..paths import ensure_home
    
    # Show welcome message on first run
    if _is_first_run():
        _show_welcome()
    
    daemon_process = None
    shutdown_requested = False
    home = ensure_home()
    log_path = home / "daemon" / "ccccd.log"
    
    def _start_daemon() -> bool:
        nonlocal daemon_process
        # Check if already running
        resp = call_daemon({"op": "ping"}, timeout_s=1.0)
        if resp.get("ok"):
            try:
                res = resp.get("result") if isinstance(resp.get("result"), dict) else {}
                daemon_version = str(res.get("version") or "").strip()
                daemon_pid = int(res.get("pid") or 0)
            except Exception:
                daemon_version = ""
                daemon_pid = 0

            def _daemon_supports_required_ops() -> bool:
                try:
                    for probe in (
                        {"op": "observability_get"},
                        {"op": "debug_snapshot", "args": {}},
                    ):
                        r = call_daemon(probe, timeout_s=1.0)
                        if r.get("ok"):
                            continue
                        err = r.get("error") if isinstance(r.get("error"), dict) else {}
                        if str(err.get("code") or "") == "unknown_op":
                            return False
                    return True
                except Exception:
                    return True

            needs_restart = False
            if daemon_version and daemon_version != __version__:
                needs_restart = True
            elif not _daemon_supports_required_ops():
                needs_restart = True

            if needs_restart:
                if daemon_version and daemon_version != __version__:
                    msg = (
                        f"[cccc] Detected daemon version mismatch (running {daemon_version}, expected {__version__}); restarting daemon..."
                    )
                else:
                    msg = "[cccc] Detected stale daemon missing required ops; restarting daemon..."
                print(msg, file=sys.stderr)
                try:
                    call_daemon({"op": "shutdown"}, timeout_s=2.0)
                except Exception:
                    pass

                deadline = time.time() + 2.0
                while time.time() < deadline:
                    if not call_daemon({"op": "ping"}, timeout_s=0.5).get("ok"):
                        break
                    time.sleep(0.1)

                if call_daemon({"op": "ping"}, timeout_s=0.5).get("ok") and daemon_pid > 0:
                    try:
                        try:
                            os.killpg(os.getpgid(daemon_pid), signal.SIGTERM)
                        except Exception:
                            try:
                                os.kill(daemon_pid, signal.SIGTERM)
                            except Exception:
                                pass
                    except Exception:
                        pass

                    deadline = time.time() + 2.0
                    while time.time() < deadline:
                        if not call_daemon({"op": "ping"}, timeout_s=0.5).get("ok"):
                            break
                        time.sleep(0.1)

                # If it's still running, don't stomp its socket/pid files.
                if call_daemon({"op": "ping"}, timeout_s=0.5).get("ok"):
                    print("[cccc] Warning: could not stop stale daemon; continuing with existing daemon.", file=sys.stderr)
                    return True
            else:
                return True
        
        # Clean up stale socket/pid files.
        # If a daemon pid is present but unresponsive, terminate it first to avoid orphan daemons.
        sock_path = home / "daemon" / "ccccd.sock"
        addr_path = home / "daemon" / "ccccd.addr.json"
        pid_path = home / "daemon" / "ccccd.pid"
        try:
            pid = 0
            if pid_path.exists():
                txt = pid_path.read_text(encoding="utf-8").strip()
                pid = int(txt) if txt.isdigit() else 0
            if pid > 0:
                try:
                    os.kill(pid, 0)
                except Exception:
                    pid = 0
            if pid > 0:
                def _pid_alive_local(p: int) -> bool:
                    try:
                        os.kill(p, 0)
                        return True
                    except Exception:
                        return False

                try:
                    os.killpg(os.getpgid(pid), signal.SIGTERM)
                except Exception:
                    try:
                        os.kill(pid, signal.SIGTERM)
                    except Exception:
                        pass
                deadline = time.time() + 2.0
                while time.time() < deadline:
                    if not _pid_alive_local(pid):
                        break
                    time.sleep(0.05)
                if _pid_alive_local(pid):
                    try:
                        os.killpg(os.getpgid(pid), signal.SIGKILL)
                    except Exception:
                        try:
                            os.kill(pid, signal.SIGKILL)
                        except Exception:
                            pass

            sock_path.unlink(missing_ok=True)
            addr_path.unlink(missing_ok=True)
            pid_path.unlink(missing_ok=True)
        except Exception:
            pass
        
        # Start daemon as subprocess, capture output to log file
        (home / "daemon").mkdir(parents=True, exist_ok=True)
        log_file = log_path.open("a", encoding="utf-8")
        try:
            daemon_process = subprocess.Popen(
                [sys.executable, "-m", "cccc.daemon_main", "run"],
                stdout=log_file,
                stderr=log_file,
                env=os.environ.copy(),
                start_new_session=True,  # Don't forward SIGINT to daemon
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
            print(f"[cccc] Failed to start daemon: {e}", file=sys.stderr)
            return False
        
        # Wait for daemon to be ready
        for _ in range(50):
            time.sleep(0.1)
            # Check if daemon crashed
            if daemon_process.poll() is not None:
                print(f"[cccc] Daemon crashed! Check log: {log_path}", file=sys.stderr)
                # Show last 20 lines of log
                try:
                    lines = log_path.read_text().strip().split("\n")[-20:]
                    for line in lines:
                        print(f"  {line}", file=sys.stderr)
                except Exception:
                    pass
                return False
            resp = call_daemon({"op": "ping"})
            if resp.get("ok"):
                return True
        
        print("[cccc] Daemon failed to start in time", file=sys.stderr)
        return False
    
    def _monitor_daemon() -> None:
        """Background thread to monitor daemon and report crashes."""
        nonlocal daemon_process, shutdown_requested
        while not shutdown_requested and daemon_process is not None:
            ret = daemon_process.poll()
            if ret is not None and not shutdown_requested:
                print(f"\n[cccc] Daemon crashed (exit code {ret})! Check log: {log_path}", file=sys.stderr)
                try:
                    lines = log_path.read_text().strip().split("\n")[-15:]
                    for line in lines:
                        print(f"  {line}", file=sys.stderr)
                except Exception:
                    pass
                break
            time.sleep(1.0)
    
    def _stop_daemon() -> None:
        nonlocal daemon_process
        # Send shutdown command (works even if we didn't start the daemon)
        try:
            call_daemon({"op": "shutdown"}, timeout_s=2.0)
        except Exception:
            pass

        # Wait for our subprocess to exit (if we started it)
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
    
    # Start daemon
    print("[cccc] Starting daemon...", file=sys.stderr)
    if not _start_daemon():
        print("[cccc] Error: Could not start daemon", file=sys.stderr)
        return 1
    print("[cccc] Daemon started", file=sys.stderr)

    # Start daemon monitor thread
    monitor_thread = threading.Thread(target=_monitor_daemon, daemon=True)
    monitor_thread.start()

    # Build web args from environment
    host = str(os.environ.get("CCCC_WEB_HOST") or "").strip() or "0.0.0.0"
    port = int(os.environ.get("CCCC_WEB_PORT") or 8848)
    log_level = str(os.environ.get("CCCC_WEB_LOG_LEVEL") or "").strip() or "info"
    reload_mode = _env_flag("CCCC_WEB_RELOAD", default=False)
    
    # Run web. Let uvicorn own signal handling; set a bounded graceful timeout to
    # avoid hanging forever on long-lived connections (e.g. SSE/WebSocket).
    import uvicorn

    config = uvicorn.Config(
        "cccc.ports.web.app:create_app",
        factory=True,
        host=host,
        port=port,
        log_level=log_level,
        reload=reload_mode,
        timeout_graceful_shutdown=3,
    )
    server = uvicorn.Server(config)

    # Get LAN IP for display
    def _get_lan_ip() -> str:
        try:
            # Create a socket to an external address to determine the local IP
            s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
            s.settimeout(0.1)
            s.connect(("8.8.8.8", 80))
            ip = s.getsockname()[0]
            s.close()
            return ip
        except Exception:
            return ""

    try:
        print("[cccc] Starting web server...", file=sys.stderr)
        print(f"[cccc]   Local:   http://{_http_host_literal(host)}:{port}", file=sys.stderr)
        lan_ip = _get_lan_ip()
        if lan_ip and lan_ip != host and lan_ip != "127.0.0.1":
            print(f"[cccc]   Network: http://{lan_ip}:{port}", file=sys.stderr)
        server.run()
    except (SystemExit, KeyboardInterrupt):
        pass
    finally:
        shutdown_requested = True
        _stop_daemon()
    
    return 0


# Export helper symbols (including leading underscore names) for CLI submodules.
__all__ = [name for name in globals().keys() if not name.startswith("__") or name == "__version__"]
