#!/bin/bash
# Build the previous probe commit without sharing the current checkout's target.
set -euo pipefail
umask 077
out=${1:?evidence directory required}
parent=$(git rev-parse HEAD^)
source_dir=$(mktemp -d "${RUNNER_TEMP:?}/cccc-previous.XXXXXXXX")
trap 'rm -rf "$source_dir"' EXIT
git archive "$parent" | tar -x -C "$source_dir"
mkdir -p "$out/bin"
printf '%s\n' "$parent" > "$out/previous_commit.txt"
(
    cd "$source_dir"
    npm ci --prefix web
    npm -C web run build
    CARGO_TARGET_DIR="$source_dir/target" cargo build --workspace --locked
    cp target/debug/cccc "$out/bin/cccc-previous"
    du -sk target/debug > "$out/previous_target_size.txt"
)
