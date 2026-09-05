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


def _nightly_workflow() -> dict:
    return yaml.load(
        (ROOT / ".github/workflows/nightly.yml").read_text(encoding="utf-8"),
        Loader=yaml.BaseLoader,
    )


def _runs(job: dict) -> str:
    return "\n".join(step.get("run", "") for step in job.get("steps", []))


def test_ci_has_read_only_permissions_bounded_jobs_and_cancels_stale_runs() -> None:
    workflow = _workflow()
    jobs = workflow["jobs"]

    assert workflow["permissions"] == {"contents": "read"}
    assert workflow["concurrency"] == {
        "group": "ci-${{ github.event.pull_request.number || github.ref }}",
        "cancel-in-progress": "true",
    }
    assert all(job.get("timeout-minutes") for job in jobs.values())
    assert max(int(job["timeout-minutes"]) for job in jobs.values()) <= 60
    rust_toolchain = next(
        step["uses"]
        for step in jobs["rust-linux"]["steps"]
        if step.get("uses", "").startswith("dtolnay/rust-toolchain")
    )
    assert rust_toolchain == "dtolnay/rust-toolchain@1.88.0"


def test_pr_jobs_keep_quality_web_package_and_native_boundaries() -> None:
    jobs = _workflow()["jobs"]

    assert set(jobs) == {
        "quality",
        "web",
        "package",
        "windows-smoke",
        "rust-linux",
        "ci-required",
    }
    assert set(jobs["ci-required"]["needs"]) == set(jobs) - {"ci-required"}
    assert jobs["ci-required"]["if"] == "always()"
    assert jobs["package"]["needs"] == "quality"
    assert "ruff check" in _runs(jobs["quality"])
    assert "python -m pytest -q" in _runs(jobs["quality"])
    assert "npm -C web test" in _runs(jobs["web"])
    assert "npm -C web run build" in _runs(jobs["web"])
    assert any(step.get("uses", "").startswith("actions/upload-artifact") for step in jobs["web"]["steps"])
    assert not any(step.get("uses", "").startswith("actions/download-artifact") for step in jobs["package"]["steps"])

    for name in ("quality", "package"):
        setup = next(
            step
            for step in jobs[name]["steps"]
            if step.get("uses", "").startswith("actions/setup-python")
        )
        assert setup["with"]["cache"] == "pip", name
        assert setup["with"]["cache-dependency-path"] == "pyproject.toml", name


def test_web_ci_uses_managed_node_and_composite_vite_plus_check() -> None:
    web = _workflow()["jobs"]["web"]
    runs = _runs(web)
    node_setup = next(step for step in web["steps"] if step.get("uses", "").startswith("actions/setup-node"))

    assert node_setup["with"]["node-version"] == "24.19.0"
    assert "npm -C web run check" in runs
    assert "npm -C web run typecheck" not in runs
    assert "npm -C web run lint" not in runs


def test_native_empty_session_smoke_is_enabled_without_provider_secrets() -> None:
    steps = _workflow()["jobs"]["rust-linux"]["steps"]
    install = next(step for step in steps if step.get("name") == "Install verified native CLI versions")
    assert "@openai/codex@0.153.2" in install["run"]
    assert "@anthropic-ai/claude-code@2.1.261" in install["run"]
    assert "@kilocode/cli@7.5.14" in install["run"]
    smoke = next(step for step in steps if step.get("name") == "Verify native sessions without external model access")
    assert smoke["timeout-minutes"] == "5"
    assert smoke["env"]["CCCC_CODEX_EMPTY_LIVE"] == "1"
    assert smoke["env"]["CCCC_CLAUDE_EMPTY_LIVE"] == "1"
    assert smoke["env"]["CCCC_KILO_MANAGED_LIVE"] == "1"
    assert smoke["env"]["CCCC_KILO_MODEL_SYNC_LIVE"] == "1"
    assert smoke["env"]["CCCC_LAUNCHER_PATH"].endswith("/target/debug/cccc")
    assert "cargo build --package cccc --bin cccc --locked" in smoke["run"]
    assert "live_codex_empty_actor_and_analyst_resume_with_native_terminal" in smoke["run"]
    assert "live_claude_empty_session_resumes_without_a_prompt" in smoke["run"]
    assert "live_kilo -- --test-threads=1" in smoke["run"]
    assert "secrets." not in json.dumps(smoke)


