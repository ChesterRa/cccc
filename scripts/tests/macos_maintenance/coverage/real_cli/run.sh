#!/bin/bash
set -euo pipefail
runtime=${1:?runtime name}
case "$runtime" in pi|kilo|opencode|kimi|copilot) ;; *) exit 2 ;; esac
shift
old=${1:?old program}
new=${2:?new program}
out=${3:?evidence directory}
inspect=${4:-}
here=$(cd "$(dirname "$0")" && pwd)
python=$(command -v python3)
npm_exe=$(command -v npm)
node_dir=$(dirname "$(command -v node)")
if [ "$(uname -s)" = Darwin ]; then test -x "$inspect"; fi
mkdir -p "$out"
fixture_root=$(mktemp -d /tmp/cccc-isolated.XXXXXXXX)
cleanup() {
    if [ "$?" -eq 0 ] || [ "${CCCC_TEST_KEEP_FAILED_PROFILE:-0}" != 1 ]; then
        rm -rf -- "$fixture_root"
    else
        printf 'Failed isolated fixture retained in %s\n' "$fixture_root" >&2
    fi
}
trap cleanup EXIT
mkdir -p "$fixture_root"/{home,cccc,codex,claude,tmp,pkg}
private=(env -i PATH="$node_dir:/usr/bin:/bin" HOME="$fixture_root/home"
    CCCC_HOME="$fixture_root/cccc" CODEX_HOME="$fixture_root/codex"
    CLAUDE_CONFIG_DIR="$fixture_root/claude" TMPDIR="$fixture_root/tmp"
    LANG=C CCCC_TEST_KILO_EARLY="${CCCC_TEST_KILO_EARLY:-0}" CCCC_TEST_SDK_INSPECTOR="$inspect")
cp "$here/$runtime"/{package,package-lock}.json "$fixture_root/pkg/"
(cd "$fixture_root/pkg" && "${private[@]}" "$npm_exe" ci --ignore-scripts --no-audit --no-fund) >"$out/npm.log" 2>&1
"${private[@]}" HTTP_PROXY=http://127.0.0.1:1 HTTPS_PROXY=http://127.0.0.1:1 NO_PROXY=127.0.0.1,localhost \
    "$python" -I "$here/managed_cli.py" "$runtime" "$old" "$new" "$here/../portable/mock_model.py" "$fixture_root/pkg" "$out/$runtime"
