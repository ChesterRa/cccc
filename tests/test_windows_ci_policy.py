from pathlib import Path

import yaml


ROOT = Path(__file__).resolve().parents[1]


def test_windows_smoke_keeps_focused_native_checks() -> None:
    workflow = yaml.load(
        (ROOT / ".github/workflows/ci.yml").read_text(encoding="utf-8"),
        Loader=yaml.BaseLoader,
    )
    windows = workflow["jobs"]["windows-smoke"]
    runs = "\n".join(step.get("run", "") for step in windows["steps"])
    uses = {step.get("uses", "") for step in windows["steps"]}

    assert windows["needs"] == "web"
    assert "cargo build" not in runs
    assert "install_windows.ps1" not in runs
    assert any(item.startswith("actions/download-artifact") for item in uses)
    assert any(item.startswith("dtolnay/rust-toolchain") for item in uses)
    assert any(item.startswith("Swatinem/rust-cache") for item in uses)
    assert "cargo test --package cccc-pair-daemon --lib --locked" in runs
    assert (
        "process_tree::tests::abrupt_daemon_exit_reaps_child_and_grandchild_without_deleting_history"
        in runs
    )
    assert "cargo test --package cccc-pair-runtime --lib --locked" in runs
    assert (
        "manager_windows_tests::npm_style_batch_actor_survives_utf8_message_delivery"
        in runs
    )
    assert "cargo test --package cccc --bin cccc --locked" in runs
    assert (
        "console_encoding::tests::console_uses_utf8_for_cli_lifetime_and_restores_both_original_pages"
        in runs
    )
    assert "-- --test-threads=1" in runs
    # Node runs the offline npm entrypoint fixture; Web is still prebuilt and
    # this focused job does not download model runtimes or install npm packages.
    steps = windows["steps"]
    setup = next(step for step in steps if step.get("uses", "").startswith("actions/setup-node"))
    smoke = next(step for step in steps if step.get("name") == "Verify Windows Kilo npm managed launch")
    assert setup["with"]["node-version"] == "24.19.0"
    assert steps.index(setup) < steps.index(smoke)
    assert smoke["timeout-minutes"] == "3"
    assert "cargo test --package cccc-pair-daemon --lib --locked" in smoke["run"]
    assert "opencode::launch_tests:: -- --test-threads=1" in smoke["run"]
    assert "secrets." not in str(smoke)
    assert not any(item.startswith("actions/setup-python") for item in uses)
    assert "npm " not in runs
    assert "python " not in runs.lower()


def test_windows_smoke_executes_owned_process_regressions() -> None:
    import shlex

    workflow = yaml.load(
        (ROOT / ".github/workflows/ci.yml").read_text(encoding="utf-8"),
        Loader=yaml.BaseLoader,
    )
    commands = [
        shlex.split(step["run"])
        for step in workflow["jobs"]["windows-smoke"]["steps"]
        if "run" in step
    ]
    for package, test_filter in [
        ("cccc-windows-process", None),
        ("cccc-pair-runtime", "process_tree::windows_tests::"),
    ]:
        matching = [
            args for args in commands
            if args[:2] == ["cargo", "test"]
            and args[args.index("--package") + 1] == package
            and (test_filter is None or test_filter in args)
        ]
        assert len(matching) == 1, (package, test_filter)
        args = matching[0]
        assert "--lib" in args and "--locked" in args
        assert args[args.index("--") + 1:] == ["--test-threads=1"]
