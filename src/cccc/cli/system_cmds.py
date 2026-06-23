from __future__ import annotations

"""System/daemon/web/mcp CLI command handlers."""

import json
from importlib.metadata import PackageNotFoundError, distribution

from .common import *  # noqa: F401,F403

__all__ = [
    "cmd_version",
    "cmd_status",
    "cmd_doctor",
    "cmd_web",
    "cmd_mcp",
    "cmd_setup",
    "cmd_update",
    "cmd_daemon",
]

_UPDATE_PACKAGE_NAME = "cccc-pair"
_RC_INDEX_URL = "https://test.pypi.org/simple/"
_STABLE_CHANNEL = "stable"
_RC_CHANNEL = "rc"
_KNOWN_UPDATE_CHANNELS = {_STABLE_CHANNEL, _RC_CHANNEL}
_BLOCKED_INSTALL_KINDS = {"editable", "local_path"}


def _find_installed_distribution() -> Any:
    """Return the installed CCCC distribution, preferring the published package name."""
    for dist_name in (_UPDATE_PACKAGE_NAME, "cccc"):
        try:
            return distribution(dist_name)
        except PackageNotFoundError:
            continue
    raise PackageNotFoundError(_UPDATE_PACKAGE_NAME)


def _read_direct_url_payload(dist: Any) -> dict[str, Any]:
    """Parse direct_url metadata when present to classify install source."""
    raw = str(dist.read_text("direct_url.json") or "").strip()
    if not raw:
        return {}
    try:
        payload = json.loads(raw)
    except json.JSONDecodeError:
        return {}
    return payload if isinstance(payload, dict) else {}


def _distribution_name(dist: Any) -> str:
    """Return a human-readable distribution name from metadata when available."""
    metadata = getattr(dist, "metadata", None)
    if metadata is not None:
        try:
            value = str(metadata.get("Name") or "").strip()
            if value:
                return value
        except Exception:
            pass
    return str(getattr(dist, "name", "") or _UPDATE_PACKAGE_NAME)


def _detect_install_kind(dist: Any) -> tuple[str, dict[str, Any]]:
    """Classify the current installation without guessing beyond trusted metadata."""
    direct_url = _read_direct_url_payload(dist)
    if direct_url:
        dir_info = direct_url.get("dir_info")
        if isinstance(dir_info, dict) and bool(dir_info.get("editable")):
            return "editable", direct_url
        url = str(direct_url.get("url") or "").strip().lower()
        if url.startswith("file:") or url.startswith("/"):
            return "local_path", direct_url
    return "standard", direct_url


def _detect_update_channel(dist: Any, direct_url: dict[str, Any]) -> str:
    """Infer the current release channel when metadata contains a reliable hint."""
    version_text = str(getattr(dist, "version", "") or "").strip().lower()
    if any(marker in version_text for marker in ("a", "b", "rc", "dev")):
        return _RC_CHANNEL

    archive_info = direct_url.get("archive_info")
    if isinstance(archive_info, dict):
        indexes = archive_info.get("index_urls")
        if isinstance(indexes, list):
            normalized = {str(item or "").strip().rstrip("/") for item in indexes}
            if _RC_INDEX_URL.rstrip("/") in normalized:
                return _RC_CHANNEL
    return _STABLE_CHANNEL


def _build_update_command(channel: str) -> list[str]:
    """Build the exact pip invocation for the selected release channel."""
    command = [sys.executable, "-m", "pip", "install", "-U"]
    if channel == _RC_CHANNEL:
        command.extend(
            [
                "--pre",
                "--index-url",
                _RC_INDEX_URL,
                "--extra-index-url",
                "https://pypi.org/simple/",
            ]
        )
    command.append(_UPDATE_PACKAGE_NAME)
    return command


def _recommendation_for_install_kind(install_kind: str) -> str:
    """Return a concrete follow-up command when auto-update is intentionally blocked."""
    if install_kind == "editable":
        return "python -m pip install -e ."
    return f"python -m pip install -U {_UPDATE_PACKAGE_NAME}"


def _blocked_update_message(install_kind: str) -> str:
    """Explain clearly why auto-update is blocked for this install source."""
    if install_kind == "editable":
        return "editable installs are not updated automatically by `cccc update`"
    if install_kind == "local_path":
        return "local path installs are not updated automatically by `cccc update`"
    return "this install source is not updated automatically by `cccc update`"


def _command_text(command: list[str]) -> str:
    """Render the exact subprocess command for JSON output."""
    return " ".join(shlex.quote(part) for part in command)


