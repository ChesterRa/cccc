#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
TMP_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/cccc-install-test.XXXXXX")"
trap 'rm -rf "$TMP_ROOT"' EXIT

case "$(uname -s):$(uname -m)" in
  Linux:x86_64|Linux:amd64)
    target=x86_64-unknown-linux-gnu
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
  for fixture_target in x86_64-unknown-linux-gnu aarch64-apple-darwin x86_64-pc-windows-msvc; do
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
  for platform_tag in manylinux_2_28_x86_64 macosx_11_0_arm64 win_amd64; do
    printf '%s  cccc_pair-%s-py3-none-%s.whl\n' "$(printf '0%.0s' {1..64})" "$wheel_version" "$platform_tag" >> "$release_dir/SHA256SUMS"
  done
  rm -rf "$TMP_ROOT/package"
}

version=0.0.0-test
make_release "$version"

# Intel Macs fail before version resolution or download.
mkdir -p "$TMP_ROOT/fake-intel-bin"
cat > "$TMP_ROOT/fake-intel-bin/uname" <<'EOF'
#!/usr/bin/env sh
case "${1:-}" in
  -s) printf 'Darwin\n' ;;
  -m) printf 'x86_64\n' ;;
  *) printf 'Darwin\n' ;;
esac
EOF
chmod 755 "$TMP_ROOT/fake-intel-bin/uname"
if PATH="$TMP_ROOT/fake-intel-bin:$PATH" \
  HOME="$TMP_ROOT/intel-home" \
  CCCC_VERSION="$version" \
  CCCC_RELEASE_BASE_URL="file://$TMP_ROOT/releases" \
  CCCC_NO_MODIFY_PATH=1 \
  sh "$ROOT_DIR/scripts/install.sh" 2> "$TMP_ROOT/intel-error"; then
  echo "installer accepted a retired macOS Intel host" >&2
  exit 1
fi
grep -Fq 'macOS Intel (x86_64) support ended with CCCC v0.4.37' "$TMP_ROOT/intel-error"

# Older releases used a four-archive manifest without native wheels. Keep that
# published shape installable from a currently supported host.
legacy_manifest_version=0.0.12-test
make_release "$legacy_manifest_version"
legacy_manifest="$TMP_ROOT/releases/download/v${legacy_manifest_version}/SHA256SUMS"
grep -v 'cccc_pair-' "$legacy_manifest" > "$legacy_manifest.legacy"
printf '%s  cccc-v%s-x86_64-apple-darwin.tar.gz\n' "$(printf '0%.0s' {1..64})" "$legacy_manifest_version" >> "$legacy_manifest.legacy"
mv "$legacy_manifest.legacy" "$legacy_manifest"
HOME="$TMP_ROOT/legacy-manifest-home" \
  CCCC_VERSION="$legacy_manifest_version" \
  CCCC_RELEASE_BASE_URL="file://$TMP_ROOT/releases" \
  CCCC_NO_MODIFY_PATH=1 \
  sh "$ROOT_DIR/scripts/install.sh"
[[ "$("$TMP_ROOT/legacy-manifest-home/.local/bin/cccc" --version)" == "cccc $legacy_manifest_version" ]]

