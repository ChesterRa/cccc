#!/usr/bin/env sh
set -eu

DEFAULT_VERSION="@CCCC_VERSION@"
RELEASE_TAG_PREFIX="${CCCC_RELEASE_TAG_PREFIX:-@CCCC_RELEASE_TAG_PREFIX@}"
REPOSITORY="${CCCC_GITHUB_REPOSITORY:-ChesterRa/cccc}"
RELEASE_BASE_URL="${CCCC_RELEASE_BASE_URL:-https://github.com/$REPOSITORY/releases}"
VERSION="${CCCC_VERSION:-}"
INSTALL_DIR="${CCCC_INSTALL_DIR:-$HOME/.local/bin}"
NO_MODIFY_PATH="${CCCC_NO_MODIFY_PATH:-0}"
ALLOW_REPLACE_EXISTING="${CCCC_ALLOW_REPLACE_EXISTING:-0}"
BINARIES="cccc"
INSTALL_MARKER=".cccc-standalone"
INSTALL_MARKER_VERSION="standalone-v1"

case "$RELEASE_TAG_PREFIX" in
  @*) RELEASE_TAG_PREFIX=v ;;
esac

fail() {
  printf 'CCCC installer: %s\n' "$*" >&2
  exit 1
}

need() {
  command -v "$1" >/dev/null 2>&1 || fail "required command not found: $1"
}

need curl
need tar
need awk
need grep
need mktemp

canonical_command_path() {
  command_path=$1
  command_dir=${command_path%/*}
  command_name=${command_path##*/}
  [ "$command_dir" != "$command_path" ] || command_dir=.
  if canonical_dir=$(CDPATH= cd -P "$command_dir" 2>/dev/null && pwd -P); then
    printf '%s/%s\n' "$canonical_dir" "$command_name"
  else
    printf '%s\n' "$command_path"
  fi
}

list_cccc_commands() {
  remaining_path=${PATH:-}
  while :; do
    case "$remaining_path" in
      *:*)
        path_directory=${remaining_path%%:*}
        remaining_path=${remaining_path#*:}
        path_done=0
        ;;
      *)
        path_directory=$remaining_path
        path_done=1
        ;;
    esac
    [ -n "$path_directory" ] || path_directory=.
    candidate="$path_directory/cccc"
    if [ -f "$candidate" ] && [ -x "$candidate" ]; then
      canonical_command_path "$candidate"
    fi
    [ "$path_done" -eq 0 ] || break
  done | awk '!seen[$0]++'
}

download() {
  url=$1
  destination=$2
  if [ -n "${CCCC_RELEASE_BASE_URL:-}" ]; then
    curl -fsSL --retry 3 --retry-delay 1 -o "$destination" "$url"
    return
  fi
  effective_url=$(curl -fsSL --retry 3 --retry-delay 1 \
    --proto '=https' --proto-redir '=https' -o "$destination" -w '%{url_effective}' "$url")
  case "$effective_url" in
    https://github.com/*|https://*.githubusercontent.com/*) ;;
    *) fail "release asset redirected outside GitHub HTTPS: $effective_url" ;;
  esac
}

os=$(uname -s)
arch=$(uname -m)
case "$os:$arch" in
  Linux:x86_64|Linux:amd64)
    if command -v getconf >/dev/null 2>&1 && getconf GNU_LIBC_VERSION >/dev/null 2>&1; then
      :
    elif command -v ldd >/dev/null 2>&1 && ldd --version 2>&1 | grep -Eqi 'glibc|GNU libc'; then
      :
    else
      fail "Linux x86_64 requires glibc; musl/Alpine is not supported"
    fi
    target=x86_64-unknown-linux-gnu
    ;;
  Darwin:x86_64|Darwin:amd64) target=x86_64-apple-darwin ;;
  Darwin:arm64|Darwin:aarch64) target=aarch64-apple-darwin ;;
  *) fail "unsupported platform: $os $arch" ;;
esac

RELEASE_BASE_URL=${RELEASE_BASE_URL%/}
if [ -z "$VERSION" ] && printf '%s\n' "$DEFAULT_VERSION" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+'; then
  VERSION=$DEFAULT_VERSION
fi
if [ -z "$VERSION" ]; then
  latest_url=$(curl -fsSL -o /dev/null -w '%{url_effective}' "$RELEASE_BASE_URL/latest") ||
    fail "could not resolve the latest release"
  if [ -z "${CCCC_RELEASE_BASE_URL:-}" ]; then
    expected_prefix="https://github.com/$REPOSITORY/releases/tag/v"
    case "$latest_url" in
      "$expected_prefix"*) ;;
      *) fail "latest release redirected outside $expected_prefix" ;;
    esac
  fi
  tag=${latest_url##*/}
  case "$tag" in
    v*) VERSION=${tag#v} ;;
    *) fail "latest release did not resolve to a v-prefixed tag: $latest_url" ;;
  esac
