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


def test_ci_has_read_only_permissions_bounded_jobs_and_cancels_stale_runs() -> None:
    workflow = _workflow()
    jobs = workflow["jobs"]

    assert workflow["permissions"] == {"contents": "read"}
    assert workflow["concurrency"] == {
        "group": "ci-${{ github.workflow }}-${{ github.ref }}",
        "cancel-in-progress": "${{ github.event_name != 'schedule' }}",
    }
    assert {name: job.get("timeout-minutes") for name, job in jobs.items()} == {
        "quality": "15",
        "web": "15",
        "python-tests": "25",
        "python-compat": "15",
        "package": "25",
        "windows-smoke": "40",
        "rust": "45",
        "interop": "30",
        "nightly-serial": "45",
    }
    rust_toolchain = next(
        step["uses"]
        for step in jobs["rust"]["steps"]
        if step.get("uses", "").startswith("dtolnay/rust-toolchain")
    )
    assert rust_toolchain == "dtolnay/rust-toolchain@1.88.0"


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

    assert windows["needs"] == "web"
    assert "tests/test_socket_special_ops.py" in runs
    assert "tests/test_windows_pty_backend.py" in runs
    assert "tests/test_installation_diagnostics.py" in runs
    assert "tests/test_system_cmds_doctor.py" in runs
    assert "cargo build --release --locked -p cccc --bin cccc" in runs
    assert "scripts/tests/install_windows.ps1" in runs
    assert any(item.startswith("actions/download-artifact") for item in uses)
    assert any(item.startswith("dtolnay/rust-toolchain") for item in uses)
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


def test_rust_job_and_manual_verifiers_cover_replacement_smoke() -> None:
    runs = _runs(_workflow()["jobs"]["rust"])
    unix_verifier = (ROOT / "scripts/tests/verify_release_unix.sh").read_text(encoding="utf-8")
    windows_verifier = (ROOT / "scripts/tests/verify_release_windows.ps1").read_text(encoding="utf-8")

    assert "scripts/tests/smoke_rust_replacement.sh target/release/cccc" in runs
    assert "smoke_rust_replacement.sh" in unix_verifier
    assert '"method":"initialize"' in windows_verifier
    assert "Daemon:      stopped" in windows_verifier


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


def test_python_release_builds_one_atomic_dual_implementation_set() -> None:
    workflow = _release_workflow()
    jobs = workflow["jobs"]

    build_setup = next(
        step for step in jobs["build"]["steps"] if step.get("uses", "").startswith("actions/setup-python")
    )
    publish_setup = next(
        step for step in jobs["publish"]["steps"] if step.get("uses", "").startswith("actions/setup-python")
    )
    assert set(jobs) == {"build", "native-linux-x64", "native-desktop", "collect", "publish"}
    assert workflow["concurrency"] == {
        "group": "release-${{ github.ref }}",
        "cancel-in-progress": "false",
    }
    assert jobs["publish"]["timeout-minutes"] == "10"
    assert build_setup["with"]["python-version"] == "3.14"
    assert publish_setup["with"]["python-version"] == "3.14"
    assert jobs["native-linux-x64"]["needs"] == "build"
    assert jobs["native-desktop"]["needs"] == "build"
    assert set(jobs["collect"]["needs"]) == {"build", "native-linux-x64", "native-desktop"}
    assert jobs["publish"]["needs"] == "collect"
    assert any(
        step.get("uses", "").startswith("actions/checkout") for step in jobs["collect"]["steps"]
    )
    desktop_matrix = jobs["native-desktop"]["strategy"]["matrix"]["include"]
    assert {item["target"] for item in desktop_matrix} == {
        "aarch64-apple-darwin",
        "x86_64-apple-darwin",
        "x86_64-pc-windows-msvc",
    }
    assert {item["platform_tag"] for item in desktop_matrix} == {
        "macosx_11_0_arm64",
        "macosx_11_0_x86_64",
        "win_amd64",
    }

    build_runs = _runs(jobs["build"])
    release_runs = "\n".join(_runs(job) for job in jobs.values()).lower()
    collect_runs = _runs(jobs["collect"])
    assert "python -m build" in build_runs
    assert "python -m twine check" in build_runs
    assert "python -m venv .release-smoke" in build_runs
    assert "--no-deps --force-reinstall dist/*.whl" in build_runs
    assert ".release-smoke/bin/cccc version" in build_runs
    assert "cargo build --release --locked" in release_runs
    assert "scripts/check_release_versions.py --rust-binary" in release_runs
    assert "scripts/verify_native_wheel.py" in release_runs
    assert "auditwheel==6.7.0" in release_runs
    assert "delocate==0.13.0" in release_runs
    assert "delvewheel==1.13.0" in release_runs
    assert "scripts/verify_python_release_set.py dist" in collect_runs
    assert "python -m twine upload" in _runs(jobs["publish"])
    assert "cccc rust" not in release_runs
    for source_test in ("cargo test", "pytest", "context_python_interop", "python_storage_interop"):
        assert source_test not in release_runs


