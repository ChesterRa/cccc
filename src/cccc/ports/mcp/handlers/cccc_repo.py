"""Workspace-scoped local-power tools for remote MCP runtimes."""

from __future__ import annotations

import os
import shutil
import subprocess
from pathlib import Path
from typing import Any, Dict, List, Mapping, Sequence

from ....kernel.group import load_group
from ....kernel.prompt_files import resolve_active_scope_root
from ....util.fs import atomic_write_text
from ..common import MCPError

_MAX_READ_BYTES = 1_000_000
_DEFAULT_READ_BYTES = 200_000
_MAX_WRITE_CHARS = 1_000_000
_MAX_PATCH_BYTES = 600_000
_MAX_OUTPUT_BYTES = 1_000_000
_DEFAULT_OUTPUT_BYTES = 200_000
_MAX_SHELL_TIMEOUT_S = 600
_DEFAULT_SHELL_TIMEOUT_S = 60
_MAX_GIT_LOG_COUNT = 100
_SKIP_DIRS = {"", ".git", ".hg", ".svn", ".cccc", ".venv", "venv", "node_modules", "__pycache__"}


def _coerce_int(value: Any, *, default: int, minimum: int, maximum: int) -> int:
    try:
        out = int(value)
    except Exception:
        out = default
    return max(minimum, min(maximum, out))


def _repo_root(group_id: str) -> tuple[Any, Path, str]:
    gid = str(group_id or "").strip()
    if not gid:
        raise MCPError(code="missing_group_id", message="missing group_id")
    group = load_group(gid)
    if group is None:
        raise MCPError(code="group_not_found", message=f"group not found: {gid}")
    root = resolve_active_scope_root(group)
    if root is None:
        raise MCPError(code="missing_scope", message="group has no active scope")
    root = root.expanduser().resolve()
    if not root.exists() or not root.is_dir():
        raise MCPError(code="invalid_scope", message=f"active scope root is not a directory: {root}")
    return group, root, str(group.doc.get("active_scope_key") or "").strip()


def _resolve_under_root(root: Path, raw_path: Any) -> Path:
    value = str(raw_path or "").strip() or "."
    candidate = Path(value).expanduser()
    if not candidate.is_absolute():
        candidate = root / candidate
    resolved = candidate.resolve(strict=False)
    try:
        resolved.relative_to(root)
    except ValueError:
        raise MCPError(code="invalid_path", message="path must stay under the group's active scope root")
    return resolved


def _relative(root: Path, path: Path) -> str:
    try:
        return path.relative_to(root).as_posix()
    except ValueError:
        return str(path)


def _truncate_text(text: str, *, max_bytes: int) -> tuple[str, bool]:
    raw = str(text or "").encode("utf-8", errors="replace")
    limit = min(max_bytes, _MAX_OUTPUT_BYTES)
    if len(raw) <= limit:
        return str(text or ""), False
    clipped = raw[:limit].decode("utf-8", errors="replace")
    return clipped.rstrip() + "\n[cccc] output truncated", True


def _paths_arg(value: Any) -> List[str]:
    if isinstance(value, list):
        return [str(item or "").strip() for item in value if str(item or "").strip()]
    if isinstance(value, str) and value.strip():
        return [value.strip()]
    return []


def _relative_paths_under_root(root: Path, raw_paths: Sequence[str]) -> List[str]:
    out: List[str] = []
    for raw in raw_paths:
        target = _resolve_under_root(root, raw)
        out.append(_relative(root, target))
    return out


