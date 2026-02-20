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

from . import __version__
from .contracts.v1 import ChatMessageData
from .daemon.server import call_daemon
from .kernel.active import load_active, set_active_group_id
from .kernel.actors import add_actor, list_actors, remove_actor, resolve_recipient_tokens, update_actor
from .kernel.group import (
    attach_scope_to_group,
    create_group,
    delete_group,
    detach_scope_from_group,
    ensure_group_for_scope,
    load_group,
    set_active_scope,
    update_group,
)
from .kernel.inbox import find_event, get_cursor, get_quote_text, set_cursor, unread_messages
from .kernel.ledger import append_event, follow, read_last_lines
from .kernel.ledger_retention import compact as compact_ledger
from .kernel.ledger_retention import snapshot as snapshot_ledger
from .kernel.messaging import default_reply_recipients
from .kernel.permissions import require_actor_permission, require_group_permission, require_inbox_permission
from .kernel.registry import load_registry
from .kernel.scope import detect_scope
from .kernel.system_prompt import render_system_prompt
from .paths import ensure_home
from .util.conv import coerce_bool


def _print_json(obj: Any) -> None:
    print(json.dumps(obj, ensure_ascii=False, indent=2))


def _default_runner_kind() -> str:
    """Pick a sensible default runner for this platform."""
    try:
        from .runners import pty as pty_runner

        return "pty" if bool(getattr(pty_runner, "PTY_SUPPORTED", True)) else "headless"
    except Exception:
        return "headless"


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
                return True

        needs_restart = False
        if daemon_version and daemon_version != __version__:
            needs_restart = True
        elif not _daemon_supports_required_ops():
            needs_restart = True

        if needs_restart:
            try:
                call_daemon({"op": "shutdown"}, timeout_s=2.0)
            except Exception:
                pass

            deadline = time.time() + 2.0
            while time.time() < deadline:
                if not call_daemon({"op": "ping"}, timeout_s=0.5).get("ok"):
                    break
                time.sleep(0.1)

            # Last resort: terminate the stale daemon by pid (best-effort).
            if call_daemon({"op": "ping"}, timeout_s=0.5).get("ok") and daemon_pid > 0:
                try:
                    import signal

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
            except Exception:
                pass
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
    
    from .paths import ensure_home
    
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
    host = str(os.environ.get("CCCC_WEB_HOST") or "").strip() or "127.0.0.1"
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
        print(f"[cccc]   Local:   http://{host}:{port}", file=sys.stderr)
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


def cmd_attach(args: argparse.Namespace) -> int:
    if _ensure_daemon_running():
        resp = call_daemon(
            {"op": "attach", "args": {"path": args.path, "by": "cli", "group_id": str(args.group_id or "")}}
        )
        if resp.get("ok"):
            try:
                gid = str((resp.get("result") or {}).get("group_id") or "").strip()
                if gid:
                    set_active_group_id(gid)
            except Exception:
                pass
            _print_json(resp)
            return 0

    # Fallback: local execution (dev convenience)
    scope = detect_scope(Path(args.path))
    reg = load_registry()
    if args.group_id:
        group = load_group(str(args.group_id))
        if group is None:
            _print_json({"ok": False, "error": f"group not found: {args.group_id}"})
            return 2
        group = attach_scope_to_group(reg, group, scope, set_active=True)
    else:
        group = ensure_group_for_scope(reg, scope)
    append_event(
        group.ledger_path,
        kind="group.attach",
        group_id=group.group_id,
        scope_key=scope.scope_key,
        by="cli",
        data={"url": scope.url, "label": scope.label, "git_remote": scope.git_remote},
    )
    set_active_group_id(group.group_id)
    _print_json(
        {
            "ok": True,
            "result": {"group_id": group.group_id, "scope_key": scope.scope_key, "title": group.doc.get("title")},
        }
    )
    return 0


def cmd_group_create(args: argparse.Namespace) -> int:
    if _ensure_daemon_running():
        resp = call_daemon({"op": "group_create", "args": {"title": args.title, "topic": str(args.topic or ""), "by": "cli"}})
        if resp.get("ok"):
            try:
                gid = str((resp.get("result") or {}).get("group_id") or "").strip()
                if gid:
                    set_active_group_id(gid)
            except Exception:
                pass
            _print_json(resp)
            return 0

    reg = load_registry()
    group = create_group(reg, title=str(args.title or "working-group"), topic=str(args.topic or ""))
    ev = append_event(
        group.ledger_path,
        kind="group.create",
        group_id=group.group_id,
        scope_key="",
        by="cli",
        data={"title": group.doc.get("title", ""), "topic": group.doc.get("topic", "")},
    )
    set_active_group_id(group.group_id)
    _print_json({"ok": True, "result": {"group_id": group.group_id, "title": group.doc.get("title"), "event": ev}})
    return 0


def cmd_group_show(args: argparse.Namespace) -> int:
    group = load_group(args.group_id)
    if group is None:
        _print_json({"ok": False, "error": {"code": "group_not_found", "message": f"group not found: {args.group_id}"}})
        return 2
    _print_json({"ok": True, "result": {"group": group.doc}})
    return 0


def cmd_group_update(args: argparse.Namespace) -> int:
    group_id = _resolve_group_id(getattr(args, "group", ""))
    if not group_id:
        _print_json({"ok": False, "error": {"code": "missing_group_id", "message": "missing group_id (no active group?)"}})
        return 2
    by = str(args.by or "user").strip()

    patch: dict[str, Any] = {}
    if args.title is not None:
        title = str(args.title or "").strip()
        if not title:
            _print_json({"ok": False, "error": {"code": "invalid_title", "message": "title cannot be empty"}})
            return 2
        patch["title"] = title
    if args.topic is not None:
        patch["topic"] = str(args.topic or "")
    if not patch:
        _print_json({"ok": False, "error": {"code": "invalid_patch", "message": "provide --title and/or --topic"}})
        return 2

    if _ensure_daemon_running():
        resp = call_daemon({"op": "group_update", "args": {"group_id": group_id, "by": by, "patch": patch}})
        if resp.get("ok"):
            _print_json(resp)
            return 0

    group = load_group(group_id)
    if group is None:
        _print_json({"ok": False, "error": {"code": "group_not_found", "message": f"group not found: {group_id}"}})
        return 2
    try:
        require_group_permission(group, by=by, action="group.update")
        reg = load_registry()
        group = update_group(reg, group, patch=dict(patch))
    except Exception as e:
        _print_json({"ok": False, "error": {"code": "group_update_failed", "message": str(e)}})
        return 2
    ev = append_event(group.ledger_path, kind="group.update", group_id=group.group_id, scope_key="", by=by, data={"patch": dict(patch)})
    _print_json({"ok": True, "result": {"group_id": group.group_id, "group": group.doc, "event": ev}})
    return 0


def cmd_group_detach_scope(args: argparse.Namespace) -> int:
    group_id = _resolve_group_id(getattr(args, "group", ""))
    if not group_id:
        _print_json({"ok": False, "error": {"code": "missing_group_id", "message": "missing group_id (no active group?)"}})
        return 2
    by = str(args.by or "user").strip()
    scope_key = str(args.scope_key or "").strip()
    if not scope_key:
        _print_json({"ok": False, "error": {"code": "missing_scope_key", "message": "missing scope_key"}})
        return 2

    if _ensure_daemon_running():
        resp = call_daemon({"op": "group_detach_scope", "args": {"group_id": group_id, "scope_key": scope_key, "by": by}})
        if resp.get("ok"):
            _print_json(resp)
            return 0

    group = load_group(group_id)
    if group is None:
        _print_json({"ok": False, "error": {"code": "group_not_found", "message": f"group not found: {group_id}"}})
        return 2
    try:
        require_group_permission(group, by=by, action="group.detach_scope")
        reg = load_registry()
        group = detach_scope_from_group(reg, group, scope_key=scope_key)
    except Exception as e:
        _print_json({"ok": False, "error": {"code": "group_detach_scope_failed", "message": str(e)}})
        return 2
    ev = append_event(
        group.ledger_path,
        kind="group.detach_scope",
        group_id=group.group_id,
        scope_key=scope_key,
        by=by,
        data={"scope_key": scope_key},
    )
    _print_json({"ok": True, "result": {"group_id": group.group_id, "event": ev}})
    return 0


def cmd_group_delete(args: argparse.Namespace) -> int:
    group_id = _resolve_group_id(getattr(args, "group", ""))
    if not group_id:
        _print_json({"ok": False, "error": {"code": "missing_group_id", "message": "missing group_id (no active group?)"}})
        return 2
    by = str(args.by or "user").strip()
    confirm = str(args.confirm or "").strip()
    if confirm != group_id:
        _print_json({"ok": False, "error": {"code": "confirm_required", "message": f"pass --confirm {group_id} to delete"}})
        return 2

    if not _ensure_daemon_running():
        _print_json({"ok": False, "error": {"code": "daemon_unavailable", "message": "ccccd unavailable"}})
        return 2
    resp = call_daemon({"op": "group_delete", "args": {"group_id": group_id, "by": by}})
    _print_json(resp)
    return 0 if resp.get("ok") else 2


def cmd_group_use(args: argparse.Namespace) -> int:
    if _ensure_daemon_running():
        resp = call_daemon({"op": "group_use", "args": {"group_id": args.group_id, "path": args.path, "by": "cli"}})
        if resp.get("ok"):
            _print_json(resp)
            return 0

    group = load_group(args.group_id)
    if group is None:
        _print_json({"ok": False, "error": {"code": "group_not_found", "message": f"group not found: {args.group_id}"}})
        return 2
    scope = detect_scope(Path(args.path))
    reg = load_registry()
    try:
        group = set_active_scope(reg, group, scope_key=scope.scope_key)
    except ValueError as e:
        _print_json(
            {
                "ok": False,
                "error": {"code": "scope_not_attached", "message": str(e), "details": {"hint": "cccc attach <path> --group <id>"}},
            }
        )
        return 2
    ev = append_event(
        group.ledger_path,
        kind="group.set_active_scope",
        group_id=group.group_id,
        scope_key=scope.scope_key,
        by="cli",
        data={"path": scope.url},
    )
    _print_json({"ok": True, "result": {"group_id": group.group_id, "active_scope_key": scope.scope_key, "event": ev}})
    return 0


def cmd_group_start(args: argparse.Namespace) -> int:
    group_id = _resolve_group_id(getattr(args, "group", ""))
    if not group_id:
        _print_json({"ok": False, "error": {"code": "missing_group_id", "message": "missing group_id (no active group?)"}})
        return 2
    by = str(args.by or "user").strip()

    if not _ensure_daemon_running():
        _print_json({"ok": False, "error": {"code": "daemon_unavailable", "message": "ccccd unavailable"}})
        return 2
    resp = call_daemon({"op": "group_start", "args": {"group_id": group_id, "by": by}})
    _print_json(resp)
    return 0 if resp.get("ok") else 2


def cmd_group_stop(args: argparse.Namespace) -> int:
    group_id = _resolve_group_id(getattr(args, "group", ""))
    if not group_id:
        _print_json({"ok": False, "error": {"code": "missing_group_id", "message": "missing group_id (no active group?)"}})
        return 2
    by = str(args.by or "user").strip()

    if not _ensure_daemon_running():
        _print_json({"ok": False, "error": {"code": "daemon_unavailable", "message": "ccccd unavailable"}})
        return 2
    resp = call_daemon({"op": "group_stop", "args": {"group_id": group_id, "by": by}})
    _print_json(resp)
    return 0 if resp.get("ok") else 2


def cmd_group_set_state(args: argparse.Namespace) -> int:
    """Set group state (active/idle/paused/stopped)."""
    group_id = _resolve_group_id(getattr(args, "group", ""))
    if not group_id:
        _print_json({"ok": False, "error": {"code": "missing_group_id", "message": "missing group_id (no active group?)"}})
        return 2
    state = str(args.state or "").strip()
    by = str(args.by or "user").strip()

    if not _ensure_daemon_running():
        _print_json({"ok": False, "error": {"code": "daemon_unavailable", "message": "ccccd unavailable"}})
        return 2
    if state == "stopped":
        resp = call_daemon({"op": "group_stop", "args": {"group_id": group_id, "by": by}})
    else:
        resp = call_daemon({"op": "group_set_state", "args": {"group_id": group_id, "state": state, "by": by}})
    _print_json(resp)
    return 0 if resp.get("ok") else 2


def cmd_groups(args: argparse.Namespace) -> int:
    resp = call_daemon({"op": "groups"})
    if resp.get("ok"):
        _print_json(resp)
        return 0
    reg = load_registry()
    groups = list(reg.groups.values())
    groups.sort(key=lambda g: (g.get("updated_at") or "", g.get("created_at") or ""), reverse=True)
    _print_json({"ok": True, "result": {"groups": groups}})
    return 0


