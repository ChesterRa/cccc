#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
TMP_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/cccc-release-assets-test.XXXXXX")"
trap 'rm -rf "$TMP_ROOT"' EXIT
VERSION=0.0.0-test

for archive in \
  "cccc-v${VERSION}-aarch64-apple-darwin.tar.gz" \
  "cccc-v${VERSION}-x86_64-apple-darwin.tar.gz" \
  "cccc-v${VERSION}-x86_64-pc-windows-msvc.zip" \
  "cccc-v${VERSION}-x86_64-unknown-linux-gnu.tar.gz"; do
  printf 'fixture %s\n' "$archive" > "$TMP_ROOT/$archive"
done

"$ROOT_DIR/scripts/package_release_assets.sh" "$TMP_ROOT" "$VERSION"
test "$(wc -l < "$TMP_ROOT/SHA256SUMS" | tr -d ' ')" -eq 4
grep -Fq "DEFAULT_VERSION=\"$VERSION\"" "$TMP_ROOT/install.sh"
grep -Fq "defaultVersion = \"$VERSION\"" "$TMP_ROOT/install.ps1"
grep -Fq 'RELEASE_TAG_PREFIX="rust-v"' "$TMP_ROOT/install.sh"
grep -Fq 'releaseTagPrefix = "rust-v"' "$TMP_ROOT/install.ps1"

printf 'unexpected\n' > "$TMP_ROOT/cccc-v${VERSION}-aarch64-unknown-linux-gnu.tar.gz"
if "$ROOT_DIR/scripts/package_release_assets.sh" "$TMP_ROOT" "$VERSION"; then
  echo "release asset packaging accepted an unexpected archive" >&2
  exit 1
fi

test "$("$ROOT_DIR/scripts/release_prerelease.sh" 0.5.0)" = false
test "$("$ROOT_DIR/scripts/release_prerelease.sh" 0.5.0+build-1)" = false
test "$("$ROOT_DIR/scripts/release_prerelease.sh" 0.5.0-preview.1)" = true
test "$("$ROOT_DIR/scripts/release_prerelease.sh" 0.5.0-preview.1+build-1)" = true
grep -Fq 'scripts/release_prerelease.sh "$version"' "$ROOT_DIR/.github/workflows/release-rust.yml"
grep -Fq 'prerelease: ${{ env.RELEASE_PRERELEASE }}' "$ROOT_DIR/.github/workflows/release-rust.yml"

echo "OK: release assets"
