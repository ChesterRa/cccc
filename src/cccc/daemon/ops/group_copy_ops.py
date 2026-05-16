from __future__ import annotations

import base64
import hashlib
import io
import json
import os
import platform
import shutil
import tempfile
import uuid
import zipfile
from pathlib import Path, PurePosixPath
from typing import Any, Dict, Iterable, List, Optional, Tuple

import yaml  # type: ignore

from ... import __version__
from ...contracts.v1.ipc import DaemonError, DaemonResponse
from ...kernel.group import load_group
from ...kernel.registry import load_registry
from ...kernel.scope import detect_scope
from ...paths import ensure_home
from ...util.fs import atomic_write_text
from ...util.time import utc_now_iso
from ..actors.actor_profile_runtime import actor_profile_ref
from ..actors.actor_profile_store import get_actor_profile, get_actor_profile_by_ref

GROUP_COPY_KIND = "cccc.group_copy"
GROUP_COPY_VERSION = 1

MAX_PACKAGE_BYTES = 100 * 1024 * 1024
MAX_UNCOMPRESSED_BYTES = 500 * 1024 * 1024
MAX_FILE_COUNT = 20_000
MAX_COMPRESSION_RATIO = 200.0

_WINDOWS_RESERVED_NAMES = {
    "CON",
    "PRN",
    "AUX",
    "NUL",
    *(f"COM{i}" for i in range(1, 10)),
    *(f"LPT{i}" for i in range(1, 10)),
}

_SENSITIVE_FILE_PARTS = {
    "auth",
    "cookie",
    "credential",
    "credentials",
    "oauth",
    "password",
    "secret",
    "secrets",
    "session_auth",
    "token",
    "tokens",
}

_RUNTIME_DIR_PARTS = {
    "browser",
    "browsers",
    "cdp",
    "chrome",
    "chrome_profile",
    "claude",
    "codex",
    "headless",
    "playwright",
    "projections",
    "pty",
    "runners",
    "runtime_sessions",
    "web_model",
}

_RESET_FILE_NAMES = {
    "preamble_sent.json",
    "unread_index.json",
}

_RESET_RELPATHS = {
    "state/assistants.json",
}

_LEDGER_CACHE_NAMES = {
    "index.sqlite3",
    "ledger.lock",
    "manifest.json",
    "status.sqlite3",
}

_RUNTIME_SUFFIXES = {
    ".db",
    ".db-shm",
    ".db-wal",
    ".lock",
    ".pid",
    ".sock",
    ".sqlite",
    ".sqlite3",
}


def _error(code: str, message: str, *, details: Optional[Dict[str, Any]] = None) -> DaemonResponse:
    return DaemonResponse(ok=False, error=DaemonError(code=code, message=message, details=details or {}))


def _slug_filename(value: str) -> str:
    out = "".join(ch.lower() if ch.isalnum() else "-" for ch in str(value or "").strip())
    out = "-".join(part for part in out.split("-") if part)
    return out[:80] or "group"


def _new_group_id() -> str:
    return "g_" + uuid.uuid4().hex[:12]


def _new_scope_key() -> str:
    return "s_" + uuid.uuid4().hex[:12]


def _safe_b64_decode(value: Any) -> bytes:
    raw = str(value or "").strip()
    if not raw:
        raise ValueError("missing package_b64")
    try:
        return base64.b64decode(raw.encode("ascii"), validate=True)
    except Exception as exc:
        raise ValueError("invalid package_b64") from exc


def _is_zip_symlink(info: zipfile.ZipInfo) -> bool:
    mode = (int(info.external_attr or 0) >> 16) & 0o170000
    return mode == 0o120000


def _has_control_chars(value: str) -> bool:
    return any(ord(ch) < 32 for ch in value)


def _is_windows_reserved(part: str) -> bool:
    stem = str(part or "").split(".")[0].strip().upper()
    return stem in _WINDOWS_RESERVED_NAMES


