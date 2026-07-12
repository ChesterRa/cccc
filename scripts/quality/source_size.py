#!/usr/bin/env python3
from __future__ import annotations

import argparse
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
EXCLUDED_PARTS = {"__pycache__", "_vendor", "dist", "generated", "node_modules", "tests", "vendor"}
TEST_SUFFIXES = (".test.ts", ".test.tsx", ".spec.ts", ".spec.tsx")
BASE_REF_ENV = "SOURCE_SIZE_BASE_REF"


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
    limit: int = DEFAULT_LIMIT,
) -> list[str]:
    root = root.resolve()
    errors: list[str] = []

    if base_baseline is not None:
        for relative, allowed in sorted(baseline.items()):
            previous = base_baseline.get(relative)
            if previous is None:
                errors.append(f"{relative} adds a new oversized-file baseline of {allowed}")
            elif allowed > previous:
                errors.append(f"{relative} baseline was raised from {previous} to {allowed}")

    discovered = discover_source_files(root)
    current = {relative: count_lines(root / relative) for relative in discovered}

    for relative, allowed in sorted(baseline.items()):
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


def load_baseline(path: Path) -> dict[str, int]:
    document = json.loads(path.read_text(encoding="utf-8"))
    files = document.get("files", document)
    if not isinstance(files, dict):
        raise ValueError(f"invalid source-size baseline: {path}")
    return {str(relative): int(lines) for relative, lines in files.items()}


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
        document = json.loads(result.stdout)
        files = document.get("files", document)
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
    if args.write_baseline:
        write_baseline(root, baseline_path, args.limit)
        print(f"wrote {baseline_path.relative_to(root)}")
        return 0

    baseline = load_baseline(baseline_path)
    if args.bootstrap_baseline:
        base_ref = ""
        base_baseline = None
    else:
        try:
            base_ref = resolve_base_ref(root, explicit=args.base_ref)
        except BaseRefError as exc:
            print(f"Source-size gate failed: {exc}", file=sys.stderr)
            return 2
        base_baseline = load_baseline_from_git(root, base_ref, baseline_path)
    errors = check_source_sizes(root, baseline, base_baseline=base_baseline, limit=args.limit)
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
