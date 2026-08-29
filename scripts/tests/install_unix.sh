#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
TMP_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/cccc-install-test.XXXXXX")"
trap 'rm -rf "$TMP_ROOT"' EXIT

case "$(uname -s):$(uname -m)" in
  Linux:x86_64|Linux:amd64)
    target=x86_64-unknown-linux-gnu
    ;;
  Darwin:x86_64|Darwin:amd64)
    target=x86_64-apple-darwin
    ;;
  Darwin:arm64|Darwin:aarch64)
    target=aarch64-apple-darwin
    ;;
  *) echo "unsupported test platform" >&2; exit 1 ;;
esac

checksum() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{ print $1 }'
  else
    shasum -a 256 "$1" | awk '{ print $1 }'
  fi
}

make_release() {
  local version=$1
  local checksum_value=${2:-valid}
  local reported_version=${3:-$version}
  local missing_binary=${4:-}
  local custom_binary=${5:-}
  local package="cccc-v${version}-${target}"
  local release_dir="$TMP_ROOT/releases/download/v${version}"
  mkdir -p "$release_dir" "$TMP_ROOT/package/$package"
  binary=cccc
  if [[ "$binary" != "$missing_binary" ]]; then
    if [[ -n "$custom_binary" ]]; then
      cp "$custom_binary" "$TMP_ROOT/package/$package/$binary"
    else
      printf '#!/usr/bin/env sh\nif [ "${1:-}" = "--version" ]; then printf "cccc %s\\n"; exit 0; fi\nexit 1\n' "$reported_version" > "$TMP_ROOT/package/$package/$binary"
    fi
    chmod 755 "$TMP_ROOT/package/$package/$binary"
  fi
  tar -C "$TMP_ROOT/package" -czf "$release_dir/$package.tar.gz" "$package"
  if [[ "$checksum_value" == valid ]]; then
    checksum_value=$(checksum "$release_dir/$package.tar.gz")
  fi
  : > "$release_dir/SHA256SUMS"
  for fixture_target in x86_64-unknown-linux-gnu x86_64-apple-darwin aarch64-apple-darwin x86_64-pc-windows-msvc; do
    fixture_name="cccc-v${version}-${fixture_target}"
    fixture_ext=tar.gz
    [[ "$fixture_target" == x86_64-pc-windows-msvc ]] && fixture_ext=zip
    fixture_checksum=$(printf '0%.0s' {1..64})
    [[ "$fixture_name.$fixture_ext" == "$package.tar.gz" ]] && fixture_checksum=$checksum_value
    printf '%s  %s.%s\n' "$fixture_checksum" "$fixture_name" "$fixture_ext" >> "$release_dir/SHA256SUMS"
  done
  local wheel_version=$version
  case "$wheel_version" in
    *-alpha[0-9]*) wheel_version=${wheel_version/-alpha/a} ;;
    *-beta[0-9]*) wheel_version=${wheel_version/-beta/b} ;;
    *-rc[0-9]*) wheel_version=${wheel_version/-rc/rc} ;;
    *) wheel_version=${wheel_version//-/} ;;
  esac
  for platform_tag in manylinux_2_28_x86_64 macosx_11_0_x86_64 macosx_11_0_arm64 win_amd64; do
    printf '%s  cccc_pair-%s-py3-none-%s.whl\n' "$(printf '0%.0s' {1..64})" "$wheel_version" "$platform_tag" >> "$release_dir/SHA256SUMS"
  done
  rm -rf "$TMP_ROOT/package"
}

version=0.0.0-test
make_release "$version"

# Older complete releases used a four-archive manifest without native wheels.
# Keep that published shape installable while current release sets add four
# native wheels to the same checksum manifest.
legacy_manifest_version=0.0.12-test
make_release "$legacy_manifest_version"
legacy_manifest="$TMP_ROOT/releases/download/v${legacy_manifest_version}/SHA256SUMS"
grep -v 'cccc_pair-' "$legacy_manifest" > "$legacy_manifest.legacy"
mv "$legacy_manifest.legacy" "$legacy_manifest"
HOME="$TMP_ROOT/legacy-manifest-home" \
  CCCC_VERSION="$legacy_manifest_version" \
  CCCC_RELEASE_BASE_URL="file://$TMP_ROOT/releases" \
  CCCC_NO_MODIFY_PATH=1 \
  sh "$ROOT_DIR/scripts/install.sh"
[[ "$("$TMP_ROOT/legacy-manifest-home/.local/bin/cccc" --version)" == "cccc $legacy_manifest_version" ]]

