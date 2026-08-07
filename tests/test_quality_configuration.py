from __future__ import annotations

import json
import subprocess
import tomllib
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


def _assert_vite_plus_package_contract(package: dict[str, object]) -> None:
    scripts = package["scripts"]
    expected_scripts = {
        "dev": "vp dev",
        "build": "vp build",
        "preview": "vp preview",
        "test": "vp test run",
        "check": "vp check && npm run typecheck",
        "lint": "vp lint src --deny-warnings",
        "lint:fix": "vp lint src --fix --deny-warnings",
        "typecheck": "tsc --noEmit -p tsconfig.json",
    }
    assert scripts == expected_scripts
    assert package["engines"] == {"node": "^20.19.0 || ^22.18.0 || >=24.11.0"}
    assert "devEngines" not in package

    dev_dependencies = package["devDependencies"]
    expected_toolchain = {
        "vite-plus": "0.2.4",
        "typescript": "^5.9.3",
    }
    assert {name: dev_dependencies[name] for name in expected_toolchain} == expected_toolchain
    assert "overrides" not in package

    removed_compatibility_dependencies = {
        "@eslint/js",
        "@voidzero-dev/vite-plus-core",
        "eslint",
        "eslint-plugin-react-hooks",
        "eslint-plugin-react-refresh",
        "globals",
        "typescript-eslint",
        "vite",
        "vitest",
    }
    installed = set(package["dependencies"]) | set(dev_dependencies)
    assert installed.isdisjoint(removed_compatibility_dependencies)


def _assert_oxlint_contract(source: str) -> None:
    assert 'import { defineConfig } from "vite-plus";' in source
    lint_start = source.index("  lint: {")
    lint_end = source.index('  base: "/ui/",', lint_start)
    lint_source = " ".join(source[lint_start:lint_end].split())

    assert 'ignorePatterns: ["dist/**", "node_modules/**"]' in lint_source
    assert 'plugins: ["typescript", "react"]' in lint_source
    assert "denyWarnings: true" in lint_source
    assert "typeAware:" not in lint_source
    assert "typeCheck:" not in lint_source

    simple_rules = {
        "react/rules-of-hooks": "error",
        "react/exhaustive-deps": "error",
        "typescript/no-explicit-any": "error",
        "prefer-const": "error",
        "unicorn/no-useless-length-check": "allow",
        "unicorn/no-useless-fallback-in-spread": "allow",
    }
    for rule, severity in simple_rules.items():
        assert lint_source.count(f'"{rule}"') == 1
        assert f'"{rule}": "{severity}"' in lint_source

    assert (
        '"react/only-export-components": [ "error", '
        '{ allowConstantExport: true, customHOCs: ["createIcon", "createControlIcon"] }, ]'
        in lint_source
    )
    assert (
        '"no-unused-vars": ["error", '
        '{ argsIgnorePattern: "^_", varsIgnorePattern: "^_" }]'
        in lint_source
    )
    assert '"no-console": ["error", { allow: ["warn", "error"] }]' in lint_source


def test_web_package_preserves_vite_plus_toolchain_contract() -> None:
    package = json.loads((ROOT / "web/package.json").read_text(encoding="utf-8"))

    _assert_vite_plus_package_contract(package)
    assert not (ROOT / "web/eslint.config.js").exists()


def test_precommit_uses_project_local_vite_plus_composite_check() -> None:
    source = (ROOT / "scripts/pre_commit_checks.sh").read_text(encoding="utf-8")

    assert ".vite-plus/env" not in source
    assert "npm -C web run check" in source
    assert "npm -C web run lint" not in source
    assert "npm -C web run typecheck" not in source


def test_vite_config_preserves_oxlint_rule_parity_contract() -> None:
    source = (ROOT / "web/vite.config.ts").read_text(encoding="utf-8")

    _assert_oxlint_contract(source)
    fmt_source = source[source.index("  fmt: {") : source.index("  lint: {")]
    assert 'ignorePatterns: ["dist/**"]' in fmt_source
    assert 'objectWrap: "collapse"' in fmt_source


def test_web_tsconfig_is_compatible_with_tsgolint() -> None:
    config = json.loads((ROOT / "web/tsconfig.json").read_text(encoding="utf-8"))

    assert "baseUrl" not in config["compilerOptions"]


