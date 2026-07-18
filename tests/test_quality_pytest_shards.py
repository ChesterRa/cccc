from __future__ import annotations

from pathlib import Path

import pytest

from scripts.quality.pytest_shards import assign_shards, discover_test_files, select_shard


def _write_lines(path: Path, count: int) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("line\n" * count, encoding="utf-8")


def test_discovery_is_recursive_and_sorted(tmp_path: Path) -> None:
    expected = ["tests/integration/test_beta.py", "tests/test_alpha.py"]
    for relative in reversed(expected):
        _write_lines(tmp_path / relative, 1)
    _write_lines(tmp_path / "tests/helpers.py", 1)

    assert discover_test_files(tmp_path) == expected


def test_assignment_is_stable_complete_and_has_no_duplicates() -> None:
    weights = {
        "tests/test_a.py": 50,
        "tests/test_b.py": 40,
        "tests/test_c.py": 30,
        "tests/test_d.py": 20,
        "tests/test_e.py": 10,
    }

    first = assign_shards(weights, 3)
    second = assign_shards(dict(reversed(list(weights.items()))), 3)

    assert first == second
    flattened = [path for shard in first for path in shard]
    assert sorted(flattened) == sorted(weights)
    assert len(flattened) == len(set(flattened))


def test_lpt_assignment_keeps_load_spread_within_largest_file() -> None:
    weights = {f"tests/test_{index}.py": weight for index, weight in enumerate([100, 90, 80, 70, 60, 50, 40])}

    shards = assign_shards(weights, 3)
    loads = [sum(weights[path] for path in shard) for shard in shards]

    assert max(loads) - min(loads) <= max(weights.values())


@pytest.mark.parametrize(("shard_count", "shard_index"), [(0, 0), (-1, 0), (2, -1), (2, 2)])
def test_select_shard_rejects_invalid_coordinates(shard_count: int, shard_index: int) -> None:
    with pytest.raises(ValueError):
        select_shard({"tests/test_a.py": 1}, shard_count=shard_count, shard_index=shard_index)
