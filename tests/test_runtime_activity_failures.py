from __future__ import annotations

from pathlib import Path

from cccc.kernel.runtime_hooks.activity import read_events
from cccc.kernel.runtime_hooks.store import begin_launch, record_hook_event


def test_claude_tool_failure_closes_started_activity_with_duration(
    tmp_path: Path,
) -> None:
    begin_launch(tmp_path, "claude", "g1", "peer", "token")
    for payload in (
        {"hook_event_name": "SessionStart", "session_id": "session-1"},
        {
            "hook_event_name": "PreToolUse",
            "session_id": "session-1",
            "tool_use_id": "op-1",
            "tool_name": "Bash",
        },
        {
            "hook_event_name": "PostToolUseFailure",
            "session_id": "session-1",
            "tool_use_id": "op-1",
        },
    ):
        record_hook_event(
            tmp_path, "claude", "g1", "peer", "token", payload
        )

    tool = next(event for event in read_events(tmp_path, "g1") if event.kind == "tool")
    assert tool.status == "failed"
    assert tool.event_type == "PostToolUseFailure"
    assert tool.tool_name == "Bash"
    assert tool.duration_ms is not None
