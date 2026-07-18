#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
from pathlib import Path
from typing import Mapping


def discover_test_files(root: Path) -> list[str]:
    root = root.resolve()
    candidates = [*root.glob("tests/**/test_*.py"), *root.glob("tests/**/*_test.py")]
    return sorted({path.relative_to(root).as_posix() for path in candidates if path.is_file()})


def test_file_weights(root: Path) -> dict[str, int]:
    return {
        relative: max(1, len((root / relative).read_text(encoding="utf-8", errors="replace").splitlines()))
        for relative in discover_test_files(root)
    }


def assign_shards(weights: Mapping[str, int], shard_count: int) -> list[list[str]]:
    if shard_count <= 0:
        raise ValueError("shard_count must be positive")
    shards: list[list[str]] = [[] for _ in range(shard_count)]
    loads = [0] * shard_count
    for relative, weight in sorted(weights.items(), key=lambda item: (-item[1], item[0])):
        shard_index = min(range(shard_count), key=lambda index: (loads[index], index))
        shards[shard_index].append(relative)
        loads[shard_index] += weight
    for shard in shards:
        shard.sort()
    return shards


def select_shard(weights: Mapping[str, int], *, shard_count: int, shard_index: int) -> list[str]:
    if shard_count <= 0:
        raise ValueError("shard_count must be positive")
    if shard_index < 0 or shard_index >= shard_count:
        raise ValueError(f"shard_index must be between 0 and {shard_count - 1}")
    return assign_shards(weights, shard_count)[shard_index]


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description="Select a deterministic, line-count-balanced pytest file shard.")
    parser.add_argument("--root", type=Path, default=Path.cwd())
    parser.add_argument("--total", type=int, required=True, dest="shard_count")
    parser.add_argument("--index", type=int, required=True, dest="shard_index")
    parser.add_argument("--json", action="store_true", dest="as_json")
    return parser


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    files = select_shard(
        test_file_weights(args.root),
        shard_count=args.shard_count,
        shard_index=args.shard_index,
    )
    if args.as_json:
        print(json.dumps(files))
    else:
        print("\n".join(files))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
