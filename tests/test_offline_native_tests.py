from __future__ import annotations

import importlib.util
from pathlib import Path

import pytest

spec = importlib.util.spec_from_file_location(
    "check_offline_native_tests",
    Path(__file__).resolve().parents[1] / "scripts/check_offline_native_tests.py",
)
assert spec is not None and spec.loader is not None
checker = importlib.util.module_from_spec(spec)
spec.loader.exec_module(checker)


def listing(names: tuple[str, ...]) -> str:
    return "\n".join(f"ops::probes::{name}: test" for name in names)


def test_accepts_registered_probes_in_cargo_listing() -> None:
    checker.check_listing(listing(checker.REQUIRED_TESTS) + "\n4 tests, 0 benchmarks\n")


def test_rejects_zero_test_success_output() -> None:
    with pytest.raises(ValueError, match="Missing offline native-runtime tests"):
        checker.check_listing("0 tests, 0 benchmarks\n")


@pytest.mark.parametrize("missing", checker.REQUIRED_TESTS)
def test_rejects_each_missing_probe_even_when_other_tests_remain(missing: str) -> None:
    remaining = tuple(name for name in checker.REQUIRED_TESTS if name != missing)
    with pytest.raises(ValueError, match=missing):
        checker.check_listing(listing(remaining))
