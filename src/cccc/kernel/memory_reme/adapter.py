from __future__ import annotations

from typing import Any, Dict, List, Optional


def _normalize_messages(messages: List[Dict[str, Any]]) -> List[Dict[str, str]]:
    out: List[Dict[str, str]] = []
    for item in messages:
        if not isinstance(item, dict):
            continue
        role = str(item.get("role") or "").strip().lower()
        if role not in {"system", "user", "assistant", "tool"}:
            role = "assistant"
        out.append(
            {
                "role": role,
                "name": str(item.get("name") or "").strip(),
                "content": str(item.get("content") or ""),
            }
        )
    return out


def _message_tokens(msg: Dict[str, str]) -> int:
    # ReMe-like lightweight estimate (embedding-free baseline).
    txt = str(msg.get("content") or "")
    return max(1, len(txt) // 4)


def _total_tokens(messages: List[Dict[str, str]]) -> int:
    return sum(_message_tokens(m) for m in messages)


def _find_turn_start_index(messages: List[Dict[str, str]], entry_index: int) -> int:
    if not messages or entry_index < 0 or entry_index >= len(messages):
        return -1
    for i in range(entry_index, -1, -1):
        if str(messages[i].get("role") or "") == "user":
            return i
    return -1


def context_check_messages(
    messages: List[Dict[str, Any]],
    *,
    context_window_tokens: int = 128000,
    reserve_tokens: int = 36000,
    keep_recent_tokens: int = 20000,
) -> Dict[str, Any]:
    normalized = _normalize_messages(messages)
    token_count = _total_tokens(normalized)
    threshold = max(1, int(context_window_tokens) - int(reserve_tokens))
    if token_count < threshold:
        return {
            "needs_compaction": False,
            "token_count": token_count,
            "threshold": threshold,
            "messages_to_summarize": [],
            "turn_prefix_messages": [],
            "left_messages": normalized,
            "is_split_turn": False,
            "cut_index": 0,
        }

    # Walk from tail to keep a recent window, then cut before that.
    accumulated = 0
    cut_index = 0
    for i in range(len(normalized) - 1, -1, -1):
        accumulated += _message_tokens(normalized[i])
        if accumulated >= int(keep_recent_tokens):
            cut_index = i
            break

    if cut_index <= 0:
        return {
            "needs_compaction": True,
            "token_count": token_count,
            "threshold": threshold,
            "messages_to_summarize": [],
            "turn_prefix_messages": [],
            "left_messages": normalized,
            "is_split_turn": False,
            "cut_index": 0,
        }

    cut_role = str(normalized[cut_index].get("role") or "")
    if cut_role == "user":
        return {
            "needs_compaction": True,
            "token_count": token_count,
            "threshold": threshold,
            "messages_to_summarize": normalized[:cut_index],
            "turn_prefix_messages": [],
            "left_messages": normalized[cut_index:],
            "is_split_turn": False,
            "cut_index": cut_index,
        }

    # Split-turn case: cut between user start and assistant/tool continuation.
    turn_start = _find_turn_start_index(normalized, cut_index)
    if turn_start < 0:
        return {
            "needs_compaction": True,
            "token_count": token_count,
            "threshold": threshold,
            "messages_to_summarize": normalized[:cut_index],
            "turn_prefix_messages": [],
            "left_messages": normalized[cut_index:],
            "is_split_turn": False,
            "cut_index": cut_index,
        }
    return {
        "needs_compaction": True,
        "token_count": token_count,
        "threshold": threshold,
        "messages_to_summarize": normalized[:turn_start],
        "turn_prefix_messages": normalized[turn_start:cut_index],
        "left_messages": normalized[cut_index:],
        "is_split_turn": True,
        "cut_index": cut_index,
    }


def _serialize_for_prompt(messages: List[Dict[str, str]]) -> str:
    parts: List[str] = []
    for msg in messages:
        role = str(msg.get("role") or "assistant").upper()
        name = str(msg.get("name") or "").strip()
        prefix = f"{role}({name})" if name else role
        parts.append(f"{prefix}: {str(msg.get('content') or '')}".strip())
    return "\n".join(parts).strip()


def compact_messages(
    *,
    messages_to_summarize: List[Dict[str, Any]],
    turn_prefix_messages: Optional[List[Dict[str, Any]]] = None,
    previous_summary: str = "",
    language: str = "en",
    return_prompt: bool = False,
) -> Dict[str, Any]:
    history = _normalize_messages(messages_to_summarize)
    prefix = _normalize_messages(turn_prefix_messages or [])

    serialized_history = _serialize_for_prompt(history)
    serialized_prefix = _serialize_for_prompt(prefix)
    system_prompt = (
        "Summarize high-signal durable facts only. Remove chit-chat and duplicates. "
        f"Output language: {str(language or 'en')}."
    )

    if return_prompt:
        prompt: Dict[str, str] = {"system": system_prompt}
        if serialized_history:
            prompt["history_user"] = serialized_history
        if serialized_prefix:
            prompt["turn_prefix_user"] = serialized_prefix
        if previous_summary:
            prompt["previous_summary"] = str(previous_summary)
        return {"prompt": prompt}

    # Deterministic fallback summary (daemon-safe when no LLM call is delegated).
    lines: List[str] = []
    if previous_summary:
        lines.append(f"Previous summary: {str(previous_summary).strip()}")
    if serialized_history:
        snippet = serialized_history.splitlines()[:12]
        lines.append("History summary:")
        lines.extend(f"- {x[:400]}" for x in snippet if x.strip())
    if serialized_prefix:
        snippet = serialized_prefix.splitlines()[:6]
        lines.append("Turn context:")
        lines.extend(f"- {x[:400]}" for x in snippet if x.strip())
    summary = "\n".join(lines).strip()
    return {"summary": summary}


def summarize_daily_messages(
    messages: List[Dict[str, Any]],
    *,
    signal_pack: Optional[Dict[str, Any]] = None,
    max_lines: int = 16,
) -> str:
    normalized = _normalize_messages(messages)
    sections: List[str] = []

    conversation_lines: List[str] = []
    for msg in normalized:
        content = str(msg.get("content") or "").strip()
        if not content:
            continue
        role = str(msg.get("role") or "assistant")
        conversation_lines.append(f"- [{role}] {content[:500]}")
        if len(conversation_lines) >= max_lines:
            break
    if conversation_lines:
        sections.append("## Conversation Summary\n" + "\n".join(conversation_lines))

    if isinstance(signal_pack, dict):
        brief = signal_pack.get("coordination_brief") if isinstance(signal_pack.get("coordination_brief"), dict) else {}
        brief_lines: List[str] = []
        objective = str(brief.get("objective") or "").strip()
        current_focus = str(brief.get("current_focus") or "").strip()
        project_brief = str(brief.get("project_brief") or "").strip()
        constraints = brief.get("constraints") if isinstance(brief.get("constraints"), list) else []
        if objective:
            brief_lines.append(f"- Objective: {objective[:240]}")
        if current_focus:
            brief_lines.append(f"- Current Focus: {current_focus[:240]}")
        for item in constraints[:4]:
            text_item = str(item or "").strip()
            if text_item:
                brief_lines.append(f"- Constraint: {text_item[:160]}")
        if project_brief:
            brief_lines.append(f"- Project Brief: {project_brief[:240]}")
        if brief_lines:
            sections.append("## Coordination Snapshot\n" + "\n".join(brief_lines))

        tasks = signal_pack.get("tasks") if isinstance(signal_pack.get("tasks"), dict) else {}
        task_lines: List[str] = []
        label_map = [
            ("active", "Active"),
            ("done_recent", "Done Recently"),
            ("blocked", "Blocked"),
            ("waiting_user", "Waiting User"),
            ("planned", "Planned"),
        ]
        for key, label in label_map:
            values = tasks.get(key) if isinstance(tasks.get(key), list) else []
            for value in values[:4]:
                text_item = str(value or "").strip()
                if text_item:
                    task_lines.append(f"- {label}: {text_item[:200]}")
        if task_lines:
            sections.append("## Task Snapshot\n" + "\n".join(task_lines))

        agent_states = signal_pack.get("agent_states") if isinstance(signal_pack.get("agent_states"), list) else []
        agent_lines: List[str] = []
        for raw_agent in agent_states[:4]:
            if not isinstance(raw_agent, dict):
                continue
            actor_id = str(raw_agent.get("id") or "").strip()
            hot = raw_agent.get("hot") if isinstance(raw_agent.get("hot"), dict) else {}
            warm = raw_agent.get("warm") if isinstance(raw_agent.get("warm"), dict) else {}
            parts: List[str] = []
            if actor_id:
                parts.append(actor_id)
            focus = str(hot.get("focus") or "").strip()
            next_action = str(hot.get("next_action") or "").strip()
            blockers = hot.get("blockers") if isinstance(hot.get("blockers"), list) else []
            what_changed = str(warm.get("what_changed") or "").strip()
            open_loops = warm.get("open_loops") if isinstance(warm.get("open_loops"), list) else []
            commitments = warm.get("commitments") if isinstance(warm.get("commitments"), list) else []
            if focus:
                parts.append(f"focus={focus[:120]}")
            if next_action:
                parts.append(f"next={next_action[:120]}")
            blocker_text = "; ".join(str(x or "").strip()[:80] for x in blockers[:2] if str(x or "").strip())
            if blocker_text:
                parts.append(f"blockers={blocker_text}")
            if what_changed:
                parts.append(f"changed={what_changed[:120]}")
            loop_text = "; ".join(str(x or "").strip()[:80] for x in open_loops[:2] if str(x or "").strip())
            if loop_text:
                parts.append(f"open={loop_text}")
            commitment_text = "; ".join(str(x or "").strip()[:80] for x in commitments[:2] if str(x or "").strip())
            if commitment_text:
                parts.append(f"commit={commitment_text}")
            if parts:
                agent_lines.append("- " + " | ".join(parts))
        if agent_lines:
            sections.append("## Agent Resume Cues\n" + "\n".join(agent_lines))

    return "\n\n".join(section for section in sections if section.strip()).strip()