prerelease_version=0.0.13-rc1
make_release "$prerelease_version"
HOME="$TMP_ROOT/prerelease-home" \
  CCCC_VERSION="$prerelease_version" \
  CCCC_RELEASE_BASE_URL="file://$TMP_ROOT/releases" \
  CCCC_NO_MODIFY_PATH=1 \
  sh "$ROOT_DIR/scripts/install.sh"
[[ "$("$TMP_ROOT/prerelease-home/.local/bin/cccc" --version)" == "cccc $prerelease_version" ]]

version_shaped_install="$TMP_ROOT/version-shaped-foreign-installed"
mkdir -p "$version_shaped_install"
cat > "$version_shaped_install/cccc" <<'EOF'
#!/usr/bin/env sh
if [ "${1:-}" = --version ]; then
  printf 'cccc 1.2.3\n'
  exit 0
fi
if [ "${1:-}" = version ]; then
  printf '1.2.3\n'
  exit 0
fi
exit 1
EOF
chmod 755 "$version_shaped_install/cccc"
version_shaped_hash=$(checksum "$version_shaped_install/cccc")
if HOME="$TMP_ROOT/version-shaped-home" \
  CCCC_VERSION="$version" \
  CCCC_RELEASE_BASE_URL="file://$TMP_ROOT/releases" \
  CCCC_INSTALL_DIR="$version_shaped_install" \
  CCCC_NO_MODIFY_PATH=1 \
  sh "$ROOT_DIR/scripts/install.sh" 2> "$TMP_ROOT/version-shaped-error"; then
  echo "installer inferred ownership from generic version output" >&2
  exit 1
fi
grep -Fq 'managed by another installation; refusing to replace it' "$TMP_ROOT/version-shaped-error"
[[ "$(checksum "$version_shaped_install/cccc")" == "$version_shaped_hash" ]]
test ! -e "$version_shaped_install/.cccc-standalone"

blocking_install="$TMP_ROOT/blocking-markerless-installed"
mkdir -p "$blocking_install"
cat > "$blocking_install/cccc" <<'EOF'
#!/usr/bin/env sh
if [ "${1:-}" = --version ] || [ "${1:-}" = version ]; then
  : > "$CCCC_TEST_PROBE_MARKER"
  sleep 7
  printf 'cccc 1.2.3\n'
  exit 0
fi
exit 1
EOF
chmod 755 "$blocking_install/cccc"
blocking_hash=$(checksum "$blocking_install/cccc")
blocking_started=$SECONDS
if HOME="$TMP_ROOT/blocking-home" \
  CCCC_VERSION="$version" \
  CCCC_RELEASE_BASE_URL="file://$TMP_ROOT/releases" \
  CCCC_INSTALL_DIR="$blocking_install" \
  CCCC_TEST_PROBE_MARKER="$TMP_ROOT/blocking-probe-invoked" \
  CCCC_NO_MODIFY_PATH=1 \
  sh "$ROOT_DIR/scripts/install.sh" 2> "$TMP_ROOT/blocking-error"; then
  echo "installer replaced a blocking markerless command" >&2
  exit 1
fi
(( SECONDS - blocking_started < 5 ))
test ! -e "$TMP_ROOT/blocking-probe-invoked"
grep -Fq 'managed by another installation; refusing to replace it' "$TMP_ROOT/blocking-error"
[[ "$(checksum "$blocking_install/cccc")" == "$blocking_hash" ]]

HOME="$TMP_ROOT/version-shaped-home" \
CCCC_VERSION="$version" \
CCCC_RELEASE_BASE_URL="file://$TMP_ROOT/releases" \
CCCC_INSTALL_DIR="$version_shaped_install" \
CCCC_NO_MODIFY_PATH=1 \
CCCC_ALLOW_REPLACE_EXISTING=1 \
sh "$ROOT_DIR/scripts/install.sh"
[[ "$("$version_shaped_install/cccc" --version)" == "cccc $version" ]]
grep -Fxq 'standalone-v1' "$version_shaped_install/.cccc-standalone"

markerless_foreign_install="$TMP_ROOT/markerless-foreign-installed"
mkdir -p "$markerless_foreign_install"
cat > "$markerless_foreign_install/cccc" <<'EOF'
#!/usr/bin/env sh
if [ "${1:-}" = --version ] || [ "${1:-}" = version ]; then
  printf 'not CCCC\n'
  exit 0
