#!/usr/bin/env python3
"""Verify the native dependency boundary promised by standalone CCCC artifacts."""

from __future__ import annotations

import argparse
import re
import subprocess
import sys
from collections.abc import Sequence
from pathlib import Path


_ELF_VERSION_RE = re.compile(r"\b(GLIBCXX|GLIBC|CXXABI|GCC)_([0-9]+(?:\.[0-9]+)+)\b")
_ELF_NEEDED_RE = re.compile(r"\(NEEDED\).*?\[([^\]]+)\]")
_MACOS_DEPENDENCY_RE = re.compile(r"^\s+(\S+)\s+\(compatibility version ", re.MULTILINE)

_LINUX_VERSION_LIMITS = {
    "GLIBC": (2, 28),
    "GLIBCXX": (3, 4, 24),
    "CXXABI": (1, 3, 11),
    "GCC": (7, 0, 0),
}

# The standalone Linux baseline intentionally matches the system-library
# boundary of the repository's manylinux_2_28 wheel. OpenSSL is not on this
# list: the standalone build must carry it statically.
_LINUX_ALLOWED_NEEDED = frozenset(
    {
        "ld-linux-x86-64.so.2",
        "libICE.so.6",
        "libSM.so.6",
        "libX11.so.6",
        "libXext.so.6",
        "libXrender.so.1",
        "libanl.so.1",
        "libatomic.so.1",
        "libc.so.6",
        "libdl.so.2",
        "libexpat.so.1",
        "libgcc_s.so.1",
        "libglib-2.0.so.0",
        "libgobject-2.0.so.0",
        "libgthread-2.0.so.0",
        "libm.so.6",
        "libmvec.so.1",
        "libnsl.so.1",
        "libpthread.so.0",
        "libresolv.so.2",
        "librt.so.1",
        "libstdc++.so.6",
        "libutil.so.1",
        "libz.so.1",
    }
)


def _version(value: str) -> tuple[int, ...]:
    return tuple(int(part) for part in value.split("."))


def _format_version(value: tuple[int, ...]) -> str:
    return ".".join(str(part) for part in value)


def parse_elf_version_references(output: str) -> dict[str, set[tuple[int, ...]]]:
    references: dict[str, set[tuple[int, ...]]] = {}
    for family, raw_version in _ELF_VERSION_RE.findall(output):
        references.setdefault(family, set()).add(_version(raw_version))
    return references


def parse_elf_needed(output: str) -> set[str]:
    return {name.strip() for name in _ELF_NEEDED_RE.findall(output) if name.strip()}


def parse_macos_minimum_versions(output: str) -> list[tuple[int, ...]]:
    versions: list[tuple[int, ...]] = []
    load_command = ""
    for raw_line in output.splitlines():
        line = raw_line.strip()
        if line.startswith("Load command "):
            load_command = ""
            continue
        if line.startswith("cmd "):
            load_command = line.removeprefix("cmd ").strip()
            continue

        field = "minos" if load_command == "LC_BUILD_VERSION" else "version"
        if load_command not in {"LC_BUILD_VERSION", "LC_VERSION_MIN_MACOSX"}:
            continue
        match = re.fullmatch(rf"{field}\s+([0-9]+(?:\.[0-9]+)+)", line)
        if match:
            versions.append(_version(match.group(1)))
    return versions


def parse_macos_dependencies(output: str) -> set[str]:
    return {name.strip() for name in _MACOS_DEPENDENCY_RE.findall(output) if name.strip()}


def validate_linux(version_output: str, dynamic_output: str) -> list[str]:
    errors: list[str] = []
    references = parse_elf_version_references(version_output)
    needed = parse_elf_needed(dynamic_output)

    if not references.get("GLIBC"):
        errors.append("ELF binary exposes no GLIBC version references")
    for family, limit in _LINUX_VERSION_LIMITS.items():
        versions = references.get(family, set())
        newer = sorted(version for version in versions if version > limit)
        if newer:
            rendered = ", ".join(f"{family}_{_format_version(version)}" for version in newer)
            errors.append(
                f"ELF binary exceeds the manylinux_2_28 {family}_{_format_version(limit)} boundary: "
                f"{rendered}"
            )

    if not needed:
        errors.append("ELF binary exposes no dynamic dependency table")
    unexpected = sorted(needed - _LINUX_ALLOWED_NEEDED)
    if unexpected:
        errors.append("ELF binary has non-baseline runtime dependencies: " + ", ".join(unexpected))
    return errors


def validate_macos(build_output: str, libraries_output: str) -> list[str]:
    errors: list[str] = []
    minimum_versions = parse_macos_minimum_versions(build_output)
    if not minimum_versions:
        errors.append("Mach-O binary exposes no macOS minimum deployment version")
    too_new = sorted(version for version in minimum_versions if version > (11, 0))
    if too_new:
        errors.append(
            "Mach-O binary exceeds the macOS 11.0 deployment boundary: "
            + ", ".join(_format_version(version) for version in too_new)
        )

    dependencies = parse_macos_dependencies(libraries_output)
    if not dependencies:
        errors.append("Mach-O binary exposes no dynamic dependency table")
    unexpected = sorted(
        dependency
        for dependency in dependencies
        if not dependency.startswith(("/usr/lib/", "/System/Library/"))
    )
    if unexpected:
        errors.append("Mach-O binary has non-system runtime dependencies: " + ", ".join(unexpected))
    return errors


def _run(command: Sequence[str]) -> str:
    completed = subprocess.run(command, capture_output=True, text=True, check=False)
    output = "\n".join(part for part in (completed.stdout, completed.stderr) if part).strip()
    if completed.returncode != 0:
        rendered = " ".join(command)
        raise ValueError(f"{rendered} failed with exit code {completed.returncode}: {output}")
    return output


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("binary", type=Path)
    parser.add_argument("--platform", choices=("linux", "macos"), required=True)
    args = parser.parse_args()

    binary = args.binary.resolve()
    if not binary.is_file():
        parser.error(f"binary not found: {binary}")

    if args.platform == "linux":
        errors = validate_linux(
            _run(("readelf", "--version-info", str(binary))),
            _run(("readelf", "-d", str(binary))),
        )
    else:
        errors = validate_macos(
            _run(("vtool", "-show-build", str(binary))),
            _run(("otool", "-L", str(binary))),
        )

    if errors:
        for error in errors:
            print(f"error: {error}", file=sys.stderr)
        return 1
    print(f"OK: verified standalone {args.platform} dependency boundary for {binary}")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except ValueError as error:
        print(f"error: {error}", file=sys.stderr)
        raise SystemExit(1) from None
