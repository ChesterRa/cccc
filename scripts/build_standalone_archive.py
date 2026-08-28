#!/usr/bin/env python3
"""Wrap one release-ready CCCC executable in a deterministic standalone archive."""

from __future__ import annotations

import argparse
import gzip
import io
import os
import stat
import tarfile
import time
import tomllib
import zipfile
from pathlib import Path


_TARGETS = {
    "x86_64-unknown-linux-gnu": (".tar.gz", "cccc"),
    "x86_64-apple-darwin": (".tar.gz", "cccc"),
    "aarch64-apple-darwin": (".tar.gz", "cccc"),
    "x86_64-pc-windows-msvc": (".zip", "cccc.exe"),
}


def _epoch() -> int:
    raw = str(os.environ.get("SOURCE_DATE_EPOCH") or "").strip()
    return max(int(raw) if raw else 315532800, 315532800)


def _zip_time() -> tuple[int, int, int, int, int, int]:
    return time.gmtime(_epoch())[:6]


def _version(root: Path) -> str:
    with root.joinpath("Cargo.toml").open("rb") as stream:
        return str(tomllib.load(stream)["workspace"]["package"]["version"])


def _files(
    root: Path, binary: Path, executable_name: str
) -> list[tuple[str, bytes, int]]:
    return [
        (executable_name, binary.read_bytes(), 0o755),
        ("LICENSE", root.joinpath("LICENSE").read_bytes(), 0o644),
        ("README.md", root.joinpath("README.md").read_bytes(), 0o644),
        (
            "rust-migration.md",
            root.joinpath("docs/rust-migration.md").read_bytes(),
            0o644,
        ),
    ]


def _tar(output: Path, package: str, files: list[tuple[str, bytes, int]]) -> None:
    with output.open("wb") as raw:
        with gzip.GzipFile(
            filename="", mode="wb", fileobj=raw, mtime=_epoch(), compresslevel=9
        ) as zipped:
            with tarfile.open(
                fileobj=zipped, mode="w", format=tarfile.PAX_FORMAT
            ) as archive:
                directory = tarfile.TarInfo(package)
                directory.type = tarfile.DIRTYPE
                directory.mode = 0o755
                directory.mtime = _epoch()
                archive.addfile(directory)
                for name, data, mode in files:
                    info = tarfile.TarInfo(f"{package}/{name}")
                    info.size = len(data)
                    info.mode = mode
                    info.mtime = _epoch()
                    archive.addfile(info, io.BytesIO(data))


def _zip(output: Path, package: str, files: list[tuple[str, bytes, int]]) -> None:
    with zipfile.ZipFile(
        output, "w", compression=zipfile.ZIP_DEFLATED, compresslevel=9
    ) as archive:
        directory = zipfile.ZipInfo(f"{package}/", date_time=_zip_time())
        directory.create_system = 3
        directory.external_attr = (stat.S_IFDIR | 0o755) << 16
        archive.writestr(directory, b"")
        for name, data, mode in files:
            info = zipfile.ZipInfo(f"{package}/{name}", date_time=_zip_time())
            info.create_system = 3
            info.external_attr = (stat.S_IFREG | mode) << 16
            info.compress_type = zipfile.ZIP_DEFLATED
            archive.writestr(info, data)


def build(binary: Path, output_dir: Path, *, target: str, root: Path) -> Path:
    binary = binary.resolve()
    if not binary.is_file():
        raise ValueError(f"release binary not found: {binary}")
    try:
        suffix, executable_name = _TARGETS[target]
    except KeyError as error:
        raise ValueError(f"unsupported release target: {target!r}") from error

    version = _version(root.resolve())
    package = f"cccc-v{version}-{target}"
    output_dir.mkdir(parents=True, exist_ok=True)
    output = output_dir / f"{package}{suffix}"
    files = _files(root.resolve(), binary, executable_name)
    if suffix == ".zip":
        _zip(output, package, files)
    else:
        _tar(output, package, files)
    return output


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    parser.add_argument("--target", choices=sorted(_TARGETS), required=True)
    parser.add_argument("--output-dir", type=Path, default=Path("dist"))
    parser.add_argument(
        "--root", type=Path, default=Path(__file__).resolve().parents[1]
    )
    args = parser.parse_args()
    try:
        output = build(
            args.binary,
            args.output_dir.resolve(),
            target=args.target,
            root=args.root,
        )
    except (OSError, ValueError, KeyError) as error:
        parser.error(str(error))
    print(output)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
