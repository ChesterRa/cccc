"""Current-group capability removal markers."""

from __future__ import annotations

from typing import Any, Dict, List


def _collect_removed_capabilities(state_doc: Dict[str, Any], *, group_id: str) -> List[str]:
    gid = str(group_id or "").strip()
    group_removed = state_doc.get("group_removed") if isinstance(state_doc.get("group_removed"), dict) else {}
    items = group_removed.get(gid) if isinstance(group_removed.get(gid), list) else []
    seen: set[str] = set()
    out: List[str] = []
    for item in items:
        cap_id = str(item or "").strip()
        if not cap_id or cap_id in seen:
            continue
        seen.add(cap_id)
        out.append(cap_id)
    return out


def _set_removed_capability(
    state_doc: Dict[str, Any],
    *,
    group_id: str,
    capability_id: str,
    removed: bool,
) -> bool:
    gid = str(group_id or "").strip()
    cap_id = str(capability_id or "").strip()
    if not gid or not cap_id:
        return False
    group_removed = state_doc.setdefault("group_removed", {})
    items = set(group_removed.get(gid) or [])
    before = set(items)
    if removed:
        items.add(cap_id)
    else:
        items.discard(cap_id)
    if items:
        group_removed[gid] = sorted(items)
    else:
        group_removed.pop(gid, None)
    if not group_removed:
        state_doc.pop("group_removed", None)
    return before != items
