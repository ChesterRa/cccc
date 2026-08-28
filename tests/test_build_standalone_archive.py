from __future__ import annotations

import hashlib
import tarfile
import zipfile
from pathlib import Path

import pytest

from scripts.build_standalone_archive import build


def _project(root: Path) -> None:
    root.joinpath("Cargo.toml").write_text(
        '[workspace.package]\nversion = "0.4.36-rc1"\n',
        encoding="utf-8",
    )
    root.joinpath("LICENSE").write_text("license\n", encoding="utf-8")
    root.joinpath("README.md").write_text("readme\n", encoding="utf-8")
    root.joinpath("docs").mkdir()
    root.joinpath("docs/rust-migration.md").write_text("migration\n", encoding="utf-8")


@pytest.mark.parametrize(
    ("target", "suffix", "executable"),
    [
        ("x86_64-unknown-linux-gnu", ".tar.gz", "cccc"),
        ("x86_64-apple-darwin", ".tar.gz", "cccc"),
        ("aarch64-apple-darwin", ".tar.gz", "cccc"),
        ("x86_64-pc-windows-msvc", ".zip", "cccc.exe"),
    ],
)
def test_builds_expected_platform_archive(
    tmp_path: Path, target: str, suffix: str, executable: str
) -> None:
    _project(tmp_path)
    binary = tmp_path / executable
    binary.write_bytes(b"release executable")

    archive = build(binary, tmp_path / "dist", target=target, root=tmp_path)
    package = f"cccc-v0.4.36-rc1-{target}"

    assert archive.name == f"{package}{suffix}"
    if suffix == ".zip":
        with zipfile.ZipFile(archive) as opened:
            assert opened.read(f"{package}/{executable}") == binary.read_bytes()
    else:
        with tarfile.open(archive, "r:gz") as opened:
            stream = opened.extractfile(f"{package}/{executable}")
            assert stream is not None
            assert stream.read() == binary.read_bytes()


def test_archive_is_reproducible(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    _project(tmp_path)
    binary = tmp_path / "cccc"
    binary.write_bytes(b"deterministic")
    monkeypatch.setenv("SOURCE_DATE_EPOCH", "1770000000")

    first = build(
        binary, tmp_path / "first", target="x86_64-unknown-linux-gnu", root=tmp_path
    )
    second = build(
        binary, tmp_path / "second", target="x86_64-unknown-linux-gnu", root=tmp_path
    )

    assert (
        hashlib.sha256(first.read_bytes()).digest()
        == hashlib.sha256(second.read_bytes()).digest()
    )
