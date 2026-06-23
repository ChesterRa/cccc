"""Runtime MCP installation helpers."""

from __future__ import annotations

import json
import os
import re
import subprocess
import sys
from pathlib import Path
from typing import Any, Dict

from ..kernel.hermes_runtime import hermes_runtime_status, prepare_hermes_runtime
from ..kernel.runtime import get_cccc_mcp_stdio_command
from ..util.conv import coerce_bool
from ..util.fs import read_json
from ..util.process import resolve_subprocess_argv


def _parse_mcp_get_output(output: str) -> Dict[str, str]:
    parsed: Dict[str, str] = {}
    for raw in str(output or "").splitlines():
        line = raw.strip()
        if not line or ":" not in line:
            continue
        key, value = line.split(":", 1)
        parsed[key.strip().lower()] = value.strip()
    return parsed


def _normalize_mcp_command_value(value: str) -> str:
    normalized = str(value or "").strip().strip('"').strip("'")
    if sys.platform.startswith("win"):
        return normalized.replace("/", "\\").lower()
    return normalized


def _normalize_mcp_arg_values(value: Any) -> list[str]:
    if value is None:
        return []
    if isinstance(value, (list, tuple)):
        parts = value
    else:
        parts = str(value or "").split()
    return [str(part or "").strip().strip('"').strip("'") for part in parts if str(part or "").strip()]


def _entry_command_matches_expected(command: Any, args: Any, expected_cmd: list[str], *, strict: bool) -> bool:
    if not expected_cmd:
        return False
    actual_command = str(command or "").strip()
    if not actual_command:
        return not strict
    expected_command = _normalize_mcp_command_value(expected_cmd[0])
    if _normalize_mcp_command_value(actual_command) != expected_command:
        return False
    return _normalize_mcp_arg_values(args) == _normalize_mcp_arg_values(expected_cmd[1:])


def _mcp_transport_matches(entry: Dict[str, Any]) -> bool:
    transport = entry.get("transport", entry.get("type", "stdio"))
    value = str(transport or "stdio").strip().lower()
    return not value or value == "stdio"


def _coerce_output_text(output: Any) -> str:
    if isinstance(output, bytes):
        return output.decode(errors="ignore")
    return str(output or "")


def _codex_mcp_entry_matches_expected(output: str, expected_cmd: list[str]) -> bool:
    entry = _parse_mcp_get_output(output)
    if not entry:
        return False
    if str(entry.get("enabled", "true")).strip().lower() == "false":
        return False
    if not _mcp_transport_matches(entry):
        return False
    persisted_env = str(entry.get("env") or entry.get("environment") or "")
    if any(key in persisted_env for key in _CODEX_CONTEXT_ENV_KEYS):
        return False
    return _entry_command_matches_expected(
        entry.get("command", ""),
        entry.get("args", ""),
        expected_cmd,
        strict=sys.platform.startswith("win"),
    )


def _decode_debug_quoted_string(value: str) -> str:
    raw = str(value or "")
    try:
        return str(json.loads(f'"{raw}"'))
    except Exception:
        return raw.replace('\\"', '"').replace("\\\\", "\\")


def _devin_debug_string_field(output: str, field: str) -> str:
    match = re.search(rf"\b{re.escape(field)}:\s*\"((?:\\.|[^\"\\])*)\"", str(output or ""))
    if not match:
        return ""
    return _decode_debug_quoted_string(match.group(1))


def _devin_debug_args(output: str) -> list[str]:
    match = re.search(r"\bargs:\s*\[(.*?)\]", str(output or ""), flags=re.S)
    if not match:
        return []
    return [
        _decode_debug_quoted_string(item)
        for item in re.findall(r"\"((?:\\.|[^\"\\])*)\"", match.group(1))
    ]


def _devin_mcp_entry_matches_expected(output: str, expected_cmd: list[str]) -> bool:
    text = str(output or "")
    if "stdio" not in text.lower():
        return False
    command = _devin_debug_string_field(text, "command")
    if not command:
        return False
    args = _devin_debug_args(text)
    return _entry_command_matches_expected(
        command,
        args,
        expected_cmd,
        strict=sys.platform.startswith("win"),
    )


def _claude_mcp_entry_matches_expected(output: str, expected_cmd: list[str]) -> bool:
    entry = _parse_mcp_get_output(output)
    if not entry:
        return False
    if not _mcp_transport_matches(entry):
        return False
    return _entry_command_matches_expected(
        entry.get("command", ""),
        entry.get("args", ""),
        expected_cmd,
        strict=sys.platform.startswith("win"),
    )


