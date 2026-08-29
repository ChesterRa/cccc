#!/usr/bin/env bash
set -euo pipefail

ROOT="$(git rev-parse --show-toplevel)"
cd "$ROOT"

mode="${1:-fast}"
case "$mode" in
  fast)
    uvx ruff check scripts tests
    scripts/pre_commit_checks.sh
    ;;
  full)
    uvx ruff check scripts tests
    scripts/pre_commit_checks.sh --full
    ;;
  *)
    echo "usage: scripts/quality_gate.sh [fast|full]" >&2
    exit 2
    ;;
esac