def test_windows_rust_binaries_use_the_static_crt() -> None:
    cargo_config = (ROOT / ".cargo/config.toml").read_text(encoding="utf-8")

    assert "[target.x86_64-pc-windows-msvc]" in cargo_config
    assert 'target-feature=+crt-static' in cargo_config


def test_product_tag_publishes_pypi_while_standalone_preview_is_manual() -> None:
    release = _release_workflow()
    rust_candidate = _rust_release_workflow()

    assert release["on"]["push"]["tags"] == ["v*"]
    assert release["jobs"]["publish"]["if"] == "github.event_name == 'push'"
    assert release["jobs"]["publish"]["needs"] == "collect"
    release_runs = "\n".join(_runs(job) for job in release["jobs"].values())
    assert "manylinux_2_28_x86_64" in release_runs
    assert "delocate==0.13.0" in release_runs
    assert "delvewheel==1.13.0" in release_runs
    assert "scripts/publish_rust_crates.sh --publish" not in release_runs
    assert "python -m twine upload" in _runs(release["jobs"]["publish"])

    assert set(rust_candidate["on"]) == {"workflow_dispatch"}
    assert rust_candidate["concurrency"] == {
        "group": "rust-preview-${{ github.ref }}",
        "cancel-in-progress": "false",
    }
    assert rust_candidate["jobs"]["publish"]["if"] == "startsWith(github.ref, 'refs/tags/v')"
    assert set(rust_candidate["jobs"]) == {"web", "build", "prepare", "verify", "publish"}
    assert {
        name: job.get("timeout-minutes") for name, job in rust_candidate["jobs"].items()
    } == {
        "web": "15",
        "build": "45",
        "prepare": "10",
        "verify": "5",
        "publish": "10",
    }
    assert rust_candidate["jobs"]["build"]["needs"] == "web"
    assert {item["target"] for item in rust_candidate["jobs"]["build"]["strategy"]["matrix"]["include"]} == {
        "aarch64-apple-darwin",
        "x86_64-apple-darwin",
        "x86_64-unknown-linux-gnu",
        "x86_64-pc-windows-msvc",
    }
    web_runs = _runs(rust_candidate["jobs"]["web"])
    assert "prepare_rust_web_assets.mjs --install-deps" in web_runs
    build_uses = {
        step.get("uses", "") for step in rust_candidate["jobs"]["build"]["steps"]
    }
    assert not any(item.startswith("actions/setup-python") for item in build_uses)
    assert not any(item.startswith("actions/setup-node") for item in build_uses)
    rust_release_runs = "\n".join(_runs(job) for job in rust_candidate["jobs"].values())
    assert "Smoke native Rust binary" in {
        step.get("name", "")
        for job in rust_candidate["jobs"].values()
        for step in job.get("steps", [])
    }
    verify = rust_candidate["jobs"]["verify"]
    assert verify["needs"] == "prepare"
    assert verify["timeout-minutes"] == "5"
    assert {item["target"] for item in verify["strategy"]["matrix"]["include"]} == {
        "x86_64-unknown-linux-gnu",
        "x86_64-pc-windows-msvc",
    }
    assert "scripts/tests/verify_release_unix.sh" in rust_release_runs
    assert "scripts/tests/verify_release_windows.ps1" in rust_release_runs
    for source_test in ("cargo test", "pytest", "context_python_interop", "python_storage_interop"):
        assert source_test not in rust_release_runs
    assert rust_candidate["jobs"]["publish"]["needs"] == "verify"
    publish_runs = _runs(rust_candidate["jobs"]["publish"])
    assert "scripts/check_release_versions.py --tag" in publish_runs
    assert "gh release create" in publish_runs
    assert "gh release upload" in publish_runs
    assert "--prerelease" in publish_runs
    assert "experimental standalone Rust preview" in publish_runs
    assert "recommended stable distribution remains cccc-pair from PyPI" in publish_runs