def test_windows_installer_job_is_a_nightly_native_fixture() -> None:
    installer = _nightly_workflow()["jobs"]["windows-installer"]
    runs = _runs(installer)
    uses = {step.get("uses", "") for step in installer["steps"]}

    assert installer["needs"] == "web-bundle"
    assert "cargo build --release --locked -p cccc --bin cccc" in runs
    assert "scripts/tests/install_windows.ps1" in runs
    assert any(item.startswith("actions/download-artifact") for item in uses)
    assert any(item.startswith("dtolnay/rust-toolchain") for item in uses)


def test_rust_linux_job_is_python_free_and_reuses_one_workspace() -> None:
    jobs = _workflow()["jobs"]
    job = jobs["rust-linux"]
    test_runs = _runs(job)
    uses = {step.get("uses", "") for step in job["steps"]}

    assert "env" not in job
    assert not any(item.startswith("actions/setup-python") for item in uses)
    assert "python -m" not in test_runs.lower()
    assert "pip install" not in test_runs.lower()
    assert "scripts/check_version_parity.sh" not in test_runs
    assert "cargo test --workspace --exclude cccc-pair-daemon --locked" in test_runs
    assert (
        "cargo test --package cccc-pair-daemon --locked"
        in test_runs
    )
    assert "python_interop_" not in test_runs
    assert "--skip daemon_self_launch::" in test_runs
    assert "--test integration" in test_runs
    assert "daemon_self_launch::" in test_runs
    assert "--test-threads=1" in test_runs
    assert "cargo fmt --all --check" in test_runs
    assert "cargo clippy --workspace --all-targets --locked -- -D warnings" in test_runs
    windows_runs = _runs(jobs["windows-smoke"])
    assert "cargo test --package cccc --test integration --locked" in windows_runs
    assert (
        "daemon_self_launch::combined_web_bind_failure_stops_its_owned_daemon"
        in windows_runs
    )


def test_retired_python_product_and_cross_language_suites_are_absent() -> None:
    retired_sources = {
        "crates/cccc-core/tests/context_python_interop.rs",
        "crates/cccc-core/tests/group_bridge_identity_interop.rs",
        "crates/cccc-core/tests/ledger_python_interop.rs",
        "crates/cccc-core/tests/membership_interop.rs",
        "crates/cccc-core/tests/runtime_hook_identity_interop.rs",
        "crates/cccc-core/tests/runtime_hook_interop.rs",
        "crates/cccc-core/tests/shared_integration_state_interop.rs",
        "crates/cccc-daemon/tests/python_storage_interop.rs",
    }
    actual_sources = {
        path.relative_to(ROOT).as_posix()
        for path in ROOT.glob("crates/*/tests/*.rs")
        if "CCCC_TEST_PYTHON" in path.read_text(encoding="utf-8")
    }
    assert actual_sources == set()
    assert not (ROOT / "src" / "cccc").exists()
    assert all(not (ROOT / relative_path).exists() for relative_path in retired_sources)


def test_impacted_gate_routes_distribution_and_root_docs_to_tooling_checks() -> None:
    script = (ROOT / "scripts" / "pre_commit_checks.sh").read_text(encoding="utf-8")

    assert "docker/*" in script
    assert "README*.md" in script
    assert "SUPPORT.md" in script


def test_nightly_rust_dist_and_manual_verifiers_cover_replacement_smoke() -> None:
    dist = _nightly_workflow()["jobs"]["rust-dist"]
    runs = _runs(dist)
    unix_verifier = (ROOT / "scripts/tests/verify_release_unix.sh").read_text(encoding="utf-8")
    windows_verifier = (ROOT / "scripts/tests/verify_release_windows.ps1").read_text(encoding="utf-8")
    windows_installer = (ROOT / "scripts/install.ps1").read_text(encoding="utf-8")

    assert dist["needs"] == "web-bundle"
    assert (
        "cargo build --workspace --release --locked --features cccc/standalone"
        in runs
    )
    assert "scripts/tests/smoke_rust_replacement.sh target/release/cccc" in runs
    assert "smoke_rust_replacement.sh" in unix_verifier
    assert '"method":"initialize"' in windows_verifier
    assert "Daemon:      stopped" in windows_verifier
    assert "New-Object System.Diagnostics.ProcessStartInfo" in windows_installer
    assert '$startInfo.Arguments = $Arguments -join " "' in windows_installer
    assert "$startInfo.UseShellExecute = $false" in windows_installer
    assert "[int]$TimeoutMilliseconds = 35000" in windows_installer
    assert "$process.WaitForExit($TimeoutMilliseconds)" in windows_installer
    assert "$stdoutTask.Wait(1000)" in windows_installer
    assert "$stderrTask.Wait(1000)" in windows_installer
    assert "Stdout = $stdout" in windows_installer
    assert "Stderr = $stderr" in windows_installer


