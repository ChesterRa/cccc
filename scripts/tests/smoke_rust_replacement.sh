#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
BINARY="${1:-$ROOT_DIR/target/release/cccc}"
test -x "$BINARY"
command -v node >/dev/null 2>&1 || {
  echo "node is required for the code_exec replacement smoke" >&2
  exit 1
}

SMOKE_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/cccc-rust-replacement.XXXXXX")"
cleanup() {
  CCCC_HOME="$SMOKE_ROOT/home" "$BINARY" daemon stop >/dev/null 2>&1 || true
  rm -rf "$SMOKE_ROOT"
}
trap cleanup EXIT

export CCCC_HOME="$SMOKE_ROOT/home"

offline_status="$($BINARY status)"
grep -Fq 'Selected:    rust' <<<"$offline_status"
grep -Fq 'Daemon:      stopped' <<<"$offline_status"

"$BINARY" daemon start >/dev/null
mkdir -p "$SMOKE_ROOT/project"
group_json="$($BINARY attach "$SMOKE_ROOT/project")"
group_id="$(sed -n 's/.*"group_id": "\([^"]*\)".*/\1/p' <<<"$group_json" | head -1)"
test -n "$group_id"
"$BINARY" actor add web-smoke \
  --group "$group_id" \
  --runtime web_model \
  --runner headless >/dev/null

mcp_output="$(
  printf '%s\n' \
    '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}' \
    '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"cccc_code_exec","arguments":{"source":"const repo = await tools.cccc_repo({action: \"info\"}); text(repo.root ? \"rust replacement nested-tool smoke\" : \"missing root\");"}}}' |
    CCCC_GROUP_ID="$group_id" \
    CCCC_ACTOR_ID=web-smoke \
    CCCC_MCP_TOOL_PROFILE=full \
    "$BINARY" mcp
)"
grep -Fq '"name":"cccc-mcp"' <<<"$mcp_output"
grep -Fq 'rust replacement nested-tool smoke' <<<"$mcp_output"

"$BINARY" daemon stop >/dev/null
stopped_status="$($BINARY status)"
grep -Fq 'Daemon:      stopped' <<<"$stopped_status"

echo "OK: installed Rust CLI, daemon, offline status, MCP, and code_exec"
