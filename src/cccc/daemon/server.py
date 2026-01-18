from __future__ import annotations

import json
import copy
import logging
import os
import socket
import sys
import time
import signal
import threading
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Dict, Optional, Tuple

logger = logging.getLogger("cccc.daemon.server")

from .. import __version__
from ..contracts.v1 import ChatMessageData, DaemonError, DaemonRequest, DaemonResponse
from ..kernel.active import load_active, set_active_group_id
from ..kernel.group import ensure_group_for_scope, load_group
from ..kernel.group import attach_scope_to_group, create_group, delete_group, detach_scope_from_group, set_active_scope, update_group
from ..kernel.ledger import append_event
from ..kernel.registry import load_registry
from ..kernel.scope import detect_scope
from ..kernel.actors import add_actor, find_actor, find_foreman, list_actors, remove_actor, resolve_recipient_tokens, update_actor, get_effective_role
from ..kernel.blobs import resolve_blob_attachment_path
from ..kernel.inbox import find_event, get_cursor, get_quote_text, has_chat_ack, is_message_for_actor, latest_unread_event, set_cursor, unread_messages
from ..kernel.ledger_retention import compact as compact_ledger
from ..kernel.ledger_retention import snapshot as snapshot_ledger
from ..kernel.permissions import require_actor_permission, require_group_permission, require_inbox_permission
from ..kernel.settings import get_observability_settings, update_observability_settings
from ..kernel.terminal_transcript import apply_terminal_transcript_patch, get_terminal_transcript_settings
from ..kernel.messaging import get_default_send_to, enabled_recipient_actor_ids, targets_any_agent, default_reply_recipients
from ..paths import ensure_home
from ..runners import pty as pty_runner
from ..runners import headless as headless_runner
from ..util.obslog import setup_root_json_logging
from ..util.fs import atomic_write_json, atomic_write_text, read_json
from ..util.time import utc_now_iso
from .automation import AutomationManager
from .delivery import (
    inject_system_prompt as deliver_system_prompt,
    get_headless_targets_for_message,
    pty_submit_text,
    render_delivery_text,
    deliver_message_with_preamble,
    queue_chat_message,
    queue_system_notify,
    flush_pending_messages,
    tick_delivery,
    clear_preamble_sent,
    THROTTLE,
)
from .ops.context_ops import (
    handle_context_get,
    handle_context_sync,
    handle_task_list,
    handle_presence_get,
)
from .ops.runner_ops import (
    handle_headless_status,
    handle_headless_set_status,
    handle_headless_ack_message,
    is_actor_running,
    is_group_running,
    stop_actor as runner_stop_actor,
    stop_group as runner_stop_group,
    stop_all as runner_stop_all,
)
from .ops.template_ops import (
    group_create_from_template,
    group_template_export,
    group_template_import_replace,
    group_template_preview,
)

import subprocess


_OBS_LOCK = threading.Lock()
_OBSERVABILITY: Dict[str, Any] = {}


def _get_observability() -> Dict[str, Any]:
    with _OBS_LOCK:
        return copy.deepcopy(_OBSERVABILITY) if _OBSERVABILITY else get_observability_settings()


def _developer_mode_enabled() -> bool:
    obs = _get_observability()
    return bool(obs.get("developer_mode", False))


def _apply_observability_settings(home: Path, obs: Dict[str, Any]) -> None:
    """Apply observability settings in-process (best-effort)."""
    if not isinstance(obs, dict):
        return
    with _OBS_LOCK:
        _OBSERVABILITY.clear()
        _OBSERVABILITY.update(copy.deepcopy(obs))

    # Logging: keep simple; configure root JSONL logger to stderr.
    level = str(obs.get("log_level") or "INFO").strip().upper() or "INFO"
    if obs.get("developer_mode"):
        # Developer mode typically wants more detail.
        if level == "INFO":
            level = "DEBUG"
    setup_root_json_logging(component="daemon", level=level, force=True)


def _pty_backlog_bytes() -> int:
    """Best-effort per-actor PTY backlog size (ring buffer)."""
    obs = _get_observability()
    tt = obs.get("terminal_transcript") if isinstance(obs, dict) else None
    n = 10 * 1024 * 1024
    if isinstance(tt, dict):
        try:
            n = int(tt.get("per_actor_bytes") or 0)
        except Exception:
            n = 0
    if n <= 0:
        n = 10 * 1024 * 1024
    if n > 50_000_000:
        n = 50_000_000
    return int(n)


def _pty_supported() -> bool:
    return bool(getattr(pty_runner, "PTY_SUPPORTED", True))


def _effective_runner_kind(runner_kind: str) -> str:
    """Return the effective runner kind for this platform.

    Windows (and some Python builds) cannot run PTY; treat PTY as headless.
    """
    rk = str(runner_kind or "").strip() or "pty"
    if rk == "headless":
        return "headless"
    return "pty" if _pty_supported() else "headless"


def _can_read_terminal_transcript(group: Any, *, by: str, target_actor_id: str) -> bool:
    who = str(by or "").strip()
    target = str(target_actor_id or "").strip()
    if not target:
        return False
    if not who or who == "user":
        return True
    if who == target:
        return True
    if find_actor(group, who) is None:
        return False
    tt = get_terminal_transcript_settings(group.doc)
    vis = str(tt.get("visibility") or "foreman")
    if vis == "all":
        return True
    if vis == "foreman" and get_effective_role(group, who) == "foreman":
        return True
    return False

SUPPORTED_RUNTIMES = (
    "amp",
    "auggie",
    "claude",
    "codex",
    "cursor",
    "droid",
    "gemini",
    "kilocode",
    "neovate",
    "opencode",
    "copilot",
    "custom",
)

AUTO_MCP_RUNTIMES = ("claude", "codex", "droid", "amp", "auggie", "neovate", "gemini")


def _is_mcp_installed(runtime: str) -> bool:
    """Check if cccc MCP server is already installed for the runtime."""
    try:
        if runtime == "claude":
            result = subprocess.run(
                ["claude", "mcp", "get", "cccc"],
                capture_output=True,
                timeout=10,
            )
            return result.returncode == 0
        
        elif runtime == "codex":
            result = subprocess.run(
                ["codex", "mcp", "get", "cccc"],
                capture_output=True,
                timeout=10,
            )
            return result.returncode == 0
        
        elif runtime == "droid":
            # droid doesn't have 'get', use list and grep
            result = subprocess.run(
                ["droid", "mcp", "list"],
                capture_output=True,
                text=True,
                timeout=10,
            )
            return result.returncode == 0 and "cccc" in (result.stdout or "")

        elif runtime == "amp":
            settings_path = Path.home() / ".config" / "amp" / "settings.json"
            if not settings_path.exists():
                return False
            doc = json.loads(settings_path.read_text(encoding="utf-8") or "{}")
            if not isinstance(doc, dict):
                return False
            servers = doc.get("amp.mcpServers")
            return isinstance(servers, dict) and "cccc" in servers
        
        elif runtime == "auggie":
            settings_path = Path.home() / ".augment" / "settings.json"
            if not settings_path.exists():
                return False
            doc = json.loads(settings_path.read_text(encoding="utf-8") or "{}")
            if not isinstance(doc, dict):
                return False
            servers = doc.get("mcpServers")
            return isinstance(servers, dict) and "cccc" in servers

        elif runtime == "neovate":
            config_path = Path.home() / ".neovate" / "config.json"
            if not config_path.exists():
                return False
            doc = json.loads(config_path.read_text(encoding="utf-8") or "{}")
            if not isinstance(doc, dict):
                return False
            servers = doc.get("mcpServers")
            return isinstance(servers, dict) and "cccc" in servers
        
        elif runtime == "gemini":
            settings_path = Path.home() / ".gemini" / "settings.json"
            if not settings_path.exists():
                return False
            doc = json.loads(settings_path.read_text(encoding="utf-8") or "{}")
            if not isinstance(doc, dict):
                return False
            servers = doc.get("mcpServers")
            return isinstance(servers, dict) and "cccc" in servers
    
    except (FileNotFoundError, subprocess.TimeoutExpired):
        pass
    except Exception:
        pass
    
    return False


def _ensure_mcp_installed(runtime: str, cwd: Path) -> bool:
    """Ensure MCP server is installed for the given runtime.
    
    Uses CLI commands to install MCP at user level:
    - claude: claude mcp add -s user cccc -- cccc mcp
    - codex: codex mcp add cccc -- cccc mcp (always user level)
    - droid: droid mcp add cccc -- cccc mcp (always user level)
    - amp: amp mcp add cccc cccc mcp (user config)
    - auggie: auggie mcp add cccc -- cccc mcp (user config)
    - neovate: neovate mcp add -g cccc cccc mcp (global config)
    - gemini: gemini mcp add -s user cccc cccc mcp (user config)
    
    Checks if already installed first to avoid unnecessary subprocess calls.
    
    Returns True if MCP was installed or already exists, False on error.
    """
    if runtime not in AUTO_MCP_RUNTIMES:
        return True  # Manual MCP config (or unsupported): skip
    
    # Check if already installed
    if _is_mcp_installed(runtime):
        return True
    
    try:
        if runtime == "claude":
            # Claude Code: user scope (available in all projects)
            result = subprocess.run(
                ["claude", "mcp", "add", "-s", "user", "cccc", "--", "cccc", "mcp"],
                capture_output=True,
                text=True,
                cwd=str(cwd),
                timeout=30,
            )
            return result.returncode == 0
        
        elif runtime == "codex":
            # Codex CLI: user level (~/.codex/)
            result = subprocess.run(
                ["codex", "mcp", "add", "cccc", "--", "cccc", "mcp"],
                capture_output=True,
                text=True,
                cwd=str(cwd),
                timeout=30,
            )
            return result.returncode == 0
        
        elif runtime == "droid":
            # Droid: user level
            result = subprocess.run(
                ["droid", "mcp", "add", "cccc", "--", "cccc", "mcp"],
                capture_output=True,
                text=True,
                cwd=str(cwd),
                timeout=30,
            )
            return result.returncode == 0

        elif runtime == "amp":
            # Amp: user config (~/.config/amp/settings.json)
            result = subprocess.run(
                ["amp", "mcp", "add", "cccc", "cccc", "mcp"],
                capture_output=True,
                text=True,
                cwd=str(cwd),
                timeout=30,
            )
            return result.returncode == 0
        
        elif runtime == "auggie":
            # Auggie: user config (~/.augment/settings.json)
            result = subprocess.run(
                ["auggie", "mcp", "add", "cccc", "--", "cccc", "mcp"],
                capture_output=True,
                text=True,
                cwd=str(cwd),
                timeout=30,
            )
            return result.returncode == 0

        elif runtime == "neovate":
            # Neovate Code: global config (~/.neovate/config.json)
            result = subprocess.run(
                ["neovate", "mcp", "add", "-g", "cccc", "cccc", "mcp"],
                capture_output=True,
                text=True,
                cwd=str(cwd),
                timeout=30,
            )
            return result.returncode == 0
        
        elif runtime == "gemini":
            # Gemini CLI: user scope (~/.gemini/settings.json)
            result = subprocess.run(
                ["gemini", "mcp", "add", "-s", "user", "cccc", "cccc", "mcp"],
                capture_output=True,
                text=True,
                cwd=str(cwd),
                timeout=30,
            )
            return result.returncode == 0
    
    except (FileNotFoundError, subprocess.TimeoutExpired):
        pass
    
    return False


def _prepare_pty_env(env: Dict[str, Any]) -> Dict[str, str]:
    """Prepare environment variables for PTY session.
    
    Key modifications:
    - Disable bracketed paste mode via INPUTRC to prevent readline from
      interfering with programmatic text input
    - This is critical for reliable message delivery to CLI applications
    
    Returns a new dict with string values only.
    """
    result = {str(k): str(v) for k, v in env.items() if isinstance(k, str)}
    
    # Create a temporary inputrc file to disable bracketed paste
    # This is more reliable than sending escape sequences
    home = ensure_home()
    inputrc_path = home / "daemon" / "inputrc"
    try:
        inputrc_path.parent.mkdir(parents=True, exist_ok=True)
        inputrc_content = "set enable-bracketed-paste off\n"
        if not inputrc_path.exists() or inputrc_path.read_text() != inputrc_content:
            inputrc_path.write_text(inputrc_content)
        result["INPUTRC"] = str(inputrc_path)
    except Exception:
        pass
    
    return result


def _inject_actor_context_env(env: Dict[str, Any], *, group_id: str, actor_id: str) -> Dict[str, Any]:
    """Inject per-actor context for MCP servers/tools into the actor process env.

    This is runtime-only (not persisted to group docs).
    """
    out: Dict[str, Any] = dict(env or {})
    out["CCCC_GROUP_ID"] = str(group_id or "").strip()
    out["CCCC_ACTOR_ID"] = str(actor_id or "").strip()
    return out


AUTOMATION = AutomationManager()

_AUTOMATION_RESET_NOTIFY_KINDS = {"nudge", "keepalive", "help_nudge", "actor_idle", "silence_check", "standup"}


def _foreman_id(group: Any) -> str:
    try:
        foreman = find_foreman(group)
    except Exception:
        foreman = None
    if isinstance(foreman, dict):
        return str(foreman.get("id") or "").strip()
    return ""


def _reset_automation_timers_if_active(group: Any) -> None:
    """Reset automation timers without catch-up bursts.

    Used on resume/start and when foreman changes (ownership transfer).
    """
    try:
        from ..kernel.group import get_group_state

        if get_group_state(group) != "active":
            return
        AUTOMATION.on_resume(group)
        try:
            THROTTLE.clear_pending_system_notifies(group.group_id, notify_kinds=set(_AUTOMATION_RESET_NOTIFY_KINDS))
        except Exception:
            pass
    except Exception:
        pass


def _maybe_reset_automation_on_foreman_change(group: Any, *, before_foreman_id: str) -> None:
    after = _foreman_id(group)
    if str(before_foreman_id or "") == str(after or ""):
        return
    _reset_automation_timers_if_active(group)


@dataclass
class DaemonPaths:
    home: Path

    @property
    def daemon_dir(self) -> Path:
        return self.home / "daemon"

    @property
    def sock_path(self) -> Path:
        return self.daemon_dir / "ccccd.sock"

    @property
    def addr_path(self) -> Path:
        # Cross-platform daemon endpoint descriptor (TCP fallback on Windows).
        return self.daemon_dir / "ccccd.addr.json"

    @property
    def pid_path(self) -> Path:
        return self.daemon_dir / "ccccd.pid"

    @property
    def log_path(self) -> Path:
        return self.daemon_dir / "ccccd.log"


def default_paths() -> DaemonPaths:
    return DaemonPaths(home=ensure_home())


def _desired_daemon_transport() -> str:
    override = str(os.environ.get("CCCC_DAEMON_TRANSPORT") or "").strip().lower()
    if override in ("unix", "tcp"):
        return override
    # Default: AF_UNIX on POSIX, TCP on Windows.
    return "tcp" if os.name == "nt" else "unix"


def _allow_remote_daemon() -> bool:
    """Whether it's OK to bind the daemon to a non-loopback TCP host.

    Warning: the daemon IPC has no authentication.
    """
    v = str(os.environ.get("CCCC_DAEMON_ALLOW_REMOTE") or "").strip().lower()
    return v in ("1", "true", "yes", "y", "on")


def _daemon_tcp_connect_host(bind_host: str) -> str:
    """Return a TCP host that local clients can connect to."""
    h = str(bind_host or "").strip()
    if not h or h == "localhost":
        return "127.0.0.1"
    if h == "0.0.0.0":
        return "127.0.0.1"
    # This daemon currently uses AF_INET only; avoid writing an IPv6 host into addr.json.
    if ":" in h:
        return "127.0.0.1"
    return h


def _daemon_tcp_bind_host() -> str:
    host = str(os.environ.get("CCCC_DAEMON_HOST") or "").strip()
    if not host or host == "localhost":
        return "127.0.0.1"
    if ":" in host:
        logger.warning("CCCC_DAEMON_HOST=%s looks like IPv6; only IPv4 is supported. Using 127.0.0.1.", host)
        return "127.0.0.1"
    if host == "127.0.0.1":
        return host
    if not _allow_remote_daemon():
        logger.warning(
            "Refusing to bind daemon to non-loopback host %s (no auth). Using 127.0.0.1. "
            "Set CCCC_DAEMON_ALLOW_REMOTE=1 to override.",
            host,
        )
        return "127.0.0.1"
    return host


def _daemon_tcp_port() -> int:
    raw = str(os.environ.get("CCCC_DAEMON_PORT") or "").strip()
    if not raw:
        return 0
    try:
        port = int(raw)
    except Exception:
        return 0
    if port < 0 or port > 65535:
        return 0
    return port


def get_daemon_endpoint(paths: Optional[DaemonPaths] = None) -> Dict[str, Any]:
    """Best-effort: load the daemon endpoint descriptor (cross-platform)."""
    p = paths or default_paths()
    doc = read_json(p.addr_path)
    if isinstance(doc, dict):
        transport = str(doc.get("transport") or "").strip().lower()
        if transport == "tcp":
            try:
                host = str(doc.get("host") or "").strip() or "127.0.0.1"
                port = int(doc.get("port") or 0)
            except Exception:
                host = "127.0.0.1"
                port = 0
            if port > 0:
                return {"transport": "tcp", "host": _daemon_tcp_connect_host(host), "port": port}
        if transport == "unix":
            path = str(doc.get("path") or "").strip()
            if path:
                return {"transport": "unix", "path": path}

    # Back-compat: if no descriptor exists, fall back to AF_UNIX when available.
    if getattr(socket, "AF_UNIX", None) is not None:
        return {"transport": "unix", "path": str(p.sock_path)}
    return {}


def _is_daemon_alive(paths: DaemonPaths) -> bool:
    try:
        return bool(call_daemon({"op": "ping"}, paths=paths, timeout_s=0.2).get("ok"))
    except Exception:
        return False


