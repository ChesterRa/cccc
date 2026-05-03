"""Workspace-scoped local-power tools for remote MCP runtimes."""

from __future__ import annotations

import hashlib
import os
import queue
import shutil
import subprocess
import threading
import time
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
_MAX_EXEC_SESSIONS = 16
_SKIP_DIRS = {"", ".git", ".hg", ".svn", ".cccc", ".venv", "venv", "node_modules", "__pycache__"}


class _ExecSession:
    def __init__(
        self,
        *,
        session_id: str,
        proc: subprocess.Popen[str],
        root: Path,
        workdir: Path,
        command: str,
        expires_at: float,
    ) -> None:
        self.session_id = session_id
        self.proc = proc
        self.root = root
        self.workdir = workdir
        self.command = command
        self.expires_at = expires_at
        self.output: "queue.Queue[str]" = queue.Queue()
        self.created_at = time.monotonic()
        self.last_used_at = self.created_at


_EXEC_LOCK = threading.Lock()
_EXEC_SESSIONS: Dict[str, _ExecSession] = {}
_NEXT_EXEC_SESSION_ID = 1


def _coerce_int(value: Any, *, default: int, minimum: int, maximum: int) -> int:
    try:
        out = int(value)
    except Exception:
        out = default
    return max(minimum, min(maximum, out))


def _coerce_optional_int(value: Any, *, minimum: int, maximum: int) -> int | None:
    if value is None:
        return None
    if isinstance(value, str) and not value.strip():
        return None
    try:
        out = int(value)
    except Exception:
        return None
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


def _patch_relative_path(raw_path: str) -> str:
    value = str(raw_path or "").strip()
    if not value:
        raise MCPError(code="invalid_patch", message="patch file path is required")
    if Path(value).is_absolute():
        raise MCPError(code="invalid_patch", message="Codex apply_patch paths must be relative")
    return value


def _truncate_text(text: str, *, max_bytes: int) -> tuple[str, bool]:
    raw = str(text or "").encode("utf-8", errors="replace")
    limit = min(max_bytes, _MAX_OUTPUT_BYTES)
    if len(raw) <= limit:
        return str(text or ""), False
    clipped = raw[:limit].decode("utf-8", errors="replace")
    return clipped.rstrip() + "\n[cccc] output truncated", True