def _run_command(
    args: Sequence[str],
    *,
    root: Path,
    timeout_s: int,
    max_output_bytes: int,
    input_text: str = "",
) -> Dict[str, Any]:
    try:
        result = subprocess.run(
            list(args),
            cwd=str(root),
            input=input_text if input_text else None,
            text=True,
            capture_output=True,
            check=False,
            timeout=timeout_s,
        )
    except subprocess.TimeoutExpired as exc:
        stdout, stdout_truncated = _truncate_text(str(exc.stdout or ""), max_bytes=max_output_bytes)
        stderr, stderr_truncated = _truncate_text(str(exc.stderr or ""), max_bytes=max_output_bytes)
        return {
            "root_path": str(root),
            "timed_out": True,
            "timeout_s": timeout_s,
            "returncode": None,
            "stdout": stdout,
            "stderr": stderr,
            "stdout_truncated": stdout_truncated,
            "stderr_truncated": stderr_truncated,
        }
    stdout, stdout_truncated = _truncate_text(str(result.stdout or ""), max_bytes=max_output_bytes)
    stderr, stderr_truncated = _truncate_text(str(result.stderr or ""), max_bytes=max_output_bytes)
    return {
        "root_path": str(root),
        "timed_out": False,
        "returncode": int(result.returncode),
        "stdout": stdout,
        "stderr": stderr,
        "stdout_truncated": stdout_truncated,
        "stderr_truncated": stderr_truncated,
    }


def _read_text(path: Path, *, max_bytes: int) -> tuple[str, bool, int]:
    if not path.exists() or not path.is_file():
        raise MCPError(code="not_found", message=f"file not found: {path}")
    size = int(path.stat().st_size)
    limit = min(max_bytes, _MAX_READ_BYTES)
    with path.open("rb") as fh:
        raw = fh.read(limit)
    if b"\0" in raw:
        raise MCPError(code="binary_file", message="refusing to read binary file as text")
    text = raw.decode("utf-8", errors="replace")
    return text, size > len(raw), size


def _list_files(root: Path, base: Path, *, limit: int, include_hidden: bool) -> tuple[List[str], bool]:
    if not base.exists():
        raise MCPError(code="not_found", message=f"path not found: {base}")
    if base.is_file():
        return [_relative(root, base)], False
    if not base.is_dir():
        raise MCPError(code="invalid_path", message="path must be a file or directory")

    out: List[str] = []
    truncated = False
    for dirpath, dirnames, filenames in os.walk(base):
        dirnames[:] = [
            name
            for name in sorted(dirnames)
            if name not in _SKIP_DIRS and (include_hidden or not name.startswith("."))
        ]
        for name in sorted(filenames):
            if not include_hidden and name.startswith("."):
                continue
            path = Path(dirpath) / name
            out.append(_relative(root, path))
            if len(out) >= limit:
                truncated = True
                return out, truncated
    return out, truncated