# The final Intel-capable releases contain all four archive/wheel pairs. Their
# extra Intel entries must not block rollback on a supported host.
legacy_complete_version=0.0.13-test
make_release "$legacy_complete_version"
legacy_complete_manifest="$TMP_ROOT/releases/download/v${legacy_complete_version}/SHA256SUMS"
legacy_complete_wheel_version=${legacy_complete_version//-/}
printf '%s  cccc-v%s-x86_64-apple-darwin.tar.gz\n' "$(printf '0%.0s' {1..64})" "$legacy_complete_version" >> "$legacy_complete_manifest"
printf '%s  cccc_pair-%s-py3-none-macosx_11_0_x86_64.whl\n' "$(printf '0%.0s' {1..64})" "$legacy_complete_wheel_version" >> "$legacy_complete_manifest"
HOME="$TMP_ROOT/legacy-complete-home" \
  CCCC_VERSION="$legacy_complete_version" \
  CCCC_RELEASE_BASE_URL="file://$TMP_ROOT/releases" \
  CCCC_NO_MODIFY_PATH=1 \
  sh "$ROOT_DIR/scripts/install.sh"
[[ "$("$TMP_ROOT/legacy-complete-home/.local/bin/cccc" --version)" == "cccc $legacy_complete_version" ]]

prerelease_version=0.0.14-rc1
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
if compgen -G "$TMP_ROOT/home with space/.local/bin/.cccc-backup.*" > /dev/null; then
  echo "a rolled-back install left its backup directory" >&2
  exit 1
fi

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

# Phased updates: a running CCCC prepares one transaction, stops its own daemon,
# applies it, and then confirms or aborts it. Paths carry spaces and UTF-8.
phase_old_version=0.0.15-test
phase_new_version=0.0.16-test
phase_bad_version=0.0.17-test
phase_daemon="$TMP_ROOT/phase-daemon-running"
phase_root="$TMP_ROOT/phase home 更新"
phase_cli() {
  cat > "$1" <<EOF
#!/usr/bin/env sh
if [ "\${1:-}" = --version ]; then
  printf 'cccc $2\\n'
  exit 0
fi
if [ "\${1:-}" = daemon ] && [ "\${2:-}" = status ]; then
  [ -e '$phase_daemon' ]
  exit
fi
exit 1
EOF
  chmod 755 "$1"
}
phase_cli "$TMP_ROOT/phase-old-cccc" "$phase_old_version"
phase_cli "$TMP_ROOT/phase-new-cccc" "$phase_new_version"
make_release "$phase_new_version" valid "$phase_new_version" "" "$TMP_ROOT/phase-new-cccc"
make_release "$phase_bad_version" valid 9.9.9

phase_setup() {
  phase_id="20261117T031511Z-$1"
  phase_install="$phase_root/$1/bin"
  phase_txn="$phase_root/$1/update/$phase_id"
  phase_staged="$phase_install/cccc.cccc-install.$phase_id"
  phase_backup="$phase_install/.cccc-backup.$phase_id"
  rm -f "$phase_daemon"
  mkdir -p "$phase_install" "$phase_txn"
  cp "$TMP_ROOT/phase-old-cccc" "$phase_install/cccc"
  printf 'standalone-v1\n' > "$phase_install/.cccc-standalone"
  cp "$ROOT_DIR/scripts/install.sh" "$phase_txn/install.sh"
  phase_old_hash=$(checksum "$phase_install/cccc")
}

run_phase() {
  local phase=$1 txn=$2
  shift 2
  env HOME="$phase_root/home" \
    CCCC_HOME="$phase_root/cccc home" \
    CCCC_INSTALL_PHASE="$phase" \
    CCCC_INSTALL_TXN="$txn" \
    CCCC_INSTALL_DIR="$phase_install" \
    CCCC_RELEASE_BASE_URL="file://$TMP_ROOT/releases" \
    CCCC_NO_MODIFY_PATH=1 \
    "$@" sh "$txn/install.sh"
}

phase_listing() {
  (cd "$phase_install" && LC_ALL=C ls -A)
}

phase_setup commit
run_phase prepare "$phase_txn" CCCC_VERSION="$phase_new_version" > "$TMP_ROOT/phase-prepare.out"
grep -Fxq "CCCC_INSTALL_STAGED=$phase_staged" "$TMP_ROOT/phase-prepare.out"
[[ "$("$phase_staged" --version)" == "cccc $phase_new_version" ]]
[[ "$("$phase_install/cccc" --version)" == "cccc $phase_old_version" ]]
[[ "$(cat "$phase_install/.cccc-install.lock/txn")" == "$(cd -P "$phase_txn" && pwd -P)" ]]
[[ "$(cat "$phase_txn/installer.state")" == "phase=prepared"$'\n'"version=$phase_new_version" ]]
test ! -e "$phase_backup"
run_phase apply "$phase_txn" > "$TMP_ROOT/phase-apply.out"
grep -Fxq "CCCC_INSTALL_APPLIED=$phase_new_version" "$TMP_ROOT/phase-apply.out"
[[ "$("$phase_install/cccc" --version)" == "cccc $phase_new_version" ]]
[[ "$(checksum "$phase_backup/cccc")" == "$phase_old_hash" ]]
grep -Fxq standalone-v1 "$phase_backup/.cccc-standalone"
grep -Fxq standalone-v1 "$phase_install/.cccc-standalone"
grep -Fxq phase=applied "$phase_txn/installer.state"
test ! -e "$phase_staged"
[[ "$(cat "$phase_install/.cccc-install.lock/txn")" == "$(cd -P "$phase_txn" && pwd -P)" ]]
run_phase confirm "$phase_txn" > "$TMP_ROOT/phase-confirm.out"
grep -Fxq "CCCC_INSTALL_CONFIRMED=$phase_new_version" "$TMP_ROOT/phase-confirm.out"
[[ "$("$phase_install/cccc" --version)" == "cccc $phase_new_version" ]]
[[ "$(phase_listing)" == $'.cccc-standalone\ncccc' ]]
# The applied state stays until the caller removes the transaction: an abort
# after a finished confirm reports the update as committed, never as undone,
# and confirm can be repeated.
grep -Fxq phase=applied "$phase_txn/installer.state"
if run_phase abort "$phase_txn" > "$TMP_ROOT/phase-after-confirm.out" 2> "$TMP_ROOT/phase-after-confirm.err"; then
  echo "phased abort after a finished confirm reported a restore" >&2
  exit 1
fi
grep -Fxq "CCCC_INSTALL_COMMITTED=$phase_new_version" "$TMP_ROOT/phase-after-confirm.out"
if grep -Fq CCCC_INSTALL_ABORTED "$TMP_ROOT/phase-after-confirm.out"; then
  echo "phased abort after a finished confirm printed an aborted result" >&2
  exit 1
fi
run_phase confirm "$phase_txn" > "$TMP_ROOT/phase-confirm.out"
grep -Fxq "CCCC_INSTALL_CONFIRMED=$phase_new_version" "$TMP_ROOT/phase-confirm.out"
[[ "$("$phase_install/cccc" --version)" == "cccc $phase_new_version" ]]
[[ "$(phase_listing)" == $'.cccc-standalone\ncccc' ]]

# Before anything moves, the old daemon may still run; abort only discards.
phase_setup abort-prepared
run_phase prepare "$phase_txn" CCCC_VERSION="$phase_new_version" > /dev/null
: > "$phase_daemon"
run_phase abort "$phase_txn" > "$TMP_ROOT/phase-abort.out"
grep -Fxq "CCCC_INSTALL_ABORTED=$phase_id" "$TMP_ROOT/phase-abort.out"
rm -f "$phase_daemon"
[[ "$(checksum "$phase_install/cccc")" == "$phase_old_hash" ]]
[[ "$(phase_listing)" == $'.cccc-standalone\ncccc' ]]
test ! -e "$phase_txn/installer.state"

# Neither apply nor a rollback swaps files under a running daemon.
phase_setup rollback
run_phase prepare "$phase_txn" CCCC_VERSION="$phase_new_version" > /dev/null
: > "$phase_daemon"
if run_phase apply "$phase_txn" > "$TMP_ROOT/phase-running.out" 2> "$TMP_ROOT/phase-running.err"; then
  echo "phased apply replaced files while the daemon was running" >&2
  exit 1
fi
grep -Fq 'daemon is still running' "$TMP_ROOT/phase-running.err"
if grep -Fq CCCC_INSTALL_ROLLED_BACK "$TMP_ROOT/phase-running.out"; then
  echo "a refused apply reported that it ended the transaction" >&2
  exit 1
fi
[[ "$(checksum "$phase_install/cccc")" == "$phase_old_hash" ]]
grep -Fxq phase=prepared "$phase_txn/installer.state"
test -f "$phase_staged"
rm -f "$phase_daemon"
run_phase apply "$phase_txn" > /dev/null
[[ "$("$phase_install/cccc" --version)" == "cccc $phase_new_version" ]]
: > "$phase_daemon"
if run_phase abort "$phase_txn" > /dev/null 2> "$TMP_ROOT/phase-abort-running.err"; then
  echo "phased abort rolled back files under a running daemon" >&2
  exit 1
fi
grep -Fq 'stop it before rolling back' "$TMP_ROOT/phase-abort-running.err"
[[ "$("$phase_install/cccc" --version)" == "cccc $phase_new_version" ]]
rm -f "$phase_daemon"
run_phase abort "$phase_txn" > /dev/null
[[ "$(checksum "$phase_install/cccc")" == "$phase_old_hash" ]]
grep -Fxq standalone-v1 "$phase_install/.cccc-standalone"
[[ "$(phase_listing)" == $'.cccc-standalone\ncccc' ]]
test ! -e "$phase_txn/installer.state"

# An apply killed between its renames is refused again and undone by abort.
phase_setup interrupted
run_phase prepare "$phase_txn" CCCC_VERSION="$phase_new_version" > /dev/null
mkdir "$phase_backup"
mv "$phase_install/cccc" "$phase_backup/cccc"
if run_phase apply "$phase_txn" > /dev/null 2> "$TMP_ROOT/phase-interrupted.err"; then
  echo "phased apply ignored a half-applied transaction" >&2
  exit 1
fi
grep -Fq 'interrupted while applying' "$TMP_ROOT/phase-interrupted.err"
run_phase abort "$phase_txn" > /dev/null
[[ "$(checksum "$phase_install/cccc")" == "$phase_old_hash" ]]
[[ "$(phase_listing)" == $'.cccc-standalone\ncccc' ]]

# A failed apply restores the previous files and ends the transaction.
phase_setup mismatch
run_phase prepare "$phase_txn" CCCC_VERSION="$phase_bad_version" > /dev/null
if run_phase apply "$phase_txn" > "$TMP_ROOT/phase-mismatch.out" 2> "$TMP_ROOT/phase-mismatch.err"; then
  echo "phased apply accepted a mismatched binary version" >&2
  exit 1
fi
grep -Fq 'installed version mismatch' "$TMP_ROOT/phase-mismatch.err"
grep -Fxq "CCCC_INSTALL_ROLLED_BACK=$phase_id" "$TMP_ROOT/phase-mismatch.out"
[[ "$(checksum "$phase_install/cccc")" == "$phase_old_hash" ]]
grep -Fxq standalone-v1 "$phase_install/.cccc-standalone"
[[ "$(phase_listing)" == $'.cccc-standalone\ncccc' ]]
test ! -e "$phase_txn/installer.state"

# When the restore itself fails, apply keeps the transaction so abort can
# finish the restore; it does not report that the transaction ended.
phase_setup restore-fails
real_mv=$(command -v mv)
mkdir -p "$TMP_ROOT/phase-mv-bin"
cat > "$TMP_ROOT/phase-mv-bin/mv" <<EOF
#!/usr/bin/env sh
case "\$*" in
  *'.cccc-backup.'*' '*'/cccc')
    [ ! -e '$TMP_ROOT/phase-mv-fails' ] || exit 1
    ;;
