#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TARGET="$(rustc -vV | sed -n 's/^host: //p')"
VERSION="$(sed -n 's/^version = "\([^"]*\)"/\1/p' "$ROOT_DIR/Cargo.toml" | head -1)"
NAME="cccc-v${VERSION}-${TARGET}"

"$ROOT_DIR/scripts/build_web.sh"
cargo build --manifest-path "$ROOT_DIR/Cargo.toml" --release --locked -p cccc-cli --bin cccc

rm -rf "$ROOT_DIR/dist/$NAME"
mkdir -p "$ROOT_DIR/dist/$NAME"
cp "$ROOT_DIR/target/release/cccc" "$ROOT_DIR/dist/$NAME/"
cp "$ROOT_DIR/LICENSE" "$ROOT_DIR/README.md" "$ROOT_DIR/docs/rust-migration.md" "$ROOT_DIR/dist/$NAME/"
tar -C "$ROOT_DIR/dist" -czf "$ROOT_DIR/dist/$NAME.tar.gz" "$NAME"
echo "OK: built dist/$NAME.tar.gz"