def _json_mcp_entry_matches_expected(entry: Any, expected_cmd: list[str]) -> bool:
    if not isinstance(entry, dict):
        return bool(entry)
    if coerce_bool(entry.get("disabled"), default=False):
        return False
    if not _mcp_transport_matches(entry):
        return False
    return _entry_command_matches_expected(
        entry.get("command", ""),
        entry.get("args", []),
        expected_cmd,
        strict=sys.platform.startswith("win"),
    )


def _mcp_command_array_matches_expected(command: Any, expected_cmd: list[str]) -> bool:
    if not isinstance(command, list) or len(command) != len(expected_cmd):
        return False
    if not expected_cmd:
        return False
    actual = [str(part or "").strip().strip('"').strip("'") for part in command]
    expected = [str(part or "").strip().strip('"').strip("'") for part in expected_cmd]
    if _normalize_mcp_command_value(actual[0]) != _normalize_mcp_command_value(expected[0]):
        return False
    return actual[1:] == expected[1:]


def _grok_mcp_entry_matches_expected(entry: Any, expected_cmd: list[str]) -> bool:
    if not isinstance(entry, dict):
        return False
    if str(entry.get("name") or "").strip() != "cccc":
        return False
    if coerce_bool(entry.get("enabled"), default=True) is False:
        return False
    if not _entry_command_matches_expected(
        entry.get("command", ""),
        entry.get("args", []),
        expected_cmd,
        strict=sys.platform.startswith("win"),
    ):
        return False
    env = entry.get("env")
    if not isinstance(env, dict):
        return False
    return str(env.get("PYTHONUNBUFFERED") or "").strip() == "1"


def _runtime_expected_cccc_command(runtime: str) -> list[str]:
    cmd = list(get_cccc_mcp_stdio_command())
    if sys.platform.startswith("win") and runtime == "droid" and cmd:
        cmd[0] = str(cmd[0]).replace("\\", "/")
    return cmd


def _home_dir(env: Dict[str, str] | None) -> Path:
    raw = ""
    if isinstance(env, dict):
        raw = str(env.get("HOME") or env.get("USERPROFILE") or "").strip()
    if raw:
        return Path(raw).expanduser()
    return Path.home()


def _cccc_home_dir(env: Dict[str, str] | None) -> Path | None:
    raw = ""
    if isinstance(env, dict):
        raw = str(env.get("CCCC_HOME") or "").strip()
    if raw:
        return Path(raw).expanduser().resolve()
    return None


def _hermes_home_override(env: Dict[str, str] | None) -> Path | None:
    raw = ""
    if isinstance(env, dict):
        raw = str(env.get("HERMES_HOME") or "").strip()
    if raw:
        return Path(raw).expanduser()
    return None


def _kimi_share_dir(env: Dict[str, str] | None) -> Path:
    raw = ""
    if isinstance(env, dict):
        raw = str(env.get("KIMI_SHARE_DIR") or "").strip()
    if raw:
        return Path(raw).expanduser()
    return _home_dir(env) / ".kimi"


def _kiro_home_dir(env: Dict[str, str] | None) -> Path:
    raw = ""
    if isinstance(env, dict):
        raw = str(env.get("KIRO_HOME") or "").strip()
    if not raw:
        raw = str(os.environ.get("KIRO_HOME") or "").strip()
    if raw:
        return Path(raw).expanduser()
    return _home_dir(env) / ".kiro"


_OPENCODE_CONTEXT_ENV_KEYS = ("CCCC_HOME", "CCCC_GROUP_ID", "CCCC_ACTOR_ID")
_CODEX_CONTEXT_ENV_KEYS = ("CCCC_HOME", "CCCC_GROUP_ID", "CCCC_ACTOR_ID")


def _opencode_context_environment(env: Dict[str, str] | None) -> Dict[str, str]:
    result: Dict[str, str] = {}
    if not isinstance(env, dict):
        return result
    for key in _OPENCODE_CONTEXT_ENV_KEYS:
        value = str(env.get(key) or "").strip()
        if value:
            result[key] = value
    return result


def _opencode_cccc_entry(env: Dict[str, str] | None) -> Dict[str, Any]:
    return {
        "type": "local",
        "command": _runtime_expected_cccc_command("opencode"),
        "enabled": True,
        "environment": _opencode_context_environment(env),
    }