def repo_tool(
    *,
    group_id: str,
    action: str,
    path: str = "",
    content: str = "",
    patch: str = "",
    dest_path: str = "",
    recursive: bool = False,
    exist_ok: bool = True,
    max_bytes: Any = _DEFAULT_READ_BYTES,
    limit: Any = 200,
    include_hidden: bool = False,
) -> Dict[str, Any]:
    """Run a repository operation under the group's active scope root."""
    _group, root, scope_key = _repo_root(group_id)
    act = str(action or "info").strip().lower()

    if act == "info":
        return {"root_path": str(root), "scope_key": scope_key}

    target = _resolve_under_root(root, path)
    if act == "list":
        files, truncated = _list_files(
            root,
            target,
            limit=_coerce_int(limit, default=200, minimum=1, maximum=500),
            include_hidden=bool(include_hidden),
        )
        return {"root_path": str(root), "path": _relative(root, target), "files": files, "truncated": truncated}

    if act == "read":
        text, truncated, size = _read_text(
            target,
            max_bytes=_coerce_int(max_bytes, default=_DEFAULT_READ_BYTES, minimum=1, maximum=_MAX_READ_BYTES),
        )
        return {
            "root_path": str(root),
            "path": _relative(root, target),
            "content": text,
            "bytes": size,
            "truncated": truncated,
        }

    if act == "write":
        payload = str(content or "")
        if len(payload) > _MAX_WRITE_CHARS:
            raise MCPError(code="content_too_large", message="content is too large")
        target.parent.mkdir(parents=True, exist_ok=True)
        atomic_write_text(target, payload, encoding="utf-8")
        return {"root_path": str(root), "path": _relative(root, target), "written": True, "bytes": len(payload.encode("utf-8"))}

    if act == "apply_patch":
        payload = str(patch or "")
        if not payload.strip():
            raise MCPError(code="missing_patch", message="patch is required")
        if len(payload.encode("utf-8")) > _MAX_PATCH_BYTES:
            raise MCPError(code="patch_too_large", message="patch is too large")
        result = subprocess.run(
            ["git", "-C", str(root), "apply", "--whitespace=nowarn", "-"],
            input=payload,
            text=True,
            capture_output=True,
            check=False,
            timeout=30,
        )
        if result.returncode != 0:
            raise MCPError(code="patch_failed", message=(result.stderr or result.stdout or "git apply failed").strip())
        return {"root_path": str(root), "applied": True, "stdout": str(result.stdout or "").strip()}

    if act == "mkdir":
        target.mkdir(parents=True, exist_ok=bool(exist_ok))
        return {"root_path": str(root), "path": _relative(root, target), "created": True}

    if act == "delete":
        if target == root:
            raise MCPError(code="invalid_path", message="refusing to delete the active scope root")
        if not target.exists() and not target.is_symlink():
            raise MCPError(code="not_found", message=f"path not found: {target}")
        if target.is_dir() and not target.is_symlink():
            if not bool(recursive):
                raise MCPError(code="directory_requires_recursive", message="set recursive=true to delete a directory")
            shutil.rmtree(target)
        else:
            target.unlink()
        return {"root_path": str(root), "path": _relative(root, target), "deleted": True}

    if act == "move":
        if target == root:
            raise MCPError(code="invalid_path", message="refusing to move the active scope root")
        if not target.exists() and not target.is_symlink():
            raise MCPError(code="not_found", message=f"path not found: {target}")
        dest = _resolve_under_root(root, dest_path)
        if dest == root:
            raise MCPError(code="invalid_path", message="destination cannot be the active scope root")
        dest.parent.mkdir(parents=True, exist_ok=True)
        shutil.move(str(target), str(dest))
        return {
            "root_path": str(root),
            "path": _relative(root, target),
            "dest_path": _relative(root, dest),
            "moved": True,
        }

    raise MCPError(code="invalid_action", message="cccc_repo action must be info|list|read|write|apply_patch|mkdir|delete|move")