fi
exit 1
EOF
chmod 755 "$markerless_foreign_install/cccc"
markerless_foreign_hash=$(checksum "$markerless_foreign_install/cccc")
if HOME="$TMP_ROOT/markerless-foreign-home" \
  CCCC_VERSION="$version" \
  CCCC_RELEASE_BASE_URL="file://$TMP_ROOT/releases" \
  CCCC_INSTALL_DIR="$markerless_foreign_install" \
  CCCC_NO_MODIFY_PATH=1 \
  sh "$ROOT_DIR/scripts/install.sh" 2> "$TMP_ROOT/markerless-foreign-error"; then
  echo "installer replaced an unrecognized markerless command" >&2
  exit 1
fi
grep -Fq 'managed by another installation; refusing to replace it' "$TMP_ROOT/markerless-foreign-error"
[[ "$(checksum "$markerless_foreign_install/cccc")" == "$markerless_foreign_hash" ]]
if HOME="$TMP_ROOT/markerless-foreign-home" \
  CCCC_VERSION="$version" \
  CCCC_RELEASE_BASE_URL="file://$TMP_ROOT/releases" \
  CCCC_INSTALL_DIR="$markerless_foreign_install" \
  CCCC_TRUSTED_EXISTING_CLI="$TMP_ROOT/not-the-install-target/cccc" \
  CCCC_NO_MODIFY_PATH=1 \
  sh "$ROOT_DIR/scripts/install.sh" 2> "$TMP_ROOT/trusted-mismatch-error"; then
  echo "installer trusted a self-update path that did not match the install target" >&2
  exit 1
fi
grep -Fq 'managed by another installation; refusing to replace it' "$TMP_ROOT/trusted-mismatch-error"
[[ "$(checksum "$markerless_foreign_install/cccc")" == "$markerless_foreign_hash" ]]

trusted_install="$TMP_ROOT/trusted-self-update"
mkdir -p "$trusted_install"
cp "$markerless_foreign_install/cccc" "$trusted_install/cccc"
trusted_output=$(HOME="$TMP_ROOT/trusted-home" \
  CCCC_VERSION="$version" \
  CCCC_RELEASE_BASE_URL="file://$TMP_ROOT/releases" \
  CCCC_INSTALL_DIR="$trusted_install" \
  CCCC_TRUSTED_EXISTING_CLI="$trusted_install/cccc" \
  CCCC_NO_MODIFY_PATH=1 \
  sh "$ROOT_DIR/scripts/install.sh")
printf '%s\n' "$trusted_output" | grep -Fq "Adopting existing CCCC command at $trusted_install/cccc"
[[ "$("$trusted_install/cccc" --version)" == "cccc $version" ]]
grep -Fxq 'standalone-v1' "$trusted_install/.cccc-standalone"

pip_install="$TMP_ROOT/pip-owned-installed"
mkdir -p "$pip_install"
cat > "$pip_install/cccc" <<'EOF'
#!/usr/bin/env sh
exit 1
EOF
chmod 755 "$pip_install/cccc"
printf 'pip-v1\n' > "$pip_install/.cccc-standalone"
pip_hash=$(checksum "$pip_install/cccc")
if HOME="$TMP_ROOT/pip-owned-home" \
  CCCC_VERSION="$version" \
  CCCC_RELEASE_BASE_URL="file://$TMP_ROOT/releases" \
  CCCC_INSTALL_DIR="$pip_install" \
  CCCC_NO_MODIFY_PATH=1 \
  CCCC_ALLOW_REPLACE_EXISTING=1 \
  sh "$ROOT_DIR/scripts/install.sh" 2> "$TMP_ROOT/pip-owned-error"; then
  echo "installer replaced a pip-owned command" >&2
  exit 1
fi
grep -Fq 'managed by pip; run python -m pip uninstall cccc-pair' "$TMP_ROOT/pip-owned-error"
[[ "$(checksum "$pip_install/cccc")" == "$pip_hash" ]]
grep -Fxq 'pip-v1' "$pip_install/.cccc-standalone"

foreign_install="$TMP_ROOT/foreign-installed"
mkdir -p "$foreign_install"
cat > "$foreign_install/cccc" <<'EOF'
#!/usr/bin/env sh
exit 1
EOF
chmod 755 "$foreign_install/cccc"
printf 'foreign-v1\n' > "$foreign_install/.cccc-standalone"
foreign_hash=$(checksum "$foreign_install/cccc")
if HOME="$TMP_ROOT/foreign-home" \
  CCCC_VERSION="$version" \
  CCCC_RELEASE_BASE_URL="file://$TMP_ROOT/releases" \
  CCCC_INSTALL_DIR="$foreign_install" \
  CCCC_NO_MODIFY_PATH=1 \
  sh "$ROOT_DIR/scripts/install.sh" 2> "$TMP_ROOT/foreign-error"; then
  echo "installer replaced a command owned by another installation" >&2
  exit 1