def _normalize_zip_name(name: str) -> str:
    raw = str(name or "")
    if not raw:
        raise ValueError("empty package entry")
    if "\x00" in raw or _has_control_chars(raw):
        raise ValueError(f"invalid package path: {raw!r}")
    if "\\" in raw:
        raise ValueError(f"backslash path not allowed: {raw}")
    if raw.startswith("/") or raw.startswith("//"):
        raise ValueError(f"absolute package path not allowed: {raw}")
    drive, _tail = os.path.splitdrive(raw)
    if drive:
        raise ValueError(f"drive-prefixed package path not allowed: {raw}")
    posix = PurePosixPath(raw)
    parts = posix.parts
    if not parts:
        raise ValueError("empty package entry")
    if any(part in {"", ".", ".."} for part in parts):
        raise ValueError(f"path traversal not allowed: {raw}")
    if any(_is_windows_reserved(part) for part in parts):
        raise ValueError(f"windows reserved path not allowed: {raw}")
    normalized = posix.as_posix()
    if normalized.startswith("../") or normalized == "..":
        raise ValueError(f"path traversal not allowed: {raw}")
    return normalized


def _validate_zip_bytes(data: bytes) -> Tuple[zipfile.ZipFile, List[zipfile.ZipInfo]]:
    if len(data) > MAX_PACKAGE_BYTES:
        raise ValueError("copy package too large")
    try:
        zf = zipfile.ZipFile(io.BytesIO(data), "r")
    except Exception as exc:
        raise ValueError("invalid zip package") from exc

    infos = zf.infolist()
    if len(infos) > MAX_FILE_COUNT:
        zf.close()
        raise ValueError("copy package has too many files")

    seen: set[str] = set()
    seen_folded: set[str] = set()
    total_uncompressed = 0
    total_compressed = 0
    for info in infos:
        normalized = _normalize_zip_name(info.filename)
        if normalized in seen:
            zf.close()
            raise ValueError(f"duplicate package entry: {normalized}")
        folded = normalized.casefold()
        if folded in seen_folded:
            zf.close()
            raise ValueError(f"case-insensitive package path collision: {normalized}")
        seen.add(normalized)
        seen_folded.add(folded)

        if _is_zip_symlink(info):
            zf.close()
            raise ValueError(f"symlink package entry not allowed: {normalized}")
        if info.is_dir():
            continue
        total_uncompressed += max(0, int(info.file_size or 0))
        total_compressed += max(0, int(info.compress_size or 0))
        if total_uncompressed > MAX_UNCOMPRESSED_BYTES:
            zf.close()
            raise ValueError("copy package uncompressed size too large")
        compressed = max(1, int(info.compress_size or 0))
        ratio = float(max(0, int(info.file_size or 0))) / float(compressed)
        if ratio > MAX_COMPRESSION_RATIO and int(info.file_size or 0) > 1024 * 1024:
            zf.close()
            raise ValueError("suspicious copy package compression ratio")
    if total_compressed > 0 and total_uncompressed / float(total_compressed) > MAX_COMPRESSION_RATIO:
        zf.close()
        raise ValueError("suspicious copy package compression ratio")
    return zf, infos


def _load_yaml_bytes(data: bytes) -> Dict[str, Any]:
    try:
        raw = yaml.safe_load(data.decode("utf-8")) or {}
    except Exception as exc:
        raise ValueError(f"invalid group.yaml: {exc}") from exc
    if not isinstance(raw, dict):
        raise ValueError("group.yaml must be a mapping")
    return raw


def _dump_yaml(doc: Dict[str, Any]) -> bytes:
    return yaml.safe_dump(doc, allow_unicode=True, sort_keys=False).encode("utf-8")


