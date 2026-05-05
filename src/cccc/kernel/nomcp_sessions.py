from __future__ import annotations

import base64
import hashlib
import hmac
import html
import json
import os
import secrets
import subprocess
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Dict, Iterable, Optional

from ..contracts.v1 import ChatMessageData
from ..paths import ensure_home
from ..util.file_lock import acquire_lockfile, release_lockfile
from ..util.fs import atomic_write_text
from ..util.time import utc_now_iso
from .group import Group, load_group
from .ledger import append_event
from .prompt_files import resolve_active_scope_root

_SID_PREFIX = "nomcp_"
_SECRET_PREFIX = "nomcps_"
_DEFAULT_EXPIRES_SECONDS = 24 * 60 * 60
_MAX_READ_LINES = 500
_DEFAULT_READ_LINES = 300
_MAX_READ_BYTES = 512 * 1024
_MAX_DIFF_BYTES = 160 * 1024
_MAX_SEARCH_MATCHES = 80
_MAX_ADVISORY_BYTES = 32 * 1024
_MAX_GET_ADVISORY_BYTES = 12 * 1024

_DENY_DIR_NAMES = {
    ".git",
    "node_modules",
    "dist",
    "build",
    "__pycache__",
    ".venv",
    "venv",
    "state",
    "blobs",
}
_DENY_SUFFIXES = {
    ".pem",
    ".key",
    ".sqlite",
    ".db",
    ".p12",
    ".crt",
    ".cer",
    ".log",
}


@dataclass(frozen=True)
class NomcpSessionError(Exception):
    code: str
    message: str
    status_code: int = 400

    def __str__(self) -> str:
        return self.message


def _sessions_dir(home: Optional[Path] = None) -> Path:
    base = Path(home) if home is not None else ensure_home()
    return base / "state" / "nomcp_sessions"


def _session_path(sid: str, home: Optional[Path] = None) -> Path:
    return _sessions_dir(home) / f"{sid}.json"


def _lock_path(sid: str, home: Optional[Path] = None) -> Path:
    return _sessions_dir(home) / f"{sid}.lock"


def _hash_secret(secret: str) -> str:
    return hashlib.sha256(str(secret or "").encode("utf-8")).hexdigest()


def _preview(secret: str) -> str:
    raw = str(secret or "")
    if len(raw) <= 12:
        return "****"
    return raw[:7] + "..." + raw[-4:]


def _utc_timestamp(value: str) -> float:
    import datetime as _dt

    raw = str(value or "").strip()
    if not raw:
        return 0.0
    try:
        if raw.endswith("Z"):
            raw = raw[:-1] + "+00:00"
        return _dt.datetime.fromisoformat(raw).timestamp()
    except Exception:
        return 0.0


def _iso_after(seconds: int) -> str:
    import datetime as _dt

    return (
        _dt.datetime.now(_dt.timezone.utc)
        + _dt.timedelta(seconds=max(60, int(seconds or _DEFAULT_EXPIRES_SECONDS)))
    ).isoformat().replace("+00:00", "Z")


def _new_sid() -> str:
    return f"{_SID_PREFIX}{secrets.token_urlsafe(18).replace('-', '').replace('_', '')[:24]}"


def _new_secret() -> str:
    return f"{_SECRET_PREFIX}{secrets.token_urlsafe(32)}"


def _load_session_file(sid: str, home: Optional[Path] = None) -> Optional[Dict[str, Any]]:
    clean = str(sid or "").strip()
    if not clean.startswith(_SID_PREFIX):
        return None
    path = _session_path(clean, home)
    if not path.exists():
        return None
    try:
        raw = json.loads(path.read_text(encoding="utf-8"))
    except Exception:
        return None
    return raw if isinstance(raw, dict) else None


def _save_session_file(session: Dict[str, Any], home: Optional[Path] = None) -> None:
    sid = str(session.get("sid") or "").strip()
    if not sid:
        raise ValueError("missing sid")
    path = _session_path(sid, home)
    path.parent.mkdir(parents=True, exist_ok=True)
    atomic_write_text(path, json.dumps(session, ensure_ascii=False, indent=2, sort_keys=True) + "\n")


def _with_session_lock(sid: str, fn):
    lock = _lock_path(sid)
    lock.parent.mkdir(parents=True, exist_ok=True)
    lk = acquire_lockfile(lock, blocking=True)
    try:
        return fn()
    finally:
        release_lockfile(lk)


