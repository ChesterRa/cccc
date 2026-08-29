#!/usr/bin/env python3
"""Wrap one release-ready CCCC executable in a deterministic native wheel."""

from __future__ import annotations

import argparse
import base64
import csv
import hashlib
import io
import os
import re
import stat
import time
import tomllib
import zipfile
from pathlib import Path


_TAG_RE = re.compile(r"^[A-Za-z0-9_.]+$")
_WHEEL_COMPONENT_RE = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._!+]*$")


def _project(root: Path) -> tuple[str, str, str, str, str]:
    with root.joinpath("pyproject.toml").open("rb") as stream:
        project = tomllib.load(stream)["project"]
    name = str(project["name"])
    version = str(project["version"])
    summary = str(project["description"])
    readme = project.get("readme")
    if isinstance(readme, dict):
        readme_path = root.joinpath(str(readme["file"]))
        readme_content_type = str(readme.get("content-type") or "text/markdown")
    elif isinstance(readme, str):
        readme_path = root.joinpath(readme)
        readme_content_type = "text/markdown"
    else:
        raise ValueError("project readme must name the release description file")
    if not _WHEEL_COMPONENT_RE.fullmatch(version):
        raise ValueError(f"project version is not wheel-safe PEP 440: {version!r}")
    return (
        name,
        version,
        summary,
        readme_content_type,
        readme_path.read_text(encoding="utf-8"),
    )


def _wheel_name_component(value: str) -> str:
    return re.sub(r"[-_.]+", "_", value)


def _digest(data: bytes) -> str:
    encoded = base64.urlsafe_b64encode(hashlib.sha256(data).digest()).rstrip(b"=")
    return f"sha256={encoded.decode('ascii')}"


def _timestamp() -> tuple[int, int, int, int, int, int]:
    raw = str(os.environ.get("SOURCE_DATE_EPOCH") or "").strip()
    epoch = int(raw) if raw else 315532800
    return time.gmtime(max(epoch, 315532800))[:6]


def _zip_info(name: str, *, executable: bool = False) -> zipfile.ZipInfo:
    info = zipfile.ZipInfo(name, date_time=_timestamp())
    info.compress_type = zipfile.ZIP_DEFLATED
    info.create_system = 3
    mode = stat.S_IFREG | (0o755 if executable else 0o644)
    info.external_attr = mode << 16
    return info


def _metadata(
    name: str,
    version: str,
    summary: str,
    readme_content_type: str,
    readme: str,
) -> bytes:
    return (
        "Metadata-Version: 2.4\n"
        f"Name: {name}\n"
        f"Version: {version}\n"
        f"Summary: {summary}\n"
        f"Description-Content-Type: {readme_content_type}\n"
        "License-Expression: Apache-2.0\n"
        "Project-URL: Homepage, https://github.com/ChesterRa/cccc\n"
        "Project-URL: Repository, https://github.com/ChesterRa/cccc\n"
        "Classifier: Development Status :: 5 - Production/Stable\n"
        "Classifier: Environment :: Console\n"
        "Classifier: Programming Language :: Rust\n"
        "Classifier: Operating System :: POSIX :: Linux\n"
        "Classifier: Operating System :: MacOS\n"
        "Classifier: Operating System :: Microsoft :: Windows\n"
        "\n"
        f"{readme.rstrip()}\n"
    ).encode("utf-8")


def build(binary: Path, output_dir: Path, *, platform_tag: str, root: Path) -> Path:
    binary = binary.resolve()
    if not binary.is_file():
        raise ValueError(f"release binary not found: {binary}")
    if _TAG_RE.fullmatch(platform_tag) is None or platform_tag.lower() == "any":
        raise ValueError(f"invalid native wheel platform tag: {platform_tag!r}")

    name, version, summary, readme_content_type, readme = _project(root.resolve())
    distribution = _wheel_name_component(name)
    version_component = version
    stem = f"{distribution}-{version_component}"
    wheel_name = f"{stem}-py3-none-{platform_tag}.whl"
    dist_info = f"{stem}.dist-info"
    executable_name = "cccc.exe" if platform_tag == "win_amd64" else "cccc"
    script = f"{stem}.data/scripts/{executable_name}"
    install_marker = f"{stem}.data/scripts/.cccc-standalone"
    wheel = f"{dist_info}/WHEEL"
    metadata = f"{dist_info}/METADATA"
    license_path = f"{dist_info}/licenses/LICENSE"
    record = f"{dist_info}/RECORD"

    entries = {
        script: binary.read_bytes(),
        install_marker: b"pip-v1\n",
        metadata: _metadata(
            name,
            version,
            summary,
            readme_content_type,
            readme,
        ),
        wheel: (
            "Wheel-Version: 1.0\n"
            "Generator: cccc-native-wheel 1\n"
            "Root-Is-Purelib: false\n"
            f"Tag: py3-none-{platform_tag}\n\n"
        ).encode("utf-8"),
        license_path: root.joinpath("LICENSE").read_bytes(),
    }
    rows = [[path, _digest(data), str(len(data))] for path, data in entries.items()]
    rows.append([record, "", ""])
    rendered_record = io.StringIO(newline="")
    csv.writer(rendered_record, lineterminator="\n").writerows(rows)

    output_dir.mkdir(parents=True, exist_ok=True)
    output = output_dir.joinpath(wheel_name)
    with zipfile.ZipFile(
        output, "w", compression=zipfile.ZIP_DEFLATED, compresslevel=9
    ) as archive:
        for path, data in entries.items():
            archive.writestr(_zip_info(path, executable=path == script), data)
        archive.writestr(_zip_info(record), rendered_record.getvalue().encode("utf-8"))
    return output


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    parser.add_argument("--platform-tag", required=True)
    parser.add_argument("--output-dir", type=Path, default=Path("dist"))
    parser.add_argument(
        "--root", type=Path, default=Path(__file__).resolve().parents[1]
    )
    args = parser.parse_args()
    try:
        output = build(
            args.binary,
            args.output_dir.resolve(),
            platform_tag=str(args.platform_tag).strip(),
            root=args.root,
        )
    except (OSError, ValueError, KeyError) as error:
        parser.error(str(error))
    print(output)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