def _scrub_group_doc_for_copy(doc: Dict[str, Any]) -> Dict[str, Any]:
    out = json.loads(json.dumps(doc, ensure_ascii=False))
    actors = out.get("actors") if isinstance(out.get("actors"), list) else []
    for actor in actors:
        if not isinstance(actor, dict):
            continue
        actor["env"] = {}
        actor.pop("env_private", None)
        actor.pop("private_env", None)
    return out


def _actor_profile_exists(actor: Dict[str, Any]) -> bool:
    profile_id = str(actor.get("profile_id") or "").strip()
    if not profile_id:
        return True
    try:
        ref = actor_profile_ref(actor)
        if ref is not None and get_actor_profile_by_ref(ref) is not None:
            return True
    except Exception:
        pass
    try:
        return get_actor_profile(profile_id) is not None
    except Exception:
        return False


def _is_internal_actor_doc(actor: Dict[str, Any]) -> bool:
    return bool(str(actor.get("internal_kind") or "").strip())


def _sanitize_import_actor_profiles(doc: Dict[str, Any]) -> None:
    actors = doc.get("actors") if isinstance(doc.get("actors"), list) else []
    for actor in actors:
        if not isinstance(actor, dict):
            continue
        actor["env"] = {}
        actor.pop("env_private", None)
        actor.pop("private_env", None)
        if _actor_profile_exists(actor):
            continue
        for key in ("profile_id", "profile_scope", "profile_owner", "profile_revision_applied"):
            actor.pop(key, None)


def _rel_parts(rel: str) -> Tuple[str, ...]:
    return tuple(part.casefold() for part in PurePosixPath(rel).parts)


def _should_exclude_group_relpath(rel: str, *, is_dir: bool = False) -> bool:
    parts = _rel_parts(rel)
    name = parts[-1] if parts else ""
    if not parts:
        return False
    if PurePosixPath(rel).as_posix().casefold() in _RESET_RELPATHS:
        return True
    if name in _RESET_FILE_NAMES:
        return True
    if name.endswith("~") or name.endswith(".tmp"):
        return True
    if any(name.endswith(suffix) for suffix in _RUNTIME_SUFFIXES):
        return True
    if any(part.startswith(".deleting-") for part in parts):
        return True
    if any(part in _SENSITIVE_FILE_PARTS for part in parts):
        return True
    if len(parts) >= 2 and parts[0] == "state":
        if parts[1] in _RUNTIME_DIR_PARTS:
            return True
        if parts[1] == "ledger" and len(parts) >= 3:
            if parts[2] in _LEDGER_CACHE_NAMES or parts[2] in {"tmp"}:
                return True
        if parts[1] in {"group_space", "notebooklm", "notebooklm_auth"}:
            return True
    return False


def _iter_export_group_files(group_path: Path) -> Iterable[Tuple[str, bytes]]:
    for path in sorted(group_path.rglob("*")):
        try:
            rel_path = path.relative_to(group_path)
        except ValueError:
            continue
        rel = rel_path.as_posix()
        if not rel or _should_exclude_group_relpath(rel, is_dir=path.is_dir()):
            continue
        if path.is_symlink():
            continue
        if path.is_dir():
            continue
        if not path.is_file():
            continue
        if rel == "group.yaml":
            doc = _load_yaml_bytes(path.read_bytes())
            yield rel, _dump_yaml(_scrub_group_doc_for_copy(doc))
            continue
        yield rel, path.read_bytes()


def _content_digest(files: Iterable[Tuple[str, bytes]]) -> str:
    h = hashlib.sha256()
    for rel, data in sorted(files, key=lambda item: item[0]):
        h.update(rel.encode("utf-8"))
        h.update(b"\0")
        h.update(hashlib.sha256(data).hexdigest().encode("ascii"))
        h.update(b"\0")
    return "sha256:" + h.hexdigest()


