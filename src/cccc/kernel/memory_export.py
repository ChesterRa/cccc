"""Memory export: read-only Markdown + manifest generation.

Produces a memory.md file grouped by kind with metadata,
and a manifest.json with SHA-256 hash for integrity verification.
"""

from __future__ import annotations

import hashlib
from typing import Any, Dict, List

from .memory import MEMORY_KINDS, MemoryStore
from ..util.time import utc_now_iso


def export_markdown(
    store: MemoryStore,
    *,
    include_draft: bool = False,
) -> str:
    """Export memories as Markdown, grouped by kind.

    By default only exports solid memories.
    Returns the Markdown string.
    """
    # Collect memories grouped by kind
    by_kind: Dict[str, List[Dict[str, Any]]] = {}
    total = 0

    for kind in MEMORY_KINDS:
        if include_draft:
            mems = store.list_memories(kind=kind, limit=10000)
        else:
            mems = store.list_memories(kind=kind, status="solid", limit=10000)
        if mems:
            by_kind[kind] = mems
            total += len(mems)

    # Build Markdown
    lines: List[str] = []
    lines.append("# Memory Export")
    lines.append("")
    lines.append(f"Group: {store.group_id}")
    lines.append(f"Total: {total}")
    lines.append(f"Status filter: {'all' if include_draft else 'solid only'}")
    lines.append("")

    for kind in MEMORY_KINDS:
        mems = by_kind.get(kind)
        if not mems:
            continue
        lines.append(f"## {kind}")
        lines.append("")
        for mem in mems:
            lines.append(f"### [{mem['id'][:8]}] {mem['content'][:80]}")
            lines.append("")
            lines.append(f"- **Status**: {mem.get('status', '')}")
            lines.append(f"- **Confidence**: {mem.get('confidence', '')}")
            if mem.get("actor_id"):
                lines.append(f"- **Actor**: {mem['actor_id']}")
            if mem.get("task_id"):
                lines.append(f"- **Task**: {mem['task_id']}")
            if mem.get("tags"):
                lines.append(f"- **Tags**: {', '.join(mem['tags'])}")
            lines.append(f"- **Created**: {mem.get('created_at', '')}")
            lines.append("")
            # Full content if longer than 80 chars
            if len(mem["content"]) > 80:
                lines.append(mem["content"])
                lines.append("")

    return "\n".join(lines)


def export_manifest(
    markdown_content: str,
    *,
    group_id: str,
    memory_count: int = 0,
) -> Dict[str, Any]:
    """Generate a manifest dict for the exported Markdown.

    Includes SHA-256 hash for integrity verification.
    """
    sha256 = hashlib.sha256(markdown_content.encode("utf-8")).hexdigest()
    return {
        "group_id": group_id,
        "sha256": sha256,
        "memory_count": memory_count,
        "exported_at": utc_now_iso(),
        "format": "markdown",
    }