fi
grep -Fq 'managed by another installation; refusing to replace it' "$TMP_ROOT/foreign-error"
[[ "$(checksum "$foreign_install/cccc")" == "$foreign_hash" ]]
grep -Fxq 'foreign-v1' "$foreign_install/.cccc-standalone"

printf 'foreign-v1\nstandalone-v1\n' > "$foreign_install/.cccc-standalone"
malformed_marker_hash=$(checksum "$foreign_install/.cccc-standalone")
if HOME="$TMP_ROOT/foreign-home" \
  CCCC_VERSION="$version" \
  CCCC_RELEASE_BASE_URL="file://$TMP_ROOT/releases" \
  CCCC_INSTALL_DIR="$foreign_install" \
  CCCC_NO_MODIFY_PATH=1 \
  sh "$ROOT_DIR/scripts/install.sh" 2> "$TMP_ROOT/malformed-marker-error"; then
  echo "installer accepted a malformed standalone ownership marker" >&2
  exit 1
fi
grep -Fq 'managed by another installation; refusing to replace it' "$TMP_ROOT/malformed-marker-error"
[[ "$(checksum "$foreign_install/cccc")" == "$foreign_hash" ]]
[[ "$(checksum "$foreign_install/.cccc-standalone")" == "$malformed_marker_hash" ]]

HOME="$TMP_ROOT/foreign-home" \
CCCC_VERSION="$version" \
CCCC_RELEASE_BASE_URL="file://$TMP_ROOT/releases" \
CCCC_INSTALL_DIR="$foreign_install" \
CCCC_NO_MODIFY_PATH=1 \
CCCC_ALLOW_REPLACE_EXISTING=1 \
sh "$ROOT_DIR/scripts/install.sh"
[[ "$("$foreign_install/cccc" --version)" == "cccc $version" ]]
grep -Fxq 'standalone-v1' "$foreign_install/.cccc-standalone"

readonly_marker_version=0.0.10-test
readonly_marker_state="$TMP_ROOT/readonly-marker-daemon-state"
readonly_marker_release="$TMP_ROOT/readonly-marker-release-cccc"
cat > "$readonly_marker_release" <<EOF
#!/usr/bin/env sh
if [ "\${1:-}" = "--version" ]; then
  printf 'cccc $readonly_marker_version\\n'
  exit 0
fi
if [ "\${1:-}" = daemon ] && [ "\${2:-}" = start ]; then
  printf 'new-running\\n' > "\$CCCC_TEST_DAEMON_STATE"
  exit 0
fi
exit 1
EOF
chmod 755 "$readonly_marker_release"
make_release "$readonly_marker_version" valid "$readonly_marker_version" "" "$readonly_marker_release"
readonly_marker_install="$TMP_ROOT/readonly-marker-installed"
readonly_marker_old="$TMP_ROOT/readonly-marker-old-cccc"
mkdir -p "$readonly_marker_install"
printf 'old-running\n' > "$readonly_marker_state"
cat > "$readonly_marker_old" <<'EOF'
#!/usr/bin/env sh
if [ "${1:-}" = daemon ] && [ "${2:-}" = status ]; then
  [ "$(cat "$CCCC_TEST_DAEMON_STATE")" = old-running ]
  exit
fi
if [ "${1:-}" = daemon ] && [ "${2:-}" = stop ]; then
  printf 'stopped\n' > "$CCCC_TEST_DAEMON_STATE"
  exit 0
fi
if [ "${1:-}" = daemon ] && [ "${2:-}" = start ]; then
  [ "$(cat "$CCCC_TEST_DAEMON_STATE")" != new-running ] || exit 1
  printf 'old-running\n' > "$CCCC_TEST_DAEMON_STATE"
  exit 0