def test_ci_does_not_carry_retired_source_size_or_one_time_migration_governance() -> None:
    runs = "\n".join(_runs(job) for job in _workflow()["jobs"].values())

    assert "source_size.py" not in runs
    assert "verify_oxfmt_migration" not in runs
    assert "test:quality" not in runs


def test_ci_keeps_python_only_for_release_and_repository_tooling() -> None:
    jobs = _workflow()["jobs"]

    for name in ("quality", "package"):
        setup = next(step for step in jobs[name]["steps"] if step.get("uses", "").startswith("actions/setup-python"))
        assert setup["with"]["python-version"] == "3.14"

    assert "python-tests" not in jobs
    assert "interop" not in jobs
    assert "python-compat" not in jobs
    assert not any(
        step.get("uses", "").startswith("actions/setup-python")
        for name in ("web", "windows-smoke", "rust-linux")
        for step in jobs[name]["steps"]
    )


def test_package_job_exercises_only_the_rust_only_release_shape() -> None:
    package = _workflow()["jobs"]["package"]
    runs = _runs(package)

    assert not any(step.get("uses", "").startswith("actions/download-artifact") for step in package["steps"])
    assert "python -m build" not in runs
    assert "packaged_web_dist" not in runs
    assert "scripts/build_native_wheel.py" in runs
    assert "scripts/verify_native_wheel.py" in runs
    assert "tests/test_build_native_wheel.py" in runs
    assert ".data/scripts/cccc" in runs


def test_nightly_workflow_owns_slow_native_verification() -> None:
    workflow = _nightly_workflow()
    jobs = workflow["jobs"]

    assert set(workflow["on"]) == {"schedule", "workflow_dispatch"}
    assert workflow["permissions"] == {"contents": "read"}
    assert workflow["concurrency"] == {
        "group": "nightly-${{ github.ref }}",
        "cancel-in-progress": "false",
    }
    assert set(jobs) == {"web-bundle", "rust-dist", "windows-installer"}
    assert not any(
        step.get("uses", "").startswith("actions/setup-python")
        for job in jobs.values()
        for step in job["steps"]
    )
    assert not (ROOT / ".github/workflows/post-merge.yml").exists()


def test_workflows_use_node24_actions_and_schedule_action_updates() -> None:
    workflow_sources = "\n".join(
        path.read_text(encoding="utf-8")
        for path in sorted((ROOT / ".github/workflows").glob("*.yml"))
    )

    for legacy in (
        "actions/checkout@v4",
        "actions/setup-python@v5",
        "actions/setup-node@v4",
        "actions/upload-artifact@v4",
        "actions/download-artifact@v4",
    ):
        assert legacy not in workflow_sources
    for current in (
        "actions/checkout@v7",
        "actions/setup-python@v7",
        "actions/setup-node@v7",
        "actions/upload-artifact@v7",
        "actions/download-artifact@v8",
    ):
        assert current in workflow_sources

    dependabot = yaml.safe_load(
        (ROOT / ".github/dependabot.yml").read_text(encoding="utf-8")
    )
    action_updates = next(
        update
        for update in dependabot["updates"]
        if update["package-ecosystem"] == "github-actions"
    )
    assert action_updates["directory"] == "/"
    assert action_updates["schedule"]["interval"] == "weekly"
    assert {
        dependency["dependency-name"] for dependency in action_updates["ignore"]
    } == {"dtolnay/rust-toolchain"}


