#!/usr/bin/env bash
set -euo pipefail

ROOT="$(git rev-parse --show-toplevel)"
cd "$ROOT"
precommit_started=$SECONDS

full=0
dry_run=0
if [[ "${CCCC_PRECOMMIT_FULL:-}" == "1" || "${PRECOMMIT_FULL:-}" == "1" ]]; then
  full=1
fi
for arg in "$@"; do
  case "$arg" in
    --all|--full)
      full=1
      ;;
    --dry-run)
      dry_run=1
      ;;
    *)
      echo "usage: scripts/pre_commit_checks.sh [--full|--all] [--dry-run]" >&2
      exit 2
      ;;
  esac
done

fast_budget_seconds="${CCCC_PRECOMMIT_BUDGET_SECONDS:-120}"
if [[ ! "$fast_budget_seconds" =~ ^[1-9][0-9]*$ ]]; then
  echo "CCCC_PRECOMMIT_BUDGET_SECONDS must be a positive integer" >&2
  exit 2
fi
bash -n scripts/pre_commit_checks.sh scripts/pre_commit_rust.sh

staged=1
changed_files=()
append_changed_file() {
  local candidate="$1"
  local existing
  [[ -n "$candidate" ]] || return 0
  if [[ ${#changed_files[@]} -gt 0 ]]; then
    for existing in "${changed_files[@]}"; do
      [[ "$existing" == "$candidate" ]] && return 0
    done
  fi
  changed_files+=("$candidate")
}

while IFS= read -r file; do
  append_changed_file "$file"
done < <(git diff --cached --name-only --diff-filter=ACMRD)
if [[ ${#changed_files[@]} -eq 0 ]]; then
  staged=0
  while IFS= read -r file; do
    append_changed_file "$file"
  done < <(git diff --name-only --diff-filter=ACMRD)
  while IFS= read -r file; do
    append_changed_file "$file"
  done < <(git ls-files --others --exclude-standard)
fi

run_whitespace_check() {
  echo "Checking whitespace..."
  if [[ "$staged" == "1" ]]; then
    git diff --cached --check
  else
    git diff --check
  fi
  echo "✓ Whitespace check passed"
  echo ""
}

run_frontend_checks() {
  echo "Running web format, lint, and type checks..."
  npm -C web run check
  echo "✓ Web checks passed"
  echo ""
}

run_rust_checks() {
  local rust_args=()
  if [[ "$full" == "1" ]]; then
    rust_args+=("--full")
  fi
  rust_args+=("--")
  if [[ ${#rust_files[@]} -gt 0 ]]; then
    rust_args+=("${rust_files[@]}")
  fi
  scripts/pre_commit_rust.sh "${rust_args[@]}"
}

print_check_plan() {
  echo "=== Pre-commit check plan ==="
  echo "mode=$([[ "$full" == "1" ]] && echo full || echo impacted)"
  if [[ "$full" != "1" ]]; then
    echo "budget_seconds=$fast_budget_seconds"
  fi
  if [[ "$needs_rust" == "1" ]]; then
    local rust_args=("--dry-run")
    if [[ "$full" == "1" ]]; then
      rust_args+=("--full")
    fi
    rust_args+=("--")
    if [[ ${#rust_files[@]} -gt 0 ]]; then
      rust_args+=("${rust_files[@]}")
    fi
    scripts/pre_commit_rust.sh "${rust_args[@]}"
  else
    echo "rust_scope=skip"
  fi
  echo "web=$([[ "$needs_web" == "1" ]] && echo check || echo skip)"
  echo "tooling=$([[ "$needs_tooling" == "1" ]] && echo check || echo skip)"
}

run_tooling_checks() {
  echo "Checking release-tool syntax and SyntaxWarnings..."
  uv run --no-project --with pytest --with pyyaml \
    python -W error::SyntaxWarning -m compileall -q scripts tests
  echo "✓ Release-tool syntax check passed"
  echo ""

  echo "Running release-tool and repository contract tests..."
  env -u CCCC_GROUP_ID -u CCCC_ACTOR_ID \
    uv run --no-project --with pytest --with pyyaml python -m pytest -q
  echo "✓ Tooling tests passed"
  echo ""
}

if [[ ${#changed_files[@]} -eq 0 && "$full" != "1" ]]; then
  echo "No staged or working-tree file changes found."
  exit 0
fi

needs_web=0
needs_tooling=0
needs_rust=0
rust_files=()

if [[ ${#changed_files[@]} -gt 0 ]]; then
  for file in "${changed_files[@]}"; do
    case "$file" in
      web/*)
        needs_web=1
        ;;
      package.json|package-lock.json|npm-shrinkwrap.json)
        needs_web=1
        ;;
      .github/*|scripts/*|tests/*|docs/*|docker/*|pyproject.toml|Dockerfile*|README*.md|SUPPORT.md|.gitignore|.dockerignore)
        needs_tooling=1
        ;;
      Cargo.toml|Cargo.lock|rust-toolchain|rust-toolchain.toml|.cargo/*|crates/*|*.rs)
        needs_rust=1
        rust_files+=("$file")
        ;;
    esac
  done
fi

if [[ "$full" == "1" ]]; then
  needs_web=1
  needs_tooling=1
  needs_rust=1
fi

if [[ "$dry_run" == "1" ]]; then
  print_check_plan
  exit 0
fi

if [[ "$full" == "1" ]]; then
  echo "=== Pre-commit checks (full) ==="
  echo ""
  run_whitespace_check
  run_frontend_checks
  npm -C web test
  npm -C web run build
  run_tooling_checks
  run_rust_checks
  echo "All checks passed."
  exit 0
fi

echo "=== Pre-commit checks (impacted) ==="
echo ""

run_whitespace_check
if [[ "$needs_rust" == "1" ]]; then
  run_rust_checks
else
  echo "Skipping Rust checks; no Rust files changed."
  echo ""
fi

if [[ "$needs_web" == "1" ]]; then
  run_frontend_checks
else
  echo "Skipping web checks; no web files changed."
  echo ""
fi

if [[ "$needs_tooling" == "1" ]]; then
  run_tooling_checks
else
  echo "Skipping release-tool tests; no tooling or contract files changed."
  echo ""
fi

echo "All impacted checks passed."
elapsed=$((SECONDS - precommit_started))
echo "Impacted checks completed in ${elapsed}s (budget: ${fast_budget_seconds}s)."
if ((elapsed > fast_budget_seconds)); then
  echo "Warning: impacted checks exceeded the local feedback budget; inspect the timed steps above." >&2
fi