def cmd_send(args: argparse.Namespace) -> int:
    group_id = _resolve_group_id(getattr(args, "group", ""))
    if not group_id:
        _print_json({"ok": False, "error": {"code": "missing_group_id", "message": "missing group_id (no active group?)"}})
        return 2
    group = load_group(group_id)
    if group is None:
        _print_json({"ok": False, "error": {"code": "group_not_found", "message": f"group not found: {group_id}"}})
        return 2

    to_tokens: list[str] = []
    to_raw = getattr(args, "to", None)
    if isinstance(to_raw, list):
        for item in to_raw:
            if not isinstance(item, str):
                continue
            parts = [p.strip() for p in item.split(",") if p.strip()]
            to_tokens.extend(parts)
    priority = str(getattr(args, "priority", "normal") or "normal").strip() or "normal"
    if priority not in ("normal", "attention"):
        _print_json({"ok": False, "error": {"code": "invalid_priority", "message": "priority must be 'normal' or 'attention'"}})
        return 2
    reply_required = bool(getattr(args, "reply_required", False))

    if _ensure_daemon_running():
        resp = call_daemon(
            {
                "op": "send",
                "args": {
                    "group_id": group_id,
                    "text": args.text,
                    "by": str(args.by or "user"),
                    "path": str(args.path or ""),
                    "to": to_tokens,
                    "priority": priority,
                    "reply_required": reply_required,
                },
            }
        )
        if resp.get("ok"):
            _print_json(resp)
            return 0

    # Fallback: local execution (dev convenience)
    try:
        to = resolve_recipient_tokens(group, to_tokens)
    except Exception as e:
        _print_json({"ok": False, "error": {"code": "invalid_recipient", "message": str(e)}})
        return 2
    scope_key = str(group.doc.get("active_scope_key") or "")
    if args.path:
        scope = detect_scope(Path(args.path))
        scope_key = scope.scope_key
        scopes = group.doc.get("scopes")
        attached = False
        if isinstance(scopes, list):
            attached = any(isinstance(item, dict) and item.get("scope_key") == scope_key for item in scopes)
        if not attached:
            _print_json(
                {
                    "ok": False,
                    "error": {
                        "code": "scope_not_attached",
                        "message": f"scope not attached: {scope_key}",
                        "details": {"hint": "cccc attach <path> --group <id>"},
                    },
                }
            )
            return 2
    if not scope_key:
        scope_key = ""
    event = append_event(
        group.ledger_path,
        kind="chat.message",
        group_id=group.group_id,
        scope_key=scope_key,
        by=str(args.by or "user"),
        data=ChatMessageData(
            text=args.text,
            format="plain",
            to=to,
            priority=priority,
            reply_required=reply_required,
        ).model_dump(),
    )
    try:
        reg = load_registry()
        meta = reg.groups.get(group.group_id)
        if isinstance(meta, dict):
            ts = str(event.get("ts") or meta.get("updated_at") or "")
            if ts:
                meta["updated_at"] = ts
                reg.save()
    except Exception:
        pass
    _print_json({"ok": True, "result": {"event": event}})
    return 0


def cmd_reply(args: argparse.Namespace) -> int:
    """Reply to a message (IM-style, with quote)"""
    group_id = _resolve_group_id(getattr(args, "group", ""))
    if not group_id:
        _print_json({"ok": False, "error": {"code": "missing_group_id", "message": "missing group_id (no active group?)"}})
        return 2
    group = load_group(group_id)
    if group is None:
        _print_json({"ok": False, "error": {"code": "group_not_found", "message": f"group not found: {group_id}"}})
        return 2

    reply_to = str(args.event_id or "").strip()
    if not reply_to:
        _print_json({"ok": False, "error": {"code": "missing_event_id", "message": "missing event_id to reply to"}})
        return 2

    # Find the original message to get quote_text
    original = find_event(group, reply_to)
    if original is None:
        _print_json({"ok": False, "error": {"code": "event_not_found", "message": f"event not found: {reply_to}"}})
        return 2

    quote_text = get_quote_text(group, reply_to, max_len=100)

    to_tokens: list[str] = []
    to_raw = getattr(args, "to", None)
    if isinstance(to_raw, list):
        for item in to_raw:
            if not isinstance(item, str):
                continue
            parts = [p.strip() for p in item.split(",") if p.strip()]
            to_tokens.extend(parts)

    priority = str(getattr(args, "priority", "normal") or "normal").strip() or "normal"
    if priority not in ("normal", "attention"):
        _print_json({"ok": False, "error": {"code": "invalid_priority", "message": "priority must be 'normal' or 'attention'"}})
        return 2
    reply_required = bool(getattr(args, "reply_required", False))

    if _ensure_daemon_running():
        resp = call_daemon(
            {
                "op": "reply",
                "args": {
                    "group_id": group_id,
                    "text": args.text,
                    "by": str(args.by or "user"),
                    "reply_to": reply_to,
                    "to": to_tokens,
                    "priority": priority,
                    "reply_required": reply_required,
                },
            }
        )
        if resp.get("ok"):
            _print_json(resp)
            return 0

    # Fallback: local execution
    if not to_tokens:
        to_tokens = default_reply_recipients(group, by=str(args.by or "user"), original_event=original)
    try:
        to = resolve_recipient_tokens(group, to_tokens)
    except Exception as e:
        _print_json({"ok": False, "error": {"code": "invalid_recipient", "message": str(e)}})
        return 2

    scope_key = str(group.doc.get("active_scope_key") or "")
    event = append_event(
        group.ledger_path,
        kind="chat.message",
        group_id=group.group_id,
        scope_key=scope_key,
        by=str(args.by or "user"),
        data=ChatMessageData(
            text=args.text,
            format="plain",
            to=to,
            priority=priority,
            reply_required=reply_required,
            reply_to=reply_to,
            quote_text=quote_text,
        ).model_dump(),
    )
    try:
        reg = load_registry()
        meta = reg.groups.get(group.group_id)
        if isinstance(meta, dict):
            ts = str(event.get("ts") or meta.get("updated_at") or "")
            if ts:
                meta["updated_at"] = ts
                reg.save()
    except Exception:
        pass
    _print_json({"ok": True, "result": {"event": event}})
    return 0


def cmd_tail(args: argparse.Namespace) -> int:
    group_id = _resolve_group_id(getattr(args, "group", ""))
    if not group_id:
        _print_json({"ok": False, "error": {"code": "missing_group_id", "message": "missing group_id (no active group?)"}})
        return 2
    group = load_group(group_id)
    if group is None:
        _print_json({"ok": False, "error": {"code": "group_not_found", "message": f"group not found: {group_id}"}})
        return 2
    if args.follow:
        for line in follow(group.ledger_path):
            print(line)
        return 0
    for line in read_last_lines(group.ledger_path, args.lines):
        print(line)
    return 0


def cmd_ledger_snapshot(args: argparse.Namespace) -> int:
    group_id = _resolve_group_id(getattr(args, "group", ""))
    if not group_id:
        _print_json({"ok": False, "error": {"code": "missing_group_id", "message": "missing group_id (no active group?)"}})
        return 2
    by = str(args.by or "user").strip()
    reason = str(args.reason or "manual").strip()

    if _ensure_daemon_running():
        resp = call_daemon({"op": "ledger_snapshot", "args": {"group_id": group_id, "by": by, "reason": reason}})
        _print_json(resp)
        return 0 if resp.get("ok") else 2

    group = load_group(group_id)
    if group is None:
        _print_json({"ok": False, "error": {"code": "group_not_found", "message": f"group not found: {group_id}"}})
        return 2
    try:
        require_group_permission(group, by=by, action="group.update")
        snap = snapshot_ledger(group, reason=reason)
    except Exception as e:
        _print_json({"ok": False, "error": {"code": "ledger_snapshot_failed", "message": str(e)}})
        return 2
    _print_json({"ok": True, "result": {"snapshot": snap}})
    return 0


def cmd_ledger_compact(args: argparse.Namespace) -> int:
    group_id = _resolve_group_id(getattr(args, "group", ""))
    if not group_id:
        _print_json({"ok": False, "error": {"code": "missing_group_id", "message": "missing group_id (no active group?)"}})
        return 2
    by = str(args.by or "user").strip()
    reason = str(args.reason or "manual").strip()
    force = bool(args.force)

    if _ensure_daemon_running():
        resp = call_daemon(
            {"op": "ledger_compact", "args": {"group_id": group_id, "by": by, "reason": reason, "force": force}}
        )
        _print_json(resp)
        return 0 if resp.get("ok") else 2

    group = load_group(group_id)
    if group is None:
        _print_json({"ok": False, "error": {"code": "group_not_found", "message": f"group not found: {group_id}"}})
        return 2
    try:
        require_group_permission(group, by=by, action="group.update")
        res = compact_ledger(group, reason=reason, force=force)
    except Exception as e:
        _print_json({"ok": False, "error": {"code": "ledger_compact_failed", "message": str(e)}})
        return 2
    _print_json({"ok": True, "result": res})
    return 0


def cmd_version(_: argparse.Namespace) -> int:
    print(__version__)
    return 0


def cmd_status(_: argparse.Namespace) -> int:
    """Show overall CCCC status: daemon, groups, actors."""
    from .kernel.runtime import detect_all_runtimes
    
    home = ensure_home()
    
    # Check daemon
    daemon_resp = call_daemon({"op": "ping"})
    daemon_ok = daemon_resp.get("ok", False)
    
    # Get groups
    groups_resp = call_daemon({"op": "groups"}) if daemon_ok else {"ok": False}
    groups = groups_resp.get("result", {}).get("groups", []) if groups_resp.get("ok") else []
    
    # Get active group
    active = load_active()
    active_group_id = str(active.get("active_group_id") or "").strip()
    
    # Get runtimes
    runtimes = detect_all_runtimes(primary_only=False)
    available_runtimes = [r.name for r in runtimes if r.available]
    
    print(f"CCCC Status")
    print(f"===========")
    print(f"Version:     {__version__}")
    print(f"Home:        {home}")
    print(f"Daemon:      {'running' if daemon_ok else 'stopped'}")
    print(f"Runtimes:    {', '.join(available_runtimes) if available_runtimes else '(none detected)'}")
    print()
    
    if not groups:
        print("Groups:      (none)")
    else:
        print(f"Groups:      {len(groups)}")
        for g in groups:
            gid = str(g.get("group_id") or "")
            title = str(g.get("title") or gid)
            running = g.get("running", False)
            active_mark = " *" if gid == active_group_id else ""
            status = "running" if running else "stopped"
            print(f"  - {title} ({gid}){active_mark} [{status}]")
            
            # Get actors for this group
            if daemon_ok:
                actors_resp = call_daemon({"op": "actor_list", "args": {"group_id": gid}})
                actors = actors_resp.get("result", {}).get("actors", []) if actors_resp.get("ok") else []
                for a in actors:
                    aid = str(a.get("id") or "")
                    role = str(a.get("role") or "peer")
                    enabled = a.get("enabled", False)
                    runtime = str(a.get("runtime") or "codex")
                    runner = str(a.get("runner") or "pty")
                    status = "on" if enabled else "off"
                    print(f"      {aid} ({role}, {runtime}, {runner}) [{status}]")
    
    return 0


def cmd_use(args: argparse.Namespace) -> int:
    group_id = str(args.group_id or "").strip()
    group = load_group(group_id)
    if group is None:
        _print_json({"ok": False, "error": {"code": "group_not_found", "message": f"group not found: {group_id}"}})
        return 2
    doc = set_active_group_id(group_id)
    _print_json({"ok": True, "result": doc})
    return 0


def cmd_active(_: argparse.Namespace) -> int:
    doc = load_active()
    _print_json({"ok": True, "result": doc})
    return 0


def cmd_actor_list(args: argparse.Namespace) -> int:
    group_id = _resolve_group_id(getattr(args, "group", ""))
    if not group_id:
        _print_json({"ok": False, "error": {"code": "missing_group_id", "message": "missing group_id (no active group?)"}})
        return 2

    if _ensure_daemon_running():
        resp = call_daemon({"op": "actor_list", "args": {"group_id": group_id}})
        if resp.get("ok"):
            _print_json(resp)
            return 0

    group = load_group(group_id)
    if group is None:
        _print_json({"ok": False, "error": {"code": "group_not_found", "message": f"group not found: {group_id}"}})
        return 2
    _print_json({"ok": True, "result": {"actors": list_actors(group)}})
    return 0


