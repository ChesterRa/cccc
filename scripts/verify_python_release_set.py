#!/usr/bin/env python3
"""Verify the complete, atomic CCCC Python distribution set."""

from __future__ import annotations

import argparse
from pathlib import Path


WHEEL_SUFFIXES = (
    "-py3-none-any.whl",
    "-py3-none-manylinux_2_28_x86_64.whl",
    "-py3-none-macosx_11_0_x86_64.whl",
    "-py3-none-macosx_11_0_arm64.whl",
    "-py3-none-win_amd64.whl",
)
MAX_WHEEL_BYTES = 100 * 1024 * 1024


def verify(directory: Path) -> None:
    if not directory.is_dir():
        raise ValueError(f"distribution directory not found: {directory}")

    files = sorted((path for path in directory.iterdir() if path.is_file()), key=lambda path: path.name)
    wheels = [path for path in files if path.name.endswith(".whl")]
    sdists = [path for path in files if path.name.endswith(".tar.gz")]
    if len(files) != 6 or len(wheels) != 5 or len(sdists) != 1:
        raise ValueError(
            "expected exactly one sdist and five wheels, found "
            f"{[path.name for path in files]}"
        )

    prefixes: set[str] = set()
    for suffix in WHEEL_SUFFIXES:
        matches = [path for path in wheels if path.name.endswith(suffix)]
        if len(matches) != 1:
            raise ValueError(
                f"expected one wheel ending in {suffix!r}, found {[path.name for path in matches]}"
            )
        prefixes.add(matches[0].name[: -len(suffix)])

    if len(prefixes) != 1:
        raise ValueError(f"wheel project/version prefixes do not match: {sorted(prefixes)}")
    prefix = prefixes.pop()
    expected_sdist = f"{prefix}.tar.gz"
    if sdists[0].name != expected_sdist:
        raise ValueError(f"expected sdist {expected_sdist!r}, found {sdists[0].name!r}")

    oversized = {path.name: path.stat().st_size for path in wheels if path.stat().st_size >= MAX_WHEEL_BYTES}
    if oversized:
        raise ValueError(f"wheel exceeds {MAX_WHEEL_BYTES} bytes: {oversized}")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("directory", type=Path)
    args = parser.parse_args()
    try:
        verify(args.directory.resolve())
    except ValueError as error:
        parser.error(str(error))
    print("OK: one sdist, one universal wheel, and four native wheels")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
