from __future__ import annotations

from typing import Dict


_GUIDE_VERSION = "1"

_MEMORY_GUIDES: Dict[str, str] = {
    "store": """## Memory Store Best Practices

Before storing a new memory:
1. Always run a quick consolidation pass first: `cccc_memory_ingest(mode="signal")` + `cccc_memory_search(...)`.
2. Search for related memories before writing: `cccc_memory_search(query="<your topic>", tags=["<relevant-tag>"])`.
3. If a related memory exists and content has evolved -> update it: `cccc_memory_store(id="<existing_id>", content="<updated>")`.
4. If a related memory exists but is outdated -> delete old + store new: `cccc_memory_delete(id="<old>")` then `cccc_memory_store(...)`.
5. Only create a new memory if no related entry exists.

This prevents memory fragmentation and contradictions.
""",
    "search": """## Memory Search Best Practices

Use targeted recall before broad recall:
1. Start with clear keywords in `query` (nouns, identifiers, milestone/task IDs).
2. Add structured filters (`status`, `kind`, `tags`, `since/until`) to reduce noise.
3. Keep `track_hit=false` for normal lookup and exploration.
4. Use `track_hit=true` only on explicit reinforcement paths (confirmed reuse/accepted decision/verified fact), because it increments hit_count and can auto-solidify drafts at threshold.
5. Results are ordered solid-first, then recency. Prefer solid memories for stable decisions; use draft memories as candidates to verify.
6. Default depth is L0 (returns summary only). Use `depth='L2'` when you need full content, or fetch a single memory by id via `get(id)`.
""",
    "consolidation": """## Memory Consolidation Workflow

Use this flow to evolve memory quality:
1. Ingest fresh signals: `cccc_memory_ingest(mode="signal")`
2. Find patterns and duplicates: `cccc_memory_search(...)`
3. Merge fragmented insights into one higher-level memory: `cccc_memory_store(...)` (update existing when possible)
4. Identify stale candidates safely: `cccc_memory_decay(...)`
5. Remove obvious low-value fragments intentionally: `cccc_memory_delete(...)`
6. Report what changed in 1-2 lines (what was consolidated/cleaned).
""",
    "lifecycle": """## Memory Lifecycle Overview

Memory lifecycle in this system:
1. Draft stage: new memories usually start as `status=draft` with confidence levels.
2. Promotion stage: drafts become solid via explicit solidify flows or auto-solidify on reinforced recall paths (hit_count threshold).
3. Maintenance stage: `cccc_memory_decay` proposes stale candidates (non-destructive).
4. Cleanup stage: deletion is explicit via `cccc_memory_delete`; no hidden auto-delete.

Trust model:
- Stage A: draft + confidence reflects provisional understanding.
- Stage B: solid memories represent higher-trust, reusable knowledge.
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
