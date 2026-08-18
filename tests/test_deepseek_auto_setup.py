from __future__ import annotations

import json
import os
import threading
from pathlib import Path

from cccc.contracts.v1.deepseek import (
    DEEPSEEK_PACKAGE_VERSIONS,
    is_canonical_profile_manifest,
)
from cccc.daemon.actors.deepseek_setup import (
    _packages_ready,
    _write_profile_files,
    ensure_deepseek_setup,
)
from cccc.kernel.runtime import _is_canonical_deepseek_config, _is_canonical_deepseek_patch
from cccc.kernel.runtime import resolve_deepseek_home, runtime_start_preflight_error

_PACKAGES = DEEPSEEK_PACKAGE_VERSIONS


def _env(tmp_path: Path) -> dict[str, str]:
    executable = tmp_path / ("cccc.exe" if os.name == "nt" else "cccc")
    executable.write_text("cccc", encoding="utf-8")
    if os.name != "nt":
        executable.chmod(0o755)
    return {
        "HOME": str(tmp_path),
        "PATH": str(tmp_path),
        "CCCC_LAUNCHER_PATH": str(executable),
    }


def _install(dsh_home: Path, _env: dict[str, str]) -> None:
    for package, version in _PACKAGES:
        manifest = dsh_home / "node_modules" / package / "package.json"
        manifest.parent.mkdir(parents=True, exist_ok=True)
        manifest.write_text(json.dumps({"version": version}), encoding="utf-8")


def _ready(_command, *, runner="headless", env=None) -> str:
    del runner
    dsh_home = Path((env or {})["DSH_HOME"])
    for package, version in _PACKAGES:
        try:
            found = json.loads(
                (dsh_home / "node_modules" / package / "package.json").read_text(encoding="utf-8")
            )["version"]
        except (OSError, ValueError, KeyError):
            return "packages missing"
        if found != version:
            return "package version mismatch"
    profile = dsh_home / "profiles" / "cccc-acp"
    try:
        manifest = json.loads((profile / "package.json").read_text(encoding="utf-8"))
        patch = (profile / "cordis.patch.yml").read_text(encoding="utf-8")
        config = (profile / "cordis.yml").read_text(encoding="utf-8")
    except (OSError, ValueError):
        return "profile missing"
    if (
        not is_canonical_profile_manifest(manifest)
        or not _is_canonical_deepseek_patch(patch, env=env)
        or not _is_canonical_deepseek_config(config)
    ):
        return "profile invalid"
    return ""


def _external(_command, *, env=None) -> str:
    del env
    return ""


def test_first_use_installs_packages_creates_profile_and_is_idempotent(tmp_path) -> None:
    env = _env(tmp_path)
    first = ensure_deepseek_setup(
        env,
        installer=_install,
        external_preflight=_external,
        ready_preflight=_ready,
    )
    assert first.dsh_home == tmp_path / ".dsh"
    assert first.packages_installed is True
    assert first.profile_created is True
    assert env["DSH_HOME"] == str(first.dsh_home)
    assert env["PATH"].split(os.pathsep)[0] == str(first.dsh_home / "node_modules" / ".bin")
    first_files = {
        name: (first.profile / name).read_bytes()
        for name in ("package.json", "cordis.patch.yml", "cordis.yml")
    }

    second = ensure_deepseek_setup(
        env,
        installer=lambda *_args: (_ for _ in ()).throw(AssertionError("installer reran")),
        external_preflight=_external,
        ready_preflight=_ready,
    )
    assert second.packages_installed is False
    assert second.profile_created is False
    assert {
        name: (second.profile / name).read_bytes() for name in first_files
    } == first_files


def test_profile_paths_escape_yaml_apostrophes(tmp_path) -> None:
    executable = tmp_path / "acme's" / ("cccc.exe" if os.name == "nt" else "cccc")
    executable.parent.mkdir()
    executable.write_text("cccc", encoding="utf-8")
    if os.name != "nt":
        executable.chmod(0o755)
    profile = tmp_path / "profile"

    _write_profile_files(profile, executable)

    escaped_path = str(executable).replace("'", "''")
    patch = (profile / "cordis.patch.yml").read_text(encoding="utf-8")
    config = (profile / "cordis.yml").read_text(encoding="utf-8")
    assert f"command: '{escaped_path}'" in patch
    assert f"command: '{escaped_path}'" in config
    assert _is_canonical_deepseek_patch(patch)
    assert _is_canonical_deepseek_config(config)