def cmd_actor_add(args: argparse.Namespace) -> int:
    group_id = _resolve_group_id(getattr(args, "group", ""))
    if not group_id:
        _print_json({"ok": False, "error": {"code": "missing_group_id", "message": "missing group_id (no active group?)"}})
        return 2
    group = load_group(group_id)
    if group is None:
        _print_json({"ok": False, "error": {"code": "group_not_found", "message": f"group not found: {group_id}"}})
        return 2

    actor_id = str(args.actor_id or "").strip()
    title = str(args.title or "").strip()
    by = str(args.by or "user").strip()
    submit = str(args.submit or "enter").strip() or "enter"
    runner = str(getattr(args, "runner", "") or "pty").strip() or "pty"
    runtime = str(getattr(args, "runtime", "") or "codex").strip() or "codex"
    command: list[str] = []
    if args.command:
        try:
            command = shlex.split(str(args.command), posix=(os.name != "nt"))
        except Exception:
            command = [str(args.command)]
    
    # Auto-set command based on runtime if not provided
    if not command:
        from .kernel.runtime import get_runtime_command_with_flags
        command = get_runtime_command_with_flags(runtime)
    if runtime == "custom" and runner != "headless" and not command:
        _print_json({
            "ok": False,
            "error": {"code": "missing_command", "message": "custom runtime requires a command (PTY runner)"},
        })
        return 2
    
    env: dict[str, str] = {}
    if isinstance(args.env, list):
        for item in args.env:
            if not isinstance(item, str) or "=" not in item:
                continue
            k, v = item.split("=", 1)
            k = k.strip()
            if not k:
                continue
            env[k] = v
    default_scope_key = ""
    if args.scope:
        default_scope_key = detect_scope(Path(args.scope)).scope_key
        scopes = group.doc.get("scopes") if isinstance(group.doc.get("scopes"), list) else []
        attached = any(isinstance(s, dict) and s.get("scope_key") == default_scope_key for s in scopes)
        if not attached:
            _print_json({"ok": False, "error": {"code": "scope_not_attached", "message": f"scope not attached: {default_scope_key}"}})
            return 2

    if _ensure_daemon_running():
        resp = call_daemon(
            {
                "op": "actor_add",
                "args": {
                    "group_id": group_id,
                    "actor_id": actor_id,
                    "title": title,
                    "submit": submit,
                    "runner": runner,
                    "runtime": runtime,
                    "by": by,
                    "command": command,
                    "env": env,
                    "default_scope_key": default_scope_key,
                },
            }
        )
        if resp.get("ok"):
            _print_json(resp)
            return 0

    try:
        require_actor_permission(group, by=by, action="actor.add")
        # Note: role is auto-determined by position (first enabled = foreman)
        if runner not in ("pty", "headless"):
            raise ValueError("invalid runner (must be 'pty' or 'headless')")
        if runtime not in ("amp", "auggie", "claude", "codex", "cursor", "droid", "neovate", "gemini", "kilocode", "opencode", "copilot", "custom"):
            raise ValueError("invalid runtime")
        if runtime == "custom" and runner != "headless" and not command:
            raise ValueError("custom runtime requires a command (PTY runner)")
        actor = add_actor(
            group,
            actor_id=actor_id,
            title=title,
            command=command,
            env=env,
            default_scope_key=default_scope_key,
            submit=submit,
            runner=runner,  # type: ignore
            runtime=runtime,  # type: ignore
        )
    except Exception as e:
        _print_json({"ok": False, "error": {"code": "actor_add_failed", "message": str(e)}})
        return 2
    ev = append_event(group.ledger_path, kind="actor.add", group_id=group.group_id, scope_key="", by=by, data={"actor": actor})
    _print_json({"ok": True, "result": {"actor": actor, "event": ev}})
    return 0


def cmd_actor_remove(args: argparse.Namespace) -> int:
    group_id = _resolve_group_id(getattr(args, "group", ""))
    if not group_id:
        _print_json({"ok": False, "error": {"code": "missing_group_id", "message": "missing group_id (no active group?)"}})
        return 2
    actor_id = str(args.actor_id or "").strip()
    by = str(args.by or "user").strip()

    if _ensure_daemon_running():
        resp = call_daemon({"op": "actor_remove", "args": {"group_id": group_id, "actor_id": actor_id, "by": by}})
        if resp.get("ok"):
            _print_json(resp)
            return 0

    group = load_group(group_id)
    if group is None:
        _print_json({"ok": False, "error": {"code": "group_not_found", "message": f"group not found: {group_id}"}})
        return 2
    try:
        require_actor_permission(group, by=by, action="actor.remove", target_actor_id=actor_id)
        remove_actor(group, actor_id)
    except Exception as e:
        _print_json({"ok": False, "error": {"code": "actor_remove_failed", "message": str(e)}})
        return 2
    ev = append_event(group.ledger_path, kind="actor.remove", group_id=group.group_id, scope_key="", by=by, data={"actor_id": actor_id})
    _print_json({"ok": True, "result": {"actor_id": actor_id, "event": ev}})
    return 0


def cmd_actor_start(args: argparse.Namespace) -> int:
    group_id = _resolve_group_id(getattr(args, "group", ""))
    if not group_id:
        _print_json({"ok": False, "error": {"code": "missing_group_id", "message": "missing group_id (no active group?)"}})
        return 2
    actor_id = str(args.actor_id or "").strip()
    by = str(args.by or "user").strip()

    if _ensure_daemon_running():
        resp = call_daemon({"op": "actor_start", "args": {"group_id": group_id, "actor_id": actor_id, "by": by}})
        if resp.get("ok"):
            _print_json(resp)
            return 0

    group = load_group(group_id)
    if group is None:
        _print_json({"ok": False, "error": {"code": "group_not_found", "message": f"group not found: {group_id}"}})
        return 2
    try:
        require_actor_permission(group, by=by, action="actor.start", target_actor_id=actor_id)
        actor = update_actor(group, actor_id, {"enabled": True})
    except Exception as e:
        _print_json({"ok": False, "error": {"code": "actor_start_failed", "message": str(e)}})
        return 2
    ev = append_event(group.ledger_path, kind="actor.start", group_id=group.group_id, scope_key="", by=by, data={"actor_id": actor_id})
    _print_json({"ok": True, "result": {"actor": actor, "event": ev}})
    return 0


def cmd_actor_stop(args: argparse.Namespace) -> int:
    group_id = _resolve_group_id(getattr(args, "group", ""))
    if not group_id:
        _print_json({"ok": False, "error": {"code": "missing_group_id", "message": "missing group_id (no active group?)"}})
        return 2
    actor_id = str(args.actor_id or "").strip()
    by = str(args.by or "user").strip()

    if _ensure_daemon_running():
        resp = call_daemon({"op": "actor_stop", "args": {"group_id": group_id, "actor_id": actor_id, "by": by}})
        if resp.get("ok"):
            _print_json(resp)
            return 0

    group = load_group(group_id)
    if group is None:
        _print_json({"ok": False, "error": {"code": "group_not_found", "message": f"group not found: {group_id}"}})
        return 2
    try:
        require_actor_permission(group, by=by, action="actor.stop", target_actor_id=actor_id)
        actor = update_actor(group, actor_id, {"enabled": False})
    except Exception as e:
        _print_json({"ok": False, "error": {"code": "actor_stop_failed", "message": str(e)}})
        return 2
    ev = append_event(group.ledger_path, kind="actor.stop", group_id=group.group_id, scope_key="", by=by, data={"actor_id": actor_id})
    _print_json({"ok": True, "result": {"actor": actor, "event": ev}})
    return 0


def cmd_actor_restart(args: argparse.Namespace) -> int:
    group_id = _resolve_group_id(getattr(args, "group", ""))
    if not group_id:
        _print_json({"ok": False, "error": {"code": "missing_group_id", "message": "missing group_id (no active group?)"}})
        return 2
    actor_id = str(args.actor_id or "").strip()
    by = str(args.by or "user").strip()

    if _ensure_daemon_running():
        resp = call_daemon({"op": "actor_restart", "args": {"group_id": group_id, "actor_id": actor_id, "by": by}})
        if resp.get("ok"):
            _print_json(resp)
            return 0

    group = load_group(group_id)
    if group is None:
        _print_json({"ok": False, "error": {"code": "group_not_found", "message": f"group not found: {group_id}"}})
        return 2
    try:
        require_actor_permission(group, by=by, action="actor.restart", target_actor_id=actor_id)
        actor = update_actor(group, actor_id, {"enabled": True})
    except Exception as e:
        _print_json({"ok": False, "error": {"code": "actor_restart_failed", "message": str(e)}})
        return 2
    ev = append_event(group.ledger_path, kind="actor.restart", group_id=group.group_id, scope_key="", by=by, data={"actor_id": actor_id})
    _print_json({"ok": True, "result": {"actor": actor, "event": ev}})
    return 0


def cmd_actor_update(args: argparse.Namespace) -> int:
    group_id = _resolve_group_id(getattr(args, "group", ""))
    if not group_id:
        _print_json({"ok": False, "error": {"code": "missing_group_id", "message": "missing group_id (no active group?)"}})
        return 2

    actor_id = str(args.actor_id or "").strip()
    by = str(args.by or "user").strip()

    group = load_group(group_id)
    if group is None:
        _print_json({"ok": False, "error": {"code": "group_not_found", "message": f"group not found: {group_id}"}})
        return 2

    patch: dict[str, Any] = {}
    if args.title is not None:
        patch["title"] = str(args.title or "")
    role = getattr(args, "role", None)
    if role:
        patch["role"] = str(role)
    if args.command is not None:
        cmd: list[str] = []
        if str(args.command).strip():
            try:
                cmd = shlex.split(str(args.command), posix=(os.name != "nt"))
            except Exception:
                cmd = [str(args.command)]
        patch["command"] = cmd
    if isinstance(args.env, list) and args.env:
        env: dict[str, str] = {}
        for item in args.env:
            if not isinstance(item, str) or "=" not in item:
                continue
            k, v = item.split("=", 1)
            k = k.strip()
            if not k:
                continue
            env[k] = v
        patch["env"] = env
    if args.scope:
        scope_key = detect_scope(Path(args.scope)).scope_key
        scopes = group.doc.get("scopes") if isinstance(group.doc.get("scopes"), list) else []
        attached = any(isinstance(s, dict) and s.get("scope_key") == scope_key for s in scopes)
        if not attached:
            _print_json({"ok": False, "error": {"code": "scope_not_attached", "message": f"scope not attached: {scope_key}"}})
            return 2
        patch["default_scope_key"] = scope_key
    if args.submit is not None:
        patch["submit"] = str(args.submit)
    if getattr(args, "runner", None) is not None:
        patch["runner"] = str(args.runner)
    if getattr(args, "runtime", None) is not None:
        patch["runtime"] = str(args.runtime)
    if args.enabled is not None:
        patch["enabled"] = bool(args.enabled)

    if not patch:
        _print_json({"ok": False, "error": {"code": "empty_patch", "message": "nothing to update"}})
        return 2

    if _ensure_daemon_running():
        resp = call_daemon({"op": "actor_update", "args": {"group_id": group_id, "actor_id": actor_id, "patch": patch, "by": by}})
        _print_json(resp)
        return 0 if resp.get("ok") else 2

    try:
        require_actor_permission(group, by=by, action="actor.update", target_actor_id=actor_id)
        actor = update_actor(group, actor_id, patch)
    except Exception as e:
        _print_json({"ok": False, "error": {"code": "actor_update_failed", "message": str(e)}})
        return 2
    ev = append_event(group.ledger_path, kind="actor.update", group_id=group.group_id, scope_key="", by=by, data={"actor_id": actor_id, "patch": patch})
    _print_json({"ok": True, "result": {"actor": actor, "event": ev}})
    return 0


def cmd_actor_secrets(args: argparse.Namespace) -> int:
    """Manage per-actor runtime-only secrets env (stored under CCCC_HOME/state, not in ledger)."""
    group_id = _resolve_group_id(getattr(args, "group", ""))
    if not group_id:
        _print_json({"ok": False, "error": {"code": "missing_group_id", "message": "missing group_id (no active group?)"}})
        return 2

    actor_id = str(args.actor_id or "").strip()
    by = str(args.by or "user").strip() or "user"

    if not _ensure_daemon_running():
        _print_json({"ok": False, "error": {"code": "daemon_unavailable", "message": "daemon unavailable"}})
        return 2

    if getattr(args, "keys", False):
        resp = call_daemon({"op": "actor_env_private_keys", "args": {"group_id": group_id, "actor_id": actor_id, "by": by}})
        _print_json(resp)
        return 0 if resp.get("ok") else 2

    set_vars: dict[str, str] = {}
    for item in (args.set or []):
        if not isinstance(item, str) or "=" not in item:
            continue
        k, v = item.split("=", 1)
        k = k.strip()
        if not k:
            continue
        set_vars[k] = v

    unset_keys: list[str] = []
    for item in (args.unset or []):
        k = str(item or "").strip()
        if k:
            unset_keys.append(k)

    clear = bool(getattr(args, "clear", False))
    restart = bool(getattr(args, "restart", False))

    resp = call_daemon(
        {
            "op": "actor_env_private_update",
            "args": {
                "group_id": group_id,
                "actor_id": actor_id,
                "by": by,
                "set": set_vars,
                "unset": unset_keys,
                "clear": clear,
            },
        }
    )
    if not resp.get("ok"):
        _print_json(resp)
        return 2

    if restart:
        r = call_daemon({"op": "actor_restart", "args": {"group_id": group_id, "actor_id": actor_id, "by": by}})
        if not r.get("ok"):
            _print_json(r)
            return 2
        _print_json({"ok": True, "result": {"secrets": resp.get("result", {}), "restart": r.get("result", {})}})
        return 0

    _print_json(resp)
    return 0