fi
exit 1
EOF
chmod 755 "$readonly_marker_old"
cp "$readonly_marker_old" "$readonly_marker_install/cccc"
printf 'foreign-v1\n' > "$readonly_marker_install/.cccc-standalone"
chmod 444 "$readonly_marker_install/.cccc-standalone"
HOME="$TMP_ROOT/readonly-marker-home" \
CCCC_VERSION="$readonly_marker_version" \
CCCC_RELEASE_BASE_URL="file://$TMP_ROOT/releases" \
CCCC_INSTALL_DIR="$readonly_marker_install" \
CCCC_NO_MODIFY_PATH=1 \
CCCC_ALLOW_REPLACE_EXISTING=1 \
CCCC_TEST_DAEMON_STATE="$readonly_marker_state" \
sh "$ROOT_DIR/scripts/install.sh"
[[ "$("$readonly_marker_install/cccc" --version)" == "cccc $readonly_marker_version" ]]
grep -Fxq 'standalone-v1' "$readonly_marker_install/.cccc-standalone"
[[ "$(cat "$readonly_marker_state")" == new-running ]]

marker_rollback_version=0.0.11-test
make_release "$marker_rollback_version"
marker_rollback_install="$TMP_ROOT/marker-rollback-installed"
marker_rollback_state="$TMP_ROOT/marker-rollback-daemon-state"
mkdir -p "$marker_rollback_install"
cp "$readonly_marker_old" "$marker_rollback_install/cccc"
printf 'old-running\n' > "$marker_rollback_state"
printf 'foreign-v1\n' > "$marker_rollback_install/.cccc-standalone"
marker_rollback_hash=$(checksum "$marker_rollback_install/cccc")
if HOME="$TMP_ROOT/marker-rollback-home" \
  CCCC_VERSION="$marker_rollback_version" \
  CCCC_RELEASE_BASE_URL="file://$TMP_ROOT/releases" \
  CCCC_INSTALL_DIR="$marker_rollback_install" \
  CCCC_NO_MODIFY_PATH=1 \
  CCCC_ALLOW_REPLACE_EXISTING=1 \
  CCCC_TEST_DAEMON_STATE="$marker_rollback_state" \
  sh "$ROOT_DIR/scripts/install.sh" 2> "$TMP_ROOT/marker-rollback-error"; then
  echo "installer accepted a replacement whose daemon could not restart" >&2
  exit 1
fi
grep -Fq 'updated CCCC daemon could not restart' "$TMP_ROOT/marker-rollback-error"
[[ "$(checksum "$marker_rollback_install/cccc")" == "$marker_rollback_hash" ]]
grep -Fxq 'foreign-v1' "$marker_rollback_install/.cccc-standalone"
[[ "$(cat "$marker_rollback_state")" == old-running ]]

sed "s/@CCCC_VERSION@/$version/g" "$ROOT_DIR/scripts/install.sh" > "$TMP_ROOT/versioned-install.sh"
HOME="$TMP_ROOT/versioned-home" \
SHELL=/bin/zsh \
CCCC_RELEASE_BASE_URL="file://$TMP_ROOT/releases" \
sh "$TMP_ROOT/versioned-install.sh"
[[ "$("$TMP_ROOT/versioned-home/.local/bin/cccc" --version)" == "cccc $version" ]]
grep -Fxq 'standalone-v1' "$TMP_ROOT/versioned-home/.local/bin/.cccc-standalone"

mkdir -p "$TMP_ROOT/home with space"
cat > "$TMP_ROOT/home with space/.profile" <<'EOF'
# existing login profile

# CCCC
case ":$PATH:" in *":$HOME/.local/bin:"*) ;; *) export PATH="$HOME/.local/bin:$PATH" ;; esac
EOF
shadow_bin="$TMP_ROOT/older-cccc/bin"
mkdir -p "$shadow_bin"
cat > "$shadow_bin/cccc" <<'EOF'
#!/usr/bin/env sh
if [ "${1:-}" = "--version" ]; then printf 'cccc 0.3.0\n'; fi
EOF
chmod 755 "$shadow_bin/cccc"
shadow_hash=$(checksum "$shadow_bin/cccc")
HOME="$TMP_ROOT/home with space" \
SHELL=/bin/bash \
PATH="$shadow_bin:$TMP_ROOT/home with space/.local/bin:$PATH" \
CCCC_VERSION="$version" \
CCCC_RELEASE_BASE_URL="file://$TMP_ROOT/releases" \
sh "$ROOT_DIR/scripts/install.sh" > "$TMP_ROOT/bash-install.out"

