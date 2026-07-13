#!/usr/bin/env python3
from __future__ import annotations

import argparse
import hashlib
import io
import json
import os
import subprocess
import sys
import tarfile
from pathlib import Path
from typing import Mapping


DEFAULT_LIMIT = 300
DEFAULT_BASELINE = "scripts/quality/source-size-baseline.json"
DEFAULT_FORMATTER_MIGRATION = "scripts/quality/oxfmt-migration-v1.json"
DEFAULT_PREEXISTING_REVIEWED = "scripts/quality/preexisting-reviewed-v1.json"
EXCLUDED_PARTS = {"__pycache__", "_vendor", "dist", "generated", "node_modules", "tests", "vendor"}
TEST_SUFFIXES = (".test.ts", ".test.tsx", ".spec.ts", ".spec.tsx")
BASE_REF_ENV = "SOURCE_SIZE_BASE_REF"
PREEXISTING_REVIEWED_PATHS = {
    "web/src/components/AgentTab.tsx",
    "web/src/components/ContextModal/index.tsx",
    "web/src/components/browser/ProjectedBrowserSurfacePanel.tsx",
    "web/src/components/modals/ActorConfigModal.tsx",
    "web/src/components/modals/settings/GuidanceTab.tsx",
    "web/src/components/modals/settings/IMBridgeTab.tsx",
}


class BaseRefError(RuntimeError):
    pass


def _is_excluded(path: Path) -> bool:
    if any(part in EXCLUDED_PARTS for part in path.parts):
        return True
    name = path.name
    return name.startswith("test_") or name.endswith("_test.py") or name.endswith(TEST_SUFFIXES)


def _is_source_path(path: Path) -> bool:
    relative = path.as_posix()
    in_python = relative.startswith("src/cccc/") and path.suffix == ".py"
    in_web = relative.startswith("web/src/") and path.suffix in {".ts", ".tsx"}
    return (in_python or in_web) and not _is_excluded(path)


def discover_source_files(root: Path) -> list[str]:
    root = root.resolve()
    candidates = [*root.glob("src/cccc/**/*.py"), *root.glob("web/src/**/*.ts"), *root.glob("web/src/**/*.tsx")]
    return sorted(
        path.relative_to(root).as_posix()
        for path in candidates
        if path.is_file() and _is_source_path(path.relative_to(root))
    )


def count_lines(path: Path) -> int:
    return len(path.read_text(encoding="utf-8", errors="replace").splitlines())


def current_oversized_files(root: Path, limit: int = DEFAULT_LIMIT) -> dict[str, int]:
    return {
        relative: lines
        for relative in discover_source_files(root)
        if (lines := count_lines(root / relative)) > limit
    }


