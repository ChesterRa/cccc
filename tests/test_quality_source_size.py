from __future__ import annotations

import hashlib
import json
import subprocess
import sys
from pathlib import Path

import pytest

from scripts.quality import source_size
from scripts.quality.source_size import check_source_sizes, discover_source_files, load_baseline_from_git


def _write_lines(path: Path, count: int) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("line\n" * count, encoding="utf-8")


def _sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


REVIEWED_PATHS = [
    "web/src/components/AgentTab.tsx",
    "web/src/components/ContextModal/index.tsx",
    "web/src/components/browser/ProjectedBrowserSurfacePanel.tsx",
    "web/src/components/modals/ActorConfigModal.tsx",
    "web/src/components/modals/settings/GuidanceTab.tsx",
    "web/src/components/modals/settings/IMBridgeTab.tsx",
]


def _reviewed_manifest(root: Path, *, lines: int = 330) -> dict[str, object]:
    files = []
    for index, relative in enumerate(REVIEWED_PATHS):
        source = root / relative
        _write_lines(source, lines)
        files.append(
            {
                "path": relative,
                "baseBlobOid": f"{index + 1:040x}",
                "currentSha256": _sha256(source),
                "baseLines": 320,
                "currentLines": lines,
                "oldBaseline": 320,
                "newBaseline": lines,
            }
        )
    return {"version": 1, "files": files}


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


def test_exact_formatter_migration_allows_only_the_recorded_baseline_raise(tmp_path: Path) -> None:
    source = tmp_path / "web/src/legacy.ts"
    _write_lines(source, 330)
    migration = {
        "web/src/legacy.ts": {
            "path": "web/src/legacy.ts",
            "baseBlobOid": "a" * 40,
            "formattedSha256": _sha256(source),
            "baseLines": 320,
            "formattedLines": 330,
        }
    }

    assert check_source_sizes(
        tmp_path,
        baseline={"web/src/legacy.ts": 330},
        base_baseline={"web/src/legacy.ts": 320},
        formatter_migration=migration,
    ) == []

    assert check_source_sizes(
        tmp_path,
        baseline={"web/src/legacy.ts": 331},
        base_baseline={"web/src/legacy.ts": 320},
        formatter_migration=migration,
    ) == ["web/src/legacy.ts formatter migration requires an exact baseline of 330, got 331"]


def test_formatter_migration_rejects_changed_bytes_and_non_crossing_new_baselines(tmp_path: Path) -> None:
    source = tmp_path / "web/src/new_legacy.ts"
    _write_lines(source, 310)
    migration = {
        "web/src/new_legacy.ts": {
            "path": "web/src/new_legacy.ts",
            "baseBlobOid": "b" * 40,
            "formattedSha256": "0" * 64,
            "baseLines": 301,
            "formattedLines": 310,
        }
    }

    assert check_source_sizes(
        tmp_path,
        baseline={"web/src/new_legacy.ts": 310},
        base_baseline={},
        formatter_migration=migration,
    ) == [
        "web/src/new_legacy.ts formatter migration hash does not match the current file",
        "web/src/new_legacy.ts formatter migration cannot add a baseline because the base had 301 lines",
    ]


def test_formatter_migration_loader_rejects_paths_outside_web_sources(tmp_path: Path) -> None:
    manifest_path = tmp_path / "migration.json"
    manifest_path.write_text(
        json.dumps(
            {
                "version": 1,
                "formatter": {"name": "oxfmt", "version": "0.57.0"},
                "files": [
                    {
                        "path": "../secrets.ts",
                        "baseBlobOid": "a" * 40,
                        "formattedSha256": "b" * 64,
                        "baseLines": 300,
                        "formattedLines": 301,
                    }
                ],
            }
        ),
        encoding="utf-8",
    )

    try:
        source_size.load_formatter_migration(manifest_path)
    except ValueError as exc:
        assert str(exc) == "invalid formatter migration path: ../secrets.ts"
    else:
        raise AssertionError("path traversal manifest entry was accepted")


def test_preexisting_reviewed_manifest_requires_exactly_four_fixed_paths(tmp_path: Path) -> None:
    manifest = _reviewed_manifest(tmp_path)
    manifest_path = tmp_path / "reviewed.json"

    for files in (
        manifest["files"][:-1],
        [*manifest["files"], {**manifest["files"][0], "path": "web/src/pages/chat/ChatComposer.tsx"}],
        [*manifest["files"], {**manifest["files"][0], "path": "web/src/extra.ts"}],
    ):
        manifest_path.write_text(json.dumps({"version": 1, "files": files}), encoding="utf-8")
        try:
            source_size.load_preexisting_reviewed(manifest_path)
        except ValueError as exc:
            assert "must contain exactly the four reviewed paths" in str(exc)
        else:
            raise AssertionError("invalid reviewed path set was accepted")