def _cleanup_stale_daemon_endpoints(paths: DaemonPaths) -> None:
    if _is_daemon_alive(paths):
        return
    for stale in (paths.addr_path, paths.sock_path):
        try:
            stale.unlink(missing_ok=True)
        except Exception:
            pass


def _write_pid(pid_path: Path) -> None:
    pid_path.parent.mkdir(parents=True, exist_ok=True)
    atomic_write_text(pid_path, str(os.getpid()) + "\n")


def _pid_alive(pid: int) -> bool:
    if pid <= 0:
        return False
    try:
        os.kill(pid, 0)
        return True
    except Exception:
        return False


def _best_effort_killpg(pid: int, sig: signal.Signals) -> None:
    if pid <= 0:
        return
    try:
        os.killpg(pid, sig)
    except Exception:
        try:
            os.kill(pid, sig)
        except Exception:
            pass


def _proc_cccc_home(pid: int) -> Optional[Path]:
    """
    Best-effort: read CCCC_HOME for a pid (Linux /proc only).
    Returns resolved home path, or None if unavailable.
    """
    try:
        env_path = Path("/proc") / str(pid) / "environ"
        raw = env_path.read_bytes()
    except Exception:
        return None
    cccc_home = None
    try:
        for item in raw.split(b"\x00"):
            if item.startswith(b"CCCC_HOME="):
                cccc_home = item.split(b"=", 1)[1].decode("utf-8", "ignore").strip()
                break
    except Exception:
        cccc_home = None
    if cccc_home:
        try:
            return Path(cccc_home).expanduser().resolve()
        except Exception:
            return None
    # If env var isn't present, it defaults to ~/.cccc.
    try:
        return (Path.home() / ".cccc").resolve()
    except Exception:
        return None


def _stop_im_bridges_for_group(home: Path, *, group_id: str) -> int:
    """Stop IM bridge processes for a specific group_id. Returns number of pids signaled."""
    gid = str(group_id or "").strip()
    if not gid:
        return 0

    killed: set[int] = set()

    # Stop by pid file first (fast path).
    pid_path = home / "groups" / gid / "state" / "im_bridge.pid"
    if pid_path.exists():
        try:
            pid = int(pid_path.read_text(encoding="utf-8").strip())
            if pid > 0:
                _best_effort_killpg(pid, signal.SIGTERM)
                killed.add(pid)
        except Exception:
            pass
        try:
            pid_path.unlink(missing_ok=True)
        except Exception:
            pass

    # Also scan for orphan processes (pid file may be missing/stale).
    proc = Path("/proc")
    if proc.exists():
        for d in proc.iterdir():
            if not d.is_dir() or not d.name.isdigit():
                continue
            pid = int(d.name)
            if pid in killed:
                continue
            try:
                cmdline = (d / "cmdline").read_bytes().decode("utf-8", "ignore")
            except Exception:
                continue
            if "cccc.ports.im.bridge" not in cmdline or gid not in cmdline:
                continue
            ph = _proc_cccc_home(pid)
            if ph is None:
                continue
            try:
                if ph != home.resolve():
                    continue
            except Exception:
                continue
            _best_effort_killpg(pid, signal.SIGTERM)
            killed.add(pid)

    return len(killed)


def _stop_all_im_bridges(home: Path) -> int:
    """Stop all IM bridge processes for this CCCC_HOME. Returns number of pids signaled."""
    killed: set[int] = set()

    # Stop by pid files under this home.
    base = home / "groups"
    if base.exists():
        for pid_path in base.glob("*/state/im_bridge.pid"):
            try:
                pid = int(pid_path.read_text(encoding="utf-8").strip())
                if pid > 0:
                    _best_effort_killpg(pid, signal.SIGTERM)
                    killed.add(pid)
            except Exception:
                pass
            try:
                pid_path.unlink(missing_ok=True)
            except Exception:
                pass

    # Scan /proc for any remaining bridge processes bound to this home.
    proc = Path("/proc")
    if proc.exists():
        for d in proc.iterdir():
            if not d.is_dir() or not d.name.isdigit():
                continue
            pid = int(d.name)
            if pid in killed:
                continue
            try:
                cmdline = (d / "cmdline").read_bytes().decode("utf-8", "ignore")
            except Exception:
                continue
            if "cccc.ports.im.bridge" not in cmdline:
                continue
            ph = _proc_cccc_home(pid)
            if ph is None:
                continue
            try:
                if ph != home.resolve():
                    continue
            except Exception:
                continue
            _best_effort_killpg(pid, signal.SIGTERM)
            killed.add(pid)

    return len(killed)


def _cleanup_invalid_im_bridges(home: Path) -> Dict[str, int]:
    """
    Cleanup stale/broken IM bridge state for this CCCC_HOME.

    We only remove/stop things that are clearly invalid:
    - Stale pidfiles (pid not alive)
    - Running bridge processes whose group.yaml no longer exists
    """
    killed = 0
    stale_pidfiles = 0

    base = home / "groups"
    if base.exists():
        for pid_path in base.glob("*/state/im_bridge.pid"):
            gid = pid_path.parent.parent.name
            group_yaml = base / gid / "group.yaml"
            try:
                pid = int(pid_path.read_text(encoding="utf-8").strip())
            except Exception:
                pid = 0

            if pid <= 0 or not _pid_alive(pid):
                stale_pidfiles += 1
                try:
                    pid_path.unlink(missing_ok=True)
                except Exception:
                    pass
                continue

            if not group_yaml.exists():
                _best_effort_killpg(pid, signal.SIGTERM)
                killed += 1
                try:
                    pid_path.unlink(missing_ok=True)
                except Exception:
                    pass

    # Also scan /proc for orphan bridge processes (pidfile may be missing).
    proc = Path("/proc")
    if proc.exists():
        for d in proc.iterdir():
            if not d.is_dir() or not d.name.isdigit():
                continue
            pid = int(d.name)
            try:
                cmdline = (d / "cmdline").read_bytes().decode("utf-8", "ignore")
            except Exception:
                continue
            if "cccc.ports.im.bridge" not in cmdline:
                continue

            # Only touch processes that belong to this CCCC_HOME.
            ph = _proc_cccc_home(pid)
            if ph is None:
                continue
            try:
                if ph != home.resolve():
                    continue
            except Exception:
                continue

            # Parse group_id from argv.
            argv = [a for a in cmdline.split("\x00") if a]
            try:
                i = argv.index("cccc.ports.im.bridge")
            except ValueError:
                continue
            if i + 1 >= len(argv):
                continue
            gid = str(argv[i + 1] or "").strip()
            if not gid.startswith("g_"):
                continue

            group_yaml = home / "groups" / gid / "group.yaml"
            if not group_yaml.exists():
                _best_effort_killpg(pid, signal.SIGTERM)
                killed += 1

    return {"killed": killed, "stale_pidfiles": stale_pidfiles}


def _pty_state_path(group_id: str, actor_id: str) -> Path:
    home = ensure_home()
    return home / "groups" / str(group_id) / "state" / "runners" / "pty" / f"{actor_id}.json"


def _write_pty_state(group_id: str, actor_id: str, *, pid: int) -> None:
    p = _pty_state_path(group_id, actor_id)
    atomic_write_json(
        p,
        {
            "v": 1,
            "kind": "pty",
            "group_id": str(group_id),
            "actor_id": str(actor_id),
            "pid": int(pid),
            "started_at": utc_now_iso(),
        },
    )


def _remove_pty_state_if_pid(group_id: str, actor_id: str, *, pid: int) -> None:
    p = _pty_state_path(group_id, actor_id)
    if not p.exists():
        return
    doc = read_json(p)
    try:
        cur = int(doc.get("pid") or 0) if isinstance(doc, dict) else 0
    except Exception:
        cur = 0
    if cur and int(pid) and cur != int(pid):
        return
    try:
        p.unlink()
    except Exception:
        pass


def _headless_state_path(group_id: str, actor_id: str) -> Path:
    home = ensure_home()
    return home / "groups" / str(group_id) / "state" / "runners" / "headless" / f"{actor_id}.json"


def _write_headless_state(group_id: str, actor_id: str) -> None:
    p = _headless_state_path(group_id, actor_id)
    atomic_write_json(
        p,
        {
            "v": 1,
            "kind": "headless",
            "group_id": str(group_id),
            "actor_id": str(actor_id),
            "started_at": utc_now_iso(),
        },
    )


def _remove_headless_state(group_id: str, actor_id: str) -> None:
    p = _headless_state_path(group_id, actor_id)
    try:
        if p.exists():
            p.unlink()
    except Exception:
        pass


def _cleanup_stale_pty_state(home: Path) -> None:
    base = home / "groups"
    if not base.exists():
        return
    for p in base.glob("*/state/runners/pty/*.json"):
        doc = read_json(p)
        if not isinstance(doc, dict) or str(doc.get("kind") or "") != "pty":
            try:
                p.unlink()
            except Exception:
                pass
            continue
        try:
            pid = int(doc.get("pid") or 0)
        except Exception:
            pid = 0
        if pid <= 0 or not _pid_alive(pid):
            try:
                p.unlink()
            except Exception:
                pass
            continue
        _best_effort_killpg(pid, signal.SIGTERM)
        deadline = time.time() + 1.0
        while time.time() < deadline and _pid_alive(pid):
            time.sleep(0.05)
        if _pid_alive(pid):
            _best_effort_killpg(pid, signal.SIGKILL)
        try:
            p.unlink()
        except Exception:
            pass


def _maybe_autostart_enabled_im_bridges() -> None:
    """Autostart IM bridges that are marked enabled in group.yaml.

    We keep this best-effort: failures shouldn't prevent the daemon from coming up.
    """
    home = ensure_home()
    base = home / "groups"
    if not base.exists():
        return

    for p in base.glob("*/group.yaml"):
        gid = p.parent.name
        group = load_group(gid)
        if group is None:
            continue

        im_cfg = group.doc.get("im") if isinstance(group.doc.get("im"), dict) else None
        if not isinstance(im_cfg, dict) or not bool(im_cfg.get("enabled", False)):
            continue

        platform = str(im_cfg.get("platform") or "telegram").strip() or "telegram"
        pid_path = group.path / "state" / "im_bridge.pid"

        if pid_path.exists():
            try:
                pid = int(pid_path.read_text(encoding="utf-8").strip())
            except Exception:
                pid = 0
            if pid > 0 and _pid_alive(pid):
                continue
            try:
                pid_path.unlink(missing_ok=True)
            except Exception:
                pass

        state_dir = group.path / "state"
        try:
            state_dir.mkdir(parents=True, exist_ok=True)
        except Exception:
            pass
        log_path = state_dir / "im_bridge.log"

        log_file = None
        try:
            log_file = log_path.open("a", encoding="utf-8")
            env = os.environ.copy()
            env["CCCC_HOME"] = str(home)
            proc = subprocess.Popen(
                [sys.executable, "-m", "cccc.ports.im.bridge", gid, platform],
                env=env,
                stdout=log_file,
                stderr=log_file,
                stdin=subprocess.DEVNULL,
                start_new_session=True,
                cwd=str(home),
            )
            time.sleep(0.25)
            rc = proc.poll()
            if rc is not None:
                logger.warning("IM bridge autostart failed for %s (platform=%s, code=%s). See log: %s", gid, platform, rc, log_path)
                continue
            try:
                pid_path.write_text(str(proc.pid), encoding="utf-8")
            except Exception:
                pass
        except Exception as e:
            logger.warning("IM bridge autostart failed for %s (platform=%s): %s", gid, platform, e)
        finally:
            try:
                if log_file:
                    log_file.close()
            except Exception:
                pass


def _maybe_autostart_running_groups() -> None:
    home = ensure_home()
    base = home / "groups"
    if not base.exists():
        return
    for p in base.glob("*/group.yaml"):
        gid = p.parent.name
        group = load_group(gid)
        if group is None:
            continue
        if not bool(group.doc.get("running", False)):
            continue
        group_scope_key = str(group.doc.get("active_scope_key") or "").strip()
        if not group_scope_key:
            group.doc["running"] = False
            try:
                group.save()
            except Exception:
                pass
            continue
        for actor in list_actors(group):
            if not isinstance(actor, dict):
                continue
            aid = str(actor.get("id") or "").strip()
            if not aid:
                continue
            if not bool(actor.get("enabled", True)):
                continue
            runner_kind = str(actor.get("runner") or "pty").strip()
            effective_runner = _effective_runner_kind(runner_kind)
            scope_key = str(actor.get("default_scope_key") or group_scope_key).strip()
            url = _find_scope_url(group, scope_key)
            if not url:
                continue
            cwd = Path(url).expanduser().resolve()
            if not cwd.exists():
                continue
            cmd = actor.get("command") if isinstance(actor.get("command"), list) else []
            env = actor.get("env") if isinstance(actor.get("env"), dict) else {}

            runtime = str(actor.get("runtime") or "codex").strip()
            if runtime not in SUPPORTED_RUNTIMES:
                continue
            if runtime == "custom" and effective_runner != "headless" and not cmd:
                continue

            # Best-effort MCP installation (non-fatal, but important for correctness).
            ok_mcp = True
            try:
                ok_mcp = bool(_ensure_mcp_installed(runtime, cwd))
            except Exception:
                ok_mcp = False
            if not ok_mcp and runtime in AUTO_MCP_RUNTIMES:
                logger.warning(
                    "MCP server 'cccc' is not installed for %s/%s (runtime=%s); actor will start but tools may not work.",
                    gid,
                    aid,
                    runtime,
                )

            # Clear preamble state so system prompt will be injected on first message.
            # NOTE: Do NOT call THROTTLE.clear_actor() here!
            # THROTTLE is an in-memory object, empty after daemon restart.
            # If a user sends a message between socket listen and autostart completion,
            # that message is queued in THROTTLE. Clearing it would lose user messages.
            clear_preamble_sent(group, aid)

            # Start actor session (errors skip this actor, continue with others)
            try:
                if effective_runner == "headless":
                    if runner_kind != "headless" and not _pty_supported():
                        logger.warning(
                            "pty runner is not supported on this platform; autostarting %s/%s as headless",
                            gid,
                            aid,
                        )
                    headless_runner.SUPERVISOR.start_actor(
                        group_id=group.group_id,
                        actor_id=aid,
                        cwd=cwd,
                        env=dict(_inject_actor_context_env(env, group_id=group.group_id, actor_id=aid)),
                    )
                else:
                    session = pty_runner.SUPERVISOR.start_actor(
                        group_id=group.group_id,
                        actor_id=aid,
                        cwd=cwd,
                        command=list(cmd or []),
                        env=_prepare_pty_env(_inject_actor_context_env(env, group_id=group.group_id, actor_id=aid)),
                        max_backlog_bytes=_pty_backlog_bytes(),
                    )
            except Exception as e:
                logger.warning("Autostart failed for %s/%s: %s", gid, aid, e)
                continue

            # Write state file (non-fatal, session already started)
            try:
                if effective_runner == "headless":
                    _write_headless_state(group.group_id, aid)
                else:
                    _write_pty_state(group.group_id, aid, pid=session.pid)
            except Exception as e:
                logger.debug("State write failed for %s/%s: %s", gid, aid, e)

            # Ensure fresh sessions always receive the lazy preamble on first delivery
            clear_preamble_sent(group, aid)
            # Do not drop any messages that may have been queued while the daemon was starting.
            THROTTLE.reset_actor(group.group_id, aid, keep_pending=True)
            # NOTE: Do not inject the system prompt at startup (lazy preamble).
        # Daemon restart should behave like a resume: do not "catch up" on reminders.
        try:
            from ..kernel.group import get_group_state

            if (
                get_group_state(group) == "active"
                and (
                    pty_runner.SUPERVISOR.group_running(group.group_id)
                    or headless_runner.SUPERVISOR.group_running(group.group_id)
                )
            ):
                AUTOMATION.on_resume(group)
        except Exception:
            pass


def _maybe_compact_ledgers(home: Path) -> None:
    base = home / "groups"
    if not base.exists():
        return
    for p in base.glob("*/group.yaml"):
        gid = p.parent.name
        group = load_group(gid)
        if group is None:
            continue
        if not bool(group.doc.get("running", False)):
            continue
        try:
            _ = compact_ledger(group, reason="auto", force=False)
        except Exception:
            continue


def _inject_system_prompt(group: Any, actor: Dict[str, Any]) -> None:
    try:
        deliver_system_prompt(group, actor=actor)
    except Exception:
        pass


def _remove_stale_socket(sock_path: Path) -> None:
    # Deprecated: daemon IPC is now cross-platform and uses `ccccd.addr.json`.
    # Keep this as a best-effort cleanup helper for older call sites.
    try:
        sock_path.unlink(missing_ok=True)
    except Exception:
        pass


def _recv_json_line(conn: socket.socket) -> Dict[str, Any]:
    buf = b""
    while b"\n" not in buf:
        chunk = conn.recv(65536)
        if not chunk:
            break
        buf += chunk
        if len(buf) > 2_000_000:
            break
    line = buf.split(b"\n", 1)[0]
    try:
        return json.loads(line.decode("utf-8", errors="replace"))
    except Exception:
        return {}


def _send_json(conn: socket.socket, obj: Dict[str, Any]) -> None:
    data = (json.dumps(obj, ensure_ascii=False) + "\n").encode("utf-8")
    conn.sendall(data)


def _dump_response(resp: Any) -> Dict[str, Any]:
    """Serialize a daemon response without crashing the daemon.

    The daemon protocol expects `DaemonResponse`, but older/stale code paths (or
    future regressions) might accidentally return a raw dict. Keep the daemon
    alive and return a best-effort error payload instead of raising.
    """
    if resp is None:
        return {"ok": False, "error": {"code": "internal_error", "message": "invalid daemon response: None"}}

    # Pydantic v2
    try:
        fn = getattr(resp, "model_dump", None)
        if callable(fn):
            return fn()
    except Exception:
        pass

    # Pydantic v1 / dataclasses (best-effort)
    try:
        fn = getattr(resp, "dict", None)
        if callable(fn):
            out = fn()
            if isinstance(out, dict):
                return out
    except Exception:
        pass

    if isinstance(resp, dict):
        return resp

    return {
        "ok": False,
        "error": {"code": "internal_error", "message": f"invalid daemon response type: {type(resp).__name__}"},
    }


