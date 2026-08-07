from __future__ import annotations

import os
from pathlib import Path

from cccc.cli.installation_diagnostics import find_cccc_commands, inspect_cccc_installation


def _write_command(path: Path) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("#!/bin/sh\nexit 0\n", encoding="utf-8")
    path.chmod(0o755)


def test_installation_diagnostics_reports_an_older_command_ahead_of_launcher(tmp_path: Path) -> None:
    current = tmp_path / "current" / "cccc"
    older = tmp_path / "older" / "cccc"
    _write_command(current)
    _write_command(older)
    environment = {
        "PATH": os.pathsep.join((str(older.parent), str(current.parent))),
        "CCCC_LAUNCHER_PATH": str(current),
    }

    report = inspect_cccc_installation(environ=environment)

    assert report["current_executable"] == str(current)
    assert report["resolved_command"] == str(older)
    assert report["path_status"] == "conflict"
    assert report["path_conflict"] is True
    assert report["conflicting_commands"] == [str(older)]


def test_installation_diagnostics_keeps_non_active_duplicates_visible(tmp_path: Path) -> None:
    current = tmp_path / "current" / "cccc"
    older = tmp_path / "older" / "cccc"
    _write_command(current)
    _write_command(older)
    environment = {
        "PATH": os.pathsep.join((str(current.parent), str(older.parent), str(current.parent))),
        "CCCC_LAUNCHER_PATH": str(current),
    }

    report = inspect_cccc_installation(environ=environment)

    assert report["path_status"] == "ok"
    assert report["path_conflict"] is False
    assert report["command_candidates"] == [str(current), str(older)]
    assert report["conflicting_commands"] == [str(older)]
    assert find_cccc_commands(environ=environment) == [current, older]
