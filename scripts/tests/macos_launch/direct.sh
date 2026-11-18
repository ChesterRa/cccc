#!/bin/bash
# Fixture-only build and verification for the three retained browser journeys.
set -euo pipefail
umask 077
previous=24b81439e17583d29335af2cdc32326b470f32f9
current=${GITHUB_SHA:?}
patches=$(cd "$(dirname "$0")" && pwd -P)
work="${RUNNER_TEMP:?}/cccc-busy-direct"
evidence="$work/evidence"
checkout="${GITHUB_WORKSPACE:?}"

environment() {
    sw_vers
    uname -m
    xcrun --sdk macosx --show-sdk-version
    rustc -Vv
    /usr/bin/defaults read "/Applications/Google Chrome.app/Contents/Info" CFBundleShortVersionString
    node --version
    python3 --version
    df -Pk "$RUNNER_TEMP"
}

prepare() {
    [[ $(uname -s) == Darwin && $(uname -m) == arm64 ]]
    [[ -x "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome" ]]
    mkdir "$work"
    mkdir "$evidence" "$work/previous" "$work/current" "$work/bin"
    environment | tee "$evidence/environment.txt"
    printf '%s\n' "$GITHUB_SHA" > "$evidence/fixture-commit.txt"
    for role in previous current; do
        if [[ $role == previous ]]; then ref=$previous; else ref=$current; fi
        git -C "$checkout" archive "$ref" | tar -x -C "$work/$role"
        printf '%s\n' "$ref" > "$evidence/$role-commit.txt"
    done
    (cd "$work/previous" && git apply --check "$patches/previous-direct.patch" && git apply "$patches/previous-direct.patch")
    shasum -a 256 "$patches/previous-direct.patch" > "$evidence/patches.sha256"
    xcrun --sdk macosx clang -std=c11 -D_DARWIN_C_SOURCE -Wall -Wextra -Werror \
        "$work/current/scripts/tests/macos_maintenance/inspect_process.c" -o "$work/inspect-process"
    for role in previous current; do
        (
            cd "$work/$role/web"
            npm ci
            npm run build
            test -s dist/index.html
        )
    done
}

verify_program() {
    local role=$1 expected=$2
    /usr/bin/codesign --verify "$work/bin/$role"
    "$work/bin/$role" update --self-check > "$evidence/$role.json"
    python3 -I - "$evidence/$role.json" "$expected" <<'PY'
import json, sys
with open(sys.argv[1]) as stream:
    result = json.load(stream)
assert result["version"] == "0.4.58"
assert result["build"]["source_id"] == sys.argv[2], result
assert result["capabilities"]["preserve_agents"] is True
PY
    shasum -a 256 "$work/bin/$role" > "$evidence/$role.sha256"
}

build() {
    role=$1
    case "$role" in
        previous) expected=d86c7b6159cbeee50763ed8ed07fe2228f95ac5a695e4bf92613ec3b231c8276 ;;
        current) expected=05669bd542e265a9adf1e706c1776d6fcafa58348599ecf01df61065a319131c ;;
        *) exit 2 ;;
    esac
    cd "$work/$role"
    df -Pk . > "$evidence/$role-disk-before.txt"
    CARGO_TARGET_DIR="$PWD/target" CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0 \
        cargo build --locked -p cccc --bin cccc 2>&1 | tee "$evidence/$role-build.log"
    cp target/debug/cccc "$work/bin/$role"
    verify_program "$role" "$expected"
    du -sk target/debug web/dist > "$evidence/$role-target-kib.txt"
    df -Pk . > "$evidence/$role-disk-built.txt"
    # Both debug source trees and their web/dist stay in place until every case finishes.
    python3 -I -c 'import shutil; shutil.rmtree("target")'
    df -Pk . > "$evidence/$role-disk-after.txt"
}

run_case() {
    case "$1" in daemon|local|release) kind=$1 ;; *) exit 2 ;; esac
    cd "$work/current"
    out="$evidence/results"
    mkdir -p "$out"
    private=$(cd "$(mktemp -d /tmp/cccc-isolated.XXXXXXXX)" && pwd -P)
    trap 'python3 -I -c "import shutil, sys; shutil.rmtree(sys.argv[1])" "$private"' EXIT
    mkdir -p "$private"/{home,tmp,cccc,codex,claude}
    py=$(command -v python3)
    /usr/bin/env -i PATH=/usr/bin:/bin LANG=C HOME="$private/home" TMPDIR="$private/tmp" \
        CCCC_HOME="$private/cccc" CODEX_HOME="$private/codex" CLAUDE_CONFIG_DIR="$private/claude" \
        CCCC_TEST_NODE="$(command -v node)" CCCC_TEST_PREVIOUS_DIST="$work/previous/web/dist" \
        CCCC_TEST_SDK_INSPECTOR="$work/inspect-process" "$py" -BI \
        scripts/tests/macos_maintenance/busy.py "$work/bin/previous" "$work/bin/current" "$out" "$kind" \
        2>&1 | tee "$evidence/$kind-driver.log"
    test -s "$out/$kind/result.json"
}

kilo_case() {
    local old mode
    case "$1" in
        cold) old=current; mode=only ;;
        upgrade) old=previous; mode=0 ;;
        *) exit 2 ;;
    esac
    CCCC_TEST_KILO_EARLY="$mode" bash "$work/current/scripts/tests/macos_maintenance/coverage/real_cli/run.sh" \
        kilo "$work/bin/$old" "$work/bin/current" "$evidence/kilo-$1" "$work/inspect-process"
    test -s "$evidence/kilo-$1/kilo/result.json"
}

case "${1:-}" in
    prepare) prepare ;;
    build) build "${2:?previous or current}" ;;
    case) run_case "${2:?daemon, local or release}" ;;
    kilo) kilo_case "${2:?cold or upgrade}" ;;
    *) echo 'Expected prepare, build ROLE, case ENTRY, or kilo CASE' >&2; exit 2 ;;
esac