def _error(code: str, message: str, *, details: Optional[Dict[str, Any]] = None) -> DaemonResponse:
    return DaemonResponse(ok=False, error=DaemonError(code=code, message=message, details=(details or {})))


def _redact_group_doc(doc: Dict[str, Any]) -> Dict[str, Any]:
    """Redact secrets from group.yaml before returning to clients.

    The group doc may contain IM tokens; those should never be exposed via generic
    metadata APIs (Web UI and agents use dedicated endpoints/tools instead).
    """
    try:
        out = copy.deepcopy(doc)
    except Exception:
        out = dict(doc or {})

    im = out.get("im")
    if isinstance(im, dict):
        # Remove raw token values (keep env var names and platform).
        im.pop("token", None)
        im.pop("bot_token", None)
        im.pop("app_token", None)
    return out


def _find_scope_url(group: Any, scope_key: str) -> str:
    wanted = str(scope_key or "").strip()
    if not wanted:
        return ""
    scopes = group.doc.get("scopes")
    if not isinstance(scopes, list):
        return ""
    for sc in scopes:
        if not isinstance(sc, dict):
            continue
        if str(sc.get("scope_key") or "").strip() != wanted:
            continue
        return str(sc.get("url") or "").strip()
    return ""


def _start_actor_process(
    group: Any,
    actor_id: str,
    *,
    command: List[str],
    env: Dict[str, str],
    runner: str,
    runtime: str,
    by: str,
) -> Dict[str, Any]:
    """Start actor PTY/headless session.

    This is the common startup logic used by both actor_add and actor_start.

    Returns:
        {
            "success": True/False,
            "event": {...} or None,  # actor.start event if successful
            "effective_runner": "pty"/"headless" or None,
            "error": "..." or None,  # error message if failed
        }
    """
    # Get working directory from scope
    group_scope_key = str(group.doc.get("active_scope_key") or "").strip()
    if not group_scope_key:
        return {"success": False, "error": "no active scope for group"}

    actor = find_actor(group, actor_id)
    if actor is None:
        return {"success": False, "error": f"actor not found: {actor_id}"}

    scope_key = str(actor.get("default_scope_key") or group_scope_key).strip()
    url = _find_scope_url(group, scope_key)
    if not url:
        return {"success": False, "error": f"scope not attached: {scope_key}"}

    cwd = Path(url).expanduser().resolve()
    if not cwd.exists():
        return {"success": False, "error": f"project root path does not exist: {cwd}"}

    # Validate runtime
    if runtime not in SUPPORTED_RUNTIMES:
        return {"success": False, "error": f"unsupported runtime: {runtime}"}

    effective_runner = _effective_runner_kind(runner)

    # Validate custom runtime
    if runtime == "custom" and effective_runner != "headless" and not command:
        return {"success": False, "error": "custom runtime requires a command (PTY runner)"}

    # Ensure MCP is installed for the runtime
    try:
        _ensure_mcp_installed(runtime, cwd)
    except Exception as e:
        return {"success": False, "error": f"failed to install MCP: {e}"}

    # Start the session
    try:
        if effective_runner == "headless":
            # Start headless session (no PTY, MCP-driven)
            headless_runner.SUPERVISOR.start_actor(
                group_id=group.group_id,
                actor_id=actor_id,
                cwd=cwd,
                env=dict(_inject_actor_context_env(env, group_id=group.group_id, actor_id=actor_id)),
            )
            try:
                _write_headless_state(group.group_id, actor_id)
            except Exception:
                pass
        else:
            # Start PTY session (interactive terminal)
            session = pty_runner.SUPERVISOR.start_actor(
                group_id=group.group_id,
                actor_id=actor_id,
                cwd=cwd,
                command=list(command or []),
                env=_prepare_pty_env(_inject_actor_context_env(env, group_id=group.group_id, actor_id=actor_id)),
                max_backlog_bytes=_pty_backlog_bytes(),
            )
            try:
                _write_pty_state(group.group_id, actor_id, pid=session.pid)
            except Exception:
                pass
    except Exception as e:
        return {"success": False, "error": f"failed to start session: {e}"}

    # Clear preamble state so system prompt will be injected on first message
    clear_preamble_sent(group, actor_id)
    # Reset delivery metadata but keep queued messages.
    THROTTLE.reset_actor(group.group_id, actor_id, keep_pending=True)

    # Mark group as running
    try:
        group.doc["running"] = True
        group.save()
    except Exception:
        pass

    # Record actor.start event
    start_data: Dict[str, Any] = {"actor_id": actor_id, "runner": runner}
    if effective_runner != runner:
        start_data["runner_effective"] = effective_runner
    start_event = append_event(
        group.ledger_path,
        kind="actor.start",
        group_id=group.group_id,
        scope_key="",
        by=by,
        data=start_data,
    )

    return {
        "success": True,
        "event": start_event,
        "effective_runner": effective_runner,
        "error": None,
    }


def _normalize_attachments(group: Any, raw: Any) -> list[dict[str, Any]]:
    if raw is None:
        return []
    if not isinstance(raw, list):
        raise ValueError("attachments must be a list")
    out: list[dict[str, Any]] = []
    for item in raw:
        if not isinstance(item, dict):
            raise ValueError("invalid attachment (must be object)")
        rel_path = str(item.get("path") or "").strip()
        if not rel_path:
            raise ValueError("attachment missing path")
        abs_path = resolve_blob_attachment_path(group, rel_path=rel_path)
        if not abs_path.exists() or not abs_path.is_file():
            raise ValueError(f"attachment not found: {rel_path}")
        try:
            size = int(abs_path.stat().st_size)
        except Exception:
            size = int(item.get("bytes") or 0)
        out.append(
            {
                "kind": str(item.get("kind") or "file"),
                "path": rel_path,
                "title": str(item.get("title") or ""),
                "mime_type": str(item.get("mime_type") or ""),
                "bytes": size,
                "sha256": str(item.get("sha256") or ""),
            }
        )
    return out


