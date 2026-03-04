from __future__ import annotations

from typing import Dict


_GUIDE_VERSION = "1"

_MEMORY_GUIDES: Dict[str, str] = {
    "store": """## Memory Write Best Practices

1. Recall before write: `cccc_memory(action="search", query=...)` then `cccc_memory(action="get", path=...)`.
2. Write daily execution deltas to `target="daily"`; write stable reusable know-how to `target="memory"`.
3. Use concise, deduplicated statements; avoid dumping raw logs (`dedup_intent=new|update|supersede|silent`).
4. Prefer append mode with an idempotency key for retry-safe writes.
5. When content supersedes earlier memory, include `supersedes[]` and state the update explicitly.
""",
    "search": """## Memory Recall Best Practices

1. Start with clear query nouns (task ids, decisions, preferences, component names).
2. Use `cccc_memory(action="search")` for candidate snippets, then `cccc_memory(action="get")` for exact lines.
3. Run recall before planning/high-impact edits to avoid contradiction with prior decisions.
4. On start/resume, consume `cccc_bootstrap.memory_recall_gate` first, then expand with manual search/get if needed.
""",
    "consolidation": """## Memory Consolidation Workflow

1. Check context pressure: `cccc_memory_admin(action="context_check", messages=[...])`.
2. If compaction is needed, run `cccc_memory_admin(action="compact", ...)`.
3. Flush durable conversation deltas: `cccc_memory_admin(action="daily_flush", messages=[...])`.
4. Promote stable long-horizon knowledge with `cccc_memory(action="write", target="memory", ...)`.
5. Re-sync index after large edits: `cccc_memory_admin(action="index_sync", mode="scan"|"rebuild")`.
""",
    "lifecycle": """## Memory Lifecycle Overview

File-first lifecycle:
1. Conversation/task deltas are appended to daily files.
2. Stable reusable knowledge is promoted into `MEMORY.md`.
3. Recall always uses search/get over indexed files.

Guardrails:
- Context is short-term execution memory.
- Memory files are long-term reusable memory.
- No legacy sqlite path in runtime.
""",
}


def build_memory_guide(topic: str) -> Dict[str, str]:
    """Return a structured memory guide payload for a known topic."""
    key = str(topic or "").strip().lower()
    if key not in _MEMORY_GUIDES:
        allowed = ", ".join(sorted(_MEMORY_GUIDES.keys()))
        raise ValueError(f"invalid topic: {topic!r}. allowed topics: {allowed}")
    return {
        "topic": key,
        "markdown": _MEMORY_GUIDES[key],
        "source": "builtin",
        "version": _GUIDE_VERSION,
    }
