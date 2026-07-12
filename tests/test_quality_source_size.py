from __future__ import annotations

import json
import subprocess
from pathlib import Path

from scripts.quality import source_size
from scripts.quality.source_size import check_source_sizes, discover_source_files, load_baseline_from_git


def _write_lines(path: Path, count: int) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("line\n" * count, encoding="utf-8")


def _init_git_repo(root: Path) -> str:
    subprocess.run(["git", "init", "-q"], cwd=root, check=True)
    subprocess.run(["git", "config", "user.email", "quality@example.test"], cwd=root, check=True)
    subprocess.run(["git", "config", "user.name", "Quality Test"], cwd=root, check=True)
    _write_lines(root / "src/cccc/legacy.py", 320)
    subprocess.run(["git", "add", "src"], cwd=root, check=True)
    subprocess.run(["git", "commit", "-qm", "base"], cwd=root, check=True)
    return subprocess.run(
        ["git", "rev-parse", "HEAD"], cwd=root, check=True, capture_output=True, text=True
    ).stdout.strip()


def test_discovery_includes_production_sources_and_excludes_tests_and_vendors(tmp_path: Path) -> None:
    included = [
        tmp_path / "src/cccc/kernel/group.py",
        tmp_path / "web/src/components/App.tsx",
        tmp_path / "web/src/utils/state.ts",
    ]
    excluded = [
        tmp_path / "src/cccc/vendor/copied.py",
        tmp_path / "src/cccc/providers/notebooklm/_vendor/copied.py",
        tmp_path / "web/src/components/App.test.tsx",
        tmp_path / "web/src/utils/state.test.ts",
        tmp_path / "web/src/generated/schema.ts",
        tmp_path / "web/src/dist/bundle.ts",
    ]
    for path in [*included, *excluded]:
        _write_lines(path, 1)

    discovered = discover_source_files(tmp_path)

    assert discovered == sorted(path.relative_to(tmp_path).as_posix() for path in included)


def test_unbaselined_source_over_limit_fails(tmp_path: Path) -> None:
    _write_lines(tmp_path / "src/cccc/new_module.py", 301)

    errors = check_source_sizes(tmp_path, baseline={})

    assert errors == ["src/cccc/new_module.py has 301 lines; new source files must not exceed 300"]


def test_existing_source_must_match_its_ratchet_baseline(tmp_path: Path) -> None:
    source = tmp_path / "src/cccc/legacy.py"
    _write_lines(source, 321)
    assert check_source_sizes(tmp_path, {"src/cccc/legacy.py": 320}) == [
        "src/cccc/legacy.py grew to 321 lines above its baseline of 320"
    ]

    _write_lines(source, 319)
    assert check_source_sizes(tmp_path, {"src/cccc/legacy.py": 320}) == [
        "src/cccc/legacy.py decreased to 319 lines; lower its baseline from 320 to 319"
    ]

    assert check_source_sizes(tmp_path, {"src/cccc/legacy.py": 319}) == []


def test_baseline_entry_is_removed_after_file_reaches_limit(tmp_path: Path) -> None:
    _write_lines(tmp_path / "web/src/legacy.ts", 300)

    errors = check_source_sizes(tmp_path, {"web/src/legacy.ts": 320})

    assert errors == ["web/src/legacy.ts is now 300 lines; remove its obsolete baseline entry"]


def test_merge_base_comparison_rejects_raised_or_new_baselines(tmp_path: Path) -> None:
    _write_lines(tmp_path / "src/cccc/legacy.py", 321)
    _write_lines(tmp_path / "src/cccc/new_legacy.py", 350)

    errors = check_source_sizes(
        tmp_path,
        baseline={"src/cccc/legacy.py": 321, "src/cccc/new_legacy.py": 350},
        base_baseline={"src/cccc/legacy.py": 320},
    )

    assert errors == [
        "src/cccc/legacy.py baseline was raised from 320 to 321",
        "src/cccc/new_legacy.py adds a new oversized-file baseline of 350",
    ]


def test_merge_base_without_baseline_derives_limits_from_source_archive(tmp_path: Path) -> None:
    _init_git_repo(tmp_path)
    _write_lines(tmp_path / "src/cccc/small.py", 20)

    baseline = load_baseline_from_git(
        tmp_path,
        "HEAD",
        tmp_path / "scripts/quality/source-size-baseline.json",
    )

    assert baseline == {"src/cccc/legacy.py": 320}


def test_base_ref_resolution_prefers_explicit_then_environment_then_remote_default(tmp_path: Path) -> None:
    head = _init_git_repo(tmp_path)
    subprocess.run(["git", "update-ref", "refs/remotes/origin/main", head], cwd=tmp_path, check=True)

    assert source_size.resolve_base_ref(tmp_path, explicit="HEAD") == head
    assert source_size.resolve_base_ref(
        tmp_path,
        environ={"SOURCE_SIZE_BASE_REF": "origin/main"},
    ) == head
    assert source_size.resolve_base_ref(tmp_path, environ={}) == head


def test_local_default_rejects_source_and_baseline_raised_together(tmp_path: Path) -> None:
    head = _init_git_repo(tmp_path)
    subprocess.run(["git", "update-ref", "refs/remotes/origin/main", head], cwd=tmp_path, check=True)
    _write_lines(tmp_path / "src/cccc/legacy.py", 321)
    baseline_path = tmp_path / "scripts/quality/source-size-baseline.json"
    baseline_path.parent.mkdir(parents=True)
    baseline_path.write_text(
        json.dumps({"version": 1, "limit": 300, "files": {"src/cccc/legacy.py": 321}}),
        encoding="utf-8",
    )

    result = source_size.main(["--root", str(tmp_path), "--baseline", str(baseline_path)])

    assert result == 1


def test_missing_trusted_base_requires_explicit_bootstrap(tmp_path: Path) -> None:
    _init_git_repo(tmp_path)
    baseline_path = tmp_path / "scripts/quality/source-size-baseline.json"
    baseline_path.parent.mkdir(parents=True)
    baseline_path.write_text(
        json.dumps({"version": 1, "limit": 300, "files": {"src/cccc/legacy.py": 320}}),
        encoding="utf-8",
    )
    assert source_size.main(["--root", str(tmp_path), "--baseline", str(baseline_path)]) == 2
    assert source_size.main([
        "--root", str(tmp_path),
        "--baseline", str(baseline_path),
        "--bootstrap-baseline",
    ]) == 0
