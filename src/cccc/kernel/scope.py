from __future__ import annotations

import hashlib
from dataclasses import dataclass
from pathlib import Path
from typing import Optional

from .git import git_origin_url, git_root, normalize_git_remote


@dataclass(frozen=True)
class ScopeIdentity:
    url: str
    scope_key: str
    label: str
    git_remote: str = ""
    worktree_path: str = ""  # If this scope is a git worktree, path to the worktree


def _hash_key(value: str) -> str:
    h = hashlib.sha256(value.encode("utf-8")).hexdigest()
    return "s_" + h[:12]


def detect_scope(path: Path) -> ScopeIdentity:
    p = path.resolve()
    repo_root = git_root(p) or p
    remote_raw = git_origin_url(repo_root) if repo_root else ""
    remote_norm = normalize_git_remote(remote_raw) if remote_raw else ""

    url = str(repo_root)
    label = repo_root.name if repo_root.name else "scope"

    if remote_norm:
        scope_key = _hash_key(remote_norm)
        return ScopeIdentity(url=url, scope_key=scope_key, label=label, git_remote=remote_norm)

    scope_key = _hash_key(url)
    return ScopeIdentity(url=url, scope_key=scope_key, label=label, git_remote="")

