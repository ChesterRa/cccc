from __future__ import annotations

from pathlib import Path

import pytest

from scripts.verify_python_release_set import MAX_WHEEL_BYTES, WHEEL_SUFFIXES, verify


PREFIX = "cccc_pair-0.4.34rc2"


def _complete_set(directory: Path) -> None:
    for suffix in WHEEL_SUFFIXES:
        directory.joinpath(f"{PREFIX}{suffix}").touch()
    directory.joinpath(f"{PREFIX}.tar.gz").touch()


def test_accepts_one_sdist_universal_wheel_and_four_native_wheels(tmp_path: Path) -> None:
    _complete_set(tmp_path)

    verify(tmp_path)


def test_rejects_an_incomplete_distribution_set(tmp_path: Path) -> None:
    _complete_set(tmp_path)
    tmp_path.joinpath(f"{PREFIX}-py3-none-win_amd64.whl").unlink()

    with pytest.raises(ValueError, match="exactly one sdist and five wheels"):
        verify(tmp_path)


def test_rejects_mixed_versions(tmp_path: Path) -> None:
    _complete_set(tmp_path)
    windows = tmp_path.joinpath(f"{PREFIX}-py3-none-win_amd64.whl")
    windows.rename(tmp_path / "cccc_pair-9.9.9-py3-none-win_amd64.whl")

    with pytest.raises(ValueError, match="prefixes do not match"):
        verify(tmp_path)


def test_rejects_a_mismatched_sdist(tmp_path: Path) -> None:
    _complete_set(tmp_path)
    sdist = tmp_path.joinpath(f"{PREFIX}.tar.gz")
    sdist.rename(tmp_path / "cccc_pair-9.9.9.tar.gz")

    with pytest.raises(ValueError, match="expected sdist"):
        verify(tmp_path)


def test_rejects_an_oversized_wheel(tmp_path: Path) -> None:
    _complete_set(tmp_path)
    wheel = tmp_path.joinpath(f"{PREFIX}-py3-none-any.whl")
    with wheel.open("wb") as handle:
        handle.truncate(MAX_WHEEL_BYTES)

    with pytest.raises(ValueError, match="wheel exceeds"):
        verify(tmp_path)