def _active_scope(group: Group, requested_scope_key: str = "") -> tuple[str, Path]:
    wanted = str(requested_scope_key or "").strip() or str(group.doc.get("active_scope_key") or "").strip()
    scopes = group.doc.get("scopes") if isinstance(group.doc.get("scopes"), list) else []
    if wanted:
        for item in scopes:
            if not isinstance(item, dict):
                continue
            if str(item.get("scope_key") or "").strip() == wanted:
                url = str(item.get("url") or "").strip()
                if not url:
                    break
                return wanted, Path(url).expanduser().resolve()
    root = resolve_active_scope_root(group)
    if root is None:
        raise NomcpSessionError("missing_scope", "group has no active project scope", 400)
    return wanted or str(group.doc.get("active_scope_key") or "").strip(), root


def _run_git(root: Path, args: list[str], *, timeout: float = 5.0, max_bytes: int = 96 * 1024) -> str:
    try:
        proc = subprocess.run(
            ["git", "-C", str(root), *args],
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            timeout=timeout,
            check=False,
        )
    except Exception:
        return ""
    text = str(proc.stdout or "")
    if len(text.encode("utf-8", errors="replace")) > max_bytes:
        raw = text.encode("utf-8", errors="replace")[:max_bytes]
        text = raw.decode("utf-8", errors="replace") + "\n[truncated]\n"
    return text.strip()


def _digest(text: str) -> str:
    return hashlib.sha256(str(text or "").encode("utf-8", errors="replace")).hexdigest()


def _status_digest(root: Path) -> str:
    return _digest(_run_git(root, ["status", "--short"], max_bytes=64 * 1024))


def _diff_digest(root: Path) -> str:
    return _digest(_run_git(root, ["diff", "--stat"], max_bytes=64 * 1024) + "\n" + _run_git(root, ["diff", "--name-status"], max_bytes=64 * 1024))


def _git_head(root: Path) -> str:
    return _run_git(root, ["rev-parse", "--short=12", "HEAD"], max_bytes=1024)


def _normalize_rel_path(path: str) -> str:
    raw = str(path or "").strip().replace("\\", "/")
    if not raw or raw.startswith("/") or raw.startswith("~"):
        raise NomcpSessionError("invalid_path", "path must be repo-relative", 400)
    parts = [part for part in raw.split("/") if part and part != "."]
    if any(part == ".." for part in parts):
        raise NomcpSessionError("invalid_path", "path traversal is not allowed", 400)
    return "/".join(parts)


def _denylisted(rel_path: str) -> bool:
    if not rel_path:
        return True
    parts = rel_path.split("/")
    name = parts[-1]
    if name.startswith(".env"):
        return True
    if any(part in _DENY_DIR_NAMES for part in parts):
        return True
    lower = name.lower()
    return any(lower.endswith(suffix) for suffix in _DENY_SUFFIXES)


def _glob_allowed(rel_path: str) -> bool:
    if rel_path in {"README.md", "PROJECT.md", "pyproject.toml", "package.json"}:
        return True
    if rel_path.startswith("docs/"):
        return True
    if rel_path.startswith("src/") and rel_path.endswith(".py"):
        return True
    if rel_path.startswith("tests/") and rel_path.endswith(".py"):
        return True
    if rel_path.startswith("web/src/") and (rel_path.endswith(".ts") or rel_path.endswith(".tsx")):
        return True
    return False


def _session_allowed(rel_path: str, allowed_paths: Any) -> bool:
    if not isinstance(allowed_paths, list) or not allowed_paths:
        return True
    for raw in allowed_paths:
        try:
            allowed = _normalize_rel_path(str(raw or ""))
        except NomcpSessionError:
            continue
        if rel_path == allowed or rel_path.startswith(allowed.rstrip("/") + "/"):
            return True
    return False


def _resolve_allowed_file(session: Dict[str, Any], rel_path: str) -> Path:
    normalized = _normalize_rel_path(rel_path)
    if _denylisted(normalized) or not _glob_allowed(normalized) or not _session_allowed(normalized, session.get("allowed_paths")):
        raise NomcpSessionError("path_not_allowed", f"path is not allowlisted for this No-MCP session: {normalized}", 403)
    root = Path(str(session.get("repo_root") or "")).expanduser().resolve()
    target = (root / normalized).resolve()
    try:
        target.relative_to(root)
    except ValueError as exc:
        raise NomcpSessionError("path_out_of_scope", "path escapes the active scope", 403) from exc
    if not target.exists() or not target.is_file():
        raise NomcpSessionError("file_not_found", f"file not found: {normalized}", 404)
    return target


