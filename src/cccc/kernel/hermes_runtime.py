"""Hermes runtime setup helpers.

Hermes follows the same integration model as Claude/Codex: CCCC uses the
selected user Hermes home/profile and injects per-actor CCCC context at process
launch. CCCC does not create or select a separate Hermes profile.
"""

from __future__ import annotations

import json
import os
import shlex
import subprocess
from pathlib import Path
from typing import Any, Dict, Iterable, Optional

import yaml  # type: ignore

from ..paths import ensure_home
from ..util.file_lock import acquire_lockfile, release_lockfile
from ..util.process import find_subprocess_executable, resolve_subprocess_argv
from .runtime import get_cccc_mcp_stdio_command

HERMES_PROVIDER_ID = "xai-oauth"
HERMES_MCP_SERVER_NAME = "cccc"

HERMES_MCP_ENV_PLACEHOLDERS: Dict[str, str] = {
    "CCCC_HOME": "${CCCC_HOME}",
    "CCCC_GROUP_ID": "${CCCC_GROUP_ID}",
    "CCCC_ACTOR_ID": "${CCCC_ACTOR_ID}",
}
HERMES_DISCOVERY_GROUP_ID = "g_probe"
HERMES_DISCOVERY_ACTOR_ID = "hermes-probe"


def user_hermes_home() -> Path:
    return (Path.home() / ".hermes").expanduser()


def hermes_home(*, hermes_home_override: Optional[Path] = None) -> Path:
    if hermes_home_override is not None:
        return Path(hermes_home_override).expanduser()
    return user_hermes_home()


def hermes_profile_dir(*, hermes_home_override: Optional[Path] = None) -> Path:
    return hermes_home(hermes_home_override=hermes_home_override)


def hermes_profile_config_path(*, hermes_home_override: Optional[Path] = None) -> Path:
    return hermes_profile_dir(hermes_home_override=hermes_home_override) / "config.yaml"


def build_hermes_auth_add_command(*, no_browser: bool = False) -> list[str]:
    cmd = ["hermes", "auth", "add", HERMES_PROVIDER_ID]
    if no_browser:
        cmd.append("--no-browser")
    return cmd


def build_hermes_mcp_test_command() -> list[str]:
    return ["hermes", "mcp", "test", HERMES_MCP_SERVER_NAME]


def build_hermes_launch_command() -> list[str]:
    return ["hermes", "--tui", "--yolo"]


def _effective_hermes_home_override(hermes_home_override: Optional[Path]) -> Optional[Path]:
    if hermes_home_override is not None:
        return Path(hermes_home_override).expanduser()
    raw = str(os.environ.get("HERMES_HOME") or "").strip()
    return Path(raw).expanduser() if raw else None


def build_hermes_mcp_add_command(
    cccc_cmd: Optional[list[str]] = None,
    *,
    env_values: Optional[Dict[str, str]] = None,
) -> list[str]:
    cmd = list(cccc_cmd or get_cccc_mcp_stdio_command())
    if not cmd:
        cmd = ["cccc", "mcp"]
    out = ["hermes", "mcp", "add", HERMES_MCP_SERVER_NAME, "--command", str(cmd[0])]
    args = [str(part) for part in cmd[1:] if str(part).strip()]
    if args:
        out.append("--args")
        out.extend(args)
    out.append("--env")
    env = dict(env_values or HERMES_MCP_ENV_PLACEHOLDERS)
    out.extend(f"{key}={env.get(key, value)}" for key, value in HERMES_MCP_ENV_PLACEHOLDERS.items())
    return out


def _shell_join(parts: Iterable[str]) -> str:
    return " ".join(shlex.quote(str(part)) for part in parts)


def _read_yaml(path: Path) -> Dict[str, Any]:
    try:
        data = yaml.safe_load(path.read_text(encoding="utf-8")) or {}
    except FileNotFoundError:
        return {}
    except Exception:
        return {}
    return data if isinstance(data, dict) else {}


def _read_json(path: Path) -> Dict[str, Any]:
    try:
        data = json.loads(path.read_text(encoding="utf-8") or "{}")
    except FileNotFoundError:
        return {}
    except Exception:
        return {}
    return data if isinstance(data, dict) else {}


