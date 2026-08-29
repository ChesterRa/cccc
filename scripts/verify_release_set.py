#!/usr/bin/env python3
"""Verify the complete Rust-only CCCC release artifact set."""

from __future__ import annotations

import argparse
import hashlib
import re
import stat
import tarfile
import tomllib
import zipfile
from pathlib import Path

if __package__:
    from .verify_native_wheel import verify as verify_wheel
else:
    from verify_native_wheel import verify as verify_wheel


WHEEL_TARGETS = {
    "manylinux_2_28_x86_64": "x86_64-unknown-linux-gnu",
    "macosx_11_0_x86_64": "x86_64-apple-darwin",
    "macosx_11_0_arm64": "aarch64-apple-darwin",
    "win_amd64": "x86_64-pc-windows-msvc",
}
ARCHIVE_SUFFIXES = {
    "x86_64-unknown-linux-gnu": ".tar.gz",
    "x86_64-apple-darwin": ".tar.gz",
    "aarch64-apple-darwin": ".tar.gz",
    "x86_64-pc-windows-msvc": ".zip",
}
SUPPORT_FILES = frozenset({"SHA256SUMS", "install.sh", "install.ps1"})
MAX_WHEEL_BYTES = 100 * 1024 * 1024


def _sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _archive_binary(path: Path, *, package: str, windows: bool) -> bytes:
    member = f"{package}/{'cccc.exe' if windows else 'cccc'}"
    expected = {
        f"{package}/",
        member,
        f"{package}/LICENSE",
        f"{package}/README.md",
        f"{package}/rust-migration.md",
    }
    if windows:
        with zipfile.ZipFile(path) as archive:
            names = archive.namelist()
            if len(names) != len(set(names)):
                raise ValueError(f"{path.name} contains duplicate members")
            if set(names) != expected:
                raise ValueError(
                    f"{path.name} has an invalid archive layout: "
                    f"missing={sorted(expected - set(names))}, "
                    f"unexpected={sorted(set(names) - expected)}"
                )
            if not archive.getinfo(f"{package}/").is_dir():
                raise ValueError(f"{path.name} has an invalid package directory")
            for name in expected - {f"{package}/"}:
                info = archive.getinfo(name)
                mode = (info.external_attr >> 16) & 0o170000
                if info.is_dir() or mode not in {0, stat.S_IFREG}:
                    raise ValueError(f"{path.name} member {name} is not a regular file")
            return archive.read(member)
    with tarfile.open(path, "r:gz") as archive:
        members = archive.getmembers()
        names = [item.name + ("/" if item.isdir() else "") for item in members]
        if len(names) != len(set(names)):
            raise ValueError(f"{path.name} contains duplicate members")
        if set(names) != expected:
            raise ValueError(
                f"{path.name} has an invalid archive layout: "
                f"missing={sorted(expected - set(names))}, "
                f"unexpected={sorted(set(names) - expected)}"
            )
        for item in members:
            expected_directory = item.name == package
            if item.isdir() != expected_directory or (
                not expected_directory and not item.isfile()
            ):
                raise ValueError(f"{path.name} member {item.name} has an invalid type")
        item = archive.getmember(member)
        if item.mode & 0o111 == 0:
            raise ValueError(f"{path.name} member {member} is not executable")
        stream = archive.extractfile(item)
        if stream is None:
            raise ValueError(f"{path.name} could not read {member}")
        return stream.read()


def _checksum_manifest(path: Path) -> dict[str, str]:
    checksums: dict[str, str] = {}
    for line in path.read_text(encoding="utf-8").splitlines():
        match = re.fullmatch(r"([0-9a-f]{64})  \*?([^/\\]+)", line)
        if match is None:
            raise ValueError(f"invalid SHA256SUMS line: {line!r}")
        digest, filename = match.groups()
        if filename in checksums:
            raise ValueError(f"duplicate SHA256SUMS entry: {filename}")
        checksums[filename] = digest
    return checksums


