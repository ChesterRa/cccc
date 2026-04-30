"""Runtime detection and configuration for agent CLIs."""
from __future__ import annotations

import shutil
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Dict, List, Optional

from ..util.process import find_subprocess_executable


@dataclass
class RuntimeInfo:
    """Information about an agent runtime."""
    name: str
    display_name: str
    command: str
    available: bool
    path: Optional[str]
    capabilities: str
    mcp_add_command: Optional[List[str]]  # Command to add MCP server, None if manual config required
    # From runtime pool (if configured)
    priority: int = 999
    scenarios: List[str] = None  # type: ignore
    
    def __post_init__(self) -> None:
        if self.scenarios is None:
            self.scenarios = []


# Known agent runtimes with their configurations
KNOWN_RUNTIMES: Dict[str, Dict[str, Any]] = {
    "amp": {
        "display_name": "Amp",
        "command": "amp",
        "capabilities": "MCP; MCP setup: auto",
        "mcp_add_pattern": "amp mcp add {name} {cmd}",
    },
    "auggie": {
        "display_name": "Auggie (Augment)",
        "command": "auggie",
        "capabilities": "MCP; MCP setup: auto",
        "mcp_add_pattern": "auggie mcp add {name} -- {cmd}",
    },
    "claude": {
        "display_name": "Claude Code",
        "command": "claude",
        "capabilities": "MCP; MCP setup: auto",
        "mcp_add_pattern": "claude mcp add -s user {name} -- {cmd}",
    },
    "codex": {
        "display_name": "Codex CLI",
        "command": "codex",
        "capabilities": "MCP; MCP setup: auto",
        "mcp_add_pattern": "codex mcp add {name} -- {cmd}",
    },
    "droid": {
        "display_name": "Droid CLI",
        "command": "droid",
        "capabilities": "MCP; MCP setup: auto",
        "mcp_add_pattern": "droid mcp add --type stdio {name} {cmd}",
    },
    "gemini": {
        "display_name": "Gemini CLI",
        "command": "gemini",
        "capabilities": "MCP; MCP setup: auto",
        "mcp_add_pattern": "gemini mcp add -s user {name} {cmd}",
    },
    "kimi": {
        "display_name": "Kimi CLI",
        "command": "kimi",
        "capabilities": "MCP; MCP setup: auto",
        "mcp_add_pattern": "kimi mcp add --transport stdio {name} -- {cmd}",
    },
    "neovate": {
        "display_name": "Neovate Code",
        "command": "neovate",
        "capabilities": "MCP; MCP setup: auto",
        "mcp_add_pattern": "neovate mcp add -g {name} {cmd}",
    },
    "custom": {
        "display_name": "Custom Runtime",
        "command": "custom",
        "capabilities": "MCP; MCP setup: manual",
        "mcp_add_pattern": None,
    },
    "web_model": {
        "display_name": "Browser Web Model",
        "command": "",
        "capabilities": "Remote MCP connector; pull-based turns; no local process",
        "mcp_add_pattern": None,
    },
}

# First-class supported runtimes (CCCC manages startup defaults + MCP wiring)
PRIMARY_RUNTIMES = ["claude", "codex", "droid", "amp", "auggie", "neovate", "gemini", "kimi", "web_model"]
def detect_runtime(name: str) -> RuntimeInfo:
    """Detect if a specific runtime is available on the system."""
    config = KNOWN_RUNTIMES.get(name)
    if not config:
        return RuntimeInfo(
            name=name,
            display_name=name,
            command=name,
            available=False,
            path=None,
            capabilities="Unknown runtime",
            mcp_add_command=None,
        )
    
    # Custom is "supported", but not auto-detectable as installed (user provides the actual command line).
    if name == "custom":
        return RuntimeInfo(
            name=name,
            display_name=config["display_name"],
            command=config["command"],
            available=False,
            path=None,
            capabilities=config["capabilities"],
            mcp_add_command=None,
        )
    if name == "web_model":
        return RuntimeInfo(
            name=name,
            display_name=config["display_name"],
            command=config["command"],
            available=True,
            path=None,
            capabilities=config["capabilities"],
            mcp_add_command=None,
        )

    command = config["command"]
    path = find_subprocess_executable(command)
    available = path is not None
    
    mcp_add_command = None
    if config.get("mcp_add_pattern") and available:
        # Parse the pattern into a command list
        # This is just for reference; actual execution happens elsewhere
        mcp_add_command = config["mcp_add_pattern"].split()
    
    return RuntimeInfo(
        name=name,
        display_name=config["display_name"],
        command=command,
        available=available,
        path=path,
        capabilities=config["capabilities"],
        mcp_add_command=mcp_add_command,
    )