def _manifest_for_group(*, group_id: str, title: str, files: List[Tuple[str, bytes]]) -> Dict[str, Any]:
    rels = {rel for rel, _data in files}
    return {
        "kind": GROUP_COPY_KIND,
        "version": GROUP_COPY_VERSION,
        "source_group_id": group_id,
        "source_title": title,
        "exported_at": utc_now_iso(),
        "cccc_version": __version__,
        "source_platform": platform.system().lower() or "unknown",
        "export_mode": "group_state_only",
        "workspace_included": False,
        "contains_secrets": False,
        "content_digest": _content_digest(files),
        "content": {
            "ledger": "ledger.jsonl" in rels,
            "context": any(rel.startswith("context/") for rel in rels),
            "blobs": any(rel.startswith("state/blobs/") for rel in rels),
            "memory": any(rel.startswith("state/memory") or rel == "state/memory.md" for rel in rels),
            "assistants": "state/assistants.json" in rels,
            "automation": "state/automation.json" in rels or "group.yaml" in rels,
        },
    }


def _copy_package_filename(group_id: str, title: str) -> str:
    return f"cccc-group--{_slug_filename(title)}--{group_id}--{utc_now_iso().replace(':', '').replace('-', '')[:15]}.zip"


def _build_package_bytes(group_id: str) -> Tuple[bytes, Dict[str, Any], str]:
    group = load_group(group_id)
    if group is None:
        raise ValueError(f"group not found: {group_id}")
    files = list(_iter_export_group_files(group.path))
    if not any(rel == "group.yaml" for rel, _ in files):
        raise ValueError("group.yaml missing")
    title = str(group.doc.get("title") or group_id)
    manifest = _manifest_for_group(group_id=group_id, title=title, files=files)
    out = io.BytesIO()
    with zipfile.ZipFile(out, "w", compression=zipfile.ZIP_DEFLATED) as zf:
        zf.writestr("manifest.json", json.dumps(manifest, ensure_ascii=False, sort_keys=True, indent=2))
        for rel, data in files:
            zf.writestr(f"group/{rel}", data)
    return out.getvalue(), manifest, _copy_package_filename(group_id, title)


def _read_package(data: bytes) -> Tuple[Dict[str, Any], Dict[str, bytes], Dict[str, Any]]:
    zf, infos = _validate_zip_bytes(data)
    try:
        entries: Dict[str, bytes] = {}
        manifest: Dict[str, Any] = {}
        for info in infos:
            name = _normalize_zip_name(info.filename)
            if info.is_dir():
                continue
            payload = zf.read(info)
            if name == "manifest.json":
                try:
                    raw_manifest = json.loads(payload.decode("utf-8"))
                except Exception as exc:
                    raise ValueError("invalid manifest.json") from exc
                if not isinstance(raw_manifest, dict):
                    raise ValueError("manifest.json must be an object")
                manifest = raw_manifest
                continue
            if name.startswith("group/"):
                rel = name[len("group/") :]
                if not rel:
                    continue
                entries[rel] = payload
        if not manifest:
            raise ValueError("manifest.json missing")
        _validate_manifest(manifest)
        if "group.yaml" not in entries:
            raise ValueError("group/group.yaml missing")
        expected_digest = str(manifest.get("content_digest") or "").strip()
        if expected_digest and expected_digest != _content_digest(list(entries.items())):
            raise ValueError("copy package content digest mismatch")
        group_doc = _load_yaml_bytes(entries["group.yaml"])
        return manifest, entries, group_doc
    finally:
        zf.close()


def _validate_manifest(manifest: Dict[str, Any]) -> None:
    if str(manifest.get("kind") or "") != GROUP_COPY_KIND:
        raise ValueError("unsupported copy package kind")
    try:
        version = int(manifest.get("version") or 0)
    except Exception:
        version = 0
    if version != GROUP_COPY_VERSION:
        raise ValueError("unsupported copy package version")
    if bool(manifest.get("workspace_included")):
        raise ValueError("workspace-including copy packages are not supported")
    if bool(manifest.get("contains_secrets")):
        raise ValueError("secret-containing copy packages are not supported")
    if str(manifest.get("export_mode") or "") != "group_state_only":
        raise ValueError("unsupported copy package export_mode")


