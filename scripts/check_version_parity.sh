#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"
python_version="$(sed -n 's/^version = "\([^"]*\)"/\1/p' "$ROOT_DIR/pyproject.toml" | head -1)"
rust_version="$(sed -n 's/^version = "\([^"]*\)"/\1/p' "$ROOT_DIR/Cargo.toml" | head -1)"

if [[ -z "$python_version" || -z "$rust_version" ]]; then
  echo "failed to read CCCC versions from pyproject.toml and Cargo.toml" >&2
  exit 1
fi

if [[ "$python_version" != "$rust_version" ]]; then
  echo "CCCC version mismatch: Python=$python_version Rust=$rust_version" >&2
  exit 1
fi

echo "CCCC version parity: $rust_version"
