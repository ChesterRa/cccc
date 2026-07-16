from __future__ import annotations

import time
import uuid
from pathlib import Path
from typing import Any

from fastapi import HTTPException

from ....paths import ensure_home

WEB_MAX_GROUP_COPY_PACKAGE_BYTES = 1024 * 1024 * 1024
WEB_GROUP_COPY_UPLOAD_CHUNK_BYTES = 4 * 1024 * 1024
WEB_GROUP_COPY_UPLOAD_TTL_SECONDS = 6 * 60 * 60


def group_copy_too_large_error(actual_bytes: int) -> HTTPException:
    max_bytes = int(WEB_MAX_GROUP_COPY_PACKAGE_BYTES)
    return HTTPException(
        status_code=413,
        detail={
            "code": "copy_package_too_large",
            "message": f"group copy too large: max {max_bytes} bytes, selected file is {int(actual_bytes)} bytes",
            "details": {"max_bytes": max_bytes, "actual_bytes": int(actual_bytes)},
        },
    )


def group_copy_upload_stage_dir() -> Path:
    path = ensure_home() / "tmp" / "group-copy-uploads"
    path.mkdir(parents=True, exist_ok=True)
    return path


def cleanup_group_copy_uploads() -> None:
    cutoff = time.time() - float(WEB_GROUP_COPY_UPLOAD_TTL_SECONDS)
    stage_dir = group_copy_upload_stage_dir()
    for path in stage_dir.glob("*.zip"):
        try:
            if path.stat().st_mtime < cutoff:
                path.unlink()
        except FileNotFoundError:
            continue
        except Exception:
            pass


def cleanup_group_copy_uploads_once() -> None:
    cleanup_group_copy_uploads()


def group_copy_upload_path(upload_id: str) -> Path:
    raw = str(upload_id or "").strip()
    if not raw or any(ch not in "0123456789abcdef" for ch in raw) or len(raw) != 32:
        raise HTTPException(status_code=404, detail={"code": "copy_upload_not_found", "message": "group copy upload not found"})
    path = group_copy_upload_stage_dir() / f"{raw}.zip"
    if not path.is_file():
        raise HTTPException(status_code=404, detail={"code": "copy_upload_not_found", "message": "group copy upload not found"})
    return path


async def spool_group_copy_upload(file: Any) -> tuple[str, Path, int]:
    cleanup_group_copy_uploads()
    upload_id = uuid.uuid4().hex
    path = group_copy_upload_stage_dir() / f"{upload_id}.zip"
    total = 0
    try:
        with path.open("wb") as out:
            while True:
                chunk = await file.read(WEB_GROUP_COPY_UPLOAD_CHUNK_BYTES)
                if not chunk:
                    break
                total += len(chunk)
                if total > WEB_MAX_GROUP_COPY_PACKAGE_BYTES:
                    raise group_copy_too_large_error(total)
                out.write(chunk)
        return upload_id, path, total
    except Exception:
        try:
            path.unlink()
        except FileNotFoundError:
            pass
        raise
