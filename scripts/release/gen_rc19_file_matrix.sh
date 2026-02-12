#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
OUT_CSV="$ROOT_DIR/docs/release/rc19_file_matrix.csv"

mkdir -p "$(dirname "$OUT_CSV")"

declare -A existing_status
declare -A existing_owner
declare -A existing_notes

# Preserve prior review progress when regenerating.
if [[ -f "$OUT_CSV" ]]; then
  while IFS=',' read -r path _tier _domain _mode status owner notes; do
    if [[ -z "$path" || "$path" == "path" ]]; then
      continue
    fi
    existing_status["$path"]="${status:-pending}"
    existing_owner["$path"]="${owner:-unassigned}"
    existing_notes["$path"]="${notes:-}"
  done < "$OUT_CSV"
fi

classify_tier() {
  local p="$1"
  case "$p" in
    src/cccc/*|web/src/*|.github/workflows/*|pyproject.toml|README.md|README.zh-CN.md|README.ja.md)
      echo "A"
      ;;
    tests/*|scripts/*|docker/*|docs/guide/*|docs/reference/*|docs/standards/*|docs/index.md|docs/.vitepress/*|docs/vnext/RELEASE.md|docs/vnext/STATUS.md|docs/vnext/FEATURES.md|docs/vnext/README.md)
      echo "B"
      ;;
    docs/vnext/archive/*|old_v0.3.28/*|dist/*)
      echo "C"
      ;;
    docs/*)
      echo "B"
      ;;
    *)
      echo "B"
      ;;
  esac
}

classify_domain() {
  local p="$1"
  case "$p" in
    src/cccc/contracts/*) echo "contracts" ;;
    src/cccc/kernel/*) echo "kernel" ;;
    src/cccc/daemon/*) echo "daemon" ;;
    src/cccc/ports/web/*) echo "port-web" ;;
    src/cccc/ports/mcp/*) echo "port-mcp" ;;
    src/cccc/ports/im/*) echo "port-im" ;;
    src/cccc/runners/*) echo "runners" ;;
    src/cccc/*) echo "core-other" ;;
    web/src/*) echo "web-ui" ;;
    tests/*) echo "tests" ;;
    docs/standards/*) echo "docs-standards" ;;
    docs/reference/*) echo "docs-reference" ;;
    docs/guide/*) echo "docs-guide" ;;
    docs/*) echo "docs-other" ;;
    .github/workflows/*) echo "ci-release" ;;
    scripts/*) echo "ops-scripts" ;;
    docker/*) echo "docker" ;;
    *) echo "misc" ;;
  esac
}

review_mode_for_tier() {
  local tier="$1"
  case "$tier" in
    A) echo "deep-file+function" ;;
    B) echo "standard-file" ;;
    C) echo "light-risk-only" ;;
    *) echo "standard-file" ;;
  esac
}

{
  echo "path,tier,domain,review_mode,status,owner,notes"
  git -C "$ROOT_DIR" ls-files --cached --others --exclude-standard | LC_ALL=C sort -u | while IFS= read -r path; do
    case "$path" in
      .venv/*|.venv-*/*|venv/*)
        continue
        ;;
    esac
    tier="$(classify_tier "$path")"
    domain="$(classify_domain "$path")"
    mode="$(review_mode_for_tier "$tier")"
    status="${existing_status[$path]:-pending}"
    owner="${existing_owner[$path]:-unassigned}"
    notes="${existing_notes[$path]:-}"
    echo "$path,$tier,$domain,$mode,$status,$owner,$notes"
  done
} > "$OUT_CSV"

echo "Generated: $OUT_CSV"
echo "Rows: $(($(wc -l < "$OUT_CSV") - 1))"
echo "Tier counts:"
awk -F',' 'NR>1{c[$2]++} END{for(k in c) printf "  %s: %d\n", k, c[k]}' "$OUT_CSV" | LC_ALL=C sort
