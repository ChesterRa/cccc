from __future__ import annotations

from pathlib import Path
from typing import Any, Dict

from fastapi import HTTPException
from pydantic import BaseModel


class CreateDirectoryRequest(BaseModel):
    parent: str
    name: str


def create_directory(payload: CreateDirectoryRequest, *, read_only: bool) -> Dict[str, Any]:
    if read_only:
        raise HTTPException(
            status_code=403,
            detail={
                "code": "read_only",
                "message": "File system endpoints are disabled in read-only (exhibit) mode.",
                "details": {"endpoint": "fs_create_directory"},
            },
        )
    parent_input = payload.parent.strip()
    if not parent_input or not (
        Path(parent_input).is_absolute()
        or parent_input == "~"
        or parent_input.startswith(("~/", "~\\"))
    ):
        raise HTTPException(
            status_code=400,
            detail={
                "code": "INVALID_PARENT",
                "message": "Parent directory must be absolute or start with ~",
            },
        )
    name = payload.name.strip()
    if not name or name in {".", ".."} or any(char in name for char in "/\\\0"):
        raise HTTPException(
            status_code=400,
            detail={
                "code": "INVALID_NAME",
                "message": "Directory name must be a single non-empty path segment",
            },
        )
    parent = Path(parent_input).expanduser().resolve()
    if not parent.exists():
        raise HTTPException(
            status_code=404,
            detail={"code": "NOT_FOUND", "message": f"Path not found: {payload.parent}"},
        )
    if not parent.is_dir():
        raise HTTPException(
            status_code=400,
            detail={"code": "NOT_DIR", "message": f"Not a directory: {payload.parent}"},
        )
    target = parent / name
    try:
        target.mkdir()
    except FileExistsError as error:
        raise HTTPException(
            status_code=409,
            detail={"code": "ALREADY_EXISTS", "message": f"Path already exists: {target}"},
        ) from error
    except PermissionError as error:
        raise HTTPException(
            status_code=403,
            detail={"code": "PERMISSION", "message": f"Permission denied: {target}"},
        ) from error
    except FileNotFoundError as error:
        raise HTTPException(
            status_code=404,
            detail={"code": "NOT_FOUND", "message": f"Path not found: {parent}"},
        ) from error
    except NotADirectoryError as error:
        raise HTTPException(
            status_code=400,
            detail={"code": "NOT_DIR", "message": f"Not a directory: {parent}"},
        ) from error
    except OSError as error:
        raise HTTPException(
            status_code=400,
            detail={"code": "filesystem_error", "message": str(error)},
        ) from error
    return {"ok": True, "result": {"path": str(target)}}