else
  VERSION=${VERSION#v}
fi

if ! printf '%s\n' "$VERSION" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z]+([.-][0-9A-Za-z]+)*)?(\+[0-9A-Za-z]+([.-][0-9A-Za-z]+)*)?$'; then
  fail "invalid semantic version: $VERSION"
fi

package="cccc-v${VERSION}-${target}"
archive="${package}.tar.gz"
download_url="$RELEASE_BASE_URL/download/${RELEASE_TAG_PREFIX}${VERSION}"
tmp_dir=$(mktemp -d "${TMPDIR:-/tmp}/cccc-install.XXXXXX")
stage_suffix=".cccc-install.$$"
backup_dir="$INSTALL_DIR/.cccc-backup.$$"
install_lock="$INSTALL_DIR/.cccc-install.lock"
originals="$tmp_dir/originals"
lock_acquired=0
transaction_started=0
transaction_committed=0
daemon_was_running=0
marker_touched=0
marker_original=0
: > "$originals"

rollback_install() {
  [ "$transaction_started" -eq 1 ] || return 0
  for binary in $BINARIES; do
    destination="$INSTALL_DIR/$binary"
    if grep -Fqx "$binary" "$originals"; then
      if [ -f "$backup_dir/$binary" ]; then
        if ! rm -f "$destination" || ! mv "$backup_dir/$binary" "$destination"; then
          printf 'CCCC installer: rollback failed to restore %s\n' "$destination" >&2
        fi
      fi
    else
      rm -f "$destination" || printf 'CCCC installer: rollback failed to remove %s\n' "$destination" >&2
    fi
  done
  if [ "$marker_touched" -eq 1 ]; then
    rm -f "$marker_path" || printf 'CCCC installer: rollback failed to remove %s\n' "$marker_path" >&2
    if [ "$marker_original" -eq 1 ] &&
      ! mv "$backup_dir/$INSTALL_MARKER" "$marker_path"; then
      printf 'CCCC installer: rollback failed to restore %s\n' "$marker_path" >&2
    fi
  fi
  if [ "$daemon_was_running" -eq 1 ] && [ -x "$INSTALL_DIR/cccc" ]; then
    if ! "$INSTALL_DIR/cccc" daemon start >/dev/null 2>&1; then
      printf 'CCCC installer: rollback restored the previous binary but failed to restart its daemon\n' >&2
    fi
  fi
}

cleanup() {
  if [ "$transaction_committed" -eq 0 ]; then
    rollback_install
  fi
  rm -rf "$tmp_dir"
  for binary in $BINARIES; do
    rm -f "$INSTALL_DIR/$binary$stage_suffix"
  done
  rm -f "$INSTALL_DIR/$INSTALL_MARKER$stage_suffix"
  if [ "$transaction_committed" -eq 1 ]; then
    rm -rf "$backup_dir"
  fi
  if [ "$lock_acquired" -eq 1 ]; then
    rm -rf "$install_lock"
  fi
}
trap cleanup EXIT
trap 'exit 129' HUP
trap 'exit 130' INT
trap 'exit 143' TERM

printf 'Downloading CCCC v%s for %s...\n' "$VERSION" "$target"
download "$download_url/SHA256SUMS" "$tmp_dir/SHA256SUMS"
if ! awk -v version="$VERSION" '
  BEGIN {
    valid["cccc-v" version "-x86_64-unknown-linux-gnu.tar.gz"] = 1
    valid["cccc-v" version "-x86_64-apple-darwin.tar.gz"] = 1
    valid["cccc-v" version "-aarch64-apple-darwin.tar.gz"] = 1
    valid["cccc-v" version "-x86_64-pc-windows-msvc.zip"] = 1
  }
  NF == 0 { next }
  NF != 2 || length($1) != 64 || $1 ~ /[^0-9A-Fa-f]/ { exit 1 }
  {
    name = $2
    sub(/^\*/, "", name)
    if (!(name in valid) || seen[name]++) { exit 1 }
    count++
  }
  END { if (count != 4) exit 1 }
' "$tmp_dir/SHA256SUMS"; then
  fail "SHA256SUMS must contain four unique, well-formed archive entries"
fi

expected=$(awk -v name="$archive" '$2 == name || $2 == "*" name { print $1 }' "$tmp_dir/SHA256SUMS")
[ "$(printf '%s\n' "$expected" | awk 'NF { count++ } END { print count + 0 }')" -eq 1 ] ||
  fail "SHA256SUMS must contain exactly one entry for $archive"