def test_preexisting_reviewed_raise_is_exact_and_disjoint_from_formatter_manifest(
    tmp_path: Path,
) -> None:
    manifest = _reviewed_manifest(tmp_path)
    reviewed = {entry["path"]: entry for entry in manifest["files"]}
    base_baseline = {relative: 320 for relative in REVIEWED_PATHS}
    baseline = {relative: 330 for relative in REVIEWED_PATHS}
    base_blob_oids = {
        relative: f"{index + 1:040x}" for index, relative in enumerate(REVIEWED_PATHS)
    }
    base_line_counts = {relative: 320 for relative in REVIEWED_PATHS}

    assert check_source_sizes(
        tmp_path,
        baseline,
        base_baseline=base_baseline,
        preexisting_reviewed=reviewed,
        base_blob_oids=base_blob_oids,
        base_line_counts=base_line_counts,
    ) == []

    reviewed[REVIEWED_PATHS[0]]["currentSha256"] = "0" * 64
    errors = check_source_sizes(
        tmp_path,
        baseline,
        base_baseline=base_baseline,
        formatter_migration={REVIEWED_PATHS[0]: {}},
        preexisting_reviewed=reviewed,
        base_blob_oids=base_blob_oids,
        base_line_counts=base_line_counts,
    )
    assert errors == [
        f"{REVIEWED_PATHS[0]} appears in both formatter and reviewed manifests",
        f"{REVIEWED_PATHS[0]} reviewed current hash does not match",
    ]


def test_preexisting_reviewed_rejects_non_exact_smaller_current_file(tmp_path: Path) -> None:
    manifest = _reviewed_manifest(tmp_path)
    relative = REVIEWED_PATHS[0]
    reviewed = {entry["path"]: entry for entry in manifest["files"]}
    for other in REVIEWED_PATHS[1:]:
        (tmp_path / other).unlink()
    _write_lines(tmp_path / relative, 329)

    errors = check_source_sizes(
        tmp_path,
        {relative: 329},
        base_baseline={relative: 320},
        preexisting_reviewed={relative: reviewed[relative]},
        base_blob_oids={relative: reviewed[relative]["baseBlobOid"]},
        base_line_counts={relative: 320},
    )
    assert errors == [
        f"{relative} reviewed baseline must be exactly 330, got 329",
        f"{relative} reviewed current hash does not match",
        f"{relative} reviewed current lines must be exactly 330, got 329",
    ]


def test_preexisting_reviewed_manifest_is_immutable_once_present_in_base(tmp_path: Path) -> None:
    _init_git_repo(tmp_path)
    manifest_path = tmp_path / "scripts/quality/preexisting-reviewed-v1.json"
    manifest_path.parent.mkdir(parents=True, exist_ok=True)
    manifest_path.write_text(json.dumps(_reviewed_manifest(tmp_path), indent=2) + "\n", encoding="utf-8")
    subprocess.run(["git", "add", "."], cwd=tmp_path, check=True)
    subprocess.run(["git", "commit", "-qm", "reviewed manifest"], cwd=tmp_path, check=True)
    base = subprocess.run(
        ["git", "rev-parse", "HEAD"], cwd=tmp_path, check=True, capture_output=True, text=True
    ).stdout.strip()

    assert source_size.verify_reviewed_manifest_lifecycle(tmp_path, base, manifest_path) == []
    manifest_path.write_text(manifest_path.read_text(encoding="utf-8") + "\n", encoding="utf-8")
    assert source_size.verify_reviewed_manifest_lifecycle(tmp_path, base, manifest_path) == [
        "scripts/quality/preexisting-reviewed-v1.json must remain byte-for-byte unchanged"
    ]


@pytest.mark.parametrize(
    ("manifest_content", "message_fragment"),
    [
        ("[", "Expecting value"),
        ("[]", "invalid preexisting reviewed manifest"),
    ],
)
def test_cli_reports_invalid_manifest_without_a_traceback(
    tmp_path: Path, manifest_content: str, message_fragment: str
) -> None:
    _init_git_repo(tmp_path)
    baseline_path = tmp_path / "scripts/quality/source-size-baseline.json"
    baseline_path.parent.mkdir(parents=True, exist_ok=True)
    baseline_path.write_text(
        json.dumps({"version": 1, "limit": 300, "files": {"src/cccc/legacy.py": 320}}),
        encoding="utf-8",
    )
    reviewed_path = tmp_path / "scripts/quality/preexisting-reviewed-v1.json"
    reviewed_path.write_text(manifest_content, encoding="utf-8")

    result = subprocess.run(
        [
            sys.executable,
            str(Path(source_size.__file__).resolve()),
            "--root",
            str(tmp_path),
            "--baseline",
            str(baseline_path),
            "--preexisting-reviewed",
            str(reviewed_path),
            "--bootstrap-baseline",
        ],
        cwd=tmp_path,
        capture_output=True,
        text=True,
        check=False,
    )

    assert result.returncode == 2
    assert result.stdout == ""
    assert result.stderr.startswith("Source-size gate failed: ")
    assert message_fragment in result.stderr
    assert "Traceback" not in result.stderr


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
