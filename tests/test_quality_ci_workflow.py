from __future__ import annotations

from pathlib import Path

import yaml


ROOT = Path(__file__).resolve().parents[1]


def _workflow() -> dict:
    return yaml.load((ROOT / ".github/workflows/ci.yml").read_text(encoding="utf-8"), Loader=yaml.BaseLoader)


def _release_workflow() -> dict:
    return yaml.load((ROOT / ".github/workflows/release.yml").read_text(encoding="utf-8"), Loader=yaml.BaseLoader)


def _runs(job: dict) -> str:
    return "\n".join(step.get("run", "") for step in job.get("steps", []))


def test_pr_jobs_keep_full_quality_web_python_and_package_boundaries() -> None:
    jobs = _workflow()["jobs"]

    assert {"quality", "web", "python-tests", "python-compat", "package", "windows-smoke", "nightly-serial"} <= set(
        jobs
    )
    assert set(jobs["package"]["needs"]) == {"quality", "web", "python-tests", "python-compat"}
    assert "ruff check" in _runs(jobs["quality"])
    assert "npm -C web test" in _runs(jobs["web"])
    assert "npm -C web run build" in _runs(jobs["web"])
    assert any(step.get("uses", "").startswith("actions/upload-artifact") for step in jobs["web"]["steps"])
    assert any(step.get("uses", "").startswith("actions/download-artifact") for step in jobs["package"]["steps"])


def test_web_ci_uses_managed_node_and_composite_vite_plus_check() -> None:
    web = _workflow()["jobs"]["web"]
    runs = _runs(web)
    node_setup = next(step for step in web["steps"] if step.get("uses", "").startswith("actions/setup-node"))

    assert node_setup["with"]["node-version"] == "20.19.5"
    assert "npm -C web run check" in runs
    assert "npm -C web run typecheck" not in runs
    assert "npm -C web run lint" not in runs


def test_windows_smoke_keeps_the_product_pty_checks_without_web_migration_setup() -> None:
    windows = _workflow()["jobs"]["windows-smoke"]
    runs = _runs(windows)
    uses = {step.get("uses", "") for step in windows["steps"]}

    assert "tests/test_socket_special_ops.py" in runs
    assert "tests/test_windows_pty_backend.py" in runs
    assert not any(item.startswith("actions/setup-node") for item in uses)
    assert "npm " not in runs


def test_rust_job_is_python_free_and_serializes_daemon_tests() -> None:
    job = _workflow()["jobs"]["rust"]
    runs = _runs(job)
    uses = {step.get("uses", "") for step in job["steps"]}

    assert "env" not in job
    assert not any(item.startswith("actions/setup-python") for item in uses)
    assert "python -m" not in runs.lower()
    assert "pip install" not in runs.lower()
    assert "scripts/check_version_parity.sh" not in runs
    assert "cargo test --workspace --exclude cccc-pair-daemon --locked" in runs
    assert (
        "cargo test --package cccc-pair-daemon --locked"
        in runs
    )
    for legacy_test in (
        "python_and_rust_share_context_tasks_and_version_state",
        "python_and_rust_share_identity_and_signed_session_hello",
        "python_and_rust_processes_share_paths_files_and_locks",
        "python_and_rust_share_launch_identity_format",
        "rust_append_waits_for_the_python_ledger_lock",
        "python_and_rust_share_persisted_control_plane_state",
        "python_and_rust_accept_each_others_group_copy_packages",
    ):
        assert f"--skip {legacy_test}" in runs


def test_ci_does_not_carry_retired_source_size_or_one_time_migration_governance() -> None:
    runs = "\n".join(_runs(job) for job in _workflow()["jobs"].values())

    assert "source_size.py" not in runs
    assert "verify_oxfmt_migration" not in runs
    assert "test:quality" not in runs


def test_pr_python_matrix_uses_four_stable_file_shards_without_xdist() -> None:
    job = _workflow()["jobs"]["python-tests"]
    runs = _runs(job)
    web_bundle = next(
        step
        for step in job["steps"]
        if step.get("uses", "").startswith("actions/download-artifact")
    )

    assert job["needs"] == "web"
    assert web_bundle["with"] == {
        "name": "bundled-web",
        "path": "src/cccc/ports/web/dist",
    }
    assert job["strategy"]["matrix"]["shard"] == ["0", "1", "2", "3"]
    assert "scripts/quality/pytest_shards.py" in runs
    assert "--total 4" in runs
    assert "env -u CCCC_GROUP_ID -u CCCC_ACTOR_ID python -m pytest" in runs
    assert '-m "not packaged_web_dist"' in runs
    assert "pytest-xdist" not in runs
    assert " -n " not in runs


def test_ci_exercises_the_supported_python_range_without_four_full_pr_suites() -> None:
    jobs = _workflow()["jobs"]

    for name in ("quality", "python-tests", "package", "windows-smoke"):
        setup = next(step for step in jobs[name]["steps"] if step.get("uses", "").startswith("actions/setup-python"))
        assert setup["with"]["python-version"] == "3.14"

    compat = jobs["python-compat"]
    assert compat["strategy"]["matrix"]["python-version"] == ["3.11", "3.12", "3.13"]
    compat_runs = _runs(compat)
    assert "python -W error::SyntaxWarning -m compileall -q src/cccc" in compat_runs
    assert "cccc version" in compat_runs
    assert '"method": "initialize"' in compat_runs

    nightly = jobs["nightly-serial"]
    assert nightly["strategy"]["matrix"]["python-version"] == ["3.11", "3.14"]


def test_package_job_owns_the_built_web_bundle_contract() -> None:
    package = _workflow()["jobs"]["package"]
    runs = _runs(package)

    assert any(step.get("uses", "").startswith("actions/download-artifact") for step in package["steps"])
    assert "-m packaged_web_dist tests/test_web_manifest_static.py" in runs


def test_schedule_runs_serial_full_python_suites_at_both_support_endpoints() -> None:
    workflow = _workflow()
    nightly = workflow["jobs"]["nightly-serial"]
    runs = _runs(nightly)

    assert "schedule" in workflow["on"]
    assert "github.event_name == 'schedule'" in nightly["if"]
    assert "python -m pytest tests/" in runs
    assert "env -u CCCC_GROUP_ID -u CCCC_ACTOR_ID python -m pytest tests/" in runs
    assert '-m "not packaged_web_dist"' in runs
    assert "pytest_shards.py" not in runs
    assert "pytest-xdist" not in runs
    assert " -n " not in runs


def test_release_builds_on_314_and_smokes_the_wheel_on_the_311_floor() -> None:
    jobs = _release_workflow()["jobs"]

    verify_setup = next(
        step for step in jobs["verify-linux"]["steps"] if step.get("uses", "").startswith("actions/setup-python")
    )
    publish_setup = next(
        step for step in jobs["publish"]["steps"] if step.get("uses", "").startswith("actions/setup-python")
    )
    assert verify_setup["with"]["python-version"] == "3.14"
    assert publish_setup["with"]["python-version"] == "3.14"

    platform_rows = jobs["verify-platform-smoke"]["strategy"]["matrix"]["include"]
    assert any(row["os"] == "ubuntu-latest" and row["python_version"] == "3.11" for row in platform_rows)
    assert all(row["python_version"] == "3.14" for row in platform_rows if row["os"] != "ubuntu-latest")
