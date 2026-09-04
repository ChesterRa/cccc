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
    assert not any(item.startswith("actions/setup-node") for item in uses)
    assert not any(item.startswith("actions/setup-python") for item in uses)
    assert "npm " not in runs
    assert "python " not in runs.lower()
