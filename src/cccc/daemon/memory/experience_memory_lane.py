from __future__ import annotations

import hashlib
import json
import re
from pathlib import Path
from typing import Any, Dict, List, Optional

from ...kernel.experience import load_experience_candidates
from ...kernel.group import load_group
from ...kernel.memory_reme import build_memory_entry, index_sync, write_raw_content
from ...kernel.memory_reme.layout import resolve_memory_layout
from ...util.fs import atomic_write_json
from ...util.time import utc_now_iso
from .experience_common import (
    _PROMOTED_STATUSES,
    _RETIRED_STATUSES,
    _append_unique,
    _candidate_governance,
    _candidate_ref,
    _string_list,
)

_MEMORY_ENTRY_BLOCK_RE = re.compile(
    r"(?ms)^## (?P<entry_id>\S+) \[(?P<kind>[^\]]+)\] [^\n]+\n"
    r"<!-- cccc\.memory\.meta (?P<meta>\{.*?\}) -->\n\n"
    r"(?P<body>.*?)(?=^## \S+ \[[^\]]+\] [^\n]+\n<!-- cccc\.memory\.meta |\Z)"
)


def _review_failure_signals(candidate: Dict[str, Any]) -> List[str]:
    review = candidate.get("review") if isinstance(candidate.get("review"), dict) else {}
    out: List[str] = []
    for key in ("failure_signals", "fail_signals", "risks", "watchouts"):
        raw = review.get(key)
        if isinstance(raw, list):
            for item in raw:
                text = str(item or "").strip()
                if text and text not in out:
                    out.append(text)
    for key in ("rejected_reason", "reason", "note"):
        text = str(review.get(key) or "").strip()
        if text and text not in out:
            out.append(text)
    if out:
        return out[:5]
    return []


def _sha256(text: str) -> str:
    return hashlib.sha256(text.encode("utf-8")).hexdigest()


def _render_memory_summary(candidate: Dict[str, Any]) -> str:
    candidate_id = str(candidate.get("id") or "").strip()
    summary = str(candidate.get("summary") or "").strip()
    source_kind = str(candidate.get("source_kind") or "").strip()
    task_id = str(candidate.get("task_id") or "").strip()
    status = str(candidate.get("status") or "").strip()
    detail = candidate.get("detail") if isinstance(candidate.get("detail"), dict) else {}
    source_refs = [str(x).strip() for x in (candidate.get("source_refs") or []) if str(x).strip()]
    title = str(candidate.get("title") or "").strip() or str(detail.get("title") or "").strip() or candidate_id
    outcome = str(detail.get("outcome") or "").strip()
    failure_signals = _review_failure_signals(candidate)
    governance = _candidate_governance(candidate)
    lines: List[str] = ["experience_record"]
    if candidate_id:
        lines.append(f"- candidate_id: {candidate_id}")
    if title:
        lines.append(f"- title: {title}")
    if summary:
        lines.append(f"- summary: {summary}")
    if status:
        lines.append(f"- status: {status}")
    if source_kind:
        lines.append(f"- source_kind: {source_kind}")
    if task_id:
        lines.append(f"- task_id: {task_id}")
    if outcome:
        lines.append(f"- outcome: {outcome}")
    if source_refs:
        lines.append(f"- source_refs: {', '.join(source_refs)}")
    merged_from = _string_list(governance.get("merged_from"))
    if merged_from:
        lines.append(f"- merged_from: {', '.join(merged_from)}")
    supersedes = _string_list(governance.get("supersedes"))
    if supersedes:
        lines.append(f"- supersedes: {', '.join(supersedes)}")
    if failure_signals:
        lines.append(f"- failure_signals: {'; '.join(failure_signals)}")
    return "\n".join(lines) + "\n"


def persist_candidates(path: Path, candidates: List[Dict[str, Any]]) -> None:
    payload = {
        "schema": 1,
        "updated_at": utc_now_iso(),
        "candidates": candidates[:500],
    }
    atomic_write_json(path, payload, indent=2)


