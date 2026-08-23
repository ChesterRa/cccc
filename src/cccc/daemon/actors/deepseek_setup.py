"""Idempotent first-use setup for the managed DeepSeek ACP profile."""
from __future__ import annotations

import json
import os
import shutil
import signal
import subprocess
import sys
import tempfile
from dataclasses import dataclass
from pathlib import Path
from typing import Callable, Dict

from ...contracts.v1.deepseek import (
    DEEPSEEK_ACP_PACKAGE,
    DEEPSEEK_ACP_APP_PACKAGE,
    DEEPSEEK_ACP_APP_VERSION,
    DEEPSEEK_ACP_VERSION,
    DEEPSEEK_LLM_ADAPTER_PACKAGE,
    DEEPSEEK_LLM_ADAPTER_VERSION,
    DEEPSEEK_MAX_OUTPUT_TOKENS,
    DEEPSEEK_MCP_CLIENT_PACKAGE,
    DEEPSEEK_MCP_CLIENT_VERSION,
    DEEPSEEK_NPM_BEFORE,
)
from ...kernel.deepseek_runtime import (
    canonical_deepseek_runtime_manifest,
    deepseek_lockfile_is_pinned,
    is_canonical_deepseek_runtime_manifest,
)
from ...kernel.runtime import (
    deepseek_external_preflight_error,
    deepseek_preflight_error,
)
from ...util.file_lock import acquire_lockfile, release_lockfile
from ...util.fs import atomic_write_json, atomic_write_text
from .deepseek_setup_env import prepare_deepseek_setup_env

_INSTALL_TIMEOUT_SECONDS = 300.0
_PACKAGES = (
    (DEEPSEEK_ACP_PACKAGE, DEEPSEEK_ACP_VERSION),
    (DEEPSEEK_MCP_CLIENT_PACKAGE, DEEPSEEK_MCP_CLIENT_VERSION),
    (DEEPSEEK_ACP_APP_PACKAGE, DEEPSEEK_ACP_APP_VERSION),
    (DEEPSEEK_LLM_ADAPTER_PACKAGE, DEEPSEEK_LLM_ADAPTER_VERSION),
)


@dataclass(frozen=True)
class DeepSeekSetupOutcome:
    dsh_home: Path
    profile: Path
    packages_installed: bool
    profile_created: bool


def ensure_deepseek_setup(
    env: Dict[str, str],
    *,
    installer: Callable[[Path, Dict[str, str]], None] | None = None,
    external_preflight: Callable[..., str] = deepseek_external_preflight_error,
    ready_preflight: Callable[..., str] = deepseek_preflight_error,
) -> DeepSeekSetupOutcome:
    effective_env, dsh_home = prepare_deepseek_setup_env(env)
    command = ["dsh-acp-demo"]
    error = external_preflight(command, env=effective_env)
    # The ACP app itself is one of the packages CCCC installs.  Permit the
    # first-use path to reach npm when only that executable is missing, while
    # still failing closed for a missing Node/runtime prerequisite.
    if error and "deepseek executable not found" not in str(error).lower():
        raise RuntimeError(error)
    profile = dsh_home / "profiles" / "cccc-acp"
    if not ready_preflight(command, runner="headless", env=effective_env):
        return DeepSeekSetupOutcome(dsh_home, profile, False, False)
    dsh_home.mkdir(parents=True, exist_ok=True)
    lock = acquire_lockfile(dsh_home / "cccc-acp.setup.lock", blocking=True)
    try:
        if not ready_preflight(command, runner="headless", env=effective_env):
            return DeepSeekSetupOutcome(dsh_home, profile, False, False)
        packages_installed = False
        if not _packages_ready(dsh_home):
            (installer or _install_packages)(dsh_home, effective_env)
            if not _packages_ready(dsh_home):
                raise RuntimeError("DeepSeek packages remain incomplete after automatic installation")
            packages_installed = True
        executable = _resolve_cccc_executable(effective_env)
        profile_created = _ensure_profile(dsh_home, executable)
        error = ready_preflight(command, runner="headless", env=effective_env)
        if error:
            raise RuntimeError(error)
        return DeepSeekSetupOutcome(dsh_home, profile, packages_installed, profile_created)
    finally:
        release_lockfile(lock)


def _packages_ready(dsh_home: Path) -> bool:
    for package, version in _PACKAGES:
        manifest = dsh_home / "node_modules" / package / "package.json"
        try:
            found = json.loads(manifest.read_text(encoding="utf-8")).get("version")
        except (OSError, ValueError, TypeError):
            return False
        if found != version:
            return False
    try:
        manifest = json.loads((dsh_home / "package.json").read_text(encoding="utf-8"))
    except (OSError, ValueError, TypeError):
        return False
    return is_canonical_deepseek_runtime_manifest(manifest) and deepseek_lockfile_is_pinned(dsh_home)


