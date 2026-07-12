from __future__ import annotations

import json
import tomllib
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


def test_web_package_exposes_full_test_and_zero_warning_lint_commands() -> None:
    package = json.loads((ROOT / "web/package.json").read_text(encoding="utf-8"))

    assert package["scripts"]["test"] == "vitest run"
    assert "--max-warnings=0" in package["scripts"]["lint"]


def test_agent_terminal_initial_snapshot_does_not_replace_live_option_updates() -> None:
    source = (ROOT / "web/src/components/AgentTab.tsx").read_text(encoding="utf-8")

    assert "terminalOptionsSnapshotRef" in source
    assert "terminalOptionsSnapshotRef.current.isDark = isDark" in source
    assert "terminalOptionsSnapshotRef.current.scrollbackLines = terminalScrollbackLines" in source
    assert "theme: getTerminalTheme(terminalOptionsSnapshotRef.current.isDark)" in source
    assert "scrollback: terminalOptionsSnapshotRef.current.scrollbackLines || 8000" in source
    assert "terminalRef.current.options.theme = getTerminalTheme(isDark)" in source
    assert "terminalRef.current.options.scrollback = terminalScrollbackLines" in source
    assert "}, [isDark]);" in source
    assert "}, [terminalScrollbackLines]);" in source


def test_ruff_is_limited_to_error_level_rules() -> None:
    config = tomllib.loads((ROOT / "pyproject.toml").read_text(encoding="utf-8"))

    assert config["tool"]["ruff"]["lint"]["select"] == ["E9", "F63", "F7", "F82"]


def test_local_fast_gate_reuses_quality_tools_without_running_full_python_suite() -> None:
    source = (ROOT / "scripts/quality_gate.sh").read_text(encoding="utf-8")
    fast_block = source.split("fast)", 1)[1].split(";;", 1)[0]

    assert "scripts/quality/source_size.py" in fast_block
    assert "ruff check" in fast_block
    assert "scripts/pre_commit_checks.sh" in fast_block
    assert "pytest tests/" not in fast_block


def test_full_precommit_path_does_not_use_xdist_auto_workers() -> None:
    source = (ROOT / "scripts/pre_commit_checks.sh").read_text(encoding="utf-8")

    assert "pytest-xdist" not in source
    assert "PYTEST_WORKERS" not in source
    assert 'python -m pytest tests/ "${pytest_common[@]}"' in source
    assert source.count("env -u CCCC_GROUP_ID -u CCCC_ACTOR_ID") >= 2
