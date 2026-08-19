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
    DEEPSEEK_LLM_ADAPTER_PACKAGE,
    DEEPSEEK_LLM_ADAPTER_VERSION,
    DEEPSEEK_MCP_CLIENT_PACKAGE,
    DEEPSEEK_MCP_CLIENT_VERSION,
    DEEPSEEK_NODE_RANGE,
    DEEPSEEK_PACKAGE_VERSIONS,
    DEEPSEEK_RELEASE_VERSION,
    is_canonical_profile_manifest,
)

def _package_version(package: str, *, env: Optional[Dict[str, str]] = None) -> str:
    """Read an installed package manifest without mutating the user's profile."""
    environ = dict(os.environ)
    environ.update(env or {})
    for name in (
        "CCCC_DEEPSEEK_ACP_VERSION",
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
        "    persistenceRoot: !!js process.env.CCCC_DEEPSEEK_SESSION_ROOT",
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
    command = lines[14]
    if not command.startswith("    command: '") or not command.endswith("'"):
        return False
    command_path = command[len("    command: '") : -1].replace("''", "'")
    return _deepseek_executable(command_path, env) is not None


def deepseek_lockfile_is_pinned(dsh_home: Path) -> bool:
    """Require every installed DeepSeek Harness package to use one release."""
    try:
        payload = json.loads((Path(dsh_home) / "package-lock.json").read_text(encoding="utf-8"))
    except (OSError, ValueError, TypeError):
        return False
    packages = payload.get("packages") if isinstance(payload, dict) else None
    if not isinstance(packages, dict):
        return False
    root = packages.get("")
    if not isinstance(root, dict) or not _deepseek_dependencies_are_exact(root.get("dependencies")):
        return False
    matched = False
    for lock_path, entry in packages.items():
        tail = str(lock_path).rsplit("node_modules/", 1)[-1]
        if tail != "@deepseek-ai/dsh" and not tail.startswith("@deepseek-ai/dsh-"):
            continue
        matched = True
        if not isinstance(entry, dict) or entry.get("version") != DEEPSEEK_RELEASE_VERSION:
            return False
    return matched


def canonical_deepseek_runtime_manifest() -> dict[str, object]:
    return {
        "name": "cccc-deepseek-runtime",
        "private": True,
        "ccccManaged": True,
        "dependencies": dict(DEEPSEEK_PACKAGE_VERSIONS),
    }


def is_canonical_deepseek_runtime_manifest(value: object) -> bool:
    return (
        isinstance(value, dict)
        and value.get("name") == "cccc-deepseek-runtime"
        and value.get("private") is True
        and value.get("ccccManaged") is True
        and _deepseek_dependencies_are_exact(value.get("dependencies"))
    )


def _deepseek_dependencies_are_exact(value: object) -> bool:
    return isinstance(value, dict) and value == dict(DEEPSEEK_PACKAGE_VERSIONS)


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
    node's reported version, and packages under CCCC_HOME's managed runtime.
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
    try:
        runtime_manifest = json.loads((dsh_home / "package.json").read_text(encoding="utf-8"))
    except (OSError, ValueError, TypeError):
        return "setup_required: DeepSeek managed runtime manifest is missing or invalid"
    if not is_canonical_deepseek_runtime_manifest(runtime_manifest):
        return "setup_required: DeepSeek managed runtime dependency set is not canonical"
    if not deepseek_lockfile_is_pinned(dsh_home):
        return f"setup_required: DeepSeek dependency graph must be pinned to {DEEPSEEK_RELEASE_VERSION}"
    profile = dsh_home / "profiles" / "cccc-acp"
    manifest_path = profile / "package.json"
    try:
        manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    except (OSError, ValueError, TypeError):
        return "setup_required: deepseek profile cccc-acp is not configured; run cccc setup --runtime deepseek"
    if not is_canonical_profile_manifest(manifest):
        return "setup_required: deepseek profile cccc-acp is unmanaged or invalid"
    try:
        config_text = (profile / "cordis.yml").read_text(encoding="utf-8")
    except OSError:
        return "setup_required: deepseek profile cccc-acp config is missing"
    if not _is_canonical_deepseek_config(config_text, env=env):
        return "setup_required: deepseek profile cccc-acp config is invalid"
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
    return _deepseek_node_error(env)


def deepseek_bootstrap_preflight_error(*, env: Optional[Dict[str, str]] = None) -> str:
    """Check only prerequisites that CCCC cannot install on first use."""
    node_error = _deepseek_node_error(env)
    if node_error:
        return node_error
    environ = dict(os.environ)
    environ.update(env or {})
    if not shutil.which("npm", path=environ.get("PATH")):
        return "setup_required: npm is required to install DeepSeek Harness"
    return ""


def _deepseek_node_error(env: Optional[Dict[str, str]] = None) -> str:
    node = _node_version(env)
    if not ((node[0] == 22 and node >= (22, 19, 0)) or node[0] >= 24):
        return f"setup_required: DeepSeek Harness requires Node {DEEPSEEK_NODE_RANGE} (found {'.'.join(map(str, node))})"
    return ""


def resolve_deepseek_home(env: Optional[Dict[str, str]] = None) -> Optional[Path]:
    environ = dict(os.environ)
    environ.update(env or {})
    cccc_root = str(environ.get("CCCC_HOME") or "").strip()
    if not cccc_root:
        base = str(environ.get("HOME") or environ.get("USERPROFILE") or "").strip()
        if not base:
            return None
        cccc_root = str(Path(base).expanduser() / ".cccc")
    return Path(cccc_root).expanduser() / "runtimes" / "deepseek" / DEEPSEEK_RELEASE_VERSION