def cmd_inbox(args: argparse.Namespace) -> int:
    group_id = _resolve_group_id(getattr(args, "group", ""))
    actor_id = str(args.actor_id or "").strip()
    by = str(args.by or "user").strip()
    limit = int(args.limit) if isinstance(args.limit, int) else 50
    kind_filter = str(getattr(args, "kind_filter", "all") or "all").strip()
    if kind_filter not in ("all", "chat", "notify"):
        kind_filter = "all"

    if not group_id:
        _print_json({"ok": False, "error": {"code": "missing_group_id", "message": "missing group_id (no active group?)"}})
        return 2
    if not actor_id:
        _print_json({"ok": False, "error": {"code": "missing_actor_id", "message": "missing actor_id"}})
        return 2

    if _ensure_daemon_running():
        resp = call_daemon({"op": "inbox_list", "args": {"group_id": group_id, "actor_id": actor_id, "by": by, "limit": limit, "kind_filter": kind_filter}})
        if resp.get("ok") and not args.mark_read:
            _print_json(resp)
            return 0
        if resp.get("ok") and args.mark_read:
            result = resp.get("result") if isinstance(resp.get("result"), dict) else {}
            messages = result.get("messages") if isinstance(result.get("messages"), list) else []
            if messages:
                last_id = str((messages[-1] or {}).get("id") or "").strip()
                if last_id:
                    mark = call_daemon({"op": "inbox_mark_read", "args": {"group_id": group_id, "actor_id": actor_id, "event_id": last_id, "by": by}})
                    if mark.get("ok"):
                        _print_json({"ok": True, "result": {"messages": messages, "marked": mark.get("result", {})}})
                        return 0
            _print_json(resp)
            return 0

    group = load_group(group_id)
    if group is None:
        _print_json({"ok": False, "error": {"code": "group_not_found", "message": f"group not found: {group_id}"}})
        return 2
    try:
        require_inbox_permission(group, by=by, target_actor_id=actor_id)
    except Exception as e:
        _print_json({"ok": False, "error": {"code": "permission_denied", "message": str(e)}})
        return 2

    messages = unread_messages(group, actor_id=actor_id, limit=limit, kind_filter=kind_filter)  # type: ignore
    cur_event_id, cur_ts = get_cursor(group, actor_id)
    if args.mark_read and messages:
        last = messages[-1]
        last_id = str(last.get("id") or "").strip()
        last_ts = str(last.get("ts") or "")
        if last_id:
            cursor = set_cursor(group, actor_id, event_id=last_id, ts=last_ts)
            read_ev = append_event(
                group.ledger_path,
                kind="chat.read",
                group_id=group.group_id,
                scope_key="",
                by=by,
                data={"actor_id": actor_id, "event_id": last_id},
            )
            _print_json({"ok": True, "result": {"messages": messages, "cursor": cursor, "event": read_ev}})
            return 0

    _print_json({"ok": True, "result": {"messages": messages, "cursor": {"event_id": cur_event_id, "ts": cur_ts}}})
    return 0


def cmd_read(args: argparse.Namespace) -> int:
    group_id = _resolve_group_id(getattr(args, "group", ""))
    actor_id = str(args.actor_id or "").strip()
    by = str(args.by or "user").strip()
    event_id = str(args.event_id or "").strip()

    if not group_id:
        _print_json({"ok": False, "error": {"code": "missing_group_id", "message": "missing group_id (no active group?)"}})
        return 2
    if not actor_id:
        _print_json({"ok": False, "error": {"code": "missing_actor_id", "message": "missing actor_id"}})
        return 2
    if not event_id:
        _print_json({"ok": False, "error": {"code": "missing_event_id", "message": "missing event_id"}})
        return 2

    if _ensure_daemon_running():
        resp = call_daemon({"op": "inbox_mark_read", "args": {"group_id": group_id, "actor_id": actor_id, "event_id": event_id, "by": by}})
        _print_json(resp)
        return 0 if resp.get("ok") else 2

    group = load_group(group_id)
    if group is None:
        _print_json({"ok": False, "error": {"code": "group_not_found", "message": f"group not found: {group_id}"}})
        return 2
    try:
        require_inbox_permission(group, by=by, target_actor_id=actor_id)
    except Exception as e:
        _print_json({"ok": False, "error": {"code": "permission_denied", "message": str(e)}})
        return 2
    ev = find_event(group, event_id)
    if ev is None:
        _print_json({"ok": False, "error": {"code": "event_not_found", "message": f"event not found: {event_id}"}})
        return 2
    ts = str(ev.get("ts") or "")
    cursor = set_cursor(group, actor_id, event_id=event_id, ts=ts)
    read_ev = append_event(
        group.ledger_path,
        kind="chat.read",
        group_id=group.group_id,
        scope_key="",
        by=by,
        data={"actor_id": actor_id, "event_id": event_id},
    )
    _print_json({"ok": True, "result": {"cursor": cursor, "event": read_ev}})
    return 0


def cmd_prompt(args: argparse.Namespace) -> int:
    group_id = _resolve_group_id(getattr(args, "group", ""))
    actor_id = str(args.actor_id or "").strip()
    if not group_id:
        _print_json({"ok": False, "error": {"code": "missing_group_id", "message": "missing group_id (no active group?)"}})
        return 2
    if not actor_id:
        _print_json({"ok": False, "error": {"code": "missing_actor_id", "message": "missing actor id"}})
        return 2

    group = load_group(group_id)
    if group is None:
        _print_json({"ok": False, "error": {"code": "group_not_found", "message": f"group not found: {group_id}"}})
        return 2

    actor = None
    for item in list_actors(group):
        if item.get("id") == actor_id:
            actor = item
            break
    if actor is None:
        _print_json({"ok": False, "error": {"code": "actor_not_found", "message": f"actor not found: {actor_id}"}})
        return 2
    prompt = render_system_prompt(group=group, actor=actor)

    _print_json({"ok": True, "result": {"group_id": group_id, "actor_id": actor_id, "prompt": prompt}})
    return 0


def cmd_doctor(args: argparse.Namespace) -> int:
    """Check environment and show available agent runtimes."""
    import shutil
    from .kernel.runtime import detect_all_runtimes, PRIMARY_RUNTIMES
    
    print("[DOCTOR] CCCC Environment Check")
    print()
    
    # Python version
    print(f"Python: {sys.version.split()[0]} ({sys.executable})")
    
    # CCCC version
    print(f"CCCC: {__version__}")
    
    # CCCC_HOME
    home = ensure_home()
    print(f"CCCC_HOME: {home}")
    
    # Daemon status
    resp = call_daemon({"op": "ping"})
    if resp.get("ok"):
        r = resp.get("result") if isinstance(resp.get("result"), dict) else {}
        print(f"Daemon: running (pid={r.get('pid')}, version={r.get('version')})")
    else:
        print("Daemon: not running")
    
    print()
    print("Agent Runtimes:")
    
    # Check all runtimes
    all_runtimes = args.all if hasattr(args, 'all') else False
    runtimes = detect_all_runtimes(primary_only=not all_runtimes)
    
    available_count = 0
    for rt in runtimes:
        status = "OK" if rt.available else "NOT FOUND"
        mark = "✓" if rt.available else "✗"
        path_info = f" ({rt.path})" if rt.available else ""
        print(f"  {mark} {rt.name}: {status}{path_info}")
        if rt.available:
            available_count += 1
    
    print()
    if available_count == 0:
        print("No agent runtimes detected. Install one of:")
        print("  - Claude Code: https://claude.ai/code")
        print("  - Codex CLI: https://github.com/openai/codex")
        print("  - Droid: https://github.com/anthropics/droid")
        print("  - OpenCode: https://github.com/opencode-ai/opencode")
    else:
        print(f"{available_count} runtime(s) available.")
        print()
        print("Quick start:")
        print(f"  cccc setup --runtime {runtimes[0].name if runtimes[0].available else 'claude'}")
        print("  cccc attach .")
        print("  cccc actor add my-agent --runtime <name>")
        print("  cccc")
    
    return 0


def cmd_runtime_list(args: argparse.Namespace) -> int:
    """List available agent runtimes."""
    from .kernel.runtime import detect_all_runtimes
    
    all_runtimes = args.all if hasattr(args, 'all') else False
    runtimes = detect_all_runtimes(primary_only=not all_runtimes)
    
    result = {
        "runtimes": [
            {
                "name": rt.name,
                "display_name": rt.display_name,
                "command": rt.command,
                "available": rt.available,
                "path": rt.path,
                "capabilities": rt.capabilities,
            }
            for rt in runtimes
        ],
        "available": [rt.name for rt in runtimes if rt.available],
    }
    
    _print_json({"ok": True, "result": result})
    return 0


def cmd_web(args: argparse.Namespace) -> int:
    from .ports.web.main import main as web_main

    argv: list[str] = []
    if str(args.host or "").strip():
        argv.extend(["--host", str(args.host)])
    if args.port is not None:
        argv.extend(["--port", str(int(args.port))])
    if bool(getattr(args, "exhibit", False)):
        argv.append("--exhibit")
    elif str(getattr(args, "mode", "") or "").strip():
        argv.extend(["--mode", str(getattr(args, "mode"))])
    if bool(args.reload):
        argv.append("--reload")
    if str(args.log_level or "").strip():
        argv.extend(["--log-level", str(args.log_level)])
    return int(web_main(argv))


def cmd_mcp(args: argparse.Namespace) -> int:
    from .ports.mcp.main import main as mcp_main

    return int(mcp_main())