test -x "$TMP_ROOT/home with space/.local/bin/cccc"
grep -Fxq 'standalone-v1' "$TMP_ROOT/home with space/.local/bin/.cccc-standalone"
[[ "$("$TMP_ROOT/home with space/.local/bin/cccc" --version)" == "cccc $version" ]]
for bash_profile in .profile .bashrc; do
  test "$(grep -Fc '# CCCC' "$TMP_ROOT/home with space/$bash_profile")" -eq 1
  grep -Fq 'case "$PATH" in "$HOME/.local/bin"|"$HOME/.local/bin:"*)' "$TMP_ROOT/home with space/$bash_profile"
  ! grep -Fq 'case ":$PATH:" in *":$HOME/.local/bin:"*)' "$TMP_ROOT/home with space/$bash_profile"
done
test ! -e "$TMP_ROOT/home with space/.bash_profile"
expected_bash_command="$(cd -P "$TMP_ROOT/home with space/.local/bin" && pwd -P)/cccc"
grep -Fq "✅ CCCC v$version installed successfully!" "$TMP_ROOT/bash-install.out"
grep -Fq "📦 Installed to: $expected_bash_command" "$TMP_ROOT/bash-install.out"
grep -Fq '⚡ Activate now: source ~/.bashrc' "$TMP_ROOT/bash-install.out"
grep -Fq '🔍 Verify:       cccc doctor' "$TMP_ROOT/bash-install.out"
grep -Fq '🎉 Open a new terminal and run: cccc' "$TMP_ROOT/bash-install.out"
grep -Fq 'Other CCCC commands were left unchanged:' "$TMP_ROOT/bash-install.out"
grep -Fq 'older-cccc/bin/cccc' "$TMP_ROOT/bash-install.out"
[[ "$(checksum "$shadow_bin/cccc")" == "$shadow_hash" ]]
HOME="$TMP_ROOT/home with space" \
PATH="$shadow_bin:$TMP_ROOT/home with space/.local/bin:$PATH" \
/bin/bash -c '. "$HOME/.profile"; [ "${PATH%%:*}" = "$HOME/.local/bin" ]'

login_profile_before=$(checksum "$TMP_ROOT/home with space/.profile")
interactive_profile_before=$(checksum "$TMP_ROOT/home with space/.bashrc")
rollback_version=0.0.2-test
make_release "$rollback_version" valid 9.9.9
if HOME="$TMP_ROOT/home with space" \
  SHELL=/bin/bash \
  CCCC_VERSION="$rollback_version" \
  CCCC_RELEASE_BASE_URL="file://$TMP_ROOT/releases" \
  sh "$ROOT_DIR/scripts/install.sh"; then
  echo "installer accepted a mismatched binary version" >&2
  exit 1
fi
[[ "$("$TMP_ROOT/home with space/.local/bin/cccc" --version)" == "cccc $version" ]]
[[ "$(checksum "$TMP_ROOT/home with space/.profile")" == "$login_profile_before" ]]
[[ "$(checksum "$TMP_ROOT/home with space/.bashrc")" == "$interactive_profile_before" ]]

zsh_home="$TMP_ROOT/zsh-home"
HOME="$zsh_home" \
SHELL=/bin/zsh \
CCCC_VERSION="$version" \
CCCC_RELEASE_BASE_URL="file://$TMP_ROOT/releases" \
sh "$ROOT_DIR/scripts/install.sh" > "$TMP_ROOT/zsh-install.out"
for zsh_profile in .zprofile .zshrc; do
  test "$(grep -Fc '# CCCC' "$zsh_home/$zsh_profile")" -eq 1
done
grep -Fq '⚡ Activate now: source ~/.zshrc' "$TMP_ROOT/zsh-install.out"

missing_version=0.0.3-test
make_release "$missing_version" valid "$missing_version" cccc
if HOME="$TMP_ROOT/missing-home" \
  CCCC_VERSION="$missing_version" \
  CCCC_RELEASE_BASE_URL="file://$TMP_ROOT/releases" \
  CCCC_NO_MODIFY_PATH=1 \
  sh "$ROOT_DIR/scripts/install.sh"; then
  echo "installer accepted an archive missing cccc" >&2
  exit 1
fi
test ! -e "$TMP_ROOT/missing-home/.local/bin/cccc"

duplicate_version=0.0.4-test
make_release "$duplicate_version"
duplicate_package="cccc-v${duplicate_version}-${target}"
duplicate_manifest="$TMP_ROOT/releases/download/v${duplicate_version}/SHA256SUMS"
duplicate_line=$(grep -F "  $duplicate_package.tar.gz" "$duplicate_manifest")
printf '%s\n' "$duplicate_line" >> "$duplicate_manifest"
if HOME="$TMP_ROOT/duplicate-home" \
  CCCC_VERSION="$duplicate_version" \
  CCCC_RELEASE_BASE_URL="file://$TMP_ROOT/releases" \
  CCCC_NO_MODIFY_PATH=1 \
  sh "$ROOT_DIR/scripts/install.sh"; then
  echo "installer accepted a duplicate checksum entry" >&2
  exit 1