def _iter_allowed_files(session: Dict[str, Any], *, limit: int = 240) -> list[str]:
    root = Path(str(session.get("repo_root") or "")).expanduser().resolve()
    if not root.exists():
        return []
    out: list[str] = []
    for dirpath, dirnames, filenames in os.walk(root):
        rel_dir = Path(dirpath).resolve().relative_to(root).as_posix() if Path(dirpath).resolve() != root else ""
        dirnames[:] = [
            name for name in dirnames
            if not _denylisted(f"{rel_dir}/{name}".strip("/"))
        ]
        for filename in sorted(filenames):
            rel = f"{rel_dir}/{filename}".strip("/")
            if _denylisted(rel) or not _glob_allowed(rel) or not _session_allowed(rel, session.get("allowed_paths")):
                continue
            try:
                path = (root / rel).resolve()
                path.relative_to(root)
            except Exception:
                continue
            out.append(rel)
            if len(out) >= limit:
                return out
    return out


def _changed_files(root: Path) -> list[str]:
    raw = _run_git(root, ["diff", "--name-only"], max_bytes=64 * 1024)
    status = _run_git(root, ["status", "--short"], max_bytes=64 * 1024)
    items: list[str] = []
    for line in raw.splitlines():
        text = line.strip()
        if not text:
            continue
        if " -> " in text:
            text = text.rsplit(" -> ", 1)[-1].strip()
        if text and text not in items:
            items.append(text)
    for line in status.splitlines():
        text = line.rstrip()
        if not text:
            continue
        path = text[3:].strip() if len(text) > 3 else text.strip()
        if " -> " in path:
            path = path.rsplit(" -> ", 1)[-1].strip()
        if path and path not in items:
            items.append(path)
    return items


def create_nomcp_session(
    *,
    group_id: str,
    title: str = "",
    brief: str = "",
    reply_to_event_id: str = "",
    recipient: str = "user",
    scope_key: str = "",
    allowed_paths: Optional[list[str]] = None,
    expires_in_seconds: int = _DEFAULT_EXPIRES_SECONDS,
) -> Dict[str, Any]:
    gid = str(group_id or "").strip()
    if not gid:
        raise NomcpSessionError("missing_group_id", "group_id is required", 400)
    group = load_group(gid)
    if group is None:
        raise NomcpSessionError("group_not_found", f"group not found: {gid}", 404)
    effective_scope_key, root = _active_scope(group, scope_key)
    sid = _new_sid()
    secret = _new_secret()
    now = utc_now_iso()
    status_digest = _status_digest(root)
    diff_digest = _diff_digest(root)
    session = {
        "schema": "cccc.nomcp.session.v1",
        "sid": sid,
        "token_hash": _hash_secret(secret),
        "token_preview": _preview(secret),
        "created_at": now,
        "updated_at": now,
        "expires_at": _iso_after(expires_in_seconds),
        "revoked_at": "",
        "group_id": gid,
        "scope_key": effective_scope_key,
        "repo_root": str(root),
        "created_git_head": _git_head(root),
        "created_status_digest": status_digest,
        "created_diff_digest": diff_digest,
        "title": str(title or "").strip() or "No-MCP advisory session",
        "brief": str(brief or "").strip() or "Review the linked CCCC project context and return advisory findings.",
        "reply_to_event_id": str(reply_to_event_id or "").strip(),
        "recipient": str(recipient or "user").strip() or "user",
        "allowed_paths": [str(item or "").strip().replace("\\", "/") for item in (allowed_paths or []) if str(item or "").strip()],
        "sent_message_ids": [],
        "sent_messages": {},
    }
    _save_session_file(session)
    return {**mask_nomcp_session(session), "secret": secret}


def mask_nomcp_session(session: Dict[str, Any]) -> Dict[str, Any]:
    out = dict(session)
    out.pop("token_hash", None)
    out.pop("secret", None)
    out.pop("sent_messages", None)
    out["revoked"] = bool(str(session.get("revoked_at") or "").strip())
    out["expired"] = is_nomcp_session_expired(session)
    return out


