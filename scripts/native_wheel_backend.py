"""Reject generic source builds that cannot produce the native CCCC artifact.

Published platform wheels are assembled by ``build_native_wheel.py`` from the
same release executable as the standalone archive. A source-tree PEP 517 build
cannot reproduce that artifact and must not silently install an empty wheel.
"""

from __future__ import annotations


_MESSAGE = (
    "CCCC does not build a wheel, editable install, or sdist from source "
    "through PEP 517. "
    "Install a published platform wheel, use the website installer, or build "
    "the native executable with scripts/build_package.sh (or "
    "scripts/build_package.ps1 on Windows)."
)


def get_requires_for_build_wheel(config_settings=None) -> list[str]:
    return []


def get_requires_for_build_sdist(config_settings=None) -> list[str]:
    return []


def get_requires_for_build_editable(config_settings=None) -> list[str]:
    return []


def build_wheel(wheel_directory, config_settings=None, metadata_directory=None) -> str:
    raise RuntimeError(_MESSAGE)


def build_sdist(sdist_directory, config_settings=None) -> str:
    raise RuntimeError(_MESSAGE)


def build_editable(
    wheel_directory, config_settings=None, metadata_directory=None
) -> str:
    raise RuntimeError(_MESSAGE)