fi
test ! -e "$TMP_ROOT/duplicate-home/.local/bin/cccc"

unsafe_version=0.0.5-test
make_release "$unsafe_version"
unsafe_package="cccc-v${unsafe_version}-${target}"
unsafe_dir="$TMP_ROOT/releases/download/v${unsafe_version}"
mkdir -p "$TMP_ROOT/unsafe-root"
tar -xzf "$unsafe_dir/$unsafe_package.tar.gz" -C "$TMP_ROOT/unsafe-root"
printf 'outside package\n' > "$TMP_ROOT/unsafe-root/outside.txt"
tar -C "$TMP_ROOT/unsafe-root" -czf "$unsafe_dir/$unsafe_package.tar.gz" "$unsafe_package" outside.txt
unsafe_checksum=$(checksum "$unsafe_dir/$unsafe_package.tar.gz")
awk -v name="$unsafe_package.tar.gz" -v hash="$unsafe_checksum" \
  '$2 == name { $1 = hash } { print }' "$unsafe_dir/SHA256SUMS" > "$unsafe_dir/SHA256SUMS.new"
mv "$unsafe_dir/SHA256SUMS.new" "$unsafe_dir/SHA256SUMS"
if HOME="$TMP_ROOT/unsafe-home" \
  CCCC_VERSION="$unsafe_version" \
  CCCC_RELEASE_BASE_URL="file://$TMP_ROOT/releases" \
  CCCC_NO_MODIFY_PATH=1 \
  sh "$ROOT_DIR/scripts/install.sh"; then
  echo "installer accepted an archive entry outside its package root" >&2
  exit 1
fi
test ! -e "$TMP_ROOT/unsafe-home/.local/bin/cccc"

lock_version=0.0.6-test
make_release "$lock_version"
lock_install="$TMP_ROOT/lock-installed"
mkdir -p "$lock_install/.cccc-install.lock"
printf 'old binary\n' > "$lock_install/cccc"
printf 'standalone-v1\n' > "$lock_install/.cccc-standalone"
lock_hash=$(checksum "$lock_install/cccc")
if HOME="$TMP_ROOT/lock-home" \
  CCCC_VERSION="$lock_version" \
  CCCC_RELEASE_BASE_URL="file://$TMP_ROOT/releases" \
  CCCC_INSTALL_DIR="$lock_install" \
  CCCC_NO_MODIFY_PATH=1 \
  sh "$ROOT_DIR/scripts/install.sh"; then
  echo "installer ignored an existing transaction lock" >&2
  exit 1
fi
[[ "$(checksum "$lock_install/cccc")" == "$lock_hash" ]]
rm -rf "$lock_install/.cccc-install.lock"

stale_version=0.0.8-test
make_release "$stale_version"
failing_version=0.0.9-test
make_release "$failing_version" valid 9.9.9
stale_install="$TMP_ROOT/stale-installed"
stale_signal="$TMP_ROOT/stale-lock-reached"
stale_release="$TMP_ROOT/stale-lock-release"
mkdir -p "$TMP_ROOT/stale-bin"
real_mkdir=$(command -v mkdir)
cat > "$TMP_ROOT/stale-bin/mkdir" <<EOF
#!/usr/bin/env sh
case "\${*}" in
  *'.cccc-install.lock'*)
    : > '$stale_signal'
    while [ ! -e '$stale_release' ]; do sleep 0.01; done
    ;;
esac
exec '$real_mkdir' "\$@"
EOF
chmod 755 "$TMP_ROOT/stale-bin/mkdir"
HOME="$TMP_ROOT/stale-home-b" \
PATH="$TMP_ROOT/stale-bin:$PATH" \
CCCC_VERSION="$failing_version" \
CCCC_RELEASE_BASE_URL="file://$TMP_ROOT/releases" \
CCCC_INSTALL_DIR="$stale_install" \
CCCC_NO_MODIFY_PATH=1 \
sh "$ROOT_DIR/scripts/install.sh" > "$TMP_ROOT/stale-b.out" 2> "$TMP_ROOT/stale-b.err" &
stale_pid=$!
for _ in {1..500}; do
  [[ -e "$stale_signal" ]] && break
  sleep 0.01
