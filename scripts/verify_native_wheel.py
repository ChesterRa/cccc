#!/usr/bin/env python3
"""Verify that a platform wheel contains exactly one executable Rust payload."""

from __future__ import annotations

import argparse
import os
import zipfile
from pathlib import Path


def verify(wheel: Path, *, platform_tag: str) -> None:
    if not wheel.is_file():
        raise ValueError(f"wheel not found: {wheel}")
    expected_tag = f"Tag: py3-none-{platform_tag}"
    with zipfile.ZipFile(wheel) as archive:
        names = archive.namelist()
        payloads = [
            name
            for name in names
            if name.endswith("/cccc/_native/cccc-rust")
            or name.endswith("/cccc/_native/cccc-rust.exe")
            or name in {"cccc/_native/cccc-rust", "cccc/_native/cccc-rust.exe"}
        ]
        if len(payloads) != 1:
            raise ValueError(f"expected one Rust payload, found {payloads}")
        wheel_metadata = [name for name in names if name.endswith(".dist-info/WHEEL")]
        if len(wheel_metadata) != 1:
            raise ValueError(f"expected one WHEEL metadata file, found {wheel_metadata}")
        metadata = archive.read(wheel_metadata[0]).decode("utf-8")
        if "Root-Is-Purelib: false" not in metadata:
            raise ValueError("native wheel must set Root-Is-Purelib: false")
        if expected_tag not in metadata:
            raise ValueError(f"native wheel is missing {expected_tag!r}")
        payload = archive.getinfo(payloads[0])
        if os.name != "nt" and not payload.filename.endswith(".exe"):
            mode = (payload.external_attr >> 16) & 0o777
            if mode & 0o111 == 0:
                raise ValueError("Unix Rust payload is not executable in the wheel archive")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("wheel", type=Path)
    parser.add_argument("--platform-tag", required=True)
    args = parser.parse_args()
    try:
        verify(args.wheel.resolve(), platform_tag=str(args.platform_tag).strip())
    except ValueError as error:
        parser.error(str(error))
    print(f"OK: verified native CCCC wheel {args.wheel}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