esac
exec '$real_mv' "\$@"
EOF
chmod 755 "$TMP_ROOT/phase-mv-bin/mv"
run_phase prepare "$phase_txn" CCCC_VERSION="$phase_bad_version" > /dev/null
: > "$TMP_ROOT/phase-mv-fails"
if run_phase apply "$phase_txn" PATH="$TMP_ROOT/phase-mv-bin:$PATH" \
  > "$TMP_ROOT/phase-restore.out" 2> "$TMP_ROOT/phase-restore.err"; then
  echo "phased apply accepted a mismatched binary version" >&2
  exit 1
fi
grep -Fq 'rollback failed to restore' "$TMP_ROOT/phase-restore.err"
if grep -Fq CCCC_INSTALL_ROLLED_BACK "$TMP_ROOT/phase-restore.out"; then
  echo "apply reported an ended transaction after a failed restore" >&2
  exit 1
fi
[[ "$(cat "$phase_install/.cccc-install.lock/txn")" == "$(cd -P "$phase_txn" && pwd -P)" ]]
[[ "$(checksum "$phase_backup/cccc")" == "$phase_old_hash" ]]
rm -f "$TMP_ROOT/phase-mv-fails"
run_phase abort "$phase_txn" > /dev/null
[[ "$(checksum "$phase_install/cccc")" == "$phase_old_hash" ]]
grep -Fxq standalone-v1 "$phase_install/.cccc-standalone"
[[ "$(phase_listing)" == $'.cccc-standalone\ncccc' ]]

