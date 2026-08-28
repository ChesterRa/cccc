#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ASSET_DIR=${1:?usage: package_release_assets.sh ASSET_DIR VERSION [WHEEL_VERSION]}
VERSION=${2:?usage: package_release_assets.sh ASSET_DIR VERSION [WHEEL_VERSION]}
WHEEL_VERSION=${3:-$(sed -n 's/^version = "\([^"]*\)"/\1/p' "$ROOT_DIR/pyproject.toml" | head -1)}

if ! [[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z]+([.-][0-9A-Za-z]+)*)?(\+[0-9A-Za-z]+([.-][0-9A-Za-z]+)*)?$ ]]; then
  echo "invalid semantic version: $VERSION" >&2
  exit 1
fi
command -v sha256sum >/dev/null 2>&1 || {
  echo "sha256sum is required" >&2
  exit 1
}

expected=$(mktemp)
actual=$(mktemp)
trap 'rm -f "$expected" "$actual"' EXIT
printf '%s\n' \
  "cccc-v${VERSION}-aarch64-apple-darwin.tar.gz" \
  "cccc-v${VERSION}-x86_64-apple-darwin.tar.gz" \
  "cccc-v${VERSION}-x86_64-pc-windows-msvc.zip" \
  "cccc-v${VERSION}-x86_64-unknown-linux-gnu.tar.gz" \
  "cccc_pair-${WHEEL_VERSION}-py3-none-macosx_11_0_arm64.whl" \
  "cccc_pair-${WHEEL_VERSION}-py3-none-macosx_11_0_x86_64.whl" \
  "cccc_pair-${WHEEL_VERSION}-py3-none-manylinux_2_28_x86_64.whl" \
  "cccc_pair-${WHEEL_VERSION}-py3-none-win_amd64.whl" > "$expected"

find "$ASSET_DIR" -maxdepth 1 -type f \
  \( -name 'cccc-v*.tar.gz' -o -name 'cccc-v*.zip' -o -name 'cccc_pair-*.whl' \) \
  -exec basename {} \; | LC_ALL=C sort > "$actual"
if ! diff -u "$expected" "$actual"; then
  echo "release payload set does not match the four supported targets" >&2
  exit 1
fi

rm -f "$ASSET_DIR/SHA256SUMS"
while IFS= read -r archive; do
  (cd "$ASSET_DIR" && sha256sum "$archive") >> "$ASSET_DIR/SHA256SUMS"
done < "$expected"

for installer in install.sh install.ps1; do
  source_path="$ROOT_DIR/scripts/$installer"
  grep -Fq '@CCCC_VERSION@' "$source_path"
  grep -Fq '@CCCC_RELEASE_TAG_PREFIX@' "$source_path"
  sed \
    -e "s/@CCCC_VERSION@/$VERSION/g" \
    -e 's/@CCCC_RELEASE_TAG_PREFIX@/v/g' \
    "$source_path" > "$ASSET_DIR/$installer"
  if grep -Eq '@CCCC_(VERSION|RELEASE_TAG_PREFIX)@' "$ASSET_DIR/$installer"; then
    echo "failed to render $installer release metadata" >&2
    exit 1
  fi
done
chmod 755 "$ASSET_DIR/install.sh"

echo "OK: prepared four archives, four Rust-only wheels, SHA256SUMS, and versioned installers for v$VERSION"
