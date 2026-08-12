#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
ARTIFACT_DIR="$(cd "${1:?usage: verify_release_unix.sh ARTIFACT_DIR TARGET}" && pwd)"
TARGET=${2:?usage: verify_release_unix.sh ARTIFACT_DIR TARGET}
VERSION="$(sed -n 's/^version = "\([^"]*\)"/\1/p' "$ROOT_DIR/Cargo.toml" | head -1)"

case "$(uname -s):$(uname -m)" in
  Linux:x86_64|Linux:amd64) host_target=x86_64-unknown-linux-gnu ;;
  Darwin:x86_64|Darwin:amd64) host_target=x86_64-apple-darwin ;;
  Darwin:arm64|Darwin:aarch64) host_target=aarch64-apple-darwin ;;
  *) echo "unsupported release verification platform" >&2; exit 1 ;;
esac
test "$TARGET" = "$host_target"

package="cccc-v${VERSION}-${TARGET}"
archive="$ARTIFACT_DIR/$package.tar.gz"
test -f "$archive"
test -f "$ARTIFACT_DIR/SHA256SUMS"
test -f "$ARTIFACT_DIR/install.sh"

TMP_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/cccc-release-verify.XXXXXX")"
installed="$TMP_ROOT/installed/cccc"
web_pid=
cleanup() {
  if [ -x "$installed" ]; then
    CCCC_HOME="$TMP_ROOT/home" "$installed" daemon stop >/dev/null 2>&1 || true
  fi
  if [ -n "$web_pid" ] && kill -0 "$web_pid" >/dev/null 2>&1; then
    kill "$web_pid" >/dev/null 2>&1 || true
    wait "$web_pid" >/dev/null 2>&1 || true
  fi
  rm -rf "$TMP_ROOT"
}
trap cleanup EXIT

mkdir -p "$TMP_ROOT/extracted"
tar -xzf "$archive" -C "$TMP_ROOT/extracted"
package_dir="$TMP_ROOT/extracted/$package"
test -x "$package_dir/cccc"
for helper in ccccd cccc-mcp cccc-web; do
  test ! -e "$package_dir/$helper"
done
executable_count=$(find "$package_dir" -maxdepth 1 -type f -perm -111 | wc -l | tr -d ' ')
test "$executable_count" = 1

release_dir="$TMP_ROOT/releases/download/v$VERSION"
mkdir -p "$release_dir"
cp "$ARTIFACT_DIR"/* "$release_dir/"

CCCC_VERSION="$VERSION" \
CCCC_RELEASE_BASE_URL="file://$TMP_ROOT/releases" \
CCCC_INSTALL_DIR="$TMP_ROOT/installed" \
CCCC_NO_MODIFY_PATH=1 \
sh "$ARTIFACT_DIR/install.sh"

test -x "$installed"
test "$(find "$TMP_ROOT/installed" -maxdepth 1 -type f | wc -l | tr -d ' ')" = 2
grep -Fxq 'standalone-v1' "$TMP_ROOT/installed/.cccc-standalone"
cmp "$package_dir/cccc" "$installed"
test "$("$installed" --version)" = "cccc $VERSION"
update_check=$("$installed" update --check)
printf '%s\n' "$update_check" | grep -Fx "Current version: $VERSION"
printf '%s\n' "$update_check" | grep -Fx "Install directory: $TMP_ROOT/installed"

export CCCC_HOME="$TMP_ROOT/home"
"$installed" --port 0 >"$TMP_ROOT/cccc-web.log" 2>&1 &
web_pid=$!
deadline=$((SECONDS + 30))
until "$installed" daemon status >/dev/null 2>&1; do
  if ! kill -0 "$web_pid" >/dev/null 2>&1; then
    sed -n '1,200p' "$TMP_ROOT/cccc-web.log" >&2
    echo "combined CCCC exited before daemon startup" >&2
    exit 1
  fi
  [ "$SECONDS" -lt "$deadline" ] || {
    echo "combined CCCC daemon did not start in time" >&2
    exit 1
  }
  sleep 0.1
done

# Reinstalling an owned standalone while its daemon is running is the same
# lifecycle exercised by `cccc update`: stop fully, replace, then restart.
CCCC_VERSION="$VERSION" \
CCCC_RELEASE_BASE_URL="file://$TMP_ROOT/releases" \
CCCC_INSTALL_DIR="$TMP_ROOT/installed" \
CCCC_NO_MODIFY_PATH=1 \
sh "$ARTIFACT_DIR/install.sh"
deadline=$((SECONDS + 10))
while kill -0 "$web_pid" >/dev/null 2>&1 && [ "$SECONDS" -lt "$deadline" ]; do
  sleep 0.1
done
if kill -0 "$web_pid" >/dev/null 2>&1; then
  echo "running-daemon reinstall returned before the combined Web process exited" >&2
  exit 1
fi
wait "$web_pid"
web_pid=
"$installed" daemon status
"$installed" daemon stop

address="$CCCC_HOME/daemon/ccccd.addr.json"
deadline=$((SECONDS + 10))
while [ -e "$address" ] && [ "$SECONDS" -lt "$deadline" ]; do
  sleep 0.1
done
test ! -e "$address"

bash "$ROOT_DIR/scripts/tests/smoke_rust_replacement.sh" "$installed"

echo "OK: verified $package release archive and installed self-launch"
