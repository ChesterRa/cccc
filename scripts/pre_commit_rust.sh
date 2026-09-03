#!/usr/bin/env bash
set -euo pipefail

ROOT="$(git rev-parse --show-toplevel)"
cd "$ROOT"

full=0
dry_run=0
rust_files=()
reading_files=0
for arg in "$@"; do
  if [[ "$reading_files" == "1" ]]; then
    rust_files+=("$arg")
    continue
  fi
  case "$arg" in
    --full)
      full=1
      ;;
    --dry-run)
      dry_run=1
      ;;
    --)
      reading_files=1
      ;;
    *)
      echo "usage: scripts/pre_commit_rust.sh [--full] [--dry-run] -- [changed files...]" >&2
      exit 2
      ;;
  esac
done

cargo_jobs="${CCCC_CARGO_JOBS:-2}"
if [[ ! "$cargo_jobs" =~ ^[1-9][0-9]*$ ]]; then
  echo "CCCC_CARGO_JOBS must be a positive integer" >&2
  exit 2
fi

rust_packages=()
rust_source_packages=()
rust_test_specs=()
rust_workspace=0
daemon_impacted=0
runtime_impacted=0

append_unique_package() {
  local candidate="$1"
  local existing
  [[ -n "$candidate" ]] || return 0
  if [[ ${#rust_packages[@]} -gt 0 ]]; then
    for existing in "${rust_packages[@]}"; do
      [[ "$existing" == "$candidate" ]] && return 0
    done
  fi
  rust_packages+=("$candidate")
}

append_unique_source_package() {
  local candidate="$1"
  local existing
  [[ -n "$candidate" ]] || return 0
  if [[ ${#rust_source_packages[@]} -gt 0 ]]; then
    for existing in "${rust_source_packages[@]}"; do
      [[ "$existing" == "$candidate" ]] && return 0
    done
  fi
  rust_source_packages+=("$candidate")
}

append_unique_test_spec() {
  local candidate="$1"
  local existing
  [[ -n "$candidate" ]] || return 0
  if [[ ${#rust_test_specs[@]} -gt 0 ]]; then
    for existing in "${rust_test_specs[@]}"; do
      [[ "$existing" == "$candidate" ]] && return 0
    done
  fi
  rust_test_specs+=("$candidate")
}

mark_rust_change() {
  local file="$1"
  local relative crate_dir crate_relative manifest package_name test_relative test_name

  case "$file" in
    Cargo.toml|Cargo.lock|rust-toolchain|rust-toolchain.toml|.cargo/*|crates/third-party/*)
      rust_workspace=1
      return
      ;;
    crates/*)
      relative="${file#crates/}"
      crate_dir="${relative%%/*}"
      manifest="crates/$crate_dir/Cargo.toml"
      if [[ ! -f "$manifest" ]]; then
        rust_workspace=1
        return
      fi
      package_name="$(sed -n 's/^name = "\([^"]*\)"/\1/p' "$manifest" | head -n 1)"
      if [[ -z "$package_name" ]]; then
        rust_workspace=1
        return
      fi
      append_unique_package "$package_name"
      if [[ "$package_name" == "cccc-pair-daemon" ]]; then
        daemon_impacted=1
      elif [[ "$package_name" == "cccc-pair-runtime" ]]; then
        runtime_impacted=1
      fi
      crate_relative="${relative#*/}"
      case "$crate_relative" in
        tests/*.rs)
          test_relative="${crate_relative#tests/}"
          if [[ "$test_relative" != */* && ! -f "$file" ]]; then
            append_unique_source_package "$package_name"
          elif [[ "$test_relative" != */* ]]; then
            test_name="${test_relative%.rs}"
            append_unique_test_spec "$package_name:$test_name"
          elif [[ "$test_relative" == suite/* && -f "crates/$crate_dir/tests/integration.rs" ]]; then
            append_unique_test_spec "$package_name:integration"
          else
            rust_workspace=1
          fi
          ;;
        *)
          append_unique_source_package "$package_name"
          ;;
      esac
      ;;
    *.rs)
      rust_workspace=1
      ;;
  esac
}

if [[ "$full" == "1" ]]; then
  rust_workspace=1
elif [[ ${#rust_files[@]} -gt 0 ]]; then
  for file in "${rust_files[@]}"; do
    mark_rust_change "$file"
  done
fi

rust_source_scope_args=()
if [[ "$rust_workspace" == "1" ]]; then
  rust_source_scope_args+=("--workspace")
elif [[ ${#rust_source_packages[@]} -gt 0 ]]; then
  for package_name in "${rust_source_packages[@]}"; do
    rust_source_scope_args+=("--package" "$package_name")
  done
fi

scope_label=""
if [[ "$rust_workspace" == "1" ]]; then
  scope_label="workspace"
elif [[ ${#rust_packages[@]} -gt 0 ]]; then
  scope_label="${rust_packages[*]}"
fi

rust_lint_command=()
rust_test_command=()
rust_runtime_test_command=()
rust_daemon_test_command=()
if [[ "$full" == "1" ]]; then
  rust_lint_command=(cargo clippy --workspace --all-targets --locked --jobs "$cargo_jobs" -- -D warnings)
elif [[ ${#rust_source_scope_args[@]} -gt 0 ]]; then
  rust_lint_command=(cargo clippy "${rust_source_scope_args[@]}" --locked --jobs "$cargo_jobs" -- -D warnings)
fi
if [[ "$full" == "1" || "$rust_workspace" == "1" || "$daemon_impacted" == "1" || "$runtime_impacted" == "1" ]]; then
  rust_test_command=(cargo test --workspace --exclude cccc-pair-daemon --exclude cccc-pair-runtime --locked --jobs "$cargo_jobs")
  rust_runtime_test_command=(cargo test --package cccc-pair-runtime --locked --jobs "$cargo_jobs" -- --test-threads=1)
  rust_daemon_test_command=(cargo test --package cccc-pair-daemon --locked --jobs "$cargo_jobs" -- --test-threads=1)
elif [[ ${#rust_source_scope_args[@]} -gt 0 ]]; then
  rust_test_command=(cargo test "${rust_source_scope_args[@]}" --locked --jobs "$cargo_jobs")
fi

changed_test_lint_command=()
changed_test_command=()
plan_changed_test() {
  local test_spec="$1"
  local package_name="${test_spec%%:*}"
  local test_name="${test_spec#*:}"
  changed_test_lint_command=(cargo clippy --package "$package_name" --test "$test_name" --locked --jobs "$cargo_jobs" -- -D warnings)
  changed_test_command=(cargo test --package "$package_name" --test "$test_name" --locked --jobs "$cargo_jobs")
  if [[ "$package_name" == "cccc-pair-daemon" || "$package_name" == "cccc-pair-runtime" ]]; then
    changed_test_command+=(-- --test-threads=1)
  fi
}

print_command() {
  local label="$1"
  shift
  [[ $# -gt 0 ]] || return 0
  printf '%s=' "$label"
  printf '%q ' "$@"
  echo ""
}

print_plan() {
  echo "cargo_jobs=$cargo_jobs"
  echo "rust_scope=$scope_label"
  if [[ "$full" == "1" ]]; then
    echo "rust_targets=all"
  else
    echo "rust_targets=default,changed-tests"
  fi
  print_command rust_clippy "${rust_lint_command[@]}"
  print_command rust_test "${rust_test_command[@]}"
  print_command rust_runtime_test "${rust_runtime_test_command[@]}"
  print_command rust_daemon_test "${rust_daemon_test_command[@]}"
  if [[ ${#rust_test_specs[@]} -gt 0 ]]; then
    for test_spec in "${rust_test_specs[@]}"; do
      plan_changed_test "$test_spec"
      print_command "rust_changed_test_lint[$test_spec]" "${changed_test_lint_command[@]}"
      print_command "rust_changed_test[$test_spec]" "${changed_test_command[@]}"
    done
  fi
}

if [[ "$dry_run" == "1" ]]; then
  print_plan
  exit 0
fi

run_timed() {
  local label="$1"
  shift
  local started=$SECONDS
  "$@"
  echo "✓ $label completed in $((SECONDS - started))s"
}

echo "Running Rust format, lint, and tests for: $scope_label"
echo "Cargo build jobs: $cargo_jobs"
run_timed "Rust format" cargo fmt --all --check

if [[ ${#rust_lint_command[@]} -gt 0 ]]; then
  run_timed "Rust lint" "${rust_lint_command[@]}"
fi
if [[ ${#rust_test_command[@]} -gt 0 ]]; then
  run_timed "Rust tests" "${rust_test_command[@]}"
fi
if [[ ${#rust_runtime_test_command[@]} -gt 0 ]]; then
  run_timed "Rust runtime tests" "${rust_runtime_test_command[@]}"
fi
if [[ ${#rust_daemon_test_command[@]} -gt 0 ]]; then
  run_timed "Rust daemon tests" "${rust_daemon_test_command[@]}"
fi

if [[ ${#rust_test_specs[@]} -gt 0 ]]; then
  for test_spec in "${rust_test_specs[@]}"; do
    package_name="${test_spec%%:*}"
    test_name="${test_spec#*:}"
    plan_changed_test "$test_spec"
    run_timed "$package_name/$test_name lint" "${changed_test_lint_command[@]}"
    run_timed "$package_name/$test_name test" "${changed_test_command[@]}"
  done
fi

echo "Rust checks passed"
echo ""
