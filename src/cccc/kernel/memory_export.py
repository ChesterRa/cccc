"""File-memory export helpers."""

from __future__ import annotations

import hashlib
from pathlib import Path
from typing import Any, Dict

from ..util.time import utc_now_iso
from .memory_reme.layout import resolve_memory_layout


def export_file_memory_markdown(group_id: str) -> str:
    layout = resolve_memory_layout(group_id, ensure_files=True)
    memory_text = layout.memory_file.read_text(encoding="utf-8", errors="replace")
    daily_parts = []
    for p in sorted(layout.daily_dir.glob("*.md")):
        if not p.is_file():
            continue
        daily_parts.append(f"\n\n---\n\n# {p.name}\n\n{p.read_text(encoding='utf-8', errors='replace')}")
    return memory_text + "".join(daily_parts)


def export_file_memory_manifest(markdown_content: str, *, group_id: str) -> Dict[str, Any]:
    sha256 = hashlib.sha256(markdown_content.encode("utf-8")).hexdigest()
    return {
        "group_id": group_id,
        "sha256": sha256,
        "exported_at": utc_now_iso(),
        "format": "file_markdown_bundle",
    }
