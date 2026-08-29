from __future__ import annotations

import json
import os
import subprocess
import tomllib
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
RESOLVER = "scripts/resolve_docs_installer_version.mjs"


def _release(version: str, *, complete: bool) -> dict[str, object]:
    names = [
        f"cccc-v{version}-aarch64-apple-darwin.tar.gz",
        f"cccc-v{version}-x86_64-apple-darwin.tar.gz",
        f"cccc-v{version}-x86_64-pc-windows-msvc.zip",
        f"cccc-v{version}-x86_64-unknown-linux-gnu.tar.gz",
        "SHA256SUMS",
        "install.ps1",
        "install.sh",
    ]
    return {
        "tag_name": f"v{version}",
        "draft": False,
        "prerelease": "-" in version,
        "assets": [
            {"name": name, "state": "uploaded"}
            for name in (names if complete else names[:-1])
        ],
    }


def _resolve(metadata: Path) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        ["node", RESOLVER, "--metadata", str(metadata)],
        cwd=ROOT,
        capture_output=True,
        text=True,
    )


def test_docs_installer_renderer_accepts_a_released_version_override() -> None:
    selected_version = "0.4.34-rc3"
    selected_env = {**os.environ, "CCCC_DOCS_INSTALL_VERSION": selected_version}
    try:
        subprocess.run(
            ["node", "scripts/prepare_docs_installers.mjs"],
            cwd=ROOT,
            env=selected_env,
            check=True,
        )
        shell_installer = (ROOT / "docs/public/install.sh").read_text(encoding="utf-8")
        powershell_installer = (ROOT / "docs/public/install.ps1").read_text(encoding="utf-8")
        assert f'DEFAULT_VERSION="{selected_version}"' in shell_installer
        assert f'$defaultVersion = "{selected_version}"' in powershell_installer
    finally:
        subprocess.run(["node", "scripts/prepare_docs_installers.mjs"], cwd=ROOT, check=True)


def test_docs_installer_resolver_skips_prerelease_and_newer_incomplete_release(
    tmp_path: Path,
) -> None:
    metadata = tmp_path / "releases.json"
    metadata.write_text(
        json.dumps(
            [
                _release("0.4.35", complete=False),
                _release("0.4.34-rc3", complete=True),
                _release("0.4.33", complete=True),
            ]
        ),
        encoding="utf-8",
    )

    resolved = _resolve(metadata)

    assert resolved.returncode == 0
    assert resolved.stdout.strip() == "0.4.33"


def test_docs_installer_resolver_sorts_complete_stable_releases_by_semver(
    tmp_path: Path,
) -> None:
    metadata = tmp_path / "releases.json"
    metadata.write_text(
        json.dumps(
            [
                _release("0.4.20", complete=True),
                _release("0.4.34-rc3", complete=True),
                _release("0.4.33", complete=True),
                _release("0.4.34-rc10", complete=True),
            ]
        ),
        encoding="utf-8",
    )

    resolved = _resolve(metadata)

    assert resolved.returncode == 0
    assert resolved.stdout.strip() == "0.4.33"


def test_docs_installer_resolver_prefers_a_stable_release_over_its_prerelease(tmp_path: Path) -> None:
    metadata = tmp_path / "releases.json"
    metadata.write_text(
        json.dumps(
            [
                _release("0.4.34-rc10", complete=True),
                _release("0.4.34", complete=True),
            ]
        ),
        encoding="utf-8",
    )

    resolved = _resolve(metadata)

    assert resolved.returncode == 0
    assert resolved.stdout.strip() == "0.4.34"


def test_docs_installer_resolver_rejects_only_prereleases(tmp_path: Path) -> None:
    metadata = tmp_path / "releases.json"
    metadata.write_text(
        json.dumps([_release("0.4.36-rc1", complete=True)]),
        encoding="utf-8",
    )

    resolved = _resolve(metadata)

    assert resolved.returncode != 0
    assert "published stable GitHub Release" in resolved.stderr
    assert "complete installer asset set" in resolved.stderr


def test_docs_installer_resolver_rejects_an_incomplete_release_set(tmp_path: Path) -> None:
    metadata = tmp_path / "releases.json"
    metadata.write_text(
        json.dumps([_release("0.4.34", complete=False)]),
        encoding="utf-8",
    )

    resolved = _resolve(metadata)

    assert resolved.returncode != 0
    assert "published stable GitHub Release" in resolved.stderr
    assert "complete installer asset set" in resolved.stderr


def test_rust_only_pip_guidance_cannot_fall_back_to_a_python_release() -> None:
    stable_command = 'python -m pip install -U "cccc-pair>=0.4.36"'
    active_guides = [
        "README.md",
        "README.zh-CN.md",
        "README.ja.md",
        "crates/cccc-cli/README.md",
        "docs/rust-migration.md",
        "docs/guide/faq.md",
        "docs/guide/getting-started/index.md",
        "docs/guide/operations.md",
    ]

    for relative_path in active_guides:
        contents = (ROOT / relative_path).read_text(encoding="utf-8")
        assert stable_command in contents, relative_path
        assert "python -m pip install -U cccc-pair" not in contents, relative_path

    with (ROOT / "pyproject.toml").open("rb") as stream:
        version = str(tomllib.load(stream)["project"]["version"])
    preparing_0_4_36 = version == "0.4.35" or version.startswith(
        ("0.4.36a", "0.4.36b", "0.4.36rc")
    )
    for relative_path in (
        "README.md",
        "README.zh-CN.md",
        "README.ja.md",
        "docs/guide/faq.md",
        "docs/guide/getting-started/index.md",
    ):
        contents = (ROOT / relative_path).read_text(encoding="utf-8")
        if preparing_0_4_36:
            assert '"cccc-pair>=0.4.36rc0"' in contents, relative_path
        else:
            assert '"cccc-pair>=0.4.36rc0"' not in contents, relative_path


def test_prepublication_notice_cannot_survive_the_stable_0_4_36_bump() -> None:
    with (ROOT / "pyproject.toml").open("rb") as stream:
        version = str(tomllib.load(stream)["project"]["version"])
    markers = {
        "README.md": "repository is preparing v0.4.36",
        "README.zh-CN.md": "当前仓库正在准备 v0.4.36",
        "README.ja.md": "このリポジトリは v0.4.36 を準備中",
        "docs/guide/faq.md": "v0.4.36 is being",
        "docs/guide/getting-started/index.md": "v0.4.36 is being",
        "docs/guide/operations.md": "v0.4.36 is being",
    }
    preparing_0_4_36 = version == "0.4.35" or version.startswith(
        ("0.4.36a", "0.4.36b", "0.4.36rc")
    )

    for relative_path, marker in markers.items():
        contents = (ROOT / relative_path).read_text(encoding="utf-8")
        if preparing_0_4_36:
            assert marker in contents, relative_path
        else:
            assert marker not in contents, relative_path