def _primary_workspace_root(doc: Dict[str, Any]) -> str:
    active_key = str(doc.get("active_scope_key") or "").strip()
    scopes = doc.get("scopes") if isinstance(doc.get("scopes"), list) else []
    selected: Optional[Dict[str, Any]] = None
    for item in scopes:
        if not isinstance(item, dict):
            continue
        if active_key and str(item.get("scope_key") or "").strip() == active_key:
            selected = item
            break
        if selected is None:
            selected = item
    if selected is None:
        return ""
    return str(selected.get("url") or "").strip()


def _scope_label(path_text: str) -> str:
    raw = str(path_text or "").strip().rstrip("/\\")
    if not raw:
        return "workspace"
    return Path(raw).name or "workspace"


def _build_preview(manifest: Dict[str, Any], group_doc: Dict[str, Any]) -> Dict[str, Any]:
    gid = str(group_doc.get("group_id") or manifest.get("source_group_id") or "").strip()
    title = str(group_doc.get("title") or manifest.get("source_title") or gid or "Imported group")
    workspace_root = _primary_workspace_root(group_doc)
    actors = []
    needs_chatgpt = False
    raw_actors = group_doc.get("actors") if isinstance(group_doc.get("actors"), list) else []
    for actor in raw_actors:
        if not isinstance(actor, dict):
            continue
        if _is_internal_actor_doc(actor):
            continue
        runtime = str(actor.get("runtime") or "").strip()
        if runtime == "web_model":
            needs_chatgpt = True
        actors.append(
            {
                "id": str(actor.get("id") or ""),
                "title": str(actor.get("title") or ""),
                "runtime": runtime,
                "runner": str(actor.get("runner") or ""),
                "enabled": bool(actor.get("enabled", True)),
            }
        )
    reg = load_registry()
    conflict = bool(gid and load_group(gid) is not None)
    return {
        "manifest": manifest,
        "source_group_id": gid,
        "source_title": title,
        "actor_count": len(actors),
        "actors": actors,
        "source_workspace_root": workspace_root,
        "workspace_root_exists": bool(workspace_root and Path(workspace_root).expanduser().exists()),
        "group_id_conflict": conflict,
        "target_default_scope_conflict": bool(
            group_doc.get("active_scope_key")
            and reg.defaults.get(str(group_doc.get("active_scope_key") or "")) not in {None, "", gid}
        ),
        "requires_reconnect": {
            "chatgpt_web_model": needs_chatgpt,
            "notebooklm_group_space": _doc_mentions_group_space(group_doc),
        },
        "workspace_included": False,
        "contains_secrets": False,
        "runtime_reset": {
            "actors_stopped": True,
            "group_running": False,
            "group_state": "idle",
            "browser_sessions_cleared": True,
            "runtime_sessions_cleared": True,
        },
    }


def _doc_mentions_group_space(doc: Dict[str, Any]) -> bool:
    text = json.dumps(doc, ensure_ascii=False).lower()
    return "notebooklm" in text or "group_space" in text or "group space" in text


def _write_package_entries_to_staging(entries: Dict[str, bytes], staging_group_dir: Path) -> None:
    staging_group_dir.mkdir(parents=True, exist_ok=True)
    for rel, data in entries.items():
        if _should_exclude_group_relpath(rel):
            continue
        target = staging_group_dir / Path(*PurePosixPath(rel).parts)
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(data)


def _remove_path_if_exists(path: Path) -> None:
    try:
        if path.is_dir() and not path.is_symlink():
            shutil.rmtree(path)
        else:
            path.unlink()
    except FileNotFoundError:
        pass


