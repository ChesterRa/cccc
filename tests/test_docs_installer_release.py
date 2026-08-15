from __future__ import annotations

import json
import os
import subprocess
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


def test_docs_installer_resolver_skips_a_newer_incomplete_release(tmp_path: Path) -> None:
    metadata = tmp_path / "releases.json"
    metadata.write_text(
        json.dumps([_release("0.4.34-rc4", complete=False), _release("0.4.34-rc3", complete=True)]),
        encoding="utf-8",
    )

    resolved = _resolve(metadata)

    assert resolved.returncode == 0
    assert resolved.stdout.strip() == "0.4.34-rc3"


def test_docs_installer_resolver_rejects_an_incomplete_release_set(tmp_path: Path) -> None:
    metadata = tmp_path / "releases.json"
    metadata.write_text(
        json.dumps([_release("0.4.34-rc4", complete=False)]),
        encoding="utf-8",
    )

    resolved = _resolve(metadata)

    assert resolved.returncode != 0
    assert "complete installer asset set" in resolved.stderr
