from __future__ import annotations

import json

from cccc.kernel.actors import add_actor, update_actor
from cccc.kernel.runtime import (
    DEEPSEEK_ACP_VERSION,
    DEEPSEEK_MCP_CLIENT_VERSION,
    KNOWN_RUNTIMES,
    PRIMARY_RUNTIMES,
    deepseek_preflight_error,
    get_runtime_command_with_flags,
)
from cccc.contracts.v1.deepseek import (
    DEEPSEEK_ACP_SDK_VERSION,
    DEEPSEEK_LLM_ADAPTER_PACKAGE,
    DEEPSEEK_NODE_RANGE,
    DEEPSEEK_PACKAGE_VERSIONS,
    DEEPSEEK_PROTOCOL_VERSION,
    DEEPSEEK_RELEASE_VERSION,
)
from cccc.kernel.deepseek_acp import (
    ACPProtocolError,
    NDJSONSession,
    initialize_request,
    permission_outcome,
    session_new_request,
    terminal_stop_reason,
    validate_initialize_result,
    validate_session_new_result,
    validate_session_update,
)
from cccc.kernel.deepseek_runtime import canonical_deepseek_runtime_manifest


def _canonical_config(command) -> str:
    return (
        "- id: llm-deepseek\n  name: '@deepseek-ai/dsh-llm-deepseek'\n"
        "- id: acp-demo\n  name: '@deepseek-ai/dsh-acp-demo'\n"
        "  config:\n    provider: deepseek-official\n    model: deepseek-v4-flash\n"
        "    workspaceContext: false\n"
        "    persistenceRoot: !!js process.env.CCCC_DEEPSEEK_SESSION_ROOT\n- id: cccc-mcp\n"
        "  name: '@deepseek-ai/dsh-mcp-client'\n  config:\n"
        "    transport: stdio\n    serverName: cccc\n"
        f"    command: '{command}'\n    args: [mcp]\n    env:\n"
        "      CCCC_HOME: !!js process.env.CCCC_HOME\n"
        "      CCCC_GROUP_ID: !!js process.env.CCCC_GROUP_ID\n"
        "      CCCC_ACTOR_ID: !!js process.env.CCCC_ACTOR_ID\n"
        "    failOnStartupError: true\n"
    )


def _canonical_manifest(*, adapter_version: str = "0.1.0-rc.6") -> str:
    return json.dumps(
        {
            "name": "dsh-profile-cccc-acp",
            "private": True,
            "ccccManaged": True,
            "dependencies": {
                "@deepseek-ai/dsh-acp": "0.1.0-rc.6",
                "@deepseek-ai/dsh-mcp-client": "0.1.0-rc.6",
                "@deepseek-ai/dsh-acp-demo": "0.1.0-rc.6",
                DEEPSEEK_LLM_ADAPTER_PACKAGE: adapter_version,
            },
        }
    ) + "\n"


def _managed_home(tmp_path):
    return tmp_path / "cccc-home" / "runtimes" / "deepseek" / DEEPSEEK_RELEASE_VERSION


def _write_packages(home) -> None:
    lock_packages = {"": {"dependencies": dict(DEEPSEEK_PACKAGE_VERSIONS)}}
    for package, version in DEEPSEEK_PACKAGE_VERSIONS:
        package_dir = home / "node_modules" / package
        package_dir.mkdir(parents=True, exist_ok=True)
        (package_dir / "package.json").write_text(
            json.dumps({"version": version}) + "\n", encoding="utf-8"
        )
        lock_packages[f"node_modules/{package}"] = {"version": version}
    (home / "package-lock.json").write_text(
        json.dumps({"lockfileVersion": 3, "packages": lock_packages}), encoding="utf-8"
    )
    (home / "package.json").write_text(
        json.dumps(canonical_deepseek_runtime_manifest()), encoding="utf-8"
    )


def test_deepseek_preflight_is_fail_closed_without_acp(tmp_path, monkeypatch) -> None:
    dsh = tmp_path / "dsh"
    dsh.write_text("#!/bin/sh\nexit 0\n", encoding="utf-8")
    dsh.chmod(0o755)
    node = tmp_path / "node"
    node.write_text("#!/bin/sh\nprintf 'v24.0.0\\n'\n", encoding="utf-8")
    node.chmod(0o755)
    monkeypatch.setenv("PATH", str(tmp_path))
    monkeypatch.setenv("CCCC_HOME", str(tmp_path / "cccc-home"))

    error = deepseek_preflight_error([str(dsh), "--profile", "cccc-acp"], runner="headless")

    assert error.startswith("setup_required:")
    assert DEEPSEEK_ACP_VERSION in error


def test_deepseek_preflight_rejects_non_executable_dsh_and_missing_config(tmp_path, monkeypatch) -> None:
    dsh = tmp_path / "dsh"
    dsh.write_text("#!/bin/sh\nexit 0\n", encoding="utf-8")
    dsh.chmod(0o644)
    node = tmp_path / "node"
    node.write_text("#!/bin/sh\nprintf 'v24.0.0\\n'\n", encoding="utf-8")
    node.chmod(0o755)
    home = _managed_home(tmp_path)
    _write_packages(home)
    profile = home / "profiles" / "cccc-acp"
    profile.mkdir(parents=True)
    (profile / "package.json").write_text(_canonical_manifest(), encoding="utf-8")
    monkeypatch.setenv("PATH", str(tmp_path))
    monkeypatch.setenv("CCCC_HOME", str(tmp_path / "cccc-home"))
    assert "executable not found" in deepseek_preflight_error(["dsh"], runner="headless")
    dsh.chmod(0o755)
    assert "config" in deepseek_preflight_error([str(dsh)], runner="headless")


