from __future__ import annotations

from pathlib import Path
from unittest.mock import patch

import os
import sys

from scripts.tests.smoke_wheel_frontdoor import _process_is_running, _run


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
