"""Shared actor-facing turn rendering helpers.

The daemon has multiple transports for delivering ledger events to actors
(PTY, headless app sessions, browser-delivered web models).  This module keeps
the actor-facing text shape shared so transport adapters only add transport
instructions, not their own message grammar.
"""

from __future__ import annotations

import re
from typing import Any, Dict, Iterable, List

from .inbound_rendering import ActorInboundEnvelope, render_actor_inbound_message


def compact_delivery_text(value: Any, *, limit: int) -> str:
    text = re.sub(r"\s+", " ", str(value or "").strip())
    if not text:
        return ""
    if len(text) <= limit:
        return text
    return text[: max(1, limit - 1)].rstrip() + "…"


def _presentation_slot_label(slot_id: str, label: str) -> str:
    if label:
        return label
    match = re.search(r"(\d+)$", slot_id)
    if match:
        try:
            return f"P{int(match.group(1))}"
        except Exception:
            pass
    return slot_id or "Presentation"


def render_group_bridge_route_ref(ref: dict[str, Any]) -> list[str]:
    remote_group_id = compact_delivery_text(ref.get("remote_group_id"), limit=48)
    if not remote_group_id:
        return []
    access_level = compact_delivery_text(ref.get("access_level"), limit=24)
    remote_group_title = compact_delivery_text(ref.get("remote_group_title"), limit=72)
    token = compact_delivery_text(ref.get("token"), limit=72)
    label = remote_group_title or token or remote_group_id
    if access_level:
        return [f"- Group Bridge route {label} ({remote_group_id} remote/{access_level})"]
    return [f"- Group Bridge route {label} (remote_group_id={remote_group_id})"]


def is_hidden_slash_control_ref(ref: dict[str, Any]) -> bool:
    if ref.get("hidden") is not True:
        return False
    return (
        str(ref.get("control_kind") or "").strip() == "slash_skill_dispatch"
        or str(ref.get("title") or "").strip() == "slash_skill_dispatch"
    )


def render_delivery_refs(refs: list[dict[str, Any]]) -> list[str]:
    if not refs:
        return []

    lines = ["[cccc] References:"]
    rendered = 0

    for ref in refs:
        if not isinstance(ref, dict):
            continue
        if is_hidden_slash_control_ref(ref):
            continue
        kind = str(ref.get("kind") or "").strip()
        if kind == "task_ref":
            task_id = compact_delivery_text(ref.get("task_id"), limit=40)
            title = compact_delivery_text(ref.get("title"), limit=72)
            status = compact_delivery_text(ref.get("status"), limit=24)
            if task_id:
                label = f"- Task {task_id}"
                if status:
                    label += f" [{status}]"
                if title:
                    label += f" — {title}"
                lines.append(label)
                rendered += 1
                if rendered >= 4:
                    break
                continue

        if kind == "presentation_ref":
            slot_id = compact_delivery_text(ref.get("slot_id"), limit=32)
            label = _presentation_slot_label(
                slot_id,
                compact_delivery_text(ref.get("label"), limit=24),
            )
            locator_label = compact_delivery_text(ref.get("locator_label"), limit=48)
            title = compact_delivery_text(ref.get("title"), limit=72)
            header = f"- {label}"
            if slot_id:
                header += f" ({slot_id})"
            if locator_label:
                header += f" · {locator_label}"
            if title:
                header += f" — {title}"
            lines.append(header)
            excerpt = compact_delivery_text(ref.get("excerpt"), limit=120)
            if excerpt:
                lines.append(f'  excerpt: "{excerpt}"')
            href = compact_delivery_text(ref.get("href"), limit=120)
            if href:
                lines.append(f"  href: {href}")
            locator = ref.get("locator") if isinstance(ref.get("locator"), dict) else {}
            locator_url = compact_delivery_text(locator.get("url"), limit=120)
            if locator_url and locator_url != href:
                lines.append(f"  view_url: {locator_url}")
            captured_at = compact_delivery_text(locator.get("captured_at"), limit=48)
            if captured_at:
                lines.append(f"  captured_at: {captured_at}")
            viewer_scroll_top = locator.get("viewer_scroll_top")
            if isinstance(viewer_scroll_top, (int, float)) or str(viewer_scroll_top or "").strip():
                try:
                    scroll_value = int(float(viewer_scroll_top))
                except Exception:
                    scroll_value = None
                if scroll_value is not None and scroll_value >= 0:
                    lines.append(f"  scroll_top: {scroll_value}")
            snapshot = ref.get("snapshot") if isinstance(ref.get("snapshot"), dict) else {}
            snapshot_path = compact_delivery_text(snapshot.get("path"), limit=120)
            if snapshot_path:
                width = snapshot.get("width")
                height = snapshot.get("height")
                size_label = ""
                try:
                    width_value = int(width)
                    height_value = int(height)
                    if width_value > 0 and height_value > 0:
                        size_label = f" ({width_value}x{height_value})"
                except Exception:
                    size_label = ""
                lines.append(f"  snapshot: {snapshot_path}{size_label}")
            rendered += 1
            if rendered >= 4:
                break
            continue

        if kind == "group_bridge_route":
            route_lines = render_group_bridge_route_ref(ref)
            if route_lines:
                lines.extend(route_lines)
                rendered += 1
                if rendered >= 4:
                    break
                continue

        summary = compact_delivery_text(
            ref.get("title") or ref.get("path") or ref.get("url") or kind,
            limit=96,
        )
        if summary:
            prefix = kind or "ref"
            lines.append(f"- {prefix}: {summary}")
            rendered += 1
        if rendered >= 4:
            break

    if rendered == 0:
        return []
    if len(refs) > rendered:
        lines.append(f"- … ({len(refs) - rendered} more)")
    return lines


