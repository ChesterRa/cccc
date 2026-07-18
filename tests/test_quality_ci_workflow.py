from __future__ import annotations

from pathlib import Path

import yaml


ROOT = Path(__file__).resolve().parents[1]


def _workflow() -> dict:
    return yaml.load((ROOT / ".github/workflows/ci.yml").read_text(encoding="utf-8"), Loader=yaml.BaseLoader)


def _runs(job: dict) -> str:
    return "\n".join(step.get("run", "") for step in job.get("steps", []))


def test_pr_jobs_keep_full_quality_web_python_and_package_boundaries() -> None:
    jobs = _workflow()["jobs"]

    assert {"quality", "web", "python-tests", "package", "windows-smoke", "nightly-serial"} <= set(jobs)
    assert set(jobs["package"]["needs"]) == {"quality", "web", "python-tests"}
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


def test_ci_does_not_carry_retired_source_size_or_one_time_migration_governance() -> None:
    runs = "\n".join(_runs(job) for job in _workflow()["jobs"].values())

    assert "source_size.py" not in runs
    assert "verify_oxfmt_migration" not in runs
    assert "test:quality" not in runs


def test_pr_python_matrix_uses_four_stable_file_shards_without_xdist() -> None:
    job = _workflow()["jobs"]["python-tests"]
    runs = _runs(job)

    assert job["strategy"]["matrix"]["shard"] == ["0", "1", "2", "3"]
    assert "scripts/quality/pytest_shards.py" in runs
    assert "--total 4" in runs
    assert "env -u CCCC_GROUP_ID -u CCCC_ACTOR_ID python -m pytest" in runs
    assert '-m "not packaged_web_dist"' in runs
    assert "pytest-xdist" not in runs
    assert " -n " not in runs


def test_package_job_owns_the_built_web_bundle_contract() -> None:
    package = _workflow()["jobs"]["package"]
    runs = _runs(package)

    assert any(step.get("uses", "").startswith("actions/download-artifact") for step in package["steps"])
    assert "-m packaged_web_dist tests/test_web_manifest_static.py" in runs


def test_schedule_runs_one_serial_full_python_suite() -> None:
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