def handle_request(req: DaemonRequest) -> Tuple[DaemonResponse, bool]:
    op = str(req.op or "").strip()
    args = req.args or {}

    if op == "ping":
        return (
            DaemonResponse(
                ok=True,
                result={
                    "version": __version__,
                    "pid": os.getpid(),
                    "ts": utc_now_iso(),
                    "ipc_v": 1,
                    "capabilities": {
                        "events_stream": True,
                    },
                },
            ),
            False,
        )

    if op == "shutdown":
        return DaemonResponse(ok=True, result={"message": "shutting down"}), True

    # ---------------------------------------------------------------------
    # Global observability / developer mode (daemon-owned persistence)
    # ---------------------------------------------------------------------

    if op == "observability_get":
        return DaemonResponse(ok=True, result={"observability": _get_observability()}), False

    if op == "observability_update":
        by = str(args.get("by") or "user").strip()
        if by and by != "user":
            return _error("permission_denied", "only user can update global observability settings"), False
        patch = args.get("patch") if isinstance(args.get("patch"), dict) else {}
        if not patch:
            return DaemonResponse(ok=True, result={"observability": _get_observability()}), False
        try:
            updated = update_observability_settings(dict(patch))
            _apply_observability_settings(ensure_home(), updated)
            return DaemonResponse(ok=True, result={"observability": updated}), False
        except Exception as e:
            return _error("observability_update_failed", str(e)), False

    # ---------------------------------------------------------------------
    # Debug (developer mode only)
    # ---------------------------------------------------------------------

    if op == "debug_snapshot":
        if not _developer_mode_enabled():
            return _error("developer_mode_required", "developer mode is disabled"), False
        group_id = str(args.get("group_id") or "").strip()
        by = str(args.get("by") or "user").strip()
        group = load_group(group_id) if group_id else None
        if group_id and group is None:
            return _error("group_not_found", f"group not found: {group_id}"), False
        if group is not None and by and by != "user":
            role = get_effective_role(group, by)
            if role != "foreman":
                return _error("permission_denied", "debug tools are restricted to user + foreman"), False

        try:
            out: Dict[str, Any] = {
                "developer_mode": True,
                "observability": _get_observability(),
                "daemon": {"pid": os.getpid(), "version": __version__, "ts": utc_now_iso()},
            }
            if group is not None:
                out["group"] = {
                    "group_id": group.group_id,
                    "state": str(group.doc.get("state") or "active"),
                    "active_scope_key": str(group.doc.get("active_scope_key") or ""),
                    "title": str(group.doc.get("title") or ""),
                }
                actors = []
                for a in list_actors(group):
                    if not isinstance(a, dict):
                        continue
                    aid = str(a.get("id") or "").strip()
                    if not aid:
                        continue
                    runner_kind = str(a.get("runner") or "pty")
                    effective_runner = _effective_runner_kind(runner_kind)
                    running = False
                    try:
                        if effective_runner == "pty":
                            running = pty_runner.SUPERVISOR.actor_running(group.group_id, aid)
                        elif effective_runner == "headless":
                            running = headless_runner.SUPERVISOR.actor_running(group.group_id, aid)
                    except Exception:
                        running = False
                    actors.append(
                        {
                            "id": aid,
                            "role": get_effective_role(group, aid),
                            "runtime": str(a.get("runtime") or ""),
                            "runner": runner_kind,
                            "runner_effective": (effective_runner if effective_runner != runner_kind else runner_kind),
                            "enabled": bool(a.get("enabled", True)),
                            "running": bool(running),
                            "unread_count": int(a.get("unread_count") or 0),
                        }
                    )
                out["actors"] = actors
                try:
                    out["delivery"] = THROTTLE.debug_summary(group.group_id)
                except Exception:
                    out["delivery"] = {}
            return DaemonResponse(ok=True, result=out), False
        except Exception as e:
            return _error("debug_snapshot_failed", str(e)), False

    if op == "terminal_tail":
        group_id = str(args.get("group_id") or "").strip()
        actor_id = str(args.get("actor_id") or "").strip()
        by = str(args.get("by") or "user").strip()
        max_chars = int(args.get("max_chars") or 8000)
        strip_ansi = bool(args.get("strip_ansi", True))
        compact = bool(args.get("compact", True))
        if not group_id:
            return _error("missing_group_id", "missing group_id"), False
        if not actor_id:
            return _error("missing_actor_id", "missing actor_id"), False
        group = load_group(group_id)
        if group is None:
            return _error("group_not_found", f"group not found: {group_id}"), False
        if not _can_read_terminal_transcript(group, by=by, target_actor_id=actor_id):
            tt = get_terminal_transcript_settings(group.doc)
            role = get_effective_role(group, by) if by and by != "user" else ""
            return _error(
                "permission_denied",
                "terminal transcript is restricted by group settings",
                details={
                    "visibility": str(tt.get("visibility") or "foreman"),
                    "by": by,
                    "by_role": role,
                    "target_actor_id": actor_id,
                    "how_to_enable": "Ask user/foreman to change Settings → Transcript → Visibility.",
                },
            ), False
        actor = find_actor(group, actor_id)
        if not isinstance(actor, dict):
            return _error("actor_not_found", f"actor not found: {actor_id}"), False
        runner_kind = str(actor.get("runner") or "pty").strip()
        if runner_kind != "pty":
            return _error("not_pty_actor", "terminal transcript is only available for PTY actors", details={"runner": runner_kind}), False
        if not pty_runner.SUPERVISOR.actor_running(group_id, actor_id):
            return _error("actor_not_running", "actor is not running (no live transcript available)"), False
        if max_chars <= 0:
            max_chars = 8000
        if max_chars > 200_000:
            max_chars = 200_000

        try:
            raw = b""
            try:
                raw = pty_runner.SUPERVISOR.tail_output(
                    group_id=group_id,
                    actor_id=actor_id,
                    max_bytes=_pty_backlog_bytes(),
                )
            except Exception:
                raw = b""
            raw_text = raw.decode("utf-8", errors="replace")
            text = raw_text
            hint = ""
            if strip_ansi:
                try:
                    from ..util.terminal_render import render_transcript

                    text = render_transcript(text, compact=compact)
                except Exception:
                    pass
                if not text.strip() and raw_text.strip():
                    hint = "Rendered transcript is empty; try disabling Strip ANSI for full-screen TUIs."
            if len(text) > max_chars:
                text = text[-max_chars:]
            return DaemonResponse(
                ok=True,
                result={
                    "group_id": group_id,
                    "actor_id": actor_id,
                    "warning": "Terminal transcript may include sensitive stdout/stderr.",
                    "hint": hint,
                    "text": text,
                },
            ), False
        except Exception as e:
            return _error("terminal_tail_failed", str(e)), False

    if op == "terminal_clear":
        group_id = str(args.get("group_id") or "").strip()
        actor_id = str(args.get("actor_id") or "").strip()
        by = str(args.get("by") or "user").strip()
        if not group_id:
            return _error("missing_group_id", "missing group_id"), False
        if not actor_id:
            return _error("missing_actor_id", "missing actor_id"), False
        group = load_group(group_id)
        if group is None:
            return _error("group_not_found", f"group not found: {group_id}"), False
        if not _can_read_terminal_transcript(group, by=by, target_actor_id=actor_id):
            tt = get_terminal_transcript_settings(group.doc)
            role = get_effective_role(group, by) if by and by != "user" else ""
            return _error(
                "permission_denied",
                "terminal transcript is restricted by group settings",
                details={
                    "visibility": str(tt.get("visibility") or "foreman"),
                    "by": by,
                    "by_role": role,
                    "target_actor_id": actor_id,
                    "how_to_enable": "Ask user/foreman to change Settings → Transcript → Visibility.",
                },
            ), False
        actor = find_actor(group, actor_id)
        if not isinstance(actor, dict):
            return _error("actor_not_found", f"actor not found: {actor_id}"), False
        runner_kind = str(actor.get("runner") or "pty").strip()
        if runner_kind != "pty":
            return _error("not_pty_actor", "terminal transcript is only available for PTY actors", details={"runner": runner_kind}), False
        ok = pty_runner.SUPERVISOR.clear_backlog(group_id=group_id, actor_id=actor_id)
        if not ok:
            return _error("actor_not_running", "actor is not running (nothing to clear)"), False
        return DaemonResponse(ok=True, result={"group_id": group_id, "actor_id": actor_id, "cleared": True}), False

    if op == "debug_tail_logs":
        if not _developer_mode_enabled():
            return _error("developer_mode_required", "developer mode is disabled"), False
        component = str(args.get("component") or "").strip().lower()
        by = str(args.get("by") or "user").strip()
        group_id = str(args.get("group_id") or "").strip()
        lines = int(args.get("lines") or 200)
        if lines <= 0:
            lines = 200
        if lines > 2000:
            lines = 2000

        group = load_group(group_id) if group_id else None
        if group_id and group is None:
            return _error("group_not_found", f"group not found: {group_id}"), False
        if group is not None and by and by != "user":
            role = get_effective_role(group, by)
            if role != "foreman":
                return _error("permission_denied", "debug tools are restricted to user + foreman"), False

        try:
            from ..kernel.ledger import read_last_lines

            home = ensure_home()
            path: Optional[Path] = None
            if component in ("daemon", "ccccd"):
                path = home / "daemon" / "ccccd.log"
            elif component in ("im", "im_bridge"):
                if not group_id:
                    return _error("missing_group_id", "missing group_id for im logs"), False
                path = home / "groups" / group_id / "state" / "im_bridge.log"
            elif component in ("web",):
                path = home / "daemon" / "cccc-web.log"
            else:
                return _error("invalid_component", "unknown component", details={"component": component}), False

            items = read_last_lines(path, int(lines)) if path is not None else []
            return DaemonResponse(ok=True, result={"component": component, "group_id": group_id, "path": str(path) if path else "", "lines": items}), False
        except Exception as e:
            return _error("debug_tail_logs_failed", str(e)), False

    if op == "debug_clear_logs":
        if not _developer_mode_enabled():
            return _error("developer_mode_required", "developer mode is disabled"), False
        component = str(args.get("component") or "").strip().lower()
        by = str(args.get("by") or "user").strip()
        group_id = str(args.get("group_id") or "").strip()

        group = load_group(group_id) if group_id else None
        if group_id and group is None:
            return _error("group_not_found", f"group not found: {group_id}"), False
        if group is not None and by and by != "user":
            role = get_effective_role(group, by)
            if role != "foreman":
                return _error("permission_denied", "debug tools are restricted to user + foreman"), False

        try:
            home = ensure_home()
            path: Optional[Path] = None
            if component in ("daemon", "ccccd"):
                path = home / "daemon" / "ccccd.log"
            elif component in ("im", "im_bridge"):
                if not group_id:
                    return _error("missing_group_id", "missing group_id for im logs"), False
                path = home / "groups" / group_id / "state" / "im_bridge.log"
            elif component in ("web",):
                path = home / "daemon" / "cccc-web.log"
            else:
                return _error("invalid_component", "unknown component", details={"component": component}), False

            if path is None:
                return _error("invalid_component", "unknown component", details={"component": component}), False
            try:
                path.parent.mkdir(parents=True, exist_ok=True)
            except Exception:
                pass
            try:
                with open(path, "w", encoding="utf-8"):
                    pass
            except Exception as e:
                return _error("debug_clear_logs_failed", str(e), details={"path": str(path)}), False
            return DaemonResponse(ok=True, result={"component": component, "group_id": group_id, "path": str(path), "cleared": True}), False
        except Exception as e:
            return _error("debug_clear_logs_failed", str(e)), False

    if op == "attach":
        path = Path(str(args.get("path") or "."))
        scope = detect_scope(path)
        reg = load_registry()
        requested_group_id = str(args.get("group_id") or "").strip()
        if requested_group_id:
            group = load_group(requested_group_id)
            if group is None:
                return _error("group_not_found", f"group not found: {requested_group_id}"), False
            group = attach_scope_to_group(reg, group, scope, set_active=True)
        else:
            group = ensure_group_for_scope(reg, scope)
        append_event(
            group.ledger_path,
            kind="group.attach",
            group_id=group.group_id,
            scope_key=scope.scope_key,
            by=str(args.get("by") or "cli"),
            data={"url": scope.url, "label": scope.label, "git_remote": scope.git_remote},
        )
        return (
            DaemonResponse(
                ok=True,
                result={"group_id": group.group_id, "scope_key": scope.scope_key, "title": group.doc.get("title")},
            ),
            False,
        )

    if op == "group_create":
        reg = load_registry()
        title = str(args.get("title") or "working-group")
        topic = str(args.get("topic") or "")
        group = create_group(reg, title=title, topic=topic)
        ev = append_event(
            group.ledger_path,
            kind="group.create",
            group_id=group.group_id,
            scope_key="",
            by=str(args.get("by") or "cli"),
            data={"title": group.doc.get("title", ""), "topic": group.doc.get("topic", "")},
        )
        return (
            DaemonResponse(ok=True, result={"group_id": group.group_id, "title": group.doc.get("title"), "event": ev}),
            False,
        )

    if op == "group_create_from_template":
        return group_create_from_template(args), False

    if op == "group_template_export":
        return group_template_export(args), False

    if op == "group_template_preview":
        return group_template_preview(args), False

    if op == "group_template_import_replace":
        group_id = str(args.get("group_id") or "").strip()
        before_foreman_id = ""
        if group_id:
            g = load_group(group_id)
            if g is not None:
                before_foreman_id = _foreman_id(g)
        resp = group_template_import_replace(args)
        if resp.ok and group_id:
            g2 = load_group(group_id)
            if g2 is not None:
                _maybe_reset_automation_on_foreman_change(g2, before_foreman_id=before_foreman_id)
        return resp, False

    if op == "group_show":
        group_id = str(args.get("group_id") or "").strip()
        if not group_id:
            return _error("missing_group_id", "missing group_id"), False
        group = load_group(group_id)
        if group is None:
            return _error("group_not_found", f"group not found: {group_id}"), False
        return DaemonResponse(ok=True, result={"group": _redact_group_doc(group.doc)}), False

    if op == "group_update":
        group_id = str(args.get("group_id") or "").strip()
        by = str(args.get("by") or "user").strip()
        patch = args.get("patch") if isinstance(args.get("patch"), dict) else {}
        if not group_id:
            return _error("missing_group_id", "missing group_id"), False
        group = load_group(group_id)
        if group is None:
            return _error("group_not_found", f"group not found: {group_id}"), False
        allowed = {"title", "topic"}
        unknown = set(patch.keys()) - allowed
        if unknown:
            return _error("invalid_patch", "invalid patch keys", details={"unknown_keys": sorted(unknown)}), False
        if not patch:
            return _error("invalid_patch", "empty patch"), False
        try:
            require_group_permission(group, by=by, action="group.update")
            reg = load_registry()
            group = update_group(reg, group, patch=dict(patch))
        except Exception as e:
            return _error("group_update_failed", str(e)), False
        ev = append_event(
            group.ledger_path,
            kind="group.update",
            group_id=group.group_id,
            scope_key="",
            by=by,
            data={"patch": dict(patch)},
        )
        return DaemonResponse(ok=True, result={"group_id": group.group_id, "group": _redact_group_doc(group.doc), "event": ev}), False

    if op == "group_settings_update":
        group_id = str(args.get("group_id") or "").strip()
        by = str(args.get("by") or "user").strip()
        patch = args.get("patch") if isinstance(args.get("patch"), dict) else {}
        if not group_id:
            return _error("missing_group_id", "missing group_id"), False
        group = load_group(group_id)
        if group is None:
            return _error("group_not_found", f"group not found: {group_id}"), False
        
        # Define allowed keys and their target sections
        messaging_keys = {"default_send_to"}
        delivery_keys = {"min_interval_seconds"}
        automation_keys = {
            "nudge_after_seconds",
            "actor_idle_timeout_seconds",
            "keepalive_delay_seconds",
            "keepalive_max_per_actor",
            "silence_timeout_seconds",
            "standup_interval_seconds",
            "help_nudge_interval_seconds",
            "help_nudge_min_messages",
        }
        terminal_transcript_keys = {
            "terminal_transcript_visibility",
            "terminal_transcript_notify_tail",
            "terminal_transcript_notify_lines",
        }
        allowed = messaging_keys | delivery_keys | automation_keys | terminal_transcript_keys
        
        unknown = set(patch.keys()) - allowed
        if unknown:
            return _error("invalid_patch", "invalid patch keys", details={"unknown_keys": sorted(unknown)}), False
        if not patch:
            return _error("invalid_patch", "empty patch"), False
        if "default_send_to" in patch:
            v = str(patch.get("default_send_to") or "").strip()
            if v not in ("foreman", "broadcast"):
                return (
                    _error(
                        "invalid_patch",
                        "default_send_to must be 'foreman' or 'broadcast'",
                        details={"default_send_to": v},
                    ),
                    False,
                )
        try:
            require_group_permission(group, by=by, action="group.settings_update")
            
            # Update messaging policy
            messaging_patch = {k: v for k, v in patch.items() if k in messaging_keys}
            if messaging_patch:
                messaging = group.doc.get("messaging") if isinstance(group.doc.get("messaging"), dict) else {}
                messaging["default_send_to"] = str(messaging_patch.get("default_send_to") or "foreman").strip()
                group.doc["messaging"] = messaging

            # Update delivery settings
            delivery_patch = {k: v for k, v in patch.items() if k in delivery_keys}
            if delivery_patch:
                delivery = group.doc.get("delivery") if isinstance(group.doc.get("delivery"), dict) else {}
                for k, v in delivery_patch.items():
                    delivery[k] = int(v)
                group.doc["delivery"] = delivery
            
            # Update automation settings
            automation_patch = {k: v for k, v in patch.items() if k in automation_keys}
            if automation_patch:
                automation = group.doc.get("automation") if isinstance(group.doc.get("automation"), dict) else {}
                for k, v in automation_patch.items():
                    automation[k] = int(v)
                group.doc["automation"] = automation

            # Update terminal transcript settings
            tt_patch: Dict[str, Any] = {}
            if "terminal_transcript_visibility" in patch:
                tt_patch["visibility"] = patch.get("terminal_transcript_visibility")
            if "terminal_transcript_notify_tail" in patch:
                tt_patch["notify_tail"] = patch.get("terminal_transcript_notify_tail")
            if "terminal_transcript_notify_lines" in patch:
                tt_patch["notify_lines"] = patch.get("terminal_transcript_notify_lines")
            if tt_patch:
                apply_terminal_transcript_patch(group.doc, tt_patch)
            
            group.save()
        except Exception as e:
            return _error("group_settings_update_failed", str(e)), False
        
        # Return combined settings
        combined_settings = {}
        combined_settings["default_send_to"] = get_default_send_to(group.doc)
        combined_settings.update(group.doc.get("delivery") or {})
        combined_settings.update(group.doc.get("automation") or {})
        tt = get_terminal_transcript_settings(group.doc)
        combined_settings.update(
            {
                "terminal_transcript_visibility": tt["visibility"],
                "terminal_transcript_notify_tail": bool(tt["notify_tail"]),
                "terminal_transcript_notify_lines": int(tt["notify_lines"]),
            }
        )
        
        ev = append_event(
            group.ledger_path,
            kind="group.settings_update",
            group_id=group.group_id,
            scope_key="",
            by=by,
            data={"patch": dict(patch)},
        )
        return DaemonResponse(ok=True, result={"group_id": group.group_id, "settings": combined_settings, "event": ev}), False

    if op == "group_detach_scope":
        group_id = str(args.get("group_id") or "").strip()
        by = str(args.get("by") or "user").strip()
        scope_key = str(args.get("scope_key") or "").strip()
        if not group_id:
            return _error("missing_group_id", "missing group_id"), False
        if not scope_key:
            return _error("missing_scope_key", "missing scope_key"), False
        group = load_group(group_id)
        if group is None:
            return _error("group_not_found", f"group not found: {group_id}"), False
        try:
            require_group_permission(group, by=by, action="group.detach_scope")
            reg = load_registry()
            group = detach_scope_from_group(reg, group, scope_key=scope_key)
        except Exception as e:
            return _error("group_detach_scope_failed", str(e)), False
        ev = append_event(
            group.ledger_path,
            kind="group.detach_scope",
            group_id=group.group_id,
            scope_key=scope_key,
            by=by,
            data={"scope_key": scope_key},
        )
        return DaemonResponse(ok=True, result={"group_id": group.group_id, "event": ev}), False

    if op == "group_delete":
        group_id = str(args.get("group_id") or "").strip()
        by = str(args.get("by") or "user").strip()
        if not group_id:
            return _error("missing_group_id", "missing group_id"), False
        group = load_group(group_id)
        if group is None:
            return _error("group_not_found", f"group not found: {group_id}"), False
        try:
            require_group_permission(group, by=by, action="group.delete")
            _stop_im_bridges_for_group(ensure_home(), group_id=group_id)
            pty_runner.SUPERVISOR.stop_group(group_id=group_id)
            headless_runner.SUPERVISOR.stop_group(group_id=group_id)
            reg = load_registry()
            delete_group(reg, group_id=group_id)
            active = load_active()
            if str(active.get("active_group_id") or "") == group_id:
                set_active_group_id("")
        except Exception as e:
            return _error("group_delete_failed", str(e)), False
        return DaemonResponse(ok=True, result={"group_id": group_id}), False

    if op == "group_use":
        group_id = str(args.get("group_id") or "").strip()
        if not group_id:
            return _error("missing_group_id", "missing group_id"), False
        group = load_group(group_id)
        if group is None:
            return _error("group_not_found", f"group not found: {group_id}"), False
        path = Path(str(args.get("path") or "."))
        scope = detect_scope(path)
        reg = load_registry()
        try:
            group = set_active_scope(reg, group, scope_key=scope.scope_key)
        except ValueError as e:
            return (
                _error(
                    "scope_not_attached",
                    str(e),
                    details={"hint": "attach scope first (cccc attach <path> --group <id>)"},
                ),
                False,
            )
        ev = append_event(
            group.ledger_path,
            kind="group.set_active_scope",
            group_id=group.group_id,
            scope_key=scope.scope_key,
            by=str(args.get("by") or "cli"),
            data={"path": scope.url},
        )
        return (
            DaemonResponse(
                ok=True,
                result={"group_id": group.group_id, "active_scope_key": scope.scope_key, "event": ev},
            ),
            False,
        )

    if op == "groups":
        reg = load_registry()
        groups = list(reg.groups.values())
        groups.sort(key=lambda g: (g.get("updated_at") or "", g.get("created_at") or ""), reverse=True)
        out = []
        for g in groups:
            if not isinstance(g, dict):
                continue
            gid = str(g.get("group_id") or "").strip()
            running = (
                (pty_runner.SUPERVISOR.group_running(gid) if gid else False)
                or (headless_runner.SUPERVISOR.group_running(gid) if gid else False)
            )
            item = dict(g)
            item["running"] = bool(running)
            # Load full group doc to get state
            if gid:
                full_group = load_group(gid)
                if full_group is not None:
                    item["state"] = full_group.doc.get("state", "active")
            out.append(item)
        return DaemonResponse(ok=True, result={"groups": out}), False

    if op == "group_start":
        # Batch operation: start ALL actors in the group
        group_id = str(args.get("group_id") or "").strip()
        by = str(args.get("by") or "user").strip()
        if not group_id:
            return _error("missing_group_id", "missing group_id"), False
        group = load_group(group_id)
        if group is None:
            return _error("group_not_found", f"group not found: {group_id}"), False
        group_scope_key = str(group.doc.get("active_scope_key") or "").strip()
        if not group_scope_key:
            return (
                _error(
                    "missing_project_root",
                    "missing project root for group (no active scope)",
                    details={"hint": "Attach a project root first (e.g. cccc attach <path> --group <id>)"},
                ),
                False,
            )
        try:
            require_group_permission(group, by=by, action="group.start")
            actors = list_actors(group)
            start_specs: list[tuple[str, Path, list[str], dict[str, str], Dict[str, Any], str]] = []
            for actor in actors:
                if not isinstance(actor, dict):
                    continue
                aid = str(actor.get("id") or "").strip()
                if not aid:
                    continue
                # Start ALL actors, not just enabled ones

                scope_key = str(actor.get("default_scope_key") or group_scope_key).strip()
                url = _find_scope_url(group, scope_key)
                if not url:
                    return (
                        _error(
                            "scope_not_attached",
                            f"scope not attached: {scope_key}",
                            details={
                                "group_id": group.group_id,
                                "actor_id": aid,
                                "scope_key": scope_key,
                                "hint": "Attach this scope to the group (cccc attach <path> --group <id>)",
                            },
                        ),
                        False,
                    )
                cwd = Path(url).expanduser().resolve()
                if not cwd.exists():
                    return (
                        _error(
                            "invalid_project_root",
                            "project root path does not exist",
                            details={
                                "group_id": group.group_id,
                                "actor_id": aid,
                                "scope_key": scope_key,
                                "path": str(cwd),
                                "hint": "Re-attach a valid project root (cccc attach <path> --group <id>)",
                            },
                        ),
                        False,
                    )
                cmd = actor.get("command") if isinstance(actor.get("command"), list) else []
                env = actor.get("env") if isinstance(actor.get("env"), dict) else {}
                runner_kind = str(actor.get("runner") or "pty").strip()
                start_specs.append((aid, cwd, list(cmd or []), dict(env or {}), dict(actor), runner_kind))

            started: list[str] = []
            forced_headless: list[str] = []
            for aid, cwd, cmd, env, actor, runner_kind in start_specs:
                # Set enabled=true for all actors being started
                try:
                    update_actor(group, aid, {"enabled": True})
                except Exception:
                    pass

                effective_runner = _effective_runner_kind(runner_kind)
                if effective_runner == "headless" and runner_kind != "headless" and not _pty_supported():
                    forced_headless.append(aid)
                
                # Ensure MCP is installed for the runtime BEFORE starting the actor
                runtime = str(actor.get("runtime") or "codex").strip()
                if runtime not in SUPPORTED_RUNTIMES:
                    return (
                        _error(
                            "unsupported_runtime",
                            f"unsupported runtime: {runtime}",
                            details={
                                "group_id": group.group_id,
                                "actor_id": aid,
                                "runtime": runtime,
                                "supported": list(SUPPORTED_RUNTIMES),
                                "hint": "Change the actor runtime to a supported one.",
                            },
                        ),
                        False,
                    )
                if runtime == "custom" and effective_runner != "headless" and not cmd:
                    return (
                        _error(
                            "missing_command",
                            "custom runtime requires a command (PTY runner)",
                            details={
                                "group_id": group.group_id,
                                "actor_id": aid,
                                "runtime": runtime,
                                "hint": "Set actor.command (or switch runner to headless).",
                            },
                        ),
                        False,
                    )
                _ensure_mcp_installed(runtime, cwd)
                
                if effective_runner == "headless":
                    headless_runner.SUPERVISOR.start_actor(
                        group_id=group.group_id,
                        actor_id=aid,
                        cwd=cwd,
                        env=dict(_inject_actor_context_env(env, group_id=group.group_id, actor_id=aid)),
                    )
                    try:
                        _write_headless_state(group.group_id, aid)
                    except Exception:
                        pass
                else:
                    session = pty_runner.SUPERVISOR.start_actor(
                        group_id=group.group_id,
                        actor_id=aid,
                        cwd=cwd,
                        command=cmd,
                        env=_prepare_pty_env(_inject_actor_context_env(env, group_id=group.group_id, actor_id=aid)),
                        max_backlog_bytes=_pty_backlog_bytes(),
                    )
                    try:
                        _write_pty_state(group.group_id, aid, pid=session.pid)
                    except Exception:
                        pass

                # Clear preamble state so system prompt will be injected on first message
                clear_preamble_sent(group, aid)
                # Reset delivery metadata but keep any queued messages.
                THROTTLE.reset_actor(group.group_id, aid, keep_pending=True)

                started.append(aid)
        except Exception as e:
            return _error("group_start_failed", str(e)), False
        if started:
            try:
                group.doc["running"] = True
                group.save()
            except Exception:
                pass
            # Starting a group should behave like a resume: do not "catch up" on reminders.
            _reset_automation_timers_if_active(group)
        data: Dict[str, Any] = {"started": started}
        if forced_headless:
            data["forced_headless"] = forced_headless
        ev = append_event(group.ledger_path, kind="group.start", group_id=group.group_id, scope_key="", by=by, data=data)
        result: Dict[str, Any] = {"group_id": group.group_id, "started": started, "event": ev}
        if forced_headless:
            result["forced_headless"] = forced_headless
        return DaemonResponse(ok=True, result=result), False

    if op == "group_stop":
        # Batch operation: stop ALL actors in the group
        group_id = str(args.get("group_id") or "").strip()
        by = str(args.get("by") or "user").strip()
        if not group_id:
            return _error("missing_group_id", "missing group_id"), False
        group = load_group(group_id)
        if group is None:
            return _error("group_not_found", f"group not found: {group_id}"), False
        try:
            require_group_permission(group, by=by, action="group.stop")
            
            # Set enabled=false for all actors
            actors = list_actors(group)
            stopped: list[str] = []
            for actor in actors:
                if not isinstance(actor, dict):
                    continue
                aid = str(actor.get("id") or "").strip()
                if not aid:
                    continue
                try:
                    update_actor(group, aid, {"enabled": False})
                    stopped.append(aid)
                except Exception:
                    pass
            
            # Stop both PTY and headless runners
            pty_runner.SUPERVISOR.stop_group(group_id=group.group_id)
            headless_runner.SUPERVISOR.stop_group(group_id=group.group_id)
            
            # Clean up PTY state files
            try:
                pdir = _pty_state_path(group.group_id, "_").parent
                for fp in pdir.glob("*.json"):
                    try:
                        fp.unlink()
                    except Exception:
                        pass
            except Exception:
                pass
            # Clean up headless state files
            try:
                hdir = _headless_state_path(group.group_id, "_").parent
                for fp in hdir.glob("*.json"):
                    try:
                        fp.unlink()
                    except Exception:
                        pass
            except Exception:
                pass
        except Exception as e:
            return _error("group_stop_failed", str(e)), False
        try:
            group.doc["running"] = False
            group.save()
        except Exception:
            pass
        ev = append_event(group.ledger_path, kind="group.stop", group_id=group.group_id, scope_key="", by=by, data={"stopped": stopped})
        return DaemonResponse(ok=True, result={"group_id": group.group_id, "stopped": stopped, "event": ev}), False

    if op == "group_set_state":
        group_id = str(args.get("group_id") or "").strip()
        state = str(args.get("state") or "").strip()
        by = str(args.get("by") or "user").strip()
        if not group_id:
            return _error("missing_group_id", "missing group_id"), False
        if not state:
            return _error("missing_state", "missing state"), False
        group = load_group(group_id)
        if group is None:
            return _error("group_not_found", f"group not found: {group_id}"), False
        try:
            require_group_permission(group, by=by, action="group.set_state")
            from ..kernel.group import set_group_state, get_group_state
            old_state = get_group_state(group)
            group = set_group_state(group, state=state)
            new_state = get_group_state(group)
            if old_state in ("idle", "paused") and new_state == "active":
                try:
                    AUTOMATION.on_resume(group)
                except Exception:
                    pass
                try:
                    THROTTLE.clear_pending_system_notifies(
                        group.group_id,
                        notify_kinds={"nudge", "keepalive", "help_nudge", "actor_idle", "silence_check", "standup"},
                    )
                except Exception:
                    pass
        except Exception as e:
            return _error("group_set_state_failed", str(e)), False
        ev = append_event(
            group.ledger_path,
            kind="group.set_state",
            group_id=group.group_id,
            scope_key="",
            by=by,
            data={"old_state": old_state, "new_state": new_state},
        )
        return DaemonResponse(ok=True, result={"group_id": group.group_id, "state": new_state, "event": ev}), False

    if op == "actor_list":
        group_id = str(args.get("group_id") or "").strip()
        include_unread = bool(args.get("include_unread", False))
        if not group_id:
            return _error("missing_group_id", "missing group_id"), False
        group = load_group(group_id)
        if group is None:
            return _error("group_not_found", f"group not found: {group_id}"), False
        actors = list_actors(group)
        # Add effective role and running status for each actor
        for actor in actors:
            aid = str(actor.get("id") or "")
            if aid:
                # Add effective role (based on position)
                actor["role"] = get_effective_role(group, aid)
                # Add actual running status (is the process running?)
                runner_kind = str(actor.get("runner") or "pty").strip()
                effective_runner = _effective_runner_kind(runner_kind)
                if effective_runner == "headless":
                    actor["running"] = headless_runner.SUPERVISOR.actor_running(group_id, aid)
                else:
                    actor["running"] = pty_runner.SUPERVISOR.actor_running(group_id, aid)
                if effective_runner != runner_kind:
                    actor["runner_effective"] = effective_runner
        # Optionally include unread message count for each actor
        if include_unread:
            from ..kernel.inbox import batch_unread_counts
            actor_ids = [str(a.get("id") or "") for a in actors if a.get("id")]
            counts = batch_unread_counts(group, actor_ids=actor_ids)
            for actor in actors:
                aid = str(actor.get("id") or "")
                if aid:
                    actor["unread_count"] = counts.get(aid, 0)
        return DaemonResponse(ok=True, result={"actors": actors}), False

    if op == "actor_add":
        group_id = str(args.get("group_id") or "").strip()
        actor_id = str(args.get("actor_id") or "").strip()
        title = str(args.get("title") or "").strip()
        submit = str(args.get("submit") or "").strip()
        runner = str(args.get("runner") or "").strip()
        if not runner:
            runner = "pty" if _pty_supported() else "headless"
        forced_headless = False
        if runner == "pty" and not _pty_supported():
            runner = "headless"
            forced_headless = True
        runtime = str(args.get("runtime") or "codex").strip()
        by = str(args.get("by") or "user").strip()
        command_raw = args.get("command")
        env_raw = args.get("env")
        default_scope_key = str(args.get("default_scope_key") or "").strip()
        if not group_id:
            return _error("missing_group_id", "missing group_id"), False
        group = load_group(group_id)
        if group is None:
            return _error("group_not_found", f"group not found: {group_id}"), False
        before_foreman_id = _foreman_id(group)
        try:
            require_actor_permission(group, by=by, action="actor.add")
            # Note: role is auto-determined by position (first enabled = foreman)
            if runner not in ("pty", "headless"):
                raise ValueError("invalid runner (must be 'pty' or 'headless')")
            if runtime not in SUPPORTED_RUNTIMES:
                raise ValueError("invalid runtime")
            
            # Foreman safety policy (agent-driven actor.add):
            # Foreman may only create peers by strict-cloning their own runtime config.
            # This keeps behavior predictable and avoids agents arbitrarily selecting runtimes/commands.
            foreman_cfg: Optional[Dict[str, Any]] = None
            if by and by != "user":
                try:
                    if get_effective_role(group, by) == "foreman":
                        foreman_cfg = find_actor(group, by)
                except Exception:
                    foreman_cfg = None

            # Auto-generate actor_id if not provided (use runtime as prefix)
            if not actor_id:
                from ..kernel.actors import generate_actor_id
                actor_id = generate_actor_id(group, runtime=runtime)
            
            command: list[str] = []
            if isinstance(command_raw, list) and all(isinstance(x, str) for x in command_raw):
                command = [str(x) for x in command_raw if str(x).strip()]
            from ..kernel.runtime import get_runtime_command_with_flags

            env: Dict[str, str] = {}
            if isinstance(env_raw, dict) and all(isinstance(k, str) and isinstance(v, str) for k, v in env_raw.items()):
                env = {str(k): str(v) for k, v in env_raw.items()}

            # If foreman is creating a peer, enforce strict clone of runtime/runner/command/env.
            if isinstance(foreman_cfg, dict) and foreman_cfg.get("id") == by:
                foreman_runtime = str(foreman_cfg.get("runtime") or "").strip()
                foreman_runner = str(foreman_cfg.get("runner") or "pty").strip() or "pty"
                foreman_runner_effective = _effective_runner_kind(foreman_runner)
                runner_effective = _effective_runner_kind(runner)
                foreman_command_raw = foreman_cfg.get("command") if isinstance(foreman_cfg.get("command"), list) else []
                foreman_command = [str(x) for x in foreman_command_raw if isinstance(x, str) and str(x).strip()]
                foreman_env_raw = foreman_cfg.get("env") if isinstance(foreman_cfg.get("env"), dict) else {}
                foreman_env = {str(k): str(v) for k, v in foreman_env_raw.items() if isinstance(k, str) and isinstance(v, str)}

                if not foreman_runtime:
                    raise ValueError("foreman config missing runtime")
                if runtime != foreman_runtime:
                    raise ValueError(f"foreman can only add actors with the same runtime as itself (expected: {foreman_runtime})")
                if runner_effective != foreman_runner_effective:
                    raise ValueError(
                        f"foreman can only add actors with the same runner as itself (expected: {foreman_runner_effective})"
                    )

                # Command: treat empty list as omitted (clone foreman).
                if not command:
                    command = list(foreman_command) if foreman_command else get_runtime_command_with_flags(runtime)
                else:
                    if command != foreman_command:
                        raise ValueError("foreman can only add actors by strict-cloning command (runtime/runner/command/env must match foreman)")

                # Env: treat empty dict as omitted (clone foreman).
                if not env:
                    env = dict(foreman_env)
                if env != foreman_env:
                    raise ValueError("foreman can only add actors by strict-cloning env (runtime/runner/command/env must match foreman)")
            else:
                # Auto-set command based on runtime if not provided
                if not command:
                    command = get_runtime_command_with_flags(runtime)

            if runtime == "custom" and runner != "headless" and not command:
                raise ValueError("custom runtime requires a command (PTY runner)")
            actor = add_actor(
                group,
                actor_id=actor_id,
                title=title,
                command=command,
                env=env,
                default_scope_key=default_scope_key,
                submit=submit or "enter",
                runner=runner,  # type: ignore
                runtime=runtime,  # type: ignore
            )
        except Exception as e:
            return _error("actor_add_failed", str(e)), False
        if forced_headless:
            logger.warning(
                "pty runner is not supported on this platform; forcing runner=headless for %s/%s",
                group.group_id,
                str(actor.get("id") or actor_id),
            )
        ev = append_event(
            group.ledger_path,
            kind="actor.add",
            group_id=group.group_id,
            scope_key="",
            by=by,
            data={"actor": actor},
        )
        # New actors should not inherit historical unread messages.
        # Initialize their read cursor to "now" (the actor.add event) so inbox starts empty.
        try:
            set_cursor(group, actor_id, event_id=str(ev.get("id") or ""), ts=str(ev.get("ts") or ""))
        except Exception:
            pass
        # If foreman changes (e.g., first actor created/recreated), restart automation timers to avoid bursts.
        _maybe_reset_automation_on_foreman_change(group, before_foreman_id=before_foreman_id)

        # Auto-start the actor immediately after adding (add = add + start)
        # This ensures the PTY/headless session is running and ready to receive messages.
        start_result = _start_actor_process(
            group,
            actor_id,
            command=command,
            env=env,
            runner=runner,
            runtime=runtime,
            by=by,
        )

        result: Dict[str, Any] = {"actor": actor, "event": ev}
        if start_result["success"]:
            result["start_event"] = start_result["event"]
            result["running"] = True
            if start_result.get("effective_runner") != runner:
                result["runner_effective"] = start_result.get("effective_runner")
        else:
            result["start_error"] = start_result.get("error")
            result["running"] = False
        if forced_headless:
            result["runner_effective"] = "headless"
        return DaemonResponse(ok=True, result=result), False

    if op == "actor_remove":
        group_id = str(args.get("group_id") or "").strip()
        actor_id = str(args.get("actor_id") or "").strip()
        by = str(args.get("by") or "user").strip()
        if not group_id:
            return _error("missing_group_id", "missing group_id"), False
        group = load_group(group_id)
        if group is None:
            return _error("group_not_found", f"group not found: {group_id}"), False
        before_foreman_id = _foreman_id(group)
        try:
            require_actor_permission(group, by=by, action="actor.remove", target_actor_id=actor_id)
            remove_actor(group, actor_id)
            pty_runner.SUPERVISOR.stop_actor(group_id=group.group_id, actor_id=actor_id)
            _remove_pty_state_if_pid(group.group_id, actor_id, pid=0)
            headless_runner.SUPERVISOR.stop_actor(group_id=group.group_id, actor_id=actor_id)
            _remove_headless_state(group.group_id, actor_id)
            THROTTLE.clear_actor(group.group_id, actor_id)
        except Exception as e:
            return _error("actor_remove_failed", str(e)), False
        # Update running flag if no enabled actors remain.
        try:
            any_enabled = any(
                bool(a.get("enabled", True))
                for a in list_actors(group)
                if isinstance(a, dict) and str(a.get("id") or "").strip()
            )
            if not any_enabled:
                group.doc["running"] = False
                group.save()
        except Exception:
            pass
        ev = append_event(
            group.ledger_path,
            kind="actor.remove",
            group_id=group.group_id,
            scope_key="",
            by=by,
            data={"actor_id": actor_id},
        )
        _maybe_reset_automation_on_foreman_change(group, before_foreman_id=before_foreman_id)
        return DaemonResponse(ok=True, result={"actor_id": actor_id, "event": ev}), False

    if op == "actor_update":
        group_id = str(args.get("group_id") or "").strip()
        actor_id = str(args.get("actor_id") or "").strip()
        by = str(args.get("by") or "user").strip()
        patch = args.get("patch") if isinstance(args.get("patch"), dict) else {}
        if not group_id:
            return _error("missing_group_id", "missing group_id"), False
        group = load_group(group_id)
        if group is None:
            return _error("group_not_found", f"group not found: {group_id}"), False
        allowed = {"role", "title", "command", "env", "default_scope_key", "submit", "enabled", "runner", "runtime"}
        unknown = set(patch.keys()) - allowed
        if unknown:
            return _error("invalid_patch", "invalid patch keys", details={"unknown_keys": sorted(unknown)}), False
        if not patch:
            return _error("invalid_patch", "empty patch"), False
        enabled_patched = "enabled" in patch
        before_foreman_id = _foreman_id(group) if enabled_patched else ""
        try:
            require_actor_permission(group, by=by, action="actor.update", target_actor_id=actor_id)
            actor = update_actor(group, actor_id, patch)
        except Exception as e:
            return _error("actor_update_failed", str(e)), False
        if enabled_patched:
            if bool(actor.get("enabled", False)):
                if bool(group.doc.get("running", False)):
                    group_scope_key = str(group.doc.get("active_scope_key") or "").strip()
                    if not group_scope_key:
                        return (
                            _error(
                                "missing_project_root",
                                "missing project root for group (no active scope)",
                                details={"hint": "Attach a project root first (e.g. cccc attach <path> --group <id>)"},
                            ),
                            False,
                        )
                    scope_key = str(actor.get("default_scope_key") or group_scope_key).strip()
                    url = _find_scope_url(group, scope_key)
                    if not url:
                        return (
                            _error(
                                "scope_not_attached",
                                f"scope not attached: {scope_key}",
                                details={
                                    "group_id": group.group_id,
                                    "actor_id": actor_id,
                                    "scope_key": scope_key,
                                    "hint": "Attach this scope to the group (cccc attach <path> --group <id>)",
                                },
                            ),
                            False,
                        )
                    cwd = Path(url).expanduser().resolve()
                    if not cwd.exists():
                        return (
                            _error(
                                "invalid_project_root",
                                "project root path does not exist",
                                details={
                                    "group_id": group.group_id,
                                    "actor_id": actor_id,
                                    "scope_key": scope_key,
                                    "path": str(cwd),
                                    "hint": "Re-attach a valid project root (cccc attach <path> --group <id>)",
                                },
                            ),
                            False,
                        )
                    cmd = actor.get("command") if isinstance(actor.get("command"), list) else []
                    env = actor.get("env") if isinstance(actor.get("env"), dict) else {}
                    runner_kind = str(actor.get("runner") or "pty").strip() or "pty"
                    effective_runner = _effective_runner_kind(runner_kind)
                    runtime = str(actor.get("runtime") or "codex").strip() or "codex"
                    if runtime not in SUPPORTED_RUNTIMES:
                        return (
                            _error(
                                "unsupported_runtime",
                                f"unsupported runtime: {runtime}",
                                details={
                                    "group_id": group.group_id,
                                    "actor_id": actor_id,
                                    "runtime": runtime,
                                    "supported": list(SUPPORTED_RUNTIMES),
                                    "hint": "Change the actor runtime to a supported one.",
                                },
                            ),
                            False,
                        )
                    if runtime == "custom" and effective_runner != "headless" and not cmd:
                        return (
                            _error(
                                "missing_command",
                                "custom runtime requires a command (PTY runner)",
                                details={
                                    "group_id": group.group_id,
                                    "actor_id": actor_id,
                                    "runtime": runtime,
                                    "hint": "Set actor.command (or switch runner to headless).",
                                },
                            ),
                            False,
                        )
                    _ensure_mcp_installed(runtime, cwd)

                    if effective_runner == "headless":
                        headless_runner.SUPERVISOR.start_actor(
                            group_id=group.group_id,
                            actor_id=actor_id,
                            cwd=cwd,
                            env=dict(_inject_actor_context_env(env, group_id=group.group_id, actor_id=actor_id)),
                        )
                        try:
                            _write_headless_state(group.group_id, actor_id)
                        except Exception:
                            pass
                    else:
                        session = pty_runner.SUPERVISOR.start_actor(
                            group_id=group.group_id,
                            actor_id=actor_id,
                            cwd=cwd,
                            command=list(cmd or []),
                            env=_prepare_pty_env(_inject_actor_context_env(env, group_id=group.group_id, actor_id=actor_id)),
                            max_backlog_bytes=_pty_backlog_bytes(),
                        )
                        try:
                            _write_pty_state(group.group_id, actor_id, pid=session.pid)
                        except Exception:
                            pass

                    # Clear preamble/throttle state for a fresh start.
                    clear_preamble_sent(group, actor_id)
                    THROTTLE.reset_actor(group.group_id, actor_id, keep_pending=True)
            else:
                runner_kind = str(actor.get("runner") or "pty").strip() or "pty"
                effective_runner = _effective_runner_kind(runner_kind)
                if effective_runner == "headless":
                    headless_runner.SUPERVISOR.stop_actor(group_id=group.group_id, actor_id=actor_id)
                    _remove_headless_state(group.group_id, actor_id)
                    _remove_pty_state_if_pid(group.group_id, actor_id, pid=0)
                else:
                    pty_runner.SUPERVISOR.stop_actor(group_id=group.group_id, actor_id=actor_id)
                    _remove_pty_state_if_pid(group.group_id, actor_id, pid=0)
                    _remove_headless_state(group.group_id, actor_id)
                # Keep queued messages; they should be delivered when the actor is started again.
                THROTTLE.reset_actor(group.group_id, actor_id, keep_pending=True)
                # If no enabled actors remain, mark group as not running.
                try:
                    any_enabled = any(
                        bool(a.get("enabled", True))
                        for a in list_actors(group)
                        if isinstance(a, dict) and str(a.get("id") or "").strip()
                    )
                    if not any_enabled:
                        group.doc["running"] = False
                        group.save()
                except Exception:
                    pass
        if enabled_patched:
            _maybe_reset_automation_on_foreman_change(group, before_foreman_id=before_foreman_id)
        ev = append_event(
            group.ledger_path,
            kind="actor.update",
            group_id=group.group_id,
            scope_key="",
            by=by,
            data={"actor_id": actor_id, "patch": patch},
        )
        return DaemonResponse(ok=True, result={"actor": actor, "event": ev}), False

    if op == "actor_start":
        group_id = str(args.get("group_id") or "").strip()
        actor_id = str(args.get("actor_id") or "").strip()
        by = str(args.get("by") or "user").strip()
        if not group_id:
            return _error("missing_group_id", "missing group_id"), False
        group = load_group(group_id)
        if group is None:
            return _error("group_not_found", f"group not found: {group_id}"), False
        before_foreman_id = _foreman_id(group)
        try:
            require_actor_permission(group, by=by, action="actor.start", target_actor_id=actor_id)
            actor = update_actor(group, actor_id, {"enabled": True})
        except Exception as e:
            return _error("actor_start_failed", str(e)), False

        # Get actor configuration
        cmd = actor.get("command") if isinstance(actor.get("command"), list) else []
        env = actor.get("env") if isinstance(actor.get("env"), dict) else {}
        runner_kind = str(actor.get("runner") or "pty").strip()
        runtime = str(actor.get("runtime") or "codex").strip()
        forced_headless = _effective_runner_kind(runner_kind) == "headless" and runner_kind != "headless" and not _pty_supported()

        # Start the actor process using common function
        start_result = _start_actor_process(
            group,
            actor_id,
            command=list(cmd or []),
            env=dict(env or {}),
            runner=runner_kind,
            runtime=runtime,
            by=by,
        )

        if not start_result["success"]:
            return _error("actor_start_failed", start_result.get("error") or "unknown error"), False

        _maybe_reset_automation_on_foreman_change(group, before_foreman_id=before_foreman_id)

        result: Dict[str, Any] = {"actor": actor, "event": start_result["event"]}
        if forced_headless or start_result.get("effective_runner") != runner_kind:
            result["runner_effective"] = start_result.get("effective_runner") or "headless"
        return DaemonResponse(ok=True, result=result), False

    if op == "actor_stop":
        group_id = str(args.get("group_id") or "").strip()
        actor_id = str(args.get("actor_id") or "").strip()
        by = str(args.get("by") or "user").strip()
        if not group_id:
            return _error("missing_group_id", "missing group_id"), False
        group = load_group(group_id)
        if group is None:
            return _error("group_not_found", f"group not found: {group_id}"), False
        before_foreman_id = _foreman_id(group)
        try:
            require_actor_permission(group, by=by, action="actor.stop", target_actor_id=actor_id)
            actor = update_actor(group, actor_id, {"enabled": False})
            runner_kind = str(actor.get("runner") or "pty").strip()
            effective_runner = _effective_runner_kind(runner_kind)
            if effective_runner == "headless":
                headless_runner.SUPERVISOR.stop_actor(group_id=group.group_id, actor_id=actor_id)
                _remove_headless_state(group.group_id, actor_id)
                _remove_pty_state_if_pid(group.group_id, actor_id, pid=0)
            else:
                pty_runner.SUPERVISOR.stop_actor(group_id=group.group_id, actor_id=actor_id)
                _remove_pty_state_if_pid(group.group_id, actor_id, pid=0)
                _remove_headless_state(group.group_id, actor_id)
        except Exception as e:
            return _error("actor_stop_failed", str(e)), False
        try:
            any_enabled = any(
                bool(a.get("enabled", True))
                for a in list_actors(group)
                if isinstance(a, dict) and str(a.get("id") or "").strip()
            )
            if not any_enabled:
                group.doc["running"] = False
                group.save()
        except Exception:
            pass
        _maybe_reset_automation_on_foreman_change(group, before_foreman_id=before_foreman_id)
        ev = append_event(
            group.ledger_path,
            kind="actor.stop",
            group_id=group.group_id,
            scope_key="",
            by=by,
            data={"actor_id": actor_id},
        )
        return DaemonResponse(ok=True, result={"actor": actor, "event": ev}), False

    if op == "actor_restart":
        group_id = str(args.get("group_id") or "").strip()
        actor_id = str(args.get("actor_id") or "").strip()
        by = str(args.get("by") or "user").strip()
        if not group_id:
            return _error("missing_group_id", "missing group_id"), False
        group = load_group(group_id)
        if group is None:
            return _error("group_not_found", f"group not found: {group_id}"), False
        before_foreman_id = _foreman_id(group)
        try:
            require_actor_permission(group, by=by, action="actor.restart", target_actor_id=actor_id)
            actor = update_actor(group, actor_id, {"enabled": True})
            runner_kind = str(actor.get("runner") or "pty").strip()
            effective_runner = _effective_runner_kind(runner_kind)
            # Stop existing session
            if effective_runner == "headless":
                headless_runner.SUPERVISOR.stop_actor(group_id=group.group_id, actor_id=actor_id)
                _remove_headless_state(group.group_id, actor_id)
                _remove_pty_state_if_pid(group.group_id, actor_id, pid=0)
            else:
                pty_runner.SUPERVISOR.stop_actor(group_id=group.group_id, actor_id=actor_id)
                _remove_pty_state_if_pid(group.group_id, actor_id, pid=0)
                _remove_headless_state(group.group_id, actor_id)
            # Clear preamble state so system prompt will be re-injected on restart
            clear_preamble_sent(group, actor_id)
            # Reset delivery metadata but keep queued messages.
            THROTTLE.reset_actor(group.group_id, actor_id, keep_pending=True)
        except Exception as e:
            return _error("actor_restart_failed", str(e)), False
        if bool(group.doc.get("running", False)):
            group_scope_key = str(group.doc.get("active_scope_key") or "").strip()
            if not group_scope_key:
                return (
                    _error(
                        "missing_project_root",
                        "missing project root for group (no active scope)",
                        details={"hint": "Attach a project root first (e.g. cccc attach <path> --group <id>)"},
                    ),
                    False,
                )
            scope_key = str(actor.get("default_scope_key") or group_scope_key).strip()
            url = _find_scope_url(group, scope_key)
            if not url:
                return (
                    _error(
                        "scope_not_attached",
                        f"scope not attached: {scope_key}",
                        details={
                            "group_id": group.group_id,
                            "actor_id": actor_id,
                            "scope_key": scope_key,
                            "hint": "Attach this scope to the group (cccc attach <path> --group <id>)",
                        },
                    ),
                    False,
                )
            cwd = Path(url).expanduser().resolve()
            if not cwd.exists():
                return (
                    _error(
                        "invalid_project_root",
                        "project root path does not exist",
                        details={
                            "group_id": group.group_id,
                            "actor_id": actor_id,
                            "scope_key": scope_key,
                            "path": str(cwd),
                            "hint": "Re-attach a valid project root (cccc attach <path> --group <id>)",
                        },
                    ),
                    False,
                )
            cmd = actor.get("command") if isinstance(actor.get("command"), list) else []
            env = actor.get("env") if isinstance(actor.get("env"), dict) else {}
            runner_kind = str(actor.get("runner") or "pty").strip()
            effective_runner = _effective_runner_kind(runner_kind)

            if effective_runner == "headless":
                headless_runner.SUPERVISOR.start_actor(
                    group_id=group.group_id,
                    actor_id=actor_id,
                    cwd=cwd,
                    env=dict(_inject_actor_context_env(env, group_id=group.group_id, actor_id=actor_id)),
                )
                try:
                    _write_headless_state(group.group_id, actor_id)
                except Exception:
                    pass
            else:
                session = pty_runner.SUPERVISOR.start_actor(
                    group_id=group.group_id,
                    actor_id=actor_id,
                    cwd=cwd,
                    command=list(cmd or []),
                    env=_prepare_pty_env(_inject_actor_context_env(env, group_id=group.group_id, actor_id=actor_id)),
                    max_backlog_bytes=_pty_backlog_bytes(),
                )
                try:
                    _write_pty_state(group.group_id, actor_id, pid=session.pid)
                except Exception:
                    pass
                # NOTE: Do not inject the system prompt during restart (lazy preamble).
        _maybe_reset_automation_on_foreman_change(group, before_foreman_id=before_foreman_id)
        ev = append_event(
            group.ledger_path,
            kind="actor.restart",
            group_id=group.group_id,
            scope_key="",
            by=by,
            data={"actor_id": actor_id, "runner": str(actor.get("runner") or "pty")},
        )
        return DaemonResponse(ok=True, result={"actor": actor, "event": ev}), False

    if op == "term_resize":
        group_id = str(args.get("group_id") or "").strip()
        actor_id = str(args.get("actor_id") or "").strip()
        cols_raw = args.get("cols")
        rows_raw = args.get("rows")
        try:
            cols = int(cols_raw) if isinstance(cols_raw, int) else int(str(cols_raw or "0"))
        except Exception:
            cols = 0
        try:
            rows = int(rows_raw) if isinstance(rows_raw, int) else int(str(rows_raw or "0"))
        except Exception:
            rows = 0
        if not group_id:
            return _error("missing_group_id", "missing group_id"), False
        if not actor_id:
            return _error("missing_actor_id", "missing actor_id"), False
        group = load_group(group_id)
        if group is None:
            return _error("group_not_found", f"group not found: {group_id}"), False
        pty_runner.SUPERVISOR.resize(group_id=group_id, actor_id=actor_id, cols=cols, rows=rows)
        return DaemonResponse(ok=True, result={"group_id": group_id, "actor_id": actor_id, "cols": cols, "rows": rows}), False

    if op == "inbox_list":
        group_id = str(args.get("group_id") or "").strip()
        actor_id = str(args.get("actor_id") or "").strip()
        by = str(args.get("by") or "user").strip()
        limit_raw = args.get("limit")
        limit = int(limit_raw) if isinstance(limit_raw, int) else 50
        kind_filter = str(args.get("kind_filter") or "all").strip()
        if kind_filter not in ("all", "chat", "notify"):
            kind_filter = "all"
        if not group_id:
            return _error("missing_group_id", "missing group_id"), False
        if not actor_id:
            return _error("missing_actor_id", "missing actor_id"), False
        group = load_group(group_id)
        if group is None:
            return _error("group_not_found", f"group not found: {group_id}"), False
        try:
            require_inbox_permission(group, by=by, target_actor_id=actor_id)
        except Exception as e:
            return _error("permission_denied", str(e)), False
        msgs = unread_messages(group, actor_id=actor_id, limit=limit, kind_filter=kind_filter)  # type: ignore
        cur_event_id, cur_ts = get_cursor(group, actor_id)
        return DaemonResponse(ok=True, result={"messages": msgs, "cursor": {"event_id": cur_event_id, "ts": cur_ts}}), False

    if op == "inbox_mark_read":
        group_id = str(args.get("group_id") or "").strip()
        actor_id = str(args.get("actor_id") or "").strip()
        event_id = str(args.get("event_id") or "").strip()
        by = str(args.get("by") or "user").strip()
        if not group_id:
            return _error("missing_group_id", "missing group_id"), False
        if not actor_id:
            return _error("missing_actor_id", "missing actor_id"), False
        if not event_id:
            return _error("missing_event_id", "missing event_id"), False
        group = load_group(group_id)
        if group is None:
            return _error("group_not_found", f"group not found: {group_id}"), False
        try:
            require_inbox_permission(group, by=by, target_actor_id=actor_id)
        except Exception as e:
            return _error("permission_denied", str(e)), False
        ev = find_event(group, event_id)
        if ev is None:
            return _error("event_not_found", f"event not found: {event_id}"), False
        if str(ev.get("kind") or "") not in ("chat.message", "system.notify"):
            return _error("invalid_event_kind", "event kind must be chat.message or system.notify"), False
        if not is_message_for_actor(group, actor_id=actor_id, event=ev):
            return _error("event_not_for_actor", f"event is not addressed to actor: {actor_id}"), False
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
        ack_ev: Optional[dict[str, Any]] = None
        try:
            # Attention acknowledgements are explicit and independent of read cursors.
            # For actors, we treat "mark_read on an attention message" as the ACK gesture.
            if by == actor_id and str(ev.get("kind") or "") == "chat.message":
                data = ev.get("data")
                if isinstance(data, dict) and str(data.get("priority") or "normal").strip() == "attention":
                    sender = str(ev.get("by") or "").strip()
                    if sender and sender != actor_id and not has_chat_ack(group, event_id=event_id, actor_id=actor_id):
                        ack_ev = append_event(
                            group.ledger_path,
                            kind="chat.ack",
                            group_id=group.group_id,
                            scope_key="",
                            by=by,
                            data={"actor_id": actor_id, "event_id": event_id},
                        )
        except Exception:
            ack_ev = None
        return DaemonResponse(ok=True, result={"cursor": cursor, "event": read_ev, "ack_event": ack_ev}), False

    if op == "chat_ack":
        group_id = str(args.get("group_id") or "").strip()
        actor_id = str(args.get("actor_id") or "").strip()
        event_id = str(args.get("event_id") or "").strip()
        by = str(args.get("by") or "user").strip()

        if not group_id:
            return _error("missing_group_id", "missing group_id"), False
        if not actor_id:
            return _error("missing_actor_id", "missing actor_id"), False
        if not event_id:
            return _error("missing_event_id", "missing event_id"), False
        if not by:
            by = "user"

        # ACK is self-only (no acks on behalf of other recipients).
        if by != actor_id:
            return _error("permission_denied", "ack must be performed by the recipient (by must equal actor_id)"), False

        group = load_group(group_id)
        if group is None:
            return _error("group_not_found", f"group not found: {group_id}"), False

        if actor_id != "user":
            actor = find_actor(group, actor_id)
            if not isinstance(actor, dict):
                return _error("unknown_actor", f"unknown actor: {actor_id}"), False

        target = find_event(group, event_id)
        if target is None:
            return _error("event_not_found", f"event not found: {event_id}"), False
        if str(target.get("kind") or "") != "chat.message":
            return _error("invalid_event_kind", "event kind must be chat.message"), False

        sender = str(target.get("by") or "").strip()
        if sender and sender == actor_id:
            return _error("cannot_ack_own_message", "cannot acknowledge your own message"), False

        data = target.get("data")
        if not isinstance(data, dict):
            return _error("invalid_event_data", "invalid message data"), False
        if str(data.get("priority") or "normal").strip() != "attention":
            return _error("not_an_attention_message", "message priority is not attention"), False

        # Validate that the recipient is a target of the message.
        if actor_id == "user":
            to_raw = data.get("to")
            to_tokens = [str(x).strip() for x in to_raw] if isinstance(to_raw, list) else []
            to_set = {t for t in to_tokens if t}
            if "user" not in to_set and "@user" not in to_set:
                return _error("event_not_for_actor", "message is not addressed to user"), False
        else:
            # Ensure actor existed at the time of the message (avoid ack requirements for later-added actors).
            try:
                from ..util.time import parse_utc_iso

                msg_dt = parse_utc_iso(str(target.get("ts") or ""))
                actor = find_actor(group, actor_id)
                created_ts = str(actor.get("created_at") or "").strip() if isinstance(actor, dict) else ""
                created_dt = parse_utc_iso(created_ts) if created_ts else None
                if msg_dt is not None and created_dt is not None and created_dt > msg_dt:
                    return _error("event_not_for_actor", f"actor did not exist at message time: {actor_id}"), False
            except Exception:
                pass
            if not is_message_for_actor(group, actor_id=actor_id, event=target):
                return _error("event_not_for_actor", f"event is not addressed to actor: {actor_id}"), False

        if has_chat_ack(group, event_id=event_id, actor_id=actor_id):
            return DaemonResponse(ok=True, result={"acked": True, "already": True, "event": None}), False

        ack_ev = append_event(
            group.ledger_path,
            kind="chat.ack",
            group_id=group.group_id,
            scope_key="",
            by=by,
            data={"actor_id": actor_id, "event_id": event_id},
        )
        return DaemonResponse(ok=True, result={"acked": True, "already": False, "event": ack_ev}), False

    if op == "inbox_mark_all_read":
        group_id = str(args.get("group_id") or "").strip()
        actor_id = str(args.get("actor_id") or "").strip()
        by = str(args.get("by") or "user").strip()
        kind_filter = str(args.get("kind_filter") or "all").strip()
        if kind_filter not in ("all", "chat", "notify"):
            kind_filter = "all"
        if not group_id:
            return _error("missing_group_id", "missing group_id"), False
        if not actor_id:
            return _error("missing_actor_id", "missing actor_id"), False
        group = load_group(group_id)
        if group is None:
            return _error("group_not_found", f"group not found: {group_id}"), False
        try:
            require_inbox_permission(group, by=by, target_actor_id=actor_id)
        except Exception as e:
            return _error("permission_denied", str(e)), False

        last = latest_unread_event(group, actor_id=actor_id, kind_filter=kind_filter)  # type: ignore
        if last is None:
            cur_event_id, cur_ts = get_cursor(group, actor_id)
            return DaemonResponse(ok=True, result={"cursor": {"event_id": cur_event_id, "ts": cur_ts}, "event": None}), False

        event_id = str(last.get("id") or "").strip()
        ts = str(last.get("ts") or "").strip()
        if not event_id or not ts:
            cur_event_id, cur_ts = get_cursor(group, actor_id)
            return DaemonResponse(ok=True, result={"cursor": {"event_id": cur_event_id, "ts": cur_ts}, "event": None}), False

        cursor = set_cursor(group, actor_id, event_id=event_id, ts=ts)
        read_ev = append_event(
            group.ledger_path,
            kind="chat.read",
            group_id=group.group_id,
            scope_key="",
            by=by,
            data={"actor_id": actor_id, "event_id": event_id},
        )
        return DaemonResponse(ok=True, result={"cursor": cursor, "event": read_ev}), False

    if op == "ledger_snapshot":
        group_id = str(args.get("group_id") or "").strip()
        by = str(args.get("by") or "user").strip()
        reason = str(args.get("reason") or "manual").strip()
        if not group_id:
            return _error("missing_group_id", "missing group_id"), False
        group = load_group(group_id)
        if group is None:
            return _error("group_not_found", f"group not found: {group_id}"), False
        try:
            require_group_permission(group, by=by, action="group.update")
            snap = snapshot_ledger(group, reason=reason)
        except Exception as e:
            return _error("ledger_snapshot_failed", str(e)), False
        return DaemonResponse(ok=True, result={"snapshot": snap}), False

    if op == "ledger_compact":
        group_id = str(args.get("group_id") or "").strip()
        by = str(args.get("by") or "user").strip()
        reason = str(args.get("reason") or "auto").strip()
        force = bool(args.get("force", False))
        if not group_id:
            return _error("missing_group_id", "missing group_id"), False
        group = load_group(group_id)
        if group is None:
            return _error("group_not_found", f"group not found: {group_id}"), False
        try:
            require_group_permission(group, by=by, action="group.update")
            res = compact_ledger(group, reason=reason, force=force)
        except Exception as e:
            return _error("ledger_compact_failed", str(e)), False
        return DaemonResponse(ok=True, result=res), False

    if op == "send_cross_group":
        src_group_id = str(args.get("group_id") or "").strip()
        dst_group_id = str(args.get("dst_group_id") or "").strip()
        text = str(args.get("text") or "")
        by = str(args.get("by") or "user").strip() or "user"
        priority = str(args.get("priority") or "normal").strip() or "normal"
        to_raw = args.get("to")
        dst_to_tokens: list[str] = []
        if isinstance(to_raw, list):
            dst_to_tokens = [str(x).strip() for x in to_raw if isinstance(x, str) and str(x).strip()]

        attachments_raw = args.get("attachments")
        if attachments_raw:
            return _error("attachments_not_supported", "attachments are not supported for cross-group messages yet"), False

        if priority not in ("normal", "attention"):
            return _error("invalid_priority", "priority must be 'normal' or 'attention'"), False

        if not src_group_id:
            return _error("missing_group_id", "missing group_id"), False
        if not dst_group_id:
            return _error("missing_dst_group_id", "missing dst_group_id"), False
        if src_group_id == dst_group_id:
            return _error("invalid_dst_group_id", "dst_group_id must be different from group_id"), False

        src_group = load_group(src_group_id)
        if src_group is None:
            return _error("group_not_found", f"group not found: {src_group_id}"), False
        dst_group = load_group(dst_group_id)
        if dst_group is None:
            return _error("group_not_found", f"group not found: {dst_group_id}"), False

        # Canonicalize destination recipient tokens for stable display in the source message.
        dst_to_canon: list[str] = []
        if dst_to_tokens:
            try:
                dst_to_canon = resolve_recipient_tokens(dst_group, dst_to_tokens)
            except Exception as e:
                return _error("invalid_recipient", str(e)), False

        # 1) Write a source message into the origin group (not delivered to local actors).
        src_req = DaemonRequest(
            op="send",
            args={
                "group_id": src_group_id,
                "text": text,
                "by": by,
                "to": ["user"],
                "priority": priority,
                "dst_group_id": dst_group_id,
                "dst_to": dst_to_canon,
            },
        )
        src_resp, _ = handle_request(src_req)
        if not src_resp.ok:
            return src_resp, False

        src_event = src_resp.result.get("event")
        src_event_id = str((src_event or {}).get("id") or "").strip() if isinstance(src_event, dict) else ""
        if not src_event_id:
            return _error("send_failed", "missing source event id"), False

        # 2) Forward a message into the destination group with provenance.
        dst_req = DaemonRequest(
            op="send",
            args={
                "group_id": dst_group_id,
                "text": text,
                "by": by,
                "to": dst_to_canon,
                "priority": priority,
                "src_group_id": src_group_id,
                "src_event_id": src_event_id,
            },
        )
        dst_resp, _ = handle_request(dst_req)
        if not dst_resp.ok:
            return dst_resp, False

        return DaemonResponse(ok=True, result={"src_event": src_event, "dst_event": dst_resp.result.get("event")}), False

    if op == "send":
        group_id = str(args.get("group_id") or "").strip()
        text = str(args.get("text") or "")
        by = str(args.get("by") or "user").strip()
        priority = str(args.get("priority") or "normal").strip() or "normal"
        src_group_id = str(args.get("src_group_id") or "").strip()
        src_event_id = str(args.get("src_event_id") or "").strip()
        dst_group_id = str(args.get("dst_group_id") or "").strip()
        dst_to_raw = args.get("dst_to")
        dst_to: list[str] = []
        if isinstance(dst_to_raw, list):
            dst_to = [str(x).strip() for x in dst_to_raw if isinstance(x, str) and str(x).strip()]
        if (src_group_id and not src_event_id) or (src_event_id and not src_group_id):
            # Require both fields to treat this as a relay reference.
            src_group_id = ""
            src_event_id = ""
        to_raw = args.get("to")
        to_tokens: list[str] = []
        if isinstance(to_raw, list):
            to_tokens = [str(x).strip() for x in to_raw if isinstance(x, str) and str(x).strip()]

        if priority not in ("normal", "attention"):
            return _error("invalid_priority", "priority must be 'normal' or 'attention'"), False

        if not group_id:
            return _error("missing_group_id", "missing group_id"), False
        group = load_group(group_id)
        if group is None:
            return _error("group_not_found", f"group not found: {group_id}"), False

        # If group is idle, wake it on "human" messages (non-actor sender).
        # This keeps idle stable against agent chatter / throttled deliveries.
        try:
            from ..kernel.group import get_group_state, set_group_state
            if get_group_state(group) == "idle":
                is_actor_sender = isinstance(find_actor(group, by), dict)
                if by and by != "system" and not is_actor_sender:
                    group = set_group_state(group, state="active")
                    try:
                        AUTOMATION.on_resume(group)
                    except Exception:
                        pass
                    try:
                        THROTTLE.clear_pending_system_notifies(
                            group.group_id,
                            notify_kinds={"nudge", "keepalive", "help_nudge", "actor_idle", "silence_check", "standup"},
                        )
                    except Exception:
                        pass
        except Exception:
            pass

        try:
            to = resolve_recipient_tokens(group, to_tokens)
        except Exception as e:
            return _error("invalid_recipient", str(e)), False

        # Auto-extract @mentions from message text if no explicit recipients
        if not to:
            import re
            # Match @word patterns (but not @all, @peers, @foreman which are special)
            mention_pattern = re.compile(r'@(\w[\w-]*)')
            mentions = mention_pattern.findall(text)
            if mentions:
                # Filter to valid actor IDs
                actors = list_actors(group)
                actor_ids = {str(a.get("id") or "") for a in actors if isinstance(a, dict)}
                valid_mentions = [m for m in mentions if m in actor_ids or m in ("all", "peers", "foreman")]
                if valid_mentions:
                    # Convert to proper format
                    mention_tokens = []
                    for m in valid_mentions:
                        if m in ("all", "peers", "foreman"):
                            mention_tokens.append(f"@{m}")
                        else:
                            mention_tokens.append(m)
                    try:
                        to = resolve_recipient_tokens(group, mention_tokens)
                    except Exception:
                        pass  # Ignore invalid mentions

        # Apply group policy when no recipients are specified (after mention extraction).
        if not to:
            if get_default_send_to(group.doc) == "foreman":
                to = ["@foreman"]

        # Reject agent-targeted messages that match no enabled agents.
        if targets_any_agent(to):
            matched_enabled = enabled_recipient_actor_ids(group, to)
            if by and by in matched_enabled:
                matched_enabled = [aid for aid in matched_enabled if aid != by]
            if not matched_enabled:
                wanted = " ".join(to) if to else "@all"
                return (
                    _error(
                        "no_enabled_recipients",
                        f"no enabled agents match recipients: {wanted}",
                        details={"to": list(to)},
                    ),
                    False,
                )

        path = str(args.get("path") or "").strip()
        if path:
            scope = detect_scope(Path(path))
            scope_key = scope.scope_key
            scopes = group.doc.get("scopes")
            attached = False
            if isinstance(scopes, list):
                attached = any(isinstance(item, dict) and item.get("scope_key") == scope_key for item in scopes)
            if not attached:
                return (
                    _error(
                        "scope_not_attached",
                        f"scope not attached: {scope_key}",
                        details={"hint": "cccc attach <path> --group <id>"},
                    ),
                    False,
                )
        else:
            scope_key = str(group.doc.get("active_scope_key") or "").strip()
        if not scope_key:
            scope_key = ""

        try:
            attachments = _normalize_attachments(group, args.get("attachments"))
        except Exception as e:
            return _error("invalid_attachments", str(e)), False
        ev = append_event(
            group.ledger_path,
            kind="chat.message",
            group_id=group.group_id,
            scope_key=scope_key,
            by=by,
            data=ChatMessageData(
                text=text,
                format="plain",
                priority=priority,
                to=to,
                attachments=attachments,
                src_group_id=src_group_id or None,
                src_event_id=src_event_id or None,
                dst_group_id=dst_group_id or None,
                dst_to=dst_to if dst_group_id else None,
            ).model_dump(),
        )
        # Keep group ordering IM-like by bumping the group's last activity timestamp.
        try:
            reg = load_registry()
            meta = reg.groups.get(group.group_id)
            if isinstance(meta, dict):
                meta["updated_at"] = str(ev.get("ts") or utc_now_iso())
                reg.save()
        except Exception:
            pass

        # Best-effort delivery into running actors.
        # If no explicit recipients, deliver to all actors (@all behavior)
        effective_to = to if to else ["@all"]
        event_id = str(ev.get("id") or "").strip()
        event_ts = str(ev.get("ts") or "").strip()
        delivery_text = text
        prefix_lines: list[str] = []
        if priority == "attention" and event_id:
            prefix_lines.append(f"[cccc] IMPORTANT (event_id={event_id}):")
        if src_group_id and src_event_id:
            prefix_lines.append(f"[cccc] RELAYED FROM (group_id={src_group_id}, event_id={src_event_id}):")
        if prefix_lines:
            delivery_text = "\n".join(prefix_lines) + "\n" + delivery_text
        if attachments:
            lines = ["[cccc] Attachments:"]
            for a in attachments[:8]:
                title = str(a.get("title") or a.get("path") or "file").strip()
                b = int(a.get("bytes") or 0)
                p = str(a.get("path") or "").strip()
                lines.append(f"- {title} ({b} bytes) [{p}]")
            if len(attachments) > 8:
                lines.append(f"- … ({len(attachments) - 8} more)")
            delivery_text = (delivery_text.rstrip("\n") + "\n\n" + "\n".join(lines)).strip()
        actors = list_actors(group)
        logger.debug(f"[SEND] group={group_id} text={text[:30]!r} actors={[a.get('id') for a in actors]} effective_to={effective_to}")
        for actor in actors:
            if not isinstance(actor, dict):
                continue
            aid = str(actor.get("id") or "").strip()
            if not aid or aid == "user" or aid == by:
                logger.debug(f"[SEND] skip actor={aid} (user/by)")
                continue
            # Check if message is for this actor (handles @all, @peers, @foreman, etc.)
            ev_with_effective_to = dict(ev)
            ev_with_effective_to["data"] = dict(ev.get("data") or {})
            ev_with_effective_to["data"]["to"] = effective_to
            if not is_message_for_actor(group, actor_id=aid, event=ev_with_effective_to):
                logger.debug(f"[SEND] skip actor={aid} (not for actor)")
                continue
            # PTY runner: queue message for throttled delivery
            runner_kind = str(actor.get("runner") or "pty").strip()
            if _effective_runner_kind(runner_kind) == "pty":
                queue_chat_message(
                    group,
                    actor_id=aid,
                    event_id=event_id,
                    by=by,
                    to=effective_to,
                    text=delivery_text,
                    ts=event_ts,
                )
        # Headless runners: notify via system.notify event (daemon writes to ledger)
        try:
            ev_for_headless = dict(ev)
            ev_for_headless["data"] = dict(ev.get("data") or {})
            ev_for_headless["data"]["to"] = effective_to
            headless_targets = get_headless_targets_for_message(group, event=ev_for_headless, by=by)
            notify_title = "Important message" if priority == "attention" else "New message"
            notify_priority = "urgent" if priority == "attention" else "high"
            for aid in headless_targets:
                append_event(
                    group.ledger_path,
                    kind="system.notify",
                    group_id=group.group_id,
                    scope_key="",
                    by="system",
                    data={
                        "kind": "info",
                        "priority": notify_priority,
                        "title": notify_title,
                        "message": f"New message from {by}. Check your inbox.",
                        "target_actor_id": aid,
                        "requires_ack": False,
                        "context": {"event_id": event_id, "from": by},
                    },
                )
        except Exception:
            pass

        # Trigger automation: auto-transition idle -> active on new message
        try:
            AUTOMATION.on_new_message(group)
        except Exception:
            pass

        # Delivery is handled by the background tick. Do not flush synchronously here:
        # PTY writes can block and would stall the daemon request loop, freezing the UI.

        return DaemonResponse(ok=True, result={"event": ev}), False

    if op == "reply":
        group_id = str(args.get("group_id") or "").strip()
        text = str(args.get("text") or "")
        by = str(args.get("by") or "user").strip()
        reply_to = str(args.get("reply_to") or "").strip()
        priority = str(args.get("priority") or "normal").strip() or "normal"
        to_raw = args.get("to")
        to_tokens: list[str] = []
        if isinstance(to_raw, list):
            to_tokens = [str(x).strip() for x in to_raw if isinstance(x, str) and str(x).strip()]

        if priority not in ("normal", "attention"):
            return _error("invalid_priority", "priority must be 'normal' or 'attention'"), False

        if not group_id:
            return _error("missing_group_id", "missing group_id"), False
        if not reply_to:
            return _error("missing_reply_to", "missing reply_to event_id"), False

        group = load_group(group_id)
        if group is None:
            return _error("group_not_found", f"group not found: {group_id}"), False

        # If group is idle, wake it on "human" replies (non-actor sender).
        try:
            from ..kernel.group import get_group_state, set_group_state
            if get_group_state(group) == "idle":
                is_actor_sender = isinstance(find_actor(group, by), dict)
                if by and by != "system" and not is_actor_sender:
                    group = set_group_state(group, state="active")
                    try:
                        AUTOMATION.on_resume(group)
                    except Exception:
                        pass
                    try:
                        THROTTLE.clear_pending_system_notifies(
                            group.group_id,
                            notify_kinds={"nudge", "keepalive", "help_nudge", "actor_idle", "silence_check", "standup"},
                        )
                    except Exception:
                        pass
        except Exception:
            pass

        # Find the original message being replied to.
        original = find_event(group, reply_to)
        if original is None:
            return _error("event_not_found", f"event not found: {reply_to}"), False

        # Extract quote text.
        quote_text = get_quote_text(group, reply_to, max_len=100)

        if not to_tokens:
            to_tokens = default_reply_recipients(group, by=by, original_event=original)

        try:
            to = resolve_recipient_tokens(group, to_tokens)
        except Exception as e:
            return _error("invalid_recipient", str(e)), False

        # Reject agent-targeted messages that match no enabled agents.
        if targets_any_agent(to):
            matched_enabled = enabled_recipient_actor_ids(group, to)
            if by and by in matched_enabled:
                matched_enabled = [aid for aid in matched_enabled if aid != by]
            if not matched_enabled:
                wanted = " ".join(to) if to else "@all"
                return (
                    _error(
                        "no_enabled_recipients",
                        f"no enabled agents match recipients: {wanted}",
                        details={"to": list(to)},
                    ),
                    False,
                )

        scope_key = str(group.doc.get("active_scope_key") or "").strip()
        try:
            attachments = _normalize_attachments(group, args.get("attachments"))
        except Exception as e:
            return _error("invalid_attachments", str(e)), False
        ev = append_event(
            group.ledger_path,
            kind="chat.message",
            group_id=group.group_id,
            scope_key=scope_key,
            by=by,
            data=ChatMessageData(
                text=text,
                format="plain",
                priority=priority,
                to=to,
                reply_to=reply_to,
                quote_text=quote_text,
                attachments=attachments,
            ).model_dump(),
        )

        # Update group "last active" timestamp.
        try:
            reg = load_registry()
            meta = reg.groups.get(group.group_id)
            if isinstance(meta, dict):
                meta["updated_at"] = str(ev.get("ts") or utc_now_iso())
                reg.save()
        except Exception:
            pass

        # Best-effort delivery into running actors.
        # If no explicit recipients, deliver to all actors (@all behavior).
        effective_to = to if to else ["@all"]
        ev_with_effective_to = dict(ev)
        ev_with_effective_to["data"] = dict(ev.get("data") or {})
        ev_with_effective_to["data"]["to"] = effective_to

        event_id = str(ev.get("id") or "").strip()
        event_ts = str(ev.get("ts") or "").strip()
        delivery_text = text
        if priority == "attention" and event_id:
            delivery_text = f"[cccc] IMPORTANT (event_id={event_id}):\n" + delivery_text
        if attachments:
            lines = ["[cccc] Attachments:"]
            for a in attachments[:8]:
                title = str(a.get("title") or a.get("path") or "file").strip()
                b = int(a.get("bytes") or 0)
                p = str(a.get("path") or "").strip()
                lines.append(f"- {title} ({b} bytes) [{p}]")
            if len(attachments) > 8:
                lines.append(f"- … ({len(attachments) - 8} more)")
            delivery_text = (delivery_text.rstrip("\n") + "\n\n" + "\n".join(lines)).strip()

        for actor in list_actors(group):
            if not isinstance(actor, dict):
                continue
            aid = str(actor.get("id") or "").strip()
            if not aid or aid == "user" or aid == by:
                continue
            if not is_message_for_actor(group, actor_id=aid, event=ev_with_effective_to):
                continue
            # PTY runner: queue message for throttled delivery
            runner_kind = str(actor.get("runner") or "pty").strip()
            if _effective_runner_kind(runner_kind) == "pty":
                queue_chat_message(
                    group,
                    actor_id=aid,
                    event_id=event_id,
                    by=by,
                    to=effective_to,
                    text=delivery_text,
                    reply_to=reply_to,
                    quote_text=quote_text,
                    ts=event_ts,
                )

        # Headless runners: notify via system.notify event (daemon writes to ledger)
        try:
            headless_targets = get_headless_targets_for_message(group, event=ev_with_effective_to, by=by)
            notify_title = "Important message" if priority == "attention" else "New message"
            notify_priority = "urgent" if priority == "attention" else "high"
            for aid in headless_targets:
                append_event(
                    group.ledger_path,
                    kind="system.notify",
                    group_id=group.group_id,
                    scope_key="",
                    by="system",
                    data={
                        "kind": "info",
                        "priority": notify_priority,
                        "title": notify_title,
                        "message": f"New message from {by}. Check your inbox.",
                        "target_actor_id": aid,
                        "requires_ack": False,
                        "context": {"event_id": event_id, "from": by},
                    },
                )
        except Exception:
            pass

        # Trigger automation: auto-transition idle -> active on new message
        try:
            AUTOMATION.on_new_message(group)
        except Exception:
            pass

        # Delivery is handled by the background tick. Do not flush synchronously here:
        # PTY writes can block and would stall the daemon request loop, freezing the UI.

        return DaemonResponse(ok=True, result={"event": ev}), False

    # ==========================================================================
    # Context Operations (delegated to ops/context_ops.py)
    # ==========================================================================

    if op == "context_get":
        return handle_context_get(args), False

    if op == "context_sync":
        return handle_context_sync(args), False

    if op == "task_list":
        return handle_task_list(args), False

    if op == "presence_get":
        return handle_presence_get(args), False

    # ==========================================================================
    # Headless Runner Operations (delegated to ops/runner_ops.py)
    # ==========================================================================

    if op == "headless_status":
        return handle_headless_status(args), False

    if op == "headless_set_status":
        return handle_headless_set_status(args), False

    if op == "headless_ack_message":
        return handle_headless_ack_message(args), False

    # ==========================================================================
    # System Notification Operations
    # ==========================================================================

    if op == "system_notify":
        group_id = str(args.get("group_id") or "").strip()
        by = str(args.get("by") or "system").strip()
        kind = str(args.get("kind") or "info").strip()
        priority = str(args.get("priority") or "normal").strip()
        title = str(args.get("title") or "").strip()
        message = str(args.get("message") or "").strip()
        target_actor_id = str(args.get("target_actor_id") or "").strip() or None
        requires_ack = bool(args.get("requires_ack", False))
        context = args.get("context") if isinstance(args.get("context"), dict) else {}

        if not group_id:
            return _error("missing_group_id", "missing group_id"), False
        group = load_group(group_id)
        if group is None:
            return _error("group_not_found", f"group not found: {group_id}"), False

        # Validate kind and priority.
        valid_kinds = {"nudge", "keepalive", "help_nudge", "actor_idle", "silence_check", "standup", "status_change", "error", "info"}
        valid_priorities = {"low", "normal", "high", "urgent"}
        if kind not in valid_kinds:
            kind = "info"
        if priority not in valid_priorities:
            priority = "normal"

        ev = append_event(
            group.ledger_path,
            kind="system.notify",
            group_id=group.group_id,
            scope_key="",
            by=by,
            data={
                "kind": kind,
                "priority": priority,
                "title": title,
                "message": message,
                "target_actor_id": target_actor_id,
                "requires_ack": requires_ack,
                "context": context,
            },
        )

        # Best-effort PTY delivery (high/urgent priority only).
        if priority in ("high", "urgent"):
            event_id = str(ev.get("id") or "").strip()
            event_ts = str(ev.get("ts") or "").strip()
            for actor in list_actors(group):
                if not isinstance(actor, dict):
                    continue
                aid = str(actor.get("id") or "").strip()
                if not aid or aid == "user":
                    continue
                # Respect explicit target_actor_id if set.
                if target_actor_id and aid != target_actor_id:
                    continue
                # PTY runner only.
                runner_kind = str(actor.get("runner") or "pty").strip()
                if runner_kind != "pty":
                    continue
                # Queue for throttled delivery
                queue_system_notify(
                    group,
                    actor_id=aid,
                    event_id=event_id,
                    notify_kind=kind,
                    title=title,
                    message=message,
                    ts=event_ts,
                )

        return DaemonResponse(ok=True, result={"event": ev}), False

    if op == "notify_ack":
        group_id = str(args.get("group_id") or "").strip()
        actor_id = str(args.get("actor_id") or "").strip()
        notify_event_id = str(args.get("notify_event_id") or "").strip()
        by = str(args.get("by") or "user").strip()

        if not group_id:
            return _error("missing_group_id", "missing group_id"), False
        if not actor_id:
            return _error("missing_actor_id", "missing actor_id"), False
        if not notify_event_id:
            return _error("missing_notify_event_id", "missing notify_event_id"), False

        group = load_group(group_id)
        if group is None:
            return _error("group_not_found", f"group not found: {group_id}"), False

        # Validate the referenced notify event.
        notify_ev = find_event(group, notify_event_id)
        if notify_ev is None:
            return _error("event_not_found", f"event not found: {notify_event_id}"), False
        if str(notify_ev.get("kind") or "") != "system.notify":
            return _error("invalid_event_kind", "event is not a system.notify"), False

        ev = append_event(
            group.ledger_path,
            kind="system.notify_ack",
            group_id=group.group_id,
            scope_key="",
            by=by,
            data={
                "notify_event_id": notify_event_id,
                "actor_id": actor_id,
            },
        )

        return DaemonResponse(ok=True, result={"event": ev}), False

    return _error("unknown_op", f"unknown op: {op}"), False