# The install lock belongs to one transaction until it is confirmed or aborted.
phase_setup lock
other_txn="$phase_root/lock/update/20261117T031511Z-other"
mkdir -p "$other_txn"
cp "$ROOT_DIR/scripts/install.sh" "$other_txn/install.sh"
run_phase prepare "$phase_txn" CCCC_VERSION="$phase_new_version" > /dev/null
if run_phase apply "$other_txn" > /dev/null 2> "$TMP_ROOT/phase-other.err"; then
  echo "phased apply ran without holding the install lock" >&2
  exit 1
fi
grep -Fq 'does not hold the install lock' "$TMP_ROOT/phase-other.err"
if run_phase confirm "$other_txn" > /dev/null 2> "$TMP_ROOT/phase-other.err"; then
  echo "phased confirm finished a transaction that was never applied" >&2
  exit 1
fi
grep -Fq 'has not been applied' "$TMP_ROOT/phase-other.err"
# A transaction that holds nothing aborts without touching the holder's files.
run_phase abort "$other_txn" > "$TMP_ROOT/phase-other.out"
grep -Fxq 'CCCC_INSTALL_ABORTED=20261117T031511Z-other' "$TMP_ROOT/phase-other.out"
[[ "$(cat "$phase_install/.cccc-install.lock/txn")" == "$(cd -P "$phase_txn" && pwd -P)" ]]
if run_phase prepare "$other_txn" CCCC_VERSION="$phase_new_version" > /dev/null 2> "$TMP_ROOT/phase-other.err"; then
  echo "a second update was prepared while another held the install lock" >&2
  exit 1
