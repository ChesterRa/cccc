from __future__ import annotations

import json
import os
import re
from pathlib import Path
from typing import Any, Mapping, Sequence

from ...kernel.runtime_hooks.store import begin_launch
from .provider_command import normalize_cli_command, shell_command

HOOK_TIMEOUT_SECONDS = 3
MIN_CLAUDE_VERSION = (2, 1, 141)
HOOK_EVENTS = (
    "SessionStart",
    "UserPromptSubmit",
    "PreToolUse",
    "PermissionRequest",
    "PostToolUse",
    "Stop",
    "SessionEnd",
)
NOTIFICATION_MATCHER = (
    "permission_prompt|idle_prompt|elicitation_dialog|agent_needs_input|agent_completed"
)


def is_direct_claude_command(command: Sequence[str]) -> bool:
    return bool(
        command
        and Path(str(command[0]).replace("\\", "/")).name.lower()
        in {"claude", "claude.exe"}
    )

def parse_claude_version(text: str) -> tuple[int, int, int] | None:
    for word in str(text).split():
        match = re.search(r"(\d+)\.(\d+)\.(\d+)", word)
        if match:
            return tuple(int(match.group(index)) for index in (1, 2, 3))  # type: ignore[return-value]
    return None


def _load_settings(value: str, cwd: Path) -> dict[str, Any]:
    source = str(value)
    if not source.lstrip().startswith("{"):
        path = Path(source)
        if not path.is_absolute():
            path = cwd / path
        source = path.read_text(encoding="utf-8")
    parsed = json.loads(source)
    if not isinstance(parsed, dict):
        raise ValueError("Claude settings must be a JSON object")
    return parsed


def _append_hook(
    hooks: dict[str, Any], event: str, command: str, matcher: str | None = None
) -> None:
    groups = hooks.setdefault(event, [])
    if not isinstance(groups, list):
        raise ValueError(f"Claude hook {event} must be an array")
    group: dict[str, Any] = {}
    if matcher is not None:
        group["matcher"] = matcher
    group["hooks"] = [
        {
            "type": "command",
            "command": command,
            "timeout": HOOK_TIMEOUT_SECONDS,
        }
    ]
    groups.append(group)


def append_claude_settings(
    command: Sequence[str],
    *,
    cwd: Path,
    cccc_executable: Path | None = None,
    cccc_command: Sequence[str] | None = None,
) -> list[str]:
    original = [str(item) for item in command]
    retained: list[str] = []
    prompt_tail: list[str] = []
    effective_settings: str | None = None
    index = 0
    while index < len(original):
        item = original[index]
        if item == "--":
            prompt_tail = original[index:]
            break
        if item == "--settings":
            if index + 1 >= len(original):
                raise ValueError("--settings requires a value")
            effective_settings = original[index + 1]
            index += 2
            continue
        if item.startswith("--settings="):
            effective_settings = item.split("=", 1)[1]
            index += 1
            continue
        retained.append(item)
        index += 1
    settings = (
        _load_settings(effective_settings, cwd) if effective_settings is not None else {}
    )
    hooks = settings.setdefault("hooks", {})
    if hooks is None:
        hooks = {}
        settings["hooks"] = hooks
    if not isinstance(hooks, dict):
        raise ValueError("Claude settings hooks must be an object")
    cli_command = normalize_cli_command(cccc_command, cccc_executable)
    command_text = shell_command(cli_command, "hook", "claude-state")
    for event in HOOK_EVENTS:
        _append_hook(hooks, event, command_text)
    _append_hook(hooks, "Notification", command_text, NOTIFICATION_MATCHER)
    return [
        *retained,
        "--settings",
        json.dumps(settings, ensure_ascii=False, separators=(",", ":")),
        *prompt_tail,
    ]


def configure_claude_launch(
    *,
    home: Path,
    group_id: str,
    actor_id: str,
    command: Sequence[str],
    env: Mapping[str, str],
    cwd: Path,
    launch_token: str,
    version: tuple[int, int, int] | None,
    cccc_executable: Path | None = None,
    cccc_command: Sequence[str] | None = None,
) -> tuple[list[str], dict[str, str]]:
    configured = [str(item) for item in command]
    launch_env = {str(key): str(value) for key, value in env.items()}
    if (
        not is_direct_claude_command(configured)
        or version is None
        or version < MIN_CLAUDE_VERSION
    ):
        return configured, launch_env
    begin_launch(home, "claude", group_id, actor_id, launch_token)
    configured = append_claude_settings(
        configured,
        cwd=cwd,
        cccc_executable=cccc_executable,
        cccc_command=cccc_command,
    )
    launch_env.update(
        {
            "CCCC_HOME": str(home),
            "CCCC_GROUP_ID": group_id,
            "CCCC_ACTOR_ID": actor_id,
            "CCCC_HOOK_LAUNCH_TOKEN": launch_token,
            "CCCC_CLI": normalize_cli_command(
                cccc_command, cccc_executable
            )[0],
        }
    )
    return configured, launch_env
