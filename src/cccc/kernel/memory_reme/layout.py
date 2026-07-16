from __future__ import annotations

import re
from dataclasses import dataclass
from pathlib import Path
from typing import Optional

from ...util.fs import atomic_write_text
from ...util.time import utc_now_iso
from ..group import load_group


@dataclass(frozen=True)
class MemoryLayout:
    group_id: str
    group_label: str
    memory_root: Path
    memory_file: Path
    daily_dir: Path
    today_daily_file: Path


def _sanitize_group_label(raw: str) -> str:
    text = str(raw or "").strip()
    if not text:
        return "group"
    # Keep unicode letters, numbers, "_" and "-" in filenames; collapse others.
    text = re.sub(r"[\\/:\0]+", "_", text)
    text = re.sub(r"\s+", "_", text)
    text = re.sub(r"[^\w.\-]+", "_", text, flags=re.UNICODE)
    text = re.sub(r"_+", "_", text).strip("._-")
    return text or "group"


def _resolve_group_label(group_id: str) -> str:
    group = load_group(group_id)
    if group is None:
        raise ValueError(f"group not found: {group_id}")
    raw = (
        str(group.doc.get("name") or "").strip()
        or str(group.doc.get("title") or "").strip()
        or str(group.group_id or "").strip()
    )
    return _sanitize_group_label(raw)


def _ensure_file(path: Path, content: str) -> None:
    if path.exists():
        return
    path.parent.mkdir(parents=True, exist_ok=True)
    atomic_write_text(path, content, encoding="utf-8")


def _memory_file_header(group_label: str) -> str:
    now = utc_now_iso()
    return (
        "---\n"
        f"group_label: {group_label}\n"
        "kind: memory\n"
        f"created_at: {now}\n"
        "---\n\n"
        f"# MEMORY ({group_label})\n\n"
    )


def _daily_file_header(group_label: str, date: str) -> str:
    now = utc_now_iso()
    return (
        "---\n"
        f"group_label: {group_label}\n"
        "kind: daily\n"
        f"date: {date}\n"
        f"created_at: {now}\n"
        "---\n\n"
        f"# Daily Memory ({group_label}) - {date}\n\n"
    )


def resolve_memory_layout(
    group_id: str,
    *,
    date: Optional[str] = None,
    ensure_files: bool = True,
) -> MemoryLayout:
    group = load_group(group_id)
    if group is None:
        raise ValueError(f"group not found: {group_id}")

    date_str = str(date or "")[:10] if date else utc_now_iso()[:10]
    group_label = _resolve_group_label(group_id)

    memory_root = group.path / "state" / "memory"
    memory_file = memory_root / "MEMORY.md"
    daily_dir = memory_root / "daily"
    today_daily_file = daily_dir / f"{date_str}__{group_label}.md"

    if ensure_files:
        memory_root.mkdir(parents=True, exist_ok=True)
        daily_dir.mkdir(parents=True, exist_ok=True)
        _ensure_file(memory_file, _memory_file_header(group_label))
        _ensure_file(today_daily_file, _daily_file_header(group_label, date_str))

    return MemoryLayout(
        group_id=group_id,
        group_label=group_label,
        memory_root=memory_root,
        memory_file=memory_file,
        daily_dir=daily_dir,
        today_daily_file=today_daily_file,
    )

