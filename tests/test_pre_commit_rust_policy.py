from __future__ import annotations

import os
import subprocess
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


def test_production_roots_enforce_audited_process_constructors() -> None:
    roots = sorted(ROOT.glob("crates/*/src/lib.rs")) + sorted(ROOT.glob("crates/*/src/main.rs"))
    assert roots
    for root in roots:
        assert "#![cfg_attr(not(test), deny(clippy::disallowed_methods))]" in root.read_text(), root


def _stand_in_cargo(directory: Path, *, nextest: bool, failing: str = "") -> Path:
    """Put a cargo on PATH that records each call and answers whether nextest
    is installed; a call whose arguments start with `failing` exits 1."""
    import sys
    import textwrap

    directory.mkdir(parents=True, exist_ok=True)
    cargo = directory / "cargo"
    cargo.write_text(
        f"#!{sys.executable}\n" + textwrap.dedent(f"""\
        import os
        import sys
        args = " ".join(sys.argv[1:])
        if args == "nextest --version":
            sys.exit(0 if {nextest!r} else 101)
        with open(os.environ["CARGO_CALLS"], "a", encoding="utf-8") as calls:
            calls.write(args + "\\n")
        sys.exit(1 if {failing!r} and args.startswith({failing!r}) else 0)
        """),
        encoding="utf-8",
    )
    cargo.chmod(0o755)
    return directory


def _hook_env(binaries: Path, calls: Path) -> dict[str, str]:
    env = os.environ.copy()
    for key in [key for key in env if key.startswith("CCCC_")]:
        env.pop(key)
    env["PATH"] = str(binaries) + os.pathsep + env["PATH"]
    env["CARGO_CALLS"] = str(calls)
    return env


def _rust_precommit_plan(
    tmp_path: Path, *changed_files: str, nextest: bool = False
) -> dict[str, str]:
    binaries = _stand_in_cargo(tmp_path / "bin", nextest=nextest)
    result = subprocess.run(
        ["scripts/pre_commit_rust.sh", "--dry-run", "--", *changed_files],
        cwd=ROOT,
        env=_hook_env(binaries, tmp_path / "calls"),
        check=True,
        capture_output=True,
        text=True,
    )
    return dict(
        line.split("=", 1) for line in result.stdout.splitlines() if "=" in line
    )


def test_process_global_runtime_tests_are_isolated_from_workspace_parallelism(
    tmp_path: Path,
) -> None:
    workspace = _rust_precommit_plan(tmp_path, "Cargo.toml")
    daemon = _rust_precommit_plan(
        tmp_path,
        "crates/cccc-daemon/src/lib.rs",
        "crates/cccc-daemon/tests/message_delivery.rs",
    )
    runtime = _rust_precommit_plan(
        tmp_path,
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
    # cargo test runs the daemon doctests itself on this path.
    assert "rust_daemon_doc_test" not in daemon


def test_daemon_suite_uses_nextest_like_ci_when_it_is_installed(tmp_path: Path) -> None:
    fallback = _rust_precommit_plan(tmp_path / "without", "Cargo.toml")
    plan = _rust_precommit_plan(tmp_path / "with", "Cargo.toml", nextest=True)

    # One process per test, as many at once as CI's 4-core runners.
    assert plan["rust_daemon_test"].split() == [
        "cargo", "nextest", "run", "--package", "cccc-pair-daemon", "--locked",
        "--build-jobs", "2", "--test-threads", "4",
    ]
    # nextest does not run doctests, so the hook still does.
    assert plan["rust_daemon_doc_test"].split() == [
        "cargo", "test", "--package", "cccc-pair-daemon", "--doc", "--locked", "--jobs", "2",
    ]
    # Everything outside the daemon suite is planned exactly as before.
    for key in ("rust_clippy", "rust_test", "rust_runtime_test"):
        assert plan[key] == fallback[key]


def test_a_failing_nextest_run_fails_the_hook_without_falling_back(tmp_path: Path) -> None:
    def run(failing: str) -> tuple[subprocess.CompletedProcess[str], list[str]]:
        case = tmp_path / (failing.replace(" ", "-") or "passing")
        case.mkdir()
        binaries = _stand_in_cargo(case / "bin", nextest=True, failing=failing)
        calls = case / "calls"
        result = subprocess.run(
            ["scripts/pre_commit_rust.sh", "--", "crates/cccc-daemon/src/lib.rs"],
            cwd=ROOT,
            env=_hook_env(binaries, calls),
            capture_output=True,
            text=True,
        )
        return result, calls.read_text(encoding="utf-8").splitlines()

    passed, passed_calls = run("")
    assert passed.returncode == 0, passed.stdout + passed.stderr
    def daemon_runs(calls: list[str]) -> list[str]:
        return [
            call for call in calls
            if call.startswith(("nextest run --package cccc-pair-daemon",
                                "test --package cccc-pair-daemon"))
        ]

    assert daemon_runs(passed_calls) == [
        "nextest run --package cccc-pair-daemon --locked --build-jobs 2 --test-threads 4",
        "test --package cccc-pair-daemon --doc --locked --jobs 2",
    ]

    failed, failed_calls = run("nextest run")
    assert failed.returncode != 0
    # Nothing ran after the failure: no cargo test retry, no doctests.
    assert daemon_runs(failed_calls) == [
        "nextest run --package cccc-pair-daemon --locked --build-jobs 2 --test-threads 4"
    ]

    failed_docs, _ = run("test --package cccc-pair-daemon --doc")
    assert failed_docs.returncode != 0


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