fi
grep -Fq 'another installation is using' "$TMP_ROOT/phase-other.err"
if HOME="$phase_root/home" \
  CCCC_VERSION="$phase_new_version" \
  CCCC_RELEASE_BASE_URL="file://$TMP_ROOT/releases" \
  CCCC_INSTALL_DIR="$phase_install" \
  CCCC_NO_MODIFY_PATH=1 \
  sh "$ROOT_DIR/scripts/install.sh" > /dev/null 2> "$TMP_ROOT/phase-other.err"; then
  echo "a one-go install ran while an update held the install lock" >&2
  exit 1
fi
grep -Fq 'another installation is using' "$TMP_ROOT/phase-other.err"
grep -Fq "held by CCCC update $(cd -P "$phase_txn" && pwd -P) for CCCC_HOME=$phase_root/cccc home" "$TMP_ROOT/phase-other.err"
test -f "$phase_staged"
[[ "$(checksum "$phase_install/cccc")" == "$phase_old_hash" ]]
run_phase abort "$phase_txn" > /dev/null
[[ "$(phase_listing)" == $'.cccc-standalone\ncccc' ]]

# The state file is read as two known keys, never as shell code.
phase_setup tampered
run_phase prepare "$phase_txn" CCCC_VERSION="$phase_new_version" > /dev/null
printf 'source=/tmp/elsewhere\n' >> "$phase_txn/installer.state"
if run_phase apply "$phase_txn" > /dev/null 2> "$TMP_ROOT/phase-tampered.err"; then
  echo "phased apply accepted an unknown state key" >&2
  exit 1
