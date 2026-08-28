#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TARGET="$(rustc -vV | sed -n 's/^host: //p')"
BINARY="$ROOT_DIR/target/release/cccc"

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
  --output-dir "$ROOT_DIR/dist"

echo "OK: built the native CCCC archive in dist/"
