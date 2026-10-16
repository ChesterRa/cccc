"""Execute the generated website installer against a local release server."""
import functools
import hashlib
import http.server
import os
import platform
import shutil
import subprocess
import tarfile
import threading
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[1]


@pytest.fixture
def website(tmp_path):
    scripts = tmp_path / "scripts"
    scripts.mkdir()
    for name in ("prepare_docs_installers.mjs", "install.sh", "install.ps1"):
        shutil.copyfile(ROOT / "scripts" / name, scripts / name)
    (tmp_path / "Cargo.toml").write_text('[workspace.package]\nversion = "0.1.0"\n')
    subprocess.run(
        ["node", str(scripts / "prepare_docs_installers.mjs")], check=True,
        env={**os.environ, "CCCC_DOCS_INSTALL_VERSION": "0.2.0"},
    )
    targets = {
        ("Darwin", "arm64"): "aarch64-apple-darwin",
        ("Linux", "x86_64"): "x86_64-unknown-linux-gnu",
    }
    target = targets.get((platform.system(), platform.machine()))
    if target is None:
        pytest.skip("Unix installer requires a supported host")
    for version in ("1.0.0", "1.1.0"):
        package = f"cccc-v{version}-{target}"
        binary = tmp_path / package / "cccc"
        binary.parent.mkdir()
        binary.write_text(
            f'#!/bin/sh\n[ "$1" = "--version" ] || exit 1\necho "cccc {version}"\n'
        )
        binary.chmod(0o755)
        release = tmp_path / "releases" / "download" / f"v{version}"
        release.mkdir(parents=True)
        archive = release / f"{package}.tar.gz"
        with tarfile.open(archive, "w:gz") as bundle:
            bundle.add(binary.parent, arcname=package)
        digest = hashlib.sha256(archive.read_bytes()).hexdigest()
        entries = []
        for other in (*targets.values(), "x86_64-pc-windows-msvc"):
            ext = "zip" if other.endswith("msvc") else "tar.gz"
            entries.append(f'{digest if other == target else "0" * 64}  cccc-v{version}-{other}.{ext}\n')
        (release / "SHA256SUMS").write_text("".join(entries))
    state = {"latest": "1.0.0", "requests": []}

    class Handler(http.server.SimpleHTTPRequestHandler):
        def do_GET(self):
            state["requests"].append(self.path)
            if self.path == "/releases/latest" and state["latest"]:
                self.send_response(302)
                self.send_header("Location", f'/releases/tag/v{state["latest"]}')
                self.end_headers()
            elif self.path.startswith("/releases/tag/"):
                self.send_response(200)
                self.end_headers()
            else:
                super().do_GET()

        def log_message(self, *_args):
            pass

    server = http.server.ThreadingHTTPServer(
        ("127.0.0.1", 0), functools.partial(Handler, directory=str(tmp_path)),
    )
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()

    def install(version=""):
        return subprocess.run(
            ["sh", str(tmp_path / "docs/public/install.sh")],
            env={**os.environ, "HOME": str(tmp_path / "home"),
                 "CCCC_INSTALL_DIR": str(tmp_path / "bin"), "CCCC_VERSION": version,
                 "CCCC_RELEASE_BASE_URL": f"http://127.0.0.1:{server.server_port}/releases",
                 "CCCC_NO_MODIFY_PATH": "1"},
            capture_output=True, text=True,
        )

    try:
        yield state, install, tmp_path / "bin/cccc"
    finally:
        server.shutdown()
        server.server_close()
        thread.join()


def test_website_installer_follows_latest_without_rebuilding(website):
    state, install, binary = website
    for version in ("1.0.0", "1.1.0"):
        state["latest"] = version
        result = install()
        assert result.returncode == 0, result.stderr
        assert subprocess.check_output([str(binary), "--version"], text=True).strip() == f"cccc {version}"
    assert state["requests"].count("/releases/latest") == 2


def test_explicit_version_bypasses_latest(website):
    state, install, binary = website
    state["latest"] = None
    result = install("v1.0.0")
    assert result.returncode == 0, result.stderr
    assert subprocess.check_output([str(binary), "--version"], text=True).strip() == "cccc 1.0.0"
    assert "/releases/latest" not in state["requests"]


def test_failed_latest_resolution_preserves_installed_version(website):
    state, install, binary = website
    assert install("1.0.0").returncode == 0
    previous = binary.read_bytes()
    state["latest"] = None
    state["requests"].clear()
    result = install()
    assert result.returncode != 0
    assert "could not resolve the latest release" in result.stderr
    assert state["requests"] == ["/releases/latest"]
    assert binary.read_bytes() == previous