download "$download_url/$archive" "$tmp_dir/$archive"
if command -v sha256sum >/dev/null 2>&1; then
  actual=$(sha256sum "$tmp_dir/$archive" | awk '{ print $1 }')
elif command -v shasum >/dev/null 2>&1; then
  actual=$(shasum -a 256 "$tmp_dir/$archive" | awk '{ print $1 }')
elif command -v openssl >/dev/null 2>&1; then
  actual=$(openssl dgst -sha256 "$tmp_dir/$archive" | awk '{ print $NF }')
else
  fail "sha256sum, shasum, or openssl is required to verify the download"
fi

expected=$(printf '%s' "$expected" | tr 'A-F' 'a-f')
actual=$(printf '%s' "$actual" | tr 'A-F' 'a-f')
[ "$actual" = "$expected" ] || fail "checksum mismatch for $archive"

tar -tzf "$tmp_dir/$archive" > "$tmp_dir/archive-entries"
while IFS= read -r entry; do
  case "$entry" in
    /*|../*|*/../*|*/..) fail "archive contains an unsafe path: $entry" ;;
  esac
  case "$entry" in
    "$package"|"$package/"|"$package/"*) ;;
    *) fail "archive entry is outside $package: $entry" ;;
  esac
done < "$tmp_dir/archive-entries"
if tar -tvzf "$tmp_dir/$archive" | grep -Ev '^[-d]' >/dev/null; then
  fail "archive contains an unsupported entry type"
fi
tar -xzf "$tmp_dir/$archive" -C "$tmp_dir"

package_dir="$tmp_dir/$package"
[ -d "$package_dir" ] && [ ! -L "$package_dir" ] || fail "archive is missing its package directory"
mkdir -p "$INSTALL_DIR"
for binary in $BINARIES; do
  source_path="$package_dir/$binary"
  [ -f "$source_path" ] && [ ! -L "$source_path" ] || fail "archive is missing $binary"
  cp "$source_path" "$INSTALL_DIR/$binary$stage_suffix"
  chmod 755 "$INSTALL_DIR/$binary$stage_suffix"
done

if ! mkdir "$install_lock" 2>/dev/null; then
  fail "another installation is using $INSTALL_DIR (lock: $install_lock)"
fi
lock_acquired=1
printf '%s\n' "$$" > "$install_lock/pid"
existing_cli="$INSTALL_DIR/cccc"
marker_owned=0
marker_path="$INSTALL_DIR/$INSTALL_MARKER"
if [ -e "$marker_path" ] || [ -L "$marker_path" ]; then
  [ -f "$marker_path" ] && [ ! -L "$marker_path" ] ||
    fail "existing standalone ownership marker is not a regular file: $marker_path"
  marker_value=
  if marker_value=$(cat "$marker_path" 2>/dev/null) &&
    [ "$marker_value" = "$INSTALL_MARKER_VERSION" ]; then
    marker_owned=1
  fi
fi
if { [ -e "$existing_cli" ] || [ -L "$existing_cli" ]; } &&
  [ "$marker_owned" -ne 1 ] && [ "$ALLOW_REPLACE_EXISTING" != "1" ]; then
  fail "existing $existing_cli is managed by another installation; refusing to replace it. Remove it, choose a different CCCC_INSTALL_DIR, or set CCCC_ALLOW_REPLACE_EXISTING=1 to replace it deliberately"
fi
for binary in $BINARIES; do
  if [ -e "$INSTALL_DIR/$binary" ]; then
    printf '%s\n' "$binary" >> "$originals"
  fi
done
mkdir "$backup_dir"
transaction_started=1
if [ -x "$INSTALL_DIR/cccc" ] && "$INSTALL_DIR/cccc" daemon status >/dev/null 2>&1; then
  daemon_was_running=1
  "$INSTALL_DIR/cccc" daemon stop >/dev/null || fail "could not stop the running CCCC daemon"
  attempts=0
  while "$INSTALL_DIR/cccc" daemon status >/dev/null 2>&1; do
    attempts=$((attempts + 1))
    [ "$attempts" -lt 40 ] || fail "the running CCCC daemon did not stop in time"
    sleep 0.25
  done
fi

for binary in $BINARIES; do
  if grep -Fqx "$binary" "$originals"; then
    mv "$INSTALL_DIR/$binary" "$backup_dir/$binary"
  fi
done
for binary in $BINARIES; do
  mv "$INSTALL_DIR/$binary$stage_suffix" "$INSTALL_DIR/$binary"
done