def _read_opencode_inline_config(env: Dict[str, str] | None) -> Dict[str, Any]:
    raw = ""
    if isinstance(env, dict):
        raw = str(env.get("OPENCODE_CONFIG_CONTENT") or "").strip()
    if not raw:
        return {}
    try:
        doc = json.loads(raw)
    except json.JSONDecodeError as exc:
        raise ValueError("invalid OPENCODE_CONFIG_CONTENT: expected JSON object") from exc
    if not isinstance(doc, dict):
        raise ValueError("invalid OPENCODE_CONFIG_CONTENT: expected JSON object")
    return dict(doc)


def _opencode_mcp_entry_matches_expected(entry: Any, expected_cmd: list[str], env: Dict[str, str] | None) -> bool:
    if not isinstance(entry, dict):
        return False
    if str(entry.get("type") or "").strip().lower() != "local":
        return False
    if coerce_bool(entry.get("enabled"), default=True) is False:
        return False
    if not _mcp_command_array_matches_expected(entry.get("command"), expected_cmd):
        return False
    expected_env = _opencode_context_environment(env)
    if expected_env:
        actual_env = entry.get("environment")
        if not isinstance(actual_env, dict):
            return False
        for key, value in expected_env.items():
            actual = str(actual_env.get(key) or "").strip()
            if actual not in {value, f"{{env:{key}}}"}:
                return False
    return True


def _opencode_mcp_state(env: Dict[str, str] | None) -> str:
    try:
        doc = _read_opencode_inline_config(env)
    except ValueError:
        return "stale"
    servers = doc.get("mcp") if isinstance(doc, dict) else None
    if not isinstance(servers, dict):
        return "missing"
    entry = servers.get("cccc")
    if entry is None:
        return "missing"
    expected_cmd = _runtime_expected_cccc_command("opencode")
    return "ready" if _opencode_mcp_entry_matches_expected(entry, expected_cmd, env) else "stale"


def prepare_runtime_mcp_env(runtime: str, env: Dict[str, Any] | None) -> Dict[str, str]:
    """Return the process env needed for runtime-scoped CCCC MCP wiring.

    OpenCode's `mcp add` command is interactive, so CCCC injects the CCCC MCP
    server through OpenCode's inline runtime config instead of editing global
    user configuration.
    """
    result = {str(k): str(v) for k, v in (env or {}).items() if isinstance(k, str)}
    if str(runtime or "").strip().lower() != "opencode":
        return result

    doc = _read_opencode_inline_config(result)
    mcp = doc.get("mcp")
    if not isinstance(mcp, dict):
        mcp = {}
    else:
        mcp = dict(mcp)
    mcp["cccc"] = _opencode_cccc_entry(result)
    doc["mcp"] = mcp
    result["OPENCODE_CONFIG_CONTENT"] = json.dumps(doc, ensure_ascii=True, separators=(",", ":"), sort_keys=True)
    return result


def build_mcp_add_command(runtime: str) -> list[str] | None:
    cccc_cmd = _runtime_expected_cccc_command(runtime)
    if runtime == "claude":
        return ["claude", "mcp", "add", "-s", "user", "cccc", "--", *cccc_cmd]
    if runtime == "codex":
        return ["codex", "mcp", "add", "cccc", "--", *cccc_cmd]
    if runtime == "devin":
        return ["devin", "mcp", "add", "-s", "user", "cccc", "--", *cccc_cmd]
    if runtime == "kiro":
        command = cccc_cmd[0] if cccc_cmd else "cccc"
        argv = ["kiro-cli", "mcp", "add", "--name", "cccc", "--scope", "global", "--command", command]
        args = cccc_cmd[1:] if len(cccc_cmd) > 1 else ["mcp"]
        for arg in args:
            argv.append(f"--args={arg}")
        argv.append("--force")
        return argv
    if runtime == "droid":
        return ["droid", "mcp", "add", "--type", "stdio", "cccc", *cccc_cmd]
    if runtime == "amp":
        return ["amp", "mcp", "add", "cccc", *cccc_cmd]
    if runtime == "auggie":
        return ["auggie", "mcp", "add", "cccc", "--", *cccc_cmd]
    if runtime == "grok":
        command = cccc_cmd[0] if cccc_cmd else "cccc"
        args = cccc_cmd[1:] if len(cccc_cmd) > 1 else ["mcp"]
        argv = ["grok", "mcp", "add", "cccc", "--command", command]
        if len(args) == 1 and not str(args[0]).startswith("-"):
            argv.extend(["--args", str(args[0])])
        else:
            for arg in args:
                argv.append(f"--args={arg}")
        argv.extend(["--env", "PYTHONUNBUFFERED=1"])
        return argv
    if runtime == "hermes":
        return ["cccc", "runtime", "hermes", "prepare", "--yes"]
    if runtime == "kimi":
        return ["kimi", "mcp", "add", "--transport", "stdio", "cccc", "--", *cccc_cmd]
    return None


