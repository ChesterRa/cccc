#!/bin/bash
# Run one named maintenance case in disposable homes and return its exit status.
set -euo pipefail
umask 077
if [ "$#" -eq 1 ]; then
    binaries="${RUNNER_TEMP:?}/cccc-programs"
    set -- "$1" "$binaries/native-previous-program/cccc" "$binaries/native-current-program/cccc" \
        "$RUNNER_TEMP/cccc-evidence/$1" "$RUNNER_TEMP/inspect-process" \
        "${GITHUB_WORKSPACE:?}/scripts/tests/macos_maintenance/maintenance_mcp_member.py"
    mkdir -p "$4"
    bash "$0" "$@" 2>&1 | tee "$4/driver.log"
    exit
fi
case_name=${1:?case name}
old=${2:?previous executable}
new=${3:?current executable}
out=${4:?evidence directory}
inspector=${5-}
member=${6:?terminal fixture executable}
here=$(cd "$(dirname "$0")/.." && pwd)
python=$(command -v python3)
mkdir -p "$out"
test -x "$old"
test -x "$new"
test -f "$member"
if [ "$(uname -s)" = Darwin ]; then test -x "$inspector"; fi
case "$case_name" in
    managed)
        exec bash "$here/portable/run.sh" "$old" "$new" "$out" "$inspector"
        ;;
    pi|kilo|opencode|kimi|copilot)
        exec bash "$here/real_cli/run.sh" "$case_name" "$old" "$new" "$out" "$inspector"
        ;;
    pty|browser|browser-local|browser-release|local|release|same-build|recovery) ;;
    *) echo "Unknown maintenance case: $case_name" >&2; exit 2 ;;
esac

root=$(mktemp -d /tmp/cccc-isolated.XXXXXXXX)
trap 'rm -rf -- "$root"' EXIT
mkdir -p "$root"/{home,tmp,cccc,codex,claude}
private=(env -i PATH=/usr/bin:/bin HOME="$root/home" TMPDIR="$root/tmp"
    CCCC_HOME="$root/cccc" CODEX_HOME="$root/codex" CLAUDE_CONFIG_DIR="$root/claude"
    CCCC_TEST_SDK_INSPECTOR="$inspector" LANG=C)
case "$case_name" in
    pty)
        "${private[@]}" "$python" -I "$here/cold/pty.py" --program "$old" --replacement "$new" \
            --shared-fixture "$here/portable/update_keep.py" --standin "$member" \
            --sdk-identity "$inspector" --result "$out/result.json"
        ;;
    browser|browser-local|browser-release)
        browser_case=daemon
        if [ "$case_name" != browser ]; then browser_case=${case_name#browser-}; fi
        if [ "$(uname -s)" = Darwin ]; then
            if [ "$(cat "$RUNNER_TEMP/cccc-browser-available")" != yes ]; then
                echo 'Browser maintenance not exercised: neither isolated nor system LaunchServices opened a private Chrome on this runner.' >&2
                exit 1
            fi
            private+=(CCCC_TEST_BROWSER_ENV_FILE="$RUNNER_TEMP/cccc-browser-environment.json")
        fi
        if [ "$(uname -s)" = Linux ]; then
            private=(bwrap --unshare-net --dev-bind / / --ro-bind
                "$here/wave/headless_chrome" /opt/google/chrome/google-chrome -- "${private[@]}")
        fi
        "${private[@]}" "$python" -I "$here/cold/busy.py" "$old" "$new" "$out/results" "$browser_case"
        ;;
    same-build)
        "${private[@]}" "$python" -I "$(dirname "$member")/daemon_keep.py" --program "$new" \
            --shared-fixture "$here/portable/update_keep.py" --standin "$member" \
            --sdk-identity "$inspector" --result "$out/result.json"
        ;;
    local|release)
        "${private[@]}" "$python" -I "$(dirname "$member")/update_keep.py" "$old" "$new" "$out" "$case_name"
        ;;
    recovery)
        "${private[@]}" "$python" -I "$here/portable/recovery.py" \
            "$old" "$new" "$member" "$out/results"
        ;;
esac