def test_web_tests_import_from_vite_plus() -> None:
    test_modules = [*ROOT.glob("web/src/**/*.test.ts"), *ROOT.glob("web/src/**/*.test.tsx")]

    assert test_modules
    for path in test_modules:
        source = path.read_text(encoding="utf-8")
        assert 'from "vitest"' not in source, path
        assert "from 'vitest'" not in source, path


def test_agent_terminal_initial_snapshot_does_not_replace_live_option_updates() -> None:
    source = (ROOT / "web/src/components/AgentTab.tsx").read_text(encoding="utf-8")

    assert "terminalOptionsSnapshotRef" in source
    assert "terminalOptionsSnapshotRef.current.isDark = isDark" in source
    assert "terminalOptionsSnapshotRef.current.canControl = canControl" in source
    assert "terminalOptionsSnapshotRef.current.scrollbackLines = terminalScrollbackLines" in source
    assert "theme: getTerminalTheme(terminalOptionsSnapshotRef.current.isDark)" in source
    assert "cursorBlink: terminalOptionsSnapshotRef.current.canControl" in source
    assert "disableStdin: !terminalOptionsSnapshotRef.current.canControl" in source
    assert "scrollback: terminalOptionsSnapshotRef.current.scrollbackLines || 8000" in source
    assert "terminalRef.current.options.theme = getTerminalTheme(isDark)" in source
    assert "terminalRef.current.options.disableStdin = !canControl" in source
    assert "terminalRef.current.options.cursorBlink = canControl" in source
    assert "terminalRef.current.options.scrollback = terminalScrollbackLines" in source
    assert "}, [isDark]);" in source
    assert "}, [canControl]);" in source
    assert "}, [terminalScrollbackLines]);" in source
    assert "}, [actor.id, groupId, isHeadless, isRunning, activated]);" in source
    assert "}, [actor.id, groupId, isHeadless, isRunning, activated, canControl]);" not in source


def test_ruff_is_limited_to_error_level_rules() -> None:
    config = tomllib.loads((ROOT / "pyproject.toml").read_text(encoding="utf-8"))

    assert config["tool"]["ruff"]["lint"]["select"] == ["E9", "F63", "F7", "F82"]


def test_python_support_contract_matches_the_tested_range() -> None:
    config = tomllib.loads((ROOT / "pyproject.toml").read_text(encoding="utf-8"))
    project = config["project"]

    assert project["requires-python"] == ">=3.11"
    assert config["tool"]["ruff"]["target-version"] == "py311"
    version_classifiers = {
        value
        for value in project["classifiers"]
        if value.startswith("Programming Language :: Python :: 3.")
    }
    assert version_classifiers == {
        "Programming Language :: Python :: 3.11",
        "Programming Language :: Python :: 3.12",
        "Programming Language :: Python :: 3.13",
        "Programming Language :: Python :: 3.14",
    }


def test_local_fast_gate_runs_current_checks_without_historical_migration_governance() -> None:
    source = (ROOT / "scripts/quality_gate.sh").read_text(encoding="utf-8")
    fast_block = source.split("fast)", 1)[1].split(";;", 1)[0]

    assert "source_size.py" not in source
    assert "verify_oxfmt_migration" not in source
    assert "test:quality" not in source
    assert "ruff check" in fast_block
    assert "scripts/pre_commit_checks.sh" in fast_block
    assert "pytest tests/" not in fast_block


def test_full_precommit_path_does_not_use_xdist_auto_workers() -> None:
    source = (ROOT / "scripts/pre_commit_checks.sh").read_text(encoding="utf-8")

    assert "pytest-xdist" not in source
    assert "PYTEST_WORKERS" not in source
    assert 'python -m pytest tests/ "${pytest_common[@]}"' in source
    assert source.count("env -u CCCC_GROUP_ID -u CCCC_ACTOR_ID") >= 2
    assert "python -W error::SyntaxWarning -m compileall -q src/cccc scripts tests" in source


def test_impacted_rust_precommit_supports_binary_only_packages() -> None:
    result = subprocess.run(
        [
            "bash",
            "scripts/pre_commit_rust.sh",
            "--dry-run",
            "--",
            "crates/cccc-cli/src/commands/update.rs",
        ],
        cwd=ROOT,
        check=True,
        capture_output=True,
        text=True,
    )

    assert "rust_scope=cccc" in result.stdout
    assert "rust_targets=default,changed-tests" in result.stdout
    assert "--lib" not in result.stdout