def build_mcp_remove_command(runtime: str) -> list[str] | None:
    if runtime == "claude":
        return ["claude", "mcp", "remove", "cccc", "-s", "user"]
    if runtime == "devin":
        return ["devin", "mcp", "remove", "-s", "user", "cccc"]
    if runtime == "kiro":
        return ["kiro-cli", "mcp", "remove", "--name", "cccc", "--scope", "global"]
    if runtime == "droid":
        return ["droid", "mcp", "remove", "cccc"]
    if runtime == "grok":
        return ["grok", "mcp", "remove", "cccc"]
    return None


def _run_cli(
    argv: list[str],
    *,
    cwd: Path | None = None,
    timeout: int,
    text: bool = True,
    env: Dict[str, str] | None = None,
    drop_env_keys: tuple[str, ...] = (),
) -> subprocess.CompletedProcess[Any]:
    kwargs: dict[str, object] = {
        "capture_output": True,
        "timeout": timeout,
        "text": text,
    }
    if cwd is not None:
        kwargs["cwd"] = str(cwd)
    if env is not None or drop_env_keys:
        merged_env = dict(os.environ)
        for key in drop_env_keys:
            merged_env.pop(key, None)
        merged_env.update({str(k): str(v) for k, v in (env or {}).items() if isinstance(k, str)})
        for key in drop_env_keys:
            merged_env.pop(key, None)
        kwargs["env"] = merged_env
    return subprocess.run(resolve_subprocess_argv(argv), **kwargs)


def _json_mcp_entry_state(cfg_path: Path, expected_cmd: list[str]) -> str:
    cfg = read_json(cfg_path)
    servers = cfg.get("mcpServers") if isinstance(cfg, dict) else None
    if not isinstance(servers, dict):
        return "missing"
    entry = servers.get("cccc")
    if entry is None:
        return "missing"
    return "ready" if _json_mcp_entry_matches_expected(entry, expected_cmd) else "stale"


def _json_mcp_state(paths: tuple[Path, ...], expected_cmd: list[str]) -> str:
    state = "missing"
    for cfg_path in paths:
        entry_state = _json_mcp_entry_state(cfg_path, expected_cmd)
        if entry_state == "ready":
            return "ready"
        if entry_state == "stale":
            state = "stale"
    return state


def _kiro_mcp_state(expected_cmd: list[str], *, cwd: Path | None = None, env: Dict[str, str] | None = None) -> str:
    if cwd is not None:
        local_state = _json_mcp_entry_state(Path(cwd) / ".kiro" / "settings" / "mcp.json", expected_cmd)
        if local_state != "missing":
            return local_state
    return _json_mcp_state((_kiro_home_dir(env) / "settings" / "mcp.json",), expected_cmd)


