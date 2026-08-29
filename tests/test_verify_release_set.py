from __future__ import annotations

import hashlib
import io
import stat
import tarfile
import zipfile
from pathlib import Path

import pytest

from scripts.build_native_wheel import build
from scripts.verify_release_set import (
    ARCHIVE_SUFFIXES,
    MAX_WHEEL_BYTES,
    SUPPORT_FILES,
    WHEEL_TARGETS,
    verify,
)


CARGO_VERSION = "0.4.36-rc1"
WHEEL_VERSION = "0.4.36rc1"


def _archive(path: Path, *, package: str, binary: bytes, windows: bool) -> None:
    member = f"{package}/{'cccc.exe' if windows else 'cccc'}"
    files = {
        member: binary,
        f"{package}/LICENSE": b"license\n",
        f"{package}/README.md": b"readme\n",
        f"{package}/rust-migration.md": b"migration\n",
    }
    if windows:
        with zipfile.ZipFile(path, "w") as archive:
            archive.writestr(f"{package}/", b"")
            for name, data in files.items():
                archive.writestr(name, data)
        return
    with tarfile.open(path, "w:gz") as archive:
        directory = tarfile.TarInfo(package)
        directory.type = tarfile.DIRTYPE
        directory.mode = 0o755
        archive.addfile(directory)
        for name, data in files.items():
            info = tarfile.TarInfo(name)
            info.size = len(data)
            info.mode = 0o755 if name == member else 0o644
            archive.addfile(info, io.BytesIO(data))


def _complete_set(directory: Path) -> None:
    directory.joinpath("pyproject.toml").write_text(
        f'''[project]
name = "cccc-pair"
version = "{WHEEL_VERSION}"
description = "release fixture"
readme = {{ file = "README.md", content-type = "text/markdown" }}
''',
        encoding="utf-8",
    )
    directory.joinpath("LICENSE").write_text("fixture\n", encoding="utf-8")
    directory.joinpath("README.md").write_text(
        "# Release fixture\n", encoding="utf-8"
    )
    payloads: list[Path] = []
    for index, (platform, target) in enumerate(WHEEL_TARGETS.items()):
        executable = "cccc.exe" if platform == "win_amd64" else "cccc"
        binary = directory / f"binary-{index}-{executable}"
        binary.write_bytes(f"{target}-binary".encode())
        wheel = build(binary, directory, platform_tag=platform, root=directory)
        payloads.append(wheel)
        package = f"cccc-v{CARGO_VERSION}-{target}"
        archive = directory / f"{package}{ARCHIVE_SUFFIXES[target]}"
        _archive(
            archive,
            package=package,
            binary=binary.read_bytes(),
            windows=platform == "win_amd64",
        )
        payloads.append(archive)
        binary.unlink()

    directory.joinpath("install.sh").write_text(
        f'#!/usr/bin/env sh\nDEFAULT_VERSION="{CARGO_VERSION}"\n', encoding="utf-8"
    )
    directory.joinpath("install.ps1").write_text(
        f'[CmdletBinding()]\n$defaultVersion = "{CARGO_VERSION}"\n', encoding="utf-8"
    )
    lines = [
        f"{hashlib.sha256(path.read_bytes()).hexdigest()}  {path.name}"
        for path in sorted(payloads)
    ]
    directory.joinpath("SHA256SUMS").write_text(
        "\n".join(lines) + "\n", encoding="utf-8"
    )
    directory.joinpath("pyproject.toml").unlink()
    directory.joinpath("LICENSE").unlink()
    directory.joinpath("README.md").unlink()


def test_accepts_four_wheels_and_four_byte_identical_archives(tmp_path: Path) -> None:
    _complete_set(tmp_path)
    verify(tmp_path, cargo_version=CARGO_VERSION, wheel_version=WHEEL_VERSION)


def test_rejects_an_incomplete_distribution_set(tmp_path: Path) -> None:
    _complete_set(tmp_path)
    tmp_path.joinpath(f"cccc_pair-{WHEEL_VERSION}-py3-none-win_amd64.whl").unlink()

    with pytest.raises(ValueError, match="exactly four wheels"):
        verify(tmp_path, cargo_version=CARGO_VERSION, wheel_version=WHEEL_VERSION)


