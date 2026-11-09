#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
for command_name in node npm cargo rustc python3; do
  if ! command -v "$command_name" >/dev/null 2>&1; then
    echo "Missing source-package prerequisite: $command_name" >&2
    exit 1
  fi
done
TARGET="$(rustc -vV | sed -n 's/^host: //p')"
BINARY="$ROOT_DIR/target/release/cccc"
# A local build never takes the release archive name, which only CI writes. The label
# names the commit, adds -dirty for uncommitted changes, and is "local" outside Git.
LABEL=local
if [ -e "$ROOT_DIR/.git" ] && SHA="$(git -C "$ROOT_DIR" rev-parse --short=8 HEAD 2>/dev/null)"; then
  LABEL="$SHA"
  if [ -n "$(git -C "$ROOT_DIR" status --porcelain 2>/dev/null)" ]; then
    LABEL="$SHA-dirty"
  fi
fi

node "$ROOT_DIR/scripts/prepare_rust_web_assets.mjs" --install-deps
cargo build \
  --manifest-path "$ROOT_DIR/Cargo.toml" \
  --release \
  --locked \
  --features standalone \
  -p cccc \
  --bin cccc
python3 "$ROOT_DIR/scripts/build_standalone_archive.py" \
  "$BINARY" \
  --target "$TARGET" \
  --output-dir "$ROOT_DIR/dist" \
  --label "$LABEL"
"$BINARY" --version

echo "OK: built the native CCCC archive in dist/"
echo "Run: $BINARY"