def _normalize_args(value: Any) -> list[str]:
    if isinstance(value, list):
        return [str(item) for item in value if str(item).strip()]
    if isinstance(value, tuple):
        return [str(item) for item in value if str(item).strip()]
    if value is None:
        return []
    return [str(part) for part in str(value or "").split() if str(part).strip()]


def _auth_doc_mentions_provider(value: Any) -> bool:
    needle = HERMES_PROVIDER_ID.lower()
    if isinstance(value, dict):
        for key, item in value.items():
            if needle in str(key or "").lower():
                return True
            if _auth_doc_mentions_provider(item):
                return True
        return False
    if isinstance(value, list):
        return any(_auth_doc_mentions_provider(item) for item in value)
    return needle in str(value or "").lower()


def _inspect_mcp_config(config: Dict[str, Any], *, expected_cmd: list[str]) -> Dict[str, Any]:
    servers = config.get("mcp_servers") if isinstance(config.get("mcp_servers"), dict) else {}
    entry = servers.get(HERMES_MCP_SERVER_NAME) if isinstance(servers, dict) else None
    if not isinstance(entry, dict):
        return {
            "status": "missing",
            "configured": False,
            "server_name": HERMES_MCP_SERVER_NAME,
            "expected_command": list(expected_cmd),
            "env_placeholders": dict(HERMES_MCP_ENV_PLACEHOLDERS),
        }

    command = str(entry.get("command") or "").strip()
    args = _normalize_args(entry.get("args"))
    env = entry.get("env") if isinstance(entry.get("env"), dict) else {}
    expected_command = str(expected_cmd[0] if expected_cmd else "").strip()
    expected_args = [str(part) for part in expected_cmd[1:]]
    command_ok = bool(command and expected_command and Path(command).expanduser() == Path(expected_command).expanduser())
    args_ok = args == expected_args
    env_ok = all(str(env.get(key) or "").strip() == value for key, value in HERMES_MCP_ENV_PLACEHOLDERS.items())
    enabled_ok = str(entry.get("enabled", True)).strip().lower() not in {"0", "false", "no"}
    status = "ready" if command_ok and args_ok and env_ok and enabled_ok else "stale"
    return {
        "status": status,
        "configured": True,
        "server_name": HERMES_MCP_SERVER_NAME,
        "command": command,
        "args": args,
        "enabled": enabled_ok,
        "expected_command": list(expected_cmd),
        "command_matches": command_ok,
        "args_match": args_ok,
        "env_placeholders": {key: str(env.get(key) or "") for key in HERMES_MCP_ENV_PLACEHOLDERS},
        "env_placeholders_match": env_ok,
    }


def _inspect_auth(profile_dir: Path) -> Dict[str, Any]:
    auth_path = profile_dir / "auth.json"
    exists = auth_path.exists()
    provider_present = False
    if exists:
        provider_present = _auth_doc_mentions_provider(_read_json(auth_path))
    return {
        "provider": HERMES_PROVIDER_ID,
        "auth_path": str(auth_path),
        "auth_file_exists": bool(exists),
        "status": "present" if provider_present else "missing",
    }


