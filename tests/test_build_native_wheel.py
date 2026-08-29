from __future__ import annotations

import base64
import hashlib
import os
import zipfile
from pathlib import Path

import pytest

from scripts.build_native_wheel import build
from scripts import native_wheel_backend
from scripts.verify_native_wheel import verify


def _project(root: Path) -> None:
    root.joinpath("pyproject.toml").write_text(
        """[project]
name = "cccc-pair"
version = "0.4.36rc1"
description = "Rust-only CCCC fixture"
readme = { file = "README.md", content-type = "text/markdown" }
""",
        encoding="utf-8",
    )
    root.joinpath("LICENSE").write_text("fixture license\n", encoding="utf-8")
    root.joinpath("README.md").write_text(
        "# CCCC fixture\n\nNative product description.\n", encoding="utf-8"
    )


@pytest.mark.parametrize(
    ("platform_tag", "executable"),
    [
        ("manylinux_2_28_x86_64", "cccc"),
        ("macosx_11_0_x86_64", "cccc"),
        ("macosx_11_0_arm64", "cccc"),
        ("win_amd64", "cccc.exe"),
    ],
)
def test_builds_dependency_free_rust_only_wheel(
    tmp_path: Path, platform_tag: str, executable: str
) -> None:
    _project(tmp_path)
    binary = tmp_path / executable
    binary.write_bytes(b"native-cccc\0fixture")

    wheel = build(binary, tmp_path / "dist", platform_tag=platform_tag, root=tmp_path)
    payload = verify(wheel, platform_tag=platform_tag, binary=binary)

    assert payload == binary.read_bytes()
    assert wheel.name == f"cccc_pair-0.4.36rc1-py3-none-{platform_tag}.whl"
    with zipfile.ZipFile(wheel) as archive:
        assert any(
            name.endswith(f".data/scripts/{executable}") for name in archive.namelist()
        )
        pip_marker = next(
            name
            for name in archive.namelist()
            if name.endswith(".data/scripts/.cccc-standalone")
        )
        assert archive.read(pip_marker) == b"pip-v1\n"
        metadata = archive.read("cccc_pair-0.4.36rc1.dist-info/METADATA").decode()
    assert "Requires-Dist:" not in metadata
    assert "Programming Language :: Rust" in metadata
    assert "Description-Content-Type: text/markdown" in metadata
    assert "# CCCC fixture" in metadata


def test_wheel_is_reproducible(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    _project(tmp_path)
    binary = tmp_path / "cccc"
    binary.write_bytes(b"deterministic payload")
    monkeypatch.setenv("SOURCE_DATE_EPOCH", "1770000000")

    first = build(
        binary, tmp_path / "first", platform_tag="linux_x86_64", root=tmp_path
    )
    second = build(
        binary, tmp_path / "second", platform_tag="linux_x86_64", root=tmp_path
    )

    assert (
        hashlib.sha256(first.read_bytes()).digest()
        == hashlib.sha256(second.read_bytes()).digest()
    )


def test_rejects_a_universal_platform_tag(tmp_path: Path) -> None:
    _project(tmp_path)
    binary = tmp_path / "cccc"
    binary.write_bytes(b"payload")

    with pytest.raises(ValueError, match="invalid native wheel platform tag"):
        build(binary, tmp_path / "dist", platform_tag="any", root=tmp_path)


def test_source_pep517_build_fails_instead_of_installing_an_empty_wheel() -> None:
    assert native_wheel_backend.get_requires_for_build_wheel() == []
    assert native_wheel_backend.get_requires_for_build_editable() == []
    with pytest.raises(RuntimeError, match="does not build a wheel, editable install"):
        native_wheel_backend.build_wheel("unused")
    with pytest.raises(RuntimeError, match="does not build a wheel, editable install"):
        native_wheel_backend.build_sdist("unused")
    with pytest.raises(RuntimeError, match="scripts/build_package.sh"):
        native_wheel_backend.build_editable("unused")


def test_rejects_an_extra_fully_recorded_wheel_member(tmp_path: Path) -> None:
    _project(tmp_path)
    binary = tmp_path / "cccc"
    binary.write_bytes(b"native payload")
    wheel = build(binary, tmp_path / "dist", platform_tag="linux_x86_64", root=tmp_path)
    replacement = tmp_path / "replacement.whl"

    with zipfile.ZipFile(wheel) as source:
        entries = [(info, source.read(info.filename)) for info in source.infolist()]
    record_name = next(
        info.filename for info, _ in entries if info.filename.endswith("/RECORD")
    )
    unexpected = "cccc_pair-0.4.36rc1.data/unexpected.bin"
    rewritten = []
    for info, data in entries:
        if info.filename == record_name:
            digest = hashlib.sha256(b"unexpected").digest()
            encoded = base64.urlsafe_b64encode(digest).rstrip(b"=").decode("ascii")
            data += f"{unexpected},sha256={encoded},10\n".encode()
        rewritten.append((info, data))
    with zipfile.ZipFile(replacement, "w") as destination:
        for info, data in rewritten:
            destination.writestr(info, data)
        destination.writestr(unexpected, b"unexpected")
    os.replace(replacement, wheel)

    with pytest.raises(ValueError, match="invalid Rust-only layout"):
        verify(wheel, platform_tag="linux_x86_64", binary=binary)