def test_start_preflight_allows_first_use_setup_before_managed_profile_exists(monkeypatch) -> None:
    import cccc.kernel.runtime as runtime
    import cccc.kernel.deepseek_runtime as deepseek_runtime

    monkeypatch.setattr(
        deepseek_runtime,
        "_deepseek_executable",
        lambda *_args, **_kwargs: "/bin/dsh-acp-demo",
    )
    monkeypatch.setattr(deepseek_runtime, "_node_version", lambda *_args, **_kwargs: (24, 0, 0))
    assert runtime_start_preflight_error("deepseek", ["dsh-acp-demo"], runner="headless", env={}) == ""
    assert resolve_deepseek_home({"HOME": "/users/test", "DSH_HOME": ""}) == Path("/users/test/.dsh")


def test_runtime_catalog_enables_installed_dsh_before_profile_setup(monkeypatch) -> None:
    import cccc.kernel.runtime as runtime

    monkeypatch.setattr(runtime, "_deepseek_executable", lambda *_args, **_kwargs: "/bin/dsh-acp-demo")
    monkeypatch.setattr(runtime, "deepseek_external_preflight_error", lambda *_args, **_kwargs: "")
    assert runtime.detect_runtime("deepseek").available is True


def test_runtime_catalog_finds_managed_dsh_bin(tmp_path, monkeypatch) -> None:
    import cccc.kernel.runtime as runtime

    managed_bin = tmp_path / ".dsh" / "node_modules" / ".bin"
    managed_bin.mkdir(parents=True)
    (managed_bin / "dsh-acp-demo").write_text("dsh-acp-demo", encoding="utf-8")
    (managed_bin / "dsh-acp-demo").chmod(0o755)
    monkeypatch.setenv("HOME", str(tmp_path))
    monkeypatch.setenv("PATH", "")
    monkeypatch.setattr(runtime, "deepseek_external_preflight_error", lambda command, env=None: "")
    info = runtime.detect_runtime("deepseek")
    assert info.available is True
    assert info.path == str(managed_bin / "dsh-acp-demo")


def test_failed_install_leaves_profile_absent_and_retryable(tmp_path) -> None:
    env = _env(tmp_path)

    def fail_install(_home, _env):
        raise RuntimeError("offline")

    try:
        ensure_deepseek_setup(
            env,
            installer=fail_install,
            external_preflight=_external,
            ready_preflight=_ready,
        )
    except RuntimeError as exc:
        assert "offline" in str(exc)
    else:
        raise AssertionError("installation failure must propagate")
    assert not (tmp_path / ".dsh" / "profiles" / "cccc-acp").exists()
    ensure_deepseek_setup(
        env,
        installer=_install,
        external_preflight=_external,
        ready_preflight=_ready,
    )


def test_three_package_managed_profile_migrates_to_full_acp_composition(tmp_path) -> None:
    env = _env(tmp_path)
    dsh_home = tmp_path / ".dsh"
    for package, version in _PACKAGES[:3]:
        manifest = dsh_home / "node_modules" / package / "package.json"
        manifest.parent.mkdir(parents=True, exist_ok=True)
        manifest.write_text(json.dumps({"version": version}), encoding="utf-8")
    profile = dsh_home / "profiles" / "cccc-acp"
    profile.mkdir(parents=True)
    (profile / "package.json").write_text('{"ccccManaged":true}\n', encoding="utf-8")
    (profile / "cordis.yml").write_text("[]\n", encoding="utf-8")

    outcome = ensure_deepseek_setup(
        env,
        installer=_install,
        external_preflight=lambda *_args, **_kwargs: (
            "setup_required: deepseek executable not found: dsh-acp-demo"
        ),
        ready_preflight=_ready,
    )

    assert outcome.packages_installed is True
    assert outcome.profile_created is False
    assert _packages_ready(dsh_home)
    assert _is_canonical_deepseek_config((profile / "cordis.yml").read_text(encoding="utf-8"))
    first_files = {
        name: (profile / name).read_bytes()
        for name in ("package.json", "cordis.patch.yml", "cordis.yml")
    }
    second = ensure_deepseek_setup(
        env,
        installer=lambda *_args: (_ for _ in ()).throw(AssertionError("installer reran")),
        external_preflight=_external,
        ready_preflight=_ready,
    )
    assert second.packages_installed is False
    assert second.profile_created is False
    assert {name: (profile / name).read_bytes() for name in first_files} == first_files


def test_concurrent_first_use_installs_once(tmp_path) -> None:
    installs = 0
    installs_lock = threading.Lock()
    failures: list[BaseException] = []
    base_env = _env(tmp_path)

    def install(home, env):
        nonlocal installs
        with installs_lock:
            installs += 1
        _install(home, env)

    def run() -> None:
        try:
            ensure_deepseek_setup(
                dict(base_env),
                installer=install,
                external_preflight=_external,
                ready_preflight=_ready,
            )
        except BaseException as exc:
            failures.append(exc)

    threads = [threading.Thread(target=run) for _ in range(4)]
    for thread in threads:
        thread.start()
    for thread in threads:
        thread.join()
    assert failures == []
    assert installs == 1
