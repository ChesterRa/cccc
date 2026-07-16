from __future__ import annotations

import re
import subprocess
from pathlib import Path
from typing import Optional


def _run_git(args: list[str], *, cwd: Path) -> tuple[int, str]:
    try:
        p = subprocess.run(
            ["git", *args],
            cwd=str(cwd),
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
            text=True,
            check=False,
        )
        return int(p.returncode), (p.stdout or "").strip()
    except Exception:
        return 1, ""


def git_root(path: Path) -> Optional[Path]:
    code, out = _run_git(["rev-parse", "--show-toplevel"], cwd=path)
    if code != 0 or not out:
        return None
    try:
        return Path(out).resolve()
    except Exception:
        return None


def git_origin_url(repo_root: Path) -> str:
    code, out = _run_git(["config", "--get", "remote.origin.url"], cwd=repo_root)
    return out if code == 0 else ""


_SSH_SCPLIKE = re.compile(r"^(?P<user>[^@]+)@(?P<host>[^:]+):(?P<path>.+)$")


def normalize_git_remote(url: str) -> str:
    u = (url or "").strip()
    if not u:
        return ""
    m = _SSH_SCPLIKE.match(u)
    if m:
        host = m.group("host")
        path = m.group("path")
        if path.endswith(".git"):
            path = path[: -len(".git")]
        return f"https://{host}/{path}"
    if u.startswith("ssh://"):
        u2 = u[len("ssh://") :]
        u2 = u2.replace("git@", "", 1)
        if "/" in u2:
            host, path = u2.split("/", 1)
            if path.endswith(".git"):
                path = path[: -len(".git")]
            return f"https://{host}/{path}"
    if u.startswith("http://") or u.startswith("https://"):
        if u.endswith(".git"):
            u = u[: -len(".git")]
        return u
    return u

