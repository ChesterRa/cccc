from __future__ import annotations

import os
from pathlib import Path
from typing import Dict

from ...kernel.runtime import resolve_deepseek_home

_NODE_USE_ENV_PROXY = "NODE_USE_ENV_PROXY"


def prepare_deepseek_setup_env(env: Dict[str, str]) -> tuple[Dict[str, str], Path]:
    effective_env = dict(os.environ)
    effective_env.update(env)
    dsh_home = resolve_deepseek_home(effective_env)
    if dsh_home is None:
        raise RuntimeError(
            "DeepSeek runtime root cannot be inferred because CCCC_HOME and HOME are not configured"
        )

    cccc_home = dsh_home.parents[2]
    effective_env["CCCC_HOME"] = str(cccc_home)
    env["CCCC_HOME"] = str(cccc_home)
    effective_env["DSH_HOME"] = str(dsh_home)
    env["DSH_HOME"] = str(dsh_home)
    effective_env.setdefault(_NODE_USE_ENV_PROXY, "1")
    env.setdefault(_NODE_USE_ENV_PROXY, effective_env[_NODE_USE_ENV_PROXY])
    local_bin = str(dsh_home / "node_modules" / ".bin")
    inherited_path = str(effective_env.get("PATH") or "")
    effective_env["PATH"] = local_bin + (
        os.pathsep + inherited_path if inherited_path else ""
    )
    env["PATH"] = effective_env["PATH"]
    return effective_env, dsh_home
