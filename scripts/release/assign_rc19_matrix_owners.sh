#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
MATRIX="$ROOT_DIR/docs/release/rc19_file_matrix.csv"
TMP="$MATRIX.tmp"

if [[ ! -f "$MATRIX" ]]; then
  echo "Matrix not found: $MATRIX" >&2
  echo "Run: ./scripts/release/gen_rc19_file_matrix.sh" >&2
  exit 1
fi

owner_for_domain() {
  case "$1" in
    contracts|kernel|daemon|runners|core-other) echo "core-platform" ;;
    port-mcp) echo "mcp-surface" ;;
    port-web|web-ui) echo "web-ux" ;;
    port-im) echo "im-bridge" ;;
    ci-release) echo "releng" ;;
    tests) echo "qa" ;;
    docs-standards|docs-reference|docs-guide|docs-other) echo "docs" ;;
    ops-scripts|docker|misc) echo "ops" ;;
    *) echo "ops" ;;
  esac
}

{
  IFS= read -r header
  echo "$header"
  while IFS=',' read -r path tier domain review_mode status owner notes; do
    new_owner="$owner"
    if [[ -z "${owner}" || "${owner}" == "unassigned" ]]; then
      new_owner="$(owner_for_domain "$domain")"
    fi
    echo "$path,$tier,$domain,$review_mode,$status,$new_owner,$notes"
  done
} < "$MATRIX" > "$TMP"

mv "$TMP" "$MATRIX"

echo "Updated owners in: $MATRIX"
echo "Owner counts:"
awk -F',' 'NR>1{c[$6]++} END{for(k in c) printf "  %s: %d\n", k, c[k]}' "$MATRIX" | LC_ALL=C sort