def cmd_setup(args: argparse.Namespace) -> int:
    """Setup CCCC MCP for agent runtimes (configure MCP, print guidance)."""
    import os
    import shutil

    runtime = str(args.runtime or "").strip()
    project_path = Path(args.path or ".").resolve()

    # Supported runtimes
    # - claude/codex/droid/amp/auggie/neovate/gemini: MCP setup can be automated via their CLIs
    # - cursor/kilocode/opencode/copilot: MCP setup is manual (cccc prints config guidance)
    # - custom: user-provided runtime; MCP setup is manual (generic guidance only)
    SUPPORTED_RUNTIMES = [
        "claude",
        "codex",
        "droid",
        "amp",
        "auggie",
        "neovate",
        "gemini",
        "cursor",
        "kilocode",
        "opencode",
        "copilot",
        "custom",
    ]

    if runtime and runtime not in SUPPORTED_RUNTIMES:
        _print_json({
            "ok": False,
            "error": {
                "code": "unsupported_runtime",
                "message": f"Unsupported runtime: {runtime}. Supported: {', '.join(SUPPORTED_RUNTIMES)}",
            },
        })
        return 2

    results: dict[str, Any] = {"mcp": {}, "notes": []}

    # Find cccc executable path for MCP config
    cccc_path = shutil.which("cccc") or sys.executable
    if cccc_path == sys.executable:
        cccc_cmd = [sys.executable, "-m", "cccc.ports.mcp.main"]
    else:
        cccc_cmd = ["cccc", "mcp"]

    def _cmd_line(parts: list[str]) -> str:
        return " ".join(shlex.quote(p) for p in parts)

    # Runtime-specific setup
    runtimes_to_setup = [runtime] if runtime else SUPPORTED_RUNTIMES

    for rt in runtimes_to_setup:
        if rt == "claude":
            cmd = ["claude", "mcp", "add", "-s", "user", "cccc", "--", *cccc_cmd]
            try:
                result = subprocess.run(
                    cmd,
                    capture_output=True,
                    text=True,
                    cwd=str(project_path),
                )
                if result.returncode == 0:
                    results["mcp"]["claude"] = {"mode": "auto", "status": "added"}
                else:
                    results["mcp"]["claude"] = {"mode": "manual", "command": _cmd_line(cmd)}
                    results["notes"].append("claude: MCP CLI failed; run the command shown in result.mcp.claude.command")
            except FileNotFoundError:
                results["mcp"]["claude"] = {"mode": "manual", "command": _cmd_line(cmd)}
                results["notes"].append("claude: CLI not found; run the command shown in result.mcp.claude.command")

        elif rt == "codex":
            cmd = ["codex", "mcp", "add", "cccc", "--", *cccc_cmd]
            try:
                result = subprocess.run(
                    cmd,
                    capture_output=True,
                    text=True,
                    cwd=str(project_path),
                )
                if result.returncode == 0:
                    results["mcp"]["codex"] = {"mode": "auto", "status": "added"}
                else:
                    results["mcp"]["codex"] = {"mode": "manual", "command": _cmd_line(cmd)}
                    results["notes"].append("codex: MCP CLI failed; run the command shown in result.mcp.codex.command")
            except FileNotFoundError:
                results["mcp"]["codex"] = {"mode": "manual", "command": _cmd_line(cmd)}
                results["notes"].append("codex: CLI not found; run the command shown in result.mcp.codex.command")

        elif rt == "droid":
            cmd = ["droid", "mcp", "add", "--type", "stdio", "cccc", *cccc_cmd]
            try:
                result = subprocess.run(
                    cmd,
                    capture_output=True,
                    text=True,
                    cwd=str(project_path),
                )
                if result.returncode == 0:
                    results["mcp"]["droid"] = {"mode": "auto", "status": "added"}
                else:
                    results["mcp"]["droid"] = {"mode": "manual", "command": _cmd_line(cmd)}
                    results["notes"].append("droid: MCP CLI failed; run the command shown in result.mcp.droid.command")
            except FileNotFoundError:
                results["mcp"]["droid"] = {"mode": "manual", "command": _cmd_line(cmd)}
                results["notes"].append("droid: CLI not found; run the command shown in result.mcp.droid.command")

        elif rt == "amp":
            cmd = ["amp", "mcp", "add", "cccc", *cccc_cmd]
            try:
                result = subprocess.run(
                    cmd,
                    capture_output=True,
                    text=True,
                    cwd=str(project_path),
                )
                if result.returncode == 0:
                    results["mcp"]["amp"] = {"mode": "auto", "status": "added"}
                else:
                    results["mcp"]["amp"] = {"mode": "manual", "command": _cmd_line(cmd)}
                    results["notes"].append("amp: MCP CLI failed; run the command shown in result.mcp.amp.command")
            except FileNotFoundError:
                results["mcp"]["amp"] = {"mode": "manual", "command": _cmd_line(cmd)}
                results["notes"].append("amp: CLI not found; run the command shown in result.mcp.amp.command")

        elif rt == "auggie":
            cmd = ["auggie", "mcp", "add", "cccc", "--", *cccc_cmd]
            try:
                result = subprocess.run(
                    cmd,
                    capture_output=True,
                    text=True,
                    cwd=str(project_path),
                )
                if result.returncode == 0:
                    results["mcp"]["auggie"] = {"mode": "auto", "status": "added"}
                else:
                    results["mcp"]["auggie"] = {"mode": "manual", "command": _cmd_line(cmd)}
                    results["notes"].append("auggie: MCP CLI failed; run the command shown in result.mcp.auggie.command")
            except FileNotFoundError:
                results["mcp"]["auggie"] = {"mode": "manual", "command": _cmd_line(cmd)}
                results["notes"].append("auggie: CLI not found; run the command shown in result.mcp.auggie.command")

        elif rt == "neovate":
            cmd = ["neovate", "mcp", "add", "-g", "cccc", *cccc_cmd]
            try:
                result = subprocess.run(
                    cmd,
                    capture_output=True,
                    text=True,
                    cwd=str(project_path),
                )
                if result.returncode == 0:
                    results["mcp"]["neovate"] = {"mode": "auto", "status": "added"}
                else:
                    results["mcp"]["neovate"] = {"mode": "manual", "command": _cmd_line(cmd)}
                    results["notes"].append("neovate: MCP CLI failed; run the command shown in result.mcp.neovate.command")
            except FileNotFoundError:
                results["mcp"]["neovate"] = {"mode": "manual", "command": _cmd_line(cmd)}
                results["notes"].append("neovate: CLI not found; run the command shown in result.mcp.neovate.command")

        elif rt == "gemini":
            cmd = ["gemini", "mcp", "add", "-s", "user", "cccc", *cccc_cmd]
            try:
                result = subprocess.run(
                    cmd,
                    capture_output=True,
                    text=True,
                    cwd=str(project_path),
                )
                if result.returncode == 0:
                    results["mcp"]["gemini"] = {"mode": "auto", "status": "added"}
                else:
                    results["mcp"]["gemini"] = {"mode": "manual", "command": _cmd_line(cmd)}
                    results["notes"].append("gemini: MCP CLI failed; run the command shown in result.mcp.gemini.command")
            except FileNotFoundError:
                results["mcp"]["gemini"] = {"mode": "manual", "command": _cmd_line(cmd)}
                results["notes"].append("gemini: CLI not found; run the command shown in result.mcp.gemini.command")

        elif rt == "cursor":
            cursor_config_path = Path.home() / ".cursor" / "mcp.json"
            mcp_config = {
                "mcpServers": {
                    "cccc": {
                        "command": cccc_cmd[0],
                        "args": cccc_cmd[1:] if len(cccc_cmd) > 1 else [],
                    }
                }
            }
            results["mcp"]["cursor"] = {
                "mode": "manual",
                "file": str(cursor_config_path),
                "snippet": mcp_config,
                "hint": "Create ~/.cursor/mcp.json (or .cursor/mcp.json in the project) and add mcpServers.cccc with the provided snippet.",
            }
            results["notes"].append(
                "cursor: MCP config is manual. Create ~/.cursor/mcp.json (or .cursor/mcp.json in the project) "
                "and add `mcpServers.cccc` with the provided snippet."
            )

        elif rt == "kilocode":
            kilocode_config_path = project_path / ".kilocode" / "mcp.json"
            mcp_config = {
                "mcpServers": {
                    "cccc": {
                        "command": cccc_cmd[0],
                        "args": cccc_cmd[1:] if len(cccc_cmd) > 1 else [],
                    }
                }
            }
            results["mcp"]["kilocode"] = {
                "mode": "manual",
                "file": str(kilocode_config_path),
                "snippet": mcp_config,
                "hint": "Create <project>/.kilocode/mcp.json and add mcpServers.cccc with the provided snippet.",
            }
            results["notes"].append(
                "kilocode: MCP config is manual. Create <project>/.kilocode/mcp.json and add `mcpServers.cccc` "
                "with the provided snippet."
            )

        elif rt == "opencode":
            # OpenCode: MCP config is manual.
            xdg_config_home = Path(os.environ.get("XDG_CONFIG_HOME") or (Path.home() / ".config"))
            opencode_config_path = xdg_config_home / "opencode" / "opencode.json"
            mcp_config = {
                "mcp": {
                    "cccc": {
                        "type": "local",
                        "command": [cccc_cmd[0], *cccc_cmd[1:]] if len(cccc_cmd) > 1 else [cccc_cmd[0]],
                        "environment": {},
                    }
                }
            }

            results["mcp"]["opencode"] = {
                "mode": "manual",
                "file": str(opencode_config_path),
                "snippet": mcp_config,
            }
            results["notes"].append(
                f"opencode: MCP is manual. Add `mcp.cccc` to {opencode_config_path} with the provided snippet."
            )

        elif rt == "copilot":
            copilot_config_path = Path.home() / ".copilot" / "mcp-config.json"
            mcp_config = {
                "mcpServers": {
                    "cccc": {
                        "command": cccc_cmd[0],
                        "args": cccc_cmd[1:] if len(cccc_cmd) > 1 else [],
                        "tools": ["*"],
                    }
                }
            }
            results["mcp"]["copilot"] = {
                "mode": "manual",
                "file": str(copilot_config_path),
                "snippet": mcp_config,
                "hint": f"Add mcpServers.cccc to {copilot_config_path} (or run: copilot --additional-mcp-config @<file>)",
            }
            results["notes"].append(
                f"copilot: MCP is manual. Add `mcpServers.cccc` to {copilot_config_path} "
                f"(or run Copilot with `--additional-mcp-config @<file>`)."
            )

        elif rt == "custom":
            results["mcp"]["custom"] = {
                "mode": "manual",
                "hint": f"Add an MCP stdio server named 'cccc' that runs: {_cmd_line(cccc_cmd)}",
            }
            results["notes"].append(
                "custom: MCP setup depends on your runtime. Add an MCP stdio server named 'cccc' that runs the command in result.mcp.custom.hint."
            )

    # Clean up empty notes
    if not results["notes"]:
        del results["notes"]

    _print_json({"ok": True, "result": results})
    return 0


def cmd_daemon(args: argparse.Namespace) -> int:
    if args.action == "status":
        resp = call_daemon({"op": "ping"})
        if resp.get("ok"):
            r = resp.get("result") if isinstance(resp.get("result"), dict) else {}
            print(f"ccccd: running pid={r.get('pid')} version={r.get('version')}")
            return 0
        print("ccccd: not running")
        return 1

    if args.action == "start":
        if _ensure_daemon_running():
            print("ccccd: running")
            return 0
        print("ccccd: failed to start")
        return 1

    if args.action == "stop":
        resp = call_daemon({"op": "shutdown"})
        if resp.get("ok"):
            print("ccccd: shutdown requested")
            return 0
        print("ccccd: not running")
        return 0

    return 2


# =============================================================================
# IM Bridge Commands
# =============================================================================


def cmd_im_set(args: argparse.Namespace) -> int:
    """Set IM bridge configuration for a group."""
    group_id = _resolve_group_id(getattr(args, "group", ""))
    if not group_id:
        _print_json({"ok": False, "error": {"code": "missing_group_id", "message": "missing group_id (no active group?)"}})
        return 2

    platform = str(args.platform or "").strip().lower()
    if platform not in ("telegram", "slack", "discord", "feishu", "dingtalk"):
        _print_json({"ok": False, "error": {"code": "invalid_platform", "message": "platform must be telegram, slack, discord, feishu, or dingtalk"}})
        return 2

    # Get token fields
    bot_token_env = str(getattr(args, "bot_token_env", "") or "").strip()
    app_token_env = str(getattr(args, "app_token_env", "") or "").strip()
    token_env = str(args.token_env or "").strip()
    token = str(args.token or "").strip()
    # Feishu/DingTalk specific (app credentials)
    app_key_env = str(getattr(args, "app_key_env", "") or "").strip()
    app_secret_env = str(getattr(args, "app_secret_env", "") or "").strip()
    feishu_domain = str(getattr(args, "domain", "") or "").strip()
    dingtalk_robot_code_env = str(getattr(args, "robot_code_env", "") or "").strip()
    dingtalk_robot_code = str(getattr(args, "robot_code", "") or "").strip()

    # Backward compat: if only token_env provided, use as bot_token_env
    if token_env and not bot_token_env:
        bot_token_env = token_env

    # Interactive mode if required fields are missing
    if platform in ("feishu", "dingtalk"):
        # Feishu/DingTalk use app credentials (env var names by default).
        if not app_key_env or not app_secret_env:
            try:
                platform_name = "Feishu/Lark" if platform == "feishu" else "DingTalk"
                default_key = "FEISHU_APP_ID" if platform == "feishu" else "DINGTALK_APP_KEY"
                default_secret = "FEISHU_APP_SECRET" if platform == "feishu" else "DINGTALK_APP_SECRET"
                print(f"{platform_name} requires app credentials:")
                if not app_key_env:
                    print(f"Enter App Key/ID env var name (default: {default_key}):")
                    key_input = input("> ").strip()
                    app_key_env = key_input or default_key
                if not app_secret_env:
                    print(f"Enter App Secret env var name (default: {default_secret}):")
                    secret_input = input("> ").strip()
                    app_secret_env = secret_input or default_secret
            except (EOFError, KeyboardInterrupt):
                print()
                return 1
    elif not bot_token_env and not token:
        try:
            if platform == "slack":
                print(f"Slack requires two tokens:")
                print(f"  1. Bot Token (xoxb-) for outbound messages")
                print(f"  2. App Token (xapp-) for inbound messages (Socket Mode)")
                print()
                print("Enter Bot Token env var name (e.g., SLACK_BOT_TOKEN):")
                bot_input = input("> ").strip()
                if not bot_input:
                    _print_json({"ok": False, "error": {"code": "no_token", "message": "no bot token provided"}})
                    return 2
                bot_token_env = bot_input
                print("Enter App Token env var name (e.g., SLACK_APP_TOKEN):")
                app_input = input("> ").strip()
                if app_input:
                    app_token_env = app_input
            else:
                print(f"Enter token or environment variable name for {platform}:")
                user_input = input("> ").strip()
                if not user_input:
                    _print_json({"ok": False, "error": {"code": "no_token", "message": "no token provided"}})
                    return 2
                # Heuristic: if it looks like an env var name (all caps, underscores), treat as token_env
                if user_input.isupper() or "_" in user_input and not ":" in user_input:
                    bot_token_env = user_input
                else:
                    token = user_input
        except (EOFError, KeyboardInterrupt):
            print()
            return 1

    group = load_group(group_id)
    if group is None:
        _print_json({"ok": False, "error": {"code": "group_not_found", "message": f"group not found: {group_id}"}})
        return 2

    # Update group.yaml with IM config
    im_config: dict[str, Any] = {"platform": platform}
    # Default file transfer settings (can be customized in group.yaml).
    default_max_mb = 20 if platform in ("telegram", "slack") else 10
    im_config["files"] = {"enabled": True, "max_mb": default_max_mb}
    
    if platform == "slack":
        # Slack uses dual tokens
        if bot_token_env:
            im_config["bot_token_env"] = bot_token_env
        if app_token_env:
            im_config["app_token_env"] = app_token_env
    elif platform == "feishu":
        # Feishu: app_id/app_secret (stored as env var names; the bridge reads FEISHU_APP_ID/FEISHU_APP_SECRET).
        if feishu_domain:
            v = feishu_domain.strip().lower()
            if v in (
                "lark",
                "global",
                "intl",
                "international",
                "open.larkoffice.com",
                "https://open.larkoffice.com",
                # Historical alias used in some SDKs/docs.
                "open.larksuite.com",
                "https://open.larksuite.com",
            ):
                im_config["feishu_domain"] = "https://open.larkoffice.com"
            else:
                im_config["feishu_domain"] = "https://open.feishu.cn"
        if app_key_env:
            im_config["feishu_app_id_env"] = app_key_env
        if app_secret_env:
            im_config["feishu_app_secret_env"] = app_secret_env
    elif platform == "dingtalk":
        # DingTalk: app_key/app_secret (+ optional robot_code).
        if app_key_env:
            im_config["dingtalk_app_key_env"] = app_key_env
        if app_secret_env:
            im_config["dingtalk_app_secret_env"] = app_secret_env
        if dingtalk_robot_code_env:
            im_config["dingtalk_robot_code_env"] = dingtalk_robot_code_env
        if dingtalk_robot_code:
            im_config["dingtalk_robot_code"] = dingtalk_robot_code
    else:
        # Telegram/Discord use single token
        if bot_token_env:
            im_config["token_env"] = bot_token_env

    if token and platform not in ("feishu", "dingtalk"):
        im_config["token"] = token

    # Update group doc and save
    group.doc["im"] = im_config
    group.save()

    _print_json({"ok": True, "result": {"group_id": group_id, "im": im_config}})
    return 0