def _cleanup_imported_runtime_state(group_dir: Path) -> None:
    for path in list(group_dir.rglob("*")):
        try:
            rel = path.relative_to(group_dir).as_posix()
        except ValueError:
            continue
        if _should_exclude_group_relpath(rel, is_dir=path.is_dir()):
            _remove_path_if_exists(path)


def _choose_scope_for_workspace(
    *,
    workspace_root: str,
    final_group_id: str,
    old_scope_key: str,
    force_new_scope_identity: bool,
) -> Tuple[str, Dict[str, Any], bool]:
    root = str(workspace_root or "").strip()
    if not root:
        return old_scope_key, {}, False
    try:
        detected = detect_scope(Path(root).expanduser())
        scope_key = detected.scope_key
        scope_entry = {
            "scope_key": scope_key,
            "url": detected.url,
            "label": detected.label,
            "git_remote": detected.git_remote,
        }
    except Exception:
        scope_key = old_scope_key or _new_scope_key()
        scope_entry = {
            "scope_key": scope_key,
            "url": root,
            "label": _scope_label(root),
            "git_remote": "",
        }

    reg = load_registry()
    default_owner = str(reg.defaults.get(scope_key) or "").strip()
    if force_new_scope_identity or (default_owner and default_owner != final_group_id):
        scope_key = _new_scope_key()
        scope_entry["scope_key"] = scope_key
        return scope_key, scope_entry, True
    return scope_key, scope_entry, scope_key != old_scope_key


def _is_exact_cccc_home_path(value: str) -> bool:
    raw = str(value or "").strip()
    if not raw:
        return False
    try:
        return Path(raw).expanduser().resolve() == ensure_home().resolve()
    except Exception:
        return False


def _rewrite_actor_scope_references(doc: Dict[str, Any], *, old_scope_key: str, new_scope_key: str) -> None:
    if not old_scope_key or not new_scope_key or old_scope_key == new_scope_key:
        return
    actors = doc.get("actors") if isinstance(doc.get("actors"), list) else []
    for actor in actors:
        if not isinstance(actor, dict):
            continue
        if str(actor.get("default_scope_key") or "").strip() == old_scope_key:
            actor["default_scope_key"] = new_scope_key


def _apply_workspace_remap(
    doc: Dict[str, Any],
    *,
    workspace_root: str,
    final_group_id: str,
    force_new_scope_identity: bool,
) -> Tuple[str, str]:
    old_active_key = str(doc.get("active_scope_key") or "").strip()
    scopes = doc.get("scopes") if isinstance(doc.get("scopes"), list) else []
    if not isinstance(scopes, list):
        scopes = []
    if not str(workspace_root or "").strip():
        return old_active_key, old_active_key
    new_scope_key, new_entry, _changed = _choose_scope_for_workspace(
        workspace_root=workspace_root,
        final_group_id=final_group_id,
        old_scope_key=old_active_key,
        force_new_scope_identity=force_new_scope_identity,
    )
    replaced = False
    out_scopes: List[Dict[str, Any]] = []
    for item in scopes:
        if not isinstance(item, dict):
            continue
        if (old_active_key and str(item.get("scope_key") or "") == old_active_key) or (not old_active_key and not replaced):
            merged = dict(item)
            merged.update(new_entry)
            out_scopes.append(merged)
            replaced = True
        else:
            out_scopes.append(item)
    if not replaced:
        out_scopes.insert(0, dict(new_entry))
    doc["scopes"] = out_scopes
    doc["active_scope_key"] = new_scope_key
    _rewrite_actor_scope_references(doc, old_scope_key=old_active_key, new_scope_key=new_scope_key)
    return old_active_key, new_scope_key


