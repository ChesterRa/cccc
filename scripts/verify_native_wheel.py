#!/usr/bin/env python3
"""Verify the Rust-only CCCC wheel structure and executable identity."""

from __future__ import annotations

import argparse
import base64
import csv
import hashlib
import io
import stat
import zipfile
from pathlib import Path


def _digest(data: bytes) -> str:
    encoded = base64.urlsafe_b64encode(hashlib.sha256(data).digest()).rstrip(b"=")
    return f"sha256={encoded.decode('ascii')}"


def verify(wheel: Path, *, platform_tag: str, binary: Path | None = None) -> bytes:
    if not wheel.is_file():
        raise ValueError(f"wheel not found: {wheel}")
    suffix = f"-py3-none-{platform_tag}.whl"
    if not wheel.name.endswith(suffix):
        raise ValueError(f"wheel filename is missing platform suffix {suffix!r}")
    stem = wheel.name[: -len(suffix)]
    if not stem.startswith("cccc_pair-") or not stem.removeprefix("cccc_pair-"):
        raise ValueError(
            "wheel filename must use the cccc_pair distribution and a version"
        )
    version = stem.removeprefix("cccc_pair-")
    dist_info = f"{stem}.dist-info"
    expected_tag = f"Tag: py3-none-{platform_tag}"
    with zipfile.ZipFile(wheel) as archive:
        names = archive.namelist()
        if len(names) != len(set(names)):
            raise ValueError("wheel contains duplicate archive members")
        if any(name.startswith("/") or ".." in Path(name).parts for name in names):
            raise ValueError("wheel contains an unsafe archive member")
        executable_name = "cccc.exe" if platform_tag == "win_amd64" else "cccc"
        payload = f"{stem}.data/scripts/{executable_name}"
        install_marker = f"{stem}.data/scripts/.cccc-standalone"
        wheel_metadata = f"{dist_info}/WHEEL"
        core_metadata = f"{dist_info}/METADATA"
        license_path = f"{dist_info}/licenses/LICENSE"
        record = f"{dist_info}/RECORD"
        expected_names = {
            payload,
            install_marker,
            wheel_metadata,
            core_metadata,
            license_path,
            record,
        }
        if set(names) != expected_names:
            raise ValueError(
                "wheel has an invalid Rust-only layout: "
                f"missing={sorted(expected_names - set(names))}, "
                f"unexpected={sorted(set(names) - expected_names)}"
            )
        for name in expected_names:
            info = archive.getinfo(name)
            mode = (info.external_attr >> 16) & 0o170000
            if info.is_dir() or mode not in {0, stat.S_IFREG}:
                raise ValueError(f"wheel member {name} is not a regular file")

        metadata = archive.read(wheel_metadata).decode("utf-8")
        if "Root-Is-Purelib: false" not in metadata:
            raise ValueError("native wheel must set Root-Is-Purelib: false")
        if expected_tag not in metadata:
            raise ValueError(f"native wheel is missing {expected_tag!r}")
        rendered_core = archive.read(core_metadata).decode("utf-8")
        if "Name: cccc-pair\n" not in rendered_core:
            raise ValueError("wheel project name must remain cccc-pair")
        if f"Version: {version}\n" not in rendered_core:
            raise ValueError("wheel metadata version does not match its filename")
        headers, separator, description = rendered_core.partition("\n\n")
        if "Description-Content-Type: text/markdown\n" not in f"{headers}\n":
            raise ValueError("wheel metadata must declare its Markdown description")
        if not separator or not description.strip():
            raise ValueError("wheel metadata must include a non-empty description")
        if "Requires-Dist:" in rendered_core:
            raise ValueError(
                "Rust-only wheel must not declare Python runtime dependencies"
            )
        rows = list(csv.reader(io.StringIO(archive.read(record).decode("utf-8"))))
        if {row[0] for row in rows if len(row) == 3} != set(names):
            raise ValueError("wheel RECORD does not cover the exact archive contents")
        for path, digest, size in rows:
            if path == record:
                if digest or size:
                    raise ValueError(
                        "wheel RECORD must leave its own hash and size empty"
                    )
                continue
            data = archive.read(path)
            if digest != _digest(data) or size != str(len(data)):
                raise ValueError(f"wheel RECORD mismatch for {path}")
        payload_info = archive.getinfo(payload)
        if not payload_info.filename.endswith(".exe"):
            mode = (payload_info.external_attr >> 16) & 0o777
            if mode & 0o111 == 0:
                raise ValueError(
                    "Unix Rust payload is not executable in the wheel archive"
                )
        marker_info = archive.getinfo(install_marker)
        marker_mode = (marker_info.external_attr >> 16) & 0o777
        if marker_mode & 0o111:
            raise ValueError("wheel ownership marker must not be executable")
        if archive.read(install_marker) != b"pip-v1\n":
            raise ValueError("wheel ownership marker is invalid")
        payload_bytes = archive.read(payload)
        if binary is not None and payload_bytes != binary.read_bytes():
            raise ValueError("wheel executable bytes differ from the release binary")
        return payload_bytes


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("wheel", type=Path)
    parser.add_argument("--platform-tag", required=True)
    parser.add_argument("--binary", type=Path)
    args = parser.parse_args()
    try:
        binary = args.binary.resolve() if args.binary is not None else None
        verify(
            args.wheel.resolve(),
            platform_tag=str(args.platform_tag).strip(),
            binary=binary,
        )
    except ValueError as error:
        parser.error(str(error))
    print(f"OK: verified native CCCC wheel {args.wheel}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