def test_rejects_different_wheel_and_archive_binaries(tmp_path: Path) -> None:
    _complete_set(tmp_path)
    target = "x86_64-pc-windows-msvc"
    package = f"cccc-v{CARGO_VERSION}-{target}"
    archive = tmp_path / f"{package}.zip"
    _archive(archive, package=package, binary=b"different", windows=True)
    lines = []
    for name in (tmp_path / "SHA256SUMS").read_text(encoding="utf-8").splitlines():
        if name.endswith(archive.name):
            name = f"{hashlib.sha256(archive.read_bytes()).hexdigest()}  {archive.name}"
        lines.append(name)
    (tmp_path / "SHA256SUMS").write_text("\n".join(lines) + "\n", encoding="utf-8")

    with pytest.raises(ValueError, match="different CCCC executable bytes"):
        verify(tmp_path, cargo_version=CARGO_VERSION, wheel_version=WHEEL_VERSION)


def test_rejects_a_checksum_mismatch(tmp_path: Path) -> None:
    _complete_set(tmp_path)
    checksum = tmp_path / "SHA256SUMS"
    lines = checksum.read_text(encoding="utf-8").splitlines()
    digest, filename = lines[0].split("  ", 1)
    replacement = "0" if digest[0] != "0" else "1"
    lines[0] = f"{replacement}{digest[1:]}  {filename}"
    checksum.write_text("\n".join(lines) + "\n", encoding="utf-8")

    with pytest.raises(ValueError, match="SHA256SUMS mismatch"):
        verify(tmp_path, cargo_version=CARGO_VERSION, wheel_version=WHEEL_VERSION)


def test_rejects_an_archive_with_an_unexpected_member(tmp_path: Path) -> None:
    _complete_set(tmp_path)
    target = "x86_64-pc-windows-msvc"
    package = f"cccc-v{CARGO_VERSION}-{target}"
    archive_path = tmp_path / f"{package}.zip"
    with zipfile.ZipFile(archive_path, "a") as archive:
        archive.writestr(f"{package}/unexpected.dll", b"foreign")
    lines = []
    for line in (tmp_path / "SHA256SUMS").read_text(encoding="utf-8").splitlines():
        if line.endswith(archive_path.name):
            line = f"{hashlib.sha256(archive_path.read_bytes()).hexdigest()}  {archive_path.name}"
        lines.append(line)
    (tmp_path / "SHA256SUMS").write_text("\n".join(lines) + "\n", encoding="utf-8")

    with pytest.raises(ValueError, match="invalid archive layout"):
        verify(tmp_path, cargo_version=CARGO_VERSION, wheel_version=WHEEL_VERSION)


def test_rejects_a_windows_archive_symlink_member(tmp_path: Path) -> None:
    _complete_set(tmp_path)
    target = "x86_64-pc-windows-msvc"
    package = f"cccc-v{CARGO_VERSION}-{target}"
    archive_path = tmp_path / f"{package}.zip"
    with zipfile.ZipFile(archive_path) as archive:
        entries = [(item, archive.read(item.filename)) for item in archive.infolist()]
    with zipfile.ZipFile(archive_path, "w") as archive:
        for item, data in entries:
            if item.filename == f"{package}/cccc.exe":
                item.create_system = 3
                item.external_attr = (stat.S_IFLNK | 0o777) << 16
            archive.writestr(item, data)
    lines = []
    for line in (tmp_path / "SHA256SUMS").read_text(encoding="utf-8").splitlines():
        if line.endswith(archive_path.name):
            line = (
                f"{hashlib.sha256(archive_path.read_bytes()).hexdigest()}  "
                f"{archive_path.name}"
            )
        lines.append(line)
    (tmp_path / "SHA256SUMS").write_text("\n".join(lines) + "\n", encoding="utf-8")

    with pytest.raises(ValueError, match="is not a regular file"):
        verify(tmp_path, cargo_version=CARGO_VERSION, wheel_version=WHEEL_VERSION)


def test_rejects_an_oversized_wheel_before_opening_it(tmp_path: Path) -> None:
    _complete_set(tmp_path)
    wheel = tmp_path / f"cccc_pair-{WHEEL_VERSION}-py3-none-win_amd64.whl"
    with wheel.open("ab") as stream:
        stream.truncate(MAX_WHEEL_BYTES)

    with pytest.raises(ValueError, match="PyPI"):
        verify(tmp_path, cargo_version=CARGO_VERSION, wheel_version=WHEEL_VERSION)


def test_rejects_an_installer_bound_to_another_release(tmp_path: Path) -> None:
    _complete_set(tmp_path)
    installer = tmp_path / "install.sh"
    installer.write_text(
        installer.read_text(encoding="utf-8").replace(CARGO_VERSION, "9.9.9"),
        encoding="utf-8",
    )

    with pytest.raises(ValueError, match="install.sh is not bound"):
        verify(tmp_path, cargo_version=CARGO_VERSION, wheel_version=WHEEL_VERSION)


def test_support_file_contract_is_explicit() -> None:
    assert SUPPORT_FILES == {"SHA256SUMS", "install.sh", "install.ps1"}
