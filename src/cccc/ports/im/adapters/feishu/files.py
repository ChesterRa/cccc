from __future__ import annotations

import mimetypes
from dataclasses import dataclass
from pathlib import Path


@dataclass(frozen=True)
class FeishuUploadFile:
    filename: str
    mime_type: str
    is_image: bool


def prepare_upload_file(file_path: Path, filename: str) -> FeishuUploadFile:
    """Normalize filename and MIME metadata for Feishu uploads."""
    safe_filename = (filename or file_path.name or "file").replace("\\", "_").replace("/", "_")
    mime_type = mimetypes.guess_type(safe_filename)[0] or "application/octet-stream"
    return FeishuUploadFile(
        filename=safe_filename,
        mime_type=mime_type,
        is_image=mime_type.startswith("image/"),
    )