installed_version=$("$INSTALL_DIR/cccc" --version) || fail "installed cccc failed its version check"
[ "$installed_version" = "cccc $VERSION" ] ||
  fail "installed version mismatch: expected cccc $VERSION, got $installed_version"
if [ -e "$marker_path" ] || [ -L "$marker_path" ]; then
  mv "$marker_path" "$backup_dir/$INSTALL_MARKER"
  marker_original=1
fi
marker_touched=1
marker_stage="$INSTALL_DIR/$INSTALL_MARKER$stage_suffix"
printf '%s\n' "$INSTALL_MARKER_VERSION" > "$marker_stage"
mv "$marker_stage" "$marker_path"
if [ "$daemon_was_running" -eq 1 ]; then
  "$INSTALL_DIR/cccc" daemon start >/dev/null || fail "the updated CCCC daemon could not restart"
fi
transaction_committed=1
rm -rf "$backup_dir"

add_path_profile() {
  profile_path=$1
  touch "$profile_path"
  if grep -Fqx "$path_line" "$profile_path"; then
    :
  elif grep -Fqx "$legacy_path_line" "$profile_path"; then
    replacement="$profile_path.cccc-install.$$"
    if ! awk -v old="$legacy_path_line" -v new="$path_line" \
      '{ if ($0 == old) print new; else print }' "$profile_path" > "$replacement" ||
      ! mv "$replacement" "$profile_path"; then
      rm -f "$replacement"
      fail "could not update CCCC PATH entry in $profile_path"
    fi
  else
    printf '\n# CCCC\n%s\n' "$path_line" >> "$profile_path"
  fi
  printf 'Ensured %s is first on PATH in %s.\n' "$INSTALL_DIR" "$profile_path"
}

bash_login_profile() {
  if [ -e "$HOME/.bash_profile" ]; then
    printf '%s\n' "$HOME/.bash_profile"
  elif [ -e "$HOME/.bash_login" ]; then
    printf '%s\n' "$HOME/.bash_login"
  else
    printf '%s\n' "$HOME/.profile"
  fi
}

activation_hint=
if [ "$NO_MODIFY_PATH" != "1" ] && [ "$INSTALL_DIR" = "$HOME/.local/bin" ]; then
  legacy_path_line='case ":$PATH:" in *":$HOME/.local/bin:"*) ;; *) export PATH="$HOME/.local/bin:$PATH" ;; esac'
  path_line='case "$PATH" in "$HOME/.local/bin"|"$HOME/.local/bin:"*) ;; *) export PATH="$HOME/.local/bin:$PATH" ;; esac'
  case "${SHELL:-}" in
    */zsh)
      add_path_profile "$HOME/.zprofile"
      add_path_profile "$HOME/.zshrc"
      activation_hint='source ~/.zshrc'
      ;;
    */bash)
      add_path_profile "$(bash_login_profile)"
      add_path_profile "$HOME/.bashrc"
      activation_hint='source ~/.bashrc'
      ;;
    *) printf 'Move %s to the front of PATH, then open a new terminal.\n' "$INSTALL_DIR" ;;
  esac
else
  printf 'Move %s to the front of PATH, then open a new terminal.\n' "$INSTALL_DIR"
fi

installed_command=$(canonical_command_path "$INSTALL_DIR/cccc")
list_cccc_commands > "$tmp_dir/path-commands"
other_commands=0
while IFS= read -r command_path; do
  [ -n "$command_path" ] || continue
  if [ "$command_path" != "$installed_command" ]; then
    if [ "$other_commands" -eq 0 ]; then
      printf 'Other CCCC commands were left unchanged:\n'
    fi
    printf '  - %s\n' "$command_path"
    other_commands=$((other_commands + 1))
  fi
done < "$tmp_dir/path-commands"

PATH="$INSTALL_DIR${PATH:+:$PATH}"
export PATH
resolved_command=$(command -v cccc 2>/dev/null || :)
resolved_command=$(canonical_command_path "$resolved_command")
[ "$resolved_command" = "$installed_command" ] ||
  fail "PATH verification resolved cccc to $resolved_command instead of $installed_command"

printf 'Installed CCCC v%s in %s\n' "$VERSION" "$INSTALL_DIR"
printf 'Verify installed command directly: "%s/cccc" doctor\n' "$INSTALL_DIR"
if [ -n "$activation_hint" ]; then
  printf 'Activate in this shell: %s\n' "$activation_hint"
elif [ "$INSTALL_DIR" = "$HOME/.local/bin" ]; then
  printf 'Activate in this shell: export PATH="$HOME/.local/bin:$PATH"; hash -r\n'
fi
printf 'Verify after opening a new terminal: cccc doctor\n'
