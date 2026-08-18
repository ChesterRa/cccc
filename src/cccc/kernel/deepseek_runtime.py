"""DeepSeek runtime discovery and readiness checks."""
from __future__ import annotations

import json
import os
import re
import shutil
import subprocess
from pathlib import Path
from typing import Dict, List, Optional, Tuple

from ..contracts.v1.deepseek import (
    DEEPSEEK_ACP_APP_PACKAGE,
    DEEPSEEK_ACP_APP_VERSION,
    DEEPSEEK_ACP_PACKAGE,
    DEEPSEEK_ACP_VERSION,
    DEEPSEEK_DSH_PACKAGE,
    DEEPSEEK_DSH_VERSION,
    DEEPSEEK_LLM_ADAPTER_PACKAGE,
    DEEPSEEK_LLM_ADAPTER_VERSION,
    DEEPSEEK_MCP_CLIENT_PACKAGE,
    DEEPSEEK_MCP_CLIENT_VERSION,
    DEEPSEEK_NODE_RANGE,
    DEEPSEEK_PACKAGE_VERSIONS,
    is_canonical_profile_manifest,
)

def _package_version(package: str, *, env: Optional[Dict[str, str]] = None) -> str:
    """Read an installed package manifest without mutating the user's profile."""
    environ = dict(os.environ)
    environ.update(env or {})
    for name in (
        "CCCC_DEEPSEEK_ACP_VERSION",
        "CCCC_DEEPSEEK_DSH_VERSION",
        "CCCC_DEEPSEEK_MCP_CLIENT_VERSION",
    ):
        environ.pop(name, None)
    dsh_home = resolve_deepseek_home(environ)
    if dsh_home is None:
        return ""
    manifest = dsh_home / "node_modules" / package / "package.json"
    try:
        payload = json.loads(manifest.read_text(encoding="utf-8"))
    except (OSError, ValueError, TypeError):
        return ""
    if isinstance(payload, dict):
        version = str(payload.get("version") or "").strip()
        if version:
            return version
    return ""


def _deepseek_executable(command: str, env: Optional[Dict[str, str]] = None) -> Optional[str]:
    environ = dict(os.environ)
    environ.update(env or {})
    raw = str(command or "").strip()
    if not raw:
        return None
    resolved = shutil.which(raw, path=environ.get("PATH"))
    if resolved:
        return resolved
    candidate = Path(raw)
    return str(candidate) if _is_platform_executable(candidate, environ) else None


def _is_platform_executable(path: Path, environ: Optional[Dict[str, str]] = None) -> bool:
    if not path.is_file():
        return False
    if os.name != "nt":
        return os.access(path, os.X_OK)
    values = dict(os.environ)
    values.update(environ or {})
    pathext = {
        item.strip().upper()
        for item in str(values.get("PATHEXT") or ".COM;.EXE;.BAT;.CMD").split(";")
        if item.strip()
    }
    return path.suffix.upper() in pathext


def _is_canonical_deepseek_patch(
    patch_text: str, *, env: Optional[Dict[str, str]] = None
) -> bool:
    """Validate the exact CCCC ACP/MCP patch shape we generate.

    A substring check would allow comments or arbitrary text to masquerade as
    a managed profile.  The line grammar is deliberately small and strict so
    it remains byte-compatible with the Rust preflight without requiring a
    YAML implementation in both runtimes.
    """
    lines = patch_text.splitlines()
    if len(lines) != 15 or any(not line or line.lstrip().startswith("#") for line in lines):
        return False
    expected = [
        "- insert:",
        "    - id: acp",
        "      name: '@deepseek-ai/dsh-acp'",
        "    - id: cccc-mcp",
        "      name: '@deepseek-ai/dsh-mcp-client'",
        "      config:",
        "        transport: stdio",
        "        serverName: cccc",
        None,
        "        args: [mcp]",
        "        env:",
        "          CCCC_HOME: !!js process.env.CCCC_HOME",
        "          CCCC_GROUP_ID: !!js process.env.CCCC_GROUP_ID",
        "          CCCC_ACTOR_ID: !!js process.env.CCCC_ACTOR_ID",
        "        failOnStartupError: true",
    ]
    if any(actual != wanted for actual, wanted in zip(lines, expected) if wanted is not None):
        return False
    command = lines[8]
    if not command.startswith("        command: '") or not command.endswith("'"):
        return False
    command_path = command[len("        command: '") : -1].replace("''", "'")
    return _deepseek_executable(command_path, env) is not None