def cmd_im_unset(args: argparse.Namespace) -> int:
    """Remove IM bridge configuration from a group."""
    group_id = _resolve_group_id(getattr(args, "group", ""))
    if not group_id:
        _print_json({"ok": False, "error": {"code": "missing_group_id", "message": "missing group_id (no active group?)"}})
        return 2

    group = load_group(group_id)
    if group is None:
        _print_json({"ok": False, "error": {"code": "group_not_found", "message": f"group not found: {group_id}"}})
        return 2

    if "im" in group.doc:
        del group.doc["im"]
        group.save()

    _print_json({"ok": True, "result": {"group_id": group_id, "im": None}})
    return 0


def cmd_im_config(args: argparse.Namespace) -> int:
    """Show IM bridge configuration for a group."""
    group_id = _resolve_group_id(getattr(args, "group", ""))
    if not group_id:
        _print_json({"ok": False, "error": {"code": "missing_group_id", "message": "missing group_id (no active group?)"}})
        return 2

    group = load_group(group_id)
    if group is None:
        _print_json({"ok": False, "error": {"code": "group_not_found", "message": f"group not found: {group_id}"}})
        return 2

    im_config = group.doc.get("im")
    _print_json({"ok": True, "result": {"group_id": group_id, "im": im_config}})
    return 0


def _im_find_bridge_pid(group: Any) -> Optional[int]:
    """Find running bridge PID for a group."""
    pid_path = group.path / "state" / "im_bridge.pid"
    if not pid_path.exists():
        return None
    try:
        pid = int(pid_path.read_text(encoding="utf-8").strip())
        # Check if process is alive
        os.kill(pid, 0)
        return pid
    except (ValueError, ProcessLookupError, PermissionError):
        return None


def _im_find_bridge_pids_by_script(group_id: str) -> list[int]:
    """Find all bridge processes for a group by scanning /proc."""
    pids: list[int] = []
    proc = Path("/proc")
    try:
        for d in proc.iterdir():
            if not d.is_dir() or not d.name.isdigit():
                continue
            pid = int(d.name)
            try:
                cmdline = (d / "cmdline").read_bytes().decode("utf-8", "ignore")
                # Look for our bridge module with this group_id.
                # We support both historical entrypoints:
                # - python -m cccc.ports.im.bridge <group_id> ...
                # - python -m cccc.ports.im <group_id> ...
                if (
                    ("cccc.ports.im.bridge" in cmdline or "cccc.ports.im" in cmdline)
                    and group_id in cmdline
                ):
                    pids.append(pid)
            except Exception:
                continue
    except Exception:
        pass
    return pids


def _im_group_dir(group_id: str) -> Path:
    return ensure_home() / "groups" / group_id


def cmd_im_start(args: argparse.Namespace) -> int:
    """Start IM bridge for a group."""
    group_id = _resolve_group_id(getattr(args, "group", ""))
    if not group_id:
        _print_json({"ok": False, "error": {"code": "missing_group_id", "message": "missing group_id (no active group?)"}})
        return 2

    group = load_group(group_id)
    if group is None:
        _print_json({"ok": False, "error": {"code": "group_not_found", "message": f"group not found: {group_id}"}})
        return 2

    # Check if already running
    existing_pid = _im_find_bridge_pid(group)
    if existing_pid:
        _print_json({"ok": False, "error": {"code": "already_running", "message": f"bridge already running (pid={existing_pid})"}})
        return 2
    orphan_pids = _im_find_bridge_pids_by_script(group_id)
    if orphan_pids:
        _print_json({"ok": False, "error": {"code": "already_running", "message": f"bridge already running (pid={orphan_pids[0]})"}})
        return 2

    # Check IM config
    im_config = group.doc.get("im", {})
    if not im_config:
        _print_json({"ok": False, "error": {"code": "no_im_config", "message": "no IM configuration. Run: cccc im set <platform>"}})
        return 2

    # Persist desired run-state for restart/autostart.
    if isinstance(im_config, dict):
        im_config["enabled"] = True
        group.doc["im"] = im_config
        try:
            group.save()
        except Exception:
            pass

    platform = im_config.get("platform", "telegram")

    # Prepare environment
    env = os.environ.copy()
    token_env = im_config.get("token_env")
    token = im_config.get("token")
    if token and token_env:
        env[token_env] = token
    elif token:
        # Set default env var based on platform
        default_env = {"telegram": "TELEGRAM_BOT_TOKEN", "slack": "SLACK_BOT_TOKEN", "discord": "DISCORD_BOT_TOKEN"}
        env[default_env.get(platform, "BOT_TOKEN")] = token

    # Feishu/DingTalk: set credentials from config
    # Supports both direct values and env var names (for Web UI compatibility)
    if platform == "feishu":
        # Direct values
        app_id = im_config.get("feishu_app_id", "")
        app_secret = im_config.get("feishu_app_secret", "")
        # Env var names
        app_id_env = im_config.get("feishu_app_id_env", "")
        app_secret_env = im_config.get("feishu_app_secret_env", "")
        # Set env vars (direct value takes precedence)
        if app_id:
            env["FEISHU_APP_ID"] = app_id
        elif app_id_env and app_id_env in os.environ:
            env["FEISHU_APP_ID"] = os.environ[app_id_env]
        if app_secret:
            env["FEISHU_APP_SECRET"] = app_secret
        elif app_secret_env and app_secret_env in os.environ:
            env["FEISHU_APP_SECRET"] = os.environ[app_secret_env]
    elif platform == "dingtalk":
        # Direct values
        app_key = im_config.get("dingtalk_app_key", "")
        app_secret = im_config.get("dingtalk_app_secret", "")
        robot_code = im_config.get("dingtalk_robot_code", "")
        # Env var names
        app_key_env = im_config.get("dingtalk_app_key_env", "")
        app_secret_env = im_config.get("dingtalk_app_secret_env", "")
        robot_code_env = im_config.get("dingtalk_robot_code_env", "")
        # Set env vars (direct value takes precedence)
        if app_key:
            env["DINGTALK_APP_KEY"] = app_key
        elif app_key_env and app_key_env in os.environ:
            env["DINGTALK_APP_KEY"] = os.environ[app_key_env]
        if app_secret:
            env["DINGTALK_APP_SECRET"] = app_secret
        elif app_secret_env and app_secret_env in os.environ:
            env["DINGTALK_APP_SECRET"] = os.environ[app_secret_env]
        if robot_code:
            env["DINGTALK_ROBOT_CODE"] = robot_code
        elif robot_code_env and robot_code_env in os.environ:
            env["DINGTALK_ROBOT_CODE"] = os.environ[robot_code_env]

    # Start bridge as subprocess
    state_dir = group.path / "state"
    state_dir.mkdir(parents=True, exist_ok=True)
    log_path = state_dir / "im_bridge.log"

    log_file = None
    try:
        log_file = log_path.open("a", encoding="utf-8")
        proc = subprocess.Popen(
            [sys.executable, "-m", "cccc.ports.im", group_id, platform],
            env=env,
            stdout=log_file,
            stderr=log_file,
            start_new_session=True,
        )
        # Give the bridge a moment to acquire locks / validate tokens.
        time.sleep(0.25)
        rc = proc.poll()
        try:
            log_file.close()
        except Exception:
            pass

        if rc is not None:
            _print_json({
                "ok": False,
                "error": {
                    "code": "start_failed",
                    "message": f"bridge exited immediately (code={rc}). See log: {log_path}",
                },
            })
            return 2

        # Write PID file only after we know it stayed up.
        pid_path = state_dir / "im_bridge.pid"
        pid_path.write_text(str(proc.pid), encoding="utf-8")

        _print_json({"ok": True, "result": {"group_id": group_id, "platform": platform, "pid": proc.pid, "log": str(log_path)}})
        return 0
    except Exception as e:
        try:
            if log_file:
                log_file.close()
        except Exception:
            pass
        _print_json({"ok": False, "error": {"code": "start_failed", "message": str(e)}})
        return 2


def cmd_im_stop(args: argparse.Namespace) -> int:
    """Stop IM bridge for a group."""
    import signal as sig

    group_id = _resolve_group_id(getattr(args, "group", ""))
    if not group_id:
        _print_json({"ok": False, "error": {"code": "missing_group_id", "message": "missing group_id (no active group?)"}})
        return 2

    # Persist desired run-state for restart/autostart (best-effort).
    try:
        group = load_group(group_id)
        if group is not None:
            im_cfg = group.doc.get("im")
            if isinstance(im_cfg, dict):
                im_cfg["enabled"] = False
                group.doc["im"] = im_cfg
                group.save()
    except Exception:
        pass

    stopped = 0
    group_dir = _im_group_dir(group_id)
    pid_path = group_dir / "state" / "im_bridge.pid"
    killed: set[int] = set()

    # Stop by PID file
    if pid_path.exists():
        try:
            pid = int(pid_path.read_text(encoding="utf-8").strip())
            if pid not in killed:
                try:
                    os.killpg(os.getpgid(pid), sig.SIGTERM)
                except Exception:
                    try:
                        os.kill(pid, sig.SIGTERM)
                    except Exception:
                        pass
                killed.add(pid)
                stopped += 1
        except Exception:
            pass
        try:
            pid_path.unlink(missing_ok=True)
        except Exception:
            pass

    # Also scan for any orphan processes
    orphan_pids = _im_find_bridge_pids_by_script(group_id)
    for pid in orphan_pids:
        if pid in killed:
            continue
        try:
            os.killpg(os.getpgid(pid), sig.SIGTERM)
        except Exception:
            try:
                os.kill(pid, sig.SIGTERM)
            except Exception:
                pass
        killed.add(pid)
        stopped += 1

    _print_json({"ok": True, "result": {"group_id": group_id, "stopped": stopped}})
    return 0


def cmd_im_status(args: argparse.Namespace) -> int:
    """Show IM bridge status for a group."""
    group_id = _resolve_group_id(getattr(args, "group", ""))
    if not group_id:
        _print_json({"ok": False, "error": {"code": "missing_group_id", "message": "missing group_id (no active group?)"}})
        return 2
    group = load_group(group_id)
    group_exists = group is not None

    im_config = group.doc.get("im", {}) if group_exists else {}
    platform = im_config.get("platform") if im_config else None

    # Check if running
    pid = _im_find_bridge_pid(group) if group_exists else None
    if pid is None:
        orphan_pids = _im_find_bridge_pids_by_script(group_id)
        if orphan_pids:
            pid = orphan_pids[0]
    running = pid is not None

    # Get subscriber count
    subscribers_path = _im_group_dir(group_id) / "state" / "im_subscribers.json"
    subscriber_count = 0
    if subscribers_path.exists():
        try:
            subs = json.loads(subscribers_path.read_text(encoding="utf-8"))
            subscriber_count = sum(1 for s in subs.values() if isinstance(s, dict) and s.get("subscribed"))
        except Exception:
            pass

    result = {
        "group_id": group_id,
        "group_exists": group_exists,
        "configured": bool(im_config),
        "platform": platform,
        "running": running,
        "pid": pid,
        "subscribers": subscriber_count,
    }

    _print_json({"ok": True, "result": result})
    return 0


