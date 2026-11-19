#!/bin/bash
set -euo pipefail
runtime=${1:?runtime name}
case "$runtime" in
    pi|kilo|opencode|kimi|copilot|claude|codex|cline) install=npm ;;
    grok|antigravity|hermes) install=pinned ;;
    *) exit 2 ;;
esac
shift
old=${1:?old program}
new=${2:?new program}
out=${3:?evidence directory}
inspect=${4:-}
here=$(cd "$(dirname "$0")" && pwd)
python=$(command -v python3)
npm_exe=$(command -v npm)
node_dir=$(dirname "$(command -v node)")
fixture_path="$node_dir:/usr/bin:/bin"
if [ "$(uname -s)" = Darwin ]; then
    test -x "$inspect"
    # Include macOS system utilities such as ioreg in the isolated PATH.
    fixture_path="$fixture_path:/usr/sbin"
fi
mkdir -p "$out"
fixture_root=$(cd "$(mktemp -d /tmp/cccc-isolated.XXXXXXXX)" && pwd -P)
cleanup() {
    if [ "$?" -eq 0 ] || [ "${CCCC_TEST_KEEP_FAILED_PROFILE:-0}" != 1 ]; then
        rm -rf -- "$fixture_root"
    else
        printf 'Failed isolated fixture retained in %s\n' "$fixture_root" >&2
    fi
}
trap cleanup EXIT
mkdir -p "$fixture_root"/{home,cccc,codex,claude,tmp,pkg}
private=(env -i PATH="$fixture_path" HOME="$fixture_root/home"
    CCCC_HOME="$fixture_root/cccc" CODEX_HOME="$fixture_root/codex"
    CLAUDE_CONFIG_DIR="$fixture_root/claude" TMPDIR="$fixture_root/tmp"
    LANG=C CCCC_TEST_KILO_EARLY="${CCCC_TEST_KILO_EARLY:-0}" CCCC_TEST_SDK_INSPECTOR="$inspect")
# An install that fails ends the run with status 3; its own status and its log stay in the
# evidence directory.
install_started=$(date +%s)
if [ "$install" = npm ]; then
    cp "$here/$runtime"/{package,package-lock}.json "$fixture_root/pkg/"
    (cd "$fixture_root/pkg" && "${private[@]}" "$npm_exe" ci --ignore-scripts --no-audit --no-fund) >"$out/npm.log" 2>&1 \
        || { echo "$?" >"$out/install-status"; exit 3; }
else
    "${private[@]}" /bin/bash "$here/install_pinned.sh" "$runtime" "$fixture_root/pkg" "$python" >"$out/install.log" 2>&1 \
        || { echo "$?" >"$out/install-status"; exit 3; }
fi
echo "$(( $(date +%s) - install_started ))" >"$out/install-seconds"
# A record only: the installed size, which the cleanup deletes with the case.
du -sk "$fixture_root" 2>/dev/null | cut -f1 >"$out/install-kib" || true
"${private[@]}" HTTP_PROXY=http://127.0.0.1:1 HTTPS_PROXY=http://127.0.0.1:1 NO_PROXY=127.0.0.1,localhost \
    "$python" -I "$here/managed_cli.py" "$runtime" "$old" "$new" "$here/../portable/mock_model.py" "$fixture_root/pkg" "$out/$runtime"
