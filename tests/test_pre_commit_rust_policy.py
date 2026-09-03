from __future__ import annotations

import os
import subprocess
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


def _rust_precommit_plan(*changed_files: str) -> dict[str, str]:
    env = os.environ.copy()
    env.pop("CCCC_GROUP_ID", None)
    env.pop("CCCC_ACTOR_ID", None)
    result = subprocess.run(
        ["scripts/pre_commit_rust.sh", "--dry-run", "--", *changed_files],
        cwd=ROOT,
        env=env,
        check=True,
        capture_output=True,
        text=True,
    )
    return dict(
        line.split("=", 1) for line in result.stdout.splitlines() if "=" in line
    )


def test_process_global_runtime_tests_are_isolated_from_workspace_parallelism() -> None:
    workspace = _rust_precommit_plan("Cargo.toml")
    daemon = _rust_precommit_plan(
        "crates/cccc-daemon/src/lib.rs",
        "crates/cccc-daemon/tests/message_delivery.rs",
    )
    runtime = _rust_precommit_plan(
        "crates/cccc-runtime/src/lib.rs",
        "crates/cccc-runtime/tests/terminal_replay.rs",
    )

    for commands in (workspace, daemon, runtime):
        assert (
            "cargo test --workspace --exclude cccc-pair-daemon --exclude cccc-pair-runtime --locked"
            in commands["rust_test"]
        )
        assert (
            "cargo test --package cccc-pair-runtime --locked"
            in commands["rust_runtime_test"]
        )
        assert "-- --test-threads=1" in commands["rust_runtime_test"]
        assert (
            "cargo test --package cccc-pair-daemon --locked"
            in commands["rust_daemon_test"]
        )
        assert "-- --test-threads=1" in commands["rust_daemon_test"]
    changed_test = daemon["rust_changed_test[cccc-pair-daemon:message_delivery]"]
    assert (
        "cargo test --package cccc-pair-daemon --test message_delivery --locked"
        in changed_test
    )
    assert "-- --test-threads=1" in changed_test
    changed_runtime_test = runtime[
        "rust_changed_test[cccc-pair-runtime:terminal_replay]"
    ]
    assert (
        "cargo test --package cccc-pair-runtime --test terminal_replay --locked"
        in changed_runtime_test
    )
    assert "-- --test-threads=1" in changed_runtime_test