def test_release_builds_one_atomic_rust_only_set() -> None:
    workflow = _release_workflow()
    jobs = workflow["jobs"]

    web_setup = next(
        step for step in jobs["web"]["steps"] if step.get("uses", "").startswith("actions/setup-python")
    )
    publish_setup = next(
        step for step in jobs["publish"]["steps"] if step.get("uses", "").startswith("actions/setup-python")
    )
    assert set(jobs) == {"web", "build-linux", "build-desktop", "prepare", "verify", "publish"}
    assert workflow["concurrency"] == {
        "group": "release-${{ github.ref }}",
        "cancel-in-progress": "false",
    }
    assert jobs["publish"]["timeout-minutes"] == "10"
    assert web_setup["with"]["python-version"] == "3.14"
    assert publish_setup["with"]["python-version"] == "3.14"
    assert jobs["build-linux"]["needs"] == "web"
    assert jobs["build-desktop"]["needs"] == "web"
    assert set(jobs["prepare"]["needs"]) == {"build-linux", "build-desktop"}
    assert jobs["verify"]["needs"] == "prepare"
    assert jobs["publish"]["needs"] == "verify"
    assert any(
        step.get("uses", "").startswith("actions/checkout") for step in jobs["prepare"]["steps"]
    )
    desktop_matrix = jobs["build-desktop"]["strategy"]["matrix"]["include"]
    assert {item["target"] for item in desktop_matrix} == {
        "aarch64-apple-darwin",
        "x86_64-pc-windows-msvc",
    }
    assert {item["platform_tag"] for item in desktop_matrix} == {
        "macosx_11_0_arm64",
        "win_amd64",
    }
    assert next(item for item in desktop_matrix if item["platform_tag"] == "win_amd64")["os"] == (
        "windows-2022"
    )

    release_runs = "\n".join(_runs(job) for job in jobs.values()).lower()
    assert "+            " not in release_runs
    prepare_runs = _runs(jobs["prepare"])
    assert "python -m build" not in release_runs
    assert "cargo build --release --locked --features standalone" in release_runs
    assert "scripts/check_release_versions.py --rust-binary" in release_runs
    assert "scripts/build_standalone_archive.py" in release_runs
    assert "scripts/build_native_wheel.py" in release_runs
    assert "scripts/verify_native_wheel.py" in release_runs
    assert release_runs.count("scripts/tests/smoke_wheel_frontdoor.py") == 2
    assert release_runs.count("--binary") >= 4
    assert "auditwheel==6.7.0" in release_runs
    assert 'platform tag: "manylinux_2_28_x86_64"' in release_runs
    assert "delvewheel==1.13.0" in release_runs
    assert "auditwheel repair" not in release_runs
    assert "delocate-wheel" not in release_runs
    assert "delvewheel repair" not in release_runs
    assert "scripts/package_release_assets.sh" in prepare_runs
    assert "scripts/verify_release_set.py artifacts" in prepare_runs
    assert "scripts/upload_wheel_release.py" in _runs(jobs["publish"])
    assert "cccc rust" not in release_runs
    assert not (ROOT / ".github/workflows/release-rust.yml").exists()
    assert not (ROOT / "setup.py").exists()
    for source_test in ("cargo test", "pytest", "context_python_interop", "python_storage_interop"):
        assert source_test not in release_runs


def test_windows_rust_binaries_use_the_static_crt() -> None:
    cargo_config = (ROOT / ".cargo/config.toml").read_text(encoding="utf-8")

    assert "[target.x86_64-pc-windows-msvc]" in cargo_config
    assert 'target-feature=+crt-static' in cargo_config