def test_deepseek_preflight_accepts_exact_fake_bundle(tmp_path, monkeypatch) -> None:
    dsh = tmp_path / "dsh"
    dsh.write_text("#!/bin/sh\nexit 0\n", encoding="utf-8")
    dsh.chmod(0o755)
    node = tmp_path / "node"
    node.write_text("#!/bin/sh\nprintf 'v24.0.0\\n'\n", encoding="utf-8")
    node.chmod(0o755)
    home = _managed_home(tmp_path)
    _write_packages(home)
    profile = home / "profiles" / "cccc-acp"
    profile.mkdir(parents=True)
    (profile / "package.json").write_text(_canonical_manifest(), encoding="utf-8")
    (profile / "cordis.yml").write_text(_canonical_config(dsh), encoding="utf-8")
    monkeypatch.setenv("PATH", str(tmp_path))
    monkeypatch.setenv("CCCC_HOME", str(tmp_path / "cccc-home"))

    assert deepseek_preflight_error([str(dsh), "--profile", "cccc-acp"], runner="headless") == ""
    lock_path = home / "package-lock.json"
    lock = json.loads(lock_path.read_text(encoding="utf-8"))
    lock["packages"]["node_modules/@deepseek-ai/dsh-transitive"] = {
        "version": "0.1.0-rc.7"
    }
    lock_path.write_text(json.dumps(lock), encoding="utf-8")
    assert "dependency graph" in deepseek_preflight_error([str(dsh)], runner="headless")
    _write_packages(home)
    adapter_manifest = home / "node_modules" / DEEPSEEK_LLM_ADAPTER_PACKAGE / "package.json"
    adapter_manifest.write_text('{"version":"0.1.0-rc.7"}\n', encoding="utf-8")
    mismatch = deepseek_preflight_error([str(dsh), "--profile", "cccc-acp"], runner="headless")
    assert mismatch.startswith("setup_required:")
    assert f"{DEEPSEEK_LLM_ADAPTER_PACKAGE}@0.1.0-rc.6" in mismatch
    adapter_manifest.unlink()
    missing = deepseek_preflight_error([str(dsh), "--profile", "cccc-acp"], runner="headless")
    assert missing.startswith("setup_required:")
    assert DEEPSEEK_LLM_ADAPTER_PACKAGE in missing


def test_deepseek_preflight_isolated_from_user_dsh_home_patch(tmp_path, monkeypatch) -> None:
    dsh = tmp_path / "dsh"
    dsh.write_text("#!/bin/sh\nexit 0\n", encoding="utf-8")
    dsh.chmod(0o755)
    node = tmp_path / "node"
    node.write_text("#!/bin/sh\nprintf 'v24.0.0\\n'\n", encoding="utf-8")
    node.chmod(0o755)
    home = _managed_home(tmp_path)
    _write_packages(home)
    profile = home / "profiles" / "cccc-acp"
    profile.mkdir(parents=True)
    (profile / "package.json").write_text(_canonical_manifest(), encoding="utf-8")
    (profile / "cordis.yml").write_text(_canonical_config(dsh), encoding="utf-8")
    user_dsh = tmp_path / "user-dsh"
    user_dsh.mkdir()
    (user_dsh / "cordis.patch.yml").write_text("disable: dsh-acp\n", encoding="utf-8")
    monkeypatch.setenv("PATH", str(tmp_path))
    monkeypatch.setenv("CCCC_HOME", str(tmp_path / "cccc-home"))
    monkeypatch.setenv("DSH_HOME", str(user_dsh))
    assert deepseek_preflight_error([str(dsh)], runner="headless") == ""


def test_deepseek_preflight_ignores_actor_version_overrides(tmp_path, monkeypatch) -> None:
    dsh = tmp_path / "dsh"
    dsh.write_text("#!/bin/sh\nexit 0\n", encoding="utf-8")
    dsh.chmod(0o755)
    node = tmp_path / "node"
    node.write_text("#!/bin/sh\nprintf 'v24.0.0\\n'\n", encoding="utf-8")
    node.chmod(0o755)
    monkeypatch.chdir(tmp_path)
    monkeypatch.setenv("PATH", str(tmp_path))
    monkeypatch.setenv("CCCC_HOME", str(tmp_path / "cccc-home"))
    monkeypatch.setenv("CCCC_NODE_VERSION", "24.0.0")
    monkeypatch.setenv("CCCC_DEEPSEEK_ACP_VERSION", DEEPSEEK_ACP_VERSION)
    monkeypatch.setenv("CCCC_DEEPSEEK_MCP_CLIENT_VERSION", DEEPSEEK_ACP_VERSION)

    error = deepseek_preflight_error([str(dsh), "--profile", "cccc-acp"], runner="headless")

    assert error.startswith("setup_required:")
    assert "node_modules" in error or "required" in error
