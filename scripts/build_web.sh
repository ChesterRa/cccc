#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

npm ci --prefix "$ROOT_DIR/web"
npm -C "$ROOT_DIR/web" run build

test -f "$ROOT_DIR/web/dist/index.html"
rm -rf "$ROOT_DIR/src/cccc/ports/web/dist"
mkdir -p "$ROOT_DIR/src/cccc/ports/web/dist"
cp -R "$ROOT_DIR/web/dist/." "$ROOT_DIR/src/cccc/ports/web/dist/"
echo "OK: built bundled Web UI -> web/dist and src/cccc/ports/web/dist"
