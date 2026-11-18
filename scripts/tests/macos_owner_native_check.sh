#!/usr/bin/env bash
# Native macOS validation only. No provider CLI or user service is started.
set -euo pipefail
umask 077
export CARGO_TERM_COLOR=never
ulimit -c 0
PROBE_DIR="${1:?evidence directory required}"
test "$(uname -s)" = Darwin
test "$(uname -m)" = arm64
test "$(id -u)" -ne 0
mkdir -p "$PROBE_DIR/bin" "$PROBE_DIR/test/home" "$PROBE_DIR/test/tmp"
overall=0

phase() {
  local phase_name="$1"; shift
  local phase_status=0
  "$@" > "$PROBE_DIR/$phase_name.log" 2>&1 || phase_status=$?
  if test "$phase_status" != 0; then overall=1; fi
  printf '%s\n' "$phase_status" > "$PROBE_DIR/$phase_name.rc"
  tail -n 16 "$PROBE_DIR/$phase_name.log"
  return "$phase_status"
}

{
  sw_vers
  uname -a
  xcrun --show-sdk-version
  rustc -Vv
  git rev-parse HEAD
  git rev-parse 'HEAD^{tree}'
  df -k .
} > "$PROBE_DIR/environment.txt"
if test -d target/debug; then
  du -sk target/debug > "$PROBE_DIR/target_before.txt"
else
  printf '0 target/debug\n' > "$PROBE_DIR/target_before.txt"
fi
git ls-files -z | python3 -I -c '
import hashlib, pathlib, sys
rows = [(p, hashlib.sha256(pathlib.Path(p).read_bytes()).hexdigest())
        for p in sys.stdin.buffer.read().decode().split("\0") if p]
for path, digest in rows:
    print(digest, path)
' > "$PROBE_DIR/source.sha256"

# Immutable official universal archive. Its published SHA256 is pinned, not
# taken from an unverified local tool or a latest-version alias.
nextest_archive=cargo-nextest-0.9.148-universal-apple-darwin.tar.gz
phase nextest_download curl --fail --location --retry 2 \
  "https://github.com/nextest-rs/nextest/releases/download/cargo-nextest-0.9.148/$nextest_archive" \
  --output "$PROBE_DIR/$nextest_archive"
printf '6c23b7fb4ca82c571fc8dfedbab45bfa773d45918f933170c85d21cd7f4b852b  %s\n' \
  "$PROBE_DIR/$nextest_archive" > "$PROBE_DIR/nextest_archive.sha256"
phase nextest_verify shasum -a 256 -c "$PROBE_DIR/nextest_archive.sha256"
tar -xzf "$PROBE_DIR/$nextest_archive" -C "$PROBE_DIR/bin" cargo-nextest
rm "$PROBE_DIR/$nextest_archive"
"$PROBE_DIR/bin/cargo-nextest" --version > "$PROBE_DIR/nextest-version.txt"

phase sdk_compile xcrun clang -std=c11 -D_DARWIN_C_SOURCE -Wall -Wextra \
  scripts/tests/macos_identity_layout_probe.c -o "$PROBE_DIR/sdk-layout"
phase sdk_layout "$PROBE_DIR/sdk-layout"
rm "$PROBE_DIR/sdk-layout"

# One Web build before Rust; cccc-web's build script reuses this fresh bundle.
phase web_dependencies npm ci --prefix web
phase web_bundle npm -C web run build
printf '1\n' > "$PROBE_DIR/explicit_web_builds.txt"
phase check cargo check --workspace --all-targets --locked --keep-going || true
phase clippy cargo clippy --workspace --all-targets --locked --keep-going -- -D warnings || true

filter='(package(cccc-pair-runtime) & (test(/process_tree::guard::/) | test(/process_tree::darwin_identity::/) | test(/darwin_process::/) | test(/running_image::/) | test(/process_children::/))) | (package(cccc-pair-core) & (test(/local_sessions::/) | test(/update_image::/) | test(/unix_socket_path::/))) | (package(cccc-pair-daemon) & (test(/session_owner::transport::/) | test(/an_unreachable_owner_keeps_unknown_and_exited_guidance_separate/))) | (package(cccc-pair-web) & test(/browser_surface::profile_owner::/))'
printf '%s\n' "$filter" > "$PROBE_DIR/filter.txt"
phase native_tests env -i \
  PATH="$PATH" HOME="$PROBE_DIR/test/home" TMPDIR="$PROBE_DIR/test/tmp/" \
  CARGO_HOME="${CARGO_HOME:-$HOME/.cargo}" RUSTUP_HOME="${RUSTUP_HOME:-$HOME/.rustup}" \
  CCCC_HOME="$PROBE_DIR/test/cccc" CODEX_HOME="$PROBE_DIR/test/codex" \
  CLAUDE_CONFIG_DIR="$PROBE_DIR/test/claude" LANG=en_US.UTF-8 \
  CARGO_TERM_COLOR=never NEXTEST_COLOR=never CCCC_TEST_FORBID_REAL_CHROME=1 \
  "$PROBE_DIR/bin/cargo-nextest" nextest run --workspace --locked --profile ci \
  --no-fail-fast -E "$filter" || true
phase coverage python3 -I scripts/tests/macos_owner_native_report.py "$PROBE_DIR" --verify || true
if phase binary cargo build --workspace --locked; then
  phase version env -i PATH=/usr/bin:/bin HOME="$PROBE_DIR/test/home" \
    TMPDIR="$PROBE_DIR/test/tmp/" target/debug/cccc --version || true
  phase self_check env -i PATH=/usr/bin:/bin HOME="$PROBE_DIR/test/home" \
    TMPDIR="$PROBE_DIR/test/tmp/" target/debug/cccc update --self-check || true
  phase watchdog python3 -I scripts/tests/macos_watchdog_signal_probe.py \
    --binary target/debug/cccc --output "$PROBE_DIR/watchdog.json" || true
fi
phase source_unchanged git diff --exit-code HEAD || true
df -k . > "$PROBE_DIR/disk_after.txt"
du -sk target/debug > "$PROBE_DIR/target_size.txt"
printf '%s\n' "$overall" > "$PROBE_DIR/overall.rc"
exit "$overall"