def list_nomcp_sessions(group_id: str = "") -> list[Dict[str, Any]]:
    items: list[Dict[str, Any]] = []
    for path in sorted(_sessions_dir().glob("*.json")):
        raw = _load_session_file(path.stem)
        if not isinstance(raw, dict):
            continue
        if group_id and str(raw.get("group_id") or "") != str(group_id or "").strip():
            continue
        items.append(mask_nomcp_session(raw))
    items.sort(key=lambda item: str(item.get("created_at") or ""), reverse=True)
    return items


def get_nomcp_session(sid: str) -> Optional[Dict[str, Any]]:
    raw = _load_session_file(sid)
    return mask_nomcp_session(raw) if isinstance(raw, dict) else None


def revoke_nomcp_session(sid: str) -> bool:
    clean = str(sid or "").strip()
    if not clean:
        return False

    def _update() -> bool:
        session = _load_session_file(clean)
        if not isinstance(session, dict):
            return False
        if not str(session.get("revoked_at") or "").strip():
            session["revoked_at"] = utc_now_iso()
            session["updated_at"] = utc_now_iso()
            _save_session_file(session)
        return True

    return bool(_with_session_lock(clean, _update))


def is_nomcp_session_expired(session: Dict[str, Any]) -> bool:
    expires_at = _utc_timestamp(str(session.get("expires_at") or ""))
    if expires_at <= 0:
        return False
    import time

    return time.time() > expires_at


def authorize_nomcp_session(sid: str, token: str) -> Dict[str, Any]:
    session = _load_session_file(sid)
    if not isinstance(session, dict):
        raise NomcpSessionError("session_not_found", "No-MCP session not found", 404)
    if str(session.get("revoked_at") or "").strip():
        raise NomcpSessionError("revoked", "No-MCP session has been revoked", 410)
    if is_nomcp_session_expired(session):
        raise NomcpSessionError("expired", "No-MCP session has expired", 410)
    expected = str(session.get("token_hash") or "").strip()
    actual = _hash_secret(str(token or "").strip())
    if not expected or not token or not hmac.compare_digest(expected, actual):
        raise NomcpSessionError("invalid_token", "invalid No-MCP session token", 403)
    return session


def nomcp_token_url(base_url: str, sid: str, secret: str) -> str:
    from urllib.parse import urlencode

    base = str(base_url or "").rstrip("/")
    return f"{base}/nomcp/s/{sid}?{urlencode({'token': secret})}"


def _link(path: str, token: str, label: str) -> str:
    from urllib.parse import parse_qsl, urlencode, urlsplit, urlunsplit

    parts = urlsplit(path)
    query = parse_qsl(parts.query, keep_blank_values=True)
    query.append(("token", token))
    href = urlunsplit((parts.scheme, parts.netloc, parts.path, urlencode(query), parts.fragment))
    return f'<a href="{html.escape(href, quote=True)}">{html.escape(label)}</a>'


def _read_url(base: str, rel_path: str) -> str:
    from urllib.parse import urlencode

    return f"{base}/read?{urlencode({'path': rel_path})}"


def _link_markdown(path: str, token: str) -> str:
    from urllib.parse import parse_qsl, urlencode, urlsplit, urlunsplit

    parts = urlsplit(path)
    query = parse_qsl(parts.query, keep_blank_values=True)
    query.append(("token", token))
    return urlunsplit((parts.scheme, parts.netloc, parts.path, urlencode(query), parts.fragment))


def session_status(session: Dict[str, Any]) -> Dict[str, Any]:
    root = Path(str(session.get("repo_root") or "")).expanduser().resolve()
    status = _run_git(root, ["status", "--short"], max_bytes=96 * 1024)
    branch = _run_git(root, ["branch", "--show-current"], max_bytes=1024)
    head = _git_head(root)
    current_status_digest = _digest(status)
    current_diff_digest = _diff_digest(root)
    changed = _changed_files(root)
    allowed_changed = [
        item for item in changed
        if not _denylisted(item) and _glob_allowed(item) and _session_allowed(item, session.get("allowed_paths"))
    ]
    return {
        "group_id": session.get("group_id"),
        "scope_key": session.get("scope_key"),
        "repo_label": root.name,
        "branch": branch,
        "head": head,
        "status_short": status,
        "changed_files": allowed_changed,
        "workspace_changed_since_created": (
            bool(session.get("created_status_digest")) and current_status_digest != str(session.get("created_status_digest") or "")
        ) or (
            bool(session.get("created_diff_digest")) and current_diff_digest != str(session.get("created_diff_digest") or "")
        ),
    }


