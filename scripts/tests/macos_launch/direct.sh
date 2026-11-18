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

selected_mode() {
    python3 -I - "$evidence/navigation/selection.json" <<'PYMODE'
import json, sys
with open(sys.argv[1]) as source:
    result = json.load(source)
mode = result["selected"]
assert mode in ("standard", "mock", "runner-home") and result["ready"][mode] is True
print(mode)
PYMODE
}

chrome_mode=$(selected_mode)

prepare() {
    [[ $(uname -s) == Darwin && $(uname -m) == arm64 ]]
    [[ -x "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome" ]]
    mkdir "$work/previous" "$work/current" "$work/bin"
    environment | tee "$evidence/environment.txt"
    printf '%s\n' "$GITHUB_SHA" > "$evidence/fixture-commit.txt"
    for role in previous current; do
        if [[ $role == previous ]]; then ref=$previous; else ref=$current; fi
        git -C "$checkout" archive "$ref" | tar -x -C "$work/$role"
        printf '%s\n' "$ref" > "$evidence/$role-commit.txt"
    done
    (cd "$work/previous" && git apply --check "$patches/previous-direct.patch" && git apply "$patches/previous-direct.patch")
    if [[ $chrome_mode == mock ]]; then
        (cd "$work/previous" && git apply --check "$patches/previous-test-keychain.patch" && git apply "$patches/previous-test-keychain.patch")
    fi
    if [[ $chrome_mode == runner-home ]]; then
        (cd "$work/previous" && git apply --check "$patches/previous-runner-home.patch" && git apply "$patches/previous-runner-home.patch")
    fi
    shasum -a 256 "$patches/previous-direct.patch" "$patches/previous-test-keychain.patch" "$patches/previous-runner-home.patch" > "$evidence/patches.sha256"
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
        previous)
            case "$chrome_mode" in
                mock) expected=12930903a772c0c7882ba0244113ba4593d7567cb24235fe9542196676bdb2fe ;;
                runner-home) expected=5929d3cddab561eb5a46f40416c13d17940ac5e89dd066df1a46f8cd289fd822 ;;
                standard) expected=e4fa535f1447de4d08685eaa293bd6809d5339b6d2ae7f9e60f50280996135ff ;;
            esac ;;
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
        CCCC_TEST_CHROME_MODE="$chrome_mode" \
        CCCC_TEST_NODE="$(command -v node)" CCCC_TEST_PREVIOUS_DIST="$work/previous/web/dist" \
        CCCC_TEST_SDK_INSPECTOR="$work/inspect-process" "$py" -BI \
        scripts/tests/macos_maintenance/busy.py "$work/bin/previous" "$work/bin/current" "$out" "$kind" \
        2>&1 | tee "$evidence/$kind-driver.log"
    test -s "$out/$kind/result.json"
}

case "${1:-}" in
    prepare) prepare ;;
    build) build "${2:?previous or current}" ;;
    case) run_case "${2:?daemon, local or release}" ;;
    *) echo 'Expected prepare, build ROLE, or case ENTRY' >&2; exit 2 ;;
esac