def check_source_sizes(
    root: Path,
    baseline: Mapping[str, int],
    *,
    base_baseline: Mapping[str, int] | None = None,
    formatter_migration: Mapping[str, Mapping[str, object]] | None = None,
    preexisting_reviewed: Mapping[str, Mapping[str, object]] | None = None,
    base_blob_oids: Mapping[str, str] | None = None,
    base_line_counts: Mapping[str, int] | None = None,
    limit: int = DEFAULT_LIMIT,
) -> list[str]:
    root = root.resolve()
    errors: list[str] = []

    migration = formatter_migration or {}
    reviewed = preexisting_reviewed or {}
    overlap_paths = set(migration) & set(reviewed)
    errors.extend(
        f"{relative} appears in both formatter and reviewed manifests"
        for relative in sorted(overlap_paths)
    )
    migration_paths: set[str] = set()
    if base_baseline is not None:
        for relative, allowed in sorted(baseline.items()):
            previous = base_baseline.get(relative)
            if previous is not None and allowed <= previous:
                continue

            entry = migration.get(relative)
            reviewed_entry = reviewed.get(relative)
            if entry is not None and reviewed_entry is not None:
                entry = None
            if reviewed_entry is not None:
                migration_paths.add(relative)
                new_baseline = int(reviewed_entry["newBaseline"])
                old_baseline = int(reviewed_entry["oldBaseline"])
                current_lines_expected = int(reviewed_entry["currentLines"])
                base_lines_expected = int(reviewed_entry["baseLines"])
                if allowed != new_baseline:
                    errors.append(
                        f"{relative} reviewed baseline must be exactly {new_baseline}, got {allowed}"
                    )
                current_path = root / relative
                if not current_path.is_file():
                    errors.append(f"{relative} reviewed current file is missing")
                    continue
                if hashlib.sha256(current_path.read_bytes()).hexdigest() != str(
                    reviewed_entry["currentSha256"]
                ):
                    errors.append(f"{relative} reviewed current hash does not match")
                current_lines = count_lines(current_path)
                if current_lines != current_lines_expected:
                    errors.append(
                        f"{relative} reviewed current lines must be exactly "
                        f"{current_lines_expected}, got {current_lines}"
                    )
                if new_baseline != current_lines_expected:
                    errors.append(f"{relative} reviewed new baseline does not match current lines")
                if previous != old_baseline:
                    errors.append(f"{relative} reviewed old baseline does not match trusted base")
                if old_baseline != base_lines_expected:
                    errors.append(f"{relative} reviewed old baseline does not match base lines")
                if base_blob_oids is not None and base_blob_oids.get(relative, "") != str(
                    reviewed_entry["baseBlobOid"]
                ):
                    errors.append(f"{relative} reviewed base blob does not match")
                if base_line_counts is not None and base_line_counts.get(relative) != base_lines_expected:
                    errors.append(f"{relative} reviewed base lines do not match")
                continue
            if entry is None:
                if previous is None:
                    errors.append(f"{relative} adds a new oversized-file baseline of {allowed}")
                else:
                    errors.append(f"{relative} baseline was raised from {previous} to {allowed}")
                continue

            migration_paths.add(relative)
            formatted_lines = int(entry["formattedLines"])
            base_lines = int(entry["baseLines"])
            if allowed != formatted_lines:
                errors.append(
                    f"{relative} formatter migration requires an exact baseline of "
                    f"{formatted_lines}, got {allowed}"
                )

            current_path = root / relative
            if not current_path.is_file():
                errors.append(f"{relative} formatter migration current file is missing")
                continue
            current_sha256 = hashlib.sha256(current_path.read_bytes()).hexdigest()
            if current_sha256 != str(entry["formattedSha256"]):
                errors.append(f"{relative} formatter migration hash does not match the current file")
            current_lines = count_lines(current_path)
            if current_lines != formatted_lines:
                errors.append(
                    f"{relative} formatter migration requires exactly {formatted_lines} lines, "
                    f"got {current_lines}"
                )

            if base_blob_oids is not None:
                actual_oid = base_blob_oids.get(relative, "")
                if actual_oid != str(entry["baseBlobOid"]):
                    errors.append(f"{relative} formatter migration base blob does not match")

            if previous is None:
                if base_lines > limit:
                    errors.append(
                        f"{relative} formatter migration cannot add a baseline because the base "
                        f"had {base_lines} lines"
                    )
                if formatted_lines <= limit:
                    errors.append(
                        f"{relative} formatter migration cannot add a baseline at "
                        f"{formatted_lines} lines"
                    )
            elif previous != base_lines:
                errors.append(
                    f"{relative} formatter migration base line count {base_lines} does not match "
                    f"the old baseline {previous}"
                )

    discovered = discover_source_files(root)
    current = {relative: count_lines(root / relative) for relative in discovered}

    for relative, allowed in sorted(baseline.items()):
        if relative in migration_paths:
            continue
        lines = current.get(relative)
        if lines is None:
            errors.append(f"{relative} no longer exists; remove its stale baseline entry")
        elif lines <= limit:
            errors.append(f"{relative} is now {lines} lines; remove its obsolete baseline entry")
        elif lines > allowed:
            errors.append(f"{relative} grew to {lines} lines above its baseline of {allowed}")
        elif lines < allowed:
            errors.append(f"{relative} decreased to {lines} lines; lower its baseline from {allowed} to {lines}")

    for relative, lines in sorted(current.items()):
        if lines > limit and relative not in baseline:
            errors.append(f"{relative} has {lines} lines; new source files must not exceed {limit}")

    return errors


