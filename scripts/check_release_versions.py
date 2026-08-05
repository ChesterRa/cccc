#!/usr/bin/env python3
"""Validate the shared CCCC release identity across Python, Rust, and Git tags."""

from __future__ import annotations

import argparse
import re
import sys
import tomllib
from pathlib import Path


_PYTHON_VERSION_RE = re.compile(r"^(\d+\.\d+\.\d+)(?:(a|b|rc)(\d+))?$")
_RUST_VERSION_RE = re.compile(r"^(\d+\.\d+\.\d+)(?:-(alpha|beta|rc)(\d+))?$")
_PYTHON_PHASES = {"a": "alpha", "b": "beta", "rc": "rc"}


def _python_identity(version: str) -> tuple[str, str, str]:
    match = _PYTHON_VERSION_RE.fullmatch(version)
    if match is None:
        raise ValueError(
            f"unsupported Python version {version!r}; expected X.Y.Z, X.Y.ZaN, X.Y.ZbN, or X.Y.ZrcN"
        )
    base, phase, number = match.groups()
    return base, _PYTHON_PHASES.get(phase or "", ""), number or ""


def _rust_identity(version: str) -> tuple[str, str, str]:
    match = _RUST_VERSION_RE.fullmatch(version)
    if match is None:
        raise ValueError(
            f"unsupported Rust version {version!r}; expected X.Y.Z or X.Y.Z-(alpha|beta|rc)N"
        )
    base, phase, number = match.groups()
    return base, phase or "", number or ""


def _canonical(identity: tuple[str, str, str]) -> str:
    base, phase, number = identity
    return base if not phase else f"{base}-{phase}{number}"


def _manifest_versions(root: Path) -> tuple[str, str]:
    with (root / "pyproject.toml").open("rb") as handle:
        python_version = str(tomllib.load(handle).get("project", {}).get("version", "")).strip()
    with (root / "Cargo.toml").open("rb") as handle:
        rust_version = str(
            tomllib.load(handle).get("workspace", {}).get("package", {}).get("version", "")
        ).strip()
    return python_version, rust_version


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parent.parent)
    parser.add_argument("--python-version", default="")
    parser.add_argument("--rust-version", default="")
    parser.add_argument("--tag", default="")
    args = parser.parse_args()

    python_version = str(args.python_version or "").strip()
    rust_version = str(args.rust_version or "").strip()
    if bool(python_version) != bool(rust_version):
        parser.error("--python-version and --rust-version must be provided together")
    if not python_version:
        python_version, rust_version = _manifest_versions(args.root.resolve())
    if not python_version or not rust_version:
        raise ValueError("failed to read CCCC versions from pyproject.toml and Cargo.toml")

    python_identity = _python_identity(python_version)
    rust_identity = _rust_identity(rust_version)
    if python_identity != rust_identity:
        raise ValueError(
            "CCCC release identity mismatch: "
            f"Python={python_version!r} -> {_canonical(python_identity)!r}, "
            f"Rust={rust_version!r} -> {_canonical(rust_identity)!r}"
        )

    identity = _canonical(python_identity)
    tag = str(args.tag or "").strip()
    if tag and tag != f"v{identity}":
        raise ValueError(f"tag/version mismatch: expected 'v{identity}', got {tag!r}")

    print(f"CCCC release identity: {identity} (Python={python_version}, Rust={rust_version})")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except ValueError as error:
        print(error, file=sys.stderr)
        raise SystemExit(1) from None
