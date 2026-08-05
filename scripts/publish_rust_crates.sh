#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MODE=${1:---plan}

if [ "$#" -gt 1 ] || { [ "$MODE" != "--plan" ] && [ "$MODE" != "--publish" ]; }; then
  echo "usage: publish_rust_crates.sh [--plan|--publish]" >&2
  exit 2
fi

cd "$ROOT_DIR"
"$ROOT_DIR/scripts/check_version_parity.sh" >/dev/null
VERSION="$(sed -n 's/^version = "\([^"]*\)"/\1/p' Cargo.toml | head -1)"

PACKAGES=(
  cccc-pair-contracts
  cccc-pair-notebooklm
  cccc-pair-runtime
  cccc-pair-core
  cccc-pair-client
  cccc-pair-daemon
  cccc-pair-mcp
  cccc-pair-web
  cccc
)

if [ "$MODE" = "--plan" ]; then
  for package in "${PACKAGES[@]}"; do
    printf '%s@%s\n' "$package" "$VERSION"
  done
  exit 0
fi

: "${CARGO_REGISTRY_TOKEN:?CARGO_REGISTRY_TOKEN is required for --publish}"
command -v curl >/dev/null 2>&1 || {
  echo "curl is required to verify crates.io publication" >&2
  exit 1
}

version_exists() {
  local package=$1
  curl --fail --silent --show-error \
    --user-agent "cccc-release/$VERSION" \
    "https://crates.io/api/v1/crates/$package/$VERSION" >/dev/null 2>&1
}

wait_until_resolvable() {
  local package=$1
  local attempt
  for attempt in $(seq 1 30); do
    if version_exists "$package" && cargo info --registry crates-io "$package@$VERSION" >/dev/null 2>&1; then
      return 0
    fi
    sleep 10
  done
  echo "$package@$VERSION was uploaded but did not become registry-resolvable in time" >&2
  return 1
}

for package in "${PACKAGES[@]}"; do
  if version_exists "$package"; then
    echo "SKIP: $package@$VERSION is already published"
  else
    echo "PUBLISH: $package@$VERSION"
    cargo publish --locked --package "$package"
  fi
  wait_until_resolvable "$package"
done

echo "OK: all CCCC Rust crates are available at version $VERSION"
