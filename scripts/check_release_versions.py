#!/usr/bin/env python3
"""Validate the shared CCCC release identity across package metadata, Rust, and Git tags."""

from __future__ import annotations

import argparse
import re
import subprocess
import sys
import tomllib
from pathlib import Path


_PACKAGE_VERSION_RE = re.compile(r"^(\d+\.\d+\.\d+)(?:(a|b|rc)(\d+))?$")
_RUST_VERSION_RE = re.compile(r"^(\d+\.\d+\.\d+)(?:-(alpha|beta|rc)(\d+))?$")
_PACKAGE_PHASES = {"a": "alpha", "b": "beta", "rc": "rc"}


def _package_identity(version: str) -> tuple[str, str, str]:
    match = _PACKAGE_VERSION_RE.fullmatch(version)
    if match is None:
        raise ValueError(
            f"unsupported package version {version!r}; expected X.Y.Z, X.Y.ZaN, X.Y.ZbN, or X.Y.ZrcN"
        )
    base, phase, number = match.groups()
    return base, _PACKAGE_PHASES.get(phase or "", ""), number or ""


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
        package_version = str(tomllib.load(handle).get("project", {}).get("version", "")).strip()
    with (root / "Cargo.toml").open("rb") as handle:
        rust_version = str(
            tomllib.load(handle).get("workspace", {}).get("package", {}).get("version", "")
        ).strip()
    return package_version, rust_version


def _iter_dependency_tables(manifest: dict[str, object]) -> list[tuple[str, dict[str, object]]]:
    tables: list[tuple[str, dict[str, object]]] = []
    for section in ("dependencies", "dev-dependencies", "build-dependencies"):
        table = manifest.get(section)
        if isinstance(table, dict):
            tables.append((section, table))
    target_table = manifest.get("target")
    if isinstance(target_table, dict):
        for cfg, target_spec in target_table.items():
            if not isinstance(target_spec, dict):
                continue
            for section in ("dependencies", "dev-dependencies", "build-dependencies"):
                table = target_spec.get(section)
                if isinstance(table, dict):
                    tables.append((f"target.{cfg}.{section}", table))
    return tables


def _validate_workspace_parity(root: Path, rust_version: str) -> None:
    root_manifest_path = root / "Cargo.toml"
    with root_manifest_path.open("rb") as handle:
        root_manifest = tomllib.load(handle)
    members = root_manifest.get("workspace", {}).get("members", [])
    if not isinstance(members, list) or not members:
        raise ValueError(f"failed to read workspace.members from {root_manifest_path}")

    member_manifests: list[tuple[Path, str, dict[str, object]]] = []
    member_dirs: dict[Path, str] = {}
    member_names: set[str] = set()
    for member in members:
        member_dir = (root / str(member)).resolve()
        manifest_path = member_dir / "Cargo.toml"
        if not manifest_path.is_file():
            raise ValueError(f"workspace member manifest not found: {manifest_path}")
        with manifest_path.open("rb") as handle:
            manifest = tomllib.load(handle)
        package = manifest.get("package")
        if not isinstance(package, dict):
            raise ValueError(f"workspace member missing [package]: {manifest_path}")
        crate_name = str(package.get("name", "")).strip()
        if not crate_name:
            raise ValueError(f"workspace member missing package.name: {manifest_path}")
        version_spec = package.get("version")
        if version_spec != {"workspace": True}:
            rel_path = manifest_path.relative_to(root)
            raise ValueError(
                f"CCCC workspace version mismatch: {crate_name} ({rel_path}) must inherit workspace.package.version"
            )
        member_manifests.append((manifest_path, crate_name, manifest))
        member_dirs[member_dir] = crate_name
        member_names.add(crate_name)

    expected_pin = f"={rust_version}"
    for manifest_path, _crate_name, manifest in member_manifests:
        rel_path = manifest_path.relative_to(root)
        manifest_dir = manifest_path.parent
        for section, table in _iter_dependency_tables(manifest):
            for dep_key, dep_spec in table.items():
                if not isinstance(dep_spec, dict):
                    continue
                target_pkg = str(dep_spec.get("package", dep_key)).strip()
                dep_path = dep_spec.get("path")
                points_to_member = False
                if isinstance(dep_path, str):
                    resolved_dep = (manifest_dir / dep_path).resolve()
                    points_to_member = resolved_dep in member_dirs
                if not points_to_member and target_pkg in member_names:
                    points_to_member = True
                if not points_to_member:
                    continue
                dep_version = dep_spec.get("version")
                if dep_version is not None and dep_version != expected_pin:
                    raise ValueError(
                        f"CCCC internal dependency version mismatch in {rel_path} [{section}]: "
                        f"{dep_key} ({target_pkg}) has version={dep_version!r}, expected {expected_pin!r}"
                    )

    lock_path = root / "Cargo.lock"
    if not lock_path.is_file():
        raise ValueError(f"Cargo.lock not found: {lock_path}")
    with lock_path.open("rb") as handle:
        lockfile = tomllib.load(handle)
    local_versions: dict[str, list[str]] = {}
    for entry in lockfile.get("package", []):
        if not isinstance(entry, dict) or "source" in entry:
            continue
        name = str(entry.get("name", "")).strip()
        version = str(entry.get("version", "")).strip()
        if name:
            local_versions.setdefault(name, []).append(version)

    for manifest_path, crate_name, _manifest in member_manifests:
        rel_path = manifest_path.relative_to(root)
        versions = local_versions.get(crate_name, [])
        if versions != [rust_version]:
            raise ValueError(
                f"CCCC lockfile version mismatch for {crate_name} ({rel_path}): "
                f"found {versions!r} in Cargo.lock, expected [{rust_version!r}]"
            )


