from __future__ import annotations

import hashlib
import json
import os
from pathlib import Path
from typing import Mapping, Sequence

from ...kernel.runtime_hooks.store import begin_launch
from .provider_command import normalize_cli_command, shell_command

HOOK_TIMEOUT_SECONDS = 3
# Keep this list aligned with Codex's documented hook contract. Failed tool
# commands are reported through PostToolUse; Codex does not currently expose
# separate PostToolUseFailure or StopFailure hook events.
HOOK_EVENTS = (
    ("SessionStart", "session_start"),
    ("UserPromptSubmit", "user_prompt_submit"),
    ("PreToolUse", "pre_tool_use"),
    ("PermissionRequest", "permission_request"),
    ("PostToolUse", "post_tool_use"),
    ("SubagentStart", "subagent_start"),
    ("SubagentStop", "subagent_stop"),
    ("Stop", "stop"),
    ("SessionEnd", "session_end"),
)
_CODEX_SUBCOMMANDS = {
    "app-server",
    "completion",
    "debug",
    "exec",
    "login",
    "logout",
    "mcp",
    "proto",
    "sandbox",
    "server",
    "status",
}


def is_direct_codex_command(command: Sequence[str]) -> bool:
    if not command or Path(str(command[0])).name.lower() not in {"codex", "codex.exe"}:
        return False
    for item in command[1:]:
        value = str(item).strip()
        if value == "--":
            break
        if not value or value.startswith("-"):
            continue
        return value not in _CODEX_SUBCOMMANDS
    return True


def _hook_hash(event_key: str, command: str) -> str:
    identity = {
        "event_name": event_key,
        "hooks": [
            {
                "async": False,
                "command": command,
                "timeout": HOOK_TIMEOUT_SECONDS,
                "type": "command",
            }
        ],
    }
    encoded = json.dumps(
        identity, ensure_ascii=False, sort_keys=True, separators=(",", ":")
    ).encode("utf-8")
    return f"sha256:{hashlib.sha256(encoded).hexdigest()}"


def _toml_string(value: object) -> str:
    return json.dumps(str(value), ensure_ascii=False)


def configure_codex_launch(
    *,
    home: Path,
    group_id: str,
    actor_id: str,
    command: Sequence[str],
    env: Mapping[str, str],
    launch_token: str,
    cccc_executable: Path | None = None,
    cccc_command: Sequence[str] | None = None,
) -> tuple[list[str], dict[str, str]]:
    configured = [str(item) for item in command]
    launch_env = {str(key): str(value) for key, value in env.items()}
    if not is_direct_codex_command(configured):
        return configured, launch_env
    cli_command = normalize_cli_command(cccc_command, cccc_executable)
    begin_launch(home, "codex", group_id, actor_id, launch_token)
    hook_command = shell_command(cli_command, "hook", "codex-state")
    executable_toml = _toml_string(cli_command[0])
    mcp_args = json.dumps([*cli_command[1:], "mcp"], separators=(",", ":"))
    configured.extend(
        [
            "-c",
            f"mcp_servers.cccc.command={executable_toml}",
            "-c",
            f"mcp_servers.cccc.args={mcp_args}",
            "-c",
            f"mcp_servers.cccc.env.CCCC_HOME={_toml_string(home)}",
            "-c",
            f"mcp_servers.cccc.env.CCCC_GROUP_ID={_toml_string(group_id)}",
            "-c",
            f"mcp_servers.cccc.env.CCCC_ACTOR_ID={_toml_string(actor_id)}",
        ]
    )
    hook_toml = json.dumps(hook_command)
    for event_name, _ in HOOK_EVENTS:
        configured.extend(
            [
                "-c",
                (
                    f"hooks.{event_name}=[{{hooks=[{{type=\"command\","
                    f"command={hook_toml},timeout={HOOK_TIMEOUT_SECONDS}}}]}}]"
                ),
            ]
        )
    trusted = ",".join(
        (
            f"{json.dumps(f'/<session-flags>/config.toml:{event_key}:0:0')}"
            f'={{trusted_hash="{_hook_hash(event_key, hook_command)}"}}'
        )
        for _, event_key in HOOK_EVENTS
    )
    configured.extend(["-c", f"hooks.state={{{trusted}}}"])
    launch_env.update(
        {
            "CCCC_HOME": str(home),
            "CCCC_GROUP_ID": group_id,
            "CCCC_ACTOR_ID": actor_id,
            "CCCC_HOOK_LAUNCH_TOKEN": launch_token,
            "CCCC_CLI": cli_command[0],
        }
    )
    executable_dir = str(Path(cli_command[0]).parent)
    inherited_path = launch_env.get("PATH", os.environ.get("PATH", ""))
    path_items = [item for item in inherited_path.split(os.pathsep) if item and item != executable_dir]
    launch_env["PATH"] = os.pathsep.join([executable_dir, *path_items])
    return configured, launch_env
