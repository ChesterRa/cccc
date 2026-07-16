#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

"$ROOT_DIR/scripts/build_web.sh"

python3 -m pip install -U pip build twine
python3 -m compileall -q "$ROOT_DIR/src/cccc"
python3 -m build "$ROOT_DIR"
python3 -m twine check "$ROOT_DIR"/dist/*

echo "OK: built dist/* with bundled Web UI"