def test_product_tag_publishes_one_verified_pypi_and_github_release() -> None:
    release = _release_workflow()

    assert release["on"]["push"]["tags"] == ["v*"]
    assert set(release["on"]) == {"push", "workflow_dispatch"}
    assert release["jobs"]["publish"]["if"] == "startsWith(github.ref, 'refs/tags/v')"
    assert release["jobs"]["publish"]["needs"] == "verify"
    release_runs = "\n".join(_runs(job) for job in release["jobs"].values())
    assert "manylinux_2_28_x86_64" in release_runs
    assert "delvewheel==1.13.0" in release_runs
    assert "scripts/publish_rust_crates.sh --publish" not in release_runs
    assert "scripts/upload_wheel_release.py" in _runs(release["jobs"]["publish"])

    assert release["concurrency"] == {
        "group": "release-${{ github.ref }}",
        "cancel-in-progress": "false",
    }
    assert set(release["jobs"]) == {
        "web",
        "build-linux",
        "build-desktop",
        "prepare",
        "verify",
        "publish",
    }
    assert {
        name: job.get("timeout-minutes") for name, job in release["jobs"].items()
    } == {
        "web": "15",
        "build-linux": "45",
        "build-desktop": "45",
        "prepare": "10",
        "verify": "10",
        "publish": "10",
    }
    assert release["jobs"]["build-linux"]["needs"] == "web"
    assert release["jobs"]["build-linux"]["container"] == (
        "quay.io/pypa/manylinux_2_28_x86_64:latest"
    )
    linux_runs = _runs(release["jobs"]["build-linux"])
    assert "cargo build --release --locked --features standalone" in linux_runs
    assert "scripts/verify_standalone_binary.py" in linux_runs

    desktop = release["jobs"]["build-desktop"]
    assert desktop["needs"] == "web"
    assert {item["target"] for item in desktop["strategy"]["matrix"]["include"]} == {
        "aarch64-apple-darwin",
        "x86_64-pc-windows-msvc",
    }
    assert next(item for item in desktop["strategy"]["matrix"]["include"] if "windows" in item["target"])[
        "os"
    ] == "windows-2022"
    desktop_build = next(
        step for step in desktop["steps"] if step.get("name") == "Build the release executable once"
    )
    assert desktop_build["env"]["MACOSX_DEPLOYMENT_TARGET"] == "11.0"
    assert "scripts/verify_standalone_binary.py" in _runs(desktop)
    assert set(release["jobs"]["prepare"]["needs"]) == {"build-linux", "build-desktop"}
    web_runs = _runs(release["jobs"]["web"])
    assert "prepare_rust_web_assets.mjs --install-deps" in web_runs
    build_uses = {
        step.get("uses", "") for step in desktop["steps"]
    }
    assert any(item.startswith("actions/setup-python") for item in build_uses)
    assert not any(item.startswith("actions/setup-node") for item in build_uses)
    release_runs = "\n".join(_runs(job) for job in release["jobs"].values())
    assert "Smoke native executable" in {
        step.get("name", "")
        for job in release["jobs"].values()
        for step in job.get("steps", [])
    }
    verify = release["jobs"]["verify"]
    assert verify["needs"] == "prepare"
    assert verify["timeout-minutes"] == "10"
    assert {item["target"] for item in verify["strategy"]["matrix"]["include"]} == {
        "aarch64-apple-darwin",
        "x86_64-unknown-linux-gnu",
        "x86_64-pc-windows-msvc",
    }
    assert next(item for item in verify["strategy"]["matrix"]["include"] if "windows" in item["target"])[
        "os"
    ] == "windows-2022"
    assert "scripts/tests/verify_release_unix.sh" in release_runs
    assert "scripts/tests/verify_release_windows.ps1" in release_runs
    for source_test in ("cargo test", "pytest", "context_python_interop", "python_storage_interop"):
        assert source_test not in release_runs
    publish_runs = _runs(release["jobs"]["publish"])
    assert "scripts/check_release_versions.py --tag" in publish_runs
    assert "scripts/verify_release_set.py artifacts" in publish_runs
    assert "gh release create" in publish_runs
    assert "gh release upload" in publish_runs
    assert "gh release edit" in publish_runs
    assert "--prerelease" in publish_runs
    assert "experimental standalone Rust preview" not in publish_runs
    assert 'notes_file="docs/release/v${version}_release_notes.md"' in publish_runs
    assert publish_runs.index('test -s "${notes_file}"') < publish_runs.index(
        "scripts/upload_wheel_release.py"
    )
    assert '--notes-file "${notes_file}"' in publish_runs
    assert "--generate-notes" not in publish_runs


