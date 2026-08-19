from __future__ import annotations

import os

from cccc.contracts.v1.deepseek import DEEPSEEK_NPM_BEFORE
from cccc.daemon.actors.deepseek_setup import _install_packages, _packages_ready


def test_npm_installer_writes_the_pinned_package_tuple(tmp_path, monkeypatch) -> None:
    if os.name == "nt":
        return
    npm = tmp_path / "npm"
    npm.write_text(
        """#!/bin/sh
set -eu
printf '%s\n' "$@" > npm-args.txt
for spec in "$@"; do
  case "$spec" in
    @deepseek-ai/dsh-acp@*) name=dsh-acp; version=0.1.0-rc.6 ;;
    @deepseek-ai/dsh-mcp-client@*) name=dsh-mcp-client; version=0.1.0-rc.6 ;;
    @deepseek-ai/dsh-acp-demo@*) name=dsh-acp-demo; version=0.1.0-rc.6 ;;
    @deepseek-ai/dsh-llm-deepseek@*) name=dsh-llm-deepseek; version=0.1.0-rc.6 ;;
    *) continue ;;
  esac
  mkdir -p "node_modules/@deepseek-ai/$name"
  printf '{"version":"%s"}\\n' "$version" > "node_modules/@deepseek-ai/$name/package.json"
done
printf '%s\n' '{"lockfileVersion":3,"packages":{"":{"dependencies":{"@deepseek-ai/dsh-acp":"0.1.0-rc.6","@deepseek-ai/dsh-mcp-client":"0.1.0-rc.6","@deepseek-ai/dsh-acp-demo":"0.1.0-rc.6","@deepseek-ai/dsh-llm-deepseek":"0.1.0-rc.6"}},"node_modules/@deepseek-ai/dsh-acp":{"version":"0.1.0-rc.6"},"node_modules/@deepseek-ai/dsh-mcp-client":{"version":"0.1.0-rc.6"},"node_modules/@deepseek-ai/dsh-acp-demo":{"version":"0.1.0-rc.6"},"node_modules/@deepseek-ai/dsh-llm-deepseek":{"version":"0.1.0-rc.6"}}}' > package-lock.json
""",
        encoding="utf-8",
    )
    npm.chmod(0o755)
    dsh_home = tmp_path / ".dsh"
    dsh_home.mkdir()
    path = str(tmp_path) + os.pathsep + os.environ.get("PATH", "")
    monkeypatch.setenv("PATH", path)
    _install_packages(dsh_home, {**os.environ, "PATH": path})
    assert _packages_ready(dsh_home)
    args = (dsh_home / "npm-args.txt").read_text(encoding="utf-8").splitlines()
    assert DEEPSEEK_NPM_BEFORE in args
    assert "@deepseek-ai/dsh@0.1.0-rc.6" not in args


def test_python_setup_command_uses_the_same_automatic_setup(tmp_path, monkeypatch) -> None:
    import cccc.daemon.actors.deepseek_setup as setup_module

    profile = tmp_path / ".dsh" / "profiles" / "cccc-acp"
    monkeypatch.setattr(
        setup_module,
        "ensure_deepseek_setup",
        lambda _env: setup_module.DeepSeekSetupOutcome(
            dsh_home=tmp_path / ".dsh",
            profile=profile,
            packages_installed=True,
            profile_created=True,
        ),
    )
    result, error = setup_module.setup_deepseek_result({})
    assert error is None
    assert result["status"] == "ready"
    assert result["packages_installed"] is True