fi
grep -Fq 'invalid installer state' "$TMP_ROOT/phase-tampered.err"
if run_phase abort "$phase_txn" > /dev/null 2> "$TMP_ROOT/phase-tampered.err"; then
  echo "phased abort acted on an unreadable state" >&2
  exit 1
fi
grep -Fq 'invalid installer state' "$TMP_ROOT/phase-tampered.err"
[[ "$(checksum "$phase_install/cccc")" == "$phase_old_hash" ]]
test -f "$phase_staged"
printf 'phase=prepared\nversion=%s\n' "$phase_new_version" > "$phase_txn/installer.state"
run_phase abort "$phase_txn" > /dev/null
[[ "$(phase_listing)" == $'.cccc-standalone\ncccc' ]]

# A transaction with the same name under another CCCC_HOME is not the holder,
# and a refused prepare leaves the holder's staged binary untouched.
phase_setup same-name
foreign_txn="$phase_root/same-name/other home/update/$phase_id"
mkdir -p "$foreign_txn"
cp "$ROOT_DIR/scripts/install.sh" "$foreign_txn/install.sh"
run_phase prepare "$phase_txn" CCCC_VERSION="$phase_new_version" > /dev/null
staged_hash=$(checksum "$phase_staged")
printf 'phase=prepared\nversion=%s\n' "$phase_new_version" > "$foreign_txn/installer.state"
for foreign_phase in apply abort; do
  if run_phase "$foreign_phase" "$foreign_txn" > /dev/null 2> "$TMP_ROOT/phase-foreign.err"; then
    echo "phased $foreign_phase accepted a same-name transaction from another home" >&2
    exit 1
  fi
  grep -Fq 'does not hold the install lock' "$TMP_ROOT/phase-foreign.err"
done
printf 'phase=applied\nversion=%s\n' "$phase_new_version" > "$foreign_txn/installer.state"
if run_phase confirm "$foreign_txn" > /dev/null 2> "$TMP_ROOT/phase-foreign.err"; then
  echo "phased confirm accepted a same-name transaction from another home" >&2
  exit 1
fi
grep -Fq 'does not hold the install lock' "$TMP_ROOT/phase-foreign.err"
rm "$foreign_txn/installer.state"
if run_phase prepare "$foreign_txn" CCCC_VERSION="$phase_new_version" > /dev/null 2> "$TMP_ROOT/phase-foreign.err"; then
  echo "a same-name transaction was prepared while another held the install lock" >&2
  exit 1
fi
grep -Fq 'another installation is using' "$TMP_ROOT/phase-foreign.err"
[[ "$(checksum "$phase_staged")" == "$staged_hash" ]]
[[ "$(cat "$phase_install/.cccc-install.lock/txn")" == "$(cd -P "$phase_txn" && pwd -P)" ]]
run_phase abort "$phase_txn" > /dev/null
[[ "$(phase_listing)" == $'.cccc-standalone\ncccc' ]]

# Once confirm has deleted the backup, abort cannot undo the update and says
# so instead of reporting a restore; confirm then finishes.
phase_setup confirm-interrupted
run_phase prepare "$phase_txn" CCCC_VERSION="$phase_new_version" > /dev/null
run_phase apply "$phase_txn" > /dev/null
rm -rf "$phase_backup"
if run_phase abort "$phase_txn" > "$TMP_ROOT/phase-committed.out" 2> "$TMP_ROOT/phase-committed.err"; then
  echo "phased abort reported a restore without the previous binary" >&2
  exit 1
fi
grep -Fq 'finish it with confirm' "$TMP_ROOT/phase-committed.err"
grep -Fxq "CCCC_INSTALL_COMMITTED=$phase_new_version" "$TMP_ROOT/phase-committed.out"
if grep -Fq CCCC_INSTALL_ABORTED "$TMP_ROOT/phase-committed.out"; then
  echo "phased abort printed an aborted result without restoring" >&2
  exit 1
