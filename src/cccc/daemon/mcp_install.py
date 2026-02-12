"""Runtime MCP installation helpers."""

from __future__ import annotations

import json
import subprocess
from pathlib import Path

from ..util.conv import coerce_bool
from ..util.fs import read_json


def is_mcp_installed(runtime: str) -> bool:
    try:
        if runtime == "claude":
            result = subprocess.run(
                ["claude", "mcp", "get", "cccc"],
                capture_output=True,
                timeout=10,
            )
            return result.returncode == 0

        if runtime == "codex":
            result = subprocess.run(
                ["codex", "mcp", "get", "cccc"],
                capture_output=True,
                timeout=10,
            )
            return result.returncode == 0

        if runtime == "droid":
            for cfg_path in (
                Path.home() / ".factory" / "mcp.json",
                Path.home() / ".config" / "droid" / "mcp.json",
                Path.home() / ".droid" / "mcp.json",
            ):
                cfg = read_json(cfg_path)
                servers = cfg.get("mcpServers") if isinstance(cfg, dict) else None
                if not isinstance(servers, dict):
                    continue
                entry = servers.get("cccc")
                if isinstance(entry, dict):
                    return not coerce_bool(entry.get("disabled"), default=False)
            return False

        if runtime == "amp":
            settings_path = Path.home() / ".config" / "amp" / "settings.json"
            if not settings_path.exists():
                return False
            doc = json.loads(settings_path.read_text(encoding="utf-8") or "{}")
            if not isinstance(doc, dict):
                return False
            servers = doc.get("amp.mcpServers")
            return isinstance(servers, dict) and "cccc" in servers

        if runtime == "auggie":
            settings_path = Path.home() / ".augment" / "settings.json"
            if not settings_path.exists():
                return False
            doc = json.loads(settings_path.read_text(encoding="utf-8") or "{}")
            if not isinstance(doc, dict):
                return False
            servers = doc.get("mcpServers")
            return isinstance(servers, dict) and "cccc" in servers

        if runtime == "neovate":
            config_path = Path.home() / ".neovate" / "config.json"
            if not config_path.exists():
                return False
            doc = json.loads(config_path.read_text(encoding="utf-8") or "{}")
            if not isinstance(doc, dict):
                return False
            servers = doc.get("mcpServers")
            return isinstance(servers, dict) and "cccc" in servers

        if runtime == "gemini":
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


def ensure_mcp_installed(runtime: str, cwd: Path, *, auto_mcp_runtimes: tuple[str, ...]) -> bool:
    if runtime not in auto_mcp_runtimes:
        return True
    if is_mcp_installed(runtime):
        return True
    try:
        if runtime == "claude":
            result = subprocess.run(
                ["claude", "mcp", "add", "-s", "user", "cccc", "--", "cccc", "mcp"],
                capture_output=True,
                text=True,
                cwd=str(cwd),
                timeout=30,
            )
            return result.returncode == 0

        if runtime == "codex":
            result = subprocess.run(
                ["codex", "mcp", "add", "cccc", "--", "cccc", "mcp"],
                capture_output=True,
                text=True,
                cwd=str(cwd),
                timeout=30,
            )
            return result.returncode == 0

        if runtime == "droid":
            result = subprocess.run(
                ["droid", "mcp", "add", "--type", "stdio", "cccc", "cccc", "mcp"],
                capture_output=True,
                text=True,
                cwd=str(cwd),
                timeout=30,
            )
            return result.returncode == 0

        if runtime == "amp":
            result = subprocess.run(
                ["amp", "mcp", "add", "cccc", "cccc", "mcp"],
                capture_output=True,
                text=True,
                cwd=str(cwd),
                timeout=30,
            )
            return result.returncode == 0

        if runtime == "auggie":
            result = subprocess.run(
                ["auggie", "mcp", "add", "cccc", "--", "cccc", "mcp"],
                capture_output=True,
                text=True,
                cwd=str(cwd),
                timeout=30,
            )
            return result.returncode == 0

        if runtime == "neovate":
            result = subprocess.run(
                ["neovate", "mcp", "add", "-g", "cccc", "cccc", "mcp"],
                capture_output=True,
                text=True,
                cwd=str(cwd),
                timeout=30,
            )
            return result.returncode == 0

        if runtime == "gemini":
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