def _install_packages(dsh_home: Path, env: Dict[str, str]) -> None:
    npm = shutil.which("npm", path=env.get("PATH"))
    if not npm:
        raise RuntimeError("npm is required to install DeepSeek ACP/MCP packages")
    atomic_write_json(dsh_home / "package.json", canonical_deepseek_runtime_manifest())
    command = [
        npm,
        "install",
        "--save-exact",
        "--no-audit",
        "--no-fund",
        "--before",
        DEEPSEEK_NPM_BEFORE,
        *(f"{package}@{version}" for package, version in _PACKAGES),
    ]
    process = subprocess.Popen(
        command,
        cwd=dsh_home,
        env=env,
        stdin=subprocess.DEVNULL,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        start_new_session=(os.name != "nt"),
    )
    try:
        status = process.wait(timeout=_INSTALL_TIMEOUT_SECONDS)
    except subprocess.TimeoutExpired as exc:
        _terminate_process_tree(process)
        raise RuntimeError("DeepSeek package installation timed out after 300 seconds") from exc
    if status != 0:
        raise RuntimeError(f"DeepSeek package installation failed with exit code {status}")


def _resolve_cccc_executable(env: Dict[str, str]) -> Path:
    candidates = [env.get("CCCC_LAUNCHER_PATH"), shutil.which("cccc", path=env.get("PATH")), sys.argv[0]]
    for raw in candidates:
        if not raw:
            continue
        candidate = Path(raw).expanduser()
        try:
            candidate = candidate.resolve()
        except OSError:
            continue
        if candidate.is_file() and candidate.stem.lower() == "cccc":
            return candidate
    raise RuntimeError("CCCC executable is not available for DeepSeek setup")


def _ensure_profile(dsh_home: Path, executable: Path) -> bool:
    profile_root = dsh_home / "profiles"
    profile = profile_root / "cccc-acp"
    if profile.exists():
        try:
            managed = json.loads((profile / "package.json").read_text(encoding="utf-8")).get("ccccManaged") is True
        except (OSError, ValueError, TypeError):
            managed = False
        if not managed:
            raise RuntimeError("existing cccc-acp profile is not managed by CCCC")
        _write_profile_files(profile, executable)
        return False
    profile_root.mkdir(parents=True, exist_ok=True)
    staging = Path(tempfile.mkdtemp(prefix=".cccc-acp-", dir=profile_root))
    try:
        _write_profile_files(staging, executable)
        os.replace(staging, profile)
    finally:
        if staging.exists():
            shutil.rmtree(staging, ignore_errors=True)
    return True


def _write_profile_files(profile: Path, executable: Path) -> None:
    profile.mkdir(parents=True, exist_ok=True)
    atomic_write_json(
        profile / "package.json",
        {
            "name": "dsh-profile-cccc-acp",
            "private": True,
            "ccccManaged": True,
            "dependencies": {
                DEEPSEEK_ACP_PACKAGE: DEEPSEEK_ACP_VERSION,
                DEEPSEEK_MCP_CLIENT_PACKAGE: DEEPSEEK_MCP_CLIENT_VERSION,
                DEEPSEEK_ACP_APP_PACKAGE: DEEPSEEK_ACP_APP_VERSION,
                DEEPSEEK_LLM_ADAPTER_PACKAGE: DEEPSEEK_LLM_ADAPTER_VERSION,
            },
        },
    )
    # YAML single-quoted scalars escape apostrophes by doubling them. A
    # backslash is literal in this scalar style and must not be doubled.
    cccc_path = str(executable).replace("'", "''")
    atomic_write_text(
        profile / "cordis.yml",
        "- id: llm-deepseek\n"
        "  name: '@deepseek-ai/dsh-llm-deepseek'\n"
        "  config:\n"
        f"    maxTokens: {DEEPSEEK_MAX_OUTPUT_TOKENS}\n"
        "- id: acp-demo\n"
        "  name: '@deepseek-ai/dsh-acp-demo'\n"
        "  config:\n"
        "    provider: deepseek-official\n"
        "    model: deepseek-v4-flash\n"
        "    workspaceContext: false\n"
        "    persistenceRoot: !!js process.env.CCCC_DEEPSEEK_SESSION_ROOT\n"
        "- id: cccc-mcp\n"
        "  name: '@deepseek-ai/dsh-mcp-client'\n"
        "  config:\n"
        "    transport: stdio\n"
        "    serverName: cccc\n"
        f"    command: '{cccc_path}'\n"
        "    args: [mcp]\n"
        "    env:\n"
        "      CCCC_HOME: !!js process.env.CCCC_HOME\n"
        "      CCCC_GROUP_ID: !!js process.env.CCCC_GROUP_ID\n"
        "      CCCC_ACTOR_ID: !!js process.env.CCCC_ACTOR_ID\n"
        "    failOnStartupError: true\n",
    )
    (profile / "cordis.patch.yml").unlink(missing_ok=True)


def _terminate_process_tree(process: subprocess.Popen[bytes]) -> None:
    if process.poll() is not None:
        return
    if os.name == "nt":
        subprocess.run(
            ["taskkill", "/PID", str(process.pid), "/T", "/F"],
            check=False,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
    else:
        try:
            os.killpg(process.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
    try:
        process.wait(timeout=2)
    except subprocess.TimeoutExpired:
        process.kill()


def setup_deepseek_result(env: Dict[str, str] | None = None) -> tuple[Dict[str, object], str | None]:
    try:
        outcome = ensure_deepseek_setup(dict(os.environ) if env is None else dict(env))
        return (
            {
                "mode": "auto",
                "status": "ready",
                "dsh_home": str(outcome.dsh_home),
                "profile": str(outcome.profile),
                "packages_installed": outcome.packages_installed,
                "profile_created": outcome.profile_created,
            },
            None,
        )
    except Exception as exc:
        message = str(exc)
        return ({"mode": "auto", "status": "setup_required", "error": message}, message)
