from __future__ import annotations

import hashlib
import json
import zipfile
from dataclasses import dataclass
from pathlib import Path, PurePosixPath
from typing import Any, Callable, Dict, Iterable, List, Optional, Tuple


@dataclass(frozen=True)
class GroupCopyFileDeps:
    load_group: Callable[[str], Any]
    should_exclude_group_relpath: Callable[..., bool]
    load_yaml_bytes: Callable[[bytes], Dict[str, Any]]
    dump_yaml: Callable[[Dict[str, Any]], bytes]
    scrub_group_doc_for_copy: Callable[[Dict[str, Any]], Dict[str, Any]]
    validate_zip_file: Callable[[zipfile.ZipFile], List[zipfile.ZipInfo]]
    normalize_zip_name: Callable[[str], str]
    validate_manifest: Callable[[Dict[str, Any]], None]
    content_digest_from_hashes: Callable[[Iterable[Tuple[str, str]]], str]
    manifest_for_group_source_hashes: Callable[..., Dict[str, Any]]
    copy_package_filename: Callable[[str, str], str]


def safe_package_path(value: Any) -> Path:
    raw = str(value or "").strip()
    if not raw:
        raise ValueError("missing package_path")
    path = Path(raw).expanduser().resolve()
    if not path.is_file():
        raise ValueError(f"copy package file not found: {path}")
    return path


def build_package_file(group_id: str, output_path: Path, deps: GroupCopyFileDeps) -> Tuple[Dict[str, Any], str]:
    group = deps.load_group(group_id)
    if group is None:
        raise ValueError(f"group not found: {group_id}")
    sources = list(_iter_export_group_sources(group.path, deps))
    if not any(rel == "group.yaml" for rel, _path, _data in sources):
        raise ValueError("group.yaml missing")
    title = str(group.doc.get("title") or group_id)
    output_path.parent.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(output_path, "w", compression=zipfile.ZIP_DEFLATED) as zf:
        content_hashes: List[Tuple[str, str]] = []
        for rel, path, data in sources:
            digest = _write_source_to_zip(zf, f"group/{rel}", path=path, data=data)
            content_hashes.append((rel, digest))
        manifest = deps.manifest_for_group_source_hashes(
            group_id=group_id,
            title=title,
            rels={rel for rel, _path, _data in sources},
            content_hashes=content_hashes,
        )
        zf.writestr("manifest.json", json.dumps(manifest, ensure_ascii=False, sort_keys=True, indent=2))
    return manifest, deps.copy_package_filename(group_id, title)


def scan_package_path(path: Path, deps: GroupCopyFileDeps, *, staging_group_dir: Optional[Path] = None) -> Tuple[Dict[str, Any], Dict[str, Any]]:
    zf, infos = _open_package_path(path, deps)
    try:
        manifest: Dict[str, Any] = {}
        group_yaml_bytes: Optional[bytes] = None
        content_hashes: List[Tuple[str, str]] = []
        for info in infos:
            name = deps.normalize_zip_name(info.filename)
            if info.is_dir():
                continue
            if name == "manifest.json":
                try:
                    raw_manifest = json.loads(zf.read(info).decode("utf-8"))
                except Exception as exc:
                    raise ValueError("invalid manifest.json") from exc
                if not isinstance(raw_manifest, dict):
                    raise ValueError("manifest.json must be an object")
                manifest = raw_manifest
                continue
            if not name.startswith("group/"):
                continue
            rel = name[len("group/") :]
            if not rel:
                continue
            if staging_group_dir is not None and not deps.should_exclude_group_relpath(rel):
                target = staging_group_dir / Path(*PurePosixPath(rel).parts)
                digest = _copy_zip_member_to_file(zf, info, target)
            else:
                digest = _hash_zip_member(zf, info)
            content_hashes.append((rel, digest))
            if rel == "group.yaml":
                group_yaml_bytes = zf.read(info)
        if not manifest:
            raise ValueError("manifest.json missing")
        deps.validate_manifest(manifest)
        if group_yaml_bytes is None:
            raise ValueError("group/group.yaml missing")
        expected_digest = str(manifest.get("content_digest") or "").strip()
        if expected_digest and expected_digest != deps.content_digest_from_hashes(content_hashes):
            raise ValueError("copy package content digest mismatch")
        group_doc = deps.load_yaml_bytes(group_yaml_bytes)
        return manifest, group_doc
    finally:
        zf.close()


def _iter_export_group_sources(group_path: Path, deps: GroupCopyFileDeps) -> Iterable[Tuple[str, Optional[Path], Optional[bytes]]]:
    for path in sorted(group_path.rglob("*")):
        try:
            rel_path = path.relative_to(group_path)
        except ValueError:
            continue
        rel = rel_path.as_posix()
        if not rel or deps.should_exclude_group_relpath(rel, is_dir=path.is_dir()):
            continue
        if path.is_symlink() or path.is_dir() or not path.is_file():
            continue
        if rel == "group.yaml":
            doc = deps.load_yaml_bytes(path.read_bytes())
            yield rel, None, deps.dump_yaml(deps.scrub_group_doc_for_copy(doc))
            continue
        yield rel, path, None


def _open_package_path(path: Path, deps: GroupCopyFileDeps) -> Tuple[zipfile.ZipFile, List[zipfile.ZipInfo]]:
    package_bytes = path.stat().st_size
    try:
        zf = zipfile.ZipFile(path, "r")
    except Exception as exc:
        raise ValueError("invalid zip package") from exc
    try:
        infos = deps.validate_zip_file(zf, package_bytes=package_bytes)
    except Exception:
        zf.close()
        raise
    return zf, infos


def _write_source_to_zip(
    zf: zipfile.ZipFile,
    arcname: str,
    *,
    path: Optional[Path],
    data: Optional[bytes],
) -> str:
    if data is not None:
        zf.writestr(arcname, data)
        return hashlib.sha256(data).hexdigest()
    if path is None:
        return hashlib.sha256(b"").hexdigest()

    h = hashlib.sha256()
    with path.open("rb") as src, zf.open(arcname, "w", force_zip64=True) as dst:
        for chunk in iter(lambda: src.read(1024 * 1024), b""):
            h.update(chunk)
            dst.write(chunk)
    return h.hexdigest()


def _copy_zip_member_to_file(zf: zipfile.ZipFile, info: zipfile.ZipInfo, target: Path) -> str:
    h = hashlib.sha256()
    target.parent.mkdir(parents=True, exist_ok=True)
    with zf.open(info, "r") as src, target.open("wb") as dst:
        for chunk in iter(lambda: src.read(1024 * 1024), b""):
            h.update(chunk)
            dst.write(chunk)
    return h.hexdigest()


def _hash_zip_member(zf: zipfile.ZipFile, info: zipfile.ZipInfo) -> str:
    h = hashlib.sha256()
    with zf.open(info, "r") as src:
        for chunk in iter(lambda: src.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()