def detect_all_runtimes(primary_only: bool = True) -> List[RuntimeInfo]:
    """Detect all known runtimes on the system.
    
    Args:
        primary_only: If True, only check first-class runtimes (claude, codex, droid, amp, auggie, neovate, gemini, kimi).
                     If False, check all configured runtimes (including custom).
    
    Returns:
        List of RuntimeInfo for each runtime.
    """
    names = PRIMARY_RUNTIMES if primary_only else list(KNOWN_RUNTIMES.keys())
    return [detect_runtime(name) for name in names]


def get_available_runtimes(primary_only: bool = True) -> List[RuntimeInfo]:
    """Get only the runtimes that are available on the system."""
    return [r for r in detect_all_runtimes(primary_only) if r.available]


def get_runtime_command(name: str) -> List[str]:
    """Get the default command for a runtime.
    
    Returns the command as a list suitable for subprocess.
    """
    if name in {"custom", "web_model"}:
        return []
    config = KNOWN_RUNTIMES.get(name)
    if not config:
        return [name]
    return [config["command"]]


def get_cccc_mcp_stdio_command() -> List[str]:
    """Return the most stable command line for launching `cccc mcp`.

    Prefer an absolute path to the installed `cccc` entrypoint when available.
    On Windows this avoids relying on runtime-specific PATH inheritance for MCP
    child processes. Fall back to the current Python interpreter otherwise.
    """
    candidates: List[Path] = []
    is_windows = sys.platform.startswith("win")
    try:
        bin_dir = Path(sys.executable).resolve().parent
        names = ["cccc.exe", "cccc.cmd", "cccc.bat", "cccc", "cccc-script.py"] if is_windows else ["cccc"]
        for name in names:
            candidate = bin_dir / name
            if candidate.exists():
                candidates.append(candidate)
    except Exception:
        pass
    for raw in (shutil.which("cccc"), shutil.which("cccc.exe") if is_windows else None):
        if raw:
            candidates.append(Path(raw))
    seen: set[str] = set()
    for candidate in candidates:
        try:
            resolved = str(candidate.resolve())
        except Exception:
            resolved = str(candidate)
        if not resolved or resolved in seen:
            continue
        seen.add(resolved)
        return [resolved, "mcp"]
    return [sys.executable, "-m", "cccc.ports.mcp.main"]


def get_runtime_command_with_flags(name: str) -> List[str]:
    """Get the command with recommended flags for autonomous operation."""
    commands = {
        "amp": ["amp"],
        "auggie": ["auggie"],
        "claude": ["claude", "--dangerously-skip-permissions"],
        # Codex spawns MCP servers as subprocesses; ensure it inherits actor env (CCCC_GROUP_ID/CCCC_ACTOR_ID)
        # so MCP tools can resolve "self" context reliably.
        "codex": ["codex", "-c", "shell_environment_policy.inherit=all", "--dangerously-bypass-approvals-and-sandbox", "--search"],
        "droid": ["droid", "--auto", "high"],
        "gemini": ["gemini", "--yolo"],
        "kimi": ["kimi", "--yolo"],
        "neovate": ["neovate"],
        "custom": [],
        "web_model": [],
    }
    return commands.get(name, [name])


def runtime_start_preflight_error(runtime: str, command: Optional[List[str]] = None, *, runner: str = "pty") -> str:
    """Return a user-facing startup error when a runtime cannot be launched.

    This is intentionally stricter than schema validation:
    - It checks whether the executable actually exists on this machine.
    - It keeps headless actors out of the check because they do not spawn a CLI process.
    """
    runner_kind = str(runner or "pty").strip() or "pty"
    if runner_kind == "headless":
        return ""

    rt = str(runtime or "").strip()
    cmd = [str(part) for part in (command or []) if str(part).strip()]

    if rt == "web_model":
        if runner_kind != "headless":
            return "web_model runtime uses a remote MCP connector and must use the headless runner"
        return ""

    if rt == "custom":
        if not cmd:
            return "custom runtime requires a command (PTY runner)"
        executable = str(cmd[0] or "").strip()
        if find_subprocess_executable(executable):
            return ""
        return f"runtime unavailable: custom runtime command not found: {executable}"

    info = detect_runtime(rt)
    if info.available:
        return ""

    command_hint = list(cmd or get_runtime_command_with_flags(rt))
    executable = str(command_hint[0] or "").strip() if command_hint else ""
    if executable and find_subprocess_executable(executable):
        return ""

    label = str(info.display_name or rt or "runtime").strip() or "runtime"
    if executable and executable != rt:
        return f"runtime unavailable: {label} executable not found: {executable}"
    return f"runtime unavailable: {label} is not installed or not in PATH"