def _inspect_update_target(args: argparse.Namespace) -> tuple[dict[str, Any], list[str]]:
    """Resolve current installation metadata and the command we would execute."""
    requested_channel = str(getattr(args, "channel", "") or "").strip().lower()
    if requested_channel and requested_channel not in _KNOWN_UPDATE_CHANNELS:
        raise ValueError(f"invalid channel: {requested_channel}")

    dist = _find_installed_distribution()
    install_kind, direct_url = _detect_install_kind(dist)
    detected_channel = _detect_update_channel(dist, direct_url)
    effective_channel = requested_channel or detected_channel or _STABLE_CHANNEL

    result = {
        "distribution_name": _distribution_name(dist),
        "version": str(getattr(dist, "version", "") or ""),
        "channel": effective_channel,
        "detected_channel": detected_channel,
        "install_kind": install_kind,
        "direct_url_present": bool(direct_url),
    }
    return result, _build_update_command(effective_channel)


def _build_update_result(inspection: dict[str, Any], completed: Any) -> dict[str, Any]:
    """Convert subprocess output into the structured JSON result shape."""
    return {
        **inspection,
        "before_version": inspection.get("version"),
        "stdout": completed.stdout,
        "stderr": completed.stderr,
        "exit_code": int(completed.returncode),
    }


def cmd_update(args: argparse.Namespace) -> int:
    """Upgrade the current CCCC installation in-place via the active Python runtime."""
    try:
        inspection, command = _inspect_update_target(args)
    except ValueError as e:
        _print_json({"ok": False, "error": {"code": "invalid_channel", "message": str(e)}})
        return 2
    except PackageNotFoundError:
        _print_json(
            {
                "ok": False,
                "error": {
                    "code": "distribution_not_found",
                    "message": "CCCC is not installed in the current Python environment",
                },
            }
        )
        return 1

    inspection["command"] = _command_text(command)

    if bool(getattr(args, "check", False)):
        _print_json({"ok": True, "result": inspection})
        return 0

    install_kind = str(inspection.get("install_kind") or "").strip()
    if install_kind in _BLOCKED_INSTALL_KINDS:
        _print_json(
            {
                "ok": False,
                "error": {
                    "code": "editable_install_not_supported",
                    "message": _blocked_update_message(install_kind),
                },
                "result": {
                    **inspection,
                    "recommendation": _recommendation_for_install_kind(install_kind),
                },
            }
        )
        return 1

    try:
        completed = subprocess.run(
            command,
            capture_output=True,
            text=True,
            check=False,
        )
    except Exception as e:
        _print_json(
            {
                "ok": False,
                "error": {
                    "code": "update_failed",
                    "message": str(e),
                },
                "result": inspection,
            }
        )
        return 1

    result = _build_update_result(inspection, completed)

    if completed.returncode == 0:
        try:
            refreshed = _find_installed_distribution()
            result["after_version"] = str(getattr(refreshed, "version", "") or "")
        except PackageNotFoundError:
            pass
        _print_json({"ok": True, "result": result})
        return 0

    _print_json(
        {
            "ok": False,
            "error": {
                "code": "update_failed",
                "message": "pip install command failed",
            },
            "result": result,
        }
    )
    return 1

def cmd_version(_: argparse.Namespace) -> int:
    print(__version__)
    return 0

def cmd_status(_: argparse.Namespace) -> int:
    """Show overall CCCC status: daemon, groups, actors."""
    from ..kernel.runtime import detect_all_runtimes
    
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

def cmd_doctor(args: argparse.Namespace) -> int:
    """Check environment and show available agent runtimes."""
    from ..kernel.runtime import detect_all_runtimes
    from ..runners.platform_support import pty_support_details
    
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

    pty_diag = pty_support_details()
    if sys.platform.startswith("win"):
        print()
        if bool(pty_diag.get("supported")):
            print("Windows PTY: OK (ConPTY backend available)")
        else:
            print(f"Windows PTY: NOT READY ({pty_diag.get('code')})")
            print(f"  {pty_diag.get('message')}")
            for hint in pty_diag.get("hints") if isinstance(pty_diag.get("hints"), list) else []:
                print(f"  - {hint}")

    print()
    print("Agent Runtimes:")
    
    # Check all runtimes
    all_runtimes = args.all if hasattr(args, 'all') else False
    runtimes = detect_all_runtimes(primary_only=not all_runtimes)

    encoding = str(getattr(sys.stdout, "encoding", "") or "").lower()
    ascii_only = sys.platform.startswith("win") and encoding not in ("utf-8", "utf8")

    available_count = 0
    for rt in runtimes:
        status = "OK" if rt.available else "NOT FOUND"
        mark = "[OK]" if rt.available else "[NO]" if ascii_only else ("✓" if rt.available else "✗")
        path_info = f" ({rt.path})" if rt.available else ""
        print(f"  {mark} {rt.name}: {status}{path_info}")
        if rt.available:
            available_count += 1
    
    print()
    if available_count == 0:
        print("No agent runtimes detected.")
        print(
            "First-class supported runtimes: claude, codex, copilot, cursor, devin, kiro, kilo, "
            "antigravity, droid, amp, auggie, grok, hermes, kimi, opencode"
        )
        print("Manual fallback: custom (bring your own command and MCP wiring)")
    else:
        print(f"{available_count} runtime(s) available.")
        print()
        print("Quick start:")
        print(f"  cccc setup --runtime {runtimes[0].name if runtimes[0].available else 'claude'}")
        print("  cccc attach .")
        print("  cccc actor add my-agent --runtime <name>")
        print("  cccc")
    
    return 0

