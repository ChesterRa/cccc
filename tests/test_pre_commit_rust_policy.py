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


def test_rust_checks_isolate_fixture_git_commands_from_hook_repository(tmp_path: Path) -> None:
    import shutil
    import shlex
    import sys
    import textwrap

    # Cargo is the subprocess boundary; its stand-in performs real Git work so
    # this proves isolation without recompiling Rust in a Python tooling test.
    clean_env = os.environ.copy()
    local_vars = subprocess.check_output(
        ["git", "rev-parse", "--local-env-vars"], cwd=ROOT, text=True
    ).splitlines()
    for key in local_vars:
        clean_env.pop(key, None)
    project = tmp_path / "project"
    project.mkdir()
    subprocess.run(["git", "init", "-q", str(project)], env=clean_env, check=True)
    subprocess.run(
        ["git", "-c", "user.name=Fixture", "-c", "user.email=fixture@example.test",
         "commit", "--allow-empty", "-qm", "seed"], cwd=project, env=clean_env, check=True,
    )
    worktree = tmp_path / "linked"
    subprocess.run(
        ["git", "worktree", "add", "--detach", str(worktree)],
        cwd=project, env=clean_env, check=True, capture_output=True,
    )
    scripts = worktree / "scripts"
    scripts.mkdir()
    shutil.copy2(ROOT / "scripts/pre_commit_rust.sh", scripts / "pre_commit_rust.sh")
    binaries = tmp_path / "bin"
    binaries.mkdir()
    cargo = binaries / "cargo"
    cargo.write_text(
        f"#!{sys.executable}\n" + textwrap.dedent("""\
        import os
        import subprocess
        from pathlib import Path
        fixture = Path(os.environ["PRECOMMIT_GIT_FIXTURE"])
        fixture.mkdir(exist_ok=True)
        subprocess.run(["git", "init", "--bare", "-q"], cwd=fixture, check=True)
        assert subprocess.check_output(
            ["git", "--git-dir", str(fixture), "config", "--get", "core.bare"], text=True
        ).strip() == "true"
        """),
        encoding="utf-8",
    )
    cargo.chmod(0o755)
    hook_env = dict(
        clean_env,
        PATH=str(binaries) + os.pathsep + clean_env["PATH"],
        PRECOMMIT_GIT_FIXTURE=str(tmp_path / "fixture.git"),
    )
    hook = project / ".git/hooks/pre-commit"
    hook.write_text(
        "#!/bin/sh\nexec " + shlex.quote(str(scripts / "pre_commit_rust.sh")) + " -- Cargo.toml\n",
        encoding="utf-8",
    )
    hook.chmod(0o755)
    result = subprocess.run(
        ["git", "-c", "user.name=Fixture", "-c", "user.email=fixture@example.test",
         "commit", "--allow-empty", "-qm", "exercise real hook environment"], cwd=worktree,
        env=hook_env, text=True, capture_output=True,
    )
    assert result.returncode == 0, result.stdout + result.stderr
    assert subprocess.check_output(
        ["git", "--git-dir", str(project / ".git"), "config", "--get", "core.bare"],
        env=clean_env, text=True,
    ).strip() == "false"