def _is_canonical_deepseek_config(
    config_text: str, *, env: Optional[Dict[str, str]] = None
) -> bool:
    """Validate the exact ACP app composition consumed at runtime."""
    lines = str(config_text or "").splitlines()
    expected = [
        "- id: llm-deepseek",
        "  name: '@deepseek-ai/dsh-llm-deepseek'",
        "- id: acp-demo",
        "  name: '@deepseek-ai/dsh-acp-demo'",
        "  config:",
        "    provider: deepseek-official",
        "    model: deepseek-v4-flash",
        "    workspaceContext: false",
        "- id: cccc-mcp",
        "  name: '@deepseek-ai/dsh-mcp-client'",
        "  config:",
        "    transport: stdio",
        "    serverName: cccc",
        None,
        "    args: [mcp]",
        "    env:",
        "      CCCC_HOME: !!js process.env.CCCC_HOME",
        "      CCCC_GROUP_ID: !!js process.env.CCCC_GROUP_ID",
        "      CCCC_ACTOR_ID: !!js process.env.CCCC_ACTOR_ID",
        "    failOnStartupError: true",
    ]
    if len(lines) != len(expected):
        return False
    if any(actual != wanted for actual, wanted in zip(lines, expected) if wanted is not None):
        return False
    command = lines[13]
    if not command.startswith("    command: '") or not command.endswith("'"):
        return False
    command_path = command[len("    command: '") : -1].replace("''", "'")
    return _deepseek_executable(command_path, env) is not None


def _node_version(env: Optional[Dict[str, str]] = None) -> Tuple[int, int, int]:
    environ = dict(os.environ)
    environ.update(env or {})
    environ.pop("CCCC_NODE_VERSION", None)
    try:
        result = subprocess.run(
            ["node", "--version"],
            check=False,
            capture_output=True,
            text=True,
            timeout=2,
            env=environ,
        )
        raw = result.stdout.strip() if result.returncode == 0 else ""
    except (OSError, subprocess.SubprocessError):
        raw = ""
    match = re.search(r"(\d+)\.(\d+)\.(\d+)", raw)
    return tuple(int(match.group(index)) for index in range(1, 4)) if match else (0, 0, 0)


def deepseek_preflight_error(
    command: Optional[List[str]] = None,
    *,
    runner: str = "headless",
    env: Optional[Dict[str, str]] = None,
) -> str:
    """Fail closed until the exact DSH + ACP preview tuple is installed.

    This function is deliberately side-effect free: it only probes PATH,
    node's reported version, and package manifests under the supplied DSH_HOME.
    """
    if str(runner or "").strip() != "headless":
        return "setup_required: deepseek runtime requires the headless runner"
    external_error = deepseek_external_preflight_error(command, env=env)
    if external_error:
        return external_error
    environ = dict(os.environ)
    environ.update(env or {})
    dsh_home = resolve_deepseek_home(environ)
    if dsh_home is None:
        return "setup_required: DSH_HOME is not configured"
    for package, expected in DEEPSEEK_PACKAGE_VERSIONS:
        found = _package_version(package, env=env)
        if found != expected:
            return f"setup_required: {package}@{expected} required (found {found})"
    profile = dsh_home / "profiles" / "cccc-acp"
    manifest_path = profile / "package.json"
    patch_path = profile / "cordis.patch.yml"
    try:
        manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    except (OSError, ValueError, TypeError):
        return "setup_required: deepseek profile cccc-acp is not configured; run cccc setup --runtime deepseek"
    if not is_canonical_profile_manifest(manifest):
        return "setup_required: deepseek profile cccc-acp is unmanaged or invalid"
    try:
        patch_text = patch_path.read_text(encoding="utf-8")
    except OSError:
        return "setup_required: deepseek profile cccc-acp patch is missing"
    if not _is_canonical_deepseek_patch(patch_text, env=env):
        return "setup_required: deepseek profile cccc-acp has no effective ACP/CCCC MCP rows"
    try:
        config_text = (profile / "cordis.yml").read_text(encoding="utf-8")
    except OSError:
        return "setup_required: deepseek profile cccc-acp config is missing"
    if not _is_canonical_deepseek_config(config_text, env=env):
        return "setup_required: deepseek profile cccc-acp config is invalid"
    home_patch = dsh_home / "cordis.patch.yml"
    try:
        override = home_patch.read_text(encoding="utf-8")
    except OSError:
        override = ""
    if override and any(token in override.lower() for token in ("dsh-acp", "session/request_permission", "servername: cccc")):
        return "setup_required: DSH_HOME cordis.patch.yml overrides the CCCC ACP/MCP composition"
    return ""


def deepseek_external_preflight_error(
    command: Optional[List[str]] = None,
    *,
    env: Optional[Dict[str, str]] = None,
) -> str:
    cmd = [str(part).strip() for part in (command or ["dsh-acp-demo"]) if str(part).strip()]
    executable = cmd[0] if cmd else "dsh"
    if not _deepseek_executable(executable, env):
        return f"setup_required: deepseek executable not found: {executable}"
    node = _node_version(env)
    if not ((node[0] == 22 and node >= (22, 19, 0)) or node[0] >= 24):
        return f"setup_required: DeepSeek Harness requires Node {DEEPSEEK_NODE_RANGE} (found {'.'.join(map(str, node))})"
    return ""


def resolve_deepseek_home(env: Optional[Dict[str, str]] = None) -> Optional[Path]:
    environ = dict(os.environ)
    environ.update(env or {})
    configured = str(environ.get("DSH_HOME") or "").strip()
    if configured:
        return Path(configured).expanduser()
    base = str(environ.get("HOME") or environ.get("USERPROFILE") or "").strip()
    return Path(base).expanduser() / ".dsh" if base else None