def _normalize_mcp_config_placeholders(config_path: Path) -> None:
    """Replace discovery-time CCCC env values with runtime placeholders.

    Hermes' official `mcp add` discovers tools by launching the server
    immediately, so setup must pass concrete CCCC env values to avoid creating a
    literal `${CCCC_HOME}` directory during discovery. The persisted shared
    profile, however, must retain placeholders so each actor process resolves
    its own CCCC identity at launch time.
    """
    def _matches() -> bool:
        doc_now = _read_yaml(config_path)
        servers_now = doc_now.get("mcp_servers") if isinstance(doc_now.get("mcp_servers"), dict) else None
        entry_now = servers_now.get(HERMES_MCP_SERVER_NAME) if isinstance(servers_now, dict) else None
        env_now = entry_now.get("env") if isinstance(entry_now, dict) and isinstance(entry_now.get("env"), dict) else {}
        return all(str(env_now.get(key) or "") == value for key, value in HERMES_MCP_ENV_PLACEHOLDERS.items())

    try:
        lines = config_path.read_text(encoding="utf-8").splitlines(keepends=True)
    except FileNotFoundError:
        return
    except Exception:
        lines = []

    if lines:
        changed = False
        in_mcp = False
        in_server = False
        in_env = False
        mcp_indent = 0
        server_indent = 0
        env_indent = 0
        for idx, line in enumerate(lines):
            stripped = line.lstrip(" ")
            content = stripped.strip()
            if not content or content.startswith("#"):
                continue
            indent = len(line) - len(stripped)
            if in_env and indent <= env_indent:
                in_env = False
            if in_server and indent <= server_indent and not (indent == server_indent and content.startswith(f"{HERMES_MCP_SERVER_NAME}:")):
                in_server = False
            if in_mcp and indent <= mcp_indent and not (indent == mcp_indent and content.startswith("mcp_servers:")):
                in_mcp = False
            if not in_mcp and content.startswith("mcp_servers:"):
                in_mcp = True
                mcp_indent = indent
                continue
            if in_mcp and not in_server and indent > mcp_indent and content.startswith(f"{HERMES_MCP_SERVER_NAME}:"):
                in_server = True
                server_indent = indent
                continue
            if in_server and not in_env and indent > server_indent and content.startswith("env:"):
                in_env = True
                env_indent = indent
                continue
            if in_env and indent > env_indent:
                for key, value in HERMES_MCP_ENV_PLACEHOLDERS.items():
                    if content.startswith(f"{key}:"):
                        newline = "\n" if line.endswith("\n") else ""
                        lines[idx] = f"{line[:indent]}{key}: {value}{newline}"
                        changed = True
                        break
        if changed:
            config_path.write_text("".join(lines), encoding="utf-8")
            if _matches():
                return

    doc = _read_yaml(config_path)
    servers = doc.get("mcp_servers") if isinstance(doc.get("mcp_servers"), dict) else None
    entry = servers.get(HERMES_MCP_SERVER_NAME) if isinstance(servers, dict) else None
    if not isinstance(entry, dict):
        return
    env = entry.get("env") if isinstance(entry.get("env"), dict) else {}
    changed = False
    for key, value in HERMES_MCP_ENV_PLACEHOLDERS.items():
        if str(env.get(key) or "") != value:
            env[key] = value
            changed = True
    if not changed:
        return
    entry["env"] = env
    config_path.write_text(yaml.safe_dump(doc, allow_unicode=True, sort_keys=False), encoding="utf-8")


def _hermes_version(path: str) -> str:
    if not path:
        return ""
    try:
        result = subprocess.run(
            resolve_subprocess_argv([path, "--version"]),
            capture_output=True,
            text=True,
            timeout=10,
        )
    except Exception:
        return ""
    if result.returncode != 0:
        return ""
    return str(result.stdout or "").strip().splitlines()[0] if str(result.stdout or "").strip() else ""


