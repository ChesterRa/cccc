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


def test_rejects_retired_macos_intel_target(tmp_path: Path) -> None:
    _project(tmp_path)
    binary = tmp_path / "cccc"
    binary.write_bytes(b"retired target")

    with pytest.raises(ValueError, match="unsupported release target"):
        build(
            binary,
            tmp_path / "dist",
            target="x86_64-apple-darwin",
            root=tmp_path,
        )


@pytest.mark.parametrize(
    ("target", "suffix", "executable"),
    [
        ("x86_64-unknown-linux-gnu", ".tar.gz", "cccc"),
        ("x86_64-pc-windows-msvc", ".zip", "cccc.exe"),
    ],
)
def test_local_label_names_the_file_and_a_rebuild_replaces_it(
    tmp_path: Path, target: str, suffix: str, executable: str
) -> None:
    _project(tmp_path)
    binary = tmp_path / executable
    binary.write_bytes(b"first build")
    dist = tmp_path / "dist"

    first = build(binary, dist, target=target, root=tmp_path, label="994415c1-dirty")
    binary.write_bytes(b"second build")
    second = build(binary, dist, target=target, root=tmp_path, label="994415c1-dirty")

    assert second == first
    assert first.name == f"cccc-v0.4.36-rc1+994415c1-dirty-{target}{suffix}"
    member = f"cccc-v0.4.36-rc1-{target}/{executable}"
    if suffix == ".zip":
        with zipfile.ZipFile(second) as opened:
            assert opened.read(member) == b"second build"
    else:
        with tarfile.open(second, "r:gz") as opened:
            stream = opened.extractfile(member)
            assert stream is not None
            assert stream.read() == b"second build"


@pytest.mark.parametrize(
    ("target", "suffix", "executable"),
    [
        ("x86_64-unknown-linux-gnu", ".tar.gz", "cccc"),
        ("x86_64-pc-windows-msvc", ".zip", "cccc.exe"),
    ],
)
def test_release_name_is_never_overwritten(
    tmp_path: Path, target: str, suffix: str, executable: str
) -> None:
    _project(tmp_path)
    binary = tmp_path / executable
    binary.write_bytes(b"release build")
    dist = tmp_path / "dist"
    release = build(binary, dist, target=target, root=tmp_path)
    kept = release.read_bytes()
    binary.write_bytes(b"later local build")

    with pytest.raises(ValueError, match="already exists"):
        build(binary, dist, target=target, root=tmp_path)
    local = build(binary, dist, target=target, root=tmp_path, label="local")

    assert release.read_bytes() == kept
    assert local.name == f"cccc-v0.4.36-rc1+local-{target}{suffix}"


@pytest.mark.parametrize("label", ["", "-dirty", "../escape", "a/b", "a b"])
def test_rejects_a_label_that_is_not_one_file_name_part(
    tmp_path: Path, label: str
) -> None:
    _project(tmp_path)
    binary = tmp_path / "cccc"
    binary.write_bytes(b"local build")

    with pytest.raises(ValueError, match="invalid archive label"):
        build(
            binary,
            tmp_path / "dist",
            target="x86_64-unknown-linux-gnu",
            root=tmp_path,
            label=label,
        )
    assert not (tmp_path / "dist").exists()