def _load_json_object(raw: str, error: str) -> dict[str, object]:
    document = json.loads(raw)
    if not isinstance(document, dict):
        raise ValueError(error)
    return document


def load_baseline(path: Path) -> dict[str, int]:
    error = f"invalid source-size baseline: {path}"
    document = _load_json_object(path.read_text(encoding="utf-8"), error)
    files = document.get("files", document)
    if not isinstance(files, dict):
        raise ValueError(error)
    return {str(relative): int(lines) for relative, lines in files.items()}


def load_formatter_migration(path: Path) -> dict[str, dict[str, object]]:
    if not path.exists():
        return {}
    document = _load_json_object(
        path.read_text(encoding="utf-8"), f"invalid formatter migration metadata: {path}"
    )
    if document.get("version") != 1 or document.get("formatter") != {
        "name": "oxfmt",
        "version": "0.57.0",
    }:
        raise ValueError(f"invalid formatter migration metadata: {path}")
    raw_files = document.get("files")
    if not isinstance(raw_files, list):
        raise ValueError(f"invalid formatter migration metadata: {path}")
    expected_keys = {"path", "baseBlobOid", "formattedSha256", "baseLines", "formattedLines"}
    entries: dict[str, dict[str, object]] = {}
    for raw_entry in raw_files:
        if not isinstance(raw_entry, dict) or set(raw_entry) != expected_keys:
            raise ValueError(f"invalid formatter migration entry: {path}")
        relative = str(raw_entry["path"])
        relative_path = Path(relative)
        if relative_path.is_absolute() or relative_path.as_posix() != relative or not _is_source_path(relative_path):
            raise ValueError(f"invalid formatter migration path: {relative}")
        if (
            len(str(raw_entry["baseBlobOid"])) not in {40, 64}
            or any(character not in "0123456789abcdef" for character in str(raw_entry["baseBlobOid"]))
            or len(str(raw_entry["formattedSha256"])) != 64
            or any(character not in "0123456789abcdef" for character in str(raw_entry["formattedSha256"]))
            or type(raw_entry["baseLines"]) is not int
            or type(raw_entry["formattedLines"]) is not int
        ):
            raise ValueError(f"invalid formatter migration entry: {relative}")
        if relative in entries:
            raise ValueError(f"duplicate formatter migration path: {relative}")
        entries[relative] = dict(raw_entry)
    return entries


def load_preexisting_reviewed(path: Path) -> dict[str, dict[str, object]]:
    if not path.exists():
        return {}
    document = _load_json_object(
        path.read_text(encoding="utf-8"), f"invalid preexisting reviewed manifest: {path}"
    )
    expected_keys = {
        "path",
        "baseBlobOid",
        "currentSha256",
        "baseLines",
        "currentLines",
        "oldBaseline",
        "newBaseline",
    }
    raw_files = document.get("files", [])
    if document.get("version") != 1 or not isinstance(raw_files, list):
        raise ValueError(f"invalid preexisting reviewed manifest: {path}")
    paths = {str(entry.get("path", "")) for entry in raw_files if isinstance(entry, dict)}
    if paths != PREEXISTING_REVIEWED_PATHS or len(raw_files) != len(PREEXISTING_REVIEWED_PATHS):
        raise ValueError("preexisting reviewed manifest must contain exactly the four reviewed paths")

    entries: dict[str, dict[str, object]] = {}
    for raw_entry in raw_files:
        if not isinstance(raw_entry, dict) or set(raw_entry) != expected_keys:
            raise ValueError(f"invalid preexisting reviewed entry: {path}")
        relative = str(raw_entry["path"])
        if (
            len(str(raw_entry["baseBlobOid"])) not in {40, 64}
            or any(character not in "0123456789abcdef" for character in str(raw_entry["baseBlobOid"]))
            or len(str(raw_entry["currentSha256"])) != 64
            or any(character not in "0123456789abcdef" for character in str(raw_entry["currentSha256"]))
            or any(
                type(raw_entry[key]) is not int
                for key in ("baseLines", "currentLines", "oldBaseline", "newBaseline")
            )
        ):
            raise ValueError(f"invalid preexisting reviewed entry: {relative}")
        entries[relative] = dict(raw_entry)
    return entries


