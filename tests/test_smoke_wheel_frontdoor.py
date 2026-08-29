from __future__ import annotations

from pathlib import Path
from unittest.mock import patch

import os
import subprocess
import sys

import pytest

from scripts.tests.smoke_wheel_frontdoor import _process_is_running, _run, _wait_for_child_exit


def test_run_captures_combined_output_without_a_pipe() -> None:
    completed = _run(
        [sys.executable, "-c", "import sys; print('out'); print('err', file=sys.stderr)"],
        env=os.environ.copy(),
    )

    assert completed.returncode == 0
    assert sorted(completed.stdout.splitlines()) == ["err", "out"]


def test_linux_zombie_is_treated_as_exited() -> None:
    stat = "9659 (cccc daemon) Z 1 9659 9659 0 -1 4227084"

    with (
        patch("scripts.tests.smoke_wheel_frontdoor.os.name", "posix"),
        patch("scripts.tests.smoke_wheel_frontdoor.sys.platform", "linux"),
        patch("scripts.tests.smoke_wheel_frontdoor.os.kill"),
        patch.object(Path, "read_text", return_value=stat),
    ):
        assert not _process_is_running(9659)


@pytest.mark.skipif(os.name == "nt", reason="POSIX zombie semantics")
def test_direct_child_is_reaped_instead_of_polling_its_pid_on_macos() -> None:
    process = subprocess.Popen(
        [sys.executable, "-c", "print('done')"],
        stdout=subprocess.PIPE,
        text=True,
    )
    try:
        assert process.stdout is not None
        assert process.stdout.read() == "done\n"
        with patch("scripts.tests.smoke_wheel_frontdoor.sys.platform", "darwin"):
            assert _process_is_running(process.pid)
            _wait_for_child_exit(process, timeout=1.0)
            assert not _process_is_running(process.pid)
    finally:
        if process.poll() is None:
            process.kill()
            process.wait(timeout=1.0)
