#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
BINARY=${1:?usage: install_real_unix.sh PATH_TO_CCCC}
BINARY_DIR="$(cd "$(dirname "$BINARY")" && pwd)"
BINARY="$BINARY_DIR/$(basename "$BINARY")"
test -x "$BINARY"

VERSION="$(sed -n 's/^version = "\([^"]*\)"/\1/p' "$ROOT_DIR/Cargo.toml" | head -1)"
test "$("$BINARY" --version)" = "cccc $VERSION"

case "$(uname -s):$(uname -m)" in
  Linux:x86_64|Linux:amd64) target=x86_64-unknown-linux-gnu ;;
  Darwin:x86_64|Darwin:amd64) target=x86_64-apple-darwin ;;
  Darwin:arm64|Darwin:aarch64) target=aarch64-apple-darwin ;;
  *) echo "unsupported test platform" >&2; exit 1 ;;
esac

checksum() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{ print $1 }'
  else
    shasum -a 256 "$1" | awk '{ print $1 }'
  fi
}

TMP_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/cccc-real-install-test.XXXXXX")"
trap 'rm -rf "$TMP_ROOT"' EXIT
package="cccc-v${VERSION}-${target}"
release_dir="$TMP_ROOT/releases/download/v${VERSION}"
mkdir -p "$release_dir" "$TMP_ROOT/package/$package"
cp "$BINARY" "$TMP_ROOT/package/$package/cccc"
tar -C "$TMP_ROOT/package" -czf "$release_dir/$package.tar.gz" "$package"

: > "$release_dir/SHA256SUMS"
for fixture_target in x86_64-unknown-linux-gnu x86_64-apple-darwin aarch64-apple-darwin x86_64-pc-windows-msvc; do
  fixture_name="cccc-v${VERSION}-${fixture_target}"
  fixture_ext=tar.gz
  [[ "$fixture_target" == x86_64-pc-windows-msvc ]] && fixture_ext=zip
  fixture_checksum=$(printf '0%.0s' {1..64})
  if [[ "$fixture_name.$fixture_ext" == "$package.tar.gz" ]]; then
    fixture_checksum=$(checksum "$release_dir/$package.tar.gz")
  fi
  printf '%s  %s.%s\n' "$fixture_checksum" "$fixture_name" "$fixture_ext" >> "$release_dir/SHA256SUMS"
done

CCCC_VERSION="$VERSION" \
CCCC_RELEASE_BASE_URL="file://$TMP_ROOT/releases" \
CCCC_INSTALL_DIR="$TMP_ROOT/installed" \
CCCC_NO_MODIFY_PATH=1 \
sh "$ROOT_DIR/scripts/install.sh"

cmp "$BINARY" "$TMP_ROOT/installed/cccc"
test "$("$TMP_ROOT/installed/cccc" --version)" = "cccc $VERSION"
echo "OK: real Unix cccc installer"