def _rewrite_scope_dirs(group_dir: Path, old_scope_key: str, new_scope_key: str, scope_entry: Dict[str, Any]) -> None:
    if not new_scope_key:
        return
    scopes_dir = group_dir / "scopes"
    scopes_dir.mkdir(parents=True, exist_ok=True)
    old_dir = scopes_dir / old_scope_key if old_scope_key else None
    new_dir = scopes_dir / new_scope_key
    if old_dir is not None and old_dir.exists() and old_dir != new_dir:
        if new_dir.exists():
            shutil.rmtree(new_dir)
        old_dir.rename(new_dir)
    new_dir.mkdir(parents=True, exist_ok=True)
    now = utc_now_iso()
    scope_doc = dict(scope_entry)
    scope_doc.setdefault("v", 1)
    scope_doc.setdefault("created_at", now)
    scope_doc["updated_at"] = now
    atomic_write_text(new_dir / "scope.yaml", yaml.safe_dump(scope_doc, allow_unicode=True, sort_keys=False), encoding="utf-8")


def _rewrite_ledger_identity(path: Path, *, old_group_id: str, new_group_id: str, old_scope_key: str, new_scope_key: str) -> None:
    if not path.exists() or (old_group_id == new_group_id and old_scope_key == new_scope_key):
        return
    out_lines: List[str] = []
    try:
        lines = path.read_text(encoding="utf-8").splitlines()
    except Exception:
        return
    for line in lines:
        try:
            item = json.loads(line)
        except Exception:
            out_lines.append(line)
            continue
        if not isinstance(item, dict):
            out_lines.append(line)
            continue
        if str(item.get("group_id") or "") == old_group_id:
            item["group_id"] = new_group_id
        if old_scope_key and str(item.get("scope_key") or "") == old_scope_key:
            item["scope_key"] = new_scope_key
        out_lines.append(json.dumps(item, ensure_ascii=False, sort_keys=True))
    atomic_write_text(path, "\n".join(out_lines) + ("\n" if out_lines else ""), encoding="utf-8")


def _decode_and_read_package(args: Dict[str, Any]) -> Tuple[Dict[str, Any], Dict[str, bytes], Dict[str, Any]]:
    data = _safe_b64_decode(args.get("package_b64"))
    return _read_package(data)


def group_copy_export(args: Dict[str, Any]) -> DaemonResponse:
    group_id = str(args.get("group_id") or "").strip()
    if not group_id:
        return _error("missing_group_id", "missing group_id")
    try:
        package_bytes, manifest, filename = _build_package_bytes(group_id)
    except Exception as exc:
        return _error("group_copy_export_failed", str(exc))
    return DaemonResponse(
        ok=True,
        result={
            "package_b64": base64.b64encode(package_bytes).decode("ascii"),
            "filename": filename,
            "manifest": manifest,
        },
    )


def group_copy_preview_import(args: Dict[str, Any]) -> DaemonResponse:
    try:
        manifest, _entries, group_doc = _decode_and_read_package(args)
        preview = _build_preview(manifest, group_doc)
    except Exception as exc:
        return _error("invalid_group_copy", str(exc))
    return DaemonResponse(ok=True, result={"preview": preview})