def hermes_runtime_status(
    *,
    home: Optional[Path] = None,
    include_version: bool = True,
    hermes_home_override: Optional[Path] = None,
) -> Dict[str, Any]:
    hermes_home_override = _effective_hermes_home_override(hermes_home_override)
    root = Path(home).expanduser().resolve() if home is not None else ensure_home()
    selected_home = hermes_home(hermes_home_override=hermes_home_override)
    profile_dir = hermes_profile_dir(hermes_home_override=hermes_home_override)
    config_path = hermes_profile_config_path(hermes_home_override=hermes_home_override)
    normal_home = user_hermes_home()
    expected_cmd = get_cccc_mcp_stdio_command()
    config = _read_yaml(config_path)
    hermes_path = find_subprocess_executable("hermes")

    mcp = _inspect_mcp_config(config, expected_cmd=expected_cmd)
    auth = _inspect_auth(profile_dir)
    issues: list[str] = []
    if not hermes_path:
        issues.append("hermes_cli_missing")
    if not profile_dir.exists():
        issues.append("profile_missing")
    if not config_path.exists():
        issues.append("config_missing")
    if mcp.get("status") != "ready":
        issues.append("cccc_mcp_config_not_ready")
    if auth.get("status") != "present":
        issues.append("xai_oauth_missing")

    gates = [
        {
            "id": "uses_selected_user_hermes_home",
            "status": "pass",
            "evidence": str(selected_home),
        },
        {
            "id": "selected_profile_exists",
            "status": "pass" if profile_dir.exists() else "fail",
            "evidence": str(profile_dir),
        },
        {
            "id": "mcp_config_uses_actor_placeholders",
            "status": "pass" if bool(mcp.get("env_placeholders_match")) else "fail",
            "evidence": mcp.get("env_placeholders", {}),
        },
        {
            "id": "xai_oauth_present",
            "status": "pass" if auth.get("status") == "present" else "fail",
            "evidence": auth.get("auth_path"),
        },
        {
            "id": "concurrent_actor_attribution",
            "status": "pending",
            "evidence": "requires manual two-actor Hermes smoke after OAuth",
        },
        {
            "id": "ignore_rules_safe",
            "status": "pending",
            "evidence": "not enabled by default until Hermes smoke proves MCP still works",
        },
    ]

    env_prefix = f"HERMES_HOME={shlex.quote(str(selected_home))} " if hermes_home_override is not None else ""
    mcp_add_shape = _shell_join(
        build_hermes_mcp_add_command(
            expected_cmd,
            env_values={
                "CCCC_HOME": str(root),
                "CCCC_GROUP_ID": HERMES_DISCOVERY_GROUP_ID,
                "CCCC_ACTOR_ID": HERMES_DISCOVERY_ACTOR_ID,
            },
        )
    )
    commands = {
        "prepare": f"{env_prefix}cccc runtime hermes prepare --yes",
        "mcp_test": f"{env_prefix}cccc runtime hermes mcp-test",
        "auth_add": f"{env_prefix}{_shell_join(build_hermes_auth_add_command())}",
        "auth_add_no_browser": f"{env_prefix}{_shell_join(build_hermes_auth_add_command(no_browser=True))}",
        "launch": f"{env_prefix}{_shell_join(build_hermes_launch_command())}",
        "official_mcp_add_shape": f"{env_prefix}{mcp_add_shape}",
    }

    return {
        "runtime": "hermes",
        "phase": "phase1_pty_runtime_mvp",
        "user_facing_actor_runtime_enabled": True,
        "setup_ready": bool(profile_dir.exists() and mcp.get("status") == "ready"),
        "auth_ready": bool(auth.get("status") == "present"),
        "launch_ready": bool(hermes_path and profile_dir.exists() and mcp.get("status") == "ready"),
        "hermes_cli": {
            "available": bool(hermes_path),
            "path": hermes_path,
            "version": _hermes_version(hermes_path or "") if include_version else "",
        },
        "hermes_home": str(selected_home),
        "profile": {
            "name": "default",
            "dir": str(profile_dir),
            "exists": bool(profile_dir.exists()),
            "config_path": str(config_path),
            "config_exists": bool(config_path.exists()),
        },
        "user_hermes_home": str(normal_home),
        "mcp": mcp,
        "auth": auth,
        "commands": commands,
        "phase0_gates": gates,
        "issues": issues,
    }


def _run_hermes_cli(
    argv: list[str],
    *,
    hermes_home_path: Optional[Path] = None,
    cwd: Optional[Path] = None,
    timeout: int = 60,
    input_text: Optional[str] = None,
    extra_env: Optional[Dict[str, str]] = None,
) -> subprocess.CompletedProcess[str]:
    env = dict(os.environ)
    if hermes_home_path is not None:
        env["HERMES_HOME"] = str(hermes_home_path)
    if extra_env:
        env.update({str(k): str(v) for k, v in extra_env.items() if isinstance(k, str)})
    kwargs: Dict[str, Any] = {
        "capture_output": True,
        "text": True,
        "timeout": timeout,
        "env": env,
    }
    if cwd is not None:
        kwargs["cwd"] = str(cwd)
    if input_text is not None:
        kwargs["input"] = input_text
    return subprocess.run(resolve_subprocess_argv(argv), **kwargs)


def _completed_summary(result: subprocess.CompletedProcess[str]) -> Dict[str, Any]:
    return {
        "returncode": int(result.returncode),
        "stdout": str(result.stdout or ""),
        "stderr": str(result.stderr or ""),
    }


