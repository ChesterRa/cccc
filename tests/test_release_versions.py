from __future__ import annotations

import importlib.util
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch

import pytest

REPO_ROOT = Path(__file__).resolve().parents[1]
SCRIPT = REPO_ROOT / "scripts" / "check_release_versions.py"
SPEC = importlib.util.spec_from_file_location("cccc_check_release_versions", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


def _write_workspace(
    root: Path,
    *,
    version: str = "0.4.61",
    member_version_line: str = "version.workspace = true",
    dep_version: str = "=0.4.61",
    target_dep_version: str = "=0.4.61",
    lock_core_version: str | None = None,
) -> None:
    (root / "pyproject.toml").write_text(
        f'[project]\nname = "cccc-pair"\nversion = "{version}"\n',
        encoding="utf-8",
    )
    (root / "Cargo.toml").write_text(
        "[workspace]\n"
        'members = ["crates/cccc-core", "crates/cccc-proxy", "crates/cccc-win", "crates/cccc-cli"]\n\n'
        "[workspace.package]\n"
        f'version = "{version}"\n',
        encoding="utf-8",
    )
    (root / "crates" / "cccc-core").mkdir(parents=True)
    (root / "crates" / "cccc-core" / "Cargo.toml").write_text(
        "[package]\n"
        'name = "cccc-pair-core"\n'
        f"{member_version_line}\n\n"
        '[target.\'cfg(windows)\'.dependencies]\n'
        'cccc-win = { path = "../cccc-win" }\n',
        encoding="utf-8",
    )
    (root / "crates" / "cccc-proxy").mkdir(parents=True)
    (root / "crates" / "cccc-proxy" / "Cargo.toml").write_text(
        '[package]\nname = "cccc-proxy"\nversion.workspace = true\n',
        encoding="utf-8",
    )
    (root / "crates" / "cccc-win").mkdir(parents=True)
    (root / "crates" / "cccc-win" / "Cargo.toml").write_text(
        '[package]\nname = "cccc-win"\nversion.workspace = true\n',
        encoding="utf-8",
    )
    (root / "crates" / "cccc-cli").mkdir(parents=True)
    (root / "crates" / "cccc-cli" / "Cargo.toml").write_text(
        '[package]\nname = "cccc"\nversion.workspace = true\n\n'
        "[dependencies]\n"
        f'cccc-core = {{ package = "cccc-pair-core", version = "{dep_version}", path = "../cccc-core" }}\n\n'
        '[target.\'cfg(any(target_os = "macos", windows))\'.dependencies]\n'
        f'cccc-proxy = {{ version = "{target_dep_version}", path = "../cccc-proxy" }}\n',
        encoding="utf-8",
    )
    core_lock = lock_core_version if lock_core_version is not None else version
    (root / "Cargo.lock").write_text(
        "version = 4\n\n"
        f'[[package]]\nname = "cccc-pair-core"\nversion = "{core_lock}"\n\n'
        f'[[package]]\nname = "cccc-proxy"\nversion = "{version}"\n\n'
        f'[[package]]\nname = "cccc-win"\nversion = "{version}"\n\n'
        f'[[package]]\nname = "cccc"\nversion = "{version}"\n',
        encoding="utf-8",
    )


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


def test_repository_workspace_version_parity_passes() -> None:
    assert MODULE.main(["--root", str(REPO_ROOT)]) == 0


def test_workspace_parity_accepts_inherited_members_exact_pins_and_path_only_deps(tmp_path: Path) -> None:
    _write_workspace(tmp_path)
    assert MODULE.main(["--root", str(tmp_path)]) == 0


def test_workspace_parity_rejects_member_without_workspace_version_inheritance(tmp_path: Path) -> None:
    _write_workspace(tmp_path, member_version_line='version = "0.4.60"')
    with pytest.raises(ValueError, match="must inherit workspace.package.version"):
        MODULE.main(["--root", str(tmp_path)])


def test_workspace_parity_rejects_stale_internal_dependency_pin(tmp_path: Path) -> None:
    _write_workspace(tmp_path, dep_version="=0.4.60")
    with pytest.raises(ValueError, match="internal dependency version mismatch.*cccc-core"):
        MODULE.main(["--root", str(tmp_path)])


def test_workspace_parity_rejects_stale_target_internal_dependency_pin(tmp_path: Path) -> None:
    _write_workspace(tmp_path, target_dep_version="=0.4.60")
    with pytest.raises(ValueError, match="internal dependency version mismatch.*cccc-proxy"):
        MODULE.main(["--root", str(tmp_path)])


def test_workspace_parity_rejects_stale_lockfile_member_version(tmp_path: Path) -> None:
    _write_workspace(tmp_path, lock_core_version="0.4.60")
    with pytest.raises(ValueError, match="lockfile version mismatch.*cccc-pair-core"):
        MODULE.main(["--root", str(tmp_path)])


def test_explicit_version_flags_validate_strings_without_reading_workspace(tmp_path: Path) -> None:
    assert (
        MODULE.main(
            [
                "--root",
                str(tmp_path),
                "--package-version",
                "0.5.0rc3",
                "--rust-version",
                "0.5.0-rc3",
                "--tag",
                "v0.5.0-rc3",
            ]
        )
        == 0
    )