def test_python_release_keeps_registry_tokens_out_of_step_outputs() -> None:
    publish = _release_workflow()["jobs"]["publish"]
    classify = next(step for step in publish["steps"] if step.get("id") == "channel")
    uploads = [step for step in publish["steps"] if "twine upload" in step.get("run", "")]

    assert "secrets." not in classify["run"]
    assert "token=" not in classify["run"]
    assert {step["if"] for step in uploads} == {
        "steps.channel.outputs.prerelease == 'true'",
        "steps.channel.outputs.prerelease == 'false'",
    }
    assert {step["env"]["TWINE_PASSWORD"] for step in uploads} == {
        "${{ secrets.TEST_PYPI_API_TOKEN }}",
        "${{ secrets.PYPI_API_TOKEN }}",
    }
    assert all("steps.channel.outputs.token" not in str(step) for step in publish["steps"])


def test_docs_publish_stable_installers_from_the_canonical_scripts() -> None:
    docs_workflow = yaml.load(
        (ROOT / ".github/workflows/docs.yml").read_text(encoding="utf-8"),
        Loader=yaml.BaseLoader,
    )
    paths = set(docs_workflow["on"]["push"]["paths"])
    package = json.loads((ROOT / "docs/package.json").read_text(encoding="utf-8"))

    assert {name: job.get("timeout-minutes") for name, job in docs_workflow["jobs"].items()} == {
        "build": "15",
        "deploy": "10",
    }
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
    tls_bootstrap = "[Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12"
    assert tls_bootstrap in powershell_installer
    assert powershell_installer.index(tls_bootstrap) < powershell_installer.index("Invoke-WebRequest")
    assert "@CCCC_" not in shell_installer
    assert "@CCCC_" not in powershell_installer


def test_windows_install_command_supports_cmd_and_legacy_powershell_tls() -> None:
    command = (
        "powershell.exe -NoProfile -ExecutionPolicy Bypass -Command \""
        "[Net.ServicePointManager]::SecurityProtocol = "
        "[Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12; "
        "Invoke-RestMethod 'https://chesterra.github.io/cccc/install.ps1' | Invoke-Expression\""
    )
    documentation = [
        "README.md",
        "README.zh-CN.md",
        "README.ja.md",
        "docs/rust-migration.md",
        "docs/guide/getting-started/index.md",
        "docs/guide/faq.md",
        "crates/cccc-cli/README.md",
    ]

    for relative_path in documentation:
        contents = (ROOT / relative_path).read_text(encoding="utf-8")
        assert command in contents, relative_path
        assert "irm https://chesterra.github.io/cccc/install.ps1 | iex" not in contents, relative_path


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
    tls_bootstrap = "[Net.SecurityProtocolType]::Tls12"
    assert tls_bootstrap in rust_update
    assert rust_update.index(tls_bootstrap) < rust_update.index("Invoke-RestMethod")
