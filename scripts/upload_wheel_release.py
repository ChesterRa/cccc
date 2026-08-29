#!/usr/bin/env python3
"""Upload missing native wheel files without mixing artifact builds."""

from __future__ import annotations

import argparse
import hashlib
import json
import subprocess
import sys
import urllib.error
import urllib.request
from pathlib import Path


_REPOSITORIES = {
    "pypi": ("https://pypi.org/pypi", "https://upload.pypi.org/legacy/"),
    "testpypi": ("https://test.pypi.org/pypi", "https://test.pypi.org/legacy/"),
}
_PROJECT = "cccc-pair"


def existing_hashes(repository: str) -> dict[str, str]:
    index_url, _ = _REPOSITORIES[repository]
    request = urllib.request.Request(
        f"{index_url}/{_PROJECT}/json",
        headers={"Accept": "application/json"},
    )
    try:
        with urllib.request.urlopen(request, timeout=30) as response:
            payload = json.load(response)
    except urllib.error.HTTPError as error:
        if error.code == 404:
            return {}
        raise
    hashes: dict[str, str] = {}
    for release in payload.get("releases", {}).values():
        for item in release:
            filename = str(item.get("filename") or "").strip()
            digests = item.get("digests") if isinstance(item.get("digests"), dict) else {}
            if filename:
                hashes[filename] = str(digests.get("sha256") or "").strip().lower()
    return hashes


def file_sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def upload_missing(repository: str, distributions: list[Path]) -> int:
    published = existing_hashes(repository)
    missing: list[Path] = []
    conflicts: list[tuple[Path, str, str]] = []
    for path in distributions:
        remote_hash = published.get(path.name)
        if remote_hash is None:
            missing.append(path)
            continue
        local_hash = file_sha256(path)
        if not remote_hash or remote_hash != local_hash:
            conflicts.append((path, local_hash, remote_hash or "unavailable"))

    if conflicts:
        print(
            f"Refusing to mix {_PROJECT} artifacts from different builds on {repository}:",
            file=sys.stderr,
        )
        for path, local_hash, remote_hash in conflicts:
            print(
                f"  {path.name}: local sha256={local_hash}, published sha256={remote_hash}",
                file=sys.stderr,
            )
        print("Publish a new version instead of replacing immutable artifacts.", file=sys.stderr)
        return 1

    if not missing:
        print(f"All {_PROJECT} distributions already exist on {repository}; nothing to upload.")
        return 0

    _, upload_url = _REPOSITORIES[repository]
    completed = subprocess.run(
        [
            sys.executable,
            "-m",
            "twine",
            "upload",
            "--repository-url",
            upload_url,
            *(str(path) for path in missing),
        ],
        check=False,
    )
    return completed.returncode


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--repository", choices=sorted(_REPOSITORIES), required=True)
    parser.add_argument("distributions", nargs="+", type=Path)
    args = parser.parse_args()
    missing_files = [path for path in args.distributions if not path.is_file()]
    if missing_files:
        parser.error(f"distribution does not exist: {missing_files[0]}")
    return upload_missing(args.repository, args.distributions)


if __name__ == "__main__":
    raise SystemExit(main())
