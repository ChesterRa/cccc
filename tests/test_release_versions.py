from __future__ import annotations

import importlib.util
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch


SCRIPT = Path(__file__).resolve().parents[1] / "scripts" / "check_release_versions.py"
SPEC = importlib.util.spec_from_file_location("cccc_check_release_versions", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


def test_rust_binary_version_accepts_semver_prerelease(tmp_path: Path) -> None:
    binary = tmp_path / "cccc-rust"
    binary.write_bytes(b"native")
    completed = SimpleNamespace(returncode=0, stdout="cccc 0.4.34-rc1\n", stderr="")
    with patch.object(MODULE.subprocess, "run", return_value=completed):
        assert MODULE._rust_binary_version(binary) == "0.4.34-rc1"


def test_rust_binary_version_rejects_failed_probe(tmp_path: Path) -> None:
    binary = tmp_path / "cccc-rust"
    binary.write_bytes(b"native")
    completed = SimpleNamespace(returncode=2, stdout="", stderr="boom")
    with patch.object(MODULE.subprocess, "run", return_value=completed):
        try:
            MODULE._rust_binary_version(binary)
        except ValueError as error:
            assert "exit code 2" in str(error)
        else:
            raise AssertionError("failed binary probe must be rejected")