def _rust_binary_version(binary: Path) -> str:
    if not binary.is_file():
        raise ValueError(f"Rust binary not found: {binary}")
    try:
        completed = subprocess.run(
            [str(binary), "--version"],
            capture_output=True,
            text=True,
            check=False,
            timeout=10,
        )
    except Exception as error:
        raise ValueError(f"failed to execute Rust binary {binary}: {error}") from error
    output = "\n".join(part for part in (completed.stdout, completed.stderr) if part).strip()
    if completed.returncode != 0:
        raise ValueError(
            f"Rust binary version probe failed with exit code {completed.returncode}: {output}"
        )
    match = re.search(r"(?<![0-9])(\d+\.\d+\.\d+(?:-(?:alpha|beta|rc)\d+)?)(?![0-9])", output)
    if match is None:
        raise ValueError(f"Rust binary returned an unrecognized version: {output!r}")
    return match.group(1)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parent.parent)
    parser.add_argument("--package-version", default="")
    parser.add_argument("--rust-version", default="")
    parser.add_argument("--rust-binary", type=Path)
    parser.add_argument("--tag", default="")
    args = parser.parse_args(argv)

    package_version = str(args.package_version or "").strip()
    rust_version = str(args.rust_version or "").strip()
    if bool(package_version) != bool(rust_version):
        parser.error("--package-version and --rust-version must be provided together")
    from_manifests = not package_version
    root = args.root.resolve()
    if from_manifests:
        package_version, rust_version = _manifest_versions(root)
    if not package_version or not rust_version:
        raise ValueError("failed to read CCCC versions from pyproject.toml and Cargo.toml")

    package_identity = _package_identity(package_version)
    rust_identity = _rust_identity(rust_version)
    if package_identity != rust_identity:
        raise ValueError(
            "CCCC release identity mismatch: "
            f"Package={package_version!r} -> {_canonical(package_identity)!r}, "
            f"Rust={rust_version!r} -> {_canonical(rust_identity)!r}"
        )

    if from_manifests:
        _validate_workspace_parity(root, rust_version)

    identity = _canonical(package_identity)
    tag = str(args.tag or "").strip()
    if tag and tag != f"v{identity}":
        raise ValueError(f"tag/version mismatch: expected 'v{identity}', got {tag!r}")

    if args.rust_binary is not None:
        binary_version = _rust_binary_version(args.rust_binary.resolve())
        binary_identity = _rust_identity(binary_version)
        if binary_identity != rust_identity:
            raise ValueError(
                "CCCC Rust binary identity mismatch: "
                f"binary={binary_version!r} -> {_canonical(binary_identity)!r}, "
                f"manifest={rust_version!r} -> {_canonical(rust_identity)!r}"
            )

    print(f"CCCC release identity: {identity} (Package={package_version}, Rust={rust_version})")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except ValueError as error:
        print(error, file=sys.stderr)
        raise SystemExit(1) from None