def serve_forever(paths: Optional[DaemonPaths] = None) -> int:
    p = paths or default_paths()
    p.daemon_dir.mkdir(parents=True, exist_ok=True)

    # Apply global observability settings early (logging + developer mode gating).
    try:
        _apply_observability_settings(p.home, get_observability_settings())
    except Exception:
        pass

    _cleanup_stale_daemon_endpoints(p)
    if _is_daemon_alive(p):
        return 0

    # Cleanup stale IM bridge state from previous runs/crashes.
    try:
        res = _cleanup_invalid_im_bridges(p.home)
        if res.get("killed") or res.get("stale_pidfiles"):
            logger.info(
                "im_bridge_cleanup killed=%s stale_pidfiles=%s",
                res.get("killed"),
                res.get("stale_pidfiles"),
                extra={"op": "im_bridge_cleanup"},
            )
    except Exception:
        pass

    # Best-effort cleanup of orphaned PTY actor processes from a previous daemon crash.
    try:
        _cleanup_stale_pty_state(p.home)
    except Exception:
        pass

    def _on_session_exit(session: pty_runner.PtySession) -> None:
        _remove_pty_state_if_pid(session.group_id, session.actor_id, pid=session.pid)

    try:
        pty_runner.SUPERVISOR.set_exit_hook(_on_session_exit)
    except Exception:
        pass

    try:
        p.sock_path.unlink(missing_ok=True)
    except Exception:
        pass
    try:
        p.addr_path.unlink(missing_ok=True)
    except Exception:
        pass

    stop_event = threading.Event()

    # Best-effort: enable in-process event streaming for SDKs (daemon-owned only).
    try:
        from ..kernel.ledger import set_append_hook
        from .streaming import EVENT_BROADCASTER

        set_append_hook(EVENT_BROADCASTER.on_append)
    except Exception:
        pass

    # Graceful shutdown on SIGTERM/SIGINT
    def _signal_handler(signum: int, frame: Any) -> None:
        stop_event.set()

    signal.signal(signal.SIGTERM, _signal_handler)
    signal.signal(signal.SIGINT, _signal_handler)

    def _automation_loop() -> None:
        next_compact = 0.0
        while not stop_event.is_set():
            try:
                AUTOMATION.tick(home=p.home)
            except Exception:
                pass
            # Tick delivery for all groups with running actors (flush pending messages)
            try:
                base = p.home / "groups"
                if base.exists():
                    for gp in base.glob("*/group.yaml"):
                        gid = gp.parent.name
                        group = load_group(gid)
                        if group is None:
                            continue
                        # Check if any actor is running (instead of group.running)
                        group_is_running = (
                            pty_runner.SUPERVISOR.group_running(gid)
                            or headless_runner.SUPERVISOR.group_running(gid)
                        )
                        if not group_is_running:
                            continue
                        try:
                            tick_delivery(group)
                        except Exception:
                            pass
            except Exception:
                pass
            now = time.time()
            if now >= next_compact:
                next_compact = now + 60.0
                try:
                    _maybe_compact_ledgers(p.home)
                except Exception:
                    pass
            stop_event.wait(1.0)

    threading.Thread(target=_automation_loop, name="cccc-automation", daemon=True).start()

    transport = _desired_daemon_transport()
    if transport == "unix" and getattr(socket, "AF_UNIX", None) is None:
        transport = "tcp"

    if transport == "unix":
        af_unix = getattr(socket, "AF_UNIX", None)
        assert af_unix is not None
        s = socket.socket(af_unix, socket.SOCK_STREAM)
        endpoint = {"transport": "unix", "path": str(p.sock_path)}
        s.bind(str(p.sock_path))
    else:
        host = _daemon_tcp_bind_host()
        port = _daemon_tcp_port()
        # Avoid colliding with the default web port when auto-selecting a TCP port (port=0).
        # Some Windows installs configure the dynamic port range to include 8848, so a random
        # ephemeral port can occasionally grab the web port and prevent `cccc web` from starting.
        reserved_ports = {8848} if port == 0 else set()
        s = None
        endpoint = {"transport": "tcp", "host": host, "port": port}
        for _ in range(25):
            cand = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
            try:
                cand.bind((host, port))
                bound = cand.getsockname()
                try:
                    bound_port = int(bound[1])
                except Exception:
                    bound_port = 0
                if bound_port and bound_port in reserved_ports:
                    cand.close()
                    continue
                s = cand
                break
            except Exception:
                try:
                    cand.close()
                except Exception:
                    pass
                raise
        if s is None:
            s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
            s.bind((host, port))
        try:
            bound_host, bound_port = s.getsockname()[:2]
            endpoint["host"] = str(bound_host)
            endpoint["port"] = int(bound_port)
        except Exception:
            pass

    with s:
        s.listen(50)
        s.settimeout(1.0)  # Allow periodic check of stop_event
        _write_pid(p.pid_path)
        try:
            atomic_write_json(
                p.addr_path,
                {
                    "v": 1,
                    "transport": str(endpoint.get("transport") or ""),
                    "path": str(endpoint.get("path") or ""),
                    "host": str(endpoint.get("host") or ""),
                    "port": int(endpoint.get("port") or 0),
                    "pid": int(os.getpid()),
                    "version": str(__version__),
                    "ts": utc_now_iso(),
                },
            )
        except Exception:
            pass

        # Bootstrap background work only after the daemon socket is ready, but
        # don't block the accept loop (clients should see the daemon as responsive).
        def _bootstrap_after_listen() -> None:
            try:
                _maybe_autostart_running_groups()
            except Exception:
                pass
            try:
                _maybe_autostart_enabled_im_bridges()
            except Exception:
                pass

        threading.Thread(target=_bootstrap_after_listen, name="cccc-bootstrap", daemon=True).start()

        should_exit = False
        while not should_exit and not stop_event.is_set():
            try:
                conn, _ = s.accept()
            except socket.timeout:
                continue
            except KeyboardInterrupt:
                break
            except Exception:
                continue
            raw = _recv_json_line(conn)
            try:
                req = DaemonRequest.model_validate(raw)
            except Exception as e:
                resp = _error("invalid_request", "invalid request", details={"error": str(e)})
                try:
                    _send_json(conn, _dump_response(resp))
                finally:
                    try:
                        conn.close()
                    except Exception:
                        pass
                continue

            op = str(req.op or "").strip()
            if op == "term_attach":
                args = req.args or {}
                group_id = str(args.get("group_id") or "").strip()
                actor_id = str(args.get("actor_id") or "").strip()
                if not group_id:
                    resp = _error("missing_group_id", "missing group_id")
                elif not actor_id:
                    resp = _error("missing_actor_id", "missing actor_id")
                elif not pty_runner.SUPERVISOR.actor_running(group_id, actor_id):
                    resp = _error("actor_not_running", "actor is not running")
                else:
                    resp = DaemonResponse(ok=True, result={"group_id": group_id, "actor_id": actor_id})
                try:
                    _send_json(conn, _dump_response(resp))
                    if resp.ok:
                        pty_runner.SUPERVISOR.attach(group_id=group_id, actor_id=actor_id, sock=conn)
                        continue
                except Exception:
                    pass
                try:
                    conn.close()
                except Exception:
                    pass
                continue

            if op == "events_stream":
                args = req.args or {}
                group_id = str(args.get("group_id") or "").strip()
                by = str(args.get("by") or "user").strip() or "user"
                since_event_id = str(args.get("since_event_id") or "").strip()
                since_ts = str(args.get("since_ts") or "").strip()
                kinds_raw = args.get("kinds")
                kinds: Optional[set[str]] = None
                kinds_invalid = False
                if isinstance(kinds_raw, list):
                    try:
                        from .streaming import STREAMABLE_KINDS_V1

                        items = {str(x).strip() for x in kinds_raw if isinstance(x, str) and str(x).strip()}
                        items = {k for k in items if k in STREAMABLE_KINDS_V1}
                        if items:
                            kinds = items
                        elif items == set() and any(isinstance(x, str) and str(x).strip() for x in kinds_raw):
                            kinds_invalid = True
                        else:
                            kinds = None
                    except Exception:
                        kinds = None

                if not group_id:
                    resp = _error("missing_group_id", "missing group_id")
                elif kinds_invalid:
                    try:
                        from .streaming import STREAMABLE_KINDS_V1

                        resp = _error(
                            "invalid_kinds",
                            "no supported kinds requested",
                            details={"supported": sorted(STREAMABLE_KINDS_V1)},
                        )
                    except Exception:
                        resp = _error("invalid_kinds", "no supported kinds requested")
                else:
                    group = load_group(group_id)
                    if group is None:
                        resp = _error("group_not_found", f"group not found: {group_id}")
                    else:
                        resp = DaemonResponse(ok=True, result={"group_id": group_id})

                try:
                    _send_json(conn, _dump_response(resp))
                    if resp.ok:
                        try:
                            from .streaming import stream_events_to_socket

                            threading.Thread(
                                target=stream_events_to_socket,
                                kwargs={
                                    "sock": conn,
                                    "group_id": group_id,
                                    "by": by,
                                    "kinds": kinds,
                                    "since_event_id": since_event_id,
                                    "since_ts": since_ts,
                                },
                                daemon=True,
                                name=f"cccc-events-{group_id[:8]}",
                            ).start()
                            continue
                        except Exception:
                            pass
                except Exception:
                    pass
                try:
                    conn.close()
                except Exception:
                    pass
                continue

            try:
                resp, should_exit = handle_request(req)
                if should_exit:
                    stop_event.set()
                try:
                    _send_json(conn, _dump_response(resp))
                except (BrokenPipeError, ConnectionResetError, OSError):
                    # Client disconnected before response was sent - not an error
                    pass
            except Exception as e:
                # Catch any unexpected errors in handle_request to prevent daemon crash
                logger.exception("Unexpected error in handle_request: %s", e)
                try:
                    error_resp = DaemonResponse(
                        ok=False,
                        error=DaemonError(
                            code="internal_error",
                            message=f"internal error: {type(e).__name__}: {e}",
                        ),
                    )
                    _send_json(conn, _dump_response(error_resp))
                except Exception:
                    pass
            finally:
                try:
                    conn.close()
                except Exception:
                    pass

    stop_event.set()
    
    # Graceful shutdown: stop all running actors
    try:
        _stop_all_im_bridges(p.home)
    except Exception:
        pass
    try:
        pty_runner.SUPERVISOR.stop_all()
    except Exception:
        pass
    try:
        headless_runner.SUPERVISOR.stop_all()
    except Exception:
        pass
    
    try:
        if p.sock_path.exists():
            p.sock_path.unlink()
    except Exception:
        pass
    try:
        if p.addr_path.exists():
            p.addr_path.unlink()
    except Exception:
        pass
    try:
        if p.pid_path.exists():
            p.pid_path.unlink()
    except Exception:
        pass
    return 0