def verify_reviewed_manifest_lifecycle(root: Path, base_ref: str, path: Path) -> list[str]:
    relative = path.resolve().relative_to(root.resolve()).as_posix()
    result = subprocess.run(
        ["git", "show", f"{base_ref}:{relative}"],
        cwd=root,
        capture_output=True,
    )
    if result.returncode != 0:
        return []
    if result.stdout != path.read_bytes():
        return [f"{relative} must remain byte-for-byte unchanged"]
    return []


def load_blob_oids_from_git(root: Path, ref: str, relatives: set[str]) -> dict[str, str]:
    result = subprocess.run(
        ["git", "ls-tree", "-r", ref, "--", "web/src"],
        cwd=root,
        check=True,
        capture_output=True,
        text=True,
    )
    oids: dict[str, str] = {}
    for line in result.stdout.splitlines():
        metadata, relative = line.split("\t", 1)
        if relative in relatives:
            _mode, kind, oid = metadata.split()
            if kind == "blob":
                oids[relative] = oid
    return oids


def load_line_counts_from_git(root: Path, ref: str, relatives: set[str]) -> dict[str, int]:
    counts: dict[str, int] = {}
    for relative in relatives:
        result = subprocess.run(
            ["git", "show", f"{ref}:{relative}"],
            cwd=root,
            check=True,
            capture_output=True,
        )
        counts[relative] = len(result.stdout.decode("utf-8", errors="replace").splitlines())
    return counts


def _git_output(root: Path, *args: str) -> str:
    result = subprocess.run(
        ["git", *args],
        cwd=root,
        capture_output=True,
        text=True,
    )
    return result.stdout.strip() if result.returncode == 0 else ""


def _resolve_commit(root: Path, ref: str) -> str:
    candidate = str(ref or "").strip()
    if not candidate or set(candidate) == {"0"}:
        return ""
    return _git_output(root, "rev-parse", "--verify", f"{candidate}^{{commit}}")


def resolve_base_ref(
    root: Path,
    *,
    explicit: str = "",
    environ: Mapping[str, str] | None = None,
) -> str:
    root = root.resolve()
    environment = os.environ if environ is None else environ
    requested = str(explicit or environment.get(BASE_REF_ENV, "")).strip()
    if requested:
        commit = _resolve_commit(root, requested)
        if not commit:
            raise BaseRefError(f"base ref does not resolve to a commit: {requested}")
        return commit

    head = _resolve_commit(root, "HEAD")
    if not head:
        raise BaseRefError("repository has no HEAD commit")

    candidates: list[str] = []
    upstream = _git_output(root, "rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{upstream}")
    for candidate in (upstream, "origin/main", "origin/master"):
        if candidate and candidate not in candidates:
            candidates.append(candidate)
    for candidate in candidates:
        commit = _resolve_commit(root, candidate)
        if not commit:
            continue
        merge_base = _git_output(root, "merge-base", head, commit)
        if merge_base:
            return merge_base
    raise BaseRefError(
        f"cannot resolve a trusted base ref; pass --base-ref or set {BASE_REF_ENV}, "
        "or use --bootstrap-baseline only for the first baseline"
    )