def session_resources(session: Dict[str, Any], token: str = "") -> Dict[str, Any]:
    root = Path(str(session.get("repo_root") or "")).expanduser().resolve()
    allowed = _iter_allowed_files(session)
    changed = _changed_files(root)
    changed_allowed = [item for item in changed if item in allowed]
    ordered: list[str] = []
    for item in changed_allowed + allowed:
        if item not in ordered:
            ordered.append(item)
    return {
        "resources": ordered,
        "changed_files": changed_allowed,
        "count": len(ordered),
        "truncated": len(ordered) >= 240,
    }


def read_session_file(session: Dict[str, Any], rel_path: str, *, start: int = 1, end: int = 0) -> Dict[str, Any]:
    path = _resolve_allowed_file(session, rel_path)
    raw = path.read_bytes()
    if b"\0" in raw[: min(len(raw), 8192)]:
        raise NomcpSessionError("binary_file", "binary files are not available through No-MCP sessions", 415)
    if len(raw) > _MAX_READ_BYTES:
        raw = raw[:_MAX_READ_BYTES]
        truncated_bytes = True
    else:
        truncated_bytes = False
    try:
        text = raw.decode("utf-8")
    except UnicodeDecodeError as exc:
        if truncated_bytes:
            text = raw.decode("utf-8", errors="replace")
        else:
            raise NomcpSessionError("binary_file", "file is not valid UTF-8 text", 415) from exc
    lines = text.splitlines()
    total = len(lines)
    begin = max(1, int(start or 1))
    max_end = min(total, begin + _MAX_READ_LINES - 1)
    requested_end = int(end or min(total, begin + _DEFAULT_READ_LINES - 1))
    finish = max(begin, min(requested_end, max_end))
    selected = lines[begin - 1:finish]
    rendered = "\n".join(f"{idx:>5}  {line}" for idx, line in enumerate(selected, start=begin))
    rel = _normalize_rel_path(rel_path)
    return {
        "path": rel,
        "start": begin,
        "end": finish,
        "total_lines": total,
        "truncated": truncated_bytes or finish < requested_end or finish < total,
        "content": rendered,
    }


def search_session(session: Dict[str, Any], query: str) -> Dict[str, Any]:
    needle = str(query or "")
    if not needle:
        raise NomcpSessionError("missing_query", "q is required", 400)
    matches: list[Dict[str, Any]] = []
    for rel in _iter_allowed_files(session, limit=600):
        if len(matches) >= _MAX_SEARCH_MATCHES:
            break
        try:
            result = read_session_file(session, rel, start=1, end=_MAX_READ_LINES)
        except NomcpSessionError:
            continue
        content = str(result.get("content") or "")
        for raw in content.splitlines():
            if needle in raw:
                try:
                    line_no = int(raw.split(None, 1)[0])
                except Exception:
                    line_no = 0
                matches.append({"path": rel, "line": line_no, "text": raw[7:] if len(raw) > 7 else raw})
                if len(matches) >= _MAX_SEARCH_MATCHES:
                    break
    return {"query": needle, "literal": True, "matches": matches, "truncated": len(matches) >= _MAX_SEARCH_MATCHES}


def session_diff(session: Dict[str, Any], rel_path: str = "") -> Dict[str, Any]:
    root = Path(str(session.get("repo_root") or "")).expanduser().resolve()
    path = str(rel_path or "").strip()
    args = ["diff"]
    if path:
        normalized = _normalize_rel_path(path)
        if _denylisted(normalized) or not _glob_allowed(normalized) or not _session_allowed(normalized, session.get("allowed_paths")):
            raise NomcpSessionError("path_not_allowed", f"path is not allowlisted for this No-MCP session: {normalized}", 403)
        args.extend(["--", normalized])
    stat = _run_git(root, ["diff", "--stat"] + (["--", _normalize_rel_path(path)] if path else []), max_bytes=32 * 1024)
    name_status = _run_git(root, ["diff", "--name-status"] + (["--", _normalize_rel_path(path)] if path else []), max_bytes=32 * 1024)
    diff_text = _run_git(root, args, timeout=8.0, max_bytes=_MAX_DIFF_BYTES)
    return {
        "path": _normalize_rel_path(path) if path else "",
        "stat": stat,
        "name_status": name_status,
        "diff": diff_text,
        "truncated": "[truncated]" in diff_text,
    }


