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
            stderr=subprocess.PIPE,
            text=True,
            check=False,
        )
        # Return stdout if successful, stderr if failed
        output = (p.stdout or "").strip()
        if p.returncode != 0 and not output:
            output = (p.stderr or "").strip()
        return int(p.returncode), output
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


# ============ Git Branch Functions ============


def git_current_branch(repo_root: Path) -> Optional[str]:
    """Get the current branch name.

    Args:
        repo_root: Path to the repository

    Returns:
        Current branch name, or None if detached HEAD or error
    """
    code, out = _run_git(["rev-parse", "--abbrev-ref", "HEAD"], cwd=repo_root)
    if code != 0 or not out:
        return None
    branch = out.strip()
    if branch == "HEAD":
        # Detached HEAD state
        return None
    return branch


def git_branch_exists(repo_root: Path, branch: str) -> bool:
    """Check if a branch exists in the repository.

    Args:
        repo_root: Path to the repository
        branch: Branch name to check

    Returns:
        True if the branch exists, False otherwise
    """
    code, _ = _run_git(["rev-parse", "--verify", f"refs/heads/{branch}"], cwd=repo_root)
    return code == 0


def git_remote_ref_exists(repo_root: Path, ref: str) -> bool:
    """Check if a remote ref exists (e.g., origin/main).

    Args:
        repo_root: Path to the repository
        ref: Remote ref to check (e.g., "origin/main")

    Returns:
        True if the remote ref exists, False otherwise
    """
    code, _ = _run_git(["rev-parse", "--verify", f"refs/remotes/{ref}"], cwd=repo_root)
    return code == 0


def git_list_branches(repo_root: Path, *, include_remote: bool = False) -> list[dict]:
    """List all branches with their worktree occupation status.

    Args:
        repo_root: Path to the repository
        include_remote: If True, include remote tracking branches

    Returns:
        List of dicts with keys:
        - name: branch name (without refs/heads/ or refs/remotes/ prefix)
        - is_remote: True if this is a remote tracking branch
        - in_use: True if this branch is checked out in a worktree
        - worktree_path: Path to the worktree using this branch, or None
        - is_current: True if this is the current branch (HEAD)
    """
    # Get current branch
    current_branch = git_current_branch(repo_root)

    # Get local branches
    code, out = _run_git(["branch", "--format=%(refname:short)"], cwd=repo_root)
    local_branches = out.split("\n") if code == 0 and out else []
    local_branches = [b.strip() for b in local_branches if b.strip()]

    # Get remote branches if requested
    remote_branches: list[str] = []
    if include_remote:
        code, out = _run_git(["branch", "-r", "--format=%(refname:short)"], cwd=repo_root)
        if code == 0 and out:
            remote_branches = [b.strip() for b in out.split("\n") if b.strip()]
            # Filter out HEAD pointers like "origin/HEAD" and bare remote names like "origin"
            remote_branches = [b for b in remote_branches if not b.endswith("/HEAD")]
            remote_branches = [b for b in remote_branches if "/" in b]

    # Get worktree info to determine which branches are in use
    worktrees = git_worktree_list(repo_root)
    branch_to_worktree: dict[str, str] = {}
    for wt in worktrees:
        branch = wt.get("branch", "")
        # Branch is stored as refs/heads/xxx, extract the name
        if branch.startswith("refs/heads/"):
            branch = branch[len("refs/heads/"):]
        if branch and branch not in ("(detached)", "(bare)"):
            branch_to_worktree[branch] = wt.get("path", "")

    # Build result
    result: list[dict] = []

    # Only include the current local branch (not all local branches)
    # User wants: current branch first, then remote branches only
    if current_branch and current_branch in local_branches:
        result.append({
            "name": current_branch,
            "is_remote": False,
            "in_use": current_branch in branch_to_worktree,
            "worktree_path": branch_to_worktree.get(current_branch),
            "is_current": True,
        })

    for branch in remote_branches:
        result.append({
            "name": branch,
            "is_remote": True,
            "in_use": False,  # Remote branches can't be directly checked out in worktrees
            "worktree_path": None,
            "is_current": False,  # Remote branches can't be current
        })

    return result


# ============ Git Worktree Functions ============


def git_worktree_add(
    repo_root: Path,
    worktree_path: Path,
    branch: str,
    *,
    create_branch: bool = True,
    start_point: str = "",
) -> tuple[bool, str]:
    """Create a new git worktree.

    Args:
        repo_root: Path to the main repository
        worktree_path: Path where the worktree will be created
        branch: Branch name for the worktree
        create_branch: If True, create a new branch (-b flag); if False, use existing branch
        start_point: Starting point for the new branch (e.g., "origin/main", "main").
                     Only used when create_branch=True. If empty, defaults to HEAD.

    Returns:
        (success, message) tuple
    """
    args = ["worktree", "add"]
    if create_branch:
        args.extend(["-b", branch])
    args.append(str(worktree_path))
    if create_branch and start_point:
        # git worktree add -b <new-branch> <path> <start-point>
        args.append(start_point)
    elif not create_branch:
        args.append(branch)

    code, out = _run_git(args, cwd=repo_root)
    if code == 0:
        return True, f"Worktree created at {worktree_path}"
    return False, out or "Failed to create worktree"


def git_worktree_remove(repo_root: Path, worktree_path: Path, *, force: bool = False) -> tuple[bool, str]:
    """Remove a git worktree.

    Args:
        repo_root: Path to the main repository
        worktree_path: Path of the worktree to remove
        force: If True, force removal even with uncommitted changes

    Returns:
        (success, message) tuple
    """
    args = ["worktree", "remove"]
    if force:
        args.append("--force")
    args.append(str(worktree_path))

    code, out = _run_git(args, cwd=repo_root)
    if code == 0:
        return True, f"Worktree removed: {worktree_path}"
    return False, out or "Failed to remove worktree"


def git_worktree_list(repo_root: Path) -> list[dict[str, str]]:
    """List all worktrees for a repository.

    Args:
        repo_root: Path to the main repository

    Returns:
        List of dicts with 'path', 'commit', 'branch' keys
    """
    code, out = _run_git(["worktree", "list", "--porcelain"], cwd=repo_root)
    if code != 0 or not out:
        return []

    worktrees = []
    current: dict[str, str] = {}

    for line in out.split("\n"):
        line = line.strip()
        if not line:
            if current:
                worktrees.append(current)
                current = {}
            continue
        if line.startswith("worktree "):
            current["path"] = line[9:]
        elif line.startswith("HEAD "):
            current["commit"] = line[5:]
        elif line.startswith("branch "):
            current["branch"] = line[7:]
        elif line == "detached":
            current["branch"] = "(detached)"
        elif line == "bare":
            current["branch"] = "(bare)"

    if current:
        worktrees.append(current)

    return worktrees


def git_is_worktree(path: Path) -> bool:
    """Check if a path is inside a git worktree (not the main working tree).

    Args:
        path: Path to check

    Returns:
        True if path is in a worktree, False otherwise
    """
    code, out = _run_git(["rev-parse", "--is-inside-work-tree"], cwd=path)
    if code != 0:
        return False

    # Check if this is a linked worktree by looking for .git file (not directory)
    git_path = path / ".git"
    if git_path.is_file():
        return True  # Linked worktree has .git as a file pointing to main repo
    return False

