import os
from pathlib import Path
import shutil
import subprocess

import pytest


ROOT = Path(__file__).resolve().parents[1]


def test_reused_web_build_script_uses_the_invoking_package(tmp_path: Path) -> None:
    rustc = shutil.which("rustc")
    if rustc is None:
        pytest.skip("the build-script regression requires rustc")
    manifests = [tmp_path / name / "crates/cccc-web" for name in ("first", "second")]
    for manifest in manifests:
        assets = manifest / "assets/web-dist"
        assets.mkdir(parents=True)
        (assets / "index.html").write_text(manifest.parent.parent.name, encoding="utf-8")
    executable = tmp_path / ("build-script.exe" if os.name == "nt" else "build-script")
    subprocess.run(
        [rustc, "--edition=2024", str(ROOT / "crates/cccc-web/build.rs"), "-o", str(executable)],
        env={**os.environ, "CARGO_MANIFEST_DIR": str(manifests[0])},
        check=True,
        capture_output=True,
        text=True,
        timeout=60,
    )
    for manifest in manifests:
        result = subprocess.run(
            [str(executable)],
            cwd=tmp_path,
            env={**os.environ, "CARGO_MANIFEST_DIR": str(manifest)},
            check=True,
            capture_output=True,
            text=True,
            timeout=10,
        )
        assert f"cargo:rustc-env=CCCC_WEB_DIST_DIR={manifest / 'assets/web-dist'}" in result.stdout