def _verify_installers(directory: Path, *, cargo_version: str) -> None:
    shell = directory.joinpath("install.sh").read_text(encoding="utf-8")
    powershell = directory.joinpath("install.ps1").read_text(encoding="utf-8-sig")
    if "@CCCC_" in shell or "@CCCC_" in powershell:
        raise ValueError("release installers contain unresolved metadata placeholders")
    if not shell.startswith("#!/usr/bin/env sh\n"):
        raise ValueError("install.sh is not the canonical Unix installer")
    if f'DEFAULT_VERSION="{cargo_version}"' not in shell:
        raise ValueError("install.sh is not bound to the release version")
    if not powershell.startswith("[CmdletBinding()]\n"):
        raise ValueError("install.ps1 is not the canonical Windows installer")
    if f'$defaultVersion = "{cargo_version}"' not in powershell:
        raise ValueError("install.ps1 is not bound to the release version")


def verify(directory: Path, *, cargo_version: str, wheel_version: str) -> None:
    if not directory.is_dir():
        raise ValueError(f"release directory not found: {directory}")

    wheels = {
        platform: f"cccc_pair-{wheel_version}-py3-none-{platform}.whl"
        for platform in WHEEL_TARGETS
    }
    archives = {
        target: f"cccc-v{cargo_version}-{target}{ARCHIVE_SUFFIXES[target]}"
        for target in ARCHIVE_SUFFIXES
    }
    payload_names = frozenset(wheels.values()) | frozenset(archives.values())
    expected = payload_names | SUPPORT_FILES
    actual = frozenset(path.name for path in directory.iterdir() if path.is_file())
    if actual != expected:
        raise ValueError(
            "expected exactly four wheels, four archives, two installers, and SHA256SUMS; "
            f"missing={sorted(expected - actual)}, unexpected={sorted(actual - expected)}"
        )

    _verify_installers(directory, cargo_version=cargo_version)

    oversized = {
        filename: directory.joinpath(filename).stat().st_size
        for filename in wheels.values()
        if directory.joinpath(filename).stat().st_size >= MAX_WHEEL_BYTES
    }
    if oversized:
        raise ValueError(
            f"wheel exceeds the PyPI {MAX_WHEEL_BYTES}-byte boundary: {oversized}"
        )

    for platform, target in WHEEL_TARGETS.items():
        wheel = directory / wheels[platform]
        wheel_binary = verify_wheel(wheel, platform_tag=platform)
        package = f"cccc-v{cargo_version}-{target}"
        archive_binary = _archive_binary(
            directory / archives[target],
            package=package,
            windows=platform == "win_amd64",
        )
        if wheel_binary != archive_binary:
            raise ValueError(
                f"{platform} wheel and {target} archive contain different CCCC executable bytes"
            )

    manifest = _checksum_manifest(directory / "SHA256SUMS")
    if frozenset(manifest) != payload_names:
        raise ValueError(
            "SHA256SUMS must cover the exact eight executable payloads; "
            f"found={sorted(manifest)}"
        )
    for filename, expected_digest in manifest.items():
        actual_digest = _sha256(directory / filename)
        if actual_digest != expected_digest:
            raise ValueError(
                f"SHA256SUMS mismatch for {filename}: expected {expected_digest}, got {actual_digest}"
            )


def _versions(root: Path) -> tuple[str, str]:
    with root.joinpath("Cargo.toml").open("rb") as stream:
        cargo_version = str(tomllib.load(stream)["workspace"]["package"]["version"])
    with root.joinpath("pyproject.toml").open("rb") as stream:
        wheel_version = str(tomllib.load(stream)["project"]["version"])
    return cargo_version, wheel_version


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("directory", type=Path)
    parser.add_argument(
        "--root", type=Path, default=Path(__file__).resolve().parents[1]
    )
    args = parser.parse_args()
    cargo_version, wheel_version = _versions(args.root.resolve())
    try:
        verify(
            args.directory.resolve(),
            cargo_version=cargo_version,
            wheel_version=wheel_version,
        )
    except (
        OSError,
        ValueError,
        KeyError,
        tarfile.TarError,
        zipfile.BadZipFile,
    ) as error:
        parser.error(str(error))
    print(
        "OK: four Rust-only wheels and four standalone archives contain identical platform binaries"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
