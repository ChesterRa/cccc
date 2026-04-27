#!/usr/bin/env bash
set -euo pipefail

ROOT="$(git rev-parse --show-toplevel)"
cd "$ROOT"

full=0
if [[ "${CCCC_PRECOMMIT_FULL:-}" == "1" || "${PRECOMMIT_FULL:-}" == "1" ]]; then
  full=1
fi
if [[ "${1:-}" == "--all" || "${1:-}" == "--full" ]]; then
  full=1
fi

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
done < <(git diff --cached --name-only --diff-filter=ACMR)
if [[ ${#changed_files[@]} -eq 0 ]]; then
  staged=0
  while IFS= read -r file; do
    append_changed_file "$file"
  done < <(git diff --name-only --diff-filter=ACMR)
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
  echo "Running web lint and typecheck..."
  npm -C web run lint
  npm -C web run typecheck
  echo "✓ Web checks passed"
  echo ""
}

run_full_python_tests() {
  local pytest_workers="${PYTEST_WORKERS:-auto}"

  echo "Running full Python tests with pytest-xdist (-n ${pytest_workers})..."
  if uv run --with pytest-xdist python -m pytest tests/ -x -q -n "${pytest_workers}"; then
    echo "✓ Full Python tests passed"
    echo ""
    return
  fi

  echo "Parallel pytest failed, falling back to serial run..."
  uv run python -m pytest tests/ -x -q
  echo "✓ Full Python tests passed"
  echo ""
}

append_unique_test() {
  local candidate="$1"
  [[ -f "$candidate" ]] || return 0
  local existing
  if [[ ${#python_tests[@]} -gt 0 ]]; then
    for existing in "${python_tests[@]}"; do
      [[ "$existing" == "$candidate" ]] && return 0
    done
  fi
  python_tests+=("$candidate")
}

run_targeted_python_tests() {
  if [[ ${#python_tests[@]} -eq 0 ]]; then
    echo "Skipping Python tests; no impacted test files found."
    echo ""
    return
  fi

  echo "Running impacted Python tests:"
  printf '  %s\n' "${python_tests[@]}"
  uv run python -m pytest -q "${python_tests[@]}"
  echo "✓ Impacted Python tests passed"
  echo ""
}

if [[ "$full" == "1" ]]; then
  echo "=== Pre-commit checks (full) ==="
  echo ""
  run_whitespace_check
  run_frontend_checks
  run_full_python_tests
  echo "All checks passed."
  exit 0
fi

echo "=== Pre-commit checks (impacted) ==="
echo ""

if [[ ${#changed_files[@]} -eq 0 ]]; then
  echo "No staged or working-tree file changes found."
  exit 0
fi

needs_web=0
needs_python=0
needs_full_python=0
python_tests=()

for file in "${changed_files[@]}"; do
  case "$file" in
    web/*)
      needs_web=1
      ;;
    package.json|package-lock.json|npm-shrinkwrap.json)
      needs_web=1
      ;;
    tests/*.py)
      needs_python=1
      append_unique_test "$file"
      ;;
    src/cccc/daemon/codex_app_sessions.py|src/cccc/daemon/claude_app_sessions.py)
      needs_python=1
      append_unique_test "tests/test_codex_app_flow.py"
      ;;
    src/cccc/daemon/space/*|src/cccc/providers/notebooklm/*)
      needs_python=1
      append_unique_test "tests/test_group_space_ops.py"
      ;;
    src/cccc/**/*.py|src/cccc/*.py|pyproject.toml|uv.lock)
      needs_python=1
      needs_full_python=1
      ;;
    *.py)
      needs_python=1
      needs_full_python=1
      ;;
  esac
done

run_whitespace_check

if [[ "$needs_web" == "1" ]]; then
  run_frontend_checks
else
  echo "Skipping web checks; no web files changed."
  echo ""
fi

if [[ "$needs_python" == "1" ]]; then
  if [[ "$needs_full_python" == "1" ]]; then
    run_full_python_tests
  else
    run_targeted_python_tests
  fi
else
  echo "Skipping Python tests; no Python files changed."
  echo ""
fi

echo "All impacted checks passed."
