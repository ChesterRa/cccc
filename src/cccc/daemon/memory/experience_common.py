from __future__ import annotations

import re
from typing import Any, Dict, List

_TOKEN_RE = re.compile(r"[\w\u4e00-\u9fff]+", re.UNICODE)
_PROMOTED_STATUSES = {"promoted", "promoted_to_memory"}
_RETIRED_STATUSES = {"rejected", "merged", "superseded"}


def _string_list(raw: Any) -> List[str]:
    out: List[str] = []
    if not isinstance(raw, list):
        return out
    for item in raw:
        value = str(item or "").strip()
        if value and value not in out:
            out.append(value)
    return out


def _append_unique(items: List[str], *values: str) -> List[str]:
    out = list(items)
    for value in values:
        text = str(value or "").strip()
        if text and text not in out:
            out.append(text)
    return out


def _candidate_governance(item: Dict[str, Any]) -> Dict[str, Any]:
    governance = item.get("governance")
    return dict(governance) if isinstance(governance, dict) else {}


def _candidate_review(item: Dict[str, Any]) -> Dict[str, Any]:
    review = item.get("review")
    return dict(review) if isinstance(review, dict) else {}


def _add_history_event(governance: Dict[str, Any], event: Dict[str, Any]) -> Dict[str, Any]:
    history = governance.get("history")
    normalized_history = list(history) if isinstance(history, list) else []
    normalized_history.append(dict(event))
    governance["history"] = normalized_history[-20:]
    return governance


def _candidate_ref(candidate_id: str) -> str:
    return f"experience:{candidate_id}"


def _lineage_source_refs(item: Dict[str, Any]) -> List[str]:
    governance = _candidate_governance(item)
    refs = _string_list(item.get("source_refs"))
    refs.extend(_string_list(governance.get("lineage_source_refs")))
    out: List[str] = []
    for ref in refs:
        if ref not in out:
            out.append(ref)
    return out


def _governance_conflict(item: Dict[str, Any], *, target_field: str, target_id: str) -> str | None:
    governance = _candidate_governance(item)
    existing = str(governance.get(target_field) or "").strip()
    if existing and existing != target_id:
        return existing
    return None


def _governance_lineage_ids(item: Dict[str, Any]) -> List[str]:
    governance = _candidate_governance(item)
    return _string_list(governance.get("lineage_candidate_ids"))


def _candidate_is_promoted(candidate: Dict[str, Any]) -> bool:
    return str(candidate.get("status") or "").strip() == "promoted_to_memory"


def _tokenize(text: Any) -> List[str]:
    raw = str(text or "").strip().lower()
    return [token for token in _TOKEN_RE.findall(raw) if len(token) >= 2]


def _trim_text(value: Any, *, max_chars: int) -> str:
    text = str(value or "").strip()
    if len(text) <= max_chars:
        return text
    if max_chars <= 3:
        return text[:max_chars]
    return text[: max_chars - 3].rstrip() + "..."