def call_daemon(req: Dict[str, Any], *, paths: Optional[DaemonPaths] = None, timeout_s: float = 60.0) -> Dict[str, Any]:
    p = paths or default_paths()
    try:
        request = DaemonRequest.model_validate(req)
    except Exception as e:
        return DaemonResponse(
            ok=False,
            error=DaemonError(code="invalid_request", message="invalid request", details={"error": str(e)}),
        ).model_dump()
    try:
        ep = get_daemon_endpoint(p)
        transport = str(ep.get("transport") or "").strip().lower()

        if transport == "tcp":
            host = str(ep.get("host") or "127.0.0.1").strip() or "127.0.0.1"
            try:
                port = int(ep.get("port") or 0)
            except Exception:
                port = 0
            if port <= 0:
                raise RuntimeError("invalid tcp daemon endpoint")
            s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
            try:
                s.settimeout(timeout_s)
                s.connect((host, port))
                s.sendall((json.dumps(request.model_dump(), ensure_ascii=False) + "\n").encode("utf-8"))
                with s.makefile("rb") as f:
                    line = f.readline(4_000_000)  # 4MB limit to prevent DoS
            finally:
                try:
                    s.close()
                except Exception:
                    pass

        else:
            af_unix = getattr(socket, "AF_UNIX", None)
            if af_unix is None:
                raise RuntimeError("AF_UNIX not supported")
            path = str(ep.get("path") or p.sock_path)
            s = socket.socket(af_unix, socket.SOCK_STREAM)
            try:
                s.settimeout(timeout_s)
                s.connect(path)
                s.sendall((json.dumps(request.model_dump(), ensure_ascii=False) + "\n").encode("utf-8"))
                with s.makefile("rb") as f:
                    line = f.readline(4_000_000)  # 4MB limit to prevent DoS
            finally:
                try:
                    s.close()
                except Exception:
                    pass
        obj = json.loads(line.decode("utf-8", errors="replace"))
        resp = DaemonResponse.model_validate(obj)
        return resp.model_dump()
    except Exception:
        return DaemonResponse(ok=False, error=DaemonError(code="daemon_unavailable", message="daemon unavailable")).model_dump()


def read_pid(paths: Optional[DaemonPaths] = None) -> int:
    p = paths or default_paths()
    try:
        txt = p.pid_path.read_text(encoding="utf-8").strip()
        return int(txt) if txt.isdigit() else 0
    except Exception:
        return 0
