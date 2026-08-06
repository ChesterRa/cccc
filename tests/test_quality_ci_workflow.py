from __future__ import annotations

import json
import subprocess
import tomllib
from pathlib import Path

import yaml


ROOT = Path(__file__).resolve().parents[1]


def _workflow() -> dict:
    return yaml.load((ROOT / ".github/workflows/ci.yml").read_text(encoding="utf-8"), Loader=yaml.BaseLoader)


def _release_workflow() -> dict:
    return yaml.load((ROOT / ".github/workflows/release.yml").read_text(encoding="utf-8"), Loader=yaml.BaseLoader)


def _rust_release_workflow() -> dict:
    return yaml.load(
        (ROOT / ".github/workflows/release-rust.yml").read_text(encoding="utf-8"),
        Loader=yaml.BaseLoader,
    )


def _runs(job: dict) -> str:
    return "\n".join(step.get("run", "") for step in job.get("steps", []))


def test_pr_jobs_keep_full_quality_web_python_and_package_boundaries() -> None:
    jobs = _workflow()["jobs"]

    assert {"quality", "web", "python-tests", "python-compat", "package", "windows-smoke", "interop", "nightly-serial"} <= set(
        jobs
    )
    assert set(jobs["package"]["needs"]) == {"quality", "web", "python-tests", "python-compat", "interop"}
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
    assert "scripts/verify_native_wheel.py" in runs
    assert "pure-after-native" in runs


def test_interop_job_runs_the_cross_language_tests_skipped_by_the_python_free_rust_job() -> None:
    interop = _workflow()["jobs"]["interop"]
    runs = _runs(interop)
    uses = {step.get("uses", "") for step in interop["steps"]}

    assert interop["needs"] == "web"
    assert any(item.startswith("actions/setup-python") for item in uses)
    assert any(item.startswith("dtolnay/rust-toolchain") for item in uses)
    for test_binary in (
        "context_python_interop",
        "group_bridge_identity_interop",
        "runtime_hook_interop",
        "runtime_hook_identity_interop",
        "ledger_python_interop",
        "python_storage_interop",
    ):
        assert test_binary in runs


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


def test_release_builds_on_314_without_installed_implementation_smoke_jobs() -> None:
    jobs = _release_workflow()["jobs"]

    verify_setup = next(
        step for step in jobs["verify-python"]["steps"] if step.get("uses", "").startswith("actions/setup-python")
    )
    publish_setup = next(
        step for step in jobs["publish"]["steps"] if step.get("uses", "").startswith("actions/setup-python")
    )
    assert verify_setup["with"]["python-version"] == "3.14"
    assert publish_setup["with"]["python-version"] == "3.14"

    assert "universal-floor-smoke" not in jobs

    release_runs = "\n".join(_runs(job) for job in jobs.values())
    for command in (
        "python -m pip install --force-reinstall",
        "cccc rust version",
        "cccc rust --version",
        "cccc python version",
        "cccc python doctor",
    ):
        assert command not in release_runs

    interop_step = next(
        step
        for step in jobs["interop"]["steps"]
        if step.get("name") == "Run Python and Rust persisted-state interoperability tests"
    )
    assert interop_step["env"]["CCCC_TEST_PYTHON"] == "python"


def test_windows_rust_binaries_use_the_static_crt() -> None:
    cargo_config = (ROOT / ".cargo/config.toml").read_text(encoding="utf-8")

    assert "[target.x86_64-pc-windows-msvc]" in cargo_config
    assert 'target-feature=+crt-static' in cargo_config


def test_one_tag_publishes_pypi_and_matching_standalone_rust_assets() -> None:
    release = _release_workflow()
    rust_candidate = _rust_release_workflow()

    assert release["on"]["push"]["tags"] == ["v*"]
    assert release["jobs"]["publish"]["if"] == "github.event_name == 'push'"
    assert set(release["jobs"]["collect"]["needs"]) == {
        "verify-python",
        "interop",
        "native-linux-x64",
        "native-desktop",
    }
    release_runs = "\n".join(_runs(job) for job in release["jobs"].values())
    assert "manylinux_2_28_x86_64" in release_runs
    assert "delocate==0.13.0" in release_runs
    assert "delvewheel==1.13.0" in release_runs
    assert "scripts/publish_rust_crates.sh --publish" not in release_runs
    assert "python -m twine upload" in _runs(release["jobs"]["publish"])

    assert rust_candidate["on"]["push"]["tags"] == ["v*"]
    assert "workflow_dispatch" in rust_candidate["on"]
    assert rust_candidate["jobs"]["publish"]["if"] == "github.event_name == 'push'"
    assert "verify" not in rust_candidate["jobs"]
    assert rust_candidate["jobs"]["publish"]["needs"] == "prepare"
    publish_runs = _runs(rust_candidate["jobs"]["publish"])
    assert "scripts/check_release_versions.py --tag" in publish_runs
    assert "gh release create" in publish_runs
    assert "gh release upload" in publish_runs
    assert "--prerelease" in publish_runs


def test_docs_publish_stable_installers_from_the_canonical_scripts() -> None:
    docs_workflow = yaml.load(
        (ROOT / ".github/workflows/docs.yml").read_text(encoding="utf-8"),
        Loader=yaml.BaseLoader,
    )
    paths = set(docs_workflow["on"]["push"]["paths"])
    package = json.loads((ROOT / "docs/package.json").read_text(encoding="utf-8"))

    assert {
        "scripts/install.sh",
        "scripts/install.ps1",
        "scripts/prepare_docs_installers.mjs",
    } <= paths
    assert package["scripts"]["prebuild"] == "npm run prepare:installers"
    assert package["scripts"]["prepare:installers"] == "node ../scripts/prepare_docs_installers.mjs"

    subprocess.run(["node", "scripts/prepare_docs_installers.mjs"], cwd=ROOT, check=True)
    with (ROOT / "Cargo.toml").open("rb") as handle:
        version = tomllib.load(handle)["workspace"]["package"]["version"]
    shell_installer = (ROOT / "docs/public/install.sh").read_text(encoding="utf-8")
    powershell_installer = (ROOT / "docs/public/install.ps1").read_text(encoding="utf-8")
    assert f'DEFAULT_VERSION="{version}"' in shell_installer
    assert f'$defaultVersion = "{version}"' in powershell_installer
    assert "@CCCC_" not in shell_installer
    assert "@CCCC_" not in powershell_installer


def test_rust_workspace_cannot_create_a_second_registry_distribution() -> None:
    manifests = sorted((ROOT / "crates").glob("cccc-*/Cargo.toml"))

    assert manifests
    for manifest in manifests:
        with manifest.open("rb") as handle:
            package = tomllib.load(handle)["package"]
        assert package.get("publish") is False, manifest

    assert not (ROOT / "scripts/publish_rust_crates.sh").exists()
    rust_update = (ROOT / "crates/cccc-cli/src/commands/update.rs").read_text(encoding="utf-8")
    assert "https://chesterra.github.io/cccc/install.sh" in rust_update
    assert ".cccc-standalone" in rust_update
    assert "managed by another installation" in rust_update
