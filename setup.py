from __future__ import annotations

import os
import re
import shutil
from pathlib import Path

from setuptools import Distribution, setup
from setuptools.command.bdist_wheel import bdist_wheel as _bdist_wheel
from setuptools.command.build_py import build_py as _build_py

_BINARY_ENV = "CCCC_BUILD_RUST_BINARY"
_PLATFORM_ENV = "CCCC_BUILD_WHEEL_TAG"
_PLATFORM_RE = re.compile(r"^[A-Za-z0-9_.]+$")
_NATIVE_NAMES = ("cccc-rust", "cccc-rust.exe")


def _native_build_config() -> tuple[Path | None, str | None]:
    raw_binary = str(os.environ.get(_BINARY_ENV) or "").strip()
    raw_platform = str(os.environ.get(_PLATFORM_ENV) or "").strip()
    if bool(raw_binary) != bool(raw_platform):
        raise RuntimeError(f"{_BINARY_ENV} and {_PLATFORM_ENV} must be set together")
    if not raw_binary:
        return None, None
    binary = Path(raw_binary).expanduser().resolve()
    if not binary.is_file():
        raise RuntimeError(f"Rust implementation binary does not exist: {binary}")
    if _PLATFORM_RE.fullmatch(raw_platform) is None:
        raise RuntimeError(f"invalid wheel platform tag: {raw_platform!r}")
    if raw_platform.lower() == "any":
        raise RuntimeError("a wheel containing the Rust implementation cannot use the 'any' platform tag")
    return binary, raw_platform


class BuildPy(_build_py):
    def run(self) -> None:
        super().run()
        native_dir = Path(self.build_lib) / "cccc" / "_native"
        native_dir.mkdir(parents=True, exist_ok=True)
        # PEP 517 may reuse build/lib between pure and native wheel builds.
        # Always clear staged payloads so a later universal wheel stays pure.
        for name in _NATIVE_NAMES:
            native_dir.joinpath(name).unlink(missing_ok=True)

        binary, _ = _native_build_config()
        if binary is None:
            return
        destination = native_dir / ("cccc-rust.exe" if os.name == "nt" else "cccc-rust")
        shutil.copy2(binary, destination)
        if os.name != "nt":
            destination.chmod(destination.stat().st_mode | 0o111)


class BinaryDistribution(Distribution):
    def has_ext_modules(self) -> bool:
        binary, _ = _native_build_config()
        return binary is not None


class BdistWheel(_bdist_wheel):
    def finalize_options(self) -> None:
        super().finalize_options()
        binary, platform = _native_build_config()
        if binary is not None:
            self.root_is_pure = False
            self.plat_name = platform

    def get_tag(self) -> tuple[str, str, str]:
        binary, platform = _native_build_config()
        if binary is not None and platform is not None:
            return "py3", "none", platform
        return super().get_tag()


setup(
    distclass=BinaryDistribution,
    cmdclass={"build_py": BuildPy, "bdist_wheel": BdistWheel},
)