def cmd_im_bind(args: argparse.Namespace) -> int:
    """Bind a pending authorization key to authorize a Telegram chat."""
    group_id = _resolve_group_id(getattr(args, "group", ""))
    if not group_id:
        _print_json({"ok": False, "error": {"code": "missing_group_id", "message": "missing group_id (no active group?)"}})
        return 2

    key = str(getattr(args, "key", "") or "").strip()
    if not key:
        _print_json({"ok": False, "error": {"code": "missing_key", "message": "missing --key argument"}})
        return 2

    if not _ensure_daemon_running():
        _print_json({"ok": False, "error": {"code": "daemon_error", "message": "cannot reach daemon"}})
        return 1

    resp = call_daemon({"op": "im_bind_chat", "args": {"group_id": group_id, "key": key}})
    _print_json(resp)
    return 0 if resp.get("ok") else 1


def cmd_im_authorized(args: argparse.Namespace) -> int:
    """List authorized chats for a group."""
    group_id = _resolve_group_id(getattr(args, "group", ""))
    if not group_id:
        _print_json({"ok": False, "error": {"code": "missing_group_id", "message": "missing group_id (no active group?)"}})
        return 2

    if not _ensure_daemon_running():
        _print_json({"ok": False, "error": {"code": "daemon_error", "message": "cannot reach daemon"}})
        return 1

    resp = call_daemon({"op": "im_list_authorized", "args": {"group_id": group_id}})
    _print_json(resp)
    return 0 if resp.get("ok") else 1


def cmd_im_revoke(args: argparse.Namespace) -> int:
    """Revoke authorization for a chat."""
    group_id = _resolve_group_id(getattr(args, "group", ""))
    if not group_id:
        _print_json({"ok": False, "error": {"code": "missing_group_id", "message": "missing group_id (no active group?)"}})
        return 2

    chat_id = str(getattr(args, "chat_id", "") or "").strip()
    if not chat_id:
        _print_json({"ok": False, "error": {"code": "missing_chat_id", "message": "missing --chat-id argument"}})
        return 2

    try:
        thread_id = int(getattr(args, "thread_id", 0) or 0)
    except Exception:
        thread_id = 0

    if not _ensure_daemon_running():
        _print_json({"ok": False, "error": {"code": "daemon_error", "message": "cannot reach daemon"}})
        return 1

    resp = call_daemon({"op": "im_revoke_chat", "args": {"group_id": group_id, "chat_id": chat_id, "thread_id": thread_id}})
    _print_json(resp)
    return 0 if resp.get("ok") else 1


def cmd_im_logs(args: argparse.Namespace) -> int:
    """Show IM bridge logs for a group."""
    from collections import deque

    group_id = _resolve_group_id(getattr(args, "group", ""))
    if not group_id:
        _print_json({"ok": False, "error": {"code": "missing_group_id", "message": "missing group_id (no active group?)"}})
        return 2

    log_path = _im_group_dir(group_id) / "state" / "im_bridge.log"
    if not log_path.exists():
        print(f"[IM] Log file not found: {log_path}")
        return 1

    lines = int(args.lines) if hasattr(args, "lines") and args.lines else 50
    follow = bool(args.follow) if hasattr(args, "follow") else False

    try:
        if follow:
            print(f"[IM] Tailing {log_path} (Ctrl-C to stop)...")
            with open(log_path, "r", encoding="utf-8") as f:
                # Show last N lines first
                dq = deque(f, maxlen=lines)
                for ln in dq:
                    print(ln.rstrip())
                # Then follow
                while True:
                    ln = f.readline()
                    if not ln:
                        time.sleep(0.5)
                        continue
                    print(ln.rstrip())
        else:
            # Print last N lines
            with open(log_path, "r", encoding="utf-8") as f:
                dq = deque(f, maxlen=lines)
                for ln in dq:
                    print(ln.rstrip())
    except KeyboardInterrupt:
        print()

    return 0


