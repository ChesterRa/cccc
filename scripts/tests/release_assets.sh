#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
TMP_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/cccc-release-assets-test.XXXXXX")"
trap 'rm -rf "$TMP_ROOT"' EXIT
VERSION=0.0.0-test
WHEEL_VERSION=0.0.0test

if command -v python >/dev/null 2>&1; then
  VERSION_PYTHON=python
elif command -v python3 >/dev/null 2>&1; then
  VERSION_PYTHON=python3
else
  echo "python is required to validate release versions" >&2
  exit 1
fi

for archive in \
  "cccc-v${VERSION}-aarch64-apple-darwin.tar.gz" \
  "cccc-v${VERSION}-x86_64-apple-darwin.tar.gz" \
  "cccc-v${VERSION}-x86_64-pc-windows-msvc.zip" \
  "cccc-v${VERSION}-x86_64-unknown-linux-gnu.tar.gz"; do
  printf 'fixture %s\n' "$archive" > "$TMP_ROOT/$archive"
done
for wheel in \
  "cccc_pair-${WHEEL_VERSION}-py3-none-macosx_11_0_arm64.whl" \
  "cccc_pair-${WHEEL_VERSION}-py3-none-macosx_11_0_x86_64.whl" \
  "cccc_pair-${WHEEL_VERSION}-py3-none-manylinux_2_28_x86_64.whl" \
  "cccc_pair-${WHEEL_VERSION}-py3-none-win_amd64.whl"; do
  printf 'fixture %s\n' "$wheel" > "$TMP_ROOT/$wheel"
done

"$ROOT_DIR/scripts/package_release_assets.sh" "$TMP_ROOT" "$VERSION" "$WHEEL_VERSION"
test "$(wc -l < "$TMP_ROOT/SHA256SUMS" | tr -d ' ')" -eq 8
grep -Fq "DEFAULT_VERSION=\"$VERSION\"" "$TMP_ROOT/install.sh"
grep -Fq "defaultVersion = \"$VERSION\"" "$TMP_ROOT/install.ps1"
grep -Fq 'count != 4 && count != 8' "$TMP_ROOT/install.sh"
grep -Fq '$checksumEntries.Count -ne 4 -and $checksumEntries.Count -ne 8' "$TMP_ROOT/install.ps1"
grep -Fq 'CCCC_RELEASE_TAG_PREFIX:-v' "$TMP_ROOT/install.sh"
grep -Fq 'CCCC_RELEASE_TAG_PREFIX' "$TMP_ROOT/install.ps1"
grep -Fq '"v"' "$TMP_ROOT/install.ps1"

printf 'unexpected\n' > "$TMP_ROOT/cccc-v${VERSION}-aarch64-unknown-linux-gnu.tar.gz"
if "$ROOT_DIR/scripts/package_release_assets.sh" "$TMP_ROOT" "$VERSION" "$WHEEL_VERSION"; then
  echo "release asset packaging accepted an unexpected archive" >&2
  exit 1
fi

for versions in \
  "0.5.0 0.5.0 v0.5.0" \
  "0.5.0a1 0.5.0-alpha1 v0.5.0-alpha1" \
  "0.5.0b2 0.5.0-beta2 v0.5.0-beta2" \
  "0.5.0rc3 0.5.0-rc3 v0.5.0-rc3"; do
  read -r package_version rust_version tag <<< "$versions"
  "$VERSION_PYTHON" "$ROOT_DIR/scripts/check_release_versions.py" \
    --package-version "$package_version" \
    --rust-version "$rust_version" \
    --tag "$tag" >/dev/null
done

for versions in \
  "0.5.0rc1 0.5.0-rc2 v0.5.0-rc1" \
  "0.5.0rc1 0.5.0-rc1 v0.5.0rc1" \
  "0.5.0-rc1 0.5.0-rc1 v0.5.0-rc1"; do
  read -r package_version rust_version tag <<< "$versions"
  if "$VERSION_PYTHON" "$ROOT_DIR/scripts/check_release_versions.py" \
    --package-version "$package_version" \
    --rust-version "$rust_version" \
    --tag "$tag" >/dev/null 2>&1; then
    echo "release version validation accepted invalid versions: $versions" >&2
    exit 1
  fi
done

echo "OK: release assets"