def _sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as fh:
        for chunk in iter(lambda: fh.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


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


def _new_exec_session_id() -> str:
    global _NEXT_EXEC_SESSION_ID
    with _EXEC_LOCK:
        session_id = f"exec-{_NEXT_EXEC_SESSION_ID}"
        _NEXT_EXEC_SESSION_ID += 1
        return session_id


def _prune_exec_sessions() -> None:
    with _EXEC_LOCK:
        now = time.monotonic()
        dead = []
        for sid, session in _EXEC_SESSIONS.items():
            if session.proc.poll() is not None:
                dead.append(sid)
            elif now > session.expires_at:
                try:
                    session.proc.terminate()
                except Exception:
                    pass
                dead.append(sid)
        for sid in dead:
            _EXEC_SESSIONS.pop(sid, None)
        if len(_EXEC_SESSIONS) <= _MAX_EXEC_SESSIONS:
            return
        ordered = sorted(_EXEC_SESSIONS.values(), key=lambda item: item.last_used_at)
        for session in ordered[: max(0, len(_EXEC_SESSIONS) - _MAX_EXEC_SESSIONS)]:
            try:
                session.proc.terminate()
            except Exception:
                pass
            _EXEC_SESSIONS.pop(session.session_id, None)


def _reader_thread(stream: Any, output: "queue.Queue[str]") -> None:
    try:
        while True:
            chunk = stream.read(4096)
            if not chunk:
                break
            output.put(str(chunk))
    except Exception as exc:
        output.put(f"\n[cccc] exec output reader failed: {exc}")


def _drain_exec_output(session: _ExecSession, *, max_output_bytes: int) -> tuple[str, bool]:
    chunks: List[str] = []
    while True:
        try:
            chunks.append(session.output.get_nowait())
        except queue.Empty:
            break
    return _truncate_text("".join(chunks), max_bytes=max_output_bytes)


def _sleep_for_yield(yield_time_ms: Any) -> None:
    delay_ms = _coerce_int(yield_time_ms, default=1000, minimum=0, maximum=30_000)
    if delay_ms > 0:
        time.sleep(delay_ms / 1000.0)


def _read_text(path: Path, *, max_bytes: int) -> tuple[str, bool, int, str]:
    if not path.exists() or not path.is_file():
        raise MCPError(code="not_found", message=f"file not found: {path}")
    size = int(path.stat().st_size)
    limit = min(max_bytes, _MAX_READ_BYTES)
    with path.open("rb") as fh:
        raw = fh.read(limit)
    truncated = size > len(raw)
    sha256 = "" if truncated else hashlib.sha256(raw).hexdigest()
    if b"\0" in raw:
        raise MCPError(code="binary_file", message="refusing to read binary file as text")
    text = raw.decode("utf-8", errors="replace")
    return text, truncated, size, sha256


def _line_slice(text: str, *, start_line: Any = None, end_line: Any = None) -> tuple[str, int | None, int | None, int]:
    lines = text.splitlines(keepends=True)
    total = len(lines)
    if start_line is None and end_line is None:
        return text, None, None, total
    start = _coerce_int(start_line, default=1, minimum=1, maximum=max(total, 1))
    end_default = min(total, start + 199)
    end = _coerce_int(end_line, default=end_default, minimum=start, maximum=max(total, start))
    return "".join(lines[start - 1 : end]), start, end, total


def _assert_expected_sha256(path: Path, expected_sha256: str) -> str:
    expected = str(expected_sha256 or "").strip().lower()
    if not expected:
        return _sha256_file(path) if path.exists() and path.is_file() else ""
    if not path.exists() or not path.is_file():
        raise MCPError(code="not_found", message=f"file not found for expected_sha256 check: {path}")
    current = _sha256_file(path)
    if current.lower() != expected:
        raise MCPError(
            code="stale_file",
            message=(
                "file changed since it was read; re-read cccc_repo(action=read), then retry with the new sha256 "
                f"(current_sha256={current})"
            ),
        )
    return current


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


def _list_dir_entries(
    root: Path,
    base: Path,
    *,
    offset: int,
    limit: int,
    depth: int,
    include_hidden: bool,
) -> tuple[List[Dict[str, Any]], bool, int]:
    if not base.exists():
        raise MCPError(code="not_found", message=f"path not found: {base}")
    if not base.is_dir():
        raise MCPError(code="invalid_path", message="path must be a directory")
    entries: List[Dict[str, Any]] = []
    queue_items: List[tuple[Path, int]] = [(base, 1)]
    while queue_items:
        current, current_depth = queue_items.pop(0)
        try:
            children = sorted(current.iterdir(), key=lambda item: item.name)
        except OSError as exc:
            raise MCPError(code="read_failed", message=f"failed to read directory: {exc}") from exc
        dirs_to_visit: List[Path] = []
        for child in children:
            name = child.name
            if name in _SKIP_DIRS or (not include_hidden and name.startswith(".")):
                continue
            try:
                rel = _relative(root, child)
                is_dir = child.is_dir() and not child.is_symlink()
                entry = {
                    "path": rel,
                    "name": name,
                    "depth": current_depth,
                    "type": "dir" if is_dir else "file" if child.is_file() else "symlink" if child.is_symlink() else "other",
                }
                entries.append(entry)
                if is_dir and current_depth < depth:
                    dirs_to_visit.append(child)
            except OSError:
                continue
        queue_items.extend((child, current_depth + 1) for child in dirs_to_visit)
    start = max(0, offset - 1)
    selected = entries[start : start + limit]
    return selected, start + limit < len(entries), len(entries)


def _find_line_sequence(lines: List[str], needle: List[str]) -> int:
    if not needle:
        return len(lines)
    matches: List[int] = []
    width = len(needle)
    for idx in range(0, len(lines) - width + 1):
        if lines[idx : idx + width] == needle:
            matches.append(idx)
            if len(matches) > 1:
                break
    if not matches:
        raise MCPError(code="patch_context_not_found", message="Codex patch context was not found")
    if len(matches) > 1:
        raise MCPError(code="patch_context_ambiguous", message="Codex patch context matched multiple locations")
    return matches[0]


def _apply_update_hunks(original: str, hunks: List[List[str]]) -> str:
    lines = original.splitlines()
    trailing_newline = original.endswith("\n")
    for hunk in hunks:
        old_lines: List[str] = []
        new_lines: List[str] = []
        for raw in hunk:
            if raw.startswith("@@"):
                continue
            if raw == "*** End of File":
                continue
            if not raw:
                raise MCPError(code="invalid_patch", message="empty patch hunk line must be prefixed with space, +, or -")
            marker = raw[0]
            value = raw[1:]
            if marker == " ":
                old_lines.append(value)
                new_lines.append(value)
            elif marker == "-":
                old_lines.append(value)
            elif marker == "+":
                new_lines.append(value)
            else:
                raise MCPError(code="invalid_patch", message="patch hunk lines must start with space, +, or -")
        start = _find_line_sequence(lines, old_lines)
        lines = lines[:start] + new_lines + lines[start + len(old_lines) :]
    return "\n".join(lines) + ("\n" if trailing_newline or lines else "")


def _split_codex_patch_sections(patch: str) -> List[tuple[str, str, List[str]]]:
    raw_lines = str(patch or "").splitlines()
    if not raw_lines or raw_lines[0].strip() != "*** Begin Patch":
        raise MCPError(code="invalid_patch", message="Codex apply_patch must start with *** Begin Patch")
    if raw_lines[-1].strip() != "*** End Patch":
        raise MCPError(code="invalid_patch", message="Codex apply_patch must end with *** End Patch")
    sections: List[tuple[str, str, List[str]]] = []
    current_action = ""
    current_path = ""
    current_lines: List[str] = []
    for line in raw_lines[1:-1]:
        if line.startswith("*** Add File: "):
            if current_action:
                sections.append((current_action, current_path, current_lines))
            current_action = "add"
            current_path = _patch_relative_path(line.removeprefix("*** Add File: "))
            current_lines = []
        elif line.startswith("*** Update File: "):
            if current_action:
                sections.append((current_action, current_path, current_lines))
            current_action = "update"
            current_path = _patch_relative_path(line.removeprefix("*** Update File: "))
            current_lines = []
        elif line.startswith("*** Delete File: "):
            if current_action:
                sections.append((current_action, current_path, current_lines))
            current_action = "delete"
            current_path = _patch_relative_path(line.removeprefix("*** Delete File: "))
            current_lines = []
        else:
            if not current_action:
                raise MCPError(code="invalid_patch", message="patch content must appear after a file operation header")
            current_lines.append(line)
    if current_action:
        sections.append((current_action, current_path, current_lines))
    if not sections:
        raise MCPError(code="invalid_patch", message="patch must contain at least one file operation")
    return sections


def apply_codex_patch_tool(*, group_id: str, patch: str) -> Dict[str, Any]:
    """Apply Codex-style *** Begin Patch edits under the active scope root."""
    _group, root, _scope_key = _repo_root(group_id)
    payload = str(patch or "")
    if len(payload.encode("utf-8")) > _MAX_PATCH_BYTES:
        raise MCPError(code="patch_too_large", message="patch is too large")
    sections = _split_codex_patch_sections(payload)
    operations: List[Dict[str, Any]] = []
    for action, rel_path, body in sections:
        target = _resolve_under_root(root, rel_path)
        move_to = ""
        if body and body[0].startswith("*** Move to: "):
            move_to = _patch_relative_path(body[0].removeprefix("*** Move to: "))
            body = body[1:]
        if action == "add":
            if target.exists():
                raise MCPError(code="path_exists", message=f"file already exists: {rel_path}")
            add_lines: List[str] = []
            for line in body:
                if not line.startswith("+"):
                    raise MCPError(code="invalid_patch", message="Add File lines must start with +")
                add_lines.append(line[1:])
            target.parent.mkdir(parents=True, exist_ok=True)
            atomic_write_text(target, "\n".join(add_lines) + ("\n" if add_lines else ""), encoding="utf-8")
            operations.append({"action": "add", "path": rel_path})
            continue
        if action == "delete":
            if body:
                raise MCPError(code="invalid_patch", message="Delete File sections must not contain hunks")
            if not target.exists() or not target.is_file():
                raise MCPError(code="not_found", message=f"file not found: {rel_path}")
            target.unlink()
            operations.append({"action": "delete", "path": rel_path})
            continue
        if action == "update":
            if not target.exists() or not target.is_file():
                raise MCPError(code="not_found", message=f"file not found: {rel_path}")
            original = target.read_text(encoding="utf-8")
            hunks: List[List[str]] = []
            current_hunk: List[str] = []
            for line in body:
                if line.startswith("@@"):
                    if current_hunk:
                        hunks.append(current_hunk)
                    current_hunk = [line]
                else:
                    if not current_hunk:
                        raise MCPError(code="invalid_patch", message="Update File content must be inside @@ hunks")
                    current_hunk.append(line)
            if current_hunk:
                hunks.append(current_hunk)
            updated = _apply_update_hunks(original, hunks) if hunks else original
            dest = _resolve_under_root(root, move_to) if move_to else target
            dest.parent.mkdir(parents=True, exist_ok=True)
            atomic_write_text(dest, updated, encoding="utf-8")
            if move_to and dest != target:
                target.unlink()
            operations.append({"action": "update", "path": rel_path, **({"move_to": move_to} if move_to else {})})
            continue
    return {"root_path": str(root), "applied": True, "operations": operations}


def repo_tool(
    *,
    group_id: str,
    action: str,
    path: str = "",
    content: str = "",
    patch: str = "",
    dest_path: str = "",
    old_text: str = "",
    new_text: str = "",
    expected_sha256: str = "",
    expected_replacements: Any = None,
    replace_all: bool = False,
    recursive: bool = False,
    exist_ok: bool = True,
    max_bytes: Any = _DEFAULT_READ_BYTES,
    limit: Any = 200,
    offset: Any = 1,
    depth: Any = 2,
    start_line: Any = None,
    end_line: Any = None,
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

    if act == "list_dir":
        entries, truncated, total = _list_dir_entries(
            root,
            target,
            offset=_coerce_int(offset, default=1, minimum=1, maximum=100000),
            limit=_coerce_int(limit, default=50, minimum=1, maximum=500),
            depth=_coerce_int(depth, default=2, minimum=1, maximum=8),
            include_hidden=bool(include_hidden),
        )
        return {
            "root_path": str(root),
            "path": _relative(root, target),
            "entries": entries,
            "total": total,
            "truncated": truncated,
        }

    if act == "read":
        text, truncated, size, sha256 = _read_text(
            target,
            max_bytes=_coerce_int(max_bytes, default=_DEFAULT_READ_BYTES, minimum=1, maximum=_MAX_READ_BYTES),
        )
        content, start, end, total_lines = _line_slice(text, start_line=start_line, end_line=end_line)
        return {
            "root_path": str(root),
            "path": _relative(root, target),
            "content": content,
            "bytes": size,
            "sha256": sha256,
            "truncated": truncated,
            "start_line": start,
            "end_line": end,
            "total_lines": total_lines,
        }

    if act == "write":
        payload = str(content or "")
        if len(payload) > _MAX_WRITE_CHARS:
            raise MCPError(code="content_too_large", message="content is too large")
        old_sha256 = _assert_expected_sha256(target, expected_sha256) if str(expected_sha256 or "").strip() else ""
        target.parent.mkdir(parents=True, exist_ok=True)
        atomic_write_text(target, payload, encoding="utf-8")
        return {
            "root_path": str(root),
            "path": _relative(root, target),
            "written": True,
            "bytes": len(payload.encode("utf-8")),
            "old_sha256": old_sha256,
            "sha256": _sha256_file(target),
        }

    if act == "replace":
        old = str(old_text or "")
        if not old:
            raise MCPError(code="missing_old_text", message="old_text is required for replace")
        if not target.exists() or not target.is_file():
            raise MCPError(code="not_found", message=f"file not found: {target}")
        old_sha256 = _assert_expected_sha256(target, expected_sha256)
        text, truncated, _size, _sha256 = _read_text(target, max_bytes=_MAX_READ_BYTES)
        if truncated:
            raise MCPError(code="file_too_large", message="file is too large for replace; use a smaller file or cccc_shell")
        count = text.count(old)
        if count <= 0:
            raise MCPError(
                code="old_text_not_found",
                message="old_text was not found; re-read the file and retry with exact current text",
            )
        expected = _coerce_optional_int(expected_replacements, minimum=1, maximum=10000)
        if expected is not None and count != expected:
            raise MCPError(
                code="replacement_count_mismatch",
                message=f"old_text matched {count} times, expected {expected}; re-read and use a more specific old_text",
            )
        if not bool(replace_all) and count != 1:
            raise MCPError(
                code="ambiguous_old_text",
                message=f"old_text matched {count} times; pass replace_all=true or use a more specific old_text",
            )
        updated = text.replace(old, str(new_text or ""), -1 if bool(replace_all) else 1)
        if len(updated) > _MAX_WRITE_CHARS:
            raise MCPError(code="content_too_large", message="updated content is too large")
        atomic_write_text(target, updated, encoding="utf-8")
        return {
            "root_path": str(root),
            "path": _relative(root, target),
            "replaced": True,
            "replacements": count if bool(replace_all) else 1,
            "old_sha256": old_sha256,
            "sha256": _sha256_file(target),
        }

    if act == "multi_replace":
        replacements = content if isinstance(content, list) else None
        if replacements is None:
            raise MCPError(code="missing_replacements", message="replacements list is required for multi_replace")
        if not target.exists() or not target.is_file():
            raise MCPError(code="not_found", message=f"file not found: {target}")
        old_sha256 = _assert_expected_sha256(target, expected_sha256)
        text, truncated, _size, _sha256 = _read_text(target, max_bytes=_MAX_READ_BYTES)
        if truncated:
            raise MCPError(code="file_too_large", message="file is too large for multi_replace")
        updated = text
        applied = 0
        for index, item in enumerate(replacements):
            if not isinstance(item, Mapping):
                raise MCPError(code="invalid_replacement", message=f"replacement {index} must be an object")
            old = str(item.get("old_text") or "")
            if not old:
                raise MCPError(code="missing_old_text", message=f"replacement {index} missing old_text")
            new = str(item.get("new_text") or "")
            count = updated.count(old)
            if count <= 0:
                raise MCPError(code="old_text_not_found", message=f"replacement {index} old_text was not found")
            expected = _coerce_optional_int(item.get("expected_replacements"), minimum=1, maximum=10000)
            if expected is not None and count != expected:
                raise MCPError(
                    code="replacement_count_mismatch",
                    message=f"replacement {index} old_text matched {count} times, expected {expected}",
                )
            replace_all_item = bool(item.get("replace_all"))
            if not replace_all_item and count != 1:
                raise MCPError(code="ambiguous_old_text", message=f"replacement {index} old_text matched {count} times")
            updated = updated.replace(old, new, -1 if replace_all_item else 1)
            applied += count if replace_all_item else 1
        if len(updated) > _MAX_WRITE_CHARS:
            raise MCPError(code="content_too_large", message="updated content is too large")
        atomic_write_text(target, updated, encoding="utf-8")
        return {
            "root_path": str(root),
            "path": _relative(root, target),
            "replaced": True,
            "replacements": applied,
            "old_sha256": old_sha256,
            "sha256": _sha256_file(target),
        }

    if act == "apply_patch":
        payload = str(patch or "")
        if not payload.strip():
            raise MCPError(code="missing_patch", message="patch is required")
        if len(payload.encode("utf-8")) > _MAX_PATCH_BYTES:
            raise MCPError(code="patch_too_large", message="patch is too large")
        return apply_codex_patch_tool(group_id=group_id, patch=payload)

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

    raise MCPError(code="invalid_action", message="cccc_repo action must be info|list|list_dir|read|write|replace|multi_replace|apply_patch|mkdir|delete|move")


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
            "ok": False,
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
        "ok": int(result.returncode) == 0,
        "timed_out": False,
        "returncode": int(result.returncode),
        "stdout": stdout,
        "stderr": stderr,
        "stdout_truncated": stdout_truncated,
        "stderr_truncated": stderr_truncated,
    }


def exec_command_tool(
    *,
    group_id: str,
    command: str,
    cwd: str = "",
    yield_time_ms: Any = 1000,
    max_output_bytes: Any = _DEFAULT_OUTPUT_BYTES,
    timeout_s: Any = _MAX_SHELL_TIMEOUT_S,
    env: Any = None,
) -> Dict[str, Any]:
    """Start a Codex-style shell session and return output or a session id."""
    _group, root, scope_key = _repo_root(group_id)
    cmd = str(command or "").strip()
    if not cmd:
        raise MCPError(code="missing_command", message="command is required")
    workdir = _resolve_under_root(root, cwd or ".")
    if not workdir.exists() or not workdir.is_dir():
        raise MCPError(code="invalid_cwd", message="cwd must be an existing directory under the active scope root")
    timeout = _coerce_int(timeout_s, default=_MAX_SHELL_TIMEOUT_S, minimum=1, maximum=_MAX_SHELL_TIMEOUT_S)
    output_limit = _coerce_int(max_output_bytes, default=_DEFAULT_OUTPUT_BYTES, minimum=1, maximum=_MAX_OUTPUT_BYTES)
    proc_env = os.environ.copy()
    if isinstance(env, Mapping):
        for key, value in env.items():
            k = str(key or "").strip()
            if k:
                proc_env[k] = str(value)
    _prune_exec_sessions()
    session_id = _new_exec_session_id()
    try:
        proc = subprocess.Popen(
            cmd,
            shell=True,
            cwd=str(workdir),
            env=proc_env,
            text=True,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            bufsize=1,
        )
    except OSError as exc:
        raise MCPError(code="exec_failed", message=f"failed to start command: {exc}") from exc
    session = _ExecSession(
        session_id=session_id,
        proc=proc,
        root=root,
        workdir=workdir,
        command=cmd,
        expires_at=time.monotonic() + timeout,
    )
    if proc.stdout is not None:
        threading.Thread(target=_reader_thread, args=(proc.stdout, session.output), daemon=True).start()
    with _EXEC_LOCK:
        _EXEC_SESSIONS[session_id] = session
    _sleep_for_yield(yield_time_ms)
    timed_out = time.monotonic() > session.expires_at and proc.poll() is None
    if timed_out:
        proc.terminate()
    output, output_truncated = _drain_exec_output(session, max_output_bytes=output_limit)
    returncode = proc.poll()
    if returncode is not None:
        with _EXEC_LOCK:
            _EXEC_SESSIONS.pop(session_id, None)
    return {
        "root_path": str(root),
        "scope_key": scope_key,
        "cwd": _relative(root, workdir),
        "command": cmd,
        "session_id": session_id if returncode is None else None,
        "running": returncode is None,
        "ok": returncode == 0 if returncode is not None else None,
        "timed_out": timed_out,
        "returncode": returncode,
        "output": output,
        "output_truncated": output_truncated,
    }


def write_stdin_tool(
    *,
    session_id: str,
    chars: str = "",
    yield_time_ms: Any = 1000,
    max_output_bytes: Any = _DEFAULT_OUTPUT_BYTES,
    terminate: bool = False,
) -> Dict[str, Any]:
    """Write to or poll a Codex-style shell session."""
    sid = str(session_id or "").strip()
    if not sid:
        raise MCPError(code="missing_session_id", message="session_id is required")
    with _EXEC_LOCK:
        session = _EXEC_SESSIONS.get(sid)
    if session is None:
        raise MCPError(code="session_not_found", message=f"exec session not found: {sid}")
    session.last_used_at = time.monotonic()
    proc = session.proc
    timed_out = time.monotonic() > session.expires_at and proc.poll() is None
    if timed_out:
        proc.terminate()
    if chars and proc.stdin is not None and proc.poll() is None:
        try:
            proc.stdin.write(str(chars))
            proc.stdin.flush()
        except OSError as exc:
            raise MCPError(code="stdin_write_failed", message=f"failed to write stdin: {exc}") from exc
    if terminate and proc.poll() is None:
        proc.terminate()
    _sleep_for_yield(yield_time_ms)
    output_limit = _coerce_int(max_output_bytes, default=_DEFAULT_OUTPUT_BYTES, minimum=1, maximum=_MAX_OUTPUT_BYTES)
    output, output_truncated = _drain_exec_output(session, max_output_bytes=output_limit)
    returncode = proc.poll()
    if returncode is not None:
        with _EXEC_LOCK:
            _EXEC_SESSIONS.pop(sid, None)
    return {
        "root_path": str(session.root),
        "cwd": _relative(session.root, session.workdir),
        "command": session.command,
        "session_id": sid if returncode is None else None,
        "running": returncode is None,
        "ok": returncode == 0 if returncode is not None else None,
        "timed_out": timed_out,
        "returncode": returncode,
        "output": output,
        "output_truncated": output_truncated,
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
    result["ok"] = (not bool(result.get("timed_out"))) and int(result.get("returncode") or 0) == 0
    return result
