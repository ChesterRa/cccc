#!/bin/bash
# Isolate the protocol stand-ins, pinned DeepSeek executable and its localhost model.
set -eu
old=${1:?old binary}
new=${2:?new binary}
out=${3:?absolute output directory}
inspector=${4:-}
here=$(cd "$(dirname "$0")" && pwd)
python=$(command -v python3)
node_dir=$(dirname "$(command -v node)")
npm_path=$(command -v npm)
test -x "$old" && test -x "$new" && test -x "$npm_path"
if [ "$(uname -s)" = Darwin ]; then test -x "$inspector"; fi
mkdir -p "$out"
root=$(mktemp -d /tmp/cccc-isolated.XXXXXXXX)
trap 'rm -rf -- "$root"' EXIT
mkdir -p "$root/home" "$root/tmp" "$root/cccc" "$root/codex" "$root/pinned-node"
private=(env -i PATH="$node_dir:/usr/bin:/bin" HOME="$root/home" CCCC_HOME="$root/cccc"
    CODEX_HOME="$root/codex" TMPDIR="$root/tmp" USER="$(id -un)" LANG=C
    CCCC_TEST_SDK_INSPECTOR="$inspector")
copy_cache=${CCCC_TEST_DEEPSEEK_CACHE:-}
if [ -z "$copy_cache" ]; then
    cp "$here"/deepseek/{package,package-lock}.json "$root/pinned-node/"
    "${private[@]}" "$npm_path" ci --ignore-scripts --no-audit --no-fund --prefix "$root/pinned-node" >"$out/npm.log" 2>&1
    copy_cache="$root/pinned-node"
fi
failed=0
for phase in structured real_deepseek; do
    if [ "$phase" = structured ]; then
        args=("$here/managed.py" "$old" "$new" "$out/structured")
    else
        args=("$here/real_deepseek.py" "$old" "$new" "$here/mock_model.py" "$out/real_deepseek")
    fi
    rc=0
    "${private[@]}" CCCC_TEST_DEEPSEEK_CACHE="$copy_cache" HTTP_PROXY=http://127.0.0.1:1 HTTPS_PROXY=http://127.0.0.1:1 \
        NO_PROXY=127.0.0.1,localhost "$python" -I "${args[@]}" >"$out/$phase.log" 2>&1 || rc=$?
    echo "$rc" >"$out/$phase.rc"
    tail -n 8 "$out/$phase.log"
    [ "$rc" -eq 0 ] || failed=1
done
exit "$failed"
