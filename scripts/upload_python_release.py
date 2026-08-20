#!/usr/bin/env python3
"""Upload only Python release files that are not already on the target index."""

from __future__ import annotations

import argparse
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


def existing_filenames(repository: str) -> set[str]:
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
            return set()
        raise
    return {
        str(item["filename"])
        for release in payload.get("releases", {}).values()
        for item in release
        if item.get("filename")
    }


def upload_missing(repository: str, distributions: list[Path]) -> int:
    missing = [path for path in distributions if path.name not in existing_filenames(repository)]
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
            "--skip-existing",
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
