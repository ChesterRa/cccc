from __future__ import annotations

"""System/daemon/web/mcp CLI command handlers."""

from .common import *  # noqa: F401,F403

__all__ = [
    "cmd_version",
    "cmd_status",
    "cmd_doctor",
    "cmd_web",
    "cmd_mcp",
    "cmd_setup",
    "cmd_daemon",
]

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
    import shutil
    from ..kernel.runtime import detect_all_runtimes, PRIMARY_RUNTIMES
    
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
