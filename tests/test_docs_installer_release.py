from __future__ import annotations

import json
import os
import subprocess
import tomllib
from pathlib import Path

import pytest


ROOT = Path(__file__).resolve().parents[1]
RESOLVER = "scripts/resolve_docs_installer_version.mjs"


def _release(version: str, *, complete: bool) -> dict[str, object]:
    names = [
        f"cccc-v{version}-aarch64-apple-darwin.tar.gz",
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


def _resolve(metadata: Path, output: Path | None = None) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        ["node", RESOLVER, "--metadata", str(metadata)]
        + (["--output", str(output)] if output is not None else []),
        cwd=ROOT,
        env={**os.environ, "GITHUB_REPOSITORY": "ChesterRa/cccc"},
        capture_output=True,
        text=True,
    )


def test_release_index_publishes_only_complete_channels_from_the_same_snapshot(tmp_path: Path) -> None:
    metadata = tmp_path / "input.json"
    output = tmp_path / "public/releases.json"
    draft = {**_release("1.3.0", complete=True), "draft": True}
    mislabeled = {**_release("1.3.1", complete=True), "prerelease": True}
    uploading = _release("1.4.0", complete=True)
    uploading["assets"][0]["state"] = "new"
    metadata.write_text(json.dumps([
        _release("1.1.0", complete=True),
        _release("1.2.0-rc10", complete=True),
        _release("1.2.0-rc2", complete=True),
        _release("1.2.0-rc11", complete=False),
        _release("1.2.0", complete=True),
        _release("1.3.0-rc1", complete=False),
        _release("1.3.0", complete=False),
        draft, mislabeled, uploading,
    ]), encoding="utf-8")

    resolved = _resolve(metadata, output)

    assert resolved.returncode == 0, resolved.stderr
    assert resolved.stdout.strip() == "1.2.0"
    assert json.loads(output.read_text(encoding="utf-8")) == {
        "schema_version": 1,
        "repository": "ChesterRa/cccc",
        "channels": {"stable": resolved.stdout.strip(), "rc": "1.2.0-rc10"},
    }
    assert list(output.parent.glob("*.tmp")) == []


@pytest.mark.parametrize(("versions", "expected"), [
    (["1.0.0-rc2", "1.0.0-rc10"], "1.0.0-rc10"),
    (["1.0.0-beta20", "1.0.0-rc1", "1.0.0-alpha100"], "1.0.0-rc1"),
    (["1.0.0-rc.9", "1.0.0-rc10", "1.0.0-rc.11"], "1.0.0-rc.11"),
    (["1.0.0-test.9", "1.0.0-test.10", "1.0.0-test.10.1"], "1.0.0-test.10.1"),
    (["1.0.0-1", "1.0.0-alpha"], "1.0.0-alpha"),
    (["1.0.0-rc999", "1.0.1-alpha1"], "1.0.1-alpha1"),
    (["01.0.0-rc1", "1.0.0-01", "18446744073709551616.0.0-rc1"], None),
    ([], None),
])
def test_release_index_orders_supported_prerelease_versions(
    tmp_path: Path, versions: list[str], expected: str | None,
) -> None:
    metadata = tmp_path / "input.json"
    output = tmp_path / "releases.json"
    releases = [_release("0.9.0", complete=True)]
    releases.extend(_release(version, complete=True) for version in versions)
    for ordered in (releases, list(reversed(releases))):
        metadata.write_text(json.dumps(ordered), encoding="utf-8")
        resolved = _resolve(metadata, output)
        assert resolved.returncode == 0, resolved.stderr
        assert json.loads(output.read_text(encoding="utf-8"))["channels"]["rc"] == expected


@pytest.mark.parametrize("metadata_text", [
    "{", "[]", json.dumps([_release("1.0.0", complete=False)]),
    json.dumps([_release("1.0.0-rc1", complete=True)]),
])
def test_failed_resolution_preserves_the_previous_index(tmp_path: Path, metadata_text: str) -> None:
    metadata = tmp_path / "input.json"
    output = tmp_path / "releases.json"
    metadata.write_text(metadata_text, encoding="utf-8")
    previous = '{"previous": "published snapshot"}\n'
    output.write_text(previous, encoding="utf-8")

    resolved = _resolve(metadata, output)

    assert resolved.returncode != 0
    assert resolved.stdout.strip() == ""
    assert output.read_text(encoding="utf-8") == previous
    assert list(tmp_path.glob("*.tmp")) == []


@pytest.mark.parametrize("failed_page", [None, 1, 2])
def test_release_index_uses_authenticated_paginated_ci_metadata(
    tmp_path: Path, failed_page: int | None,
) -> None:
    output = tmp_path / "releases.json"
    previous = '{"previous": "published snapshot"}\n'
    output.write_text(previous, encoding="utf-8")
    pages = [
        [_release("1.1.0-rc1", complete=True), _release("1.2.0", complete=False)],
        [_release("1.0.0", complete=True)],
    ]
    # Simulate GitHub without consuming API quota or reading real credentials.
    script = f"""
      import assert from 'node:assert/strict';
      const pages = {json.dumps(pages)};
      const failedPage = {json.dumps(failed_page)};
      let calls = 0;
      process.argv = ['node', {json.dumps(RESOLVER)}, '--output', {json.dumps(str(output))}];
      globalThis.fetch = async (url, options) => {{
        calls += 1;
        assert.equal(url, `https://api.github.com/repos/ChesterRa/cccc/releases?per_page=100&page=${{calls}}`);
        assert.equal(options.headers.Authorization, 'Bearer fixture-token');
        assert.ok(options.signal instanceof AbortSignal);
        if (calls === failedPage) return new Response('rate limited', {{status: 403}});
        assert.ok(calls <= pages.length);
        return new Response(JSON.stringify(pages[calls - 1]), {{
          headers: calls < pages.length ? {{Link: '<https://api.github.com/next>; rel="next"'}} : {{}},
        }});
      }};
      await import('./{RESOLVER}');
      assert.equal(calls, 2);
    """
    resolved = subprocess.run(
        ["node", "--input-type=module", "--eval", script], cwd=ROOT,
        env={**os.environ, "GITHUB_REPOSITORY": "ChesterRa/cccc", "GITHUB_TOKEN": "fixture-token"},
        capture_output=True, text=True,
    )
    if failed_page is None:
        assert resolved.returncode == 0, resolved.stderr
        assert resolved.stdout.strip() == "1.0.0"
        assert json.loads(output.read_text(encoding="utf-8"))["channels"] == {
            "stable": "1.0.0", "rc": "1.1.0-rc1",
        }
    else:
        assert resolved.returncode != 0
        assert "Could not list GitHub Releases (403)" in resolved.stderr
        assert output.read_text(encoding="utf-8") == previous


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