def build_actor_delivery_text(
    *,
    text: str,
    priority: str,
    reply_required: bool,
    event_id: str,
    refs: list[dict[str, Any]],
    attachments: list[dict[str, Any]],
    src_group_id: str = "",
    src_event_id: str = "",
    remote_reply_to: list[str] | None = None,
) -> str:
    delivery_text = text
    prefix_lines: list[str] = []
    if priority == "attention" and event_id:
        prefix_lines.append(f"[cccc] IMPORTANT (event_id={event_id}):")
    if reply_required and event_id:
        prefix_lines.append(f"[cccc] REPLY REQUIRED (event_id={event_id}): reply via cccc_message_reply.")
    if src_group_id and src_event_id:
        prefix_lines.append(f"[cccc] RELAYED FROM (group_id={src_group_id}, event_id={src_event_id}):")
    reply_targets = [str(item or "").strip() for item in (remote_reply_to or []) if str(item or "").strip()]
    if reply_targets:
        target_text = ", ".join(reply_targets)
        prefix_lines.append(f"[cccc] REMOTE REPLY DEFAULT: omit to in cccc_message_reply to reply to remote {target_text}.")
    if prefix_lines:
        delivery_text = "\n".join(prefix_lines) + "\n" + delivery_text
    ref_lines = render_delivery_refs(refs)
    if ref_lines:
        delivery_text = (delivery_text.rstrip("\n") + "\n\n" + "\n".join(ref_lines)).strip()
    if attachments:
        lines = [
            '[cccc] Attachments: use cccc_file(action="read", rel_path=...) for text; '
            'use action="blob_path" for binary/local tools.'
        ]
        for attachment in attachments[:8]:
            title = str(attachment.get("title") or attachment.get("path") or "file").strip()
            size_bytes = int(attachment.get("bytes") or 0)
            rel_path = str(attachment.get("path") or "").strip()
            lines.append(f"- {title} ({size_bytes} bytes) [{rel_path}]")
        if len(attachments) > 8:
            lines.append(f"- … ({len(attachments) - 8} more)")
        delivery_text = (delivery_text.rstrip("\n") + "\n\n" + "\n".join(lines)).strip()
    return delivery_text