fi
[[ "$("$phase_install/cccc" --version)" == "cccc $phase_new_version" ]]
test -d "$phase_install/.cccc-install.lock"
run_phase confirm "$phase_txn" > /dev/null
[[ "$(phase_listing)" == $'.cccc-standalone\ncccc' ]]
grep -Fxq phase=applied "$phase_txn/installer.state"

# A confirm interrupted after it released the lock leaves only an applied
# state: abort still refuses, and confirm finishes it.
phase_setup confirm-released
run_phase prepare "$phase_txn" CCCC_VERSION="$phase_new_version" > /dev/null
run_phase apply "$phase_txn" > /dev/null
rm -rf "$phase_backup" "$phase_install/.cccc-install.lock"
if run_phase abort "$phase_txn" > "$TMP_ROOT/phase-released.out" 2> "$TMP_ROOT/phase-released.err"; then
  echo "phased abort reported a restore after the update was committed" >&2
  exit 1
fi
grep -Fq 'finish it with confirm' "$TMP_ROOT/phase-released.err"
grep -Fxq "CCCC_INSTALL_COMMITTED=$phase_new_version" "$TMP_ROOT/phase-released.out"
grep -Fxq phase=applied "$phase_txn/installer.state"
run_phase confirm "$phase_txn" > "$TMP_ROOT/phase-released.out"
grep -Fxq "CCCC_INSTALL_CONFIRMED=$phase_new_version" "$TMP_ROOT/phase-released.out"
[[ "$("$phase_install/cccc" --version)" == "cccc $phase_new_version" ]]
[[ "$(phase_listing)" == $'.cccc-standalone\ncccc' ]]
grep -Fxq phase=applied "$phase_txn/installer.state"
run_phase confirm "$phase_txn" > "$TMP_ROOT/phase-released.out"
grep -Fxq "CCCC_INSTALL_CONFIRMED=$phase_new_version" "$TMP_ROOT/phase-released.out"
[[ "$(phase_listing)" == $'.cccc-standalone\ncccc' ]]

# A prepare that failed before it took the lock holds nothing, so abort ends
# it and changes nothing.
phase_setup prepare-failed
if run_phase prepare "$phase_txn" CCCC_VERSION="$bad_version" > /dev/null 2> "$TMP_ROOT/phase-unlocked.err"; then
  echo "phased prepare accepted a bad checksum" >&2
  exit 1
fi
grep -Fq 'checksum mismatch' "$TMP_ROOT/phase-unlocked.err"
run_phase abort "$phase_txn" > "$TMP_ROOT/phase-unlocked.out"
grep -Fxq "CCCC_INSTALL_ABORTED=$phase_id" "$TMP_ROOT/phase-unlocked.out"
[[ "$(checksum "$phase_install/cccc")" == "$phase_old_hash" ]]
[[ "$(phase_listing)" == $'.cccc-standalone\ncccc' ]]
run_phase abort "$phase_txn" > "$TMP_ROOT/phase-unlocked.out"
grep -Fxq "CCCC_INSTALL_ABORTED=$phase_id" "$TMP_ROOT/phase-unlocked.out"

# Files staged by a transaction whose lock is gone are never discarded: abort
# refuses instead of guessing who owns them.
phase_setup lock-lost
run_phase prepare "$phase_txn" CCCC_VERSION="$phase_new_version" > /dev/null
rm -rf "$phase_install/.cccc-install.lock"
if run_phase abort "$phase_txn" > "$TMP_ROOT/phase-lost.out" 2> "$TMP_ROOT/phase-lost.err"; then
  echo "phased abort discarded staged files without holding the install lock" >&2
  exit 1
fi
grep -Fq 'does not hold the install lock' "$TMP_ROOT/phase-lost.err"
if grep -Fq CCCC_INSTALL_ABORTED "$TMP_ROOT/phase-lost.out"; then
  echo "phased abort reported an ended transaction while its files remain" >&2
  exit 1
