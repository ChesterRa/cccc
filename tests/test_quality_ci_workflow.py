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
    assert "source_size.py" in _runs(jobs["quality"])
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
    checkout = next(step for step in web["steps"] if step.get("uses", "").startswith("actions/checkout"))
    assert checkout["with"]["fetch-depth"] == "0"
    assert "verify_oxfmt_migration.mjs" in runs
    assert "npm -C web run test:quality" in runs
    assert "github.event.pull_request.base.sha" in runs
    assert "github.event.before" in runs
    assert "npm -C web run check" in runs
    assert "npm -C web run typecheck" not in runs
    assert "npm -C web run lint" not in runs


def test_windows_smoke_runs_the_portable_oxfmt_verifier() -> None:
    windows = _workflow()["jobs"]["windows-smoke"]
    runs = _runs(windows)
    checkout = next(
        step for step in windows["steps"] if step.get("uses", "").startswith("actions/checkout")
    )
    node_setup = next(
        step for step in windows["steps"] if step.get("uses", "").startswith("actions/setup-node")
    )
    verifier = next(
        step for step in windows["steps"] if "verify_oxfmt_migration.mjs" in step.get("run", "")
    )

    assert checkout["with"]["fetch-depth"] == "0"
    assert node_setup["with"]["node-version"] == "20.19.5"
    assert "npm ci --prefix web" in runs
    assert "verify_oxfmt_migration.mjs" in runs
    assert "npm -C web run test:quality" in runs
    assert verifier["env"] == {
        "EVENT_NAME": "${{ github.event_name }}",
        "PR_BASE": "${{ github.event.pull_request.base.sha }}",
        "PUSH_BEFORE": "${{ github.event.before }}",
        "CURRENT_SHA": "${{ github.sha }}",
    }


def test_source_size_uses_pr_base_push_before_and_explicit_first_push_bootstrap() -> None:
    runs = _runs(_workflow()["jobs"]["quality"])

    assert "github.event.pull_request.base.sha" in runs
    assert "github.event.before" in runs
    assert "--base-ref" in runs
    assert "--bootstrap-baseline" in runs


def test_pr_python_matrix_uses_four_stable_file_shards_without_xdist() -> None:
    job = _workflow()["jobs"]["python-tests"]
    runs = _runs(job)

    assert job["strategy"]["matrix"]["shard"] == ["0", "1", "2", "3"]
    assert "scripts/quality/pytest_shards.py" in runs
    assert "--total 4" in runs
    assert "env -u CCCC_GROUP_ID -u CCCC_ACTOR_ID python -m pytest" in runs
    assert "pytest-xdist" not in runs
    assert " -n " not in runs


def test_schedule_runs_one_serial_full_python_suite() -> None:
    workflow = _workflow()
    nightly = workflow["jobs"]["nightly-serial"]
    runs = _runs(nightly)

    assert "schedule" in workflow["on"]
    assert "github.event_name == 'schedule'" in nightly["if"]
    assert "python -m pytest tests/" in runs
    assert "env -u CCCC_GROUP_ID -u CCCC_ACTOR_ID python -m pytest tests/" in runs
    assert "pytest_shards.py" not in runs
    assert "pytest-xdist" not in runs
    assert " -n " not in runs