def render_structured_memory_entry(entry: Dict[str, Any], *, idempotency_key: str = "") -> str:
    summary = str(entry.get("summary") or "").strip()
    content_hash = _sha256(summary)
    meta = {
        "entry_id": str(entry.get("entry_id") or ""),
        "candidate_id": str(entry.get("candidate_id") or ""),
        "kind": str(entry.get("kind") or ""),
        "lifecycle_state": str(entry.get("lifecycle_state") or ""),
        "date": str(entry.get("date") or ""),
        "group_label": str(entry.get("group_label") or ""),
        "actor_id": str(entry.get("actor_id") or ""),
        "created_at": str(entry.get("created_at") or ""),
        "source_refs": list(entry.get("source_refs") or []),
        "tags": list(entry.get("tags") or []),
        "supersedes": list(entry.get("supersedes") or []),
        "content_hash": content_hash,
    }
    if idempotency_key:
        meta["idempotency_key"] = idempotency_key
    meta_json = json.dumps(meta, ensure_ascii=False)
    return (
        f"## {meta.get('entry_id')} [{meta.get('kind')}] {meta.get('created_at')}\n"
        f"<!-- cccc.memory.meta {meta_json} -->\n\n"
        f"{summary}\n\n"
    )


def experience_memory_entry_id(candidate_id: str) -> str:
    return f"expmem_{str(candidate_id or '').strip()}"


def build_experience_memory_entry(
    *,
    group_id: str,
    candidate: Dict[str, Any],
    actor_id: str,
    entry_id: Optional[str] = None,
    created_at: Optional[str] = None,
) -> Dict[str, Any]:
    layout = resolve_memory_layout(group_id, ensure_files=False)
    candidate_id = str(candidate.get("id") or "").strip()
    source_refs = _append_unique(_string_list(candidate.get("source_refs")), _candidate_ref(candidate_id))
    timestamp = str(created_at or candidate.get("updated_at") or utc_now_iso())
    status = str(candidate.get("status") or "").strip()
    lifecycle_state = "active" if status in _PROMOTED_STATUSES else "retired"
    kind = "experience" if lifecycle_state == "active" else "experience_retired"
    summary = _render_memory_summary(candidate) if lifecycle_state == "active" else _render_retired_memory_summary(candidate)
    entry = build_memory_entry(
        group_label=layout.group_label,
        kind=kind,
        summary=summary,
        actor_id=actor_id,
        source_refs=source_refs,
        tags=["experience", "promoted" if lifecycle_state == "active" else "retired"],
        entry_id=entry_id or experience_memory_entry_id(candidate_id),
        created_at=timestamp,
        date=timestamp[:10],
    )
    entry["candidate_id"] = candidate_id
    entry["lifecycle_state"] = lifecycle_state
    return entry


def _render_retired_memory_summary(candidate: Dict[str, Any]) -> str:
    candidate_id = str(candidate.get("id") or "").strip() or "n/a"
    status = str(candidate.get("status") or "").strip() or "retired"
    governance = _candidate_governance(candidate)
    retired = governance.get(status) if isinstance(governance.get(status), dict) else {}
    reason = str(retired.get("reason") or "").strip()
    merged_into = str(governance.get("merged_into") or "").strip()
    superseded_by = str(governance.get("superseded_by") or "").strip()
    lines = [
        f"Experience retired: {str(candidate.get('summary') or '').strip() or candidate_id}",
        f"- candidate_id: {candidate_id}",
        f"- lifecycle_state: retired",
        f"- retired_status: {status}",
    ]
    if merged_into:
        lines.append(f"- merged_into: {merged_into}")
    if superseded_by:
        lines.append(f"- superseded_by: {superseded_by}")
    if reason:
        lines.append(f"- reason: {reason}")
    return "\n".join(lines) + "\n"