fi
test -f "$phase_staged"
grep -Fxq phase=prepared "$phase_txn/installer.state"
[[ "$(checksum "$phase_install/cccc")" == "$phase_old_hash" ]]
phase_setup lock-lost-applied
run_phase prepare "$phase_txn" CCCC_VERSION="$phase_new_version" > /dev/null
run_phase apply "$phase_txn" > /dev/null
rm -rf "$phase_install/.cccc-install.lock"
if run_phase abort "$phase_txn" > "$TMP_ROOT/phase-lost.out" 2> "$TMP_ROOT/phase-lost.err"; then
  echo "phased abort ended a transaction whose backup remains without holding the lock" >&2
  exit 1
fi
grep -Fq 'does not hold the install lock' "$TMP_ROOT/phase-lost.err"
if grep -Fq CCCC_INSTALL_ "$TMP_ROOT/phase-lost.out"; then
  echo "phased abort printed a result while the backup remains" >&2
  exit 1
fi
[[ "$(checksum "$phase_backup/cccc")" == "$phase_old_hash" ]]

# prepare needs an explicit version, an existing standalone installation and a
# fresh transaction.
phase_setup preconditions
if run_phase prepare "$phase_txn" > /dev/null 2> "$TMP_ROOT/phase-pre.err"; then
  echo "phased prepare ran without an explicit version" >&2
  exit 1
fi
grep -Fq 'CCCC_VERSION is required' "$TMP_ROOT/phase-pre.err"
rm "$phase_install/.cccc-standalone"
if run_phase prepare "$phase_txn" CCCC_VERSION="$phase_new_version" > /dev/null 2> "$TMP_ROOT/phase-pre.err"; then
  echo "phased prepare accepted an installation without the standalone marker" >&2
  exit 1
fi
grep -Fq 'needs an existing standalone CCCC installation' "$TMP_ROOT/phase-pre.err"
[[ "$(phase_listing)" == cccc ]]
test ! -e "$phase_txn/installer.state"
printf 'standalone-v1\n' > "$phase_install/.cccc-standalone"
run_phase prepare "$phase_txn" CCCC_VERSION="$phase_new_version" > /dev/null
if run_phase prepare "$phase_txn" CCCC_VERSION="$phase_new_version" > /dev/null 2> "$TMP_ROOT/phase-pre.err"; then
  echo "phased prepare ran twice for one transaction" >&2
  exit 1
fi
grep -Fq 'already prepared' "$TMP_ROOT/phase-pre.err"
test -f "$phase_staged"
run_phase abort "$phase_txn" > /dev/null
[[ "$(phase_listing)" == $'.cccc-standalone\ncccc' ]]

# Transaction directories must be absolute, real and plainly named.
phase_setup invalid
mkdir -p "$phase_root/invalid/update/.hidden" "$phase_root/invalid/update/has space"
ln -s "$phase_txn" "$phase_root/invalid/update/linked"
for bad_txn in "" relative/update/x "$phase_root/invalid/update/missing" \
  "$phase_root/invalid/update/.hidden" "$phase_root/invalid/update/has space" \
  "$phase_root/invalid/update/linked"; do
  if env HOME="$phase_root/home" \
    CCCC_INSTALL_PHASE=prepare \
    CCCC_INSTALL_TXN="$bad_txn" \
    CCCC_INSTALL_DIR="$phase_install" \
    CCCC_VERSION="$phase_new_version" \
    CCCC_RELEASE_BASE_URL="file://$TMP_ROOT/releases" \
    CCCC_NO_MODIFY_PATH=1 \
    sh "$ROOT_DIR/scripts/install.sh" > /dev/null 2>&1; then
    echo "phased prepare accepted transaction directory '$bad_txn'" >&2
    exit 1
  fi
done
if run_phase commit "$phase_txn" > /dev/null 2> "$TMP_ROOT/phase-invalid.err"; then
  echo "installer accepted an unknown phase" >&2
  exit 1
fi
grep -Fq 'unknown CCCC_INSTALL_PHASE: commit' "$TMP_ROOT/phase-invalid.err"
[[ "$(phase_listing)" == $'.cccc-standalone\ncccc' ]]

echo "OK: Unix installer"