def test_wheel_release_keeps_registry_tokens_out_of_step_outputs() -> None:
    publish = _release_workflow()["jobs"]["publish"]
    classify = next(step for step in publish["steps"] if step.get("id") == "channel")
    uploads = [
        step
        for step in publish["steps"]
        if "upload_wheel_release.py" in step.get("run", "")
    ]

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
    assert all("artifacts/*.whl" in step["run"] for step in uploads)
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
        "scripts/resolve_docs_installer_version.mjs",
    } <= paths
    assert docs_workflow["on"]["workflow_run"] == {
        "workflows": ["Release CCCC"],
        "types": ["completed"],
    }
    assert "workflow_run.conclusion == 'success'" in docs_workflow["jobs"]["build"]["if"]
    assert "workflow_run.event" not in docs_workflow["jobs"]["build"]["if"]
    checkout = next(
        step
        for step in docs_workflow["jobs"]["build"]["steps"]
        if step.get("uses", "").startswith("actions/checkout")
    )
    assert checkout["with"]["ref"] == "${{ github.event.repository.default_branch }}"
    docs_runs = _runs(docs_workflow["jobs"]["build"])
    resolver_command = "node scripts/resolve_docs_installer_version.mjs --output docs/public/releases.json"
    assert f'version="$({resolver_command})"' in docs_runs
    assert 'echo "version=$version" >> "$GITHUB_OUTPUT"' in docs_runs
    resolver = next(
        step for step in docs_workflow["jobs"]["build"]["steps"]
        if step.get("id") == "installer-release"
    )
    assert resolver["env"]["GITHUB_TOKEN"] == "${{ github.token }}"
    verification = "cmp docs/public/releases.json docs/.vitepress/dist/releases.json"
    assert docs_runs.index(resolver_command) < docs_runs.index("npm run build") < docs_runs.index(verification)
    steps = docs_workflow["jobs"]["build"]["steps"]
    upload = next(step for step in steps if step.get("uses", "").startswith("actions/upload-pages-artifact"))
    verify = next(step for step in steps if step.get("run") == verification)
    assert steps.index(verify) < steps.index(upload)
    assert upload["with"]["path"] == "docs/.vitepress/dist"
    assert docs_workflow["jobs"]["deploy"]["needs"] == "build"
    build = next(
        step
        for step in docs_workflow["jobs"]["build"]["steps"]
        if step.get("name") == "Build with VitePress"
    )
    assert build["env"]["CCCC_DOCS_INSTALL_VERSION"] == (
        "${{ steps.installer-release.outputs.version }}"
    )
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


def test_source_helpers_use_one_native_product_path() -> None:
    start = (ROOT / "start.ps1").read_text(encoding="utf-8-sig")
    package_unix = (ROOT / "scripts/build_package.sh").read_text(encoding="utf-8")
    package_windows = (ROOT / "scripts/build_package.ps1").read_text(encoding="utf-8-sig")

    assert "cargo build --locked -p cccc --bin cccc" in start
    assert "uv pip" not in start
    assert "cccc.daemon_main" not in start
    for script in (package_unix, package_windows):
        assert "build_standalone_archive.py" in script
        assert "--install-deps" in script
        assert "--features standalone" in script
        assert "--version" in script
        assert "Run:" in script
        assert "python -m build" not in script
        for prerequisite in ("node", "npm", "cargo", "rustc", "python"):
            assert prerequisite in script
    assert not (ROOT / "start_rust.ps1").exists()
    assert not (ROOT / "scripts/build_package_rust.sh").exists()
    assert not (ROOT / "scripts/build_package_rust.ps1").exists()


def test_standalone_installers_do_not_probe_markerless_version_for_ownership() -> None:
    subprocess.run(["node", "scripts/prepare_docs_installers.mjs"], cwd=ROOT, check=True)
    shell_installers = [
        (ROOT / "scripts/install.sh").read_text(encoding="utf-8"),
        (ROOT / "docs/public/install.sh").read_text(encoding="utf-8"),
    ]
    powershell_installers = [
        (ROOT / "scripts/install.ps1").read_text(encoding="utf-8"),
        (ROOT / "docs/public/install.ps1").read_text(encoding="utf-8"),
    ]

    for installer in shell_installers:
        assert "CCCC_TRUSTED_EXISTING_CLI" in installer
        assert "is_existing_cccc_command" not in installer
        assert "is_cccc_version_output" not in installer
    for installer in powershell_installers:
        assert "CCCC_TRUSTED_EXISTING_CLI" in installer
        assert "Test-ExistingCcccCommand" not in installer
        assert "Test-CcccVersionOutput" not in installer


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
