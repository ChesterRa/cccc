#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"
package_version="$(sed -n 's/^version = "\([^"]*\)"/\1/p' "$ROOT_DIR/pyproject.toml" | head -1)"
rust_version="$(sed -n 's/^version = "\([^"]*\)"/\1/p' "$ROOT_DIR/Cargo.toml" | head -1)"

if [[ -z "$package_version" || -z "$rust_version" ]]; then
  echo "failed to read CCCC versions from pyproject.toml and Cargo.toml" >&2
  exit 1
fi

if command -v python >/dev/null 2>&1; then
  version_python=python
elif command -v python3 >/dev/null 2>&1; then
  version_python=python3
else
  echo "python is required to validate CCCC release versions" >&2
  exit 1
fi
"$version_python" "$ROOT_DIR/scripts/check_release_versions.py" \
  --package-version "$package_version" \
  --rust-version "$rust_version" >/dev/null

for manifest in "$ROOT_DIR"/crates/cccc-*/Cargo.toml; do
  crate_name="$(sed -n 's/^name = "\([^"]*\)"/\1/p' "$manifest" | head -1)"
  if ! sed -n '/^\[package\]$/,/^\[/p' "$manifest" | grep -qx 'version.workspace = true'; then
    echo "CCCC version mismatch: ${crate_name:-$manifest} must inherit workspace.package.version" >&2
    exit 1
  fi

  lock_version="$(sed -n "/^name = \"$crate_name\"$/,/^\[\[package\]\]$/s/^version = \"\([^\"]*\)\"/\1/p" "$ROOT_DIR/Cargo.lock" | head -1)"
  if [[ "$lock_version" != "$rust_version" ]]; then
    echo "CCCC lockfile version mismatch: ${crate_name:-$manifest}=$lock_version, expected $rust_version" >&2
    exit 1
  fi

  while IFS= read -r dependency_version; do
    if [[ "$dependency_version" != "=$rust_version" ]]; then
      echo "CCCC dependency version mismatch in $manifest: expected =$rust_version, found $dependency_version" >&2
      exit 1
    fi
  done < <(sed -n 's/.*package = "cccc-pair-[^"]*".*version = "\([^"]*\)".*/\1/p' "$manifest")
done

command -v cargo >/dev/null 2>&1 || {
  echo "cargo is required to validate Cargo.lock version parity" >&2
  exit 1
}
cargo metadata --locked --format-version 1 --no-deps >/dev/null

echo "CCCC version parity: $rust_version"