def build_parser() -> argparse.ArgumentParser:
    p = argparse.ArgumentParser(prog="cccc", description="CCCC vNext (working group + scopes)")
    sub = p.add_subparsers(dest="cmd", required=True)

    p_attach = sub.add_parser("attach", help="Attach current path to a working group (auto-create if needed)")
    p_attach.add_argument("path", nargs="?", default=".", help="Path inside a repo/scope (default: .)")
    p_attach.add_argument("--group", dest="group_id", default="", help="Attach scope to an existing group_id (optional)")
    p_attach.set_defaults(func=cmd_attach)

    p_group = sub.add_parser("group", help="Working group operations")
    group_sub = p_group.add_subparsers(dest="action", required=True)

    p_group_create = group_sub.add_parser("create", help="Create an empty working group")
    p_group_create.add_argument("--title", default="working-group", help="Group title (default: working-group)")
    p_group_create.add_argument("--topic", default="", help="Group topic (optional)")
    p_group_create.set_defaults(func=cmd_group_create)

    p_group_show = group_sub.add_parser("show", help="Show group metadata")
    p_group_show.add_argument("group_id", help="Target group_id")
    p_group_show.set_defaults(func=cmd_group_show)

    p_group_update = group_sub.add_parser("update", help="Update group metadata (title/topic)")
    p_group_update.add_argument("--group", default="", help="Target group_id (default: active group)")
    p_group_update.add_argument("--title", default=None, help="New title")
    p_group_update.add_argument("--topic", default=None, help="New topic (use empty string to clear)")
    p_group_update.add_argument("--by", default="user", help="Requester (default: user)")
    p_group_update.set_defaults(func=cmd_group_update)

    p_group_detach = group_sub.add_parser("detach-scope", help="Detach a workspace scope from a group")
    p_group_detach.add_argument("scope_key", help="Scope key to detach (see: cccc group show <id>)")
    p_group_detach.add_argument("--group", default="", help="Target group_id (default: active group)")
    p_group_detach.add_argument("--by", default="user", help="Requester (default: user)")
    p_group_detach.set_defaults(func=cmd_group_detach_scope)

    p_group_delete = group_sub.add_parser("delete", help="Delete a group and its local state (destructive)")
    p_group_delete.add_argument("--group", default="", help="Target group_id (default: active group)")
    p_group_delete.add_argument("--confirm", default="", help="Type the group_id to confirm deletion")
    p_group_delete.add_argument("--by", default="user", help="Requester (default: user)")
    p_group_delete.set_defaults(func=cmd_group_delete)

    p_group_use = group_sub.add_parser("use", help="Set group's active scope (must already be attached)")
    p_group_use.add_argument("group_id", help="Target group_id")
    p_group_use.add_argument("path", nargs="?", default=".", help="Path inside target scope (default: .)")
    p_group_use.set_defaults(func=cmd_group_use)

    p_group_start = group_sub.add_parser("start", help="Start a working group (spawn enabled actors)")
    p_group_start.add_argument("--group", default="", help="Target group_id (default: active group)")
    p_group_start.add_argument("--by", default="user", help="Requester (default: user)")
    p_group_start.set_defaults(func=cmd_group_start)

    p_group_stop = group_sub.add_parser("stop", help="Stop a working group (stop all running actors)")
    p_group_stop.add_argument("--group", default="", help="Target group_id (default: active group)")
    p_group_stop.add_argument("--by", default="user", help="Requester (default: user)")
    p_group_stop.set_defaults(func=cmd_group_stop)

    p_group_set_state = group_sub.add_parser("set-state", help="Set group state (active/idle/paused/stopped)")
    p_group_set_state.add_argument("state", choices=["active", "idle", "paused", "stopped"], help="New state")
    p_group_set_state.add_argument("--group", default="", help="Target group_id (default: active group)")
    p_group_set_state.add_argument("--by", default="user", help="Requester (default: user)")
    p_group_set_state.set_defaults(func=cmd_group_set_state)

    p_groups = sub.add_parser("groups", help="List known working groups")
    p_groups.set_defaults(func=cmd_groups)

    p_use = sub.add_parser("use", help="Set the active working group (for send/tail defaults)")
    p_use.add_argument("group_id", help="Target group_id")
    p_use.set_defaults(func=cmd_use)

    p_active = sub.add_parser("active", help="Show the active working group")
    p_active.set_defaults(func=cmd_active)

    p_actor = sub.add_parser("actor", help="Manage long-session actors in a working group")
    actor_sub = p_actor.add_subparsers(dest="action", required=True)

    p_actor_list = actor_sub.add_parser("list", help="List actors (default: active group)")
    p_actor_list.add_argument("--group", default="", help="Target group_id (default: active group)")
    p_actor_list.set_defaults(func=cmd_actor_list)

    p_actor_add = actor_sub.add_parser("add", help="Add an actor (first actor = foreman, rest = peer)")
    p_actor_add.add_argument("actor_id", help="Actor id (e.g. peer-a, peer-b)")
    p_actor_add.add_argument("--title", default="", help="Display title (optional)")
    p_actor_add.add_argument(
        "--runtime",
        choices=["claude", "codex", "droid", "amp", "auggie", "neovate", "gemini", "cursor", "kilocode", "opencode", "copilot", "custom"],
        default="codex",
        help="Agent runtime (auto-sets command if not provided)",
    )
    p_actor_add.add_argument("--command", default="", help="Command to run (shell-like string; optional, auto-set by --runtime)")
    p_actor_add.add_argument("--env", action="append", default=[], help="Environment var (KEY=VAL), repeatable")
    p_actor_add.add_argument("--scope", default="", help="Default scope path for this actor (optional; must be attached)")
    p_actor_add.add_argument("--submit", choices=["enter", "newline", "none"], default="enter", help="Submit key (default: enter)")
    p_actor_add.add_argument("--runner", choices=["pty", "headless"], default=_default_runner_kind(), help="Runner type: pty (interactive) or headless (MCP-driven)")
    p_actor_add.add_argument("--by", default="user", help="Requester (default: user)")
    p_actor_add.add_argument("--group", default="", help="Target group_id (default: active group)")
    p_actor_add.set_defaults(func=cmd_actor_add)

    p_actor_rm = actor_sub.add_parser("remove", help="Remove an actor")
    p_actor_rm.add_argument("actor_id", help="Actor id")
    p_actor_rm.add_argument("--by", default="user", help="Requester (default: user)")
    p_actor_rm.add_argument("--group", default="", help="Target group_id (default: active group)")
    p_actor_rm.set_defaults(func=cmd_actor_remove)

    p_actor_start = actor_sub.add_parser("start", help="Set actor enabled=true (desired run-state)")
    p_actor_start.add_argument("actor_id", help="Actor id")
    p_actor_start.add_argument("--by", default="user", help="Requester (default: user)")
    p_actor_start.add_argument("--group", default="", help="Target group_id (default: active group)")
    p_actor_start.set_defaults(func=cmd_actor_start)

    p_actor_stop = actor_sub.add_parser("stop", help="Set actor enabled=false (desired run-state)")
    p_actor_stop.add_argument("actor_id", help="Actor id")
    p_actor_stop.add_argument("--by", default="user", help="Requester (default: user)")
    p_actor_stop.add_argument("--group", default="", help="Target group_id (default: active group)")
    p_actor_stop.set_defaults(func=cmd_actor_stop)

    p_actor_restart = actor_sub.add_parser("restart", help="Record restart intent and keep enabled=true")
    p_actor_restart.add_argument("actor_id", help="Actor id")
    p_actor_restart.add_argument("--by", default="user", help="Requester (default: user)")
    p_actor_restart.add_argument("--group", default="", help="Target group_id (default: active group)")
    p_actor_restart.set_defaults(func=cmd_actor_restart)

    p_actor_update = actor_sub.add_parser("update", help="Update an actor (title/command/env/scope/enabled/runner/runtime)")
    p_actor_update.add_argument("actor_id", help="Actor id")
    p_actor_update.add_argument("--title", default=None, help="New title")
    p_actor_update.add_argument("--runtime", choices=["claude", "codex", "droid", "amp", "auggie", "neovate", "gemini", "cursor", "kilocode", "opencode", "copilot", "custom"], default=None, help="New runtime")
    p_actor_update.add_argument("--command", default=None, help="Replace command (shell-like string); use empty to clear")
    p_actor_update.add_argument("--env", action="append", default=[], help="Replace env with these KEY=VAL entries (repeatable)")
    p_actor_update.add_argument("--scope", default="", help="Set default scope path (must be attached)")
    p_actor_update.add_argument("--submit", choices=["enter", "newline", "none"], default=None, help="Submit key")
    p_actor_update.add_argument("--runner", choices=["pty", "headless"], default=None, help="Runner type: pty (interactive) or headless (MCP-driven)")
    p_actor_update.add_argument("--enabled", type=int, choices=[0, 1], default=None, help="Set enabled (1) or disabled (0)")
    p_actor_update.add_argument("--by", default="user", help="Requester (default: user)")
    p_actor_update.add_argument("--group", default="", help="Target group_id (default: active group)")
    p_actor_update.set_defaults(func=cmd_actor_update)

    p_actor_secrets = actor_sub.add_parser("secrets", help="Manage runtime-only secrets env (not in ledger)")
    p_actor_secrets.add_argument("actor_id", help="Actor id")
    p_actor_secrets.add_argument("--set", action="append", default=[], help="Set secret env (KEY=VALUE), repeatable")
    p_actor_secrets.add_argument("--unset", action="append", default=[], help="Unset secret key (KEY), repeatable")
    p_actor_secrets.add_argument("--clear", action="store_true", help="Clear all secrets for this actor")
    p_actor_secrets.add_argument("--keys", action="store_true", help="List configured keys (no values)")
    p_actor_secrets.add_argument("--restart", action="store_true", help="Restart actor after updating secrets")
    p_actor_secrets.add_argument("--by", default="user", help="Requester (default: user)")
    p_actor_secrets.add_argument("--group", default="", help="Target group_id (default: active group)")
    p_actor_secrets.set_defaults(func=cmd_actor_secrets)

    p_inbox = sub.add_parser("inbox", help="List unread messages for an actor (chat messages + system notifications)")
    p_inbox.add_argument("--actor-id", required=True, help="Target actor id")
    p_inbox.add_argument("--by", default="user", help="Requester (default: user)")
    p_inbox.add_argument("--group", default="", help="Target group_id (default: active group)")
    p_inbox.add_argument("--limit", type=int, default=50, help="Max messages to return (default: 50)")
    p_inbox.add_argument("--kind-filter", choices=["all", "chat", "notify"], default="all", help="Filter by message type: all (default), chat (messages only), notify (system notifications only)")
    p_inbox.add_argument("--mark-read", action="store_true", help="Mark returned messages as read up to the last one")
    p_inbox.set_defaults(func=cmd_inbox)

    p_read = sub.add_parser("read", help="Mark a message event as read for an actor")
    p_read.add_argument("event_id", help="Target message event id")
    p_read.add_argument("--actor-id", required=True, help="Target actor id")
    p_read.add_argument("--by", default="user", help="Requester (default: user)")
    p_read.add_argument("--group", default="", help="Target group_id (default: active group)")
    p_read.set_defaults(func=cmd_read)

    p_prompt = sub.add_parser("prompt", help="Render a concise SYSTEM prompt for a group actor")
    p_prompt.add_argument("--actor-id", required=True, help="Target actor id")
    p_prompt.add_argument("--group", default="", help="Target group_id (default: active group)")
    p_prompt.set_defaults(func=cmd_prompt)

    p_send = sub.add_parser("send", help="Append a chat message into the active group ledger (or --group)")
    p_send.add_argument("text", help="Message text")
    p_send.add_argument("--group", default="", help="Target group_id (default: active group)")
    p_send.add_argument("--by", default="user", help="Sender label (default: user)")
    p_send.add_argument(
        "--to",
        action="append",
        default=[],
        help="Recipients/selectors (repeatable, supports comma-separated, e.g. --to peer-a --to @foreman,@peers)",
    )
    p_send.add_argument("--priority", choices=["normal", "attention"], default="normal", help="Message priority")
    p_send.add_argument("--reply-required", action="store_true", help="Require recipients to reply")
    p_send.add_argument("--path", default="", help="Send message under this scope (path inside repo/scope)")
    p_send.set_defaults(func=cmd_send)

    p_reply = sub.add_parser("reply", help="Reply to a message (IM-style, with quote)")
    p_reply.add_argument("event_id", help="Event ID of the message to reply to")
    p_reply.add_argument("text", help="Reply text")
    p_reply.add_argument("--group", default="", help="Target group_id (default: active group)")
    p_reply.add_argument("--by", default="user", help="Sender label (default: user)")
    p_reply.add_argument(
        "--to",
        action="append",
        default=[],
        help="Recipients (default: original sender); repeatable, comma-separated",
    )
    p_reply.add_argument("--priority", choices=["normal", "attention"], default="normal", help="Message priority")
    p_reply.add_argument("--reply-required", action="store_true", help="Require recipients to reply")
    p_reply.set_defaults(func=cmd_reply)

    p_tail = sub.add_parser("tail", help="Tail the active group's ledger (or --group)")
    p_tail.add_argument("--group", default="", help="Target group_id (default: active group)")
    p_tail.add_argument("-n", "--lines", type=int, default=50, help="Show last N lines (default: 50)")
    p_tail.add_argument("-f", "--follow", action="store_true", help="Follow (like tail -f)")
    p_tail.set_defaults(func=cmd_tail)

    p_ledger = sub.add_parser("ledger", help="Ledger maintenance (snapshot/compaction)")
    ledger_sub = p_ledger.add_subparsers(dest="action", required=True)

    p_ls = ledger_sub.add_parser("snapshot", help="Write a ledger snapshot under group state/")
    p_ls.add_argument("--group", default="", help="Target group_id (default: active group)")
    p_ls.add_argument("--by", default="user", help="Requester (default: user)")
    p_ls.add_argument("--reason", default="manual", help="Reason label (default: manual)")
    p_ls.set_defaults(func=cmd_ledger_snapshot)

    p_lc = ledger_sub.add_parser("compact", help="Archive globally-read events to keep active ledger small")
    p_lc.add_argument("--group", default="", help="Target group_id (default: active group)")
    p_lc.add_argument("--by", default="user", help="Requester (default: user)")
    p_lc.add_argument("--reason", default="manual", help="Reason label (default: manual)")
    p_lc.add_argument("--force", action="store_true", help="Force a compaction run (ignore thresholds)")
    p_lc.set_defaults(func=cmd_ledger_compact)

    p_daemon = sub.add_parser("daemon", help="Manage ccccd daemon")
    p_daemon.add_argument("action", choices=["start", "stop", "status"], help="Action")
    p_daemon.set_defaults(func=cmd_daemon)

    # IM Bridge commands
    p_im = sub.add_parser("im", help="Manage IM bridge (Telegram/Slack/Discord/Feishu/Lark/DingTalk)")
    im_sub = p_im.add_subparsers(dest="action", required=True)

    p_im_set = im_sub.add_parser("set", help="Set IM bridge configuration")
    p_im_set.add_argument("platform", choices=["telegram", "slack", "discord", "feishu", "dingtalk"], help="IM platform")
    p_im_set.add_argument("--token-env", default="", help="Environment variable name for token (telegram/discord)")
    p_im_set.add_argument("--bot-token-env", default="", help="Bot token env var (Slack: xoxb- for outbound)")
    p_im_set.add_argument("--app-token-env", default="", help="App token env var (Slack: xapp- for inbound Socket Mode)")
    p_im_set.add_argument("--app-key-env", default="", help="App ID (Feishu/Lark) / App Key (DingTalk) env var")
    p_im_set.add_argument("--app-secret-env", default="", help="App Secret (Feishu/Lark/DingTalk) env var")
    p_im_set.add_argument("--domain", default="", help="Feishu domain override: feishu (CN) or lark (Global)")
    p_im_set.add_argument("--robot-code-env", default="", help="Robot code env var (DingTalk; optional but recommended)")
    p_im_set.add_argument("--robot-code", default="", help="Robot code value directly (DingTalk; not recommended, prefer env var)")
    p_im_set.add_argument("--token", default="", help="Token value directly (not recommended, use env vars)")
    p_im_set.add_argument("--group", default="", help="Target group_id (default: active group)")
    p_im_set.set_defaults(func=cmd_im_set)

    p_im_unset = im_sub.add_parser("unset", help="Remove IM bridge configuration")
    p_im_unset.add_argument("--group", default="", help="Target group_id (default: active group)")
    p_im_unset.set_defaults(func=cmd_im_unset)

    p_im_config = im_sub.add_parser("config", help="Show IM bridge configuration")
    p_im_config.add_argument("--group", default="", help="Target group_id (default: active group)")
    p_im_config.set_defaults(func=cmd_im_config)

    p_im_start = im_sub.add_parser("start", help="Start IM bridge")
    p_im_start.add_argument("--group", default="", help="Target group_id (default: active group)")
    p_im_start.set_defaults(func=cmd_im_start)

    p_im_stop = im_sub.add_parser("stop", help="Stop IM bridge")
    p_im_stop.add_argument("--group", default="", help="Target group_id (default: active group)")
    p_im_stop.set_defaults(func=cmd_im_stop)

    p_im_status = im_sub.add_parser("status", help="Show IM bridge status")
    p_im_status.add_argument("--group", default="", help="Target group_id (default: active group)")
    p_im_status.set_defaults(func=cmd_im_status)

    p_im_logs = im_sub.add_parser("logs", help="Show IM bridge logs")
    p_im_logs.add_argument("--group", default="", help="Target group_id (default: active group)")
    p_im_logs.add_argument("-n", "--lines", type=int, default=50, help="Number of lines to show (default: 50)")
    p_im_logs.add_argument("-f", "--follow", action="store_true", help="Follow log output (like tail -f)")
    p_im_logs.set_defaults(func=cmd_im_logs)

    p_im_bind = im_sub.add_parser("bind", help="Bind a pending authorization key to authorize a chat")
    p_im_bind.add_argument("--key", required=True, help="Authorization key from /subscribe")
    p_im_bind.add_argument("--group", default="", help="Target group_id (default: active group)")
    p_im_bind.set_defaults(func=cmd_im_bind)

    p_im_authorized = im_sub.add_parser("authorized", help="List authorized chats")
    p_im_authorized.add_argument("--group", default="", help="Target group_id (default: active group)")
    p_im_authorized.set_defaults(func=cmd_im_authorized)

    p_im_revoke = im_sub.add_parser("revoke", help="Revoke authorization for a chat")
    p_im_revoke.add_argument("--chat-id", required=True, dest="chat_id", help="Chat ID to revoke")
    p_im_revoke.add_argument("--thread-id", type=int, default=0, dest="thread_id", help="Thread ID (default: 0)")
    p_im_revoke.add_argument("--group", default="", help="Target group_id (default: active group)")
    p_im_revoke.set_defaults(func=cmd_im_revoke)

    p_web = sub.add_parser("web", help="Run web server only (requires daemon to be running)")
    p_web.add_argument("--host", default="127.0.0.1", help="Bind host (default: 127.0.0.1)")
    p_web.add_argument("--port", type=int, default=8848, help="Bind port (default: 8848)")
    p_web.add_argument(
        "--mode",
        choices=["normal", "exhibit"],
        default="normal",
        help="Web mode: normal (read/write) or exhibit (read-only) (default: normal)",
    )
    p_web.add_argument("--exhibit", action="store_true", help="Shortcut for: --mode exhibit")
    p_web.add_argument("--reload", action="store_true", help="Enable autoreload (dev)")
    p_web.add_argument("--log-level", default="info", help="Uvicorn log level (default: info)")
    p_web.set_defaults(func=cmd_web)

    p_mcp = sub.add_parser("mcp", help="Run the MCP server (stdio mode, for agent runtimes)")
    p_mcp.set_defaults(func=cmd_mcp)

    p_setup = sub.add_parser("setup", help="Setup MCP for agent runtimes (configure MCP, print guidance)")
    p_setup.add_argument(
        "--runtime",
        choices=["claude", "codex", "droid", "amp", "auggie", "neovate", "gemini", "cursor", "kilocode", "opencode", "copilot", "custom"],
        default="",
        help="Target runtime (default: all supported runtimes)",
    )
    p_setup.add_argument("--path", default=".", help="Project path (default: current directory)")
    p_setup.set_defaults(func=cmd_setup)

    p_doctor = sub.add_parser("doctor", help="Check environment and show available agent runtimes")
    p_doctor.add_argument("--all", action="store_true", help="Show all known runtimes (not just primary ones)")
    p_doctor.set_defaults(func=cmd_doctor)

    p_runtime = sub.add_parser("runtime", help="Manage agent runtimes")
    runtime_sub = p_runtime.add_subparsers(dest="action", required=True)

    p_runtime_list = runtime_sub.add_parser("list", help="List available agent runtimes")
    p_runtime_list.add_argument("--all", action="store_true", help="Show all known runtimes (not just primary ones)")
    p_runtime_list.set_defaults(func=cmd_runtime_list)

    p_ver = sub.add_parser("version", help="Show version")
    p_ver.set_defaults(func=cmd_version)

    p_status = sub.add_parser("status", help="Show overall CCCC status (daemon, groups, actors)")
    p_status.set_defaults(func=cmd_status)

    return p


def main(argv: Optional[list[str]] = None) -> int:
    if argv is None:
        argv = sys.argv[1:]
    if len(argv) == 0:
        return int(_default_entry())
    parser = build_parser()
    args = parser.parse_args(argv)
    return int(args.func(args))


if __name__ == "__main__":
    raise SystemExit(main())
