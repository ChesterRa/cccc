#!/bin/bash
# Run the three user entry points independently on an isolated runner.
set -eu

if [ "$#" -ne 4 ]; then
    echo "usage: bash run_lifecycle.sh OLD NEW OUTPUT INSPECTOR" >&2
    exit 2
fi
old=$1
new=$2
out=$3
inspector=$4
here=$(cd "$(dirname "$0")" && pwd)
daemon_probe="$here/daemon_keep.py"
update_probe="$here/update_keep.py"
member="$here/maintenance_mcp_member.py"
python=$(command -v python3)
mkdir -p "$out"
root=$(mktemp -d /tmp/cccc-isolated.XXXXXXXX)
mkdir "$root/home" "$root/tmp" "$root/cccc" "$root/codex" "$root/claude"

phase() {
    local name=$1
    shift
    local rc=0
    env -i PATH=/usr/bin:/bin HOME="$root/home" TMPDIR="$root/tmp" \
        CCCC_HOME="$root/cccc" CODEX_HOME="$root/codex" CLAUDE_CONFIG_DIR="$root/claude" \
        CCCC_TEST_SDK_INSPECTOR="$inspector" LANG=C \
        "$python" -I "$@" > "$out/$name.log" 2>&1 || rc=$?
    echo "$rc" > "$out/$name.rc"
    if [ "$rc" -ne 0 ]; then failed=1; fi
}

failed=0
phase daemon_keep "$daemon_probe" --program "$new" --shared-fixture "$update_probe" \
    --standin "$member" --sdk-identity "$inspector" --result "$out/daemon_keep.json"
for action in local release; do
    phase "$action" "$update_probe" "$old" "$new" "$out" "$action"
done
if [ "$failed" -eq 0 ]; then
    rm -r "$root"
else
    echo "Isolated fixture kept at $root" >&2
fi
exit "$failed"
