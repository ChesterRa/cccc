from __future__ import annotations

import os
import shlex
import subprocess
import sys
from pathlib import Path
from typing import Sequence


def normalize_cli_command(
    command: Sequence[str] | None, executable: Path | None
) -> list[str]:
    if command:
        result = [str(item) for item in command if str(item)]
    elif executable is not None:
        result = [str(executable)]
    else:
        result = []
    if not result:
        raise ValueError("CCCC hook executable is unavailable")
    return result


def shell_command(command: Sequence[str], *tail: str) -> str:
    argv = [*map(str, command), *map(str, tail)]
    if os.name == "nt":
        return subprocess.list2cmdline(argv)
    return shlex.join(argv)


def cli_command_from_mcp(command: Sequence[str]) -> list[str]:
    parts = [str(item) for item in command]
    if parts and parts[-1] == "mcp":
        return parts[:-1]
    if len(parts) >= 3 and parts[1] == "-m":
        return [parts[0], "-m", "cccc.cli"]
    return [sys.executable, "-m", "cccc.cli"]