def prepare_hermes_runtime(
    *,
    home: Optional[Path] = None,
    cwd: Optional[Path] = None,
    auto_enable_tools: bool = False,
    force_mcp: bool = False,
    hermes_home_override: Optional[Path] = None,
) -> Dict[str, Any]:
    hermes_home_override = _effective_hermes_home_override(hermes_home_override)
    root = Path(home).expanduser().resolve() if home is not None else ensure_home()
    discovery_home = Path(home).expanduser() if home is not None else root
    lock = None
    commands_run: list[Dict[str, Any]] = []
    try:
        if not find_subprocess_executable("hermes"):
            return {
                "ok": False,
                "error": {"code": "hermes_cli_missing", "message": "Hermes CLI is not installed or not in PATH"},
                "status": hermes_runtime_status(
                    home=root,
                    include_version=False,
                    hermes_home_override=hermes_home_override,
                ),
            }
        lock_dir = root / "daemon"
        lock_dir.mkdir(parents=True, exist_ok=True)
        lock = acquire_lockfile(lock_dir / "hermes-runtime-setup.lock", blocking=True)

        status = hermes_runtime_status(
            home=root,
            include_version=False,
            hermes_home_override=hermes_home_override,
        )
        if force_mcp or ((status.get("mcp") or {}).get("status") != "ready"):
            if not auto_enable_tools:
                return {
                    "ok": False,
                    "error": {
                        "code": "hermes_mcp_setup_requires_confirmation",
                        "message": "Hermes MCP setup is discovery-first; rerun with auto_enable_tools/--yes to enable discovered CCCC tools.",
                    },
                    "commands_run": commands_run,
                    "status": status,
                }
            cmd = build_hermes_mcp_add_command(
                env_values={
                    "CCCC_HOME": str(discovery_home),
                    "CCCC_GROUP_ID": HERMES_DISCOVERY_GROUP_ID,
                    "CCCC_ACTOR_ID": HERMES_DISCOVERY_ACTOR_ID,
                },
            )
            result = _run_hermes_cli(cmd, hermes_home_path=hermes_home_override, cwd=cwd, timeout=120, input_text="Y\n")
            commands_run.append({"name": "mcp_add", "argv": cmd, "result": _completed_summary(result)})
            if result.returncode == 0:
                _normalize_mcp_config_placeholders(
                    hermes_profile_config_path(hermes_home_override=hermes_home_override)
                )
            status = hermes_runtime_status(
                home=root,
                include_version=False,
                hermes_home_override=hermes_home_override,
            )
            if result.returncode != 0 or ((status.get("mcp") or {}).get("status") != "ready"):
                return {
                    "ok": False,
                    "error": {"code": "hermes_mcp_add_failed", "message": "failed to configure Hermes CCCC MCP server"},
                    "commands_run": commands_run,
                    "status": status,
                }

        return {
            "ok": True,
            "commands_run": commands_run,
            "status": hermes_runtime_status(
                home=root,
                include_version=False,
                hermes_home_override=hermes_home_override,
            ),
        }
    except Exception as exc:
        return {
            "ok": False,
            "error": {"code": "hermes_prepare_failed", "message": str(exc)},
            "commands_run": commands_run,
            "status": hermes_runtime_status(
                home=root,
                include_version=False,
                hermes_home_override=hermes_home_override,
            ),
        }
    finally:
        if lock is not None:
            release_lockfile(lock)


def run_hermes_mcp_test(
    *,
    home: Optional[Path] = None,
    cwd: Optional[Path] = None,
    group_id: str = "g_probe",
    actor_id: str = "hermes-probe",
    hermes_home_override: Optional[Path] = None,
) -> Dict[str, Any]:
    hermes_home_override = _effective_hermes_home_override(hermes_home_override)
    root = Path(home).expanduser().resolve() if home is not None else ensure_home()
    cmd = build_hermes_mcp_test_command()
    env = {
        "CCCC_HOME": str(root),
        "CCCC_GROUP_ID": str(group_id or "g_probe"),
        "CCCC_ACTOR_ID": str(actor_id or "hermes-probe"),
    }
    try:
        result = _run_hermes_cli(
            cmd,
            hermes_home_path=hermes_home_override,
            cwd=cwd,
            timeout=60,
            extra_env=env,
        )
    except Exception as exc:
        return {"ok": False, "argv": cmd, "error": {"code": "hermes_mcp_test_failed", "message": str(exc)}}
    return {
        "ok": result.returncode == 0,
        "argv": cmd,
        "result": _completed_summary(result),
    }