def build_actor_headless_delivery_text(
    *,
    by: str,
    to: list[str],
    body: str,
    reply_to: str = "",
    quote_text: str = "",
    source_platform: str = "",
    source_user_name: str = "",
    source_user_id: str = "",
) -> str:
    return render_actor_inbound_message(
        ActorInboundEnvelope(
            by=by,
            to=to,
            text=body,
            reply_to=reply_to,
            quote_text=quote_text,
            source_platform=source_platform,
            source_user_name=source_user_name,
            source_user_id=source_user_id,
        )
    )


def _normalize_to(raw: Any, *, actor_id: str = "") -> list[str]:
    if isinstance(raw, list):
        out = [str(item or "").strip() for item in raw if str(item or "").strip()]
        if out:
            return out
    if isinstance(raw, str) and raw.strip():
        return [raw.strip()]
    return [actor_id] if actor_id else []


def render_actor_event_for_delivery(event: Dict[str, Any], *, actor_id: str = "") -> str:
    kind = str(event.get("kind") or "").strip()
    data = event.get("data") if isinstance(event.get("data"), dict) else {}
    event_id = str(event.get("id") or "").strip()
    by = str(event.get("by") or "").strip() or str(data.get("by") or "").strip() or "system"

    if kind == "chat.message":
        body = build_actor_delivery_text(
            text=str(data.get("text") or ""),
            priority=str(data.get("priority") or "normal"),
            reply_required=bool(data.get("reply_required")),
            event_id=event_id,
            refs=[item for item in data.get("refs", []) if isinstance(item, dict)]
            if isinstance(data.get("refs"), list)
            else [],
            attachments=[item for item in data.get("attachments", []) if isinstance(item, dict)]
            if isinstance(data.get("attachments"), list)
            else [],
            src_group_id=str(data.get("src_group_id") or ""),
            src_event_id=str(data.get("src_event_id") or ""),
            remote_reply_to=[
                str(item or "").strip()
                for item in (data.get("remote_reply_to") or [])
                if str(item or "").strip()
            ]
            if isinstance(data.get("remote_reply_to"), list)
            else [],
        )
        return build_actor_headless_delivery_text(
            by=by,
            to=_normalize_to(data.get("to"), actor_id=actor_id),
            body=body,
            reply_to=str(data.get("reply_to") or ""),
            quote_text=str(data.get("quote_text") or ""),
            source_platform=str(data.get("source_platform") or ""),
            source_user_name=str(data.get("source_user_name") or ""),
            source_user_id=str(data.get("source_user_id") or ""),
        )

    if kind == "system.notify":
        notify_kind = str(data.get("kind") or "info").strip() or "info"
        title = str(data.get("title") or data.get("summary") or "").strip()
        message = str(data.get("message") or data.get("text") or "").strip()
        body = "\n".join([item for item in (title, message) if item]).strip()
        return f"[cccc] SYSTEM ({notify_kind}): {body}".strip()

    text = str(data.get("text") or data.get("message") or data.get("summary") or "").strip()
    if not text:
        text = jsonish(data) if data else ""
    header = f"[cccc] {kind or 'event'}"
    if event_id:
        header += f" (event_id={event_id})"
    return f"{header}:\n{text}".strip()


def render_actor_event_batch_for_delivery(events: Iterable[Dict[str, Any]], *, actor_id: str = "") -> str:
    chunks: List[str] = []
    for event in events:
        if isinstance(event, dict):
            rendered = render_actor_event_for_delivery(event, actor_id=actor_id).strip()
            if rendered:
                chunks.append(rendered)
    return "\n\n".join(chunks).strip()


def jsonish(value: Any) -> str:
    try:
        import json

        return json.dumps(value, ensure_ascii=False, sort_keys=True)
    except Exception:
        return str(value or "")