def _format_home(session: Dict[str, Any], token: str, *, markdown: bool = False) -> str:
    status = session_status(session)
    resources = session_resources(session, token)
    sid = str(session.get("sid") or "")
    base = f"/nomcp/s/{sid}"
    links = {
        "status": f"{base}/status",
        "diff": f"{base}/diff",
        "resources": f"{base}/resources",
        "search": f"{base}/search?q=TODO",
        "send": f"{base}/send",
    }
    warning = "Workspace changed since this No-MCP Session was created. Review current status before relying on older links." if status.get("workspace_changed_since_created") else ""
    changed = status.get("changed_files") if isinstance(status.get("changed_files"), list) else []
    title = str(session.get("title") or "No-MCP advisory session")
    brief = str(session.get("brief") or "")
    if markdown:
        lines = [
            f"# {title}",
            "",
            brief,
            "",
            "## Reading Order",
            "1. Read this task brief.",
            "2. Inspect status and diff summary.",
            "3. Read changed files before browsing unrelated files.",
            "4. Use literal search only when needed.",
            "5. Send advisory findings back through this session.",
            "",
        ]
        if warning:
            lines.extend([f"> {warning}", ""])
        lines.extend([
            "## Links",
            f"- Status: {links['status']}?token={token}",
            f"- Diff: {links['diff']}?token={token}",
            f"- Resources: {links['resources']}?token={token}",
            f"- Search: {links['search']}&token={token}",
            "",
            "## Changed Files",
        ])
        lines.extend([f"- {item}: {_link_markdown(_read_url(base, item), token)}" for item in changed[:40]] or ["- none"])
        lines.extend([
            "",
            "## Advisory Return",
            "Submit text with POST fields `msg_id` and `text`, or use GET `/send?msg_id=...&text_b64url=...` for limited fallback.",
        ])
        return "\n".join(lines) + "\n"
    body_links = "".join(
        f"<li>{_link(url, token, label)}</li>"
        for label, url in (
            ("Status", links["status"]),
            ("Diff", links["diff"]),
            ("Resources", links["resources"]),
            ("Literal search template", links["search"]),
        )
    )
    changed_links = "".join(
        f"<li>{_link(_read_url(base, item), token, item)}</li>"
        for item in changed[:40]
    ) or "<li>none</li>"
    warning_html = f'<p class="warn">{html.escape(warning)}</p>' if warning else ""
    return f"""<!doctype html>
<meta charset="utf-8">
<title>{html.escape(title)}</title>
<style>
body {{ font-family: system-ui, sans-serif; max-width: 920px; margin: 32px auto; padding: 0 18px; line-height: 1.55; color: #14151a; }}
code, pre {{ font-family: ui-monospace, SFMono-Regular, Menlo, monospace; }}
pre {{ background: #f6f7f9; padding: 12px; overflow: auto; border-radius: 8px; }}
.muted {{ color: #667085; }}
.warn {{ color: #9a3412; background: #fff7ed; padding: 10px 12px; border-radius: 8px; }}
</style>
<h1>{html.escape(title)}</h1>
<p>{html.escape(brief)}</p>
{warning_html}
<h2>Recommended Reading Order</h2>
<ol>
<li>Read the task brief.</li>
<li>Inspect status and diff summary.</li>
<li>Read changed files before browsing unrelated files.</li>
<li>Use literal search only when the first steps do not answer the question.</li>
<li>Send advisory findings back through this session.</li>
</ol>
<h2>Project Context</h2>
<p class="muted">Group <code>{html.escape(str(session.get("group_id") or ""))}</code>, scope <code>{html.escape(str(session.get("scope_key") or ""))}</code>.</p>
<ul>{body_links}</ul>
<h2>Changed Files</h2>
<ul>{changed_links}</ul>
<h2>Advisory Return</h2>
<form method="post" action="{html.escape(base + '/send?token=' + token, quote=True)}">
<p><label>Message id <input name="msg_id" value="advice-{html.escape(secrets.token_hex(4))}"></label></p>
<p><textarea name="text" rows="10" style="width:100%" placeholder="Advisory findings only. No local execution authority."></textarea></p>
<p><button type="submit">Send advisory message to CCCC</button></p>
</form>
"""


def render_session_home(session: Dict[str, Any], token: str, *, markdown: bool = False) -> str:
    return _format_home(session, token, markdown=markdown)