def memory_locator_from_candidate(candidate: Dict[str, Any]) -> Optional[Dict[str, Any]]:
    promotion = candidate.get("promotion") if isinstance(candidate.get("promotion"), dict) else {}
    memory_entry = promotion.get("memory_entry") if isinstance(promotion.get("memory_entry"), dict) else {}
    entry_id = str(memory_entry.get("entry_id") or "").strip()
    file_path = str(memory_entry.get("file_path") or "").strip()
    if not entry_id:
        return None
    return {"entry_id": entry_id, "file_path": file_path}


def memory_file_path(group_id: str) -> str:
    return str(resolve_memory_layout(group_id, ensure_files=False).memory_file)


def legacy_memory_block_range(*, text: str, candidate: Dict[str, Any]) -> List[Dict[str, Any]]:
    legacy_body = _render_memory_summary(candidate)
    if not legacy_body.strip():
        return []
    structured_ranges = [(m.start(), m.end()) for m in _MEMORY_ENTRY_BLOCK_RE.finditer(text or "")]
    matches: List[Dict[str, Any]] = []
    start = 0
    while True:
        idx = text.find(legacy_body, start)
        if idx < 0:
            break
        end = idx + len(legacy_body)
        if any(block_start <= idx < block_end for block_start, block_end in structured_ranges):
            start = idx + 1
            continue
        start_line = text[:idx].count("\n") + 1
        end_line = max(start_line, text[:end].count("\n"))
        matches.append(
            {
                "start": idx,
                "end": end,
                "start_line": start_line,
                "end_line": end_line,
                "raw": legacy_body,
            }
        )
        start = idx + 1
    return matches


def is_structured_experience_block(block: Optional[Dict[str, Any]], *, candidate_id: str, entry_id: str) -> bool:
    if not isinstance(block, dict):
        return False
    meta = block.get("meta") if isinstance(block.get("meta"), dict) else {}
    return (
        str(block.get("entry_id") or "").strip() == entry_id
        and str(meta.get("candidate_id") or "").strip() == candidate_id
        and str(meta.get("entry_id") or "").strip() == entry_id
        and str(meta.get("kind") or block.get("kind") or "").strip() == "experience"
        and str(meta.get("lifecycle_state") or "").strip() == "active"
    )


def require_memory_locator(candidate: Dict[str, Any], *, candidate_id: str) -> Dict[str, Any]:
    locator = memory_locator_from_candidate(candidate)
    if locator is None:
        raise ValueError(
            f"candidate {candidate_id} is promoted_to_memory but has no addressable memory entry metadata"
        )
    return locator


def find_memory_entry_block(text: str, *, entry_id: str) -> Optional[Dict[str, Any]]:
    for match in _MEMORY_ENTRY_BLOCK_RE.finditer(text or ""):
        if str(match.group("entry_id") or "").strip() != entry_id:
            continue
        meta_raw = str(match.group("meta") or "").strip()
        try:
            meta = json.loads(meta_raw) if meta_raw else {}
        except Exception:
            meta = {}
        start_line = text[: match.start()].count("\n") + 1
        end_line = max(start_line, text[: match.end()].count("\n"))
        return {
            "entry_id": entry_id,
            "start": match.start(),
            "end": match.end(),
            "raw": str(match.group(0) or ""),
            "kind": str(match.group("kind") or "").strip(),
            "meta": meta if isinstance(meta, dict) else {},
            "body": str(match.group("body") or ""),
            "start_line": start_line,
            "end_line": end_line,
        }
    return None


def normalize_memory_text(text: str) -> str:
    collapsed = re.sub(r"\n{3,}", "\n\n", str(text or ""))
    if not collapsed.strip():
        return ""
    return collapsed.rstrip() + "\n\n"


