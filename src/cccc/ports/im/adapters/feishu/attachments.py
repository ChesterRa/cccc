from __future__ import annotations

from dataclasses import dataclass
from typing import Any, Dict


@dataclass(frozen=True)
class FeishuDownloadAttachment:
    kind: str
    key: str


def parse_download_attachment(attachment: Dict[str, Any]) -> FeishuDownloadAttachment:
    """Validate an inbound attachment and return the Feishu download key."""
    kind = str(attachment.get("kind") or "")

    if kind == "image":
        image_key = str(attachment.get("image_key") or "")
        if not image_key:
            raise ValueError("Missing image_key")
        return FeishuDownloadAttachment("image", image_key)

    if kind == "file":
        file_key = str(attachment.get("file_key") or "")
        if not file_key:
            raise ValueError("Missing file_key")
        return FeishuDownloadAttachment("file", file_key)

    raise ValueError(f"Unknown attachment kind: {kind}")