def _runtime_mcp_state(runtime: str, *, cwd: Path | None = None, env: Dict[str, str] | None = None) -> str:
    expected_cmd = _runtime_expected_cccc_command(runtime)

    if runtime == "claude":
        result = _run_cli(["claude", "mcp", "get", "cccc"], timeout=10, text=False, env=env)
        if result.returncode != 0:
            return "missing"
        output = _coerce_output_text(result.stdout)
        return "ready" if _claude_mcp_entry_matches_expected(output, expected_cmd) else "stale"

    if runtime == "codex":
        result = _run_cli(["codex", "mcp", "get", "cccc"], timeout=10, env=env, drop_env_keys=_CODEX_CONTEXT_ENV_KEYS)
        if result.returncode != 0:
            return "missing"
        return "ready" if _codex_mcp_entry_matches_expected(result.stdout, expected_cmd) else "stale"

    if runtime == "devin":
        kwargs: dict[str, Any] = {"timeout": 10, "env": env}
        if cwd is not None:
            kwargs["cwd"] = cwd
        result = _run_cli(["devin", "mcp", "get", "cccc"], **kwargs)
        if result.returncode != 0:
            return "missing"
        return "ready" if _devin_mcp_entry_matches_expected(result.stdout, expected_cmd) else "stale"

    if runtime == "kiro":
        return _kiro_mcp_state(expected_cmd, cwd=cwd, env=env)

    if runtime == "droid":
        home = _home_dir(env)
        return _json_mcp_state(
            (
                home / ".factory" / "mcp.json",
                home / ".config" / "droid" / "mcp.json",
                home / ".droid" / "mcp.json",
            ),
            expected_cmd,
        )

    if runtime == "amp":
        settings_path = _home_dir(env) / ".config" / "amp" / "settings.json"
        if not settings_path.exists():
            return "missing"
        doc = json.loads(settings_path.read_text(encoding="utf-8") or "{}")
        if not isinstance(doc, dict):
            return "missing"
        servers = doc.get("amp.mcpServers")
        if not isinstance(servers, dict):
            return "missing"
        entry = servers.get("cccc")
        if entry is None:
            return "missing"
        return "ready" if _json_mcp_entry_matches_expected(entry, expected_cmd) else "stale"

    if runtime == "auggie":
        settings_path = _home_dir(env) / ".augment" / "settings.json"
        if not settings_path.exists():
            return "missing"
        doc = json.loads(settings_path.read_text(encoding="utf-8") or "{}")
        if not isinstance(doc, dict):
            return "missing"
        servers = doc.get("mcpServers")
        if not isinstance(servers, dict):
            return "missing"
        entry = servers.get("cccc")
        if entry is None:
            return "missing"
        return "ready" if _json_mcp_entry_matches_expected(entry, expected_cmd) else "stale"

    if runtime == "grok":
        result = _run_cli(["grok", "mcp", "list", "--json"], timeout=10, env=env)
        if result.returncode != 0:
            return "missing"
        try:
            entries = json.loads(result.stdout or "[]")
        except json.JSONDecodeError:
            return "missing"
        if not isinstance(entries, list):
            return "missing"
        state = "missing"
        for entry in entries:
            if not isinstance(entry, dict):
                continue
            if str(entry.get("name") or "").strip() != "cccc":
                continue
            if _grok_mcp_entry_matches_expected(entry, expected_cmd):
                return "ready"
            state = "stale"
        return state

    if runtime == "hermes":
        status = hermes_runtime_status(
            home=_cccc_home_dir(env),
            include_version=False,
            hermes_home_override=_hermes_home_override(env),
        )
        mcp = status.get("mcp") if isinstance(status.get("mcp"), dict) else {}
        return str(mcp.get("status") or "missing")

    if runtime == "kimi":
        return _json_mcp_state((_kimi_share_dir(env) / "mcp.json",), expected_cmd)

    if runtime == "opencode":
        return _opencode_mcp_state(env)

    return "missing"


def is_mcp_installed(runtime: str, *, cwd: Path | None = None, env: Dict[str, str] | None = None) -> bool:
    try:
        return _runtime_mcp_state(runtime, cwd=cwd, env=env) == "ready"
    except (FileNotFoundError, subprocess.TimeoutExpired):
        pass
    except Exception:
        pass
    return False


def ensure_mcp_installed(
    runtime: str,
    cwd: Path,
    *,
    auto_mcp_runtimes: tuple[str, ...],
    env: Dict[str, str] | None = None,
) -> bool:
    if runtime not in auto_mcp_runtimes:
        return True
    if runtime == "hermes":
        try:
            state = _runtime_mcp_state(runtime, env=env)
            if state == "ready":
                return True
            result = prepare_hermes_runtime(
                home=_cccc_home_dir(env),
                cwd=cwd,
                auto_enable_tools=True,
                force_mcp=(state == "stale"),
                hermes_home_override=_hermes_home_override(env),
            )
            return bool(result.get("ok")) and _runtime_mcp_state(runtime, env=env) == "ready"
        except Exception:
            return False
    try:
        state = _runtime_mcp_state(runtime, cwd=cwd, env=env)
        if state == "ready":
            return True
        add_cmd = build_mcp_add_command(runtime)
        if not add_cmd:
            return False

        if state == "stale":
            remove_cmd = build_mcp_remove_command(runtime)
            if remove_cmd:
                remove_result = _run_cli(remove_cmd, cwd=cwd, timeout=30, env=env)
                if remove_result.returncode != 0:
                    return False

        drop_env_keys = _CODEX_CONTEXT_ENV_KEYS if runtime == "codex" else ()
        result = _run_cli(add_cmd, cwd=cwd, timeout=30, env=env, drop_env_keys=drop_env_keys)
        return result.returncode == 0 and is_mcp_installed(runtime, cwd=cwd, env=env)
    except (FileNotFoundError, subprocess.TimeoutExpired):
        pass
    return False