def compute_memory_mutation_plan(*, group_id: str, mutations: List[Dict[str, Any]]) -> Dict[str, Any]:
    layout = resolve_memory_layout(group_id, ensure_files=True)
    memory_file = layout.memory_file
    current = memory_file.read_text(encoding="utf-8", errors="replace") if memory_file.exists() else ""
    updated = current
    touched_entry_ids: List[str] = []

    for mutation in mutations:
        action = str(mutation.get("action") or "").strip()
        entry_id = str(mutation.get("entry_id") or "").strip()
        if not entry_id:
            raise ValueError("memory mutation requires entry_id")
        if entry_id not in touched_entry_ids:
            touched_entry_ids.append(entry_id)
        if action == "upsert":
            block_text = str(mutation.get("block") or "")
            if not block_text.strip():
                raise ValueError(f"memory upsert block is required for {entry_id}")
            block = find_memory_entry_block(updated, entry_id=entry_id)
            if block is None:
                prefix = "" if not updated.strip() else ("" if updated.endswith("\n\n") else ("\n" if updated.endswith("\n") else "\n\n"))
                updated = f"{updated}{prefix}{block_text}"
            else:
                updated = updated[: int(block["start"])] + block_text + updated[int(block["end"]) :]
            updated = normalize_memory_text(updated)
            continue
        raise ValueError(f"unsupported memory mutation action: {action}")
    return {
        "file_path": str(memory_file),
        "current_text": current,
        "updated_text": updated,
        "entry_ids": touched_entry_ids,
    }


def write_memory_content(*, group_id: str, content: str) -> Dict[str, Any]:
    return write_raw_content(
        group_id,
        target="memory",
        content=content,
        mode="replace",
    )


def run_index_sync_after_commit(*, group_id: str) -> Dict[str, Any]:
    try:
        result = index_sync(group_id, mode="scan")
        return {
            "status": "synced",
            "result": result,
            "commit_state": "disk_committed",
        }
    except Exception as exc:
        return {
            "status": "stale",
            "error": str(exc),
            "commit_state": "disk_committed_index_stale",
        }


def load_memory_blocks_by_start_line(*, group_id: str, group: Any | None = None) -> Dict[int, Dict[str, Any]]:
    group = group if group is not None else load_group(group_id)
    if group is not None:
        memory_file = Path(group.path) / "state" / "memory" / "MEMORY.md"
    else:
        try:
            memory_file = resolve_memory_layout(group_id, ensure_files=False).memory_file
        except Exception:
            return {}
    if not memory_file.exists():
        return {}
    text = memory_file.read_text(encoding="utf-8", errors="replace")
    blocks: Dict[int, Dict[str, Any]] = {}
    for match in _MEMORY_ENTRY_BLOCK_RE.finditer(text):
        meta_raw = str(match.group("meta") or "").strip()
        try:
            meta = json.loads(meta_raw) if meta_raw else {}
        except Exception:
            meta = {}
        start_line = text[: match.start()].count("\n") + 1
        end_line = max(start_line, text[: match.end()].count("\n"))
        blocks[start_line] = {
            "entry_id": str(match.group("entry_id") or "").strip(),
            "kind": str(match.group("kind") or "").strip(),
            "meta": meta if isinstance(meta, dict) else {},
            "body": str(match.group("body") or ""),
            "start_line": start_line,
            "end_line": end_line,
        }
    return blocks


def memory_block_for_line(blocks: Dict[int, Dict[str, Any]], start_line: int) -> Optional[Dict[str, Any]]:
    for block in blocks.values():
        if int(block.get("start_line") or 0) <= start_line <= int(block.get("end_line") or 0):
            return block
    return None


# Backward-compatible aliases for existing tests and callers during the split cleanup.
_persist_candidates = persist_candidates
_render_structured_memory_entry = render_structured_memory_entry
_experience_memory_entry_id = experience_memory_entry_id
_build_experience_memory_entry = build_experience_memory_entry
_memory_locator_from_candidate = memory_locator_from_candidate
_memory_file_path = memory_file_path
_legacy_memory_block_range = legacy_memory_block_range
_is_structured_experience_block = is_structured_experience_block
_require_memory_locator = require_memory_locator
_find_memory_entry_block = find_memory_entry_block
_normalize_memory_text = normalize_memory_text
_compute_memory_mutation_plan = compute_memory_mutation_plan
_write_memory_content = write_memory_content
_run_index_sync_after_commit = run_index_sync_after_commit
_load_memory_blocks_by_start_line = load_memory_blocks_by_start_line
_memory_block_for_line = memory_block_for_line
