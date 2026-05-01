from __future__ import annotations

from typing import Any


def voice_composer_context_lines(value: Any) -> list[str]:
    context = value if isinstance(value, dict) else {}
    lines: list[str] = []
    recipients = context.get("recipients")
    if isinstance(recipients, list):
        clean_recipients = [str(item).strip() for item in recipients if str(item).strip()]
        if clean_recipients:
            lines.append(f"Recipients: {', '.join(clean_recipients[:12])}")
    else:
        recipient_text = str(context.get("recipients") or "").strip()
        if recipient_text:
            lines.append(f"Recipients: {recipient_text[:240]}")
    recent_context = str(context.get("recent_chat_excerpt") or context.get("conversation_tail_summary") or "").strip()
    if recent_context:
        lines.append(f"Recent context:\n{recent_context[:800]}")
    message_mode = str(context.get("message_mode") or "").strip()
    if message_mode and message_mode.lower() != "normal":
        lines.append(f"Message mode: {message_mode[:240]}")
    priority = str(context.get("priority") or "").strip()
    if priority and priority.lower() != "normal":
        lines.append(f"Priority: {priority[:240]}")
    reply_required = context.get("reply_required")
    if str(reply_required).strip().lower() in {"1", "true", "yes"}:
        lines.append("Reply required: true")
    for label, key in (
        ("Reply target", "reply_target"),
        ("Quoted reference", "quoted_reference"),
    ):
        raw = context.get(key)
        if raw in (None, "", []):
            continue
        lines.append(f"{label}: {str(raw)[:240]}")
    return lines


def is_prompt_refine_replace_operation(value: Any) -> bool:
    operation = str(value or "").strip().lower()
    return operation in {"replace", "replace_with_refined_prompt"}


def build_voice_prompt_refine_input_text(
    *,
    composer_text: str,
    voice_transcript: str,
    operation: str,
    composer_context: Any,
) -> str:
    clean_operation = str(operation or "append_to_composer_end").strip() or "append_to_composer_end"
    clean_voice_transcript = str(voice_transcript or "").strip()
    replace_operation = is_prompt_refine_replace_operation(clean_operation)
    parts: list[str] = []
    parts.extend(
        [
            "Task:",
            "Refine the composer draft using the spoken input. Do not answer or execute the task.",
            "",
            "Inputs:",
            "Current composer draft:",
            composer_text or "(empty)",
            "",
            "Spoken input:",
            clean_voice_transcript or "(none; optimize current composer draft directly)",
            "",
        ]
    )
    context_lines = voice_composer_context_lines(composer_context)
    if context_lines:
        parts.extend(["Context (not task):", *context_lines, ""])
    parts.extend(
        [
            "draft_text rule:",
            "Put only composer text in the cccc_voice_secretary_composer draft_text argument; do not print the draft in chat or console.",
        ]
    )
    if replace_operation:
        parts.append("Replacement mode: draft_text must contain the complete replacement prompt.")
    else:
        parts.append("Append mode: draft_text must contain only the text to add; do not repeat useful existing draft text.")
    return "\n".join(parts)