done
if [[ ! -e "$stale_signal" ]]; then
  kill "$stale_pid" 2>/dev/null || true
  echo "second installer did not reach its transaction lock" >&2
  exit 1
fi
HOME="$TMP_ROOT/stale-home-a" \
CCCC_VERSION="$stale_version" \
CCCC_RELEASE_BASE_URL="file://$TMP_ROOT/releases" \
CCCC_INSTALL_DIR="$stale_install" \
CCCC_NO_MODIFY_PATH=1 \
sh "$ROOT_DIR/scripts/install.sh"
: > "$stale_release"
if wait "$stale_pid"; then
  echo "stale installer accepted a mismatched binary version" >&2
  exit 1
fi
test -x "$stale_install/cccc"
[[ "$($stale_install/cccc --version)" == "cccc $stale_version" ]]

restart_version=0.0.7-test
make_release "$restart_version" valid 9.9.9
restart_install="$TMP_ROOT/restart-installed"
restart_state="$TMP_ROOT/restart-daemon-state"
mkdir -p "$restart_install"
printf 'running\n' > "$restart_state"
cat > "$restart_install/cccc" <<EOF
#!/usr/bin/env sh
state_file='$restart_state'
if [ "\${1:-}" = daemon ] && [ "\${2:-}" = status ]; then
  [ "\$(cat "\$state_file")" = running ]
  exit
fi
if [ "\${1:-}" = daemon ] && [ "\${2:-}" = stop ]; then
  printf 'stopped\n' > "\$state_file"
  exit 0
fi
if [ "\${1:-}" = daemon ] && [ "\${2:-}" = start ]; then
  exit 1
fi
if [ "\${1:-}" = --version ]; then
  printf 'cccc 0.0.0-old\n'
  exit 0
fi
exit 1
EOF
chmod 755 "$restart_install/cccc"
printf 'standalone-v1\n' > "$restart_install/.cccc-standalone"
restart_hash=$(checksum "$restart_install/cccc")
if HOME="$TMP_ROOT/restart-home" \
  CCCC_VERSION="$restart_version" \
  CCCC_RELEASE_BASE_URL="file://$TMP_ROOT/releases" \
  CCCC_INSTALL_DIR="$restart_install" \
  CCCC_NO_MODIFY_PATH=1 \
  sh "$ROOT_DIR/scripts/install.sh" 2> "$TMP_ROOT/restart-error"; then
  echo "installer accepted a mismatched version during daemon rollback test" >&2
  exit 1
fi
grep -Fq 'rollback restored the previous binary but failed to restart its daemon' "$TMP_ROOT/restart-error"
[[ "$(checksum "$restart_install/cccc")" == "$restart_hash" ]]

mkdir -p "$TMP_ROOT/fake-bin"
cat > "$TMP_ROOT/fake-bin/uname" <<'EOF'
#!/usr/bin/env sh
case "${1:-}" in
  -s) printf 'Linux\n' ;;
  -m) printf 'aarch64\n' ;;
  *) exit 1 ;;
esac
EOF
chmod 755 "$TMP_ROOT/fake-bin/uname"
if HOME="$TMP_ROOT/unsupported-home" \
  PATH="$TMP_ROOT/fake-bin:$PATH" \
  CCCC_VERSION="$version" \
  sh "$ROOT_DIR/scripts/install.sh"; then
  echo "installer accepted unsupported Linux aarch64" >&2
  exit 1
fi

HOME="$TMP_ROOT/home with space" \
SHELL=/bin/bash \
CCCC_VERSION="$version" \
CCCC_RELEASE_BASE_URL="file://$TMP_ROOT/releases" \
sh "$ROOT_DIR/scripts/install.sh"
for bash_profile in .profile .bashrc; do
  test "$(grep -Fc '# CCCC' "$TMP_ROOT/home with space/$bash_profile")" -eq 1
done

bad_version=0.0.1-test
make_release "$bad_version" "$(printf '0%.0s' {1..64})"
if HOME="$TMP_ROOT/bad-home" \
  CCCC_VERSION="$bad_version" \
  CCCC_RELEASE_BASE_URL="file://$TMP_ROOT/releases" \
  CCCC_NO_MODIFY_PATH=1 \
  sh "$ROOT_DIR/scripts/install.sh"; then
  echo "installer accepted a bad checksum" >&2
  exit 1
fi
test ! -e "$TMP_ROOT/bad-home/.local/bin/cccc"

echo "OK: Unix installer"