def render_resources(session: Dict[str, Any], token: str, *, markdown: bool = False) -> str:
    resources = session_resources(session, token)
    sid = str(session.get("sid") or "")
    base = f"/nomcp/s/{sid}"
    if markdown:
        lines = ["# No-MCP Resources", ""]
        for item in resources.get("resources") or []:
            lines.append(f"- {item}: {_link_markdown(_read_url(base, item), token)}")
        return "\n".join(lines) + "\n"
    items = "".join(
        f"<li>{_link(_read_url(base, item), token, item)}</li>"
        for item in resources.get("resources") or []
    )
    return f"<!doctype html><meta charset='utf-8'><h1>No-MCP Resources</h1><ul>{items}</ul>"


def render_text_page(title: str, text: str) -> str:
    return f"<!doctype html><meta charset='utf-8'><title>{html.escape(title)}</title><pre>{html.escape(text)}</pre>"


def decode_b64url(value: str) -> str:
    raw = str(value or "").strip()
    if not raw:
        return ""
    padding = "=" * ((4 - len(raw) % 4) % 4)
    try:
        return base64.urlsafe_b64decode((raw + padding).encode("ascii")).decode("utf-8")
    except Exception as exc:
        raise NomcpSessionError("invalid_payload", "text_b64url is not valid UTF-8 base64url", 400) from exc


def send_nomcp_advisory(sid: str, token: str, *, msg_id: str, text: str, title: str = "", via: str = "post") -> Dict[str, Any]:
    clean_msg_id = str(msg_id or "").strip()
    body = str(text or "").strip()
    if not clean_msg_id:
        raise NomcpSessionError("invalid_payload", "msg_id is required", 400)
    if not body:
        raise NomcpSessionError("invalid_payload", "text is required", 400)
    raw_len = len(body.encode("utf-8", errors="replace"))
    max_len = _MAX_GET_ADVISORY_BYTES if via == "get" else _MAX_ADVISORY_BYTES
    if raw_len > max_len:
        raise NomcpSessionError("too_large", f"advisory text is too large (>{max_len} bytes)", 413)

    def _send() -> Dict[str, Any]:
        session = authorize_nomcp_session(sid, token)
        sent = session.get("sent_messages") if isinstance(session.get("sent_messages"), dict) else {}
        existing = sent.get(clean_msg_id) if isinstance(sent, dict) else None
        if isinstance(existing, dict):
            return {"status": "duplicate_ignored", "event_id": str(existing.get("event_id") or ""), "msg_id": clean_msg_id}
        group = load_group(str(session.get("group_id") or ""))
        if group is None:
            raise NomcpSessionError("group_not_found", "bound CCCC group no longer exists", 404)
        heading = str(title or "").strip()
        rendered = f"**{heading}**\n\n{body}" if heading else body
        recipient = str(session.get("recipient") or "user").strip() or "user"
        to = [recipient] if recipient else ["user"]
        data = ChatMessageData(
            text=rendered,
            format="markdown",
            priority="normal",
            reply_required=False,
            to=to,
            reply_to=str(session.get("reply_to_event_id") or "").strip() or None,
            source_platform="nomcp",
            source_user_name="No-MCP Advisory",
            source_user_id=str(session.get("sid") or ""),
            sender_title="No-MCP Advisory",
            client_id=f"nomcp:{session.get('sid')}:{clean_msg_id}",
            refs=[
                {
                    "kind": "text",
                    "title": "No-MCP advisory session",
                    "text": str(session.get("sid") or ""),
                    "source": "nomcp",
                    "cannot_execute_local_tools": True,
                }
            ],
        ).model_dump()
        event = append_event(
            group.ledger_path,
            kind="chat.message",
            group_id=group.group_id,
            scope_key=str(session.get("scope_key") or ""),
            by="nomcp-advisory",
            data=data,
        )
        event_id = str(event.get("id") or "")
        sent[clean_msg_id] = {"event_id": event_id, "at": utc_now_iso(), "via": via}
        session["sent_messages"] = sent
        ids = session.get("sent_message_ids") if isinstance(session.get("sent_message_ids"), list) else []
        if clean_msg_id not in ids:
            ids.append(clean_msg_id)
        session["sent_message_ids"] = ids
        session["updated_at"] = utc_now_iso()
        _save_session_file(session)
        return {"status": "accepted", "event_id": event_id, "msg_id": clean_msg_id}

    return _with_session_lock(str(sid or "").strip(), _send)
