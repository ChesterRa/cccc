from __future__ import annotations

import io
from pathlib import Path

import pytest

from cccc.cli.runtime_hook_cmd import run_runtime_hook
from cccc.kernel.runtime_hooks.store import begin_launch, read_state


def _env(home: Path) -> dict[str, str]:
    return {
        "CCCC_HOME": str(home),
        "CCCC_GROUP_ID": "g1",
        "CCCC_ACTOR_ID": "peer",
        "CCCC_HOOK_LAUNCH_TOKEN": "token",
    }


def test_hidden_receiver_accepts_bounded_strict_json(tmp_path: Path) -> None:
    begin_launch(tmp_path, "codex", "g1", "peer", "token")
    payload = io.BytesIO(
        b'{"hook_event_name":"SessionStart","session_id":"session-1"}'
    )
    assert run_runtime_hook("codex-state", payload, _env(tmp_path)) == 0
    state = read_state(tmp_path, "codex", "g1", "peer")
    assert state is not None and state.session_id == "session-1"


def test_hidden_receiver_rejects_missing_env_and_oversized_json(tmp_path: Path) -> None:
    with pytest.raises(ValueError, match="CCCC_ACTOR_ID"):
        run_runtime_hook(
            "codex-state",
            io.BytesIO(b"{}"),
            {key: value for key, value in _env(tmp_path).items() if key != "CCCC_ACTOR_ID"},
        )
    with pytest.raises(ValueError, match="too large"):
        run_runtime_hook(
            "claude-state",
            io.BytesIO(b"{" + b" " * (1024 * 1024) + b"}"),
            _env(tmp_path),
        )
