#!/bin/bash
# Exercise only disposable member processes, browser profiles and loopback endpoints.
set -euo pipefail
umask 077
out=${1:?absolute evidence directory required}
here=$(cd "$(dirname "$0")" && pwd)
test "$(uname -s)/$(uname -m)" = Darwin/arm64
test "$(id -u)" -ne 0
mkdir -p "$out/bin"
export CARGO_TERM_COLOR=never
export CARGO_PROFILE_DEV_DEBUG=0
export CARGO_PROFILE_TEST_DEBUG=0
overall=0
phase() {
    local name=$1 rc=0
    shift
    "$@" > "$out/$name.log" 2>&1 || rc=$?
    printf '%s\n' "$rc" > "$out/$name.rc"
    tail -n 14 "$out/$name.log"
    if [ "$rc" -ne 0 ]; then overall=1; fi
    return "$rc"
}
{
    sw_vers
    uname -m
    git rev-parse HEAD HEAD^
    df -k .
} > "$out/environment.txt"
phase previous_build bash "$here/build_previous.sh" "$out" || true
phase inspector xcrun clang -std=c11 -D_DARWIN_C_SOURCE -Wall -Wextra -Werror \
    "$here/inspect_process.c" -o "$out/bin/inspect-process"
phase web_dependencies npm ci --prefix web
phase web_bundle npm -C web run build
phase candidate_build cargo build --workspace --locked || true
if [ -x target/debug/cccc ] && [ "$(cat "$out/candidate_build.rc")" = 0 ]; then
    root=$(mktemp -d /tmp/cccc-native-test.XXXXXXXX)
    mkdir "$root/home" "$root/cccc" "$root/codex" "$root/claude" "$root/tmp"
    test_env=(env -i PATH="$PATH" HOME="$root/home" CCCC_HOME="$root/cccc" CODEX_HOME="$root/codex"
        CLAUDE_CONFIG_DIR="$root/claude" TMPDIR="$root/tmp/" LANG=en_US.UTF-8
        RUSTUP_HOME="${RUSTUP_HOME:-$HOME/.rustup}" CARGO_HOME="${CARGO_HOME:-$HOME/.cargo}"
        CARGO_TERM_COLOR=never CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0)
    # Keep the diagnostics from either phase even if the other one fails.
    phase candidate_clippy "${test_env[@]}" cargo clippy --workspace --all-targets --locked -- -D warnings || true
    phase candidate_check "${test_env[@]}" target/debug/cccc update --self-check || true
    # libtest accepts multiple positional filters. The report requires an exact PASS
    # for each name so a filter with zero matches cannot pass this job.
    phase candidate_tests "${test_env[@]}" cargo test --workspace --lib --bins --locked -- \
        --test-threads=1 native_watchdog_inherits_its_signal_mask_and_restores_the_callers_mask \
        a_group_published_while_the_watchdog_is_paused_still_ends_with_its_owner \
        self_check_reports_this_programs_constants system_browser_answers_cdp_and_stops_its_original_process \
        failed_cdp_connection_stops_the_browser_opened_for_its_profile || true
    if [ "$(cat "$out/previous_build.rc")" = 0 ]; then
        phase lifecycle bash "$here/run_lifecycle.sh" "$out/bin/cccc-previous" "$PWD/target/debug/cccc" \
            "$out" "$out/bin/inspect-process" || true
    fi
    rm -r "$root"
fi
phase source_unchanged git diff --exit-code HEAD || true
df -k . > "$out/disk_after.txt"
exit "$overall"
