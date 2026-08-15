from __future__ import annotations

import json
from pathlib import Path

from cccc.daemon.runtime_hooks.claude import (
    append_claude_settings,
    parse_claude_version,
)
from cccc.daemon.runtime_hooks.codex import configure_codex_launch


def test_codex_direct_command_gets_trusted_session_only_hooks(tmp_path: Path) -> None:
    command, env = configure_codex_launch(
        home=tmp_path,
        group_id="g1",
        actor_id="peer",
        command=["codex", "--search"],
        env={"PATH": "/usr/bin"},
        cccc_executable=Path("/opt/cccc bin/cccc"),
        launch_token="token",
    )
    assert command[:2] == ["codex", "--search"]
    assert any(item.startswith("hooks.UserPromptSubmit=") for item in command)
    assert any(item.startswith("hooks.PostToolUse=") for item in command)
    assert any(item.startswith("hooks.Stop=") for item in command)
    assert not any(item.startswith("hooks.PostToolUseFailure=") for item in command)
    assert not any(item.startswith("hooks.StopFailure=") for item in command)
    state = next(item for item in command if item.startswith("hooks.state="))
    assert "/<session-flags>/config.toml:session_start:0:0" in state
    assert "project" not in state
    assert "plugin" not in state
    assert "--dangerously-bypass-hook-trust" not in command
    assert env["CCCC_HOOK_LAUNCH_TOKEN"] == "token"


def test_wrapper_and_app_server_codex_are_not_eligible(tmp_path: Path) -> None:
    for command in (["wrapper", "codex"], ["codex", "app-server"]):
        configured, env = configure_codex_launch(
            home=tmp_path,
            group_id="g1",
            actor_id="peer",
            command=command,
            env={},
            cccc_executable=Path("/bin/cccc"),
            launch_token="token",
        )
        assert configured == command
        assert "CCCC_HOOK_LAUNCH_TOKEN" not in env


def test_codex_overrides_stay_before_prompt_tail(tmp_path: Path) -> None:
    command, _ = configure_codex_launch(
        home=tmp_path,
        group_id="g1",
        actor_id="peer",
        command=["codex", "--search", "--", "prompt"],
        env={},
        cccc_executable=Path("/bin/cccc"),
        launch_token="token",
    )

    separator = command.index("--")
    assert command[separator:] == ["--", "prompt"]
    assert "--dangerously-bypass-hook-trust" not in command[:separator]
    assert any(
        item.startswith("mcp_servers.cccc.command=")
        for item in command[:separator]
    )
    assert any(
        item.startswith("hooks.SessionStart=")
        for item in command[:separator]
    )


def test_claude_registers_tool_failure_hook(tmp_path: Path) -> None:
    command = append_claude_settings(
        ["claude"], cwd=tmp_path, cccc_executable=Path("/bin/cccc")
    )
    settings = json.loads(command[-1])

    assert settings["hooks"]["PostToolUseFailure"][0]["hooks"][0]["command"].endswith(
        "hook claude-state"
    )


def test_claude_settings_merge_preserves_existing_hooks_and_prompt_tail(
    tmp_path: Path,
) -> None:
    command = [
        "claude",
        "--settings",
        '{"language":"zh","hooks":{"Stop":[{"matcher":"existing"}]}}',
        "--",
        "--settings",
        "prompt text",
    ]
    configured = append_claude_settings(
        command, cwd=tmp_path, cccc_executable=Path("/bin/cccc")
    )
    settings = json.loads(configured[2])

    assert configured[:2] == ["claude", "--settings"]
    assert settings["language"] == "zh"
    assert settings["hooks"]["Stop"][0]["matcher"] == "existing"
    assert configured[3:] == ["--", "--settings", "prompt text"]
    assert parse_claude_version("2.1.141 (Claude Code)") == (2, 1, 141)
