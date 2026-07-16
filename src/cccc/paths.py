from __future__ import annotations

import os
from pathlib import Path


def cccc_home() -> Path:
    env = os.environ.get("CCCC_HOME", "").strip()
    if env:
        return Path(env).expanduser().resolve()
    return (Path.home() / ".cccc").resolve()


def ensure_home() -> Path:
    home = cccc_home()
    home.mkdir(parents=True, exist_ok=True)
    return home