def group_copy_import(args: Dict[str, Any]) -> DaemonResponse:
    workspace_root = str(args.get("workspace_root") or "").strip()
    title_override = str(args.get("title") or "").strip()
    try:
        manifest, entries, group_doc = _decode_and_read_package(args)
    except Exception as exc:
        return _error("invalid_group_copy", str(exc))

    source_group_id = str(group_doc.get("group_id") or manifest.get("source_group_id") or "").strip()
    if not source_group_id:
        return _error("invalid_group_copy", "group.yaml missing group_id")
    home = ensure_home()
    groups_dir = home / "groups"
    groups_dir.mkdir(parents=True, exist_ok=True)
    conflict = load_group(source_group_id) is not None or (groups_dir / source_group_id).exists()
    final_group_id = source_group_id if not conflict else _new_group_id()
    while (groups_dir / final_group_id).exists():
        final_group_id = _new_group_id()

    tmp_root = home / "tmp"
    tmp_root.mkdir(parents=True, exist_ok=True)
    final_group_dir = groups_dir / final_group_id
    staging_parent = tempfile.mkdtemp(prefix="group-copy-import-", dir=str(tmp_root))
    staging_group_dir = Path(staging_parent) / "group"
    created = False
    try:
        _write_package_entries_to_staging(entries, staging_group_dir)
        _cleanup_imported_runtime_state(staging_group_dir)

        doc_path = staging_group_dir / "group.yaml"
        doc = _load_yaml_bytes(doc_path.read_bytes())
        old_group_id = str(doc.get("group_id") or source_group_id).strip()
        doc["group_id"] = final_group_id
        if title_override:
            doc["title"] = title_override
        doc["running"] = False
        doc["state"] = "idle"
        _sanitize_import_actor_profiles(doc)

        effective_workspace_root = workspace_root or _primary_workspace_root(doc)
        if _is_exact_cccc_home_path(effective_workspace_root):
            raise ValueError("workspace_root must be a project directory, not CCCC_HOME")

        old_active_scope_key, new_active_scope_key = _apply_workspace_remap(
            doc,
            workspace_root=effective_workspace_root,
            final_group_id=final_group_id,
            force_new_scope_identity=bool(conflict),
        )
        new_scope_entry: Dict[str, Any] = {}
        for item in doc.get("scopes") if isinstance(doc.get("scopes"), list) else []:
            if isinstance(item, dict) and str(item.get("scope_key") or "") == new_active_scope_key:
                new_scope_entry = item
                break
        _rewrite_scope_dirs(staging_group_dir, old_active_scope_key, new_active_scope_key, new_scope_entry)

        atomic_write_text(doc_path, yaml.safe_dump(doc, allow_unicode=True, sort_keys=False), encoding="utf-8")
        _rewrite_ledger_identity(
            staging_group_dir / "ledger.jsonl",
            old_group_id=old_group_id,
            new_group_id=final_group_id,
            old_scope_key=old_active_scope_key,
            new_scope_key=new_active_scope_key,
        )

        if final_group_dir.exists():
            raise ValueError(f"target group already exists: {final_group_id}")
        shutil.move(str(staging_group_dir), str(final_group_dir))
        created = True

        reg = load_registry()
        now = utc_now_iso()
        reg.groups[final_group_id] = {
            "group_id": final_group_id,
            "title": str(doc.get("title") or final_group_id),
            "topic": str(doc.get("topic") or ""),
            "path": str(final_group_dir),
            "default_scope_key": new_active_scope_key,
            "created_at": str(doc.get("created_at") or now),
            "updated_at": now,
        }
        default_owner = str(reg.defaults.get(new_active_scope_key) or "").strip()
        if new_active_scope_key and not default_owner:
            reg.defaults[new_active_scope_key] = final_group_id
        reg.save()

        try:
            from ...kernel.events import publish_event

            publish_event("group.imported", {"group_id": final_group_id, "source_group_id": source_group_id, "conflict": conflict})
        except Exception:
            pass
        return DaemonResponse(
            ok=True,
            result={
                "group_id": final_group_id,
                "source_group_id": source_group_id,
                "group_id_conflict": conflict,
                "workspace_root": _primary_workspace_root(doc),
                "active_scope_key": new_active_scope_key,
            },
        )
    except Exception as exc:
        if created:
            shutil.rmtree(final_group_dir, ignore_errors=True)
        return _error("group_copy_import_failed", str(exc))
    finally:
        shutil.rmtree(staging_parent, ignore_errors=True)


def try_handle_group_copy_op(op: str, args: Dict[str, Any]) -> Optional[DaemonResponse]:
    if op == "group_copy_export":
        return group_copy_export(args)
    if op == "group_copy_preview_import":
        return group_copy_preview_import(args)
    if op == "group_copy_import":
        return group_copy_import(args)
    return None