def cmd_web(args: argparse.Namespace) -> int:
    from ..ports.web.main import main as web_main

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
    from ..ports.mcp.main import main as mcp_main

    return int(mcp_main())

def cmd_setup(args: argparse.Namespace) -> int:
    """Setup CCCC MCP for agent runtimes (configure MCP, print guidance)."""
    from ..daemon.mcp_install import build_mcp_add_command, ensure_mcp_installed, is_mcp_installed
    from ..kernel.runtime import build_prompt_assisted_mcp_setup_contract, detect_runtime, get_cccc_mcp_stdio_command

    runtime = str(args.runtime or "").strip()
    project_path = Path(args.path or ".").resolve()

    # Supported runtimes
    # - claude/codex/copilot/devin/kiro/droid/amp/auggie/grok/hermes/kimi: MCP setup can be automated via their CLIs
    # - opencode: MCP setup is injected into the actor process through OPENCODE_CONFIG_CONTENT
    # - antigravity/cursor/kilo: MCP setup is prompt-assisted inside the runtime agent; CCCC does not edit their config
    # - custom: user-provided runtime; MCP setup is manual (generic guidance only)
    SUPPORTED_RUNTIMES = [
        "claude",
        "codex",
        "copilot",
        "cursor",
        "devin",
        "kiro",
        "kilo",
        "antigravity",
        "droid",
        "amp",
        "auggie",
        "grok",
        "hermes",
        "kimi",
        "opencode",
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

    cccc_cmd = get_cccc_mcp_stdio_command()
    prompt_assisted_runtimes = ("antigravity", "cursor", "kilo")
    auto_mcp_runtimes = tuple(name for name in SUPPORTED_RUNTIMES if name not in {"custom", *prompt_assisted_runtimes})

    def _cmd_line(parts: list[str]) -> str:
        return " ".join(shlex.quote(p) for p in parts)

    def _manual_setup(rt: str, *, runtime_available: bool) -> None:
        cmd = build_mcp_add_command(rt) or cccc_cmd
        display_cmd = cmd
        if runtime_available:
            try:
                display_cmd = resolve_subprocess_argv(cmd)
            except FileNotFoundError:
                display_cmd = cmd
        results["mcp"][rt] = {"mode": "manual", "command": _cmd_line(display_cmd)}
        if runtime_available:
            results["notes"].append(f"{rt}: MCP CLI failed; run the command shown in result.mcp.{rt}.command")
        else:
            results["notes"].append(f"{rt}: CLI not found; run the command shown in result.mcp.{rt}.command")

    def _auto_setup(rt: str) -> None:
        if rt == "opencode":
            runtime_info = detect_runtime(rt)
            results["mcp"][rt] = {
                "mode": "auto",
                "status": "runtime_env",
                "hint": "CCCC injects the OpenCode MCP config through OPENCODE_CONFIG_CONTENT when each actor starts; no global OpenCode config is modified.",
            }
            if not runtime_info.available:
                results["notes"].append("opencode: CLI not found; install OpenCode before starting an opencode actor")
            return
        runtime_info = detect_runtime(rt)
        was_ready = is_mcp_installed(rt, cwd=project_path) if runtime_info.available else False
        if ensure_mcp_installed(rt, project_path, auto_mcp_runtimes=auto_mcp_runtimes):
            results["mcp"][rt] = {"mode": "auto", "status": "present" if was_ready else "added"}
            return
        _manual_setup(rt, runtime_available=runtime_info.available)

    # Runtime-specific setup
    runtimes_to_setup = [runtime] if runtime else SUPPORTED_RUNTIMES

    for rt in runtimes_to_setup:
        if rt in auto_mcp_runtimes:
            _auto_setup(rt)

        elif rt in prompt_assisted_runtimes:
            runtime_info = detect_runtime(rt)
            results["mcp"][rt] = {
                "mode": "prompt_assisted",
                "status": "runtime_prompt",
                "contract": build_prompt_assisted_mcp_setup_contract(),
                "hint": "CCCC includes this idempotent MCP setup request in the actor startup prompt; the runtime agent owns installation and validation.",
            }
            if not runtime_info.available:
                results["notes"].append(f"{rt}: CLI not found; install the runtime before starting this actor")

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