def load_baseline_from_git(root: Path, ref: str, baseline_path: Path) -> dict[str, int]:
    relative = baseline_path.resolve().relative_to(root.resolve()).as_posix()
    result = subprocess.run(
        ["git", "show", f"{ref}:{relative}"],
        cwd=root,
        capture_output=True,
        text=True,
    )
    if result.returncode == 0:
        error = f"invalid source-size baseline at {ref}:{relative}"
        document = _load_json_object(result.stdout, error)
        files = document.get("files", document)
        if not isinstance(files, dict):
            raise ValueError(error)
        return {str(path): int(lines) for path, lines in files.items()}

    archive = subprocess.run(
        ["git", "archive", "--format=tar", ref],
        cwd=root,
        check=True,
        capture_output=True,
    ).stdout
    derived: dict[str, int] = {}
    with tarfile.open(fileobj=io.BytesIO(archive), mode="r:") as tar:
        for member in tar.getmembers():
            relative_path = Path(member.name)
            if not member.isfile() or not _is_source_path(relative_path):
                continue
            extracted = tar.extractfile(member)
            if extracted is None:
                continue
            lines = len(extracted.read().decode("utf-8", errors="replace").splitlines())
            if lines > DEFAULT_LIMIT:
                derived[relative_path.as_posix()] = lines
    return dict(sorted(derived.items()))


def write_baseline(root: Path, path: Path, limit: int) -> None:
    document = {"version": 1, "limit": limit, "files": current_oversized_files(root, limit)}
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(document, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description="Enforce the production source-file line-count ratchet.")
    parser.add_argument("--root", type=Path, default=Path.cwd())
    parser.add_argument("--baseline", type=Path)
    parser.add_argument("--formatter-migration", type=Path)
    parser.add_argument("--preexisting-reviewed", type=Path)
    parser.add_argument("--base-ref", help="Git ref whose baseline must not be raised")
    parser.add_argument(
        "--bootstrap-baseline",
        action="store_true",
        help="Explicitly allow the first baseline when no trusted history exists",
    )
    parser.add_argument("--limit", type=int, default=DEFAULT_LIMIT)
    parser.add_argument("--write-baseline", action="store_true")
    return parser


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    root = args.root.resolve()
    baseline_path = (args.baseline or root / DEFAULT_BASELINE).resolve()
    formatter_migration_path = (
        args.formatter_migration or root / DEFAULT_FORMATTER_MIGRATION
    ).resolve()
    preexisting_reviewed_path = (
        args.preexisting_reviewed or root / DEFAULT_PREEXISTING_REVIEWED
    ).resolve()
    if args.write_baseline:
        write_baseline(root, baseline_path, args.limit)
        print(f"wrote {baseline_path.relative_to(root)}")
        return 0

    try:
        baseline = load_baseline(baseline_path)
        formatter_migration = load_formatter_migration(formatter_migration_path)
        preexisting_reviewed = load_preexisting_reviewed(preexisting_reviewed_path)
        if args.bootstrap_baseline:
            base_ref = ""
            base_baseline = None
        else:
            base_ref = resolve_base_ref(root, explicit=args.base_ref)
            base_baseline = load_baseline_from_git(root, base_ref, baseline_path)
        manifest_paths = set(formatter_migration) | set(preexisting_reviewed)
        base_blob_oids = load_blob_oids_from_git(root, base_ref, manifest_paths) if base_ref else None
        base_line_counts = (
            load_line_counts_from_git(root, base_ref, set(preexisting_reviewed)) if base_ref else None
        )
        lifecycle_errors = (
            verify_reviewed_manifest_lifecycle(root, base_ref, preexisting_reviewed_path)
            if base_ref and preexisting_reviewed
            else []
        )
        errors = check_source_sizes(
            root,
            baseline,
            base_baseline=base_baseline,
            formatter_migration=formatter_migration,
            preexisting_reviewed=preexisting_reviewed,
            base_blob_oids=base_blob_oids,
            base_line_counts=base_line_counts,
            limit=args.limit,
        )
    except (BaseRefError, ValueError, OSError, subprocess.CalledProcessError) as exc:
        print(f"Source-size gate failed: {exc}", file=sys.stderr)
        return 2
    errors = [*lifecycle_errors, *errors]
    if errors:
        print("Source-size gate failed:", file=sys.stderr)
        for error in errors:
            print(f"- {error}", file=sys.stderr)
        return 1
    comparison = "explicit bootstrap" if args.bootstrap_baseline else f"base {base_ref[:12]}"
    print(f"Source-size gate passed ({len(discover_source_files(root))} files checked; {comparison}).")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