def shell_tool(
    *,
    group_id: str,
    command: str,
    cwd: str = "",
    timeout_s: Any = _DEFAULT_SHELL_TIMEOUT_S,
    max_output_bytes: Any = _DEFAULT_OUTPUT_BYTES,
    env: Any = None,
) -> Dict[str, Any]:
    """Run a shell command under the group's active scope root."""
    _group, root, scope_key = _repo_root(group_id)
    cmd = str(command or "").strip()
    if not cmd:
        raise MCPError(code="missing_command", message="command is required")
    workdir = _resolve_under_root(root, cwd or ".")
    if not workdir.exists() or not workdir.is_dir():
        raise MCPError(code="invalid_cwd", message="cwd must be an existing directory under the active scope root")
    timeout = _coerce_int(timeout_s, default=_DEFAULT_SHELL_TIMEOUT_S, minimum=1, maximum=_MAX_SHELL_TIMEOUT_S)
    output_limit = _coerce_int(max_output_bytes, default=_DEFAULT_OUTPUT_BYTES, minimum=1, maximum=_MAX_OUTPUT_BYTES)
    proc_env = os.environ.copy()
    if isinstance(env, Mapping):
        for key, value in env.items():
            k = str(key or "").strip()
            if k:
                proc_env[k] = str(value)
    try:
        result = subprocess.run(
            cmd,
            shell=True,
            cwd=str(workdir),
            env=proc_env,
            text=True,
            capture_output=True,
            check=False,
            timeout=timeout,
        )
    except subprocess.TimeoutExpired as exc:
        stdout, stdout_truncated = _truncate_text(str(exc.stdout or ""), max_bytes=output_limit)
        stderr, stderr_truncated = _truncate_text(str(exc.stderr or ""), max_bytes=output_limit)
        return {
            "root_path": str(root),
            "scope_key": scope_key,
            "cwd": _relative(root, workdir),
            "command": cmd,
            "timed_out": True,
            "timeout_s": timeout,
            "returncode": None,
            "stdout": stdout,
            "stderr": stderr,
            "stdout_truncated": stdout_truncated,
            "stderr_truncated": stderr_truncated,
        }
    stdout, stdout_truncated = _truncate_text(str(result.stdout or ""), max_bytes=output_limit)
    stderr, stderr_truncated = _truncate_text(str(result.stderr or ""), max_bytes=output_limit)
    return {
        "root_path": str(root),
        "scope_key": scope_key,
        "cwd": _relative(root, workdir),
        "command": cmd,
        "timed_out": False,
        "returncode": int(result.returncode),
        "stdout": stdout,
        "stderr": stderr,
        "stdout_truncated": stdout_truncated,
        "stderr_truncated": stderr_truncated,
    }


def git_tool(
    *,
    group_id: str,
    action: str,
    paths: Any = None,
    path: str = "",
    message: str = "",
    staged: bool = False,
    count: Any = 20,
    all_changes: bool = False,
    max_output_bytes: Any = _DEFAULT_OUTPUT_BYTES,
) -> Dict[str, Any]:
    """Run common git operations under the group's active scope root."""
    _group, root, scope_key = _repo_root(group_id)
    act = str(action or "status").strip().lower()
    output_limit = _coerce_int(max_output_bytes, default=_DEFAULT_OUTPUT_BYTES, minimum=1, maximum=_MAX_OUTPUT_BYTES)
    timeout = 120

    if act == "status":
        result = _run_command(["git", "status", "--short", "--branch"], root=root, timeout_s=timeout, max_output_bytes=output_limit)
    elif act == "diff":
        args = ["git", "diff"]
        if bool(staged):
            args.append("--staged")
        rel_paths = _relative_paths_under_root(root, _paths_arg(paths) or _paths_arg(path))
        if rel_paths:
            args.extend(["--", *rel_paths])
        result = _run_command(args, root=root, timeout_s=timeout, max_output_bytes=output_limit)
    elif act == "log":
        n = _coerce_int(count, default=20, minimum=1, maximum=_MAX_GIT_LOG_COUNT)
        result = _run_command(["git", "log", "--oneline", "--decorate", f"-n{n}"], root=root, timeout_s=timeout, max_output_bytes=output_limit)
    elif act == "add":
        args = ["git", "add"]
        if bool(all_changes):
            args.append("-A")
        rel_paths = _relative_paths_under_root(root, _paths_arg(paths) or _paths_arg(path))
        if rel_paths:
            args.extend(["--", *rel_paths])
        elif not bool(all_changes):
            raise MCPError(code="missing_path", message="paths/path or all_changes=true is required for git add")
        result = _run_command(args, root=root, timeout_s=timeout, max_output_bytes=output_limit)
    elif act == "commit":
        msg = str(message or "").strip()
        if not msg:
            raise MCPError(code="missing_message", message="message is required for git commit")
        result = _run_command(["git", "commit", "-m", msg], root=root, timeout_s=timeout, max_output_bytes=output_limit)
    else:
        raise MCPError(code="invalid_action", message="cccc_git action must be status|diff|log|add|commit")

    result["scope_key"] = scope_key
    result["action"] = act
    return result
